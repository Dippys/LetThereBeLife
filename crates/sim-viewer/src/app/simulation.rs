//! Fixed-step simulation advancement and population lifecycle (reset, agent and object spawning).

use std::time::Instant;

use super::{PopulationStatus, ViewerApp};
use crate::startup::{reset, residency_ready, spawn_at};

impl ViewerApp {
    pub(super) fn update(&mut self) {
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

    pub(super) fn update_residency_status(&mut self) {
        if self.population_status != PopulationStatus::WaitingForResidency {
            return;
        }
        if residency_ready(self.engine.world()) {
            self.population_status = PopulationStatus::Ready;
            self.spawn_message = Some("READY - PRESS T ON LOADED TERRAIN".to_owned());
            self.dirty = true;
        }
    }

    pub(super) fn reset_population(&mut self) {
        reset(&mut self.engine);
        self.population_status = PopulationStatus::WaitingForResidency;
        self.spawn_message = Some("POPULATION RESET".to_owned());
        self.update_residency_status();
        self.accumulator = 0.0;
        self.last_frame = Some(Instant::now());
        self.next_frame = Instant::now();
    }

    pub(super) fn spawn_agent_at_cursor(&mut self) {
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

    pub(super) fn spawn_object_at_cursor(&mut self) {
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
}
