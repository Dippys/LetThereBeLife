use std::{borrow::Cow, sync::Arc};

use bytemuck::{Pod, Zeroable};
use sim_core::{FeatureKind, GroundType, SimulationSnapshot, World, WorldPosition, WorldRect};
use wgpu::util::DeviceExt;
use winit::window::Window;

use crate::camera::Camera;

pub struct RenderState {
    pub snapshot: SimulationSnapshot,
    pub camera: Camera,
    pub hovered: Option<WorldPosition>,
    pub selection: Option<WorldRect>,
}

pub struct Renderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    pipeline: wgpu::RenderPipeline,
    world_camera: CameraBinding,
    screen_camera: CameraBinding,
    terrain: InstanceBuffer,
    features: InstanceBuffer,
    world_overlay: InstanceBuffer,
    screen_overlay: InstanceBuffer,
    world_revision: u64,
    cached_bounds: Option<WorldRect>,
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

        let initial_bounds = WorldRect {
            min: WorldPosition { x: 0, y: 0 },
            max: WorldPosition {
                x: i64::from(world.width()),
                y: i64::from(world.height()),
            },
        };
        let (terrain, features) = build_world_instances(world, initial_bounds);
        Ok(Self {
            surface,
            terrain: InstanceBuffer::immutable(&device, "terrain instances", &terrain),
            features: InstanceBuffer::immutable(&device, "feature instances", &features),
            world_overlay: InstanceBuffer::dynamic(&device, "world overlay", 2),
            screen_overlay: InstanceBuffer::dynamic(&device, "screen overlay", 4),
            world_revision: world.revision(),
            cached_bounds: Some(initial_bounds),
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

    fn sync_view(&mut self, world: &World, requested: WorldRect) {
        if self.world_revision == world.revision()
            && self
                .cached_bounds
                .is_some_and(|cached| cached.contains_rect(requested))
        {
            return;
        }
        let cached = requested.expanded(128);
        let (terrain, features) = build_world_instances(world, cached);
        self.terrain = InstanceBuffer::immutable(&self.device, "terrain instances", &terrain);
        self.features = InstanceBuffer::immutable(&self.device, "feature instances", &features);
        self.world_revision = world.revision();
        self.cached_bounds = Some(cached);
    }

    pub fn render(&mut self, world: &World, state: RenderState) -> Result<(), wgpu::SurfaceError> {
        let view = state.camera.view(
            self.config.width,
            self.config.height,
            world.width(),
            world.height(),
        );
        self.sync_view(world, view.world_bounds());
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

        let mut world_overlay = Vec::with_capacity(2);
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
                rgba(255, 220, 35, 72),
            ));
        }
        self.world_overlay.write(&self.queue, &world_overlay);

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

fn build_world_instances(world: &World, bounds: WorldRect) -> (Vec<Instance>, Vec<Instance>) {
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

const fn rgba(red: u8, green: u8, blue: u8, alpha: u8) -> u32 {
    red as u32 | (green as u32) << 8 | (blue as u32) << 16 | (alpha as u32) << 24
}
