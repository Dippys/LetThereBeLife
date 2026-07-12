mod camera;
mod renderer;

use std::{num::NonZeroU32, sync::Arc, time::Instant};

use camera::{Camera, Viewport};
use sim_config::{AppConfig, DEFAULT_CONFIG_PATH};
use sim_core::{Engine, EngineCommand, WorldPosition, WorldRect};
use softbuffer::{Context, Surface};
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
    event_loop.set_control_flow(ControlFlow::Poll);
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
    surface: Option<Surface<Arc<Window>, Arc<Window>>>,
    engine: Engine,
    last_frame: Option<Instant>,
    accumulator: f64,
    camera: Camera,
    cursor: Option<(f64, f64)>,
    hovered: Option<WorldPosition>,
    dragging: bool,
    selection_start: Option<WorldPosition>,
    selection: Option<WorldRect>,
}

impl ViewerApp {
    fn new(engine: Engine) -> Self {
        let camera = Camera::centered(engine.world().width(), engine.world().height());
        Self {
            window: None,
            surface: None,
            engine,
            last_frame: None,
            accumulator: 0.0,
            camera,
            cursor: None,
            hovered: None,
            dragging: false,
            selection_start: None,
            selection: None,
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
        let context = Context::new(window.clone()).expect("create graphics context");
        let surface = Surface::new(&context, window.clone()).expect("create graphics surface");
        self.surface = Some(surface);
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
        let (Some(window), Some(surface)) = (&self.window, &mut self.surface) else {
            return;
        };
        let size = window.inner_size();
        let (Some(width), Some(height)) =
            (NonZeroU32::new(size.width), NonZeroU32::new(size.height))
        else {
            return;
        };
        surface.resize(width, height).expect("resize surface");
        let mut buffer = surface.buffer_mut().expect("acquire frame buffer");
        renderer::draw(
            &mut buffer,
            size.width,
            size.height,
            self.engine.world(),
            renderer::RenderState {
                snapshot: self.engine.snapshot(),
                camera: self.camera,
                hovered: self.hovered,
                selection: self.selection,
            },
        );
        buffer.present().expect("present frame");
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
                self.update();
                self.render();
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
            }
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => {
                self.dragging = state == ElementState::Pressed;
            }
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Right,
                ..
            } => match state {
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
                        self.engine
                            .command(EngineCommand::GenerateWorldArea(bounds));
                    }
                    self.selection_start = None;
                    self.update_hover();
                }
            },
            WindowEvent::MouseWheel { delta, .. } => self.zoom(delta),
            WindowEvent::KeyboardInput { event, .. }
                if event.state == ElementState::Pressed && !event.repeat =>
            {
                if let PhysicalKey::Code(code) = event.physical_key {
                    self.handle_key(code, event_loop);
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }
}
