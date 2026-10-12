//! Viewer application state: the `ViewerApp` struct, its construction, and shared app constants.

mod events;
mod frame;
mod input;
mod selection;
mod simulation;
mod windows;
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
    /// Until when frames are known to be running out of time.
    behind_until: Option<Instant>,
    /// The speed reached as shown, and when it was last changed.
    shown_speed: Option<(u32, Instant)>,
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
    /// Where people have walked lately (drawn as worn paths).
    wear: crate::paths::PathWear,
    /// People's last steps (drawn gliding) and family lines.
    motion: crate::motion::Motion,
    feed: Feed,
    /// A short message and when it was shown.
    toast: Option<(String, Instant)>,
    /// The person whose panel is open.
    selected: Option<AgentId>,
    following: bool,
    help_open: bool,
    info_open: bool,
    /// Open floating windows, back to front.
    windows: Vec<windows::OpenWindow>,
    /// A window being dragged: its index and where on it the mouse holds it.
    window_drag: Option<(usize, f32, f32)>,
    /// The Windows menu in the top bar is open.
    windows_menu: bool,
    /// The small map in the corner is shown.
    minimap: bool,
    /// What happens when something notable happens.
    alerts: Alerts,
    /// The camera follows whoever the latest notable event is about.
    director: bool,
    /// Notable events already alerted on.
    notable_seen: u64,
    /// When the director last cut to someone.
    director_cut: Option<Instant>,
    /// The start screen is up (and whether its settings are open).
    start: Option<bool>,
    /// How the people list is ordered.
    people_sort: crate::render::PeopleSort,
    /// How the band has done over time, for the charts.
    history: windows::History,
    /// Signs over people saying what they're doing (S).
    signs: bool,
    /// How far back N has jumped through the recent events.
    event_cursor: usize,
    details_open: bool,
    /// The build palette's chosen tool while it is open.
    build: Option<BuildTool>,
    /// Where the left button went down on the map, and whether it has dragged since.
    press: Option<((f64, f64), bool)>,
    shift: bool,
    initial_focus: Option<WorldPosition>,
    /// Cells across the first view of the valley or a save.
    view_span: f64,
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
            behind_until: None,
            shown_speed: None,
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
            wear: crate::paths::PathWear::default(),
            motion: crate::motion::Motion::default(),
            feed: Feed::default(),
            toast: None,
            selected: None,
            following: false,
            // Open the help on a normal launch so first-time viewers know what they see.
            help_open: smoke_frames.is_none(),
            info_open: false,
            windows: Vec::new(),
            window_drag: None,
            windows_menu: false,
            minimap: true,
            alerts: Alerts::Show,
            director: false,
            notable_seen: 0,
            director_cut: None,
            start: None,
            people_sort: crate::render::PeopleSort::default(),
            history: windows::History::default(),
            signs: true,
            event_cursor: 0,
            details_open: false,
            build: None,
            press: None,
            shift: false,
            initial_focus: None,
            view_span: VALLEY_VIEW_SPAN_CELLS,
        }
    }

    /// Opens on a band already started by `startup::start_valley`: running, with
    /// the camera on the camp once the window exists.
    /// Starts with `kinds` open.
    pub(crate) fn with_windows(mut self, kinds: &[crate::render::WindowKind]) -> Self {
        for &kind in kinds {
            self.open_window(kind);
        }
        self
    }

    /// Starts the view `cells` across (for close-up screenshots).
    pub(crate) fn with_view_span(mut self, cells: f64) -> Self {
        self.view_span = cells;
        self
    }

    pub(crate) fn with_valley(mut self, start: ValleyStart) -> Self {
        self.population_status = PopulationStatus::Active;
        self.initial_focus = Some(start.camp);
        self
    }

    /// Opens on the start screen.
    pub(crate) fn with_start_screen(mut self) -> Self {
        self.start = Some(false);
        self
    }

    /// The start screen as drawn.
    fn start_view(&self) -> Option<crate::render::StartView> {
        use crate::render::Setting;
        let settings_open = self.start?;
        let on_off = |on: bool| if on { "On" } else { "Off" }.to_owned();
        Some(crate::render::StartView {
            settings_open,
            save_exists: std::path::Path::new(input::QUICK_SAVE).exists(),
            settings: vec![
                (Setting::Signs, on_off(self.signs)),
                (Setting::SmallMap, on_off(self.minimap)),
                (
                    Setting::Alerts,
                    match self.alerts {
                        Alerts::Off => "Off",
                        Alerts::Show => "Say what happens",
                        Alerts::Pause => "Say it and pause",
                    }
                    .to_owned(),
                ),
                (Setting::Director, on_off(self.director)),
            ],
        })
    }

    /// A choice on the start screen.
    fn start_choice(&mut self, choice: crate::render::StartChoice) {
        use crate::render::{Setting, StartChoice};
        match choice {
            StartChoice::NewValley => match crate::startup::start_valley(&mut self.engine) {
                Ok(start) => {
                    self.start = None;
                    self.population_status = PopulationStatus::Active;
                    self.focus_on(start.camp);
                }
                Err(error) => self.show_toast(format!("No valley here: {error}")),
            },
            StartChoice::EmptyWorld => {
                self.start = None;
                self.show_toast("An empty world: press T to add a person at the mouse");
            }
            StartChoice::OpenSave => match self.open_world(std::path::Path::new(input::QUICK_SAVE))
            {
                Ok(()) => self.start = None,
                Err(error) => self.show_toast(format!("Couldn't open the quick save: {error}")),
            },
            StartChoice::Settings => self.start = Some(!self.start.unwrap_or(false)),
            StartChoice::Change(Setting::Signs) => self.signs = !self.signs,
            StartChoice::Change(Setting::SmallMap) => self.minimap = !self.minimap,
            StartChoice::Change(Setting::Alerts) => {
                self.alerts = match self.alerts {
                    Alerts::Off => Alerts::Show,
                    Alerts::Show => Alerts::Pause,
                    Alerts::Pause => Alerts::Off,
                }
            }
            StartChoice::Change(Setting::Director) => self.director = !self.director,
        }
        self.dirty = true;
    }

    /// A world opened from a save: it's already lived in, so look at its people.
    pub(crate) fn with_opened_world(mut self) -> Self {
        self.population_status = PopulationStatus::Active;
        self.initial_focus = self
            .engine
            .agent_views(usize::MAX)
            .find(|view| view.activity != sim_core::AgentActivity::Dead)
            .map(|view| view.position);
        self
    }

    /// Writes the world to `path` (folders are made as needed).
    pub(crate) fn save_world(
        &self,
        path: &std::path::Path,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(folder) = path.parent()
            && !folder.as_os_str().is_empty()
        {
            std::fs::create_dir_all(folder)?;
        }
        self.engine.save(std::fs::File::create(path)?)?;
        Ok(())
    }

    /// Replaces the running world with the one saved at `path`.
    pub(crate) fn open_world(
        &mut self,
        path: &std::path::Path,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.engine = sim_core::Engine::load(std::fs::File::open(path)?)?;
        self.feed.clear();
        self.notable_seen = 0;
        self.history.clear();
        self.gestures.clear();
        self.wear.clear();
        self.motion.clear();
        self.selected = None;
        self.following = false;
        self.accumulator = 0.0;
        self.year_start = (0, self.engine.snapshot().death_count, 0);
        self.population_status = PopulationStatus::Active;
        let someone = self
            .engine
            .agent_views(usize::MAX)
            .find(|view| view.activity != sim_core::AgentActivity::Dead)
            .map(|view| view.position);
        if let Some(position) = someone {
            self.focus_on(position);
        }
        self.dirty = true;
        Ok(())
    }

    /// Starts with `agent`'s panel open and the view on them.
    pub(crate) fn with_selected(mut self, agent: AgentId) -> Self {
        self.selected = Some(agent);
        if let Some(view) = self
            .engine
            .agent_views(usize::MAX)
            .find(|view| view.id == agent)
        {
            self.initial_focus = Some(view.position);
        }
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

/// What the viewer does when something notable happens (a death, a birth,
/// a couple, newcomers, a first hut or hearth).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Alerts {
    Off,
    /// Say what happened in a banner.
    Show,
    /// Say it and pause.
    Pause,
}
