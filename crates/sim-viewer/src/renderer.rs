use std::{borrow::Cow, sync::Arc};

use bytemuck::{Pod, Zeroable};
use sim_core::{
    CHUNK_SIZE, ChunkInspection, ChunkPresence, FeatureKind, GroundType, SimulationSnapshot,
    WORLD_GENERATION_BOUNDS, World, WorldPosition, WorldRect,
};
use wgpu::util::DeviceExt;
use winit::window::Window;

use crate::camera::Camera;

pub struct RenderState {
    pub snapshot: SimulationSnapshot,
    pub camera: Camera,
    pub inspected: Option<ChunkInspection>,
    pub hovered: Option<WorldPosition>,
    pub selection: Option<WorldRect>,
    pub selection_valid: bool,
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
            screen_overlay: InstanceBuffer::dynamic(&device, "screen overlay", 4),
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

        let pulse = ((state.snapshot.tick / 12) % 80) as f32;
        let marker_x = ((state.snapshot.tick / 2)
            % u64::from(self.config.width.saturating_sub(32).max(1))) as f32;
        let screen_overlay = [
            Instance::new(28.0, 28.0, 260.0, 92.0, rgba(9, 16, 20, 255)),
            Instance::new(
                44.0,
                48.0,
                12.0 + pulse,
                12.0,
                if state.snapshot.paused {
                    rgba(210, 150, 55, 255)
                } else {
                    rgba(75, 205, 125, 255)
                },
            ),
            Instance::new(
                44.0,
                72.0,
                state.snapshot.speed * 24.0,
                8.0,
                rgba(78, 145, 220, 255),
            ),
            Instance::new(
                marker_x,
                self.config.height.saturating_sub(28) as f32,
                16.0,
                16.0,
                rgba(235, 216, 130, 255),
            ),
        ];
        self.screen_overlay.write(&self.queue, &screen_overlay);

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
    match cell.ground {
        GroundType::DeepWater => rgba(16, 48 + shade, 94 + shade, 255),
        GroundType::ShallowWater => rgba(28, 84 + shade, 126 + shade, 255),
        GroundType::Sand => rgba(184 + shade, 166 + shade, 105, 255),
        GroundType::Grass => rgba(50 + shade, 112 + shade, 51, 255),
        GroundType::ForestFloor => rgba(37, 86 + shade, 39, 255),
        GroundType::Hill => rgba(100 + shade, 108 + shade, 72, 255),
        GroundType::BareRock => rgba(125 + shade, 124 + shade, 119 + shade, 255),
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
}
