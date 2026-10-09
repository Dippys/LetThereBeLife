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
    sync::Arc,
    time::{Duration, Instant},
};

use sim_core::{ChunkInspection, ChunkLoadRequest, Engine, WorldArchive, WorldPosition, WorldRect};
use winit::window::Window;

use crate::{
    camera::{Camera, Viewport},
    generation::{ChunkPager, GenerationId, GenerationKind, WorldGenerator},
    gestures::GestureLog,
    render,
    spawn_menu::SpawnMenu,
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
    camera: Camera,
    cursor: Option<(f64, f64)>,
    cursor_world: Option<WorldPosition>,
    inspected: Option<ChunkInspection>,
    hovered: Option<WorldPosition>,
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
    population_status: PopulationStatus,
    spawn_message: Option<String>,
    spawn_menu: SpawnMenu,
    gestures: GestureLog,
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
            accumulator: 0.0,
            camera,
            cursor: None,
            cursor_world: None,
            inspected: None,
            hovered: None,
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
            population_status: if ready {
                PopulationStatus::Ready
            } else {
                PopulationStatus::WaitingForResidency
            },
            spawn_message: ready.then(|| "READY - PRESS T ON LOADED TERRAIN".to_owned()),
            spawn_menu: SpawnMenu::default(),
            gestures: GestureLog::new(),
            initial_focus: None,
        }
    }

    /// Opens on a band already started by `startup::start_valley`: running, with
    /// the camera on the camp once the window exists.
    pub(crate) fn with_valley(mut self, start: ValleyStart) -> Self {
        self.population_status = PopulationStatus::Active;
        self.spawn_message = Some(format!(
            "VALLEY {},{} TO {},{}",
            start.bounds.min.x, start.bounds.min.y, start.bounds.max.x, start.bounds.max.y
        ));
        self.initial_focus = Some(start.camp);
        self
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
const WORLD_SYNC_INTERVAL: Duration = Duration::from_millis(125);
const ARCHIVE_DETAIL_MIN_SCALE: f64 = 0.25;
/// Cells visible across the shorter screen axis when opening on the valley camp.
const VALLEY_VIEW_SPAN_CELLS: f64 = 128.0;
