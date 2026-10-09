//! Background world generation and archive loading: polling, scheduling, cancellation, and dirty-region tracking.

use std::{sync::mpsc, time::Instant};

use sim_core::{
    ChunkLoadRequest, GenerateAreaError, WORLD_GENERATION_BOUNDS, WorldChunkLoad, WorldPosition,
    WorldRect,
};

use super::{
    ARCHIVE_DETAIL_MIN_SCALE, ActiveGeneration, CHUNKS_APPLIED_PER_BATCH,
    MAX_CHUNKS_APPLIED_PER_FRAME, ViewerApp, WORLD_APPLY_TIME_BUDGET,
};
use crate::{
    generation::{GenerationJob, GenerationKind, GenerationOutcome},
    render,
};

impl ViewerApp {
    pub(super) fn poll_generation(&mut self) -> bool {
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
            match outcome {
                GenerationOutcome::Failed => {
                    eprintln!(
                        "archive detail loading failed; disabling archive reads and falling back to deterministic generation"
                    );
                    self.archive = None;
                    self.update_hover();
                }
                GenerationOutcome::WorkerStopped => {
                    eprintln!("world generation worker stopped unexpectedly");
                    self.update_hover();
                }
                GenerationOutcome::Completed | GenerationOutcome::Cancelled => {}
            }
            if self.pending_world_changes.is_some() {
                self.next_world_sync = Instant::now();
            }
            changed = true;
        }
        changed
    }

    pub(super) fn generation_status(&self) -> render::GenerationStatus {
        if !self.generator.is_available() {
            return render::GenerationStatus::WorkerUnavailable;
        }
        if self
            .active_generation
            .as_ref()
            .is_some_and(|active| active.discard_loads)
        {
            return render::GenerationStatus::Cancelling;
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
            Some(GenerationKind::Manual) => render::GenerationStatus::Manual,
            Some(GenerationKind::Bootstrap) => render::GenerationStatus::Bootstrap,
            None => render::GenerationStatus::Idle,
        }
    }

    pub(super) fn schedule_generation(&mut self) -> bool {
        if self.active_generation.is_some() || !self.generator.is_available() {
            return false;
        }
        if let Some(requests) = self.pending_manual.take() {
            return self.start_generation(GenerationKind::Manual, requests);
        }
        match self.take_archive_view_requests() {
            Ok(Some(requests)) => {
                return self.start_generation(GenerationKind::Bootstrap, requests);
            }
            Ok(None) => {}
            Err(error) => eprintln!("archive detail loading paused: {error}"),
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

    fn take_archive_view_requests(
        &self,
    ) -> Result<Option<Vec<ChunkLoadRequest>>, GenerateAreaError> {
        let (Some(_), Some(window)) = (&self.archive, &self.window) else {
            return Ok(None);
        };
        let size = window.inner_size();
        let view = self.camera.view(
            size.width,
            size.height,
            self.engine.world().width(),
            self.engine.world().height(),
        );
        if view.scale() < ARCHIVE_DETAIL_MIN_SCALE {
            return Ok(None);
        }
        let Some(bounds) = view.world_bounds().intersection(WORLD_GENERATION_BOUNDS) else {
            return Ok(None);
        };
        let requests = self.engine.world().missing_chunk_load_requests(bounds)?;
        Ok((!requests.is_empty()).then_some(requests))
    }

    pub(super) fn take_next_bootstrap_requests(
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
            archive: self.archive.clone(),
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

    pub(super) fn requeue_generation(
        &mut self,
        kind: GenerationKind,
        requests: Vec<ChunkLoadRequest>,
    ) {
        let slot = match kind {
            GenerationKind::Manual => &mut self.pending_manual,
            GenerationKind::Bootstrap => &mut self.pending_bootstrap,
        };
        debug_assert!(slot.is_none(), "only one unsent page per priority exists");
        *slot = Some(requests);
    }

    pub(super) fn generation_is_pending(&self) -> bool {
        self.generator.is_available()
            && (self.pending_manual.is_some()
                || self.pending_bootstrap.is_some()
                || self.bootstrap_pager.is_some())
    }

    pub(super) fn cancel_active_generation(&mut self) {
        if let Some(active) = &mut self.active_generation
            && !active.discard_loads
        {
            active.discard_loads = true;
            self.generator.cancel(active.id);
        }
    }

    pub(super) fn cancel_background_generation(&mut self) {
        if self
            .active_generation
            .as_ref()
            .is_some_and(|active| !matches!(active.kind, GenerationKind::Manual))
        {
            self.cancel_active_generation();
        }
    }

    pub(super) fn cancel_pending_generation(&mut self) {
        self.cancel_active_generation();
        self.pending_manual = None;
        self.pending_bootstrap = None;
        self.bootstrap_pager = None;
        self.selection_start = None;
        self.selection = None;
        self.selection_validation = None;
        self.update_hover();
    }

    pub(super) fn mark_world_changed(&mut self, bounds: WorldRect) {
        self.pending_world_changes = Some(match self.pending_world_changes {
            Some(previous) => union_bounds(previous, bounds),
            None => bounds,
        });
        self.dirty = true;
    }
}

pub(super) fn union_load_bounds(loads: &[WorldChunkLoad]) -> Option<WorldRect> {
    loads
        .iter()
        .map(WorldChunkLoad::bounds)
        .reduce(union_bounds)
}

pub(super) fn union_bounds(left: WorldRect, right: WorldRect) -> WorldRect {
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
