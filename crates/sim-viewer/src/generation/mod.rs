//! Background world generation: job/outcome types and the `WorldGenerator` coordinator handle.

mod pager;
mod worker;

#[cfg(test)]
mod tests;

use std::{
    cell::Cell,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    thread,
};

use rayon::ThreadPoolBuilder;
use sim_core::{ChunkLoadRequest, WorldArchive, WorldChunkLoad};

pub use pager::ChunkPager;
use worker::{default_worker_count, run_generation_job};

pub const PAGE_CHUNKS: i64 = 32;
const PAGE_HALF: i64 = PAGE_CHUNKS / 2;

pub type GenerationId = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenerationKind {
    Bootstrap,
    Manual,
}

pub struct GenerationJob {
    pub id: GenerationId,
    pub seed: u64,
    pub requests: Vec<ChunkLoadRequest>,
    pub archive: Option<WorldArchive>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenerationOutcome {
    Completed,
    Cancelled,
    Failed,
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
    load: Result<Option<WorldChunkLoad>, String>,
}

#[derive(Clone)]
enum ChunkSource {
    Procedural(u64),
    Archive(WorldArchive),
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
