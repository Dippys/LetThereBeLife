//! GPU world renderer: public render-state view types and the `Renderer` that syncs caches, builds instances, and draws a frame.

mod colors;
mod details;
mod gpu;
mod instances;
mod summary;
mod text;
mod ui;

#[cfg(test)]
mod tests;

use std::{borrow::Cow, path::PathBuf, sync::Arc, time::Instant};

use sim_core::{
    ACQUAINTANCE_SLOTS, AcquaintanceView, AgentActivity, AgentId, AgentView, AnimalMode,
    ChunkInspection, Concept, DeathRecord, Engine, HealthView, InventoryView, LANDMARK_SLOTS,
    LEXICON_SLOTS, LandmarkKind, LandmarkSource, LandmarkView, LexiconEntryView, Material,
    MentalMapView, Personality, PhysicalNeedsView, PhysicalPolicyView, SimulationSnapshot,
    SleepView, SpawnKind, Species, StructureKind, StructureState, VocalForm, World, WorldOverview,
    WorldPosition, WorldRect,
};
use winit::window::Window;

use crate::{
    camera::Camera,
    feed::FeedEntry,
    gestures::{GestureLog, GestureMark, RECENT_GESTURE_CAPACITY},
};
use colors::{rgba, selection_color};
use details::write_details;
use gpu::{CameraBinding, CameraUniform, Instance, InstanceBuffer, StaticInstanceBuffers};
use instances::{
    append_wildlife_instances, build_agent_instances, build_gesture_instances,
    build_memory_marker_instances, build_relationship_marker_instances,
    build_spawned_object_instances, build_structure_instances, chunk_outline, world_border,
};
use summary::{
    CacheSyncAction, WorldSummaryCache, cache_margin, cache_sync_action, terrain_sample_step,
};
use ui::{Hit, build_interface};

pub struct RenderState {
    pub snapshot: SimulationSnapshot,
    pub camera: Camera,
    pub ui_scale: f32,
    /// Mouse position in window pixels.
    pub cursor: Option<(f64, f64)>,
    pub cursor_world: Option<WorldPosition>,
    pub inspected: Option<ChunkInspection>,
    pub hovered: Option<WorldPosition>,
    pub selection: Option<WorldRect>,
    pub selection_valid: bool,
    pub generation_status: GenerationStatus,
    pub population_status: PopulationStatus,
    /// What is under the mouse, for the tooltip (`None` over the interface).
    pub hover: Option<Hover>,
    pub selected: Option<AgentInspection>,
    pub following: bool,
    pub toast: Option<String>,
    pub help_open: bool,
    pub details_open: bool,
    /// The build palette's chosen tool while the palette is open.
    pub build: Option<BuildTool>,
    pub feed: Vec<FeedEntry>,
    pub census: Census,
}

/// Head counts for the top bar.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Census {
    pub people: u32,
    pub dead: u32,
    pub deer: u32,
    pub wolves: u32,
}

/// The thing under the mouse, described in the tooltip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hover {
    Person {
        id: AgentId,
        name: Option<sim_core::Name>,
        activity: AgentActivity,
    },
    Animal {
        species: Species,
        mode: AnimalMode,
    },
    Carcass {
        meat: u16,
    },
    Structure {
        kind: StructureKind,
        state: StructureState,
    },
    Resource {
        label: &'static str,
        remaining: Option<(u16, Material)>,
    },
    Terrain(&'static str),
}

/// What a click on the map places while the build palette is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildTool {
    Person,
    Object(SpawnKind),
}

impl BuildTool {
    pub const ALL: [Self; 5] = [
        Self::Person,
        Self::Object(SpawnKind::Tree),
        Self::Object(SpawnKind::BerryBush),
        Self::Object(SpawnKind::Rock),
        Self::Object(SpawnKind::Water),
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Person => "Person",
            Self::Object(kind) => crate::labels::spawn_kind(kind),
        }
    }
}

/// Something a click on the interface asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiAction {
    TogglePause,
    Slower,
    Faster,
    Help,
    CloseSelected,
    Follow,
    NextPerson,
    Tool(BuildTool),
    /// The feed entry at this index (oldest first).
    FeedEntry(usize),
}

/// What the interface has at a screen point.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiHit {
    Action(UiAction),
    /// A panel that keeps the click from reaching the map.
    Panel,
}

pub use ui::SPEEDS;

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
    pub life: Option<sim_core::LifeView>,
    pub motherhood: Option<sim_core::MotherhoodView>,
}

/// A bounded, copyable snapshot of one agent's mind (places, personality,
/// acquaintances, and lexicon) for the hover card and map markers.
#[derive(Debug, Clone, Copy)]
pub struct MemoryInspection {
    places: [LandmarkView; LANDMARK_SLOTS],
    len: u8,
    pub explored_tiles: usize,
    pub personality: Personality,
    acquaintances: [AcquaintanceView; ACQUAINTANCE_SLOTS],
    acquaintance_len: u8,
    lexicon: [LexiconEntryView; LEXICON_SLOTS],
    lexicon_len: u8,
    /// Believed food value per material (feeds minus twice sickens; `None` = no idea).
    pub food: [Option<i32>; Material::COUNT],
    /// Per species: (believed worth hunting, believed dangerous), if it has a belief.
    pub fauna: [Option<(bool, bool)>; Species::COUNT],
    pub knows_hearths: bool,
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
            tie: None,
            owed: 0,
            name: None,
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
            knows_hearths: view.knows_hearths,
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
    screen_overlay_capacity: usize,
    details_text: String,
    hits: Vec<Hit>,
    world_revision: u64,
    cached_bounds: Option<WorldRect>,
    cached_step: u32,
    summaries: WorldSummaryCache,
    overview: Option<WorldOverview>,
    capture: Option<PathBuf>,
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
        // Colors are authored in sRGB, so write them as-is to a non-sRGB surface
        // (an sRGB surface would brighten and wash out every color).
        let format = capabilities
            .formats
            .iter()
            .copied()
            .find(|format| !format.is_srgb())
            .unwrap_or(capabilities.formats[0]);
        // Copying frames out (for `--screenshot`) needs COPY_SRC where the surface allows it.
        let usage = wgpu::TextureUsages::RENDER_ATTACHMENT
            | (capabilities.usages & wgpu::TextureUsages::COPY_SRC);
        let config = wgpu::SurfaceConfiguration {
            usage,
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
            screen_overlay_capacity: SCREEN_OVERLAY_CAPACITY,
            details_text: String::new(),
            hits: Vec::new(),
            world_revision: world.revision(),
            cached_bounds: None,
            cached_step: 1,
            summaries: WorldSummaryCache::default(),
            overview,
            capture: None,
            device,
            queue,
            config,
            pipeline,
            world_camera,
            screen_camera,
        })
    }

    /// What the interface drawn in the latest frame has at window pixel (`x`, `y`).
    pub fn ui_at(&self, x: f64, y: f64) -> Option<UiHit> {
        let (x, y) = (x as f32, y as f32);
        self.hits
            .iter()
            .rev()
            .find(|hit| hit.contains(x, y))
            .map(|hit| hit.action.map_or(UiHit::Panel, UiHit::Action))
    }

    /// Saves the next rendered frame as a PNG at `path`.
    pub fn capture_next_frame(&mut self, path: PathBuf) {
        self.capture = Some(path);
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
        // A carried baby: a small pale dot on its mother.
        let bounds = view.world_bounds();
        for agent in engine.agent_views(MAX_AGENT_INSTANCES) {
            if bounds.contains(agent.position)
                && engine
                    .motherhood(agent.id)
                    .is_some_and(|motherhood| motherhood.baby.is_some())
                && self.agent_instances.len() < MAX_AGENT_INSTANCES
            {
                self.agent_instances.push(Instance::new(
                    agent.position.x as f32 + 0.55,
                    agent.position.y as f32 + 0.05,
                    0.4,
                    0.4,
                    colors::BABY,
                ));
            }
        }
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
            .selected
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

        let recent_gestures: Vec<GestureMark> = gestures.recent().copied().collect();
        build_gesture_instances(
            recent_gestures.iter(),
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
        if let Some(agent) = &state.selected {
            world_overlay.extend_from_slice(&ring(
                agent.view.position,
                view.scale() as f32,
                colors::UI_ACCENT,
            ));
        }
        if let Some(Hover::Person { .. }) = state.hover
            && let Some(position) = state.hovered
        {
            world_overlay.extend_from_slice(&ring(position, view.scale() as f32, colors::UI_DIM));
        }
        if state.details_open
            && let Some(inspection) = state.inspected
            && let Some(outline) = chunk_outline(inspection, view.scale() as f32)
        {
            world_overlay.extend_from_slice(&outline);
        }
        world_overlay.extend_from_slice(&world_border(view.scale() as f32));
        debug_assert!(world_overlay.len() <= WORLD_OVERLAY_CAPACITY);
        self.world_overlay.write(&self.queue, world_overlay);

        if state.details_open {
            write_details(&mut self.details_text, world, &state);
        } else {
            self.details_text.clear();
        }
        build_interface(
            &mut self.screen_overlay_instances,
            &mut self.hits,
            &state,
            view,
            &recent_gestures,
            (!self.details_text.is_empty()).then_some(self.details_text.as_str()),
        );
        if self.screen_overlay_instances.len() > self.screen_overlay_capacity {
            self.screen_overlay_capacity = self.screen_overlay_instances.len().next_power_of_two();
            self.screen_overlay = InstanceBuffer::dynamic(
                &self.device,
                "screen overlay",
                self.screen_overlay_capacity,
            );
        }
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
        let capture = self.capture.take().map(|path| {
            let (width, height) = (self.config.width, self.config.height);
            let row_bytes = (width * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
                * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
            let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("screenshot"),
                size: u64::from(row_bytes * height),
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            encoder.copy_texture_to_buffer(
                output.texture.as_image_copy(),
                wgpu::TexelCopyBufferInfo {
                    buffer: &buffer,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(row_bytes),
                        rows_per_image: Some(height),
                    },
                },
                wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
            );
            (path, buffer, row_bytes)
        });
        self.queue.submit(Some(encoder.finish()));
        if let Some((path, buffer, row_bytes)) = capture {
            self.save_capture(&path, &buffer, row_bytes);
        }
        output.present();
        Ok(())
    }

    fn save_capture(&self, path: &std::path::Path, buffer: &wgpu::Buffer, row_bytes: u32) {
        let slice = buffer.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        if let Err(error) = self.device.poll(wgpu::PollType::Wait) {
            eprintln!("screenshot failed: {error}");
            return;
        }
        let (width, height) = (self.config.width as usize, self.config.height as usize);
        let bgra = matches!(
            self.config.format,
            wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb
        );
        let mut pixels = Vec::with_capacity(width * height * 4);
        for row in slice.get_mapped_range().chunks(row_bytes as usize) {
            for pixel in row[..width * 4].chunks_exact(4) {
                let (red, blue) = if bgra {
                    (pixel[2], pixel[0])
                } else {
                    (pixel[0], pixel[2])
                };
                pixels.extend_from_slice(&[red, pixel[1], blue, 255]);
            }
        }
        let png = crate::screenshot::encode_png(width as u32, height as u32, &pixels);
        match std::fs::write(path, png) {
            Ok(()) => println!("saved screenshot {}", path.display()),
            Err(error) => eprintln!("screenshot {} failed: {error}", path.display()),
        }
    }
}

/// A square outline around `position`, a constant few pixels thick.
fn ring(position: WorldPosition, scale: f32, color: u32) -> [Instance; 4] {
    let line = (2.0 / scale.max(f32::EPSILON)).min(0.5);
    let margin = (3.0 / scale.max(f32::EPSILON)).max(0.25);
    let x = position.x as f32 - margin;
    let y = position.y as f32 - margin;
    let side = 1.0 + 2.0 * margin;
    [
        Instance::new(x, y, side, line, color),
        Instance::new(x, y + side - line, side, line, color),
        Instance::new(x, y, line, side, color),
        Instance::new(x + side - line, y, line, side, color),
    ]
}

const MAX_INSTANCES_PER_BUFFER: usize = 1_000_000;
const MIN_TERRAIN_SAMPLE_PIXELS: f32 = 2.0;
const CACHE_MARGIN_PIXELS: f32 = 128.0;
/// Hovered cell, generation selection, two person rings, chunk outline, world border.
const WORLD_OVERLAY_CAPACITY: usize = 18;
/// Initial interface buffer; it grows when a frame needs more.
const SCREEN_OVERLAY_CAPACITY: usize = 16_384;
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
/// Each recent gesture draws its dotted line and an end square.
const MAX_GESTURE_MARKER_INSTANCES: usize = RECENT_GESTURE_CAPACITY * (MAX_GESTURE_DOTS + 1);
const MIN_DYNAMIC_INSTANCE_PIXELS: f32 = 1.25;
const MIN_CHUNK_OUTLINE_PIXELS: f32 = 4.0;
const MAX_CHUNK_OUTLINE_WORLD_WIDTH: f32 = 8.0;
const MAX_WORLD_BORDER_WIDTH: f32 = 32.0;
