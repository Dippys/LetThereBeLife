use std::{borrow::Cow, fmt::Write, sync::Arc};

use bytemuck::{Pod, Zeroable};
use sim_core::{
    BiomeType, CHUNK_SIZE, ChunkInspection, ChunkPresence, FeatureKind, GenerateAreaError,
    PrevailingWind, SimulationSnapshot, SurfaceType, WORLD_GENERATION_BOUNDS, World, WorldPosition,
    WorldRect,
};
use wgpu::util::DeviceExt;
use winit::window::Window;

use crate::camera::Camera;

pub struct RenderState {
    pub snapshot: SimulationSnapshot,
    pub camera: Camera,
    pub ui_scale: f32,
    pub cursor_world: Option<WorldPosition>,
    pub inspected: Option<ChunkInspection>,
    pub hovered: Option<WorldPosition>,
    pub selection: Option<WorldRect>,
    pub selection_valid: bool,
    pub generation_status: GenerationStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenerationStatus {
    Idle,
    Bootstrap,
    Manual,
    Cancelling,
    WorkerUnavailable,
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
    world_overlay: InstanceBuffer,
    world_overlay_instances: Vec<Instance>,
    screen_overlay: InstanceBuffer,
    screen_overlay_instances: Vec<Instance>,
    hud_text: String,
    world_revision: u64,
    cached_bounds: Option<WorldRect>,
    cached_step: u32,
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
            world_revision: world.revision(),
            cached_bounds: None,
            cached_step: 1,
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
        let (terrain, features) = build_world_instances(world, cached, step);
        self.terrain = StaticInstanceBuffers::new(&self.device, "terrain instances", &terrain);
        self.features = StaticInstanceBuffers::new(&self.device, "feature instances", &features);
        self.world_revision = world.revision();
        self.cached_bounds = Some(cached);
        self.cached_step = step;
    }

    pub fn render(
        &mut self,
        world: &World,
        state: RenderState,
        allow_world_sync: bool,
        changed_bounds: Option<WorldRect>,
    ) -> Result<(), wgpu::SurfaceError> {
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
        build_screen_overlay(
            &mut self.screen_overlay_instances,
            &self.hud_text,
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
#[derive(Clone, Copy, Pod, Zeroable)]
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
const SCREEN_OVERLAY_CAPACITY: usize = 4_096;
const HUD_TEXT_CAPACITY: usize = 512;
const MIN_CHUNK_OUTLINE_PIXELS: f32 = 4.0;
const MAX_CHUNK_OUTLINE_WORLD_WIDTH: f32 = 8.0;
const MAX_WORLD_BORDER_WIDTH: f32 = 32.0;

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

fn build_world_instances(
    world: &World,
    bounds: WorldRect,
    step: u32,
) -> (Vec<Instance>, Vec<Instance>) {
    let mut terrain = Vec::new();
    world.visit_cells_in_step(bounds, step, |position, cell| {
        let size = terrain_block_size(world, position, step);
        terrain.push(Instance::new(
            position.x as f32,
            position.y as f32,
            size[0],
            size[1],
            terrain_color(cell),
        ));
    });
    let mut features = Vec::new();
    world.visit_features_in(bounds, |feature| {
        if feature.position.x.rem_euclid(i64::from(step)) != 0
            || feature.position.y.rem_euclid(i64::from(step)) != 0
        {
            return;
        }
        let color = match feature.kind {
            FeatureKind::Tree => rgba(24, 72, 28, 255),
            FeatureKind::Rock => rgba(118, 116, 108, 255),
            FeatureKind::BerryBush => rgba(112, 42, 74, 255),
        };
        features.push(Instance::new(
            feature.position.x as f32,
            feature.position.y as f32,
            1.0,
            1.0,
            color,
        ));
    });
    (terrain, features)
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

fn terrain_block_size(world: &World, position: WorldPosition, step: u32) -> [f32; 2] {
    let limit = world
        .loaded_bounds_at(position)
        .expect("visited terrain cells must have loaded coverage")
        .max;
    let step = i64::from(step);
    [
        (limit.x - position.x).min(step) as f32,
        (limit.y - position.y).min(step) as f32,
    ]
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
        write!(output, "SPACE PAUSE  1-4 SPEED  C CANCEL").expect("writing to String cannot fail");
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
                    write!(
                        output,
                        "WIND {}  FEATURE {}",
                        wind_label(climate.wind),
                        world
                            .feature_at(position)
                            .map_or("NONE", |feature| feature_label(feature.kind))
                    )
                    .expect("writing to String cannot fail");
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

const fn generation_label(status: GenerationStatus) -> &'static str {
    match status {
        GenerationStatus::Idle => "READY",
        GenerationStatus::Bootstrap => "LOADING WORLD",
        GenerationStatus::Manual => "GENERATING SELECTION",
        GenerationStatus::Cancelling => "CANCELLING",
        GenerationStatus::WorkerUnavailable => "WORKER OFFLINE",
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

fn build_screen_overlay(
    instances: &mut Vec<Instance>,
    text: &str,
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
            },
            camera: Camera::at_origin(),
            ui_scale: 1.0,
            cursor_world,
            inspected: None,
            hovered: None,
            selection: None,
            selection_valid: true,
            generation_status: GenerationStatus::Idle,
        }
    }

    #[test]
    fn coarse_view_reduces_terrain_instances() {
        let world = World::generate(1, WorldConfig::new(64, 64).unwrap());
        let bounds = world.initial_bounds();
        let (full, _) = build_world_instances(&world, bounds, 1);
        let (coarse, _) = build_world_instances(&world, bounds, 4);

        assert_eq!(full.len(), 4_096);
        assert_eq!(coarse.len(), 256);
        assert!(coarse.iter().all(|instance| instance.size == [4.0, 4.0]));
    }

    #[test]
    fn sample_step_targets_two_pixel_blocks_and_chunk_divisors() {
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
        let (generated_instances, _) = build_world_instances(&expanded, generated_bounds, 3);
        assert!(generated_instances.iter().all(|instance| {
            instance.position[0] >= 128.0
                && instance.position[0] + instance.size[0] <= 192.0
                && instance.position[1] + instance.size[1] <= 64.0
        }));
        assert!(
            generated_instances
                .iter()
                .any(|instance| instance.position[0] == 191.0 && instance.size[0] == 1.0)
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
                build_screen_overlay(&mut instances, &text, &state, 1_920, 1_080);
                assert!(instances.len() <= SCREEN_OVERLAY_CAPACITY);
            }
        }
    }
}
