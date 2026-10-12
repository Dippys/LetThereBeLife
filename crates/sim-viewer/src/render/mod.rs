//! GPU world renderer: public render-state view types and the `Renderer` that syncs caches, builds instances, and draws a frame.

mod colors;
mod details;
mod gpu;
mod instances;
mod sprites;
mod summary;
mod text;
mod ui;
mod windows;

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
    append_cleared_instances, append_wildlife_instances, build_agent_instances,
    build_gesture_instances, build_memory_marker_instances, build_relationship_marker_instances,
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
    pub info_open: bool,
    pub details_open: bool,
    /// The speed actually reached, when the computer can't keep up.
    pub reached_speed: Option<u32>,
    /// The build palette's chosen tool while the palette is open.
    pub build: Option<BuildTool>,
    pub feed: Vec<FeedEntry>,
    pub census: Census,
    pub season: sim_core::Season,
    /// Show small signs over people saying what they're doing (when zoomed in).
    pub signs: bool,
    /// Open floating windows, back to front.
    pub windows: Vec<WindowView>,
    /// How dark it is: 0 by day, 1 at the depth of night.
    pub darkness: f32,
    /// The windows menu is open.
    pub windows_menu: bool,
    /// Show the small map of the band's land in the corner.
    pub minimap: bool,
    /// The start screen, while it's up.
    pub start: Option<StartView>,
}

/// The start screen: what to begin with, and the settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartView {
    pub settings_open: bool,
    /// A quick save exists to open.
    pub save_exists: bool,
    /// The settings' current values, as shown on their buttons.
    pub settings: Vec<(Setting, String)>,
}

/// Something set from the start screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Setting {
    Signs,
    SmallMap,
    Alerts,
    Director,
}

/// A choice on the start screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartChoice {
    NewValley,
    EmptyWorld,
    OpenSave,
    Settings,
    Change(Setting),
}

/// Head counts for the top bar.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Census {
    pub people: u32,
    pub dead: u32,
    pub deer: u32,
    pub wolves: u32,
    /// Living people past each need's threshold.
    pub cold: u32,
    pub hungry: u32,
    pub thirsty: u32,
    pub tired: u32,
    /// Finished hearths, and how many are burning.
    pub hearths: u32,
    /// Finished huts, and the food stored in them.
    pub huts: u32,
    pub stored_food: u32,
    pub fires_burning: u32,
    pub born_this_year: u32,
    pub died_this_year: u32,
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
        /// For a fire: seconds of burning left (0 = out).
        burning: Option<u32>,
        /// What a hut holds.
        stored: sim_core::InventoryView,
        /// Who lives there: up to three names, and how many more.
        residents: ([Option<sim_core::Name>; 3], u8),
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
    Info,
    CloseSelected,
    Follow,
    NextPerson,
    Tool(BuildTool),
    /// The feed entry at this index (oldest first).
    FeedEntry(usize),
    /// Open (or bring forward) a window.
    OpenWindow(WindowKind),
    /// The windows menu in the top bar.
    WindowsMenu,
    /// Close the window at this index (back to front).
    CloseWindow(usize),
    /// Grab the window at this index by its title bar.
    WindowTitle(usize),
    /// Somewhere in the window at this index (brings it forward).
    WindowBody(usize),
    /// Pick a person (and look at them).
    Pick(sim_core::AgentId),
    /// Look at a place, picking whoever it was about.
    LookAt(sim_core::WorldPosition, Option<sim_core::AgentId>),
    /// Sort the people list by this column.
    SortPeople(PeopleSort),
    /// Center the view here (a click on the small map).
    CenterOn(sim_core::WorldPosition),
    /// A choice on the start screen.
    Start(StartChoice),
}

/// The floating windows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowKind {
    /// The band's story, year by year.
    Story,
    /// How the band has done over time.
    Charts,
    /// Who is whose child and partner.
    Family,
    /// Everyone alive, in a list.
    People,
    /// What the picked person is thinking and believes.
    Mind,
}

impl WindowKind {
    pub const ALL: [Self; 5] = [
        Self::Story,
        Self::Charts,
        Self::Family,
        Self::People,
        Self::Mind,
    ];

    pub const fn title(self) -> &'static str {
        match self {
            Self::Story => "Story",
            Self::Charts => "Charts",
            Self::Family => "Family",
            Self::People => "People",
            Self::Mind => "Mind",
        }
    }

    /// The key that opens it.
    pub const fn key(self) -> &'static str {
        match self {
            Self::Story => "J",
            Self::Charts => "G",
            Self::Family => "K",
            Self::People => "P",
            Self::Mind => "M",
        }
    }

    /// Its size, in characters across and lines down.
    pub const fn size(self) -> (usize, usize) {
        match self {
            Self::Charts => (52, 24),
            Self::Mind => (50, 26),
            _ => (48, 24),
        }
    }
}

/// How the people list is ordered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PeopleSort {
    #[default]
    Name,
    Age,
    Doing,
}

/// How a line in a window is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineStyle {
    Normal,
    Dim,
    Heading,
    Good,
    Bad,
    Talk,
}

/// One line of a window: text (indented), how it looks, and what clicking it does.
#[derive(Debug, Clone, PartialEq)]
pub struct WindowLine {
    pub text: String,
    pub style: LineStyle,
    pub indent: u8,
    pub action: Option<UiAction>,
}

impl WindowLine {
    pub fn new(text: impl Into<String>, style: LineStyle) -> Self {
        Self {
            text: text.into(),
            style,
            indent: 0,
            action: None,
        }
    }

    pub fn indented(mut self, indent: u8) -> Self {
        self.indent = indent;
        self
    }

    pub fn on_click(mut self, action: UiAction) -> Self {
        self.action = Some(action);
        self
    }
}

/// One line on a chart: its name and its values over time (oldest first).
#[derive(Debug, Clone, PartialEq)]
pub struct Series {
    pub label: &'static str,
    pub style: LineStyle,
    pub values: Vec<u32>,
    /// For counts per period (births, deaths): the total, shown instead of the latest.
    pub total: Option<u32>,
}

/// What a window shows.
#[derive(Debug, Clone, PartialEq)]
pub enum WindowBody {
    /// Lines from `first` on (what's scrolled into view), of `total`.
    Lines {
        lines: Vec<WindowLine>,
        first: usize,
        total: usize,
    },
    /// Charts over the years shown, with a note under them.
    Charts { series: Vec<Series>, years: u32 },
}

/// A floating window as drawn.
#[derive(Debug, Clone, PartialEq)]
pub struct WindowView {
    pub kind: WindowKind,
    pub title: String,
    pub x: f32,
    pub y: f32,
    pub body: WindowBody,
}

/// What the interface has at a screen point.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiHit {
    Action(UiAction),
    /// A panel that keeps the click from reaching the map.
    Panel,
}

pub(crate) use text::wrap;
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
    /// What they're set on doing, if they hold a plan.
    pub intention: Option<sim_core::IntentionView>,
}

/// A bounded, copyable snapshot of one agent's mind (places, personality,
/// acquaintances, and lexicon) for the hover card and map markers.
#[derive(Debug, Clone, Copy)]
pub struct MemoryInspection {
    places: [LandmarkView; LANDMARK_SLOTS],
    len: u8,
    pub explored_tiles: usize,
    pub personality: Personality,
    /// How good they are at each activity (0-255).
    pub abilities: [u8; sim_core::Activity::COUNT],
    /// The feeling showing on their face, if any.
    pub feeling: Option<sim_core::Feeling>,
    acquaintances: [AcquaintanceView; ACQUAINTANCE_SLOTS],
    acquaintance_len: u8,
    lexicon: [LexiconEntryView; LEXICON_SLOTS],
    lexicon_len: u8,
    /// Believed food value per material (feeds minus twice sickens; `None` = no idea).
    pub food: [Option<i32>; Material::COUNT],
    /// Per species: (believed worth hunting, believed dangerous), if it has a belief.
    pub fauna: [Option<(bool, bool)>; Species::COUNT],
    pub knows_hearths: bool,
    pub knows_knapping: bool,
    pub knows_huts: bool,
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
            abilities: view.abilities,
            feeling: view.feeling,
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
            knows_knapping: view.knows_knapping,
            knows_huts: view.knows_huts,
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
    /// Worn ground where people walk often.
    paths: InstanceBuffer,
    path_instances: Vec<Instance>,
    /// The season's light over the land, and firelight.
    atmosphere: InstanceBuffer,
    atmosphere_instances: Vec<Instance>,
    /// Zoomed in: texture on the ground, pictures of plants and buildings,
    /// and of people and animals (in place of squares).
    detail_ground: InstanceBuffer,
    detail_ground_instances: Vec<Instance>,
    detail_objects: InstanceBuffer,
    detail_object_instances: Vec<Instance>,
    detail_beings: InstanceBuffer,
    detail_being_instances: Vec<Instance>,
    /// Whether the last frame drew pictures rather than squares.
    detailed: bool,
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
            agents: InstanceBuffer::dynamic(&device, "agent instances", MAX_AGENT_DRAWN),
            agent_instances: Vec::with_capacity(MAX_AGENT_DRAWN),
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
            paths: InstanceBuffer::dynamic(&device, "worn paths", MAX_PATH_INSTANCES),
            path_instances: Vec::with_capacity(MAX_PATH_INSTANCES),
            atmosphere: InstanceBuffer::dynamic(&device, "atmosphere", MAX_ATMOSPHERE_INSTANCES),
            atmosphere_instances: Vec::with_capacity(MAX_ATMOSPHERE_INSTANCES),
            detail_ground: InstanceBuffer::dynamic(&device, "ground detail", MAX_DETAIL_GROUND),
            detail_ground_instances: Vec::with_capacity(MAX_DETAIL_GROUND),
            detail_objects: InstanceBuffer::dynamic(&device, "object pictures", MAX_DETAIL_OBJECTS),
            detail_object_instances: Vec::with_capacity(MAX_DETAIL_OBJECTS),
            detail_beings: InstanceBuffer::dynamic(&device, "people pictures", MAX_DETAIL_BEINGS),
            detail_being_instances: Vec::with_capacity(MAX_DETAIL_BEINGS),
            detailed: false,
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

    /// The window under a screen point, if any (by its index, back to front).
    pub fn window_at(&self, x: f64, y: f64) -> Option<usize> {
        let (x, y) = (x as f32, y as f32);
        self.hits.iter().rev().find_map(|hit| match hit.action {
            Some(UiAction::WindowBody(index)) if hit.contains(x, y) => Some(index),
            _ => None,
        })
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
        tracks: Tracks,
        allow_world_sync: bool,
        changed_bounds: Option<WorldRect>,
    ) -> Result<(), wgpu::SurfaceError> {
        let Tracks { wear, motion } = tracks;
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

        self.path_instances.clear();
        if view.scale() as f32 >= MIN_DYNAMIC_INSTANCE_PIXELS {
            for (cell, worn) in wear.worn_in(view.world_bounds()).take(MAX_PATH_INSTANCES) {
                self.path_instances.push(Instance::new(
                    cell.x as f32,
                    cell.y as f32,
                    1.0,
                    1.0,
                    colors::worn(worn),
                ));
            }
        }
        self.paths.write(&self.queue, &self.path_instances);
        build_atmosphere(
            engine,
            state.season,
            state.darkness,
            view.world_bounds(),
            (view.scale() as f32, state.snapshot.tick),
            &mut self.atmosphere_instances,
        );
        self.atmosphere
            .write(&self.queue, &self.atmosphere_instances);
        build_structure_instances(
            engine.structure_views(MAX_STRUCTURE_INSTANCES),
            view.world_bounds(),
            view.scale() as f32,
            (state.snapshot.tick / 60) as u32,
            &mut self.structure_instances,
        );
        debug_assert!(self.structure_instances.len() <= MAX_STRUCTURE_INSTANCES);
        self.structures
            .write(&self.queue, &self.structure_instances);
        build_agent_instances(
            engine
                .agent_views(MAX_AGENT_INSTANCES)
                .filter(|agent| agent.activity != sim_core::AgentActivity::Dead)
                .map(|agent| (agent, engine.life(agent.id))),
            view.world_bounds(),
            view.scale() as f32,
            &mut self.agent_instances,
        );
        // The dead, as what's left of them: a body, bones, or a grave.
        if view.scale() as f32 >= MIN_DYNAMIC_INSTANCE_PIXELS {
            for (_, position, state) in engine.bodies() {
                if view.world_bounds().contains(position)
                    && self.agent_instances.len() < MAX_AGENT_DRAWN
                {
                    let (inset, color) = match state {
                        sim_core::BodyState::Fresh => (0.1, colors::BODY),
                        sim_core::BodyState::Rotting => (0.1, colors::BODY_ROTTING),
                        sim_core::BodyState::Bones => (0.25, colors::BONES),
                        sim_core::BodyState::Buried => (0.2, colors::GRAVE),
                    };
                    self.agent_instances.push(Instance::new(
                        position.x as f32 + inset,
                        position.y as f32 + 0.35,
                        1.0 - 2.0 * inset,
                        0.4,
                        color,
                    ));
                }
            }
        }
        // A carried baby: a small pale dot on its mother.
        let bounds = view.world_bounds();
        for agent in engine.agent_views(MAX_AGENT_INSTANCES) {
            if bounds.contains(agent.position)
                && engine
                    .motherhood(agent.id)
                    .is_some_and(|motherhood| motherhood.baby.is_some())
                && self.agent_instances.len() < MAX_AGENT_DRAWN
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
        debug_assert!(self.agent_instances.len() <= MAX_AGENT_DRAWN);
        self.agents.write(&self.queue, &self.agent_instances);
        build_spawned_object_instances(
            engine.spawned_object_views(),
            view.world_bounds(),
            view.scale() as f32,
            &mut self.spawned_object_instances,
        );
        append_cleared_instances(
            engine.dead_plants(),
            engine.world(),
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
        // Zoomed in: pictures instead of squares.
        self.detailed = view.scale() as f32 >= sprites::SPRITE_CELL_PIXELS;
        self.detail_ground_instances.clear();
        self.detail_object_instances.clear();
        self.detail_being_instances.clear();
        if self.detailed {
            build_pictures(
                engine,
                motion,
                &state,
                view.world_bounds(),
                &mut self.detail_ground_instances,
                &mut self.detail_object_instances,
                &mut self.detail_being_instances,
            );
            // Placed water and carcasses still draw as squares; plants and
            // animals are pictures now.
            self.spawned_object_instances.clear();
            build_spawned_object_instances(
                engine
                    .spawned_object_views()
                    .filter(|object| object.kind == sim_core::SpawnKind::Water),
                view.world_bounds(),
                view.scale() as f32,
                &mut self.spawned_object_instances,
            );
            append_wildlife_instances(
                std::iter::empty(),
                engine.carcass_views(),
                view.world_bounds(),
                view.scale() as f32,
                &mut self.spawned_object_instances,
            );
            self.spawned_objects
                .write(&self.queue, &self.spawned_object_instances);
        }
        self.detail_ground_instances.truncate(MAX_DETAIL_GROUND);
        self.detail_object_instances.truncate(MAX_DETAIL_OBJECTS);
        self.detail_being_instances.truncate(MAX_DETAIL_BEINGS);
        self.detail_ground
            .write(&self.queue, &self.detail_ground_instances);
        self.detail_objects
            .write(&self.queue, &self.detail_object_instances);
        self.detail_beings
            .write(&self.queue, &self.detail_being_instances);
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
        // The season tints the land: frosty in winter, warm in autumn.
        let tint = match state.season {
            sim_core::Season::Winter => Some(colors::WINTER_TINT),
            sim_core::Season::Autumn => Some(colors::AUTUMN_TINT),
            sim_core::Season::Spring | sim_core::Season::Summer => None,
        };
        if let Some(tint) = tint {
            let bounds = view.world_bounds();
            world_overlay.push(Instance::new(
                bounds.min.x as f32,
                bounds.min.y as f32,
                (bounds.max.x - bounds.min.x) as f32,
                (bounds.max.y - bounds.min.y) as f32,
                tint,
            ));
        }
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
        let signs = if state.signs && view.scale() as f32 >= MIN_SIGN_CELL_PIXELS {
            people_signs(engine, view.world_bounds())
        } else {
            Vec::new()
        };
        // What each watcher took the latest gestures to mean, over their heads.
        let readings: Vec<Reading> = gestures
            .readings()
            .filter_map(|reading| {
                let listener = engine
                    .agent_views(MAX_AGENT_INSTANCES)
                    .find(|agent| agent.id == reading.receiver)?;
                Some((
                    listener.position,
                    format!("{}?", crate::labels::concept(reading.understood)),
                    reading.wrong,
                ))
            })
            .collect();
        // Close up, names under people's feet in their family's color.
        let names: Vec<Name> = if view.scale() as f32 >= MIN_NAME_CELL_PIXELS {
            let tick = state.snapshot.tick;
            engine
                .agent_views(MAX_AGENT_INSTANCES)
                .filter(|agent| {
                    view.world_bounds().contains(agent.position)
                        && agent.activity != sim_core::AgentActivity::Dead
                })
                .filter_map(|agent| {
                    let life = engine.life(agent.id)?;
                    let (at, _) = motion.drawn(agent.id, agent.position, tick);
                    let color = motion
                        .family_line(engine, agent.id)
                        .map_or(colors::UI_TEXT, colors::family_line);
                    Some((at, life.name.spoken(), color))
                })
                .collect()
        } else {
            Vec::new()
        };
        let minimap = state
            .minimap
            .then(|| build_minimap(engine, view.world_bounds(), state.season));
        build_interface(
            &mut self.screen_overlay_instances,
            &mut self.hits,
            &state,
            view,
            &ui::MapMarks {
                gestures: &recent_gestures,
                signs: &signs,
                readings: &readings,
                names: &names,
                minimap: minimap.as_ref(),
            },
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
            if self.detailed {
                self.detail_ground.draw(&mut pass);
            }
            self.paths.draw(&mut pass);
            if self.detailed {
                self.detail_objects.draw(&mut pass);
            } else {
                self.features.draw(&mut pass);
            }
            self.spawned_objects.draw(&mut pass);
            if !self.detailed {
                self.structures.draw(&mut pass);
            }
            self.atmosphere.draw(&mut pass);
            self.memory_markers.draw(&mut pass);
            self.relationship_markers.draw(&mut pass);
            self.gesture_markers.draw(&mut pass);
            if self.detailed {
                self.detail_beings.draw(&mut pass);
            } else {
                self.agents.draw(&mut pass);
            }
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
/// Season tint, hovered cell, generation selection, two person rings, chunk
/// outline, world border.
const WORLD_OVERLAY_CAPACITY: usize = 19;
/// Initial interface buffer; it grows when a frame needs more.
const SCREEN_OVERLAY_CAPACITY: usize = 16_384;
const MAX_AGENT_INSTANCES: usize = 4_096;
/// Up to three rectangles per person (an elder's edge, a woman's two bars) and
/// a carried baby's dot.
const MAX_AGENT_DRAWN: usize = MAX_AGENT_INSTANCES * 4;
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
/// What the viewer has noticed over time: worn ground, and people's last steps
/// and family lines.
#[derive(Clone, Copy)]
pub struct Tracks<'a> {
    pub wear: &'a crate::paths::PathWear,
    pub motion: &'a crate::motion::Motion,
}

/// Pictures drawn at most: ground texture, plants and buildings, people and animals.
const MAX_DETAIL_GROUND: usize = 1 << 17;
const MAX_DETAIL_OBJECTS: usize = 1 << 17;
const MAX_DETAIL_BEINGS: usize = 1 << 15;

/// Zoomed in, the land as pictures: texture on the ground, then plants,
/// rocks, huts and fires, then people and animals (people gliding between cells).
fn build_pictures(
    engine: &Engine,
    motion: &crate::motion::Motion,
    state: &RenderState,
    bounds: WorldRect,
    ground: &mut Vec<Instance>,
    objects: &mut Vec<Instance>,
    beings: &mut Vec<Instance>,
) {
    let tick = state.snapshot.tick;
    let now = (tick / 60) as u32;
    let winter = state.season == sim_core::Season::Winter;
    let wave = (tick / 20) as u32;
    let dead: std::collections::BTreeSet<(i64, i64)> = engine
        .dead_plants()
        .filter(|position| bounds.contains(*position))
        .map(|position| (position.x, position.y))
        .collect();
    let left = |position: sim_core::WorldPosition| {
        engine
            .available_resource_at(position)
            .ok()
            .flatten()
            .map_or(0, |resource| resource.capacity)
    };
    engine
        .world()
        .visit_area(bounds, |position, cell, feature| {
            sprites::ground(
                ground,
                position.x,
                position.y,
                cell,
                colors::terrain_color(cell),
                wave,
            );
            let Some(feature) = feature else {
                return;
            };
            if dead.contains(&(position.x, position.y)) {
                return;
            }
            let bearing = match feature.kind {
                sim_core::FeatureKind::Rock => true,
                sim_core::FeatureKind::Tree => left(position) >= GROWN_TREE_WOOD,
                _ => left(position) > 0,
            };
            sprites::feature(
                objects,
                position.x as f32,
                position.y as f32,
                feature.kind,
                bearing,
                winter,
            );
        });
    for object in engine.spawned_object_views() {
        if !bounds.contains(object.position) {
            continue;
        }
        let Some(kind) = object.kind.feature() else {
            continue;
        };
        let remaining = object.remaining.unwrap_or(0);
        let bearing = match kind {
            sim_core::FeatureKind::Rock => true,
            sim_core::FeatureKind::Tree => remaining >= GROWN_TREE_WOOD,
            _ => remaining > 0,
        };
        sprites::feature(
            objects,
            object.position.x as f32,
            object.position.y as f32,
            kind,
            bearing,
            winter,
        );
    }
    let flicker = ((tick / 6) % 4) as u8;
    // Whose home each hut is: the first resident's family line.
    let mut homes: Vec<(sim_core::WorldPosition, u8)> = Vec::new();
    for agent in engine.agent_views(MAX_AGENT_INSTANCES) {
        if agent.activity == sim_core::AgentActivity::Dead {
            continue;
        }
        if let Some(home) = engine.home_of(agent.id)
            && bounds.contains(home)
            && !homes.iter().any(|(hut, _)| *hut == home)
            && let Some(line) = motion.family_line(engine, agent.id)
        {
            homes.push((home, line));
        }
    }
    for structure in engine.structure_views(MAX_STRUCTURE_INSTANCES) {
        if !bounds.contains(structure.position) {
            continue;
        }
        let family = homes
            .iter()
            .find(|(hut, _)| *hut == structure.position)
            .map(|&(_, line)| colors::family_line(line));
        sprites::structure(
            objects,
            structure.position.x as f32,
            structure.position.y as f32,
            &sprites::StructureLook {
                hearth: structure.kind.burns(),
                finished: structure.state == sim_core::StructureState::Complete,
                burning: structure.working(now),
                flicker,
                family,
            },
        );
    }
    for animal in engine.animal_views() {
        if !bounds.contains(animal.position) {
            continue;
        }
        let alert = matches!(
            animal.mode,
            sim_core::AnimalMode::Fleeing | sim_core::AnimalMode::Hunting
        );
        // Young in their first year are drawn small.
        let young = (tick / 60) as i64 - i64::from(animal.born) < sim_core::SECONDS_PER_YEAR;
        sprites::animal(
            beings,
            animal.position.x as f32,
            animal.position.y as f32,
            animal.species,
            alert,
            young,
        );
    }
    for (_, position, state) in engine.bodies() {
        if bounds.contains(position) {
            sprites::body(beings, position.x as f32, position.y as f32, state);
        }
    }
    for agent in engine.agent_views(MAX_AGENT_INSTANCES) {
        if !bounds.contains(agent.position) || agent.activity == sim_core::AgentActivity::Dead {
            continue;
        }
        let ([x, y], step) = motion.drawn(agent.id, agent.position, tick);
        let life = engine.life(agent.id);
        let walking = matches!(agent.activity, sim_core::AgentActivity::Moving);
        let look = sprites::PersonLook {
            body: colors::agent_color(agent.activity),
            family: motion
                .family_line(engine, agent.id)
                .map(colors::family_line),
            sex: life.map(|life| life.sex),
            stage: life.map(|life| life.stage),
            activity: agent.activity,
            step: walking && step,
            hide: engine
                .inventory(agent.id)
                .is_some_and(|inventory| inventory.amount(sim_core::Material::Hide) > 0),
        };
        sprites::person(beings, x, y, &look);
        // A carried baby: a small bundle at the hip.
        if engine
            .motherhood(agent.id)
            .is_some_and(|motherhood| motherhood.baby.is_some())
        {
            beings.push(Instance::new(x + 0.62, y + 0.45, 0.22, 0.22, colors::BABY));
        }
    }
}

/// Wood from which a tree is drawn grown (below it, a sapling).
const GROWN_TREE_WOOD: u16 = 20;

/// Worn cells drawn at most.
const MAX_PATH_INSTANCES: usize = 16_384;
/// The season's tint plus fire glows drawn at most.
const MAX_ATMOSPHERE_INSTANCES: usize = 2_048;
/// How far firelight reaches, in cells, and in how many rings it fades.
const FIRELIGHT_REACH: f32 = 7.0;
const FIRELIGHT_RINGS: usize = 4;

/// The season's light over everything in `bounds`, then a warm glow around
/// every burning fire in view.
fn build_atmosphere(
    engine: &sim_core::Engine,
    season: sim_core::Season,
    darkness: f32,
    bounds: sim_core::WorldRect,
    (scale, tick): (f32, u64),
    out: &mut Vec<Instance>,
) {
    out.clear();
    let now = (tick / 60) as u32;
    let cover = |out: &mut Vec<Instance>, color: u32| {
        out.push(Instance::new(
            bounds.min.x as f32,
            bounds.min.y as f32,
            (bounds.max.x - bounds.min.x) as f32,
            (bounds.max.y - bounds.min.y) as f32,
            color,
        ));
    };
    if let Some(tint) = colors::season_tint(season) {
        cover(out, tint);
    }
    weather(season, bounds, scale, tick, out);
    if darkness > 0.0 {
        cover(out, colors::night(darkness));
    }
    let reach = FIRELIGHT_REACH as i64;
    for fire in engine.structure_views(MAX_STRUCTURE_INSTANCES) {
        let near = fire.position.x >= bounds.min.x - reach
            && fire.position.x <= bounds.max.x + reach
            && fire.position.y >= bounds.min.y - reach
            && fire.position.y <= bounds.max.y + reach;
        if !(fire.kind.burns() && fire.working(now) && near) {
            continue;
        }
        for ring in 0..FIRELIGHT_RINGS {
            if out.len() >= MAX_ATMOSPHERE_INSTANCES {
                return;
            }
            let radius = FIRELIGHT_REACH * (ring + 1) as f32 / FIRELIGHT_RINGS as f32;
            out.push(Instance::new(
                fire.position.x as f32 + 0.5 - radius,
                fire.position.y as f32 + 0.5 - radius,
                2.0 * radius,
                2.0 * radius,
                colors::firelight(darkness),
            ));
        }
    }
}

/// Cells across the small map.
pub(super) const MINIMAP_SIDE: usize = 48;

/// The small map: the land around the band (at least `MINIMAP_MIN_SPAN` cells
/// across), people and fires on it, and the part in view.
pub(super) struct Minimap {
    pub(super) area: WorldRect,
    /// `MINIMAP_SIDE` squared colors, row by row (`None` where not generated).
    pub(super) cells: Vec<Option<u32>>,
    pub(super) people: Vec<(WorldPosition, u32)>,
    pub(super) fires: Vec<WorldPosition>,
    pub(super) view: WorldRect,
}

const MINIMAP_MIN_SPAN: i64 = 192;

fn build_minimap(engine: &Engine, view: WorldRect, season: sim_core::Season) -> Minimap {
    let living: Vec<_> = engine
        .agent_views(MAX_AGENT_INSTANCES)
        .filter(|agent| agent.activity != sim_core::AgentActivity::Dead)
        .collect();
    let (mut min, mut max) = (
        WorldPosition {
            x: i64::MAX,
            y: i64::MAX,
        },
        WorldPosition {
            x: i64::MIN,
            y: i64::MIN,
        },
    );
    for agent in &living {
        min.x = min.x.min(agent.position.x);
        min.y = min.y.min(agent.position.y);
        max.x = max.x.max(agent.position.x);
        max.y = max.y.max(agent.position.y);
    }
    if living.is_empty() {
        min = view.min;
        max = view.max;
    }
    let span = (max.x - min.x).max(max.y - min.y).max(MINIMAP_MIN_SPAN) + 48;
    let center = WorldPosition {
        x: (min.x + max.x) / 2,
        y: (min.y + max.y) / 2,
    };
    let area = WorldRect {
        min: WorldPosition {
            x: center.x - span / 2,
            y: center.y - span / 2,
        },
        max: WorldPosition {
            x: center.x - span / 2 + span,
            y: center.y - span / 2 + span,
        },
    };
    let world = engine.world();
    let step = span / MINIMAP_SIDE as i64;
    let mut cells = Vec::with_capacity(MINIMAP_SIDE * MINIMAP_SIDE);
    for row in 0..MINIMAP_SIDE as i64 {
        for column in 0..MINIMAP_SIDE as i64 {
            let position = WorldPosition {
                x: area.min.x + column * step + step / 2,
                y: area.min.y + row * step + step / 2,
            };
            cells.push(world.cell(position).map(|cell| {
                let color = colors::terrain_color(cell);
                match (season, cell.water()) {
                    (sim_core::Season::Winter, None) => colors::snowy(color),
                    _ => color,
                }
            }));
        }
    }
    let now = (engine.snapshot().tick / 60) as u32;
    Minimap {
        area,
        cells,
        people: living
            .iter()
            .map(|agent| (agent.position, colors::agent_color(agent.activity)))
            .collect(),
        fires: engine
            .structure_views(MAX_STRUCTURE_INSTANCES)
            .filter(|fire| fire.kind.burns() && fire.working(now))
            .map(|fire| fire.position)
            .collect(),
        view,
    }
}

/// Flakes or leaves drifting across the view at once.
const WEATHER_SPECKS: u64 = 260;

/// Snow falling in winter and leaves drifting down in autumn: specks spread
/// over the view by a fixed hash, each falling at its own pace with the clock,
/// a few screen pixels across whatever the zoom.
fn weather(
    season: sim_core::Season,
    bounds: sim_core::WorldRect,
    scale: f32,
    tick: u64,
    out: &mut Vec<Instance>,
) {
    let (count, size, colors_of): (u64, f32, &[u32]) = match season {
        sim_core::Season::Winter => (WEATHER_SPECKS, 3.0, &[colors::SNOW]),
        sim_core::Season::Autumn => (WEATHER_SPECKS / 4, 4.0, &colors::LEAVES),
        _ => return,
    };
    let width = (bounds.max.x - bounds.min.x) as f32;
    let height = (bounds.max.y - bounds.min.y) as f32;
    let speck = size / scale.max(0.01);
    // Screen pixels fallen per tick.
    let fall = 0.5 / scale.max(0.01);
    for index in 0..count {
        let mut hash = index.wrapping_mul(0x9e37_79b9_7f4a_7c15);
        hash ^= hash >> 31;
        let across = (hash % 10_000) as f32 / 10_000.0;
        let start = ((hash >> 16) % 10_000) as f32 / 10_000.0;
        let pace = 0.6 + ((hash >> 32) % 100) as f32 / 100.0;
        let fallen = tick as f32 * fall * pace;
        let y = (start * height + fallen).rem_euclid(height);
        // A gentle sideways sway.
        let sway = ((tick as f32 / 40.0 + index as f32).sin()) * 6.0 / scale.max(0.01);
        let x = (across * width + sway + fallen * 0.3).rem_euclid(width);
        out.push(Instance::new(
            bounds.min.x as f32 + x,
            bounds.min.y as f32 + y,
            speck,
            speck,
            colors_of[index as usize % colors_of.len()],
        ));
    }
}

/// Cells must be at least this many pixels wide before signs over people show.
const MIN_SIGN_CELL_PIXELS: f32 = 6.0;

/// A small sign over someone's head saying what they're doing: where, the
/// symbol, and its color.
pub(super) type PersonSign = (sim_core::WorldPosition, char, u32);
/// What a watcher took a gesture to mean: where it stands, the meaning, and
/// whether that's not what the speaker meant.
pub(super) type Reading = (sim_core::WorldPosition, String, bool);

/// Cells must be at least this many pixels wide before names show.
const MIN_NAME_CELL_PIXELS: f32 = 24.0;

/// A name under someone: where they're drawn, the name, its color.
pub(super) type Name = ([f32; 2], String, u32);

/// Signs for the living people in `bounds`: asleep, by a fire, at water,
/// eating, carrying or gathering, building, hunting, running.
fn people_signs(engine: &sim_core::Engine, bounds: sim_core::WorldRect) -> Vec<PersonSign> {
    use sim_core::{AgentActivity, PhysicalGoal, PolicyReason};
    engine
        .agent_views(MAX_AGENT_INSTANCES)
        .filter(|agent| bounds.contains(agent.position))
        .filter_map(|agent| {
            let sign = match agent.activity {
                AgentActivity::Dead => return None,
                AgentActivity::Sleeping => ('z', colors::SIGN_SLEEP),
                AgentActivity::Building => ('#', colors::SIGN_BUILD),
                _ => {
                    let policy = engine.physical_policy(agent.id)?;
                    match (policy.goal, policy.reason) {
                        (_, PolicyReason::Fleeing) => ('!', colors::SIGN_DANGER),
                        (PhysicalGoal::Hunt, _) | (_, PolicyReason::Hunting) => {
                            ('!', colors::SIGN_HUNT)
                        }
                        (PhysicalGoal::WarmUp | PhysicalGoal::TendFire, _)
                        | (_, PolicyReason::Warming | PolicyReason::Tending) => {
                            ('^', colors::SIGN_FIRE)
                        }
                        (PhysicalGoal::SeekWater | PhysicalGoal::Drink, _) => {
                            ('~', colors::SIGN_WATER)
                        }
                        (PhysicalGoal::Eat | PhysicalGoal::SeekFood, _) => ('*', colors::SIGN_FOOD),
                        (
                            PhysicalGoal::GatherMaterial
                            | PhysicalGoal::Store
                            | PhysicalGoal::Fetch
                            | PhysicalGoal::Craft
                            | PhysicalGoal::Plant,
                            _,
                        ) => ('=', colors::SIGN_WORK),
                        (PhysicalGoal::BuildShelter | PhysicalGoal::BuildHearth, _) => {
                            ('#', colors::SIGN_BUILD)
                        }
                        _ => return None,
                    }
                }
            };
            Some((agent.position, sign.0, sign.1))
        })
        .collect()
}
const MIN_CHUNK_OUTLINE_PIXELS: f32 = 4.0;
const MAX_CHUNK_OUTLINE_WORLD_WIDTH: f32 = 8.0;
const MAX_WORLD_BORDER_WIDTH: f32 = 32.0;
