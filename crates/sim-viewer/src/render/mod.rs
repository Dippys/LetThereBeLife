//! GPU world renderer: public render-state view types and the `Renderer` that syncs caches, builds instances, and draws a frame.

mod colors;
mod gpu;
mod hud;
mod instances;
mod overlay;
mod summary;

#[cfg(test)]
mod tests;

use std::{borrow::Cow, sync::Arc, time::Instant};

use sim_core::{
    ACQUAINTANCE_SLOTS, AcquaintanceView, AgentId, AgentView, ChunkInspection, Concept,
    DeathRecord, Engine, HealthView, InventoryView, LANDMARK_SLOTS, LEXICON_SLOTS, LandmarkKind,
    LandmarkSource, LandmarkView, LexiconEntryView, Material, MentalMapView, Personality,
    PhysicalNeedsView, PhysicalPolicyView, SimulationSnapshot, SleepView, SpawnKind,
    SpawnedObjectView, Species, VocalForm, World, WorldOverview, WorldPosition, WorldRect,
};
use winit::window::Window;

use crate::{
    camera::Camera,
    gestures::{GestureLog, GestureSummary, RECENT_GESTURE_CAPACITY},
};
use colors::{rgba, selection_color};
use gpu::{CameraBinding, CameraUniform, Instance, InstanceBuffer, StaticInstanceBuffers};
use hud::{write_agent_text, write_hud_text};
use instances::{
    append_wildlife_instances, build_agent_instances, build_gesture_instances,
    build_memory_marker_instances, build_relationship_marker_instances,
    build_spawned_object_instances, build_structure_instances, chunk_outline, world_border,
};
use overlay::build_screen_overlay;
use summary::{
    CacheSyncAction, WorldSummaryCache, cache_margin, cache_sync_action, terrain_sample_step,
};

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
    pub gestures: GestureSummary,
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
    pub memory: Option<MemoryInspection>,
}

/// A bounded, copyable snapshot of one agent's mind (places, personality,
/// acquaintances, and lexicon) for the hover card and map markers.
#[derive(Debug, Clone, Copy)]
pub struct MemoryInspection {
    places: [LandmarkView; LANDMARK_SLOTS],
    len: u8,
    pub explored_tiles: usize,
    /// Born into the band with no words (rather than a founder).
    pub child: bool,
    pub personality: Personality,
    acquaintances: [AcquaintanceView; ACQUAINTANCE_SLOTS],
    acquaintance_len: u8,
    lexicon: [LexiconEntryView; LEXICON_SLOTS],
    lexicon_len: u8,
    /// Believed food value per material (feeds minus twice sickens; `None` = no idea).
    pub food: [Option<i32>; Material::COUNT],
    /// Per species: (believed worth hunting, believed dangerous), if it has a belief.
    pub fauna: [Option<(bool, bool)>; Species::COUNT],
}

impl MemoryInspection {
    pub fn from_view(view: &MentalMapView) -> Self {
        const EMPTY: LandmarkView = LandmarkView {
            kind: LandmarkKind::Water,
            position: WorldPosition { x: 0, y: 0 },
            source: LandmarkSource::Seen,
            confidence: 0,
            search_radius: 0,
            seen_second: 0,
        };
        const STRANGER: AcquaintanceView = AcquaintanceView {
            agent: AgentId::new(0),
            familiarity: 0,
            trust: 0,
            last_seen_position: None,
            last_seen_second: 0,
        };
        const UNHEARD: LexiconEntryView = LexiconEntryView {
            form: VocalForm(0),
            concept: Concept::Water,
            positive: 0,
            contradictory: 0,
            heard: 0,
            successes: 0,
            failures: 0,
        };
        let mut places = [EMPTY; LANDMARK_SLOTS];
        let len = view.landmarks.len().min(LANDMARK_SLOTS);
        places[..len].copy_from_slice(&view.landmarks[..len]);
        let mut acquaintances = [STRANGER; ACQUAINTANCE_SLOTS];
        let acquaintance_len = view.acquaintances.len().min(ACQUAINTANCE_SLOTS);
        acquaintances[..acquaintance_len].copy_from_slice(&view.acquaintances[..acquaintance_len]);
        let mut lexicon = [UNHEARD; LEXICON_SLOTS];
        let lexicon_len = view.lexicon.len().min(LEXICON_SLOTS);
        lexicon[..lexicon_len].copy_from_slice(&view.lexicon[..lexicon_len]);
        Self {
            places,
            len: len as u8,
            explored_tiles: view.explored_tiles,
            child: view.child,
            personality: view.personality,
            acquaintances,
            acquaintance_len: acquaintance_len as u8,
            lexicon,
            lexicon_len: lexicon_len as u8,
            food: Material::ALL.map(|material| {
                view.affordances
                    .iter()
                    .find(|belief| belief.material == material)
                    .map(|belief| i32::from(belief.feeds) - 2 * i32::from(belief.sickens))
            }),
            fauna: Species::ALL.map(|species| {
                view.fauna
                    .iter()
                    .find(|belief| belief.species == species)
                    .map(|belief| {
                        (
                            belief.prey > 64 && belief.prey > belief.danger,
                            belief.danger > 64,
                        )
                    })
            }),
        }
    }

    pub fn places(&self) -> &[LandmarkView] {
        &self.places[..usize::from(self.len)]
    }

    pub fn acquaintances(&self) -> &[AcquaintanceView] {
        &self.acquaintances[..usize::from(self.acquaintance_len)]
    }

    pub fn lexicon(&self) -> &[LexiconEntryView] {
        &self.lexicon[..usize::from(self.lexicon_len)]
    }

    /// The agent's word for `concept`: the entry with the most net evidence
    /// (`positive - contradictory`), ties going to more positive evidence and
    /// then the lowest form id. `None` unless some entry has net evidence above zero.
    pub fn word_for(&self, concept: Concept) -> Option<VocalForm> {
        self.lexicon()
            .iter()
            .filter(|entry| entry.concept == concept && entry.positive > entry.contradictory)
            .max_by_key(|entry| {
                (
                    i32::from(entry.positive) - i32::from(entry.contradictory),
                    entry.positive,
                    std::cmp::Reverse(entry.form.0),
                )
            })
            .map(|entry| entry.form)
    }
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
    memory_markers: InstanceBuffer,
    memory_marker_instances: Vec<Instance>,
    relationship_markers: InstanceBuffer,
    relationship_marker_instances: Vec<Instance>,
    gesture_markers: InstanceBuffer,
    gesture_marker_instances: Vec<Instance>,
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
    overview: Option<WorldOverview>,
}

impl Renderer {
    pub fn new(
        window: Arc<Window>,
        world: &World,
        overview: Option<WorldOverview>,
    ) -> Result<Self, String> {
        pollster::block_on(Self::new_async(window, world, overview))
    }

    async fn new_async(
        window: Arc<Window>,
        world: &World,
        overview: Option<WorldOverview>,
    ) -> Result<Self, String> {
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
            memory_markers: InstanceBuffer::dynamic(
                &device,
                "memory marker instances",
                MAX_MEMORY_MARKER_INSTANCES,
            ),
            memory_marker_instances: Vec::with_capacity(MAX_MEMORY_MARKER_INSTANCES),
            relationship_markers: InstanceBuffer::dynamic(
                &device,
                "relationship marker instances",
                MAX_RELATIONSHIP_MARKER_INSTANCES,
            ),
            relationship_marker_instances: Vec::with_capacity(MAX_RELATIONSHIP_MARKER_INSTANCES),
            gesture_markers: InstanceBuffer::dynamic(
                &device,
                "gesture marker instances",
                MAX_GESTURE_MARKER_INSTANCES,
            ),
            gesture_marker_instances: Vec::with_capacity(MAX_GESTURE_MARKER_INSTANCES),
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
            overview,
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
            self.overview.as_ref(),
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
        gestures: &GestureLog,
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
        append_wildlife_instances(
            engine.animal_views(),
            engine.carcass_views(),
            view.world_bounds(),
            view.scale() as f32,
            &mut self.spawned_object_instances,
        );
        self.spawned_object_instances
            .truncate(MAX_SPAWNED_OBJECT_INSTANCES);
        self.spawned_objects
            .write(&self.queue, &self.spawned_object_instances);
        let hovered_memory = state
            .hovered_agent
            .as_ref()
            .and_then(|agent| Some((agent.view.position, agent.memory.as_ref()?)));
        build_memory_marker_instances(
            hovered_memory.map_or(&[], |(_, memory)| memory.places()),
            view.scale() as f32,
            &mut self.memory_marker_instances,
        );
        debug_assert!(self.memory_marker_instances.len() <= MAX_MEMORY_MARKER_INSTANCES);
        self.memory_markers
            .write(&self.queue, &self.memory_marker_instances);
        match hovered_memory {
            Some((origin, memory)) => build_relationship_marker_instances(
                origin,
                memory.acquaintances(),
                view.scale() as f32,
                &mut self.relationship_marker_instances,
            ),
            None => self.relationship_marker_instances.clear(),
        }
        debug_assert!(
            self.relationship_marker_instances.len() <= MAX_RELATIONSHIP_MARKER_INSTANCES
        );
        self.relationship_markers
            .write(&self.queue, &self.relationship_marker_instances);

        build_gesture_instances(
            gestures.recent(),
            view.scale() as f32,
            &mut self.gesture_marker_instances,
        );
        debug_assert!(self.gesture_marker_instances.len() <= MAX_GESTURE_MARKER_INSTANCES);
        self.gesture_markers
            .write(&self.queue, &self.gesture_marker_instances);

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
            self.memory_markers.draw(&mut pass);
            self.relationship_markers.draw(&mut pass);
            self.gesture_markers.draw(&mut pass);
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

const MAX_INSTANCES_PER_BUFFER: usize = 1_000_000;
const MIN_TERRAIN_SAMPLE_PIXELS: f32 = 2.0;
const CACHE_MARGIN_PIXELS: f32 = 128.0;
const WORLD_OVERLAY_CAPACITY: usize = 10;
/// The worst-case composed card measures 10_553 instances: the personality and
/// friend lines took it to 9_195 (budget 10_240), then the two `WORDS` card
/// lines and the spoken word and mime on the HUD gesture line overflowed 10_240.
/// 11_520 keeps the ~900 instances of slack the budget has carried.
const SCREEN_OVERLAY_CAPACITY: usize = 11_520;
const HUD_TEXT_CAPACITY: usize = 640;
/// The worst-case card is 800 bytes: 733 before the two `WORDS` lines (67 bytes)
/// were added, which overflowed the former 768.
const AGENT_TEXT_CAPACITY: usize = 832;
const MAX_AGENT_INSTANCES: usize = 4_096;
const MAX_STRUCTURE_INSTANCES: usize = 4_096;
const MAX_SPAWNED_OBJECT_INSTANCES: usize = 16_384;
const MAX_MEMORY_MARKER_INSTANCES: usize = LANDMARK_SLOTS * 4;
/// Dots drawn along one acquaintance line, at most.
const MAX_RELATIONSHIP_DOTS: usize = 16;
/// Each acquaintance draws one end marker plus its dotted line.
const MAX_RELATIONSHIP_MARKER_INSTANCES: usize = ACQUAINTANCE_SLOTS * (MAX_RELATIONSHIP_DOTS + 1);
/// Dots drawn along one gesture's pointing line, at most.
const MAX_GESTURE_DOTS: usize = 16;
/// Each recent gesture draws its dotted line, a 4-sided search square, and a topic dot.
const MAX_GESTURE_MARKER_INSTANCES: usize = RECENT_GESTURE_CAPACITY * (MAX_GESTURE_DOTS + 5);
const MIN_DYNAMIC_INSTANCE_PIXELS: f32 = 1.25;
const MIN_CHUNK_OUTLINE_PIXELS: f32 = 4.0;
const MAX_CHUNK_OUTLINE_WORLD_WIDTH: f32 = 8.0;
const MAX_WORLD_BORDER_WIDTH: f32 = 32.0;
