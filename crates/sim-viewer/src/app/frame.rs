//! Window creation, per-frame render-state assembly, and the hidden-window GPU smoke loop.

use std::{
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

use sim_core::WorldArchive;
use winit::{dpi::LogicalSize, event_loop::ActiveEventLoop, window::WindowAttributes};

use super::{PopulationStatus, ViewerApp, WORLD_SYNC_INTERVAL};
use crate::{render, spawn_menu::SpawnMenuMode, startup};

impl ViewerApp {
    pub(super) fn create_window(&mut self, event_loop: &ActiveEventLoop) {
        let attributes = WindowAttributes::default()
            .with_title("Let There Be Life")
            .with_inner_size(LogicalSize::new(960, 540))
            .with_min_inner_size(LogicalSize::new(640, 360))
            .with_visible(self.smoke_frames.is_none());
        let window = Arc::new(event_loop.create_window(attributes).expect("create window"));
        self.renderer = Some(
            render::Renderer::new(
                window.clone(),
                self.engine.world(),
                self.archive.as_ref().map(WorldArchive::overview),
            )
            .expect("initialize GPU renderer"),
        );
        self.window = Some(window);
        if self.archive.is_some() {
            let size = self.window.as_ref().expect("window exists").inner_size();
            self.camera
                .show_full_world(self.viewport(size.width, size.height));
        }
        self.last_frame = Some(Instant::now());
    }

    pub(super) fn render(&mut self) -> bool {
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
                .map(|view| render::AgentInspection {
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
                    memory: self
                        .engine
                        .mental_map(view.id)
                        .as_ref()
                        .map(render::MemoryInspection::from_view),
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
            render::RenderState {
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
                    PopulationStatus::WaitingForResidency => render::PopulationStatus::Waiting,
                    PopulationStatus::Ready => render::PopulationStatus::Ready,
                    PopulationStatus::Active => render::PopulationStatus::Active,
                },
                hovered_agent,
                spawn_message: self.spawn_message.clone(),
                spawn_menu: (self.spawn_menu.mode() != SpawnMenuMode::Closed).then_some(
                    render::SpawnMenuView {
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

    pub(super) fn render_smoke_frame_if_ready(&mut self, event_loop: &ActiveEventLoop) -> bool {
        let Some(frames) = self.smoke_frames else {
            return false;
        };
        if self.population_status == PopulationStatus::WaitingForResidency {
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

    pub(super) fn run_smoke(&mut self, event_loop: &ActiveEventLoop) {
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
}
