use std::{
    cell::Cell,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    thread,
};

use sim_core::{
    ChunkCoord, ChunkLoadRequest, GenerateAreaError, World, WorldChunkLoad, WorldPosition,
    WorldRect,
};

pub const AUTOMATIC_PAGE_CHUNKS: i64 = 8;

pub type GenerationId = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenerationKind {
    Bootstrap,
    Automatic,
    Manual,
}

pub struct GenerationJob {
    pub id: GenerationId,
    pub seed: u64,
    pub requests: Vec<ChunkLoadRequest>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenerationOutcome {
    Completed,
    Cancelled,
    WorkerStopped,
}

enum WorkerMessage {
    Load {
        id: GenerationId,
        load: WorldChunkLoad,
    },
    Done {
        id: GenerationId,
        outcome: GenerationOutcome,
    },
}

pub struct GenerationPoll {
    pub loads: Vec<WorldChunkLoad>,
    pub outcome: Option<GenerationOutcome>,
}

/// One persistent worker that produces immutable world payloads. It never owns
/// or mutates `Engine`; only the viewer event loop applies completed loads.
pub struct WorldGenerator {
    jobs: SyncSender<GenerationJob>,
    completed: Receiver<WorkerMessage>,
    cancelled_job: Arc<AtomicU64>,
    disconnected: Cell<bool>,
}

impl WorldGenerator {
    pub fn new() -> Self {
        let (jobs_tx, jobs_rx) = mpsc::sync_channel::<GenerationJob>(1);
        let (completed_tx, completed_rx) = mpsc::sync_channel::<WorkerMessage>(64);
        let cancelled_job = Arc::new(AtomicU64::new(0));
        let worker_cancelled_job = Arc::clone(&cancelled_job);
        thread::Builder::new()
            .name("world-generator".to_owned())
            .spawn(move || {
                while let Ok(job) = jobs_rx.recv() {
                    let mut outcome = GenerationOutcome::Completed;
                    for request in job.requests {
                        if worker_cancelled_job.load(Ordering::Acquire) == job.id {
                            outcome = GenerationOutcome::Cancelled;
                            break;
                        }
                        let load = World::generate_chunk_load(job.seed, request);
                        if completed_tx
                            .send(WorkerMessage::Load { id: job.id, load })
                            .is_err()
                        {
                            return;
                        }
                    }
                    if completed_tx
                        .send(WorkerMessage::Done {
                            id: job.id,
                            outcome,
                        })
                        .is_err()
                    {
                        return;
                    }
                }
            })
            .expect("spawn world generator");
        Self {
            jobs: jobs_tx,
            completed: completed_rx,
            cancelled_job,
            disconnected: Cell::new(false),
        }
    }

    /// Queues one nonempty job without losing its payload when the worker is
    /// temporarily busy or has stopped.
    pub fn request(&self, job: GenerationJob) -> Result<(), mpsc::TrySendError<GenerationJob>> {
        debug_assert!(
            !job.requests.is_empty(),
            "empty generation jobs must not enter the worker queue"
        );
        if self.disconnected.get() {
            return Err(mpsc::TrySendError::Disconnected(job));
        }
        match self.jobs.try_send(job) {
            Ok(()) => Ok(()),
            Err(error @ mpsc::TrySendError::Full(_)) => Err(error),
            Err(error @ mpsc::TrySendError::Disconnected(_)) => {
                self.disconnected.set(true);
                Err(error)
            }
        }
    }

    pub fn cancel(&self, id: GenerationId) {
        self.cancelled_job.store(id, Ordering::Release);
    }

    pub fn is_available(&self) -> bool {
        !self.disconnected.get()
    }

    /// Drains at most `limit` loads for a live job. For a cancelled job, all
    /// already queued loads are discarded so the worker can observe cancellation
    /// and acknowledge its terminal outcome promptly.
    pub fn drain(
        &self,
        active_id: GenerationId,
        limit: usize,
        discard_loads: bool,
    ) -> GenerationPoll {
        if self.disconnected.get() {
            return GenerationPoll {
                loads: Vec::new(),
                outcome: Some(GenerationOutcome::WorkerStopped),
            };
        }

        let mut loads = Vec::with_capacity(limit);
        loop {
            let message = match self.completed.try_recv() {
                Ok(message) => message,
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.disconnected.set(true);
                    return GenerationPoll {
                        loads,
                        outcome: Some(GenerationOutcome::WorkerStopped),
                    };
                }
            };
            match message {
                WorkerMessage::Load { id, load } if id == active_id => {
                    if !discard_loads {
                        loads.push(load);
                        if loads.len() == limit {
                            break;
                        }
                    }
                }
                WorkerMessage::Done { id, outcome } if id == active_id => {
                    return GenerationPoll {
                        loads,
                        outcome: Some(outcome),
                    };
                }
                WorkerMessage::Load { .. } | WorkerMessage::Done { .. } => {}
            }
        }
        GenerationPoll {
            loads,
            outcome: None,
        }
    }
}

/// A bounded, deterministic center-out sequence of chunk pages for automatic
/// demand. It holds only the current page-ring iterator, never one entry per
/// chunk in an extreme zoomed-out viewport.
pub struct ViewportPager {
    bounds: WorldRect,
    min_page_x: i64,
    max_page_x: i64,
    min_page_y: i64,
    max_page_y: i64,
    focus_page_x: i64,
    focus_page_y: i64,
    next_radius: i64,
    maximum_radius: i64,
    ring: Option<PageRing>,
}

impl ViewportPager {
    pub fn new(bounds: WorldRect, focus: WorldPosition) -> Result<Self, GenerateAreaError> {
        if bounds.max.x <= bounds.min.x || bounds.max.y <= bounds.min.y {
            return Err(GenerateAreaError::Empty);
        }
        let min_chunk = ChunkCoord::from_world_position(bounds.min);
        let max_position = WorldPosition {
            x: bounds.max.x - 1,
            y: bounds.max.y - 1,
        };
        let max_chunk = ChunkCoord::from_world_position(max_position);
        for coord in [min_chunk, max_chunk] {
            coord.bounds()?;
        }

        let min_page_x = min_chunk.x.div_euclid(AUTOMATIC_PAGE_CHUNKS);
        let max_page_x = max_chunk.x.div_euclid(AUTOMATIC_PAGE_CHUNKS);
        let min_page_y = min_chunk.y.div_euclid(AUTOMATIC_PAGE_CHUNKS);
        let max_page_y = max_chunk.y.div_euclid(AUTOMATIC_PAGE_CHUNKS);
        let focus_chunk = ChunkCoord::from_world_position(focus);
        let focus_page_x = focus_chunk
            .x
            .div_euclid(AUTOMATIC_PAGE_CHUNKS)
            .clamp(min_page_x, max_page_x);
        let focus_page_y = focus_chunk
            .y
            .div_euclid(AUTOMATIC_PAGE_CHUNKS)
            .clamp(min_page_y, max_page_y);
        let maximum_radius = [
            focus_page_x - min_page_x,
            max_page_x - focus_page_x,
            focus_page_y - min_page_y,
            max_page_y - focus_page_y,
        ]
        .into_iter()
        .max()
        .expect("fixed array is nonempty");
        Ok(Self {
            bounds,
            min_page_x,
            max_page_x,
            min_page_y,
            max_page_y,
            focus_page_x,
            focus_page_y,
            next_radius: 0,
            maximum_radius,
            ring: None,
        })
    }

    pub fn next_requests(
        &mut self,
        world: &World,
    ) -> Result<Option<Vec<ChunkLoadRequest>>, GenerateAreaError> {
        while let Some(page) = self.next_page() {
            let requests = world.missing_chunk_load_requests(page)?;
            if !requests.is_empty() {
                return Ok(Some(requests));
            }
        }
        Ok(None)
    }

    fn next_page(&mut self) -> Option<WorldRect> {
        loop {
            if let Some(ring) = &mut self.ring {
                if let Some((page_x, page_y)) = ring.next() {
                    if let Some(bounds) = page_bounds(page_x, page_y, self.bounds) {
                        return Some(bounds);
                    }
                    continue;
                }
                self.ring = None;
            }
            if self.next_radius > self.maximum_radius {
                return None;
            }
            self.ring = Some(PageRing::new(
                self.focus_page_x,
                self.focus_page_y,
                self.next_radius,
                self.min_page_x,
                self.max_page_x,
                self.min_page_y,
                self.max_page_y,
            ));
            self.next_radius += 1;
        }
    }
}

struct PageRing {
    segments: [Option<PageSegment>; 4],
    segment_index: usize,
}

#[derive(Clone, Copy)]
enum PageSegment {
    Horizontal { y: i64, next: i64, end: i64 },
    Vertical { x: i64, next: i64, end: i64 },
}

impl PageRing {
    fn new(
        center_x: i64,
        center_y: i64,
        radius: i64,
        min_x: i64,
        max_x: i64,
        min_y: i64,
        max_y: i64,
    ) -> Self {
        if radius == 0 {
            return Self {
                segments: [
                    Some(PageSegment::Horizontal {
                        y: center_y,
                        next: center_x,
                        end: center_x,
                    }),
                    None,
                    None,
                    None,
                ],
                segment_index: 0,
            };
        }

        let ring_min_x = center_x - radius;
        let ring_max_x = center_x + radius;
        let ring_min_y = center_y - radius;
        let ring_max_y = center_y + radius;
        let horizontal_start = ring_min_x.max(min_x);
        let horizontal_end = ring_max_x.min(max_x);
        let vertical_start = ring_min_y.saturating_add(1).max(min_y);
        let vertical_end = ring_max_y.saturating_sub(1).min(max_y);
        Self {
            segments: [
                (ring_min_y >= min_y && ring_min_y <= max_y && horizontal_start <= horizontal_end)
                    .then_some(PageSegment::Horizontal {
                        y: ring_min_y,
                        next: horizontal_start,
                        end: horizontal_end,
                    }),
                (ring_max_y >= min_y && ring_max_y <= max_y && horizontal_start <= horizontal_end)
                    .then_some(PageSegment::Horizontal {
                        y: ring_max_y,
                        next: horizontal_start,
                        end: horizontal_end,
                    }),
                (ring_min_x >= min_x && ring_min_x <= max_x && vertical_start <= vertical_end)
                    .then_some(PageSegment::Vertical {
                        x: ring_min_x,
                        next: vertical_start,
                        end: vertical_end,
                    }),
                (ring_max_x >= min_x && ring_max_x <= max_x && vertical_start <= vertical_end)
                    .then_some(PageSegment::Vertical {
                        x: ring_max_x,
                        next: vertical_start,
                        end: vertical_end,
                    }),
            ],
            segment_index: 0,
        }
    }

    fn next(&mut self) -> Option<(i64, i64)> {
        while let Some(segment) = self.segments.get_mut(self.segment_index) {
            match segment {
                Some(PageSegment::Horizontal { y, next, end }) if *next <= *end => {
                    let position = (*next, *y);
                    *next += 1;
                    return Some(position);
                }
                Some(PageSegment::Vertical { x, next, end }) if *next <= *end => {
                    let position = (*x, *next);
                    *next += 1;
                    return Some(position);
                }
                _ => self.segment_index += 1,
            }
        }
        None
    }
}

fn page_bounds(page_x: i64, page_y: i64, bounds: WorldRect) -> Option<WorldRect> {
    let min_chunk = ChunkCoord {
        x: page_x.checked_mul(AUTOMATIC_PAGE_CHUNKS)?,
        y: page_y.checked_mul(AUTOMATIC_PAGE_CHUNKS)?,
    };
    let max_chunk = ChunkCoord {
        x: min_chunk.x.checked_add(AUTOMATIC_PAGE_CHUNKS - 1)?,
        y: min_chunk.y.checked_add(AUTOMATIC_PAGE_CHUNKS - 1)?,
    };
    let page = WorldRect {
        min: min_chunk.bounds().ok()?.min,
        max: max_chunk.bounds().ok()?.max,
    };
    let clipped = WorldRect {
        min: WorldPosition {
            x: page.min.x.max(bounds.min.x),
            y: page.min.y.max(bounds.min.y),
        },
        max: WorldPosition {
            x: page.max.x.min(bounds.max.x),
            y: page.max.y.min(bounds.max.y),
        },
    };
    (clipped.max.x > clipped.min.x && clipped.max.y > clipped.min.y).then_some(clipped)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_core::WorldConfig;

    fn test_job(id: GenerationId) -> GenerationJob {
        let world = World::new(1, WorldConfig::new(64, 64).unwrap());
        let request = world
            .missing_chunk_load_requests(world.initial_bounds())
            .unwrap()
            .pop()
            .unwrap();
        GenerationJob {
            id,
            seed: 1,
            requests: vec![request],
        }
    }

    #[test]
    fn pager_is_bounded_and_starts_at_the_focus_page() {
        let bounds = WorldRect {
            min: WorldPosition {
                x: -8_192,
                y: -8_192,
            },
            max: WorldPosition { x: 8_192, y: 8_192 },
        };
        let mut pager = ViewportPager::new(bounds, WorldPosition { x: 0, y: 0 }).unwrap();
        let world = World::new(1, WorldConfig::new(64, 64).unwrap());
        let first = pager.next_requests(&world).unwrap().unwrap();
        assert!(first.len() <= (AUTOMATIC_PAGE_CHUNKS * AUTOMATIC_PAGE_CHUNKS) as usize);
        assert!(
            first
                .iter()
                .any(|request| request.coord() == ChunkCoord { x: 0, y: 0 })
        );
        let second = pager.next_requests(&world).unwrap().unwrap();
        assert!(second.len() <= (AUTOMATIC_PAGE_CHUNKS * AUTOMATIC_PAGE_CHUNKS) as usize);
        assert!(
            !second
                .iter()
                .any(|request| request.coord() == ChunkCoord { x: 0, y: 0 })
        );
    }

    #[test]
    fn cancelled_job_discards_queued_loads_until_its_terminal_message() {
        let (jobs, _jobs_rx) = mpsc::sync_channel(1);
        let (completed_tx, completed) = mpsc::sync_channel(64);
        let generator = WorldGenerator {
            jobs,
            completed,
            cancelled_job: Arc::new(AtomicU64::new(0)),
            disconnected: Cell::new(false),
        };
        let world = World::new(1, WorldConfig::new(64, 64).unwrap());
        let request = world
            .missing_chunk_load_requests(world.initial_bounds())
            .unwrap()
            .pop()
            .unwrap();
        completed_tx
            .send(WorkerMessage::Load {
                id: 7,
                load: World::generate_chunk_load(1, request),
            })
            .unwrap();
        completed_tx
            .send(WorkerMessage::Done {
                id: 7,
                outcome: GenerationOutcome::Cancelled,
            })
            .unwrap();

        let poll = generator.drain(7, 16, true);
        assert!(poll.loads.is_empty());
        assert_eq!(poll.outcome, Some(GenerationOutcome::Cancelled));
    }

    #[test]
    fn request_returns_the_original_job_when_the_worker_queue_is_full() {
        let (jobs, _jobs_rx) = mpsc::sync_channel(1);
        let (_completed_tx, completed) = mpsc::sync_channel(64);
        let generator = WorldGenerator {
            jobs,
            completed,
            cancelled_job: Arc::new(AtomicU64::new(0)),
            disconnected: Cell::new(false),
        };
        generator.request(test_job(1)).unwrap();

        match generator.request(test_job(2)) {
            Err(mpsc::TrySendError::Full(job)) => {
                assert_eq!(job.id, 2);
                assert_eq!(job.seed, 1);
                assert_eq!(job.requests.len(), 1);
            }
            _ => panic!("full queue must return the unsent generation job"),
        }
    }

    #[test]
    fn disconnected_request_returns_the_job_and_marks_the_worker_unavailable() {
        let (jobs, jobs_rx) = mpsc::sync_channel(1);
        drop(jobs_rx);
        let (_completed_tx, completed) = mpsc::sync_channel(64);
        let generator = WorldGenerator {
            jobs,
            completed,
            cancelled_job: Arc::new(AtomicU64::new(0)),
            disconnected: Cell::new(false),
        };

        match generator.request(test_job(3)) {
            Err(mpsc::TrySendError::Disconnected(job)) => {
                assert_eq!(job.id, 3);
                assert_eq!(job.requests.len(), 1);
            }
            _ => panic!("stopped worker must return the unsent generation job"),
        }
        assert!(!generator.is_available());
    }
}
