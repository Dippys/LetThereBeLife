//! Window creation, per-frame render-state assembly, and the hidden-window GPU smoke loop.

use std::{
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

use sim_core::{AgentId, Species, WorldArchive};
use winit::{dpi::LogicalSize, event_loop::ActiveEventLoop, window::WindowAttributes};

use super::{PopulationStatus, VALLEY_VIEW_SPAN_CELLS, ViewerApp, WORLD_SYNC_INTERVAL};
use crate::{render, startup};

impl ViewerApp {
    pub(super) fn create_window(&mut self, event_loop: &ActiveEventLoop) {
        let attributes = WindowAttributes::default()
            .with_title("Let There Be Life")
            .with_inner_size(LogicalSize::new(1280, 800))
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
        let size = self.window.as_ref().expect("window exists").inner_size();
        let viewport = self.viewport(size.width, size.height);
        if let Some(center) = self.initial_focus {
            self.camera
                .focus_on(center, VALLEY_VIEW_SPAN_CELLS, viewport);
        } else if self.archive.is_some() {
            self.camera.show_full_world(viewport);
        }
        self.last_frame = Some(Instant::now());
    }

    fn inspect(&self, agent: AgentId) -> Option<render::AgentInspection> {
        let view = self
            .engine
            .agent_views(startup::VIEWER_AGENT_LIMIT)
            .find(|view| view.id == agent)?;
        Some(render::AgentInspection {
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
            life: self.engine.life(view.id),
            motherhood: self.engine.motherhood(view.id),
        })
    }

    fn census(&self) -> render::Census {
        let snapshot = self.engine.snapshot();
        let mut census = render::Census {
            people: snapshot.living_agent_count,
            dead: snapshot.death_count,
            ..render::Census::default()
        };
        for animal in self.engine.animal_views() {
            match animal.species {
                Species::Deer => census.deer += 1,
                Species::Wolf => census.wolves += 1,
            }
        }
        census
    }

    pub(super) fn render(&mut self) -> bool {
        // People and animals move under a still mouse, so refresh what it points at.
        self.update_hover();
        let selection_valid = self.selection_preview_is_valid();
        let generation_status = self.generation_status();
        let selected = self.selected.and_then(|agent| self.inspect(agent));
        if self.following
            && let (Some(agent), Some(window)) = (&selected, &self.window)
        {
            let size = window.inner_size();
            let viewport = self.viewport(size.width, size.height);
            self.camera.center_on(agent.view.position, viewport);
        }
        let census = self.census();
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
                cursor: self.cursor,
                cursor_world: self.cursor_world,
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
                hover: self.hover,
                selected,
                following: self.following,
                toast: self.toast.as_ref().map(|(message, _)| message.clone()),
                help_open: self.help_open,
                details_open: self.details_open,
                build: self.build,
                feed: self.feed.entries().cloned().collect(),
                census,
                season: self.engine.season(),
            },
            &self.gestures,
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
        if frames == 1
            && let (Some(path), Some(renderer)) = (self.screenshot.take(), &mut self.renderer)
        {
            renderer.capture_next_frame(path);
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
