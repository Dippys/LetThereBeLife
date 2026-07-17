use std::{borrow::Cow, collections::BTreeMap, fmt::Write, sync::Arc, time::Instant};

use bytemuck::{Pod, Zeroable};
use rayon::prelude::*;
use sim_core::{
    AgentActivity, AgentView, BiomeType, CHUNK_SIZE, ChunkCoord, ChunkInspection, ChunkPresence,
    DeathCause, DeathRecord, Engine, ExplorationHeading, FeatureKind, GenerateAreaError,
    HealthStatus, HealthView, InventoryView, NeedKind, PhysicalGoal, PhysicalNeedsView,
    PhysicalPolicyView, PolicyReason, PrevailingWind, ResourceKind, SimulationSnapshot,
    SleepQuality, SleepView, SpawnKind, SpawnedObjectView, StructureState, StructureView,
    SurfaceType, TerrainCell, WORLD_GENERATION_BOUNDS, World, WorldPosition, WorldRect,
};
use wgpu::util::DeviceExt;
use winit::window::Window;

use crate::camera::Camera;

pub struct RenderState {
    pub snapshot: SimulationSnapshot,
    pub camera: Camera,
    pub ui_scale: f32,
    pub cursor_world: Option<WorldPosition>,
    pub cursor_spawned_object: Option<SpawnedObjectView>,
    pub inspected: Option<ChunkInspection>,
    pub hovered: Option<WorldPosition>,
    pub selection: Option<WorldRect>,
    pub selection_valid: bool,
    pub generation_status: GenerationStatus,
    pub population_status: PopulationStatus,
    pub hovered_agent: Option<AgentInspection>,
    pub spawn_message: Option<String>,
    pub spawn_menu: Option<SpawnMenuView>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpawnMenuView {
    pub selected: SpawnKind,
    pub placing: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct AgentInspection {
    pub view: AgentView,
    pub needs: Option<PhysicalNeedsView>,
    pub inventory: Option<InventoryView>,
    pub health: Option<HealthView>,
    pub policy: Option<PhysicalPolicyView>,
    pub sleep: Option<SleepView>,
    pub death: Option<DeathRecord>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenerationStatus {
    Idle,
    Bootstrap,
    Manual,
    Cancelling,
    WorkerUnavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PopulationStatus {
    Waiting,
    Ready,
    Active,
    Failed,
}

pub struct Renderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    pipeline: wgpu::RenderPipeline,
    world_camera: CameraBinding,
    screen_camera: CameraBinding,
    terrain: StaticInstanceBuffers,
    features: StaticInstanceBuffers,
    spawned_objects: InstanceBuffer,
    spawned_object_instances: Vec<Instance>,
    structures: InstanceBuffer,
    structure_instances: Vec<Instance>,
    agents: InstanceBuffer,
    agent_instances: Vec<Instance>,
    world_overlay: InstanceBuffer,
    world_overlay_instances: Vec<Instance>,
    screen_overlay: InstanceBuffer,
    screen_overlay_instances: Vec<Instance>,
    hud_text: String,
    agent_text: String,
    world_revision: u64,
    cached_bounds: Option<WorldRect>,
    cached_step: u32,
    summaries: WorldSummaryCache,
}

impl Renderer {
    pub fn new(window: Arc<Window>, world: &World) -> Result<Self, String> {
        pollster::block_on(Self::new_async(window, world))
    }

    async fn new_async(window: Arc<Window>, world: &World) -> Result<Self, String> {
        let size = window.inner_size();
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
        let surface = instance
            .create_surface(window)
            .map_err(|error| format!("create GPU surface: {error}"))?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .map_err(|error| format!("request GPU adapter: {error}"))?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("viewer device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                memory_hints: wgpu::MemoryHints::Performance,
                trace: wgpu::Trace::Off,
            })
            .await
            .map_err(|error| format!("request GPU device: {error}"))?;
        let capabilities = surface.get_capabilities(&adapter);
        let format = capabilities
            .formats
            .iter()
            .copied()
            .find(wgpu::TextureFormat::is_srgb)
            .unwrap_or(capabilities.formats[0]);
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::AutoVsync,
            alpha_mode: capabilities.alpha_modes[0],
            view_formats: Vec::new(),
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("camera layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let world_camera = CameraBinding::new(&device, &bind_group_layout, "world camera");
        let screen_camera = CameraBinding::new(&device, &bind_group_layout, "screen camera");
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("viewer shader"),
            source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(include_str!("shader.wgsl"))),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("viewer pipeline layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("viewer pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[Instance::layout()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        Ok(Self {
            surface,
            terrain: StaticInstanceBuffers::new(&device, "terrain instances", &[]),
            features: StaticInstanceBuffers::new(&device, "feature instances", &[]),
            spawned_objects: InstanceBuffer::dynamic(
                &device,
                "spawned object instances",
                MAX_SPAWNED_OBJECT_INSTANCES,
            ),
            spawned_object_instances: Vec::new(),
            structures: InstanceBuffer::dynamic(
                &device,
                "structure instances",
                MAX_STRUCTURE_INSTANCES,
            ),
            structure_instances: Vec::with_capacity(MAX_STRUCTURE_INSTANCES),
            agents: InstanceBuffer::dynamic(&device, "agent instances", MAX_AGENT_INSTANCES),
            agent_instances: Vec::with_capacity(MAX_AGENT_INSTANCES),
            world_overlay: InstanceBuffer::dynamic(
                &device,
                "world overlay",
                WORLD_OVERLAY_CAPACITY,
            ),
            world_overlay_instances: Vec::with_capacity(WORLD_OVERLAY_CAPACITY),
            screen_overlay: InstanceBuffer::dynamic(
                &device,
                "screen overlay",
                SCREEN_OVERLAY_CAPACITY,
            ),
            screen_overlay_instances: Vec::with_capacity(SCREEN_OVERLAY_CAPACITY),
            hud_text: String::with_capacity(HUD_TEXT_CAPACITY),
            agent_text: String::with_capacity(AGENT_TEXT_CAPACITY),
            world_revision: world.revision(),
            cached_bounds: None,
            cached_step: 1,
            summaries: WorldSummaryCache::default(),
            device,
            queue,
            config,
            pipeline,
            world_camera,
            screen_camera,
        })
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
    }

    fn sync_view(
        &mut self,
        world: &World,
        requested: WorldRect,
        scale: f32,
        allow_world_sync: bool,
        changed_bounds: Option<WorldRect>,
    ) {
        let step = terrain_sample_step(scale);
        let cache_contains_view = self
            .cached_bounds
            .is_some_and(|cached| cached.contains_rect(requested));
        let revision_changed = self.world_revision != world.revision();
        let changed_affects_cache = changed_bounds.is_none_or(|changed| {
            self.cached_bounds
                .is_none_or(|cached| cached.intersects(changed))
        });
        match cache_sync_action(
            cache_contains_view,
            self.cached_step == step,
            revision_changed,
            allow_world_sync,
            changed_affects_cache,
        ) {
            CacheSyncAction::Skip => return,
            CacheSyncAction::AdvanceRevision => {
                self.world_revision = world.revision();
                return;
            }
            CacheSyncAction::Rebuild => {}
        }
        let cached = requested.expanded(cache_margin(scale));
        let build_started = Instant::now();
        let (terrain, features) = self.summaries.sync(
            world,
            cached,
            step,
            revision_changed.then_some(changed_bounds).flatten(),
        );
        let build_elapsed = build_started.elapsed();
        let upload_started = Instant::now();
        self.terrain = StaticInstanceBuffers::new(&self.device, "terrain instances", &terrain);
        self.features = StaticInstanceBuffers::new(&self.device, "feature instances", &features);
        let upload_enqueue_elapsed = upload_started.elapsed();
        if std::env::var_os("SIM_VIEWER_SUMMARY_METRICS").is_some() {
            eprintln!(
                "renderer-summary step={step} chunks={} cache_bytes={} terrain_instances={} feature_instances={} gpu_instance_bytes={} build_ms={:.3} upload_enqueue_ms={:.3} sync_cpu_ms={:.3}",
                self.summaries.chunks.len(),
                self.summaries.logical_bytes(),
                terrain.len(),
                features.len(),
                (terrain.len() + features.len()) * size_of::<Instance>(),
                build_elapsed.as_secs_f64() * 1_000.0,
                upload_enqueue_elapsed.as_secs_f64() * 1_000.0,
                (build_elapsed + upload_enqueue_elapsed).as_secs_f64() * 1_000.0,
            );
        }
        self.world_revision = world.revision();
        self.cached_bounds = Some(cached);
        self.cached_step = step;
    }

    pub fn render(
        &mut self,
        engine: &Engine,
        state: RenderState,
        allow_world_sync: bool,
        changed_bounds: Option<WorldRect>,
    ) -> Result<(), wgpu::SurfaceError> {
        let world = engine.world();
        let view = state.camera.view(
            self.config.width,
            self.config.height,
            world.width(),
            world.height(),
        );
        self.sync_view(
            world,
            view.world_bounds(),
            view.scale() as f32,
            allow_world_sync,
            changed_bounds,
        );
        self.world_camera.write(
            &self.queue,
            CameraUniform::new(
                view.center(),
                [self.config.width as f32, self.config.height as f32],
                view.scale() as f32,
            ),
        );
        self.screen_camera.write(
            &self.queue,
            CameraUniform::new(
                [
                    self.config.width as f32 / 2.0,
                    self.config.height as f32 / 2.0,
                ],
                [self.config.width as f32, self.config.height as f32],
                1.0,
            ),
        );

        build_structure_instances(
            engine.structure_views(MAX_STRUCTURE_INSTANCES),
            view.world_bounds(),
            view.scale() as f32,
            &mut self.structure_instances,
        );
        debug_assert!(self.structure_instances.len() <= MAX_STRUCTURE_INSTANCES);
        self.structures
            .write(&self.queue, &self.structure_instances);
        build_agent_instances(
            engine.agent_views(MAX_AGENT_INSTANCES),
            view.world_bounds(),
            view.scale() as f32,
            &mut self.agent_instances,
        );
        debug_assert!(self.agent_instances.len() <= MAX_AGENT_INSTANCES);
        self.agents.write(&self.queue, &self.agent_instances);
        build_spawned_object_instances(
            engine.spawned_object_views(),
            view.world_bounds(),
            view.scale() as f32,
            &mut self.spawned_object_instances,
        );
        self.spawned_objects
            .write(&self.queue, &self.spawned_object_instances);

        let world_overlay = &mut self.world_overlay_instances;
        world_overlay.clear();
        if let Some(position) = state.hovered {
            world_overlay.push(Instance::new(
                position.x as f32,
                position.y as f32,
                1.0,
                1.0,
                rgba(255, 235, 92, 150),
            ));
        }
        if let Some(bounds) = state.selection {
            world_overlay.push(Instance::new(
                bounds.min.x as f32,
                bounds.min.y as f32,
                (bounds.max.x - bounds.min.x) as f32,
                (bounds.max.y - bounds.min.y) as f32,
                selection_color(state.selection_valid),
            ));
        }
        if let Some(inspection) = state.inspected
            && let Some(outline) = chunk_outline(inspection, view.scale() as f32)
        {
            world_overlay.extend_from_slice(&outline);
        }
        world_overlay.extend_from_slice(&world_border(view.scale() as f32));
        debug_assert!(world_overlay.len() <= WORLD_OVERLAY_CAPACITY);
        self.world_overlay.write(&self.queue, world_overlay);

        write_hud_text(&mut self.hud_text, world, &state);
        write_agent_text(&mut self.agent_text, state.hovered_agent);
        build_screen_overlay(
            &mut self.screen_overlay_instances,
            &self.hud_text,
            (!self.agent_text.is_empty()).then_some(self.agent_text.as_str()),
            &state,
            self.config.width,
            self.config.height,
        );
        assert!(
            self.screen_overlay_instances.len() <= SCREEN_OVERLAY_CAPACITY,
            "HUD instance budget must cover every supported status layout"
        );
        self.screen_overlay
            .write(&self.queue, &self.screen_overlay_instances);

        let output = self.surface.get_current_texture()?;
        let texture_view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("viewer encoder"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("viewer pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &texture_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.world_camera.bind_group, &[]);
            self.terrain.draw(&mut pass);
            self.features.draw(&mut pass);
            self.spawned_objects.draw(&mut pass);
            self.structures.draw(&mut pass);
            self.agents.draw(&mut pass);
            self.world_overlay.draw(&mut pass);
            pass.set_bind_group(0, &self.screen_camera.bind_group, &[]);
            self.screen_overlay.draw(&mut pass);
        }
        self.queue.submit(Some(encoder.finish()));
        output.present();
        Ok(())
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct CameraUniform {
    center: [f32; 2],
    viewport: [f32; 2],
    scale: f32,
    _padding: [f32; 3],
}

impl CameraUniform {
    const fn new(center: [f32; 2], viewport: [f32; 2], scale: f32) -> Self {
        Self {
            center,
            viewport,
            scale,
            _padding: [0.0; 3],
        }
    }
}

const _: () = assert!(size_of::<CameraUniform>() == 32);
const _: () = assert!(size_of::<Instance>() == 20);

struct CameraBinding {
    buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
}

impl CameraBinding {
    fn new(device: &wgpu::Device, layout: &wgpu::BindGroupLayout, label: &str) -> Self {
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(label),
            contents: bytemuck::bytes_of(&CameraUniform::zeroed()),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(label),
            layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: buffer.as_entire_binding(),
            }],
        });
        Self { buffer, bind_group }
    }

    fn write(&self, queue: &wgpu::Queue, uniform: CameraUniform) {
        queue.write_buffer(&self.buffer, 0, bytemuck::bytes_of(&uniform));
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
struct Instance {
    position: [f32; 2],
    size: [f32; 2],
    color: u32,
}

impl Instance {
    const fn new(x: f32, y: f32, width: f32, height: f32, color: u32) -> Self {
        Self {
            position: [x, y],
            size: [width, height],
            color,
        }
    }

    fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: size_of::<Self>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &[
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32x2,
                    offset: 0,
                    shader_location: 0,
                },
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32x2,
                    offset: 8,
                    shader_location: 1,
                },
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Uint32,
                    offset: 16,
                    shader_location: 2,
                },
            ],
        }
    }
}

struct InstanceBuffer {
    buffer: wgpu::Buffer,
    count: u32,
}

const MAX_INSTANCES_PER_BUFFER: usize = 1_000_000;
const MIN_TERRAIN_SAMPLE_PIXELS: f32 = 2.0;
const CACHE_MARGIN_PIXELS: f32 = 128.0;
const WORLD_OVERLAY_CAPACITY: usize = 10;
const SCREEN_OVERLAY_CAPACITY: usize = 8_192;
const HUD_TEXT_CAPACITY: usize = 640;
const AGENT_TEXT_CAPACITY: usize = 640;
const MAX_AGENT_INSTANCES: usize = 4_096;
const MAX_STRUCTURE_INSTANCES: usize = 4_096;
const MAX_SPAWNED_OBJECT_INSTANCES: usize = 16_384;
const MIN_DYNAMIC_INSTANCE_PIXELS: f32 = 1.25;
const MIN_CHUNK_OUTLINE_PIXELS: f32 = 4.0;
const MAX_CHUNK_OUTLINE_WORLD_WIDTH: f32 = 8.0;
const MAX_WORLD_BORDER_WIDTH: f32 = 32.0;

fn build_spawned_object_instances(
    views: impl IntoIterator<Item = SpawnedObjectView>,
    visible: WorldRect,
    scale: f32,
    output: &mut Vec<Instance>,
) {
    output.clear();
    if scale < MIN_DYNAMIC_INSTANCE_PIXELS {
        return;
    }
    for object in views
        .into_iter()
        .filter(|object| visible.contains(object.position))
        .take(MAX_SPAWNED_OBJECT_INSTANCES)
    {
        let inset = match object.kind {
            SpawnKind::Water => 0.0,
            SpawnKind::Tree | SpawnKind::Rock => 0.08,
            SpawnKind::BerryBush => 0.18,
        };
        output.push(Instance::new(
            object.position.x as f32 + inset,
            object.position.y as f32 + inset,
            1.0 - inset * 2.0,
            1.0 - inset * 2.0,
            spawn_kind_color(object.kind),
        ));
    }
}

fn build_agent_instances(
    views: impl IntoIterator<Item = AgentView>,
    visible: WorldRect,
    scale: f32,
    output: &mut Vec<Instance>,
) {
    output.clear();
    if scale < MIN_DYNAMIC_INSTANCE_PIXELS {
        return;
    }
    for agent in views.into_iter().take(MAX_AGENT_INSTANCES) {
        if !visible.contains(agent.position) {
            continue;
        }
        let inset = if matches!(agent.activity, AgentActivity::Dead) {
            0.08
        } else {
            0.14
        };
        output.push(Instance::new(
            agent.position.x as f32 + inset,
            agent.position.y as f32 + inset,
            1.0 - inset * 2.0,
            1.0 - inset * 2.0,
            agent_color(agent.activity),
        ));
    }
}

fn build_structure_instances(
    views: impl IntoIterator<Item = StructureView>,
    visible: WorldRect,
    scale: f32,
    output: &mut Vec<Instance>,
) {
    output.clear();
    if scale < MIN_DYNAMIC_INSTANCE_PIXELS {
        return;
    }
    for structure in views.into_iter().take(MAX_STRUCTURE_INSTANCES) {
        if visible.contains(structure.position) {
            output.push(Instance::new(
                structure.position.x as f32 + 0.05,
                structure.position.y as f32 + 0.05,
                0.9,
                0.9,
                structure_color(structure.state),
            ));
        }
    }
}

const fn structure_color(state: StructureState) -> u32 {
    match state {
        StructureState::UnderConstruction => rgba(224, 170, 72, 230),
        StructureState::Complete => rgba(116, 72, 38, 255),
    }
}

const fn agent_color(activity: AgentActivity) -> u32 {
    match activity {
        AgentActivity::Idle => rgba(244, 238, 210, 255),
        AgentActivity::Moving => rgba(72, 232, 126, 255),
        AgentActivity::Gathering => rgba(250, 206, 74, 255),
        AgentActivity::Building => rgba(240, 142, 62, 255),
        AgentActivity::Sleeping => rgba(92, 164, 246, 255),
        AgentActivity::Incapacitated => rgba(180, 72, 214, 255),
        AgentActivity::Dead => rgba(118, 28, 32, 255),
    }
}

struct StaticInstanceBuffers {
    buffers: Vec<InstanceBuffer>,
}

impl StaticInstanceBuffers {
    fn new(device: &wgpu::Device, label: &str, instances: &[Instance]) -> Self {
        let chunks = static_instance_chunks(instances);
        let mut buffers = Vec::with_capacity(chunks.len());
        buffers.extend(chunks.enumerate().map(|(index, chunk)| {
            InstanceBuffer::immutable(device, &format!("{label} {index}"), chunk)
        }));
        Self { buffers }
    }

    fn draw<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>) {
        for buffer in &self.buffers {
            buffer.draw(pass);
        }
    }
}

impl InstanceBuffer {
    fn immutable(device: &wgpu::Device, label: &str, instances: &[Instance]) -> Self {
        let empty = Instance::zeroed();
        let contents = if instances.is_empty() {
            bytemuck::bytes_of(&empty)
        } else {
            bytemuck::cast_slice(instances)
        };
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(label),
            contents,
            usage: wgpu::BufferUsages::VERTEX,
        });
        Self {
            buffer,
            count: instances.len() as u32,
        }
    }

    fn dynamic(device: &wgpu::Device, label: &str, capacity: usize) -> Self {
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: (capacity * size_of::<Instance>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self { buffer, count: 0 }
    }

    fn write(&mut self, queue: &wgpu::Queue, instances: &[Instance]) {
        self.count = instances.len() as u32;
        if !instances.is_empty() {
            queue.write_buffer(&self.buffer, 0, bytemuck::cast_slice(instances));
        }
    }

    fn draw<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>) {
        if self.count > 0 {
            pass.set_vertex_buffer(0, self.buffer.slice(..));
            pass.draw(0..6, 0..self.count);
        }
    }
}

#[cfg(test)]
fn build_world_instances(
    world: &World,
    bounds: WorldRect,
    step: u32,
) -> (Vec<Instance>, Vec<Instance>) {
    let mut summaries = WorldSummaryCache::default();
    summaries.sync(world, bounds, step, None)
}

fn build_exact_world_instances(world: &World, bounds: WorldRect) -> (Vec<Instance>, Vec<Instance>) {
    let mut terrain = Vec::new();
    world.visit_cells_in(bounds, |position, cell| {
        terrain.push(Instance::new(
            position.x as f32,
            position.y as f32,
            1.0,
            1.0,
            terrain_color(cell),
        ));
    });
    let mut features = Vec::new();
    world.visit_features_in(bounds, |feature| {
        features.push(Instance::new(
            feature.position.x as f32,
            feature.position.y as f32,
            1.0,
            1.0,
            feature_color(feature.kind),
        ));
    });
    (terrain, features)
}

#[derive(Default)]
struct WorldSummaryCache {
    step: u32,
    chunks: BTreeMap<ChunkCoord, ChunkRenderSummary>,
}

impl WorldSummaryCache {
    fn sync(
        &mut self,
        world: &World,
        bounds: WorldRect,
        step: u32,
        changed_bounds: Option<WorldRect>,
    ) -> (Vec<Instance>, Vec<Instance>) {
        if step == 1 {
            self.step = 1;
            self.chunks.clear();
            return build_exact_world_instances(world, bounds);
        }
        debug_assert!(step.is_power_of_two() && step <= CHUNK_SIZE as u32);
        if self.step != step {
            self.step = step;
            self.chunks.clear();
        }
        self.chunks.retain(|coord, _| {
            coord
                .bounds()
                .is_ok_and(|chunk_bounds| chunk_bounds.intersects(bounds))
        });
        if let Some(changed) = changed_bounds {
            self.chunks.retain(|coord, _| {
                coord
                    .bounds()
                    .is_ok_and(|chunk_bounds| !chunk_bounds.intersects(changed))
            });
        }

        let mut missing = Vec::new();
        world.visit_loaded_regions_in(bounds, |coord, coverage| {
            if !self.chunks.contains_key(&coord) {
                missing.push((coord, coverage));
            }
        });
        let built: Vec<_> = missing
            .into_par_iter()
            .map(|(coord, coverage)| (coord, build_chunk_summary(world, coord, coverage, step)))
            .collect();
        self.chunks.extend(built);

        let terrain_len = self
            .chunks
            .values()
            .map(|summary| summary.terrain.len())
            .sum();
        let feature_len = self
            .chunks
            .values()
            .map(|summary| summary.features.len())
            .sum();
        let mut terrain = Vec::with_capacity(terrain_len);
        let mut features = Vec::with_capacity(feature_len);
        for summary in self.chunks.values() {
            terrain.extend_from_slice(&summary.terrain);
            features.extend_from_slice(&summary.features);
        }
        (terrain, features)
    }

    fn logical_bytes(&self) -> usize {
        self.chunks.values().fold(0, |bytes, summary| {
            bytes
                + summary.terrain.capacity() * size_of::<Instance>()
                + summary.features.capacity() * size_of::<Instance>()
        })
    }
}

struct ChunkRenderSummary {
    terrain: Vec<Instance>,
    features: Vec<Instance>,
}

const TERRAIN_VISUAL_COUNT: usize = 15;
const OCEAN_DEEP: usize = 0;
const OCEAN_SHALLOW: usize = 1;
const LAKE_VISUAL: usize = 2;
const RIVER_VISUAL: usize = 3;
const BEACH_VISUAL: usize = 4;
const DESERT_VISUAL: usize = 5;
const GRASS_VISUAL: usize = 6;
const SAVANNA_VISUAL: usize = 7;
const FOREST_VISUAL: usize = 8;
const WETLAND_VISUAL: usize = 9;
const TUNDRA_VISUAL: usize = 10;
const HILL_VISUAL: usize = 11;
const ROCK_VISUAL: usize = 12;
const SNOW_VISUAL: usize = 13;
const FALLBACK_VISUAL: usize = 14;

#[derive(Clone, Copy, Default)]
struct VisualSample {
    count: u16,
    representative: Option<TerrainCell>,
    min_x: u8,
    min_y: u8,
    max_x: u8,
    max_y: u8,
}

impl VisualSample {
    fn observe(&mut self, position: WorldPosition, origin: WorldPosition, cell: TerrainCell) {
        let x = (position.x - origin.x) as u8;
        let y = (position.y - origin.y) as u8;
        if self.count == 0 {
            self.min_x = x;
            self.min_y = y;
            self.max_x = x + 1;
            self.max_y = y + 1;
        } else {
            self.min_x = self.min_x.min(x);
            self.min_y = self.min_y.min(y);
            self.max_x = self.max_x.max(x + 1);
            self.max_y = self.max_y.max(y + 1);
        }
        self.count = self.count.saturating_add(1);
        self.representative.get_or_insert(cell);
    }

    fn world_bounds(self, origin: WorldPosition) -> WorldRect {
        debug_assert!(self.count > 0);
        WorldRect {
            min: WorldPosition {
                x: origin.x + i64::from(self.min_x),
                y: origin.y + i64::from(self.min_y),
            },
            max: WorldPosition {
                x: origin.x + i64::from(self.max_x),
                y: origin.y + i64::from(self.max_y),
            },
        }
    }
}

#[derive(Clone)]
struct SummaryAccumulator {
    visuals: [VisualSample; TERRAIN_VISUAL_COUNT],
    feature_counts: [u16; 3],
}

impl Default for SummaryAccumulator {
    fn default() -> Self {
        Self {
            visuals: [VisualSample::default(); TERRAIN_VISUAL_COUNT],
            feature_counts: [0; 3],
        }
    }
}

impl SummaryAccumulator {
    fn observe_cell(&mut self, position: WorldPosition, origin: WorldPosition, cell: TerrainCell) {
        self.visuals[terrain_visual(cell)].observe(position, origin, cell);
    }

    fn observe_feature(&mut self, kind: FeatureKind) {
        let index = match kind {
            FeatureKind::Tree => 0,
            FeatureKind::Rock => 1,
            FeatureKind::BerryBush => 2,
        };
        self.feature_counts[index] = self.feature_counts[index].saturating_add(1);
    }

    fn instances(
        &self,
        block: WorldRect,
        chunk_origin: WorldPosition,
        terrain: &mut Vec<Instance>,
        features: &mut Vec<Instance>,
    ) {
        let Some(base_index) = self.base_visual() else {
            return;
        };
        let base = self.visuals[base_index]
            .representative
            .expect("observed visual retains a representative cell");
        terrain.push(rect_instance(block, terrain_color(base)));
        if let Some(detail_index) = self.detail_visual(base_index) {
            let detail = self.visuals[detail_index];
            let detail_cell = detail
                .representative
                .expect("observed detail retains a representative cell");
            terrain.push(rect_instance(
                visible_detail_bounds(detail.world_bounds(chunk_origin), block),
                terrain_color(detail_cell),
            ));
        }

        let total_features: u16 = self.feature_counts.iter().copied().sum();
        if total_features == 0 {
            return;
        }
        let feature_index = self
            .feature_counts
            .iter()
            .enumerate()
            .max_by_key(|&(index, count)| (*count, std::cmp::Reverse(index)))
            .map(|(index, _)| index)
            .expect("fixed feature count array is nonempty");
        let area = ((block.max.x - block.min.x) * (block.max.y - block.min.y)).max(1) as f32;
        let density = f32::from(total_features) / area;
        let fraction = (0.2 + density.sqrt() * 1.6).clamp(0.25, 0.8);
        let width = ((block.max.x - block.min.x) as f32 * fraction).max(1.0);
        let height = ((block.max.y - block.min.y) as f32 * fraction).max(1.0);
        let x = block.min.x as f32 + ((block.max.x - block.min.x) as f32 - width) * 0.5;
        let y = block.min.y as f32 + ((block.max.y - block.min.y) as f32 - height) * 0.5;
        let kind = [FeatureKind::Tree, FeatureKind::Rock, FeatureKind::BerryBush][feature_index];
        features.push(Instance::new(
            x,
            y,
            width,
            height,
            summary_feature_color(kind),
        ));
    }

    fn base_visual(&self) -> Option<usize> {
        let dominant = |indices: &[usize]| {
            indices
                .iter()
                .copied()
                .filter(|&index| self.visuals[index].count > 0)
                .max_by_key(|&index| (self.visuals[index].count, std::cmp::Reverse(index)))
        };
        let ordinary = [
            OCEAN_DEEP,
            OCEAN_SHALLOW,
            BEACH_VISUAL,
            DESERT_VISUAL,
            GRASS_VISUAL,
            SAVANNA_VISUAL,
            FOREST_VISUAL,
            WETLAND_VISUAL,
            TUNDRA_VISUAL,
            HILL_VISUAL,
            ROCK_VISUAL,
            SNOW_VISUAL,
            FALLBACK_VISUAL,
        ];
        dominant(&ordinary).or_else(|| dominant(&[LAKE_VISUAL, RIVER_VISUAL]))
    }

    fn detail_visual(&self, base: usize) -> Option<usize> {
        for index in [RIVER_VISUAL, LAKE_VISUAL] {
            if index != base && self.visuals[index].count > 0 {
                return Some(index);
            }
        }
        let ocean = self.visuals[OCEAN_DEEP].count + self.visuals[OCEAN_SHALLOW].count;
        let land: u16 = self.visuals[BEACH_VISUAL..]
            .iter()
            .map(|sample| sample.count)
            .sum();
        if ocean > 0 && land > 0 {
            if base == OCEAN_DEEP || base == OCEAN_SHALLOW {
                return (BEACH_VISUAL..TERRAIN_VISUAL_COUNT)
                    .filter(|&index| self.visuals[index].count > 0)
                    .max_by_key(|&index| (self.visuals[index].count, std::cmp::Reverse(index)));
            }
            return [OCEAN_DEEP, OCEAN_SHALLOW]
                .into_iter()
                .filter(|&index| self.visuals[index].count > 0)
                .max_by_key(|&index| (self.visuals[index].count, std::cmp::Reverse(index)));
        }
        [SNOW_VISUAL, ROCK_VISUAL, HILL_VISUAL]
            .into_iter()
            .find(|&index| index != base && self.visuals[index].count > 0)
    }
}

fn build_chunk_summary(
    world: &World,
    coord: ChunkCoord,
    coverage: WorldRect,
    step: u32,
) -> ChunkRenderSummary {
    let chunk_bounds = coord
        .bounds()
        .expect("resident chunks always have representable bounds");
    let blocks_per_axis = CHUNK_SIZE as usize / step as usize;
    let mut blocks = vec![SummaryAccumulator::default(); blocks_per_axis * blocks_per_axis];
    let block_index = |position: WorldPosition| {
        let x = (position.x - chunk_bounds.min.x) as usize / step as usize;
        let y = (position.y - chunk_bounds.min.y) as usize / step as usize;
        y * blocks_per_axis + x
    };
    assert_eq!(
        world.visit_cells_in_chunk(coord, |position, cell| {
            blocks[block_index(position)].observe_cell(position, chunk_bounds.min, cell);
        }),
        Some(coverage)
    );
    assert_eq!(
        world.visit_features_in_chunk(coord, |feature| {
            blocks[block_index(feature.position)].observe_feature(feature.kind);
        }),
        Some(coverage)
    );

    let mut terrain = Vec::with_capacity(blocks.len() * 2);
    let mut features = Vec::with_capacity(blocks.len());
    for (index, block) in blocks.iter().enumerate() {
        let x = index % blocks_per_axis;
        let y = index / blocks_per_axis;
        let block_min = WorldPosition {
            x: chunk_bounds.min.x + (x * step as usize) as i64,
            y: chunk_bounds.min.y + (y * step as usize) as i64,
        };
        let block_bounds = WorldRect {
            min: block_min,
            max: WorldPosition {
                x: block_min.x + i64::from(step),
                y: block_min.y + i64::from(step),
            },
        };
        if let Some(clipped) = block_bounds.intersection(coverage) {
            block.instances(clipped, chunk_bounds.min, &mut terrain, &mut features);
        }
    }
    terrain.shrink_to_fit();
    features.shrink_to_fit();
    ChunkRenderSummary { terrain, features }
}

fn terrain_visual(cell: TerrainCell) -> usize {
    match (cell.surface(), cell.biome()) {
        (SurfaceType::DeepWater, BiomeType::Ocean) => OCEAN_DEEP,
        (SurfaceType::ShallowWater, BiomeType::Ocean) => OCEAN_SHALLOW,
        (_, BiomeType::Lake) => LAKE_VISUAL,
        (_, BiomeType::River) => RIVER_VISUAL,
        (SurfaceType::Sand, BiomeType::Beach) => BEACH_VISUAL,
        (SurfaceType::Sand, BiomeType::Desert) => DESERT_VISUAL,
        (SurfaceType::Soil, BiomeType::Grassland) => GRASS_VISUAL,
        (SurfaceType::Soil, BiomeType::Savanna) => SAVANNA_VISUAL,
        (SurfaceType::Soil, BiomeType::Forest) => FOREST_VISUAL,
        (SurfaceType::Soil, BiomeType::Wetland) => WETLAND_VISUAL,
        (_, BiomeType::Tundra) => TUNDRA_VISUAL,
        (SurfaceType::Hill, _) => HILL_VISUAL,
        (SurfaceType::Rock, _) => ROCK_VISUAL,
        (SurfaceType::SnowIce, _) => SNOW_VISUAL,
        _ => FALLBACK_VISUAL,
    }
}

fn visible_detail_bounds(detail: WorldRect, block: WorldRect) -> WorldRect {
    let minimum = ((block.max.x - block.min.x).min(block.max.y - block.min.y) / 4).max(1);
    let inflate_axis = |min: i64, max: i64, block_min: i64, block_max: i64| {
        let missing = minimum.saturating_sub(max - min);
        let before = missing / 2;
        let after = missing - before;
        ((min - before).max(block_min), (max + after).min(block_max))
    };
    let (min_x, max_x) = inflate_axis(detail.min.x, detail.max.x, block.min.x, block.max.x);
    let (min_y, max_y) = inflate_axis(detail.min.y, detail.max.y, block.min.y, block.max.y);
    WorldRect {
        min: WorldPosition { x: min_x, y: min_y },
        max: WorldPosition { x: max_x, y: max_y },
    }
}

fn rect_instance(bounds: WorldRect, color: u32) -> Instance {
    Instance::new(
        bounds.min.x as f32,
        bounds.min.y as f32,
        (bounds.max.x - bounds.min.x) as f32,
        (bounds.max.y - bounds.min.y) as f32,
        color,
    )
}

const fn feature_color(kind: FeatureKind) -> u32 {
    match kind {
        FeatureKind::Tree => rgba(24, 72, 28, 255),
        FeatureKind::Rock => rgba(118, 116, 108, 255),
        FeatureKind::BerryBush => rgba(112, 42, 74, 255),
    }
}

const fn spawn_kind_color(kind: SpawnKind) -> u32 {
    match kind {
        SpawnKind::Tree => feature_color(FeatureKind::Tree),
        SpawnKind::BerryBush => feature_color(FeatureKind::BerryBush),
        SpawnKind::Rock => feature_color(FeatureKind::Rock),
        SpawnKind::Water => rgba(45, 132, 202, 235),
    }
}

const fn summary_feature_color(kind: FeatureKind) -> u32 {
    match kind {
        FeatureKind::Tree => rgba(24, 72, 28, 230),
        FeatureKind::Rock => rgba(118, 116, 108, 230),
        FeatureKind::BerryBush => rgba(112, 42, 74, 230),
    }
}

fn terrain_sample_step(scale: f32) -> u32 {
    let requested = (MIN_TERRAIN_SAMPLE_PIXELS / scale.max(f32::EPSILON))
        .ceil()
        .max(1.0) as u32;
    requested.clamp(1, CHUNK_SIZE as u32).next_power_of_two()
}

fn cache_margin(scale: f32) -> i64 {
    (CACHE_MARGIN_PIXELS / scale.max(f32::EPSILON))
        .ceil()
        .max(1.0) as i64
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CacheSyncAction {
    Rebuild,
    Skip,
    AdvanceRevision,
}

fn cache_sync_action(
    cache_contains_view: bool,
    step_matches: bool,
    revision_changed: bool,
    allow_world_sync: bool,
    changed_affects_cache: bool,
) -> CacheSyncAction {
    if !cache_contains_view || !step_matches {
        return CacheSyncAction::Rebuild;
    }
    if !revision_changed || !changed_affects_cache {
        return if revision_changed {
            CacheSyncAction::AdvanceRevision
        } else {
            CacheSyncAction::Skip
        };
    }
    if allow_world_sync {
        CacheSyncAction::Rebuild
    } else {
        CacheSyncAction::Skip
    }
}

fn static_instance_chunks(instances: &[Instance]) -> std::slice::Chunks<'_, Instance> {
    instances.chunks(MAX_INSTANCES_PER_BUFFER)
}

fn terrain_color(cell: sim_core::TerrainCell) -> u32 {
    let shade = (cell.elevation >> 12) as u8;
    match (cell.surface(), cell.biome()) {
        (SurfaceType::DeepWater, BiomeType::Ocean) => rgba(16, 48 + shade, 94 + shade, 255),
        (SurfaceType::ShallowWater, BiomeType::Ocean) => rgba(28, 84 + shade, 126 + shade, 255),
        (SurfaceType::DeepWater, BiomeType::Lake) => rgba(24, 66 + shade, 112 + shade, 255),
        (SurfaceType::ShallowWater, BiomeType::Lake) => rgba(40, 102 + shade, 142 + shade, 255),
        (SurfaceType::DeepWater, BiomeType::River) => rgba(20, 74 + shade, 128 + shade, 255),
        (SurfaceType::ShallowWater, BiomeType::River) => rgba(38, 116 + shade, 154 + shade, 255),
        (SurfaceType::Sand, BiomeType::Beach) => rgba(210 + shade, 190 + shade, 126, 255),
        (SurfaceType::Sand, BiomeType::Desert) => rgba(184 + shade, 150 + shade, 75, 255),
        (SurfaceType::Soil, BiomeType::Grassland) => rgba(50 + shade, 112 + shade, 51, 255),
        (SurfaceType::Soil, BiomeType::Savanna) => rgba(118 + shade, 126 + shade, 55, 255),
        (SurfaceType::Soil, BiomeType::Forest) => rgba(37, 86 + shade, 39, 255),
        (SurfaceType::Soil, BiomeType::Wetland) => rgba(48, 94 + shade, 74 + shade, 255),
        (SurfaceType::Soil, BiomeType::Tundra) => rgba(105 + shade, 119 + shade, 105 + shade, 255),
        (SurfaceType::Hill, _) => rgba(100 + shade, 108 + shade, 72, 255),
        (SurfaceType::Rock, _) => rgba(125 + shade, 124 + shade, 119 + shade, 255),
        (SurfaceType::SnowIce, _) => rgba(220 + shade, 229 + shade, 234 + shade, 255),
        _ => rgba(255, 0, 255, 255),
    }
}

fn chunk_outline(inspection: ChunkInspection, scale: f32) -> Option<[Instance; 4]> {
    if scale * (CHUNK_SIZE as f32) < MIN_CHUNK_OUTLINE_PIXELS {
        return None;
    }
    let bounds = inspection.bounds;
    let x = bounds.min.x as f32;
    let y = bounds.min.y as f32;
    let width = (bounds.max.x - bounds.min.x) as f32;
    let height = (bounds.max.y - bounds.min.y) as f32;
    let line = (1.0 / scale.max(f32::EPSILON)).clamp(1.0, MAX_CHUNK_OUTLINE_WORLD_WIDTH);
    let color = match inspection.presence {
        ChunkPresence::Missing => rgba(235, 70, 70, 190),
        ChunkPresence::InitialUnloaded | ChunkPresence::PartialInitialUnloaded => {
            rgba(145, 145, 145, 190)
        }
        ChunkPresence::PartialInitial => rgba(255, 205, 55, 190),
        ChunkPresence::Initial => rgba(80, 180, 255, 180),
        ChunkPresence::Retained => rgba(85, 225, 135, 190),
        ChunkPresence::RetainedPartialInitial => rgba(85, 225, 135, 190),
    };
    Some([
        Instance::new(x, y, width, line, color),
        Instance::new(x, y + height - line, width, line, color),
        Instance::new(x, y, line, height, color),
        Instance::new(x + width - line, y, line, height, color),
    ])
}

fn world_border(scale: f32) -> [Instance; 4] {
    let bounds = WORLD_GENERATION_BOUNDS;
    let x = bounds.min.x as f32;
    let y = bounds.min.y as f32;
    let width = (bounds.max.x - bounds.min.x) as f32;
    let height = (bounds.max.y - bounds.min.y) as f32;
    let line = (2.0 / scale.max(f32::EPSILON)).clamp(1.0, MAX_WORLD_BORDER_WIDTH);
    let color = rgba(245, 40, 40, 230);
    [
        Instance::new(x, y, width, line, color),
        Instance::new(x, y + height - line, width, line, color),
        Instance::new(x, y, line, height, color),
        Instance::new(x + width - line, y, line, height, color),
    ]
}

fn write_hud_text(output: &mut String, world: &World, state: &RenderState) {
    output.clear();
    let activity = if state.snapshot.paused {
        "PAUSED"
    } else {
        "RUNNING"
    };
    let speed = state.snapshot.speed;
    let total_tenths = (state.snapshot.simulated_seconds.max(0.0) * 10.0) as u64;
    let hours = total_tenths / 36_000;
    let minutes = total_tenths / 600 % 60;
    let seconds = total_tenths / 10 % 60;
    let tenths = total_tenths % 10;

    writeln!(output, "LET THERE BE LIFE").expect("writing to String cannot fail");
    if speed.fract() == 0.0 {
        writeln!(output, "{activity}  SPEED {speed:.0}X").expect("writing to String cannot fail");
    } else {
        writeln!(output, "{activity}  SPEED {speed:.1}X").expect("writing to String cannot fail");
    }
    writeln!(
        output,
        "SIM {hours:04}:{minutes:02}:{seconds:02}.{tenths}  TICK {}",
        state.snapshot.tick
    )
    .expect("writing to String cannot fail");
    if let Some(message) = &state.spawn_message {
        writeln!(output, "{message}").expect("writing to String cannot fail");
    }
    writeln!(
        output,
        "SEED {}  LOADED {}  REV {}",
        state.snapshot.seed,
        world.loaded_chunk_count(),
        world.revision()
    )
    .expect("writing to String cannot fail");
    writeln!(output, "GEN {}", generation_label(state.generation_status))
        .expect("writing to String cannot fail");
    writeln!(
        output,
        "AGENTS {}  TOTAL {}  LIVING {}  ACTIVE {}  DEAD {}",
        population_label(state.population_status),
        state.snapshot.agent_count,
        state.snapshot.living_agent_count,
        state.snapshot.active_agent_count,
        state.snapshot.death_count,
    )
    .expect("writing to String cannot fail");
    if let Some(selection) = state.selection {
        writeln!(
            output,
            "SELECT {} X {}  {}",
            selection.max.x - selection.min.x,
            selection.max.y - selection.min.y,
            if state.selection_valid {
                "VALID"
            } else {
                "INVALID"
            }
        )
        .expect("writing to String cannot fail");
    }

    let Some(position) = state.cursor_world else {
        writeln!(output, "CURSOR  MOVE OVER MAP TO INSPECT")
            .expect("writing to String cannot fail");
        writeln!(output, "L-DRAG PAN  R-DRAG GENERATE").expect("writing to String cannot fail");
        writeln!(output, "T AGENT  NUM5 OBJECT MENU").expect("writing to String cannot fail");
        write!(output, "SPACE PAUSE  1-9 SPEED  C CANCEL").expect("writing to String cannot fail");
        return;
    };

    writeln!(output, "CURSOR X {}  Y {}", position.x, position.y)
        .expect("writing to String cannot fail");
    match world.inspect_chunk_at(position) {
        Ok(inspection) => {
            writeln!(
                output,
                "CHUNK X {} Y {}  LOCAL {},{}",
                inspection.coord.x, inspection.coord.y, inspection.local.x, inspection.local.y
            )
            .expect("writing to String cannot fail");
            writeln!(output, "COVERAGE {}", coverage_label(inspection.presence))
                .expect("writing to String cannot fail");
            if let Some(cell) = world.cell(position) {
                writeln!(
                    output,
                    "SURFACE {}  BIOME {}",
                    surface_label(cell.surface()),
                    biome_label(cell.biome())
                )
                .expect("writing to String cannot fail");
                if let Some(climate) = world.climate_at(position) {
                    writeln!(
                        output,
                        "ELEV {}  TEMP {}  MOIST {}",
                        cell.elevation, climate.temperature, climate.moisture
                    )
                    .expect("writing to String cannot fail");
                    write!(output, "WIND {}  FEATURE ", wind_label(climate.wind))
                        .expect("writing to String cannot fail");
                    if let Some(object) = state.cursor_spawned_object {
                        match object.remaining {
                            Some(remaining) => write!(
                                output,
                                "SPAWNED {}  {} CAP {}",
                                spawn_kind_label(object.kind),
                                resource_label(object.kind.resource().expect("resource kind").kind),
                                remaining
                            )
                            .expect("writing to String cannot fail"),
                            None => write!(
                                output,
                                "SPAWNED {}  DRINKABLE",
                                spawn_kind_label(object.kind)
                            )
                            .expect("writing to String cannot fail"),
                        }
                    } else if let Some(feature) = world.feature_at(position) {
                        let resource = feature.base_resource();
                        write!(
                            output,
                            "{}  {} CAP {}",
                            feature_label(feature.kind),
                            resource_label(resource.kind),
                            resource.capacity
                        )
                        .expect("writing to String cannot fail");
                    } else {
                        output.push_str("NONE");
                    }
                }
            } else {
                write!(output, "CELL UNLOADED").expect("writing to String cannot fail");
            }
        }
        Err(GenerateAreaError::OutsideWorldBounds) => {
            write!(output, "OUTSIDE WORLD BOUNDARY").expect("writing to String cannot fail");
        }
        Err(_) => {
            write!(output, "CHUNK COORDINATES UNAVAILABLE").expect("writing to String cannot fail");
        }
    }
}

fn write_agent_text(output: &mut String, inspection: Option<AgentInspection>) {
    output.clear();
    let Some(agent) = inspection else {
        return;
    };
    writeln!(output, "AGENT {}", agent.view.id.get()).unwrap();
    writeln!(
        output,
        "POSITION X {}  Y {}",
        agent.view.position.x, agent.view.position.y
    )
    .unwrap();
    writeln!(output, "ACTIVITY {}", activity_label(agent.view.activity)).unwrap();
    if let Some(policy) = agent.policy {
        writeln!(output, "GOAL {}", goal_label(policy.goal)).unwrap();
        writeln!(output, "WHY {}", policy_reason_label(policy.reason)).unwrap();
        if let Some(target) = policy.target {
            writeln!(output, "TARGET X {}  Y {}", target.x, target.y).unwrap();
        } else {
            writeln!(output, "TARGET NONE").unwrap();
        }
        writeln!(
            output,
            "STATUS {}  RETRIES {}",
            policy_status_label(agent.view.activity, policy),
            policy.retry_count
        )
        .unwrap();
        writeln!(
            output,
            "SEARCH HEADING {}",
            exploration_heading_label(policy.exploration_heading)
        )
        .unwrap();
    } else {
        writeln!(output, "POLICY NONE").unwrap();
    }
    if let Some(needs) = agent.needs {
        write_need(output, "HUNGER", needs.hunger);
        write_need(output, "THIRST", needs.thirst);
        write_need(output, "REST", needs.rest);
        write_need(output, "EXPOSURE", needs.exposure);
        if let Some(next) = needs.next_threshold {
            writeln!(
                output,
                "NEXT {} AT TICK {}",
                need_label(next.kind),
                next.due.ticks()
            )
            .unwrap();
        } else {
            writeln!(output, "NEXT NEED NONE").unwrap();
        }
    } else {
        writeln!(output, "NEEDS UNAVAILABLE").unwrap();
    }
    if let Some(inventory) = agent.inventory {
        writeln!(
            output,
            "INVENTORY F {}  W {}  S {}",
            inventory.food, inventory.wood, inventory.stone
        )
        .unwrap();
    }
    if let Some(health) = agent.health {
        writeln!(
            output,
            "HEALTH {}  {}",
            health.value,
            health_label(health.status)
        )
        .unwrap();
        if let Some(due) = health.next_consequence {
            writeln!(output, "NEXT DAMAGE TICK {}", due.ticks()).unwrap();
        }
    }
    if let Some(death) = agent.death {
        writeln!(output, "DEATH CAUSE {}", death_cause_label(death.cause)).unwrap();
        writeln!(output, "DIED AT TICK {}", death.at.ticks()).unwrap();
    }
    if let Some(sleep) = agent.sleep {
        writeln!(
            output,
            "SLEEP {}  WAKE {}",
            sleep_quality_label(sleep.quality),
            sleep.planned_wake.ticks()
        )
        .unwrap();
    } else {
        writeln!(output, "SLEEP NONE").unwrap();
    }
    debug_assert!(output.len() <= AGENT_TEXT_CAPACITY);
}

fn write_need(output: &mut String, label: &str, need: sim_core::NeedLevelView) {
    writeln!(
        output,
        "{label} {} OF {}  RATE {:+}",
        need.value, need.threshold, need.rate_per_period
    )
    .unwrap();
}

const fn generation_label(status: GenerationStatus) -> &'static str {
    match status {
        GenerationStatus::Idle => "READY",
        GenerationStatus::Bootstrap => "LOADING WORLD",
        GenerationStatus::Manual => "GENERATING SELECTION",
        GenerationStatus::Cancelling => "CANCELLING",
        GenerationStatus::WorkerUnavailable => "WORKER OFFLINE",
    }
}

const fn population_label(status: PopulationStatus) -> &'static str {
    match status {
        PopulationStatus::Waiting => "WAITING FOR WORLD",
        PopulationStatus::Ready => "READY - PRESS T",
        PopulationStatus::Active => "ACTIVE",
        PopulationStatus::Failed => "STARTUP FAILED",
    }
}

const fn activity_label(activity: AgentActivity) -> &'static str {
    match activity {
        AgentActivity::Idle => "IDLE",
        AgentActivity::Moving => "MOVING",
        AgentActivity::Gathering => "GATHERING",
        AgentActivity::Building => "BUILDING",
        AgentActivity::Sleeping => "SLEEPING",
        AgentActivity::Incapacitated => "INCAPACITATED",
        AgentActivity::Dead => "DEAD",
    }
}

const fn goal_label(goal: PhysicalGoal) -> &'static str {
    match goal {
        PhysicalGoal::SeekWater => "SEEK WATER",
        PhysicalGoal::SeekFood => "SEEK FOOD",
        PhysicalGoal::GatherMaterial => "GATHER MATERIAL",
        PhysicalGoal::Eat => "EAT",
        PhysicalGoal::Drink => "DRINK",
        PhysicalGoal::Sleep => "SLEEP",
        PhysicalGoal::SeekShelter => "SEEK SHELTER",
        PhysicalGoal::BuildShelter => "BUILD SHELTER",
        PhysicalGoal::Wait => "WAIT",
        PhysicalGoal::Incapacitated => "INCAPACITATED",
        PhysicalGoal::Explore => "EXPLORE",
    }
}

const fn policy_reason_label(reason: PolicyReason) -> &'static str {
    match reason {
        PolicyReason::InitialDecision => "INITIAL DECISION",
        PolicyReason::ThirstThreshold => "THIRST THRESHOLD",
        PolicyReason::HungerThreshold => "HUNGER THRESHOLD",
        PolicyReason::RestThreshold => "REST THRESHOLD",
        PolicyReason::ExposureThreshold => "EXPOSURE THRESHOLD",
        PolicyReason::NoUrgentNeed => "NO URGENT NEED",
        PolicyReason::RouteArrived => "ROUTE ARRIVED",
        PolicyReason::ActionCompleted => "ACTION COMPLETED",
        PolicyReason::ShelterMaterials => "SHELTER MATERIALS",
        PolicyReason::Retry => "RETRY",
    }
}

const fn death_cause_label(cause: DeathCause) -> &'static str {
    match cause {
        DeathCause::Dehydration => "DEHYDRATION",
        DeathCause::Exposure => "EXPOSURE",
        DeathCause::Starvation => "STARVATION",
        DeathCause::Exhaustion => "EXHAUSTION",
    }
}

const fn need_label(need: NeedKind) -> &'static str {
    match need {
        NeedKind::Hunger => "HUNGER",
        NeedKind::Thirst => "THIRST",
        NeedKind::Rest => "REST",
        NeedKind::Exposure => "EXPOSURE",
    }
}

const fn health_label(status: HealthStatus) -> &'static str {
    match status {
        HealthStatus::Healthy => "HEALTHY",
        HealthStatus::Incapacitated => "INCAPACITATED",
        HealthStatus::Dead => "DEAD",
    }
}

const fn sleep_quality_label(quality: SleepQuality) -> &'static str {
    match quality {
        SleepQuality::OpenGround => "OPEN GROUND",
        SleepQuality::Sheltered => "SHELTERED",
    }
}

const fn policy_status_label(activity: AgentActivity, policy: PhysicalPolicyView) -> &'static str {
    if matches!(activity, AgentActivity::Incapacitated | AgentActivity::Dead) {
        "INACTIVE"
    } else if policy.committed {
        "COMMITTED"
    } else if policy.retry_count > 0 {
        "BACKOFF"
    } else {
        "DECIDING"
    }
}

const fn exploration_heading_label(heading: ExplorationHeading) -> &'static str {
    match heading {
        ExplorationHeading::North => "N",
        ExplorationHeading::NorthEast => "NE",
        ExplorationHeading::East => "E",
        ExplorationHeading::SouthEast => "SE",
        ExplorationHeading::South => "S",
        ExplorationHeading::SouthWest => "SW",
        ExplorationHeading::West => "W",
        ExplorationHeading::NorthWest => "NW",
    }
}

const fn coverage_label(presence: ChunkPresence) -> &'static str {
    match presence {
        ChunkPresence::Missing => "MISSING",
        ChunkPresence::InitialUnloaded => "INITIAL UNLOADED",
        ChunkPresence::PartialInitialUnloaded => "PARTIAL INITIAL UNLOADED",
        ChunkPresence::PartialInitial => "PARTIAL INITIAL",
        ChunkPresence::Initial => "INITIAL",
        ChunkPresence::Retained => "RETAINED",
        ChunkPresence::RetainedPartialInitial => "PARTIAL INITIAL RETAINED",
    }
}

const fn surface_label(surface: SurfaceType) -> &'static str {
    match surface {
        SurfaceType::DeepWater => "DEEP WATER",
        SurfaceType::ShallowWater => "SHALLOW WATER",
        SurfaceType::Sand => "SAND",
        SurfaceType::Soil => "SOIL",
        SurfaceType::Hill => "HILL",
        SurfaceType::Rock => "ROCK",
        SurfaceType::SnowIce => "SNOW/ICE",
    }
}

const fn biome_label(biome: BiomeType) -> &'static str {
    match biome {
        BiomeType::Ocean => "OCEAN",
        BiomeType::Lake => "LAKE",
        BiomeType::River => "RIVER",
        BiomeType::Beach => "BEACH",
        BiomeType::Desert => "DESERT",
        BiomeType::Grassland => "GRASSLAND",
        BiomeType::Savanna => "SAVANNA",
        BiomeType::Forest => "FOREST",
        BiomeType::Wetland => "WETLAND",
        BiomeType::Tundra => "TUNDRA",
        BiomeType::Alpine => "ALPINE",
    }
}

const fn wind_label(wind: PrevailingWind) -> &'static str {
    match wind {
        PrevailingWind::Southeast => "SE",
        PrevailingWind::Northwest => "NW",
    }
}

const fn feature_label(feature: FeatureKind) -> &'static str {
    match feature {
        FeatureKind::Tree => "TREE",
        FeatureKind::Rock => "ROCK",
        FeatureKind::BerryBush => "BERRY BUSH",
    }
}

const fn resource_label(resource: ResourceKind) -> &'static str {
    match resource {
        ResourceKind::Food => "FOOD",
        ResourceKind::Wood => "WOOD",
        ResourceKind::Stone => "STONE",
    }
}

fn build_screen_overlay(
    instances: &mut Vec<Instance>,
    text: &str,
    agent_text: Option<&str>,
    state: &RenderState,
    width: u32,
    height: u32,
) {
    instances.clear();
    let scale = state.ui_scale.clamp(1.0, 3.0);
    let pixel = 2.0 * scale;
    let advance = 6.0 * pixel;
    let line_height = 9.0 * pixel;
    let margin = 14.0 * scale;
    let text_x = margin + 14.0 * scale;
    let text_y = margin + 10.0 * scale;
    let line_count = text.lines().count().max(1);
    let longest_line = text.lines().map(str::len).max().unwrap_or(1) as f32;
    let panel_width = longest_line * advance + 28.0 * scale;
    let panel_height = line_count as f32 * line_height + 20.0 * scale;

    instances.push(Instance::new(
        margin + 3.0 * scale,
        margin + 3.0 * scale,
        panel_width,
        panel_height,
        rgba(0, 0, 0, 105),
    ));
    instances.push(Instance::new(
        margin,
        margin,
        panel_width,
        panel_height,
        rgba(8, 15, 20, 232),
    ));
    instances.push(Instance::new(
        margin,
        margin,
        4.0 * scale,
        panel_height,
        rgba(71, 190, 194, 255),
    ));
    instances.push(Instance::new(
        text_x,
        text_y + line_height - 3.0 * scale,
        panel_width - 28.0 * scale,
        scale,
        rgba(71, 190, 194, 100),
    ));
    instances.push(Instance::new(
        text_x,
        text_y + 5.0 * line_height - 3.0 * scale,
        panel_width - 28.0 * scale,
        scale,
        rgba(120, 145, 150, 75),
    ));

    for (line_index, line) in text.lines().enumerate() {
        let color = match line_index {
            0 => rgba(151, 232, 229, 255),
            1 if state.snapshot.paused => rgba(240, 183, 78, 255),
            1 => rgba(100, 220, 145, 255),
            4 if state.generation_status == GenerationStatus::WorkerUnavailable => {
                rgba(245, 96, 86, 255)
            }
            4 if state.generation_status != GenerationStatus::Idle => rgba(236, 196, 84, 255),
            _ => rgba(218, 229, 226, 255),
        };
        push_bitmap_text(
            instances,
            line,
            text_x,
            text_y + line_index as f32 * line_height,
            pixel,
            color,
        );
    }

    if let Some(agent_text) = agent_text {
        push_agent_panel(instances, agent_text, width as f32, margin, scale);
    }
    if let Some(menu) = state.spawn_menu {
        push_spawn_menu(instances, menu, height as f32, margin, scale);
    }

    let rail_margin = 28.0 * scale;
    let square = 14.0 * scale;
    let rail_width = (width as f32 - rail_margin * 2.0).max(square);
    let rail_y = height as f32 - 19.0 * scale;
    let cycle = (state.snapshot.simulated_seconds.max(0.0) % 60.0) as f32 / 60.0;
    let travel = (rail_width - square).max(0.0);
    let marker_x = rail_margin + travel * cycle;
    instances.push(Instance::new(
        rail_margin,
        rail_y - scale,
        rail_width,
        2.0 * scale,
        rgba(224, 220, 191, 70),
    ));
    instances.push(Instance::new(
        rail_margin,
        rail_y - scale,
        travel * cycle + square * 0.5,
        2.0 * scale,
        rgba(235, 216, 130, 155),
    ));
    instances.push(Instance::new(
        marker_x,
        rail_y - square * 0.5,
        square,
        square,
        rgba(235, 216, 130, 255),
    ));
}

fn push_spawn_menu(
    instances: &mut Vec<Instance>,
    menu: SpawnMenuView,
    screen_height: f32,
    margin: f32,
    scale: f32,
) {
    let pixel = 2.0 * scale;
    let line_height = 9.0 * pixel;
    let panel_width = 180.0 * scale;
    let panel_height = 7.0 * line_height + 18.0 * scale;
    let x = margin;
    let y = (screen_height - panel_height - 38.0 * scale).max(margin);
    instances.push(Instance::new(
        x + 3.0 * scale,
        y + 3.0 * scale,
        panel_width,
        panel_height,
        rgba(0, 0, 0, 105),
    ));
    instances.push(Instance::new(
        x,
        y,
        panel_width,
        panel_height,
        rgba(8, 15, 20, 240),
    ));
    instances.push(Instance::new(
        x,
        y,
        4.0 * scale,
        panel_height,
        rgba(235, 216, 130, 255),
    ));
    push_bitmap_text(
        instances,
        if menu.placing {
            "PLACE MODE"
        } else {
            "SPAWN MENU"
        },
        x + 14.0 * scale,
        y + 9.0 * scale,
        pixel,
        rgba(245, 226, 145, 255),
    );
    for (index, kind) in SpawnKind::ALL.into_iter().enumerate() {
        let row_y = y + 9.0 * scale + (index as f32 + 1.5) * line_height;
        let selected = kind == menu.selected;
        if selected {
            instances.push(Instance::new(
                x + 9.0 * scale,
                row_y - 2.0 * scale,
                panel_width - 18.0 * scale,
                line_height,
                spawn_kind_color(kind) & 0x7fff_ffff,
            ));
        }
        push_bitmap_text(
            instances,
            spawn_kind_label(kind),
            x + 18.0 * scale,
            row_y,
            pixel,
            if selected {
                rgba(255, 255, 255, 255)
            } else {
                rgba(185, 199, 198, 255)
            },
        );
    }
    push_bitmap_text(
        instances,
        if menu.placing {
            "L CLICK PLACE  5 MENU  0 END"
        } else {
            "2/8 SELECT  5 PLACE  0 CLOSE"
        },
        x + 14.0 * scale,
        y + panel_height - line_height - 5.0 * scale,
        pixel,
        rgba(218, 229, 226, 255),
    );
}

const fn spawn_kind_label(kind: SpawnKind) -> &'static str {
    match kind {
        SpawnKind::Tree => "TREE",
        SpawnKind::BerryBush => "BERRIES",
        SpawnKind::Rock => "ROCK",
        SpawnKind::Water => "WATER",
    }
}

fn push_agent_panel(
    instances: &mut Vec<Instance>,
    text: &str,
    screen_width: f32,
    margin: f32,
    scale: f32,
) {
    let pixel = 2.0 * scale;
    let advance = 6.0 * pixel;
    let line_height = 9.0 * pixel;
    let line_count = text.lines().count().max(1);
    let longest_line = text.lines().map(str::len).max().unwrap_or(1) as f32;
    let panel_width = longest_line * advance + 28.0 * scale;
    let panel_height = line_count as f32 * line_height + 20.0 * scale;
    let panel_x = (screen_width - margin - panel_width).max(margin);
    let text_x = panel_x + 14.0 * scale;
    let text_y = margin + 10.0 * scale;
    instances.push(Instance::new(
        panel_x + 3.0 * scale,
        margin + 3.0 * scale,
        panel_width,
        panel_height,
        rgba(0, 0, 0, 105),
    ));
    instances.push(Instance::new(
        panel_x,
        margin,
        panel_width,
        panel_height,
        rgba(8, 15, 20, 232),
    ));
    instances.push(Instance::new(
        panel_x + panel_width - 4.0 * scale,
        margin,
        4.0 * scale,
        panel_height,
        rgba(71, 190, 194, 255),
    ));
    instances.push(Instance::new(
        text_x,
        text_y + line_height - 3.0 * scale,
        panel_width - 28.0 * scale,
        scale,
        rgba(71, 190, 194, 100),
    ));
    for (line_index, line) in text.lines().enumerate() {
        push_bitmap_text(
            instances,
            line,
            text_x,
            text_y + line_index as f32 * line_height,
            pixel,
            if line_index == 0 {
                rgba(151, 232, 229, 255)
            } else {
                rgba(218, 229, 226, 255)
            },
        );
    }
}

fn push_bitmap_text(
    instances: &mut Vec<Instance>,
    text: &str,
    x: f32,
    y: f32,
    pixel: f32,
    color: u32,
) {
    for (character_index, character) in text.chars().enumerate() {
        let glyph = glyph_rows(character);
        let glyph_x = x + character_index as f32 * pixel * 6.0;
        for (row_index, row) in glyph.into_iter().enumerate() {
            let mut column = 0;
            while column < 5 {
                if row & (1 << (4 - column)) == 0 {
                    column += 1;
                    continue;
                }
                let start = column;
                while column < 5 && row & (1 << (4 - column)) != 0 {
                    column += 1;
                }
                instances.push(Instance::new(
                    glyph_x + start as f32 * pixel,
                    y + row_index as f32 * pixel,
                    (column - start) as f32 * pixel,
                    pixel,
                    color,
                ));
            }
        }
    }
}

const fn glyph_rows(character: char) -> [u8; 7] {
    match character {
        'A' => [14, 17, 17, 31, 17, 17, 17],
        'B' => [30, 17, 17, 30, 17, 17, 30],
        'C' => [14, 17, 16, 16, 16, 17, 14],
        'D' => [30, 17, 17, 17, 17, 17, 30],
        'E' => [31, 16, 16, 30, 16, 16, 31],
        'F' => [31, 16, 16, 30, 16, 16, 16],
        'G' => [14, 17, 16, 23, 17, 17, 15],
        'H' => [17, 17, 17, 31, 17, 17, 17],
        'I' => [31, 4, 4, 4, 4, 4, 31],
        'J' => [7, 2, 2, 2, 18, 18, 12],
        'K' => [17, 18, 20, 24, 20, 18, 17],
        'L' => [16, 16, 16, 16, 16, 16, 31],
        'M' => [17, 27, 21, 21, 17, 17, 17],
        'N' => [17, 25, 21, 19, 17, 17, 17],
        'O' => [14, 17, 17, 17, 17, 17, 14],
        'P' => [30, 17, 17, 30, 16, 16, 16],
        'Q' => [14, 17, 17, 17, 21, 18, 13],
        'R' => [30, 17, 17, 30, 20, 18, 17],
        'S' => [15, 16, 16, 14, 1, 1, 30],
        'T' => [31, 4, 4, 4, 4, 4, 4],
        'U' => [17, 17, 17, 17, 17, 17, 14],
        'V' => [17, 17, 17, 17, 17, 10, 4],
        'W' => [17, 17, 17, 21, 21, 21, 10],
        'X' => [17, 17, 10, 4, 10, 17, 17],
        'Y' => [17, 17, 10, 4, 4, 4, 4],
        'Z' => [31, 1, 2, 4, 8, 16, 31],
        '0' => [14, 17, 19, 21, 25, 17, 14],
        '1' => [4, 12, 4, 4, 4, 4, 14],
        '2' => [14, 17, 1, 2, 4, 8, 31],
        '3' => [30, 1, 1, 14, 1, 1, 30],
        '4' => [2, 6, 10, 18, 31, 2, 2],
        '5' => [31, 16, 16, 30, 1, 1, 30],
        '6' => [14, 16, 16, 30, 17, 17, 14],
        '7' => [31, 1, 2, 4, 8, 8, 8],
        '8' => [14, 17, 17, 14, 17, 17, 14],
        '9' => [14, 17, 17, 15, 1, 1, 14],
        ':' => [0, 4, 4, 0, 4, 4, 0],
        ',' => [0, 0, 0, 0, 4, 4, 8],
        '.' => [0, 0, 0, 0, 0, 4, 4],
        '-' => [0, 0, 0, 31, 0, 0, 0],
        '+' => [0, 4, 4, 31, 4, 4, 0],
        '/' => [1, 1, 2, 4, 8, 16, 16],
        ' ' => [0; 7],
        _ => [31, 1, 2, 4, 0, 4, 0],
    }
}

const fn selection_color(valid: bool) -> u32 {
    if valid {
        rgba(255, 220, 35, 72)
    } else {
        rgba(235, 48, 48, 96)
    }
}

const fn rgba(red: u8, green: u8, blue: u8, alpha: u8) -> u32 {
    red as u32 | (green as u32) << 8 | (blue as u32) << 16 | (alpha as u32) << 24
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_core::WorldConfig;

    fn test_render_state(cursor_world: Option<WorldPosition>) -> RenderState {
        RenderState {
            snapshot: SimulationSnapshot {
                tick: 3_721,
                simulated_seconds: 62.0,
                paused: false,
                speed: 4.0,
                seed: 7,
                agent_count: 0,
                living_agent_count: 0,
                active_agent_count: 0,
                death_count: 0,
                scheduled_event_count: 0,
                structure_count: 0,
            },
            camera: Camera::at_origin(),
            ui_scale: 1.0,
            cursor_world,
            cursor_spawned_object: None,
            inspected: None,
            hovered: None,
            selection: None,
            selection_valid: true,
            generation_status: GenerationStatus::Idle,
            population_status: PopulationStatus::Active,
            hovered_agent: None,
            spawn_message: None,
            spawn_menu: None,
        }
    }

    #[test]
    fn coarse_view_reduces_terrain_instances() {
        let world = World::generate(1, WorldConfig::new(64, 64).unwrap());
        let bounds = world.initial_bounds();
        let (full, _) = build_world_instances(&world, bounds, 1);
        let (coarse, _) = build_world_instances(&world, bounds, 4);

        assert_eq!(full.len(), 4_096);
        assert!((256..=512).contains(&coarse.len()));
        assert_eq!(
            coarse
                .iter()
                .filter(|instance| instance.size == [4.0, 4.0])
                .count(),
            256
        );
    }

    #[test]
    fn coarse_summary_preserves_unaligned_features_as_density_markers() {
        let world = World::generate(42, WorldConfig::new(128, 128).unwrap());
        let bounds = world.initial_bounds();
        let (_, exact) = build_world_instances(&world, bounds, 1);
        assert!(!exact.is_empty(), "probe must contain generated features");
        assert!(exact.iter().any(|instance| {
            instance.position[0] as i64 % 64 != 0 || instance.position[1] as i64 % 64 != 0
        }));

        let (_, coarse) = build_world_instances(&world, bounds, 64);
        assert!(!coarse.is_empty());
        assert!(coarse.len() <= 4, "one marker per coarse block");
    }

    #[test]
    fn summary_priority_preserves_water_coasts_and_mountain_minorities() {
        let detail = |base: usize, minority: usize| {
            let mut summary = SummaryAccumulator::default();
            summary.visuals[base].count = 100;
            summary.visuals[minority].count = 1;
            (summary.base_visual(), summary.detail_visual(base))
        };
        assert_eq!(
            detail(GRASS_VISUAL, RIVER_VISUAL),
            (Some(GRASS_VISUAL), Some(RIVER_VISUAL))
        );
        assert_eq!(
            detail(GRASS_VISUAL, LAKE_VISUAL),
            (Some(GRASS_VISUAL), Some(LAKE_VISUAL))
        );
        assert_eq!(
            detail(GRASS_VISUAL, OCEAN_SHALLOW),
            (Some(GRASS_VISUAL), Some(OCEAN_SHALLOW))
        );
        assert_eq!(
            detail(FOREST_VISUAL, SNOW_VISUAL),
            (Some(FOREST_VISUAL), Some(SNOW_VISUAL))
        );
    }

    #[test]
    fn feature_summary_marker_area_increases_with_density() {
        let world = World::generate(1, WorldConfig::new(64, 64).unwrap());
        let position = WorldPosition { x: 0, y: 0 };
        let cell = world.cell(position).expect("origin is resident");
        let block = WorldRect {
            min: position,
            max: WorldPosition { x: 64, y: 64 },
        };
        let marker = |count| {
            let mut summary = SummaryAccumulator::default();
            summary.observe_cell(position, position, cell);
            summary.feature_counts[0] = count;
            let mut terrain = Vec::new();
            let mut features = Vec::new();
            summary.instances(block, position, &mut terrain, &mut features);
            features[0]
        };
        let sparse = marker(1);
        let dense = marker(100);
        assert!(dense.size[0] > sparse.size[0]);
        assert!(dense.size[1] > sparse.size[1]);
    }

    #[test]
    fn coarse_summary_preserves_a_major_river_that_misses_block_origins() {
        let mut world = World::generate(1, WorldConfig::new(64, 64).unwrap());
        let river_probe = WorldRect {
            min: WorldPosition {
                x: -14_592,
                y: -14_976,
            },
            max: WorldPosition {
                x: -14_336,
                y: -14_720,
            },
        };
        world.generate_area(river_probe).unwrap();
        let mut river_colors = Vec::new();
        let mut has_unaligned_river = false;
        world.visit_cells_in(river_probe, |position, cell| {
            if cell.biome() == BiomeType::River {
                river_colors.push(terrain_color(cell));
                has_unaligned_river |=
                    position.x.rem_euclid(64) != 0 || position.y.rem_euclid(64) != 0;
            }
        });
        river_colors.sort_unstable();
        river_colors.dedup();
        assert!(
            has_unaligned_river,
            "canonical probe must contain an unaligned river"
        );

        let (coarse, _) = build_world_instances(&world, river_probe, 64);
        assert!(
            coarse
                .iter()
                .any(|instance| river_colors.binary_search(&instance.color).is_ok()),
            "minority river color disappeared from coarse summaries"
        );
    }

    #[test]
    fn summary_cache_retains_only_the_active_step_and_resident_margin() {
        let mut world = World::generate(7, WorldConfig::new(128, 64).unwrap());
        let distant = WorldRect {
            min: WorldPosition { x: 512, y: 0 },
            max: WorldPosition { x: 576, y: 64 },
        };
        world.generate_area(distant).unwrap();
        let mut cache = WorldSummaryCache::default();
        let initial = world.initial_bounds();
        let _ = cache.sync(&world, initial, 16, None);
        assert_eq!(cache.step, 16);
        assert_eq!(cache.chunks.len(), 4);
        assert!(cache.logical_bytes() > 0);

        let _ = cache.sync(&world, distant, 16, None);
        assert_eq!(cache.chunks.len(), 1);
        assert!(cache.chunks.contains_key(&ChunkCoord { x: 8, y: 0 }));

        let _ = cache.sync(&world, distant, 32, None);
        assert_eq!(cache.step, 32);
        assert_eq!(cache.chunks.len(), 1);

        let mut repeated = WorldSummaryCache::default();
        assert_eq!(
            cache.sync(&world, distant, 32, Some(distant)),
            repeated.sync(&world, distant, 32, None),
            "parallel summary construction and change-bound invalidation must be deterministic"
        );
    }

    #[test]
    #[ignore = "release-only renderer summary measurement"]
    fn release_summary_cache_measurement() {
        let side = std::env::var("SIM_SUMMARY_BENCH_SIZE")
            .ok()
            .and_then(|value| value.parse::<u32>().ok())
            .unwrap_or(1_024);
        let world = World::generate(1, WorldConfig::new(side, side).unwrap());
        let bounds = world.initial_bounds();
        for step in [2, 4, 8, 16, 32, 64] {
            let mut cache = WorldSummaryCache::default();
            let started = Instant::now();
            let (terrain, features) = cache.sync(&world, bounds, step, None);
            let elapsed = started.elapsed();
            println!(
                "summary-bench side={side} step={step} chunks={} cache_bytes={} terrain_instances={} feature_instances={} gpu_instance_bytes={} build_ms={:.3}",
                cache.chunks.len(),
                cache.logical_bytes(),
                terrain.len(),
                features.len(),
                (terrain.len() + features.len()) * size_of::<Instance>(),
                elapsed.as_secs_f64() * 1_000.0,
            );
        }
    }

    #[test]
    fn sample_step_targets_two_pixel_blocks_and_chunk_divisors() {
        assert_eq!(size_of::<VisualSample>(), 12);
        assert_eq!(size_of::<SummaryAccumulator>(), 186);
        assert_eq!(terrain_sample_step(4.0), 1);
        assert_eq!(terrain_sample_step(1.1), 2);
        assert_eq!(terrain_sample_step(0.5), 4);
        assert_eq!(terrain_sample_step(0.01), 64);
    }

    #[test]
    fn coarse_blocks_follow_exact_loaded_tile_coverage() {
        let initial = World::generate(1, WorldConfig::new(96, 64).unwrap());
        let (initial_instances, _) = build_world_instances(
            &initial,
            WorldRect {
                min: WorldPosition { x: 0, y: 0 },
                max: WorldPosition { x: 128, y: 64 },
            },
            64,
        );
        assert!(
            initial_instances
                .iter()
                .all(|instance| instance.position[0] + instance.size[0] <= 96.0)
        );

        let mut boundary = World::generate(1, WorldConfig::new(96, 64).unwrap());
        let boundary_bounds = WorldRect {
            min: WorldPosition { x: 96, y: 0 },
            max: WorldPosition { x: 128, y: 64 },
        };
        boundary.generate_area(boundary_bounds).unwrap();
        let (boundary_instances, _) = build_world_instances(
            &boundary,
            WorldRect {
                min: WorldPosition { x: 0, y: 0 },
                max: boundary_bounds.max,
            },
            64,
        );
        assert!(
            boundary_instances.iter().any(|instance| {
                instance.position == [64.0, 0.0] && instance.size == [64.0, 64.0]
            })
        );

        let mut corner = World::generate(1, WorldConfig::new(96, 100).unwrap());
        let corner_bounds = WorldRect {
            min: WorldPosition { x: 96, y: 64 },
            max: WorldPosition { x: 128, y: 128 },
        };
        corner.generate_area(corner_bounds).unwrap();
        let (corner_instances, _) = build_world_instances(
            &corner,
            WorldRect {
                min: WorldPosition { x: 0, y: 0 },
                max: corner_bounds.max,
            },
            64,
        );
        assert!(corner_instances.iter().any(|instance| {
            instance.position == [64.0, 64.0] && instance.size == [64.0, 64.0]
        }));

        let mut expanded = World::generate(1, WorldConfig::new(64, 64).unwrap());
        let generated_bounds = WorldRect {
            min: WorldPosition { x: 128, y: 0 },
            max: WorldPosition { x: 192, y: 64 },
        };
        expanded.generate_area(generated_bounds).unwrap();
        let (generated_instances, _) = build_world_instances(&expanded, generated_bounds, 4);
        assert!(generated_instances.iter().all(|instance| {
            instance.position[0] >= 128.0
                && instance.position[0] + instance.size[0] <= 192.0
                && instance.position[1] + instance.size[1] <= 64.0
        }));
        assert!(
            generated_instances
                .iter()
                .any(|instance| instance.position[0] == 188.0 && instance.size[0] == 4.0)
        );
    }

    #[test]
    fn unloaded_bootstrap_has_no_terrain_instances() {
        let world = World::new(1, WorldConfig::new(64, 64).unwrap());
        let (terrain, features) = build_world_instances(&world, world.initial_bounds(), 1);

        assert!(terrain.is_empty());
        assert!(features.is_empty());
    }

    #[test]
    fn cache_sync_rebuilds_streamed_visible_work_without_offscreen_uploads() {
        assert_eq!(
            cache_sync_action(true, true, true, false, true),
            CacheSyncAction::Skip
        );
        assert_eq!(
            cache_sync_action(true, true, true, true, true),
            CacheSyncAction::Rebuild
        );
        assert_eq!(
            cache_sync_action(true, true, true, false, false),
            CacheSyncAction::AdvanceRevision
        );
        assert_eq!(
            cache_sync_action(false, true, false, false, false),
            CacheSyncAction::Rebuild
        );
    }

    #[test]
    fn static_buffers_partition_at_the_device_safe_limit() {
        let instances = vec![Instance::zeroed(); MAX_INSTANCES_PER_BUFFER + 1];
        let lengths: Vec<_> = static_instance_chunks(&instances).map(<[_]>::len).collect();
        assert_eq!(lengths, [MAX_INSTANCES_PER_BUFFER, 1]);
    }

    #[test]
    fn invalid_selection_uses_red_preview() {
        assert_eq!(selection_color(true), rgba(255, 220, 35, 72));
        assert_eq!(selection_color(false), rgba(235, 48, 48, 96));
    }

    #[test]
    fn world_border_marks_the_centered_generation_envelope() {
        let border = world_border(1.0);

        assert_eq!(border[0].position, [-32_768.0, -32_768.0]);
        assert_eq!(border[0].size, [65_536.0, 2.0]);
        assert_eq!(border[1].position, [-32_768.0, 32_766.0]);
        assert_eq!(border[2].size, [2.0, 65_536.0]);
        assert_eq!(border[3].position, [32_766.0, -32_768.0]);
        assert!(
            border
                .iter()
                .all(|instance| instance.color == rgba(245, 40, 40, 230))
        );
    }

    #[test]
    fn chunk_outline_uses_signed_chunk_bounds() {
        let world = World::generate(1, WorldConfig::new(64, 64).unwrap());
        let inspection = world
            .inspect_chunk_at(WorldPosition { x: -1, y: 63 })
            .unwrap();
        let outline = chunk_outline(inspection, 1.0).unwrap();

        assert_eq!(outline[0].position, [-64.0, 0.0]);
        assert_eq!(outline[0].size, [64.0, 1.0]);
        assert_eq!(outline[1].position, [-64.0, 63.0]);
        assert_eq!(outline[2].position, [-64.0, 0.0]);
        assert_eq!(outline[3].position, [-1.0, 0.0]);
    }

    #[test]
    fn subpixel_chunks_do_not_create_inspection_overlays() {
        let world = World::generate(1, WorldConfig::new(64, 64).unwrap());
        let inspection = world
            .inspect_chunk_at(WorldPosition { x: 0, y: 0 })
            .unwrap();
        assert!(chunk_outline(inspection, 0.01).is_none());
        assert!(chunk_outline(inspection, 0.1).is_some());
    }

    #[test]
    fn hud_contains_simulation_and_loaded_cell_inspection_data() {
        let world = World::generate(7, WorldConfig::new(64, 64).unwrap());
        let state = test_render_state(Some(WorldPosition { x: 0, y: 0 }));
        let mut text = String::new();

        write_hud_text(&mut text, &world, &state);

        assert!(text.contains("RUNNING  SPEED 4X"));
        assert!(text.contains("SIM 0000:01:02.0  TICK 3721"));
        assert!(text.contains("SEED 7  LOADED "));
        assert!(text.contains("AGENTS ACTIVE  TOTAL 0  LIVING 0  ACTIVE 0  DEAD 0"));
        assert!(text.contains("  REV "));
        assert!(text.contains("CURSOR X 0  Y 0"));
        assert!(text.contains("CHUNK X 0 Y 0  LOCAL 0,0"));
        assert!(text.contains("COVERAGE "));
        assert!(text.contains("SURFACE "));
        assert!(text.contains("  BIOME "));
        assert!(text.contains("ELEV "));
        assert!(text.contains("TEMP "));
        assert!(text.contains("MOIST "));
        assert!(text.contains("WIND "));
        assert!(text.contains("FEATURE "));
    }

    #[test]
    fn hud_reports_unloaded_coverage_and_worker_failure_in_game() {
        let world = World::new(7, WorldConfig::new(96, 64).unwrap());
        let mut state = test_render_state(Some(WorldPosition { x: 47, y: 0 }));
        state.generation_status = GenerationStatus::WorkerUnavailable;
        let mut text = String::new();

        write_hud_text(&mut text, &world, &state);

        assert!(text.contains("GEN WORKER OFFLINE"));
        assert!(text.contains("COVERAGE PARTIAL INITIAL UNLOADED"));
        assert!(text.contains("CELL UNLOADED"));
    }

    #[test]
    fn every_hud_layout_fits_the_fixed_gpu_instance_budget() {
        let world = World::generate(u64::MAX, WorldConfig::new(64, 64).unwrap());
        let mut state = test_render_state(None);
        state.snapshot.tick = u64::MAX;
        state.snapshot.simulated_seconds = u64::MAX as f64 / 60.0;
        state.snapshot.seed = u64::MAX;
        state.snapshot.speed = 64.0;
        state.selection = Some(WORLD_GENERATION_BOUNDS);
        state.selection_valid = false;
        state.spawn_menu = Some(SpawnMenuView {
            selected: SpawnKind::BerryBush,
            placing: true,
        });
        let mut text = String::new();
        let mut instances = Vec::new();

        for cursor in [
            None,
            Some(WorldPosition { x: 0, y: 0 }),
            Some(WorldPosition {
                x: 32_768,
                y: 32_768,
            }),
        ] {
            state.cursor_world = cursor;
            for status in [
                GenerationStatus::Idle,
                GenerationStatus::Bootstrap,
                GenerationStatus::Manual,
                GenerationStatus::Cancelling,
                GenerationStatus::WorkerUnavailable,
            ] {
                state.generation_status = status;
                write_hud_text(&mut text, &world, &state);
                assert!(text.len() <= HUD_TEXT_CAPACITY);
                build_screen_overlay(&mut instances, &text, None, &state, 1_920, 1_080);
                assert!(instances.len() <= SCREEN_OVERLAY_CAPACITY);
            }
        }
    }

    #[test]
    fn hovered_agent_panel_reports_authoritative_physical_state() {
        let view = AgentView {
            id: sim_core::AgentId::new(7),
            position: WorldPosition { x: 12, y: -9 },
            activity: AgentActivity::Moving,
        };
        let level = sim_core::NeedLevelView {
            value: 1_234,
            rate_per_period: 6,
            threshold: 6_000,
            threshold_reached: false,
        };
        let inspection = AgentInspection {
            view,
            needs: Some(PhysicalNeedsView {
                agent: view.id,
                at: sim_core::SimTime::from_ticks(90),
                hunger: level,
                thirst: level,
                rest: level,
                exposure: level,
                next_threshold: Some(sim_core::NeedThreshold {
                    kind: NeedKind::Thirst,
                    due: sim_core::SimTime::from_ticks(1_000),
                }),
            }),
            inventory: Some(InventoryView {
                food: 2,
                wood: 3,
                stone: 4,
            }),
            health: Some(HealthView {
                agent: view.id,
                value: 9_000,
                status: HealthStatus::Healthy,
                next_consequence: None,
            }),
            policy: Some(PhysicalPolicyView {
                agent: view.id,
                goal: PhysicalGoal::Explore,
                reason: PolicyReason::NoUrgentNeed,
                target: Some(WorldPosition { x: 20, y: -4 }),
                committed: true,
                retry_count: 1,
                exploration_heading: ExplorationHeading::NorthEast,
            }),
            sleep: None,
            death: None,
        };
        let mut text = String::with_capacity(AGENT_TEXT_CAPACITY);
        write_agent_text(&mut text, Some(inspection));
        assert!(text.contains("AGENT 7"));
        assert!(text.contains("ACTIVITY MOVING"));
        assert!(text.contains("GOAL EXPLORE"));
        assert!(text.contains("WHY NO URGENT NEED"));
        assert!(text.contains("STATUS COMMITTED  RETRIES 1"));
        assert!(text.contains("THIRST 1234 OF 6000  RATE +6"));
        assert!(text.contains("INVENTORY F 2  W 3  S 4"));
        assert!(text.contains("HEALTH 9000  HEALTHY"));
        assert!(text.contains("SLEEP NONE"));
        assert!(text.len() <= AGENT_TEXT_CAPACITY);

        let mut backoff = inspection;
        backoff.policy = Some(PhysicalPolicyView {
            goal: PhysicalGoal::SeekWater,
            reason: PolicyReason::Retry,
            target: None,
            committed: false,
            retry_count: u8::MAX,
            ..backoff.policy.unwrap()
        });
        write_agent_text(&mut text, Some(backoff));
        assert!(text.contains("GOAL SEEK WATER"));
        assert!(text.contains("WHY RETRY"));
        assert!(text.contains("STATUS BACKOFF  RETRIES 255"));

        let budget_view = AgentView {
            id: sim_core::AgentId::new(u32::MAX),
            position: WorldPosition {
                x: -32_768,
                y: 32_767,
            },
            activity: AgentActivity::Incapacitated,
        };
        let budget_level = sim_core::NeedLevelView {
            value: 10_000,
            rate_per_period: -12,
            threshold: 10_000,
            threshold_reached: true,
        };
        let budget_inspection = AgentInspection {
            view: budget_view,
            needs: Some(PhysicalNeedsView {
                agent: budget_view.id,
                at: sim_core::SimTime::from_ticks(u64::MAX),
                hunger: budget_level,
                thirst: budget_level,
                rest: budget_level,
                exposure: budget_level,
                next_threshold: Some(sim_core::NeedThreshold {
                    kind: NeedKind::Exposure,
                    due: sim_core::SimTime::from_ticks(u64::MAX),
                }),
            }),
            inventory: Some(InventoryView {
                food: u8::MAX,
                wood: u8::MAX,
                stone: u8::MAX,
            }),
            health: Some(HealthView {
                agent: budget_view.id,
                value: 10_000,
                status: HealthStatus::Incapacitated,
                next_consequence: Some(sim_core::SimTime::from_ticks(u64::MAX)),
            }),
            policy: Some(PhysicalPolicyView {
                agent: budget_view.id,
                goal: PhysicalGoal::Incapacitated,
                reason: PolicyReason::ExposureThreshold,
                target: Some(WorldPosition {
                    x: -32_768,
                    y: 32_767,
                }),
                committed: true,
                retry_count: u8::MAX,
                exploration_heading: ExplorationHeading::SouthWest,
            }),
            sleep: Some(SleepView {
                agent: budget_view.id,
                position: budget_view.position,
                started_at: sim_core::SimTime::from_ticks(u64::MAX),
                planned_wake: sim_core::SimTime::from_ticks(u64::MAX),
                quality: SleepQuality::OpenGround,
            }),
            death: Some(DeathRecord {
                agent: budget_view.id,
                cause: DeathCause::Exhaustion,
                at: sim_core::SimTime::from_ticks(u64::MAX),
                position: budget_view.position,
            }),
        };
        let mut budget_text = String::with_capacity(AGENT_TEXT_CAPACITY);
        write_agent_text(&mut budget_text, Some(budget_inspection));
        assert!(budget_text.contains("WHY EXPOSURE THRESHOLD"));
        assert!(budget_text.contains("DEATH CAUSE EXHAUSTION"));
        assert!(budget_text.contains("DIED AT TICK 18446744073709551615"));
        assert!(budget_text.len() <= AGENT_TEXT_CAPACITY);

        let world = World::generate(u64::MAX, WorldConfig::new(64, 64).unwrap());
        let mut state = test_render_state(Some(view.position));
        state.snapshot.tick = u64::MAX;
        state.snapshot.simulated_seconds = u64::MAX as f64 / 60.0;
        state.snapshot.seed = u64::MAX;
        state.snapshot.speed = 256.0;
        state.selection = Some(WORLD_GENERATION_BOUNDS);
        state.selection_valid = false;
        state.generation_status = GenerationStatus::WorkerUnavailable;
        state.spawn_message =
            Some("SPAWN FAILED - CELL IS OCCUPIED BY AGENT 4294967295".to_owned());
        let mut hud_text = String::with_capacity(HUD_TEXT_CAPACITY);
        write_hud_text(&mut hud_text, &world, &state);
        let mut instances = Vec::with_capacity(SCREEN_OVERLAY_CAPACITY);
        build_screen_overlay(
            &mut instances,
            &hud_text,
            Some(&budget_text),
            &state,
            1_920,
            1_080,
        );
        assert!(
            instances.len() > 4_096,
            "the regression layout must exercise the former undersized budget"
        );
        assert!(instances.len() <= SCREEN_OVERLAY_CAPACITY);
    }

    #[test]
    fn agent_instances_reflect_position_activity_and_bounded_far_zoom_culling() {
        let bounds = WorldRect {
            min: WorldPosition { x: -4, y: -4 },
            max: WorldPosition { x: 4, y: 4 },
        };
        let activities = [
            AgentActivity::Idle,
            AgentActivity::Moving,
            AgentActivity::Gathering,
            AgentActivity::Building,
            AgentActivity::Sleeping,
            AgentActivity::Incapacitated,
            AgentActivity::Dead,
        ];
        let views = activities
            .into_iter()
            .enumerate()
            .map(|(index, activity)| AgentView {
                id: sim_core::AgentId::new(index as u32),
                position: WorldPosition {
                    x: index as i64 - 3,
                    y: 0,
                },
                activity,
            });
        let mut instances = Vec::new();
        build_agent_instances(views, bounds, 2.0, &mut instances);
        assert_eq!(instances.len(), activities.len());
        assert_eq!(instances[0].color, agent_color(AgentActivity::Idle));
        assert_eq!(
            instances[5].color,
            agent_color(AgentActivity::Incapacitated)
        );
        assert_eq!(instances[6].color, agent_color(AgentActivity::Dead));
        assert_ne!(instances[5].color, instances[6].color);
        assert_eq!(instances[0].position, [-2.86, 0.14]);

        build_agent_instances(
            std::iter::repeat_n(
                AgentView {
                    id: sim_core::AgentId::new(0),
                    position: WorldPosition { x: 0, y: 0 },
                    activity: AgentActivity::Moving,
                },
                MAX_AGENT_INSTANCES + 100,
            ),
            bounds,
            2.0,
            &mut instances,
        );
        assert_eq!(instances.len(), MAX_AGENT_INSTANCES);
        build_agent_instances(std::iter::empty(), bounds, 0.5, &mut instances);
        assert!(instances.is_empty());
    }

    #[test]
    fn spawned_object_instances_use_kind_geometry_culling_and_capacity() {
        let bounds = WorldRect {
            min: WorldPosition { x: -2, y: -2 },
            max: WorldPosition { x: 3, y: 3 },
        };
        let views = SpawnKind::ALL
            .into_iter()
            .enumerate()
            .map(|(index, kind)| SpawnedObjectView {
                position: WorldPosition {
                    x: index as i64 - 1,
                    y: 0,
                },
                kind,
                remaining: kind.resource().map(|resource| resource.capacity),
            });
        let mut instances = Vec::new();
        build_spawned_object_instances(views, bounds, 2.0, &mut instances);
        assert_eq!(instances.len(), 4);
        assert_eq!(instances[0].color, spawn_kind_color(SpawnKind::Tree));
        assert_eq!(instances[3].size, [1.0, 1.0]);

        let repeated = std::iter::repeat_n(
            SpawnedObjectView {
                position: WorldPosition { x: 0, y: 0 },
                kind: SpawnKind::Rock,
                remaining: Some(80),
            },
            MAX_SPAWNED_OBJECT_INSTANCES + 1,
        );
        build_spawned_object_instances(repeated, bounds, 2.0, &mut instances);
        assert_eq!(instances.len(), MAX_SPAWNED_OBJECT_INSTANCES);
        build_spawned_object_instances(std::iter::empty(), bounds, 0.5, &mut instances);
        assert!(instances.is_empty());
    }

    #[test]
    fn dynamic_agents_do_not_enter_the_immutable_terrain_cache_key() {
        let before = AgentView {
            id: sim_core::AgentId::new(0),
            position: WorldPosition { x: 0, y: 0 },
            activity: AgentActivity::Idle,
        };
        let after = AgentView {
            position: WorldPosition { x: 1, y: 0 },
            activity: AgentActivity::Moving,
            ..before
        };
        assert_ne!(before, after);
        assert_eq!(
            cache_sync_action(true, true, false, false, false),
            CacheSyncAction::Skip
        );
    }

    #[test]
    fn shelter_lifecycle_states_have_distinct_footprint_colors() {
        assert_ne!(
            structure_color(StructureState::UnderConstruction),
            structure_color(StructureState::Complete)
        );
    }
}
