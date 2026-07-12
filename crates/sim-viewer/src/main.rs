mod camera;
mod renderer;

use std::{
    sync::{
        Arc,
        mpsc::{self, Receiver, SyncSender},
    },
    thread,
    time::{Duration, Instant},
};

use camera::{Camera, Viewport};
use sim_config::{AppConfig, DEFAULT_CONFIG_PATH};
use sim_core::{
    Engine, EngineCommand, GenerateAreaError, World, WorldChunk, WorldPosition, WorldRect,
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
    let config_path = config_path()?;
    let engine = Engine::new(AppConfig::load(config_path)?.engine_config()?);
    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Wait);
    let mut app = ViewerApp::new(engine);
    event_loop.run_app(&mut app)?;
    Ok(())
}

fn config_path() -> Result<String, Box<dyn std::error::Error>> {
    let mut path = DEFAULT_CONFIG_PATH.to_owned();
    let mut args = std::env::args().skip(1);
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--config" => path = args.next().ok_or("--config requires a path")?,
            "--help" | "-h" => {
                println!("Usage: sim-viewer [--config PATH]");
                std::process::exit(0);
            }
            _ => return Err(format!("unknown argument: {argument}").into()),
        }
    }
    Ok(path)
}

struct ViewerApp {
    window: Option<Arc<Window>>,
    renderer: Option<renderer::Renderer>,
    engine: Engine,
    last_frame: Option<Instant>,
    accumulator: f64,
    camera: Camera,
    cursor: Option<(f64, f64)>,
    hovered: Option<WorldPosition>,
    dragging: bool,
    selection_start: Option<WorldPosition>,
    selection: Option<WorldRect>,
    generator: WorldGenerator,
    generation_pending: bool,
    next_frame: Instant,
    dirty: bool,
}

impl ViewerApp {
    fn new(engine: Engine) -> Self {
        let camera = Camera::centered(engine.world().width(), engine.world().height());
        Self {
            window: None,
            renderer: None,
            engine,
            last_frame: None,
            accumulator: 0.0,
            camera,
            cursor: None,
            hovered: None,
            dragging: false,
            selection_start: None,
            selection: None,
            generator: WorldGenerator::new(),
            generation_pending: false,
            next_frame: Instant::now(),
            dirty: true,
        }
    }
}

impl ViewerApp {
    fn create_window(&mut self, event_loop: &ActiveEventLoop) {
        let attributes = WindowAttributes::default()
            .with_title("Let There Be Life")
            .with_inner_size(LogicalSize::new(960, 540))
            .with_min_inner_size(LogicalSize::new(640, 360));
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

    fn render(&mut self) {
        let (Some(window), Some(renderer)) = (&self.window, &mut self.renderer) else {
            return;
        };
        let size = window.inner_size();
        if size.width == 0 || size.height == 0 {
            return;
        }
        let result = renderer.render(
            self.engine.world(),
            renderer::RenderState {
                snapshot: self.engine.snapshot(),
                camera: self.camera,
                hovered: self.hovered,
                selection: self.selection,
            },
        );
        match result {
            Ok(()) => {}
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                renderer.resize(size.width, size.height)
            }
            Err(wgpu::SurfaceError::OutOfMemory) => std::process::exit(1),
            Err(wgpu::SurfaceError::Timeout | wgpu::SurfaceError::Other) => {}
        }
    }

    fn poll_generation(&mut self) -> bool {
        let Some(result) = self.generator.try_recv() else {
            return false;
        };
        self.generation_pending = false;
        if let Ok(chunks) = result {
            self.engine.apply_world_chunks(chunks);
        }
        self.update_hover();
        true
    }

    fn update_hover(&mut self) {
        let Some(window) = &self.window else {
            return;
        };
        let size = window.inner_size();
        self.hovered = self.cursor.and_then(|(x, y)| {
            let position =
                self.camera
                    .screen_to_world_position(x, y, self.viewport(size.width, size.height));
            self.engine.world().cell(position).map(|_| position)
        });

        let title = self.hovered.map_or_else(
            || "Let There Be Life".to_owned(),
            |position| {
                let cell = self
                    .engine
                    .world()
                    .cell(position)
                    .expect("hover is in bounds");
                let feature = self.engine.world().feature_at(position).map_or_else(
                    || "none".to_owned(),
                    |feature| format!("{:?}", feature.kind),
                );
                format!(
                    "Let There Be Life | ({}, {}) | {:?} | elevation={} moisture={} | feature={}",
                    position.x, position.y, cell.ground, cell.elevation, cell.moisture, feature
                )
            },
        );
        window.set_title(&title);
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
        let command = match code {
            KeyCode::Space => Some(EngineCommand::TogglePause),
            KeyCode::Digit1 => Some(EngineCommand::SetSpeed(1.0)),
            KeyCode::Digit2 => Some(EngineCommand::SetSpeed(2.0)),
            KeyCode::Digit3 => Some(EngineCommand::SetSpeed(4.0)),
            KeyCode::Digit4 => Some(EngineCommand::SetSpeed(8.0)),
            KeyCode::KeyR => Some(EngineCommand::Reset),
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
            }
        }
    }
}

impl ApplicationHandler for ViewerApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_none() {
            self.create_window(event_loop);
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
                self.hovered = None;
                self.dragging = false;
                self.selection_start = None;
                self.selection = None;
                if let Some(window) = &self.window {
                    window.set_title("Let There Be Life");
                }
                self.dirty = true;
            }
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => {
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
                        if let (Some(window), Some((x, y))) = (&self.window, self.cursor) {
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
                                self.generation_pending =
                                    self.generator.request(self.engine.config().seed, bounds);
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
        let previous_tick = self.engine.snapshot().tick;
        self.dirty |= self.poll_generation();
        self.update();
        let snapshot = self.engine.snapshot();
        if (self.dirty || snapshot.tick != previous_tick)
            && let Some(window) = &self.window
        {
            window.request_redraw();
            self.dirty = false;
        }
        if !snapshot.paused || self.generation_pending {
            let frame_time = Duration::from_secs_f64(1.0 / 60.0);
            let now = Instant::now();
            self.next_frame = self.next_frame.max(now) + frame_time;
            event_loop.set_control_flow(ControlFlow::WaitUntil(self.next_frame));
        } else {
            event_loop.set_control_flow(ControlFlow::Wait);
        }
    }
}

struct GenerationJob {
    seed: u64,
    bounds: WorldRect,
}

struct WorldGenerator {
    jobs: SyncSender<GenerationJob>,
    completed: Receiver<Result<Vec<WorldChunk>, GenerateAreaError>>,
}

impl WorldGenerator {
    fn new() -> Self {
        let (jobs_tx, jobs_rx) = mpsc::sync_channel::<GenerationJob>(1);
        let (completed_tx, completed_rx) = mpsc::channel();
        thread::Builder::new()
            .name("world-generator".to_owned())
            .spawn(move || {
                while let Ok(job) = jobs_rx.recv() {
                    if completed_tx
                        .send(World::generate_chunks(job.seed, job.bounds))
                        .is_err()
                    {
                        break;
                    }
                }
            })
            .expect("spawn world generator");
        Self {
            jobs: jobs_tx,
            completed: completed_rx,
        }
    }

    fn request(&self, seed: u64, bounds: WorldRect) -> bool {
        self.jobs.try_send(GenerationJob { seed, bounds }).is_ok()
    }

    fn try_recv(&self) -> Option<Result<Vec<WorldChunk>, GenerateAreaError>> {
        self.completed.try_recv().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn world_generation_runs_on_worker_thread() {
        let generator = WorldGenerator::new();
        let bounds = WorldRect::from_inclusive_points(
            WorldPosition { x: -64, y: -64 },
            WorldPosition { x: -1, y: -1 },
        );
        assert!(generator.request(7, bounds));
        let chunks = generator
            .completed
            .recv_timeout(Duration::from_secs(2))
            .expect("worker returns")
            .expect("valid generation");
        assert_eq!(chunks.len(), 1);
    }
}
