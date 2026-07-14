use std::{
    cell::Cell,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    thread,
};

use rayon::{Scope, ThreadPool, ThreadPoolBuilder};
use sim_core::{
    ChunkCoord, ChunkLoadRequest, GenerateAreaError, World, WorldChunkLoad, WorldPosition,
    WorldRect,
};

pub const AUTOMATIC_PAGE_CHUNKS: i64 = 32;
const AUTOMATIC_PAGE_HALF: i64 = AUTOMATIC_PAGE_CHUNKS / 2;

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

struct TaskCompletion {
    index: usize,
    load: Option<WorldChunkLoad>,
}

const COMPLETED_CHANNEL_CAPACITY: usize = 64;
const TASKS_PER_WORKER: usize = 4;

/// One persistent coordinator backed by a fixed computation pool. Workers
/// produce immutable world payloads; only the viewer event loop applies them.
pub struct WorldGenerator {
    jobs: SyncSender<GenerationJob>,
    completed: Receiver<WorkerMessage>,
    cancelled_job: Arc<AtomicU64>,
    disconnected: Cell<bool>,
}

impl WorldGenerator {
    pub fn new() -> Self {
        Self::with_worker_count(default_worker_count())
    }

    fn with_worker_count(worker_count: usize) -> Self {
        assert!(worker_count > 0, "generation needs at least one worker");
        let (jobs_tx, jobs_rx) = mpsc::sync_channel::<GenerationJob>(1);
        let (completed_tx, completed_rx) =
            mpsc::sync_channel::<WorkerMessage>(COMPLETED_CHANNEL_CAPACITY);
        let cancelled_job = Arc::new(AtomicU64::new(0));
        let worker_cancelled_job = Arc::clone(&cancelled_job);
        let pool = ThreadPoolBuilder::new()
            .num_threads(worker_count)
            .thread_name(|index| format!("world-generator-{index}"))
            .build()
            .expect("build world generation pool");
        thread::Builder::new()
            .name("world-generator-coordinator".to_owned())
            .spawn(move || {
                while let Ok(job) = jobs_rx.recv() {
                    if !run_generation_job(&pool, job, &worker_cancelled_job, &completed_tx) {
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

fn default_worker_count() -> usize {
    thread::available_parallelism()
        .map(|parallelism| parallelism.get().saturating_sub(1).max(1))
        .unwrap_or(1)
}

fn spawn_generation_task<'scope>(
    scope: &Scope<'scope>,
    completed: SyncSender<TaskCompletion>,
    cancelled_job: Arc<AtomicU64>,
    id: GenerationId,
    seed: u64,
    index: usize,
    request: ChunkLoadRequest,
) {
    scope.spawn(move |_| {
        let load = (cancelled_job.load(Ordering::Acquire) != id)
            .then(|| World::generate_chunk_load(seed, request));
        let _ = completed.send(TaskCompletion { index, load });
    });
}

fn run_generation_job(
    pool: &ThreadPool,
    job: GenerationJob,
    cancelled_job: &Arc<AtomicU64>,
    completed: &SyncSender<WorkerMessage>,
) -> bool {
    let id = job.id;
    let seed = job.seed;
    let request_count = job.requests.len();
    let task_window = (pool.current_num_threads() * TASKS_PER_WORKER)
        .min(COMPLETED_CHANNEL_CAPACITY)
        .min(request_count)
        .max(1);
    let (task_tx, task_rx) = mpsc::sync_channel::<TaskCompletion>(task_window);
    let (output_connected, cancelled, next_output) = pool.in_place_scope(move |scope| {
        let mut output_connected = true;
        let mut cancelled = cancelled_job.load(Ordering::Acquire) == id;
        let mut next_request = 0;
        let mut next_output = 0;
        let mut active = 0;
        let mut buffered = 0;
        let mut prepared_until = 0;
        let mut ready: Vec<Option<WorldChunkLoad>> = std::iter::repeat_with(|| None)
            .take(request_count)
            .collect();
        if !cancelled {
            prepare_request_window(seed, &job.requests, &mut prepared_until, task_window);
            cancelled = cancelled_job.load(Ordering::Acquire) == id;
        }
        while !cancelled && active + buffered < task_window && next_request < request_count {
            spawn_generation_task(
                scope,
                task_tx.clone(),
                Arc::clone(cancelled_job),
                id,
                seed,
                next_request,
                job.requests[next_request],
            );
            next_request += 1;
            active += 1;
        }

        while active > 0 {
            let Ok(task) = task_rx.recv() else {
                output_connected = false;
                break;
            };
            active -= 1;
            if cancelled_job.load(Ordering::Acquire) == id {
                cancelled = true;
                ready.iter_mut().for_each(|load| {
                    load.take();
                });
                buffered = 0;
            } else if let Some(load) = task.load {
                ready[task.index] = Some(load);
                buffered += 1;
            } else {
                cancelled = true;
            }

            while output_connected && !cancelled && next_output < request_count {
                let Some(load) = ready[next_output].take() else {
                    break;
                };
                buffered -= 1;
                if completed.send(WorkerMessage::Load { id, load }).is_err() {
                    output_connected = false;
                    break;
                }
                next_output += 1;
                if cancelled_job.load(Ordering::Acquire) == id {
                    cancelled = true;
                    ready.iter_mut().for_each(|load| {
                        load.take();
                    });
                    buffered = 0;
                }
            }

            if output_connected && !cancelled {
                while active + buffered < task_window && next_request < request_count {
                    if next_request == prepared_until {
                        prepare_request_window(
                            seed,
                            &job.requests,
                            &mut prepared_until,
                            task_window,
                        );
                        if cancelled_job.load(Ordering::Acquire) == id {
                            cancelled = true;
                            break;
                        }
                    }
                    spawn_generation_task(
                        scope,
                        task_tx.clone(),
                        Arc::clone(cancelled_job),
                        id,
                        seed,
                        next_request,
                        job.requests[next_request],
                    );
                    next_request += 1;
                    active += 1;
                }
            }
        }
        (output_connected, cancelled, next_output)
    });

    if !output_connected {
        return false;
    }
    let outcome = if cancelled || next_output < request_count {
        GenerationOutcome::Cancelled
    } else {
        GenerationOutcome::Completed
    };
    completed.send(WorkerMessage::Done { id, outcome }).is_ok()
}

fn prepare_request_window(
    seed: u64,
    requests: &[ChunkLoadRequest],
    prepared_until: &mut usize,
    task_window: usize,
) {
    let end = prepared_until
        .saturating_add(task_window)
        .min(requests.len());
    World::prepare_chunk_loads(seed, &requests[*prepared_until..end]);
    *prepared_until = end;
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

        let min_page_x = chunk_page(min_chunk.x);
        let max_page_x = chunk_page(max_chunk.x);
        let min_page_y = chunk_page(min_chunk.y);
        let max_page_y = chunk_page(max_chunk.y);
        let focus_chunk = ChunkCoord::from_world_position(focus);
        let focus_page_x = chunk_page(focus_chunk.x).clamp(min_page_x, max_page_x);
        let focus_page_y = chunk_page(focus_chunk.y).clamp(min_page_y, max_page_y);
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

fn chunk_page(chunk: i64) -> i64 {
    chunk
        .saturating_add(AUTOMATIC_PAGE_HALF)
        .div_euclid(AUTOMATIC_PAGE_CHUNKS)
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
        x: page_x
            .checked_mul(AUTOMATIC_PAGE_CHUNKS)?
            .checked_sub(AUTOMATIC_PAGE_HALF)?,
        y: page_y
            .checked_mul(AUTOMATIC_PAGE_CHUNKS)?
            .checked_sub(AUTOMATIC_PAGE_HALF)?,
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
    use std::time::{Duration, Instant};

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

    fn wait_for_job(
        generator: &WorldGenerator,
        id: GenerationId,
        expected_limit: usize,
    ) -> (Vec<WorldChunkLoad>, GenerationOutcome) {
        let deadline = Instant::now() + Duration::from_secs(30);
        let mut loads = Vec::new();
        loop {
            let poll = generator.drain(id, expected_limit.max(1), false);
            loads.extend(poll.loads);
            if let Some(outcome) = poll.outcome {
                return (loads, outcome);
            }
            assert!(
                Instant::now() < deadline,
                "generation job {id} produced {} loads but did not finish before the test deadline",
                loads.len()
            );
            thread::yield_now();
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
        let coords: Vec<_> = first.iter().map(|request| request.coord()).collect();
        assert_eq!(coords.iter().map(|coord| coord.x).min(), Some(-16));
        assert_eq!(coords.iter().map(|coord| coord.x).max(), Some(15));
        assert_eq!(coords.iter().map(|coord| coord.y).min(), Some(-16));
        assert_eq!(coords.iter().map(|coord| coord.y).max(), Some(15));
        let second = pager.next_requests(&world).unwrap().unwrap();
        assert!(second.len() <= (AUTOMATIC_PAGE_CHUNKS * AUTOMATIC_PAGE_CHUNKS) as usize);
        assert!(
            !second
                .iter()
                .any(|request| request.coord() == ChunkCoord { x: 0, y: 0 })
        );
    }

    #[test]
    fn worker_pool_sizes_preserve_request_order_and_exact_output() {
        let seed = 91;
        let world = World::new(seed, WorldConfig::new(256, 256).unwrap());
        let requests = world
            .missing_chunk_load_requests(world.initial_bounds())
            .unwrap();
        let expected: Vec<_> = requests
            .iter()
            .copied()
            .map(|request| World::generate_chunk_load(seed, request))
            .collect();
        for (worker_count, id) in [(1, 40), (4, 41)] {
            let generator = WorldGenerator::with_worker_count(worker_count);
            generator
                .request(GenerationJob {
                    id,
                    seed,
                    requests: requests.clone(),
                })
                .unwrap();

            let (actual, outcome) = wait_for_job(&generator, id, expected.len());
            assert_eq!(outcome, GenerationOutcome::Completed);
            assert_eq!(actual, expected);
        }
    }

    #[test]
    fn cold_regional_preparation_completes_with_one_worker() {
        let seed = 0x434f_4c44_5f4f_4e45;
        let world = World::new(seed, WorldConfig::new(512, 512).unwrap());
        let requests = world
            .missing_chunk_load_requests(world.initial_bounds())
            .unwrap();
        let request_count = requests.len();
        let generator = WorldGenerator::with_worker_count(1);
        generator
            .request(GenerationJob {
                id: 39,
                seed,
                requests,
            })
            .unwrap();

        let (loads, outcome) = wait_for_job(&generator, 39, request_count);
        assert_eq!(outcome, GenerationOutcome::Completed);
        assert_eq!(loads.len(), request_count);
    }

    #[test]
    fn cancelled_pool_job_does_not_start_queued_chunk_work() {
        let seed = 92;
        let world = World::new(seed, WorldConfig::new(512, 512).unwrap());
        let requests = world
            .missing_chunk_load_requests(world.initial_bounds())
            .unwrap();
        let request_count = requests.len();
        let generator = WorldGenerator::with_worker_count(4);
        generator.cancel(42);
        generator
            .request(GenerationJob {
                id: 42,
                seed,
                requests,
            })
            .unwrap();

        let (loads, outcome) = wait_for_job(&generator, 42, request_count);
        assert!(loads.is_empty());
        assert_eq!(outcome, GenerationOutcome::Cancelled);
    }

    #[test]
    #[ignore = "manual release throughput measurement"]
    fn release_generation_pool_throughput() {
        const SEED: u64 = 10_001;
        let worker_count = std::env::var("SIM_GENERATION_BENCH_WORKERS")
            .ok()
            .map(|value| {
                value
                    .parse()
                    .expect("benchmark worker count must be an integer")
            })
            .unwrap_or_else(default_worker_count);
        let world_size = std::env::var("SIM_GENERATION_BENCH_WORLD_SIZE")
            .ok()
            .map(|value| {
                value
                    .parse()
                    .expect("benchmark world size must be an integer")
            })
            .unwrap_or(4_096);
        let world = World::new(
            SEED,
            WorldConfig::new(world_size, world_size).expect("benchmark world size must be valid"),
        );
        let requests = world
            .missing_chunk_load_requests(world.initial_bounds())
            .unwrap();
        let request_count = requests.len();
        let generator = WorldGenerator::with_worker_count(worker_count);
        let started = Instant::now();
        generator
            .request(GenerationJob {
                id: SEED,
                seed: SEED,
                requests,
            })
            .unwrap();
        let (loads, outcome) = wait_for_job(&generator, SEED, request_count);
        assert_eq!(outcome, GenerationOutcome::Completed);
        assert_eq!(loads.len(), request_count);
        println!(
            "generation-workers={worker_count} chunks={request_count} elapsed-ms={:.1}",
            started.elapsed().as_secs_f64() * 1_000.0,
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
