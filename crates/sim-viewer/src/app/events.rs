//! `winit` application-handler wiring: window, mouse, keyboard, and idle event dispatch.

use std::time::Instant;

use winit::{
    application::ApplicationHandler,
    event::{ElementState, MouseButton, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow},
    keyboard::PhysicalKey,
    window::WindowId,
};

use super::{FRAME_TIME, ViewerApp, selection::bounded_selection};
use crate::generation::GenerationKind;

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
