//! Viewer application state: the `ViewerApp` struct, its construction, and shared app constants.

mod events;
mod frame;
mod input;
mod selection;
mod simulation;
mod world_loading;

#[cfg(test)]
mod tests;

use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

use sim_core::{
    AgentId, ChunkInspection, ChunkLoadRequest, Engine, WorldArchive, WorldPosition, WorldRect,
};
use winit::window::Window;

use crate::{
    camera::{Camera, Viewport},
    feed::Feed,
    generation::{ChunkPager, GenerationId, GenerationKind, WorldGenerator},
    gestures::GestureLog,
    render::{self, BuildTool},
    startup::{ValleyStart, residency_ready},
};

#[derive(Clone, Copy)]
struct SelectionValidation {
    bounds: WorldRect,
    world_revision: u64,
    valid: bool,
}

struct ActiveGeneration {
    id: GenerationId,
    kind: GenerationKind,
    discard_loads: bool,
}

pub(crate) struct ViewerApp {
    window: Option<Arc<Window>>,
    renderer: Option<render::Renderer>,
    engine: Engine,
    archive: Option<WorldArchive>,
    last_frame: Option<Instant>,
    accumulator: f64,
    /// Simulated seconds per real second actually reached lately (smoothed).
    reached_speed: f64,
    /// The year last seen, and the deaths and births counted when it began.
    year_start: (i64, u32, u32),
    camera: Camera,
    cursor: Option<(f64, f64)>,
    cursor_world: Option<WorldPosition>,
    inspected: Option<ChunkInspection>,
    hovered: Option<WorldPosition>,
    /// What the tooltip describes.
    hover: Option<render::Hover>,
    dragging: bool,
    selection_start: Option<WorldPosition>,
    selection: Option<WorldRect>,
    selection_validation: Option<SelectionValidation>,
    generator: WorldGenerator,
    active_generation: Option<ActiveGeneration>,
    pending_manual: Option<Vec<ChunkLoadRequest>>,
    pending_bootstrap: Option<Vec<ChunkLoadRequest>>,
    bootstrap_pager: Option<ChunkPager>,
    next_generation_id: GenerationId,
    pending_world_changes: Option<WorldRect>,
    next_world_sync: Instant,
    next_frame: Instant,
    dirty: bool,
    smoke_frames: Option<u32>,
    smoke_deadline: Option<Instant>,
    screenshot: Option<PathBuf>,
    population_status: PopulationStatus,
    gestures: GestureLog,
    feed: Feed,
    /// A short message and when it was shown.
    toast: Option<(String, Instant)>,
    /// The person whose panel is open.
    selected: Option<AgentId>,
    following: bool,
    help_open: bool,
    info_open: bool,
    details_open: bool,
    /// The build palette's chosen tool while it is open.
    build: Option<BuildTool>,
    /// Where the left button went down on the map, and whether it has dragged since.
    press: Option<((f64, f64), bool)>,
    shift: bool,
    initial_focus: Option<WorldPosition>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PopulationStatus {
    WaitingForResidency,
    Ready,
    Active,
}

impl ViewerApp {
    pub(crate) fn new(
        engine: Engine,
        archive: Option<WorldArchive>,
        smoke_frames: Option<u32>,
        screenshot: Option<PathBuf>,
    ) -> Self {
        let camera = Camera::at_origin();
        let bootstrap_focus = WorldPosition { x: 0, y: 0 };
        let bootstrap_pager = (!engine
            .world()
            .area_is_generated(engine.world().initial_bounds()))
        .then(|| {
            ChunkPager::new(engine.world().initial_bounds(), bootstrap_focus)
                .expect("validated configured bootstrap bounds create pages")
        });
        let ready = residency_ready(engine.world());
        Self {
            window: None,
            renderer: None,
            engine,
            archive,
            last_frame: None,
            reached_speed: 1.0,
            year_start: (0, 0, 0),
            accumulator: 0.0,
            camera,
            cursor: None,
            cursor_world: None,
            inspected: None,
            hovered: None,
            hover: None,
            dragging: false,
            selection_start: None,
            selection: None,
            selection_validation: None,
            generator: WorldGenerator::new(),
            active_generation: None,
            pending_manual: None,
            pending_bootstrap: None,
            bootstrap_pager,
            next_generation_id: 1,
            pending_world_changes: None,
            next_world_sync: Instant::now(),
            next_frame: Instant::now(),
            dirty: true,
            smoke_frames,
            smoke_deadline: smoke_frames.map(|_| Instant::now() + SMOKE_TIMEOUT),
            screenshot,
            population_status: if ready {
                PopulationStatus::Ready
            } else {
                PopulationStatus::WaitingForResidency
            },
            gestures: GestureLog::new(),
            feed: Feed::default(),
            toast: None,
            selected: None,
            following: false,
            // Open the help on a normal launch so first-time viewers know what they see.
            help_open: smoke_frames.is_none(),
            info_open: false,
            details_open: false,
            build: None,
            press: None,
            shift: false,
            initial_focus: None,
        }
    }

    /// Opens on a band already started by `startup::start_valley`: running, with
    /// the camera on the camp once the window exists.
    pub(crate) fn with_valley(mut self, start: ValleyStart) -> Self {
        self.population_status = PopulationStatus::Active;
        self.initial_focus = Some(start.camp);
        self
    }

    pub(crate) fn with_selected(mut self, agent: AgentId) -> Self {
        self.selected = Some(agent);
        self
    }

    fn show_toast(&mut self, message: impl Into<String>) {
        self.toast = Some((message.into(), Instant::now()));
        self.dirty = true;
    }

    fn viewport(&self, width: u32, height: u32) -> Viewport {
        Viewport {
            screen_width: width,
            screen_height: height,
            world_width: self.engine.world().width(),
            world_height: self.engine.world().height(),
        }
    }
}

const CHUNKS_APPLIED_PER_BATCH: usize = 16;
const MAX_CHUNKS_APPLIED_PER_FRAME: usize = 64;
const WORLD_APPLY_TIME_BUDGET: Duration = Duration::from_millis(2);
const FRAME_TIME: Duration = Duration::from_nanos(16_666_667);
const SMOKE_TIMEOUT: Duration = Duration::from_secs(30);
const TOAST_TIME: Duration = Duration::from_secs(3);
/// Mouse travel (pixels) that turns a click into a drag.
const DRAG_THRESHOLD: f64 = 4.0;
/// How near the mouse (pixels) a person or animal counts as under it.
const PICK_PIXELS: f64 = 10.0;
const WORLD_SYNC_INTERVAL: Duration = Duration::from_millis(125);
const ARCHIVE_DETAIL_MIN_SCALE: f64 = 0.25;
/// Cells visible across the shorter screen axis when opening on the valley camp.
const VALLEY_VIEW_SPAN_CELLS: f64 = 128.0;
