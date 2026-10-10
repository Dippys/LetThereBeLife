//! Fixed-step simulation advancement and population lifecycle (reset, agent and object spawning).

use std::time::Instant;

use super::{PopulationStatus, ViewerApp};
use crate::{
    gestures::GestureMark,
    labels,
    startup::{reset, residency_ready, spawn_at},
};

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
                // Engine event logs cover only the latest tick, so collect them per tick.
                self.gestures.record(
                    self.engine.signal_events().iter().map(GestureMark::from),
                    now,
                );
                self.feed.record(&self.engine);
                self.accumulator -= tick_seconds;
            }
        }
    }

    /// Runs `ticks` ticks at once (for `--advance`), keeping the feed and gestures.
    pub(crate) fn advance(&mut self, ticks: u64) {
        let now = Instant::now();
        for _ in 0..ticks {
            self.engine.tick();
            self.gestures.record(
                self.engine.signal_events().iter().map(GestureMark::from),
                now,
            );
            self.feed.record(&self.engine);
        }
    }

    pub(super) fn update_residency_status(&mut self) {
        if self.population_status != PopulationStatus::WaitingForResidency {
            return;
        }
        if residency_ready(self.engine.world()) {
            self.population_status = PopulationStatus::Ready;
            self.dirty = true;
        }
    }

    pub(super) fn reset_population(&mut self) {
        reset(&mut self.engine);
        self.gestures.clear();
        self.feed.clear();
        self.selected = None;
        self.following = false;
        self.population_status = PopulationStatus::WaitingForResidency;
        self.show_toast("Everyone was removed");
        self.update_residency_status();
        self.accumulator = 0.0;
        self.last_frame = Some(Instant::now());
        self.next_frame = Instant::now();
    }

    pub(super) fn spawn_agent_at_cursor(&mut self) {
        let Some(position) = self.cursor_world else {
            self.show_toast("Move the mouse over the map first");
            return;
        };
        match spawn_at(&mut self.engine, position) {
            Ok(agent) => {
                self.population_status = PopulationStatus::Active;
                self.show_toast(format!("Added {}", labels::person(agent)));
                self.accumulator = 0.0;
                self.last_frame = Some(Instant::now());
            }
            Err(error) => {
                eprintln!("{error}");
                self.show_toast(format!("Couldn't add a person: {error}"));
            }
        }
    }

    pub(super) fn spawn_object_at_cursor(&mut self, kind: sim_core::SpawnKind) {
        let Some(position) = self.cursor_world else {
            self.show_toast("Move the mouse over the map first");
            return;
        };
        if let Err(error) = self.engine.spawn_object(kind, position) {
            self.show_toast(format!("Couldn't place that here: {error}"));
        }
    }
}
