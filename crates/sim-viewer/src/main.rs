mod camera;
mod renderer;

use std::{
    cell::Cell,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    thread,
    time::{Duration, Instant},
};

use camera::{Camera, Viewport};
use sim_config::{AppConfig, DEFAULT_CONFIG_PATH};
use sim_core::{
    ChunkCoord, ChunkInspection, ChunkPresence, Engine, EngineCommand, GenerateAreaError, World,
    WorldChunk, WorldPosition, WorldRect,
};
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
    inspected: Option<ChunkInspection>,
    hovered: Option<WorldPosition>,
    dragging: bool,
    selection_start: Option<WorldPosition>,
    selection: Option<WorldRect>,
    selection_validation: Option<SelectionValidation>,
    generator: WorldGenerator,
    generation_pending: bool,
    automatic_generation_needed: bool,
    automatic_generation_blocked: Option<GenerateAreaError>,
    world_sync_ready: bool,
    next_frame: Instant,
    dirty: bool,
    smoke_frames: Option<u32>,
}

impl ViewerApp {
    fn new(engine: Engine, smoke_frames: Option<u32>) -> Self {
        let camera = Camera::centered(engine.world().width(), engine.world().height());
        Self {
            window: None,
            renderer: None,
            engine,
            last_frame: None,
            accumulator: 0.0,
            camera,
            cursor: None,
            inspected: None,
            hovered: None,
            dragging: false,
            selection_start: None,
            selection: None,
            selection_validation: None,
            generator: WorldGenerator::new(),
            generation_pending: false,
            automatic_generation_needed: true,
            automatic_generation_blocked: None,
            world_sync_ready: true,
            next_frame: Instant::now(),
            dirty: true,
            smoke_frames,
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
        if !snapshot.paused {
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
        let (Some(window), Some(renderer)) = (&self.window, &mut self.renderer) else {
            return false;
        };
        let size = window.inner_size();
        if size.width == 0 || size.height == 0 {
            return false;
        }
        let allow_world_sync = self.world_sync_ready;
        let result = renderer.render(
            self.engine.world(),
            renderer::RenderState {
                snapshot: self.engine.snapshot(),
                camera: self.camera,
                inspected: self.inspected,
                hovered: self.hovered,
                selection: self.selection,
                selection_valid,
            },
            allow_world_sync,
        );
        if allow_world_sync {
            self.world_sync_ready = false;
        }
        match result {
            Ok(()) => true,
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
        let GenerationPoll {
            chunks,
            outcome,
            mut changed,
        } = self.generator.drain(MAX_CHUNKS_APPLIED_PER_FRAME);
        if let Some(outcome) = outcome {
            self.generation_pending = false;
            self.world_sync_ready = true;
            self.automatic_generation_needed =
                automatic_generation_needed_after(self.automatic_generation_needed, &outcome);
            match outcome {
                GenerationOutcome::Failed(error) => {
                    eprintln!("world generation failed: {error}");
                }
                GenerationOutcome::WorkerStopped => {
                    eprintln!("world generation worker stopped unexpectedly");
                }
                GenerationOutcome::Completed | GenerationOutcome::Cancelled => {}
            }
        }
        if !chunks.is_empty() {
            if let Err(error) = self.engine.apply_world_chunks(chunks) {
                eprintln!("could not apply generated chunks: {error}");
                self.generator.cancel();
                self.generation_pending = false;
                self.automatic_generation_needed = false;
                self.world_sync_ready = true;
                changed = true;
            }
            self.update_hover();
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
        self.inspected =
            position.and_then(|position| self.engine.world().inspect_chunk_at(position).ok());
        self.hovered = position.filter(|position| self.engine.world().cell(*position).is_some());

        let title = inspection_title(
            self.engine.world(),
            position,
            self.automatic_generation_blocked,
        );
        window.set_title(&title);
    }

    fn request_visible_generation(&mut self) -> bool {
        if !can_request_automatic_generation(
            self.automatic_generation_needed,
            self.generation_pending,
            self.generator.is_available(),
            self.selection_start.is_some(),
            self.dragging,
        ) {
            return false;
        }
        self.automatic_generation_needed = false;
        let Some(window) = &self.window else {
            return false;
        };
        let size = window.inner_size();
        if size.width == 0 || size.height == 0 {
            return false;
        }
        let bounds = self
            .camera
            .view(
                size.width,
                size.height,
                self.engine.world().width(),
                self.engine.world().height(),
            )
            .world_bounds();
        let previous_block = self.automatic_generation_blocked;
        match visible_generation_coords(self.engine.world(), bounds) {
            Ok(coords) if coords.is_empty() => {
                self.automatic_generation_blocked = None;
            }
            Ok(coords) => {
                self.automatic_generation_blocked = None;
                self.generation_pending = self.generator.request(self.engine.config().seed, coords);
                if !self.generation_pending {
                    eprintln!("world generation worker could not accept automatic request");
                }
            }
            Err(error) => {
                self.automatic_generation_blocked = Some(error);
            }
        }
        let changed = previous_block != self.automatic_generation_blocked;
        if changed {
            self.update_hover();
        }
        changed
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
        self.automatic_generation_needed = true;
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
        let command = match code {
            KeyCode::Space => Some(EngineCommand::TogglePause),
            KeyCode::Digit1 => Some(EngineCommand::SetSpeed(1.0)),
            KeyCode::Digit2 => Some(EngineCommand::SetSpeed(2.0)),
            KeyCode::Digit3 => Some(EngineCommand::SetSpeed(4.0)),
            KeyCode::Digit4 => Some(EngineCommand::SetSpeed(8.0)),
            KeyCode::KeyR => Some(EngineCommand::Reset),
            KeyCode::KeyC => {
                if self.generation_pending {
                    self.generator.cancel();
                }
                self.automatic_generation_needed = false;
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
        if let Some(frames) = self.smoke_frames.take() {
            for _ in 0..frames {
                assert!(self.render(), "GPU smoke frame failed");
            }
            event_loop.exit();
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        if self.window.as_ref().is_none_or(|window| window.id() != id) {
            return;
        }
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::RedrawRequested => {
                self.render();
            }
            WindowEvent::Resized(size) => {
                if let Some(renderer) = &mut self.renderer {
                    renderer.resize(size.width, size.height);
                }
                self.automatic_generation_needed = true;
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
                    self.selection = Some(WorldRect::from_inclusive_points(start, current));
                }
                self.update_hover();
                self.dirty = true;
            }
            WindowEvent::CursorLeft { .. } => {
                self.cursor = None;
                self.inspected = None;
                self.hovered = None;
                if self.dragging {
                    self.automatic_generation_needed = true;
                }
                self.dragging = false;
                self.selection_start = None;
                self.selection = None;
                if let Some(window) = &self.window {
                    window.set_title(&inspection_title(
                        self.engine.world(),
                        None,
                        self.automatic_generation_blocked,
                    ));
                }
                self.dirty = true;
            }
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => {
                if state == ElementState::Released && self.dragging {
                    self.automatic_generation_needed = true;
                }
                self.dragging = state == ElementState::Pressed;
                self.dirty = true;
            }
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Right,
                ..
            } => {
                match state {
                    ElementState::Pressed => {
                        if can_start_selection(
                            self.generation_pending,
                            self.generator.is_available(),
                        ) && let (Some(window), Some((x, y))) = (&self.window, self.cursor)
                        {
                            let size = window.inner_size();
                            let start = self.camera.screen_to_world_position(
                                x,
                                y,
                                self.viewport(size.width, size.height),
                            );
                            self.selection_start = Some(start);
                            self.selection = Some(WorldRect::from_inclusive_points(start, start));
                        }
                    }
                    ElementState::Released => {
                        if let Some(bounds) = self.selection.take() {
                            if !self.generation_pending
                                && !self.engine.world().area_is_generated(bounds)
                            {
                                match self.engine.world().missing_chunk_coords(bounds) {
                                    Ok(coords) if !coords.is_empty() => {
                                        self.generation_pending = self
                                            .generator
                                            .request(self.engine.config().seed, coords);
                                        if !self.generation_pending {
                                            eprintln!(
                                                "world generation worker could not accept request"
                                            );
                                        }
                                    }
                                    Ok(_) => {}
                                    Err(error) => eprintln!("world generation rejected: {error}"),
                                }
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
        if frame_due {
            self.dirty |= self.poll_generation();
            self.dirty |= self.request_visible_generation();
        }
        self.update();
        let snapshot = self.engine.snapshot();
        let now = Instant::now();
        let needs_redraw = self.dirty || snapshot.tick != previous_tick;
        if needs_redraw
            && now >= self.next_frame
            && let Some(window) = &self.window
        {
            window.request_redraw();
            self.dirty = false;
            self.next_frame = now + FRAME_TIME;
        }
        if !snapshot.paused || self.generation_pending || self.dirty {
            if self.next_frame <= now {
                self.next_frame = now + FRAME_TIME;
            }
            event_loop.set_control_flow(ControlFlow::WaitUntil(self.next_frame));
        } else {
            event_loop.set_control_flow(ControlFlow::Wait);
        }
    }
}

fn inspection_title(
    world: &World,
    position: Option<WorldPosition>,
    automatic_generation_blocked: Option<GenerateAreaError>,
) -> String {
    let mut title = "Let There Be Life".to_owned();
    if let Some(position) = position {
        match world.inspect_chunk_at(position) {
            Ok(inspection) => {
                let presence = match inspection.presence {
                    ChunkPresence::Missing => "missing",
                    ChunkPresence::PartialInitial => "partial-initial",
                    ChunkPresence::Initial => "initial",
                    ChunkPresence::Retained => "retained",
                    ChunkPresence::RetainedPartialInitial => "partial-initial+retained",
                };
                title.push_str(&format!(
                    " | ({}, {}) | chunk=({}, {}) local=({}, {}) coverage={presence}",
                    position.x,
                    position.y,
                    inspection.coord.x,
                    inspection.coord.y,
                    inspection.local.x,
                    inspection.local.y,
                ));
                if let Some(cell) = world.cell(position) {
                    let feature = world.feature_at(position).map_or_else(
                        || "none".to_owned(),
                        |feature| format!("{:?}", feature.kind),
                    );
                    title.push_str(&format!(
                        " | {:?} elevation={} moisture={} feature={feature}",
                        cell.ground, cell.elevation, cell.moisture,
                    ));
                } else {
                    title.push_str(" | cell=unloaded");
                }
            }
            Err(_) => title.push_str(&format!(
                " | ({}, {}) | chunk coordinates unavailable",
                position.x, position.y
            )),
        }
    }
    if let Some(error) = automatic_generation_blocked {
        title.push_str(&format!(" | automatic generation paused: {error}"));
    }
    title
}

fn visible_generation_coords(
    world: &World,
    bounds: WorldRect,
) -> Result<Vec<ChunkCoord>, GenerateAreaError> {
    world.missing_chunk_coords(bounds)
}

fn selection_is_valid(world: &World, selection: Option<WorldRect>) -> bool {
    selection.is_none_or(|bounds| world.validate_generation_request(bounds).is_ok())
}

const fn can_request_automatic_generation(
    generation_needed: bool,
    generation_pending: bool,
    generator_available: bool,
    selection_active: bool,
    camera_dragging: bool,
) -> bool {
    generation_needed
        && !generation_pending
        && generator_available
        && !selection_active
        && !camera_dragging
}

const fn can_start_selection(generation_pending: bool, generator_available: bool) -> bool {
    !generation_pending && generator_available
}

struct GenerationJob {
    seed: u64,
    coords: Vec<ChunkCoord>,
}

const MAX_CHUNKS_APPLIED_PER_FRAME: usize = 16;
const FRAME_TIME: Duration = Duration::from_nanos(16_666_667);

enum WorkerMessage {
    Chunk(WorldChunk),
    Done(GenerationOutcome),
}

enum GenerationOutcome {
    Completed,
    Cancelled,
    Failed(GenerateAreaError),
    WorkerStopped,
}

const fn automatic_generation_needed_after(
    current_demand: bool,
    outcome: &GenerationOutcome,
) -> bool {
    current_demand || matches!(outcome, GenerationOutcome::Completed)
}

struct GenerationPoll {
    chunks: Vec<WorldChunk>,
    outcome: Option<GenerationOutcome>,
    changed: bool,
}

struct WorldGenerator {
    jobs: SyncSender<GenerationJob>,
    completed: Receiver<WorkerMessage>,
    cancelled: Arc<AtomicBool>,
    disconnected: Cell<bool>,
}

impl WorldGenerator {
    fn new() -> Self {
        let (jobs_tx, jobs_rx) = mpsc::sync_channel::<GenerationJob>(1);
        let (completed_tx, completed_rx) = mpsc::sync_channel::<WorkerMessage>(64);
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = Arc::clone(&cancelled);
        thread::Builder::new()
            .name("world-generator".to_owned())
            .spawn(move || {
                while let Ok(job) = jobs_rx.recv() {
                    let mut outcome = GenerationOutcome::Completed;
                    for coord in job.coords {
                        if worker_cancelled.load(Ordering::Relaxed) {
                            outcome = GenerationOutcome::Cancelled;
                            break;
                        }
                        let chunk = match World::generate_chunk_at(job.seed, coord) {
                            Ok(chunk) => chunk,
                            Err(error) => {
                                outcome = GenerationOutcome::Failed(error);
                                break;
                            }
                        };
                        if completed_tx.send(WorkerMessage::Chunk(chunk)).is_err() {
                            return;
                        }
                    }
                    if completed_tx.send(WorkerMessage::Done(outcome)).is_err() {
                        return;
                    }
                }
            })
            .expect("spawn world generator");
        Self {
            jobs: jobs_tx,
            completed: completed_rx,
            cancelled,
            disconnected: Cell::new(false),
        }
    }

    fn request(&self, seed: u64, coords: Vec<ChunkCoord>) -> bool {
        if self.disconnected.get() || coords.is_empty() {
            return false;
        }
        self.cancelled.store(false, Ordering::Relaxed);
        self.jobs.try_send(GenerationJob { seed, coords }).is_ok()
    }

    fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }

    fn is_available(&self) -> bool {
        !self.disconnected.get()
    }

    fn drain(&self, limit: usize) -> GenerationPoll {
        if self.disconnected.get() {
            return GenerationPoll {
                chunks: Vec::new(),
                outcome: None,
                changed: false,
            };
        }
        let mut chunks = Vec::with_capacity(limit);
        let mut outcome = None;
        let mut disconnected = false;
        for _ in 0..limit {
            let message = match self.completed.try_recv() {
                Ok(message) => message,
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.disconnected.set(true);
                    disconnected = true;
                    break;
                }
            };
            match message {
                WorkerMessage::Chunk(chunk) => chunks.push(chunk),
                WorkerMessage::Done(result) => {
                    outcome = Some(result);
                    break;
                }
            }
        }
        if disconnected && outcome.is_none() {
            outcome = Some(GenerationOutcome::WorkerStopped);
        }
        let changed = outcome.is_some();
        GenerationPoll {
            chunks,
            outcome,
            changed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn world_generation_runs_on_worker_thread() {
        let generator = WorldGenerator::new();
        assert!(generator.request(7, vec![ChunkCoord { x: -1, y: -1 }]));
        let mut chunks = Vec::new();
        while let WorkerMessage::Chunk(chunk) = generator
            .completed
            .recv_timeout(Duration::from_secs(2))
            .expect("worker returns")
        {
            chunks.push(chunk);
        }
        assert_eq!(chunks.len(), 1);
    }

    #[test]
    fn generation_drain_honors_per_frame_limit() {
        let (jobs, _job_receiver) = mpsc::sync_channel(1);
        let (completed_sender, completed) = mpsc::sync_channel(64);
        let generator = WorldGenerator {
            jobs,
            completed,
            cancelled: Arc::new(AtomicBool::new(false)),
            disconnected: Cell::new(false),
        };
        let bounds = WorldRect::from_inclusive_points(
            WorldPosition { x: -128, y: -128 },
            WorldPosition { x: -1, y: -1 },
        );
        for chunk in World::generate_chunks(3, bounds).expect("valid generation") {
            completed_sender.send(WorkerMessage::Chunk(chunk)).unwrap();
        }
        completed_sender
            .send(WorkerMessage::Done(GenerationOutcome::Completed))
            .unwrap();

        let first = generator.drain(2);
        assert_eq!(first.chunks.len(), 2);
        assert!(first.outcome.is_none());
        let second = generator.drain(2);
        assert_eq!(second.chunks.len(), 2);
        assert!(second.outcome.is_none());
        let third = generator.drain(2);
        assert!(third.chunks.is_empty());
        assert!(matches!(third.outcome, Some(GenerationOutcome::Completed)));
    }

    #[test]
    fn worker_reports_invalid_chunk_coordinate() {
        let generator = WorldGenerator::new();
        assert!(generator.request(1, vec![ChunkCoord { x: i64::MAX, y: 0 }],));
        assert!(matches!(
            generator
                .completed
                .recv_timeout(Duration::from_secs(2))
                .expect("worker returns"),
            WorkerMessage::Done(GenerationOutcome::Failed(GenerateAreaError::TooLarge))
        ));
    }

    #[test]
    fn large_generation_can_be_cancelled() {
        let generator = WorldGenerator::new();
        let coords = (0..4_096)
            .map(|index| ChunkCoord {
                x: index % 64,
                y: index / 64,
            })
            .collect();
        assert!(generator.request(1, coords));
        generator.cancel();
        loop {
            match generator
                .completed
                .recv_timeout(Duration::from_secs(5))
                .expect("worker returns")
            {
                WorkerMessage::Chunk(_) => {}
                WorkerMessage::Done(outcome) => {
                    assert!(matches!(outcome, GenerationOutcome::Cancelled));
                    break;
                }
            }
        }
    }

    #[test]
    fn disconnected_worker_is_reported_once() {
        let (jobs, _job_receiver) = mpsc::sync_channel(1);
        let (completed_sender, completed) = mpsc::sync_channel(1);
        drop(completed_sender);
        let generator = WorldGenerator {
            jobs,
            completed,
            cancelled: Arc::new(AtomicBool::new(false)),
            disconnected: Cell::new(false),
        };

        let first = generator.drain(1);
        assert!(matches!(
            first.outcome,
            Some(GenerationOutcome::WorkerStopped)
        ));
        assert!(first.changed);
        assert!(!generator.is_available());

        let second = generator.drain(1);
        assert!(second.outcome.is_none());
        assert!(!second.changed);
        assert!(!generator.request(1, vec![ChunkCoord { x: 0, y: 0 }]));
    }

    #[test]
    fn selection_preview_uses_missing_chunk_budget() {
        let engine = Engine::default();
        let valid = WorldRect {
            min: WorldPosition { x: 0, y: 0 },
            max: WorldPosition {
                x: 17 * sim_core::CHUNK_SIZE,
                y: 241 * sim_core::CHUNK_SIZE,
            },
        };
        let outside_origin = WorldPosition {
            x: 100 * sim_core::CHUNK_SIZE,
            y: 100 * sim_core::CHUNK_SIZE,
        };
        let oversized = WorldRect {
            min: outside_origin,
            max: WorldPosition {
                x: outside_origin.x + 17 * sim_core::CHUNK_SIZE,
                y: outside_origin.y + 241 * sim_core::CHUNK_SIZE,
            },
        };

        assert!(selection_is_valid(engine.world(), None));
        assert_eq!(engine.world().validate_generation_request(valid), Ok(3_841));
        assert!(selection_is_valid(engine.world(), Some(valid)));
        assert!(!selection_is_valid(engine.world(), Some(oversized)));
        assert!(can_start_selection(false, true));
        assert!(!can_start_selection(true, true));
        assert!(!can_start_selection(false, false));
    }

    #[test]
    fn automatic_generation_queues_only_visible_missing_chunks() {
        let world = World::generate(1, sim_core::WorldConfig::new(64, 64).unwrap());
        let initial = WorldRect {
            min: WorldPosition { x: 0, y: 0 },
            max: WorldPosition { x: 64, y: 64 },
        };
        assert!(
            visible_generation_coords(&world, initial)
                .unwrap()
                .is_empty()
        );

        let cases = [
            (
                WorldRect {
                    min: WorldPosition { x: -1, y: 0 },
                    max: WorldPosition { x: 64, y: 64 },
                },
                ChunkCoord { x: -1, y: 0 },
            ),
            (
                WorldRect {
                    min: WorldPosition { x: 0, y: 0 },
                    max: WorldPosition { x: 65, y: 64 },
                },
                ChunkCoord { x: 1, y: 0 },
            ),
            (
                WorldRect {
                    min: WorldPosition { x: 0, y: -1 },
                    max: WorldPosition { x: 64, y: 64 },
                },
                ChunkCoord { x: 0, y: -1 },
            ),
            (
                WorldRect {
                    min: WorldPosition { x: 0, y: 0 },
                    max: WorldPosition { x: 64, y: 65 },
                },
                ChunkCoord { x: 0, y: 1 },
            ),
        ];
        for (bounds, expected) in cases {
            assert_eq!(
                visible_generation_coords(&world, bounds).unwrap(),
                [expected]
            );
        }
    }

    #[test]
    fn automatic_generation_preserves_request_budget() {
        let world = World::generate(1, sim_core::WorldConfig::new(64, 64).unwrap());
        let maximum = WorldRect {
            min: WorldPosition { x: 64, y: 0 },
            max: WorldPosition {
                x: 64 + 64 * sim_core::CHUNK_SIZE,
                y: 64 * sim_core::CHUNK_SIZE,
            },
        };
        assert_eq!(
            visible_generation_coords(&world, maximum).unwrap().len(),
            sim_core::MAX_CHUNKS_PER_GENERATION as usize
        );

        let oversized = WorldRect {
            max: WorldPosition {
                x: 64 + 65 * sim_core::CHUNK_SIZE,
                ..maximum.max
            },
            ..maximum
        };
        assert!(matches!(
            visible_generation_coords(&world, oversized),
            Err(GenerateAreaError::TooManyChunks {
                requested: 4_097,
                maximum: sim_core::MAX_CHUNKS_PER_GENERATION,
            })
        ));
    }

    #[test]
    fn automatic_generation_does_not_repeat_retained_work() {
        let mut world = World::generate(1, sim_core::WorldConfig::new(64, 64).unwrap());
        let bounds = WorldRect {
            min: WorldPosition { x: -64, y: 0 },
            max: WorldPosition { x: 0, y: 64 },
        };
        assert_eq!(
            visible_generation_coords(&world, bounds).unwrap(),
            [ChunkCoord { x: -1, y: 0 }]
        );
        world.generate_area(bounds).unwrap();
        let revision = world.revision();

        assert!(
            visible_generation_coords(&world, bounds)
                .unwrap()
                .is_empty()
        );
        assert_eq!(world.revision(), revision);
    }

    #[test]
    fn automatic_generation_arbitration_and_retry_are_explicit() {
        assert!(can_request_automatic_generation(
            true, false, true, false, false
        ));
        assert!(!can_request_automatic_generation(
            false, false, true, false, false
        ));
        assert!(!can_request_automatic_generation(
            true, true, true, false, false
        ));
        assert!(!can_request_automatic_generation(
            true, false, false, false, false
        ));
        assert!(!can_request_automatic_generation(
            true, false, true, true, false
        ));
        assert!(!can_request_automatic_generation(
            true, false, true, false, true
        ));

        assert!(automatic_generation_needed_after(
            false,
            &GenerationOutcome::Completed
        ));
        assert!(!automatic_generation_needed_after(
            false,
            &GenerationOutcome::Cancelled
        ));
        assert!(!automatic_generation_needed_after(
            false,
            &GenerationOutcome::Failed(GenerateAreaError::TooLarge)
        ));
        assert!(!automatic_generation_needed_after(
            false,
            &GenerationOutcome::WorkerStopped
        ));
        assert!(automatic_generation_needed_after(
            true,
            &GenerationOutcome::Cancelled
        ));
    }

    #[test]
    fn inspection_title_reports_unloaded_negative_chunk_coordinates() {
        let world = World::generate(1, sim_core::WorldConfig::new(64, 64).unwrap());
        let title = inspection_title(&world, Some(WorldPosition { x: -1, y: -65 }), None);

        assert!(title.contains("chunk=(-1, -2) local=(63, 63) coverage=missing"));
        assert!(title.contains("cell=unloaded"));
    }
}
