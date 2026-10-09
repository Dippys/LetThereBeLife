//! Cursor hover tracking, mouse-wheel zoom, and keyboard command handling.

use std::time::Instant;

use sim_core::EngineCommand;
use winit::{event::MouseScrollDelta, event_loop::ActiveEventLoop, keyboard::KeyCode};

use super::ViewerApp;
use crate::{camera::Viewport, spawn_menu::SpawnMenuMode};

impl ViewerApp {
    pub(super) fn update_hover(&mut self) {
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

    pub(super) fn zoom(&mut self, delta: MouseScrollDelta) {
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

    pub(super) fn handle_key(&mut self, code: KeyCode, event_loop: &ActiveEventLoop) {
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
