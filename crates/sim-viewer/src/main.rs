mod camera;
mod generation;
mod renderer;
mod spawn_menu;
mod startup;

use std::{
    sync::{Arc, mpsc},
    thread,
    time::{Duration, Instant},
};

use camera::{Camera, Viewport};
use generation::{
    ChunkPager, GenerationId, GenerationJob, GenerationKind, GenerationOutcome, WorldGenerator,
};
use sim_config::{AppConfig, DEFAULT_CONFIG_PATH};
use sim_core::{
    ChunkInspection, ChunkLoadRequest, Engine, EngineCommand, GenerateAreaError,
    WORLD_GENERATION_BOUNDS, World, WorldChunkLoad, WorldPosition, WorldRect,
};
use spawn_menu::{SpawnMenu, SpawnMenuMode};
use startup::{reset, residency_ready, spawn_at};
use winit::{
    application::ApplicationHandler,
    dpi::LogicalSize,
    event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::{KeyCode, PhysicalKey},
    window::{Window, WindowAttributes, WindowId},
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let options = launch_options()?;
    let engine = Engine::new(AppConfig::load(options.config_path)?.engine_config()?);
    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Wait);
    let mut app = ViewerApp::new(engine, options.smoke_frames);
    event_loop.run_app(&mut app)?;
    Ok(())
}

struct LaunchOptions {
    config_path: String,
    smoke_frames: Option<u32>,
}

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

fn launch_options() -> Result<LaunchOptions, Box<dyn std::error::Error>> {
    let mut config_path = DEFAULT_CONFIG_PATH.to_owned();
    let mut smoke_frames = None;
    let mut args = std::env::args().skip(1);
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--config" => config_path = args.next().ok_or("--config requires a path")?,
            "--smoke-frames" => {
                let frames: u32 = args
                    .next()
                    .ok_or("--smoke-frames requires a number")?
                    .parse()?;
                if frames == 0 {
                    return Err("--smoke-frames must be greater than zero".into());
                }
                smoke_frames = Some(frames);
            }
            "--help" | "-h" => {
                println!("Usage: sim-viewer [--config PATH] [--smoke-frames NUMBER]");
                std::process::exit(0);
            }
            _ => return Err(format!("unknown argument: {argument}").into()),
        }
    }
    Ok(LaunchOptions {
        config_path,
        smoke_frames,
    })
}

struct ViewerApp {
    window: Option<Arc<Window>>,
    renderer: Option<renderer::Renderer>,
    engine: Engine,
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PopulationStatus {
    WaitingForResidency,
    Ready,
    Active,
    Failed,
}

impl ViewerApp {
    fn new(engine: Engine, smoke_frames: Option<u32>) -> Self {
        let camera = Camera::at_origin();
        let bootstrap_focus = WorldPosition { x: 0, y: 0 };
        let bootstrap_pager = ChunkPager::new(engine.world().initial_bounds(), bootstrap_focus)
            .expect("validated configured bootstrap bounds create pages");
        Self {
            window: None,
            renderer: None,
            engine,
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
            bootstrap_pager: Some(bootstrap_pager),
            next_generation_id: 1,
            pending_world_changes: None,
            next_world_sync: Instant::now(),
            next_frame: Instant::now(),
            dirty: true,
            smoke_frames,
            smoke_deadline: smoke_frames.map(|_| Instant::now() + SMOKE_TIMEOUT),
            population_status: PopulationStatus::WaitingForResidency,
            spawn_message: None,
            spawn_menu: SpawnMenu::default(),
        }
    }
}

impl ViewerApp {
    fn create_window(&mut self, event_loop: &ActiveEventLoop) {
        let attributes = WindowAttributes::default()
            .with_title("Let There Be Life")
            .with_inner_size(LogicalSize::new(960, 540))
            .with_min_inner_size(LogicalSize::new(640, 360))
            .with_visible(self.smoke_frames.is_none());
        let window = Arc::new(event_loop.create_window(attributes).expect("create window"));
        self.renderer = Some(
            renderer::Renderer::new(window.clone(), self.engine.world())
                .expect("initialize GPU renderer"),
        );
        self.window = Some(window);
        self.last_frame = Some(Instant::now());
    }

    fn update(&mut self) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_frame.replace(now).unwrap_or(now));
        let snapshot = self.engine.snapshot();
        if self.population_status == PopulationStatus::Active && !snapshot.paused {
            self.accumulator += elapsed.as_secs_f64().min(0.25) * f64::from(snapshot.speed);
            let tick_seconds = self.engine.config().tick_duration().as_secs_f64();
            while self.accumulator >= tick_seconds {
                self.engine.tick();
                self.accumulator -= tick_seconds;
            }
        }
    }

    fn render(&mut self) -> bool {
        let selection_valid = self.selection_preview_is_valid();
        let generation_status = self.generation_status();
        let hovered_agent = self.hovered.and_then(|position| {
            self.engine
                .agent_views(startup::VIEWER_AGENT_LIMIT)
                .filter(|agent| agent.position == position)
                .reduce(|selected, candidate| {
                    if selected.activity == sim_core::AgentActivity::Dead
                        && candidate.activity != sim_core::AgentActivity::Dead
                    {
                        candidate
                    } else {
                        selected
                    }
                })
                .map(|view| renderer::AgentInspection {
                    view,
                    needs: self.engine.physical_needs(view.id).ok(),
                    inventory: self.engine.inventory(view.id),
                    health: self.engine.health(view.id),
                    policy: self.engine.physical_policy(view.id),
                    sleep: self.engine.sleep(view.id),
                    death: self
                        .engine
                        .death_records()
                        .iter()
                        .find(|record| record.agent == view.id)
                        .copied(),
                })
        });
        let (Some(window), Some(renderer)) = (&self.window, &mut self.renderer) else {
            return false;
        };
        let size = window.inner_size();
        if size.width == 0 || size.height == 0 {
            return false;
        }
        let allow_world_sync = self
            .pending_world_changes
            .is_some_and(|_| Instant::now() >= self.next_world_sync);
        let result = renderer.render(
            &self.engine,
            renderer::RenderState {
                snapshot: self.engine.snapshot(),
                camera: self.camera,
                ui_scale: window.scale_factor() as f32,
                cursor_world: self.cursor_world,
                cursor_spawned_object: self
                    .cursor_world
                    .and_then(|position| self.engine.spawned_object_at(position)),
                inspected: self.inspected,
                hovered: self.hovered,
                selection: self.selection,
                selection_valid,
                generation_status,
                population_status: match self.population_status {
                    PopulationStatus::WaitingForResidency => renderer::PopulationStatus::Waiting,
                    PopulationStatus::Ready => renderer::PopulationStatus::Ready,
                    PopulationStatus::Active => renderer::PopulationStatus::Active,
                    PopulationStatus::Failed => renderer::PopulationStatus::Failed,
                },
                hovered_agent,
                spawn_message: self.spawn_message.clone(),
                spawn_menu: (self.spawn_menu.mode() != SpawnMenuMode::Closed).then_some(
                    renderer::SpawnMenuView {
                        selected: self.spawn_menu.selected(),
                        placing: self.spawn_menu.is_placing(),
                    },
                ),
            },
            allow_world_sync,
            self.pending_world_changes,
        );
        match result {
            Ok(()) => {
                if allow_world_sync {
                    self.pending_world_changes = None;
                    self.next_world_sync = Instant::now() + WORLD_SYNC_INTERVAL;
                }
                true
            }
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                renderer.resize(size.width, size.height);
                self.dirty = true;
                false
            }
            Err(wgpu::SurfaceError::OutOfMemory) => std::process::exit(1),
            Err(wgpu::SurfaceError::Timeout | wgpu::SurfaceError::Other) => {
                self.dirty = true;
                false
            }
        }
    }

    fn render_smoke_frame_if_ready(&mut self, event_loop: &ActiveEventLoop) -> bool {
        let Some(frames) = self.smoke_frames else {
            return false;
        };
        if matches!(
            self.population_status,
            PopulationStatus::WaitingForResidency | PopulationStatus::Failed
        ) {
            return false;
        }
        assert!(
            self.render(),
            "GPU smoke frame failed after streamed terrain arrived"
        );
        if frames == 1 {
            self.smoke_frames = None;
            self.smoke_deadline = None;
            event_loop.exit();
        } else {
            self.smoke_frames = Some(frames - 1);
            self.dirty = true;
            self.next_frame = Instant::now();
        }
        true
    }

    fn run_smoke(&mut self, event_loop: &ActiveEventLoop) {
        self.schedule_generation();
        while self.smoke_frames.is_some() {
            self.poll_generation();
            self.schedule_generation();
            if self.render_smoke_frame_if_ready(event_loop) {
                continue;
            }
            if self
                .smoke_deadline
                .is_some_and(|deadline| Instant::now() >= deadline)
            {
                panic!("viewer smoke timed out before rendering streamed terrain");
            }
            thread::sleep(Duration::from_millis(1));
        }
    }

    fn selection_preview_is_valid(&mut self) -> bool {
        let Some(bounds) = self.selection else {
            self.selection_validation = None;
            return true;
        };
        let world_revision = self.engine.world().revision();
        if let Some(cached) = self.selection_validation
            && cached.bounds == bounds
            && cached.world_revision == world_revision
        {
            return cached.valid;
        }
        let valid = selection_is_valid(self.engine.world(), Some(bounds));
        self.selection_validation = Some(SelectionValidation {
            bounds,
            world_revision,
            valid,
        });
        valid
    }

    fn poll_generation(&mut self) -> bool {
        let Some(active) = &self.active_generation else {
            return false;
        };
        let id = active.id;
        let discard_loads = active.discard_loads;
        let started = Instant::now();
        let mut applied_loads = 0;
        let mut changed_bounds = None;
        let mut outcome = None;
        let mut changed = false;

        loop {
            let remaining = MAX_CHUNKS_APPLIED_PER_FRAME.saturating_sub(applied_loads);
            if remaining == 0 {
                break;
            }
            let limit = CHUNKS_APPLIED_PER_BATCH.min(remaining);
            let poll = self.generator.drain(id, limit, discard_loads);
            let received = poll.loads.len();
            if received > 0 {
                let batch_bounds = union_load_bounds(&poll.loads);
                match self.engine.apply_world_chunk_loads(poll.loads) {
                    Ok(inserted) if inserted > 0 => {
                        if let Some(bounds) = batch_bounds {
                            changed_bounds = Some(match changed_bounds {
                                Some(previous) => union_bounds(previous, bounds),
                                None => bounds,
                            });
                        }
                        changed = true;
                        self.update_residency_status();
                    }
                    Ok(_) => {}
                    Err(error) => {
                        eprintln!("could not apply generated chunks: {error}");
                        self.cancel_active_generation();
                        changed = true;
                        break;
                    }
                }
                applied_loads += received;
            }
            if poll.outcome.is_some() {
                outcome = poll.outcome;
                break;
            }
            if received < limit || discard_loads || started.elapsed() >= WORLD_APPLY_TIME_BUDGET {
                break;
            }
        }

        if let Some(bounds) = changed_bounds {
            self.mark_world_changed(bounds);
            self.update_hover();
        }

        if let Some(outcome) = outcome {
            self.active_generation
                .take()
                .expect("worker outcomes always belong to an active job");
            if matches!(outcome, GenerationOutcome::WorkerStopped) {
                eprintln!("world generation worker stopped unexpectedly");
                self.update_hover();
            }
            if self.pending_world_changes.is_some() {
                self.next_world_sync = Instant::now();
            }
            changed = true;
        }
        changed
    }

    fn update_hover(&mut self) {
        let Some(window) = self.window.clone() else {
            return;
        };
        let size = window.inner_size();
        let position = self.cursor.map(|(x, y)| {
            self.camera
                .screen_to_world_position(x, y, self.viewport(size.width, size.height))
        });
        self.cursor_world = position;
        self.inspected =
            position.and_then(|position| self.engine.world().inspect_chunk_at(position).ok());
        self.hovered = position.filter(|position| self.engine.world().cell(*position).is_some());
    }

    fn update_residency_status(&mut self) {
        if self.population_status != PopulationStatus::WaitingForResidency {
            return;
        }
        match residency_ready(self.engine.world()) {
            Ok(false) => {}
            Ok(true) => {
                self.population_status = PopulationStatus::Ready;
                self.spawn_message = Some("READY - MOVE CURSOR AND PRESS T".to_owned());
                self.dirty = true;
            }
            Err(error) => {
                eprintln!("viewer residency gate failed: {error}");
                self.population_status = PopulationStatus::Failed;
                self.dirty = true;
            }
        }
    }

    fn reset_population(&mut self) {
        reset(&mut self.engine);
        self.population_status = PopulationStatus::WaitingForResidency;
        self.spawn_message = Some("POPULATION RESET".to_owned());
        self.update_residency_status();
        self.accumulator = 0.0;
        self.last_frame = Some(Instant::now());
        self.next_frame = Instant::now();
    }

    fn spawn_agent_at_cursor(&mut self) {
        let Some(position) = self.cursor_world else {
            self.spawn_message = Some("SPAWN FAILED - MOVE CURSOR OVER MAP".to_owned());
            return;
        };
        match spawn_at(&mut self.engine, position) {
            Ok(agent) => {
                self.population_status = PopulationStatus::Active;
                self.spawn_message = Some(format!(
                    "SPAWNED AGENT {} AT {},{}",
                    agent.get(),
                    position.x,
                    position.y
                ));
                self.accumulator = 0.0;
                self.last_frame = Some(Instant::now());
            }
            Err(error) => {
                eprintln!("{error}");
                self.spawn_message = Some(format!("SPAWN FAILED - {error}"));
            }
        }
    }

    fn spawn_object_at_cursor(&mut self) {
        let Some(position) = self.cursor_world else {
            self.spawn_message = Some("PLACE FAILED - MOVE CURSOR OVER MAP".to_owned());
            return;
        };
        let kind = self.spawn_menu.selected();
        match self.engine.spawn_object(kind, position) {
            Ok(()) => {
                self.spawn_message =
                    Some(format!("PLACED {kind:?} AT {},{}", position.x, position.y));
            }
            Err(error) => {
                self.spawn_message = Some(format!("PLACE FAILED - {error}"));
            }
        }
    }

    fn generation_status(&self) -> renderer::GenerationStatus {
        if !self.generator.is_available() {
            return renderer::GenerationStatus::WorkerUnavailable;
        }
        if self
            .active_generation
            .as_ref()
            .is_some_and(|active| active.discard_loads)
        {
            return renderer::GenerationStatus::Cancelling;
        }
        let kind = self
            .active_generation
            .as_ref()
            .map(|active| active.kind)
            .or_else(|| self.pending_manual.as_ref().map(|_| GenerationKind::Manual))
            .or_else(|| {
                (self.pending_bootstrap.is_some() || self.bootstrap_pager.is_some())
                    .then_some(GenerationKind::Bootstrap)
            });
        match kind {
            Some(GenerationKind::Manual) => renderer::GenerationStatus::Manual,
            Some(GenerationKind::Bootstrap) => renderer::GenerationStatus::Bootstrap,
            None => renderer::GenerationStatus::Idle,
        }
    }

    fn schedule_generation(&mut self) -> bool {
        if self.active_generation.is_some() || !self.generator.is_available() {
            return false;
        }
        if let Some(requests) = self.pending_manual.take() {
            return self.start_generation(GenerationKind::Manual, requests);
        }
        if let Some(requests) = self.pending_bootstrap.take() {
            return self.start_generation(GenerationKind::Bootstrap, requests);
        }
        match self.take_next_bootstrap_requests() {
            Ok(Some(requests)) => self.start_generation(GenerationKind::Bootstrap, requests),
            Ok(None) => false,
            Err(error) => {
                eprintln!("bootstrap generation paused: {error}");
                self.bootstrap_pager = None;
                true
            }
        }
    }

    fn take_next_bootstrap_requests(
        &mut self,
    ) -> Result<Option<Vec<ChunkLoadRequest>>, GenerateAreaError> {
        let requests = self
            .bootstrap_pager
            .as_mut()
            .map(|pager| pager.next_requests(self.engine.world()))
            .transpose()?
            .flatten();
        if requests.is_none() {
            self.bootstrap_pager = None;
        }
        Ok(requests)
    }

    fn start_generation(&mut self, kind: GenerationKind, requests: Vec<ChunkLoadRequest>) -> bool {
        let id = self.next_generation_id;
        self.next_generation_id = self.next_generation_id.saturating_add(1).max(1);
        let job = GenerationJob {
            id,
            seed: self.engine.config().seed,
            requests,
        };
        match self.generator.request(job) {
            Ok(()) => {
                self.active_generation = Some(ActiveGeneration {
                    id,
                    kind,
                    discard_loads: false,
                });
                true
            }
            Err(mpsc::TrySendError::Full(job)) => {
                self.requeue_generation(kind, job.requests);
                false
            }
            Err(mpsc::TrySendError::Disconnected(job)) => {
                self.requeue_generation(kind, job.requests);
                eprintln!("world generation worker stopped before accepting request");
                self.update_hover();
                true
            }
        }
    }

    fn requeue_generation(&mut self, kind: GenerationKind, requests: Vec<ChunkLoadRequest>) {
        let slot = match kind {
            GenerationKind::Manual => &mut self.pending_manual,
            GenerationKind::Bootstrap => &mut self.pending_bootstrap,
        };
        debug_assert!(slot.is_none(), "only one unsent page per priority exists");
        *slot = Some(requests);
    }

    fn generation_is_pending(&self) -> bool {
        self.generator.is_available()
            && (self.pending_manual.is_some()
                || self.pending_bootstrap.is_some()
                || self.bootstrap_pager.is_some())
    }

    fn cancel_active_generation(&mut self) {
        if let Some(active) = &mut self.active_generation
            && !active.discard_loads
        {
            active.discard_loads = true;
            self.generator.cancel(active.id);
        }
    }

    fn cancel_background_generation(&mut self) {
        if self
            .active_generation
            .as_ref()
            .is_some_and(|active| !matches!(active.kind, GenerationKind::Manual))
        {
            self.cancel_active_generation();
        }
    }

    fn cancel_pending_generation(&mut self) {
        self.cancel_active_generation();
        self.pending_manual = None;
        self.pending_bootstrap = None;
        self.bootstrap_pager = None;
        self.selection_start = None;
        self.selection = None;
        self.selection_validation = None;
        self.update_hover();
    }

    fn mark_world_changed(&mut self, bounds: WorldRect) {
        self.pending_world_changes = Some(match self.pending_world_changes {
            Some(previous) => union_bounds(previous, bounds),
            None => bounds,
        });
        self.dirty = true;
    }

    fn zoom(&mut self, delta: MouseScrollDelta) {
        let (Some(window), Some((cursor_x, cursor_y))) = (&self.window, self.cursor) else {
            return;
        };
        let steps = match delta {
            MouseScrollDelta::LineDelta(_, lines) => f64::from(lines),
            MouseScrollDelta::PixelDelta(position) => position.y / 120.0,
        };
        let size = window.inner_size();
        self.camera.zoom_at(
            steps,
            (cursor_x, cursor_y),
            Viewport {
                screen_width: size.width,
                screen_height: size.height,
                world_width: self.engine.world().width(),
                world_height: self.engine.world().height(),
            },
        );
        self.update_hover();
    }

    fn viewport(&self, width: u32, height: u32) -> Viewport {
        Viewport {
            screen_width: width,
            screen_height: height,
            world_width: self.engine.world().width(),
            world_height: self.engine.world().height(),
        }
    }

    fn handle_key(&mut self, code: KeyCode, event_loop: &ActiveEventLoop) {
        if self.spawn_menu.handle_key(code) {
            self.dragging = false;
            self.spawn_message = match self.spawn_menu.mode() {
                SpawnMenuMode::Closed => Some("SPAWN MODE CLOSED".to_owned()),
                SpawnMenuMode::Browsing => Some("SPAWN MENU - NUMPAD 2/8 THEN 5".to_owned()),
                SpawnMenuMode::Placing => Some(format!(
                    "PLACING {:?} - LEFT CLICK, NUMPAD 0 TO END",
                    self.spawn_menu.selected()
                )),
            };
            return;
        }
        let command = match code {
            KeyCode::Space => Some(EngineCommand::TogglePause),
            KeyCode::Digit1 => Some(EngineCommand::SetSpeed(1.0)),
            KeyCode::Digit2 => Some(EngineCommand::SetSpeed(2.0)),
            KeyCode::Digit3 => Some(EngineCommand::SetSpeed(4.0)),
            KeyCode::Digit4 => Some(EngineCommand::SetSpeed(8.0)),
            KeyCode::Digit5 => Some(EngineCommand::SetSpeed(16.0)),
            KeyCode::Digit6 => Some(EngineCommand::SetSpeed(32.0)),
            KeyCode::Digit7 => Some(EngineCommand::SetSpeed(64.0)),
            KeyCode::Digit8 => Some(EngineCommand::SetSpeed(128.0)),
            KeyCode::Digit9 => Some(EngineCommand::SetSpeed(256.0)),
            KeyCode::KeyR => {
                self.reset_population();
                None
            }
            KeyCode::KeyT => {
                self.spawn_agent_at_cursor();
                None
            }
            KeyCode::KeyC => {
                self.cancel_pending_generation();
                None
            }
            KeyCode::Escape => {
                event_loop.exit();
                None
            }
            _ => None,
        };
        if let Some(command) = command {
            self.engine.command(command);
            if matches!(
                command,
                EngineCommand::TogglePause | EngineCommand::SetPaused(_)
            ) {
                self.last_frame = Some(Instant::now());
                self.accumulator = 0.0;
                self.next_frame = Instant::now();
            }
        }
    }
}

impl ApplicationHandler for ViewerApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_none() {
            self.create_window(event_loop);
        }
        if self.smoke_frames.is_some() {
            self.run_smoke(event_loop);
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        if self.window.as_ref().is_none_or(|window| window.id() != id) {
            return;
        }
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::RedrawRequested => {
                if !self.render_smoke_frame_if_ready(event_loop) {
                    self.render();
                }
            }
            WindowEvent::Resized(size) => {
                if let Some(renderer) = &mut self.renderer {
                    renderer.resize(size.width, size.height);
                }
                self.camera
                    .constrain_to_viewport(self.viewport(size.width, size.height));
                self.dirty = true;
            }
            WindowEvent::CursorMoved { position, .. } => {
                if self.dragging
                    && let Some((previous_x, previous_y)) = self.cursor
                {
                    if let Some(window) = &self.window {
                        let size = window.inner_size();
                        self.camera.pan_by_screen_delta(
                            position.x - previous_x,
                            position.y - previous_y,
                            self.viewport(size.width, size.height),
                        );
                    }
                }
                self.cursor = Some((position.x, position.y));
                if let Some(start) = self.selection_start {
                    let size = self.window.as_ref().expect("window exists").inner_size();
                    let current = self.camera.screen_to_world_position(
                        position.x,
                        position.y,
                        self.viewport(size.width, size.height),
                    );
                    self.selection = bounded_selection(start, current);
                }
                self.update_hover();
                self.dirty = true;
            }
            WindowEvent::CursorLeft { .. } => {
                self.cursor = None;
                self.cursor_world = None;
                self.inspected = None;
                self.hovered = None;
                self.dragging = false;
                self.selection_start = None;
                self.selection = None;
                self.dirty = true;
            }
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => {
                if self.spawn_menu.is_placing() {
                    self.dragging = false;
                    if state == ElementState::Pressed {
                        self.spawn_object_at_cursor();
                    }
                } else {
                    self.dragging = state == ElementState::Pressed;
                }
                self.dirty = true;
            }
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Right,
                ..
            } => {
                match state {
                    ElementState::Pressed => {
                        if self.generator.is_available()
                            && !self
                                .active_generation
                                .as_ref()
                                .is_some_and(|active| matches!(active.kind, GenerationKind::Manual))
                            && let (Some(window), Some((x, y))) = (&self.window, self.cursor)
                        {
                            let size = window.inner_size();
                            let start = self.camera.screen_to_world_position(
                                x,
                                y,
                                self.viewport(size.width, size.height),
                            );
                            if let Some(selection) = bounded_selection(start, start) {
                                self.selection_start = Some(start);
                                self.selection = Some(selection);
                            }
                        }
                    }
                    ElementState::Released => {
                        if let Some(bounds) = self.selection.take() {
                            match self.engine.world().missing_chunk_load_requests(bounds) {
                                Ok(requests) if !requests.is_empty() => {
                                    self.pending_manual = Some(requests);
                                    self.cancel_background_generation();
                                }
                                Ok(_) => {}
                                Err(error) => eprintln!("world generation rejected: {error}"),
                            }
                        }
                        self.selection_start = None;
                        self.update_hover();
                    }
                }
                self.dirty = true;
            }
            WindowEvent::MouseWheel { delta, .. } => {
                self.zoom(delta);
                self.dirty = true;
            }
            WindowEvent::KeyboardInput { event, .. }
                if event.state == ElementState::Pressed && !event.repeat =>
            {
                if let PhysicalKey::Code(code) = event.physical_key {
                    self.handle_key(code, event_loop);
                    self.dirty = true;
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let frame_due = Instant::now() >= self.next_frame;
        let previous_tick = self.engine.snapshot().tick;
        // Worker arrivals only populate presentation-demanded materialization
        // after this event-loop turn's fixed simulation phase. Current
        // simulation state therefore never observes arrival timing mid-tick.
        self.update();
        if frame_due {
            self.dirty |= self.poll_generation();
            self.dirty |= self.schedule_generation();
        }
        let snapshot = self.engine.snapshot();
        let now = Instant::now();
        if self.smoke_deadline.is_some_and(|deadline| now >= deadline) {
            panic!("viewer smoke timed out before rendering streamed terrain");
        }
        if frame_due {
            self.render_smoke_frame_if_ready(event_loop);
        }
        let sync_due = self
            .pending_world_changes
            .is_some_and(|_| now >= self.next_world_sync);
        let needs_redraw = self.dirty || snapshot.tick != previous_tick || sync_due;
        if needs_redraw
            && now >= self.next_frame
            && let Some(window) = &self.window
        {
            window.request_redraw();
            self.dirty = false;
            self.next_frame = now + FRAME_TIME;
        }
        if !snapshot.paused
            || self.active_generation.is_some()
            || self.generation_is_pending()
            || self.pending_world_changes.is_some()
            || self.dirty
        {
            if self.next_frame <= now {
                self.next_frame = now + FRAME_TIME;
            }
            event_loop.set_control_flow(ControlFlow::WaitUntil(self.next_frame));
        } else {
            event_loop.set_control_flow(ControlFlow::Wait);
        }
    }
}

fn selection_is_valid(world: &World, selection: Option<WorldRect>) -> bool {
    selection.is_none_or(|bounds| world.validate_generation_request(bounds).is_ok())
}

fn bounded_selection(start: WorldPosition, current: WorldPosition) -> Option<WorldRect> {
    WORLD_GENERATION_BOUNDS
        .contains(start)
        .then(|| WorldRect::from_inclusive_points(start, current))?
        .intersection(WORLD_GENERATION_BOUNDS)
}

const CHUNKS_APPLIED_PER_BATCH: usize = 16;
const MAX_CHUNKS_APPLIED_PER_FRAME: usize = 64;
const WORLD_APPLY_TIME_BUDGET: Duration = Duration::from_millis(2);
const FRAME_TIME: Duration = Duration::from_nanos(16_666_667);
const SMOKE_TIMEOUT: Duration = Duration::from_secs(30);
const WORLD_SYNC_INTERVAL: Duration = Duration::from_millis(125);

fn union_load_bounds(loads: &[WorldChunkLoad]) -> Option<WorldRect> {
    loads
        .iter()
        .map(WorldChunkLoad::bounds)
        .reduce(union_bounds)
}

fn union_bounds(left: WorldRect, right: WorldRect) -> WorldRect {
    WorldRect {
        min: WorldPosition {
            x: left.min.x.min(right.min.x),
            y: left.min.y.min(right.min.y),
        },
        max: WorldPosition {
            x: left.max.x.max(right.max.x),
            y: left.max.y.max(right.max.y),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_core::{
        CHUNK_SIZE, ChunkPresence, EngineConfig, MAX_CHUNKS_PER_GENERATION, WorldConfig,
    };

    fn engine_with_world(width: u32, height: u32) -> Engine {
        Engine::new(EngineConfig {
            seed: 7,
            ticks_per_second: 60,
            world: WorldConfig::new(width, height).expect("small test world is valid"),
        })
    }

    #[test]
    fn streamed_changes_coalesce_into_one_dirty_region() {
        let mut app = ViewerApp::new(engine_with_world(64, 64), None);
        app.dirty = false;
        app.mark_world_changed(WorldRect {
            min: WorldPosition { x: -32, y: 8 },
            max: WorldPosition { x: 4, y: 72 },
        });
        app.mark_world_changed(WorldRect {
            min: WorldPosition { x: 2, y: -4 },
            max: WorldPosition { x: 96, y: 12 },
        });

        assert!(app.dirty);
        assert_eq!(
            app.pending_world_changes,
            Some(WorldRect {
                min: WorldPosition { x: -32, y: -4 },
                max: WorldPosition { x: 96, y: 72 },
            })
        );
    }

    #[test]
    fn bootstrap_pager_materializes_once_then_releases_its_queue() {
        let mut app = ViewerApp::new(engine_with_world(64, 64), None);
        let requests = app
            .take_next_bootstrap_requests()
            .expect("bootstrap request is valid")
            .expect("unloaded bootstrap area has work");
        assert_eq!(requests.len(), 4);
        assert!(requests.iter().all(|request| {
            app.engine
                .world()
                .initial_bounds()
                .contains_rect(request.bounds())
        }));

        let seed = app.engine.config().seed;
        let loads = requests
            .into_iter()
            .map(|request| World::generate_chunk_load(seed, request))
            .collect();
        assert_eq!(app.engine.apply_world_chunk_loads(loads), Ok(4));

        assert_eq!(
            app.take_next_bootstrap_requests()
                .expect("loaded bootstrap page is valid"),
            None
        );
        assert!(app.bootstrap_pager.is_none());
    }

    #[test]
    fn manual_generation_preempts_bootstrap_paging() {
        let mut app = ViewerApp::new(engine_with_world(64, 64), None);
        let manual_bounds = WorldRect {
            min: WorldPosition { x: 64, y: 0 },
            max: WorldPosition { x: 128, y: 64 },
        };
        app.pending_manual = Some(
            app.engine
                .world()
                .missing_chunk_load_requests(manual_bounds)
                .expect("manual selection is valid"),
        );

        assert!(app.schedule_generation());
        assert!(matches!(
            app.active_generation.as_ref().map(|active| active.kind),
            Some(GenerationKind::Manual)
        ));
        assert!(app.pending_manual.is_none());

        app.cancel_active_generation();
        assert!(
            app.active_generation
                .as_ref()
                .is_some_and(|active| active.discard_loads)
        );
    }

    #[test]
    fn no_generation_is_scheduled_without_bootstrap_or_manual_work() {
        let mut app = ViewerApp::new(engine_with_world(64, 64), None);
        app.bootstrap_pager = None;

        assert!(!app.schedule_generation());
        assert!(app.active_generation.is_none());
        assert!(!app.generation_is_pending());
    }

    #[test]
    fn requeued_bootstrap_page_is_preserved_until_higher_priority_manual_work_runs() {
        let mut app = ViewerApp::new(engine_with_world(64, 64), None);
        let bootstrap = app
            .engine
            .world()
            .missing_chunk_load_requests(app.engine.world().initial_bounds())
            .expect("bootstrap page is valid");
        let manual_bounds = WorldRect {
            min: WorldPosition { x: 64, y: 0 },
            max: WorldPosition { x: 128, y: 64 },
        };
        let manual = app
            .engine
            .world()
            .missing_chunk_load_requests(manual_bounds)
            .expect("manual selection is valid");

        app.requeue_generation(GenerationKind::Bootstrap, bootstrap);
        app.pending_manual = Some(manual);

        assert!(app.schedule_generation());
        assert!(matches!(
            app.active_generation.as_ref().map(|active| active.kind),
            Some(GenerationKind::Manual)
        ));
        assert!(app.pending_bootstrap.is_some());
    }

    #[test]
    fn background_cancellation_never_discards_manual_generation() {
        let mut app = ViewerApp::new(engine_with_world(64, 64), None);
        app.active_generation = Some(ActiveGeneration {
            id: 10,
            kind: GenerationKind::Manual,
            discard_loads: false,
        });
        app.cancel_background_generation();
        assert!(
            app.active_generation
                .as_ref()
                .is_some_and(|active| !active.discard_loads)
        );

        app.active_generation = Some(ActiveGeneration {
            id: 11,
            kind: GenerationKind::Bootstrap,
            discard_loads: false,
        });
        app.cancel_background_generation();
        assert!(
            app.active_generation
                .as_ref()
                .is_some_and(|active| active.discard_loads)
        );
    }

    #[test]
    fn cancellation_clears_an_active_right_drag_before_it_can_queue_manual_work() {
        let mut app = ViewerApp::new(engine_with_world(64, 64), None);
        let selection = WorldRect {
            min: WorldPosition { x: 0, y: 0 },
            max: WorldPosition { x: 64, y: 64 },
        };
        app.active_generation = Some(ActiveGeneration {
            id: 12,
            kind: GenerationKind::Bootstrap,
            discard_loads: false,
        });
        app.pending_manual = Some(Vec::new());
        app.pending_bootstrap = Some(Vec::new());
        app.selection_start = Some(selection.min);
        app.selection = Some(selection);
        app.selection_validation = Some(SelectionValidation {
            bounds: selection,
            world_revision: 0,
            valid: true,
        });

        app.cancel_pending_generation();

        assert!(
            app.active_generation
                .as_ref()
                .is_some_and(|active| active.discard_loads)
        );
        assert!(app.pending_manual.is_none());
        assert!(app.pending_bootstrap.is_none());
        assert!(app.selection_start.is_none());
        assert!(app.selection.is_none());
        assert!(app.selection_validation.is_none());
        assert!(app.bootstrap_pager.is_none());
    }

    #[test]
    fn streamed_load_bounds_accumulate_exact_clipped_bootstrap_coverage() {
        let world = World::new(7, WorldConfig::new(96, 64).expect("test world is valid"));
        let expected = world.initial_bounds();
        let loads = world
            .missing_chunk_load_requests(expected)
            .expect("bootstrap request is valid")
            .into_iter()
            .map(|request| World::generate_chunk_load(7, request))
            .collect::<Vec<_>>();

        assert_eq!(union_load_bounds(&loads), Some(expected));
        assert_eq!(
            union_bounds(
                WorldRect {
                    min: WorldPosition { x: -32, y: 8 },
                    max: WorldPosition { x: 4, y: 72 },
                },
                WorldRect {
                    min: WorldPosition { x: 2, y: -4 },
                    max: WorldPosition { x: 96, y: 12 },
                },
            ),
            WorldRect {
                min: WorldPosition { x: -32, y: -4 },
                max: WorldPosition { x: 96, y: 72 },
            }
        );
    }

    #[test]
    fn selection_validation_and_inspection_distinguish_unloaded_bootstrap_tiles() {
        let world = World::new(7, WorldConfig::new(96, 64).expect("test world is valid"));
        let oversized = WorldRect {
            min: WorldPosition { x: 128, y: 0 },
            max: WorldPosition {
                x: 192,
                y: (MAX_CHUNKS_PER_GENERATION as i64 + 1) * CHUNK_SIZE,
            },
        };

        assert!(selection_is_valid(&world, Some(world.initial_bounds())));
        assert!(!selection_is_valid(&world, Some(oversized)));

        let inspection = world
            .inspect_chunk_at(WorldPosition { x: 47, y: 0 })
            .expect("configured position is inspectable");
        assert_eq!(inspection.presence, ChunkPresence::PartialInitialUnloaded);
        assert!(world.cell(WorldPosition { x: 47, y: 0 }).is_none());
    }

    #[test]
    fn right_drag_selection_caps_at_the_world_boundary() {
        let world = World::new(7, WorldConfig::new(64, 64).expect("test world is valid"));
        let start = WorldPosition {
            x: WORLD_GENERATION_BOUNDS.max.x - 2,
            y: WORLD_GENERATION_BOUNDS.min.y + 2,
        };
        let expected = WorldRect {
            min: WorldPosition {
                x: WORLD_GENERATION_BOUNDS.max.x - 2,
                y: WORLD_GENERATION_BOUNDS.min.y,
            },
            max: WorldPosition {
                x: WORLD_GENERATION_BOUNDS.max.x,
                y: WORLD_GENERATION_BOUNDS.min.y + 3,
            },
        };

        assert_eq!(
            bounded_selection(
                start,
                WorldPosition {
                    x: WORLD_GENERATION_BOUNDS.max.x + 1_000,
                    y: WORLD_GENERATION_BOUNDS.min.y - 1_000,
                },
            ),
            Some(expected)
        );
        assert!(world.missing_chunk_load_requests(expected).is_ok());
    }

    #[test]
    fn right_drag_selection_does_not_start_outside_the_world_boundary() {
        assert_eq!(
            bounded_selection(
                WorldPosition {
                    x: WORLD_GENERATION_BOUNDS.max.x,
                    y: 0,
                },
                WorldPosition { x: 0, y: 0 },
            ),
            None
        );
    }
}
