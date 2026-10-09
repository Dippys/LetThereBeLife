//! Generation worker internals: per-job task fan-out on the rayon pool, ordered delivery, and request-window preparation.

use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc::{self, SyncSender},
    },
    thread,
};

use rayon::{Scope, ThreadPool};
use sim_core::{ChunkLoadRequest, World, WorldChunkLoad};

use super::{
    COMPLETED_CHANNEL_CAPACITY, ChunkSource, GenerationId, GenerationJob, GenerationOutcome,
    TASKS_PER_WORKER, TaskCompletion, WorkerMessage,
};

pub(super) fn default_worker_count() -> usize {
    thread::available_parallelism()
        .map(|parallelism| parallelism.get().saturating_sub(1).max(1))
        .unwrap_or(1)
}

fn spawn_generation_task<'scope>(
    scope: &Scope<'scope>,
    completed: SyncSender<TaskCompletion>,
    cancelled_job: Arc<AtomicU64>,
    id: GenerationId,
    index: usize,
    request: ChunkLoadRequest,
    source: ChunkSource,
) {
    scope.spawn(move |_| {
        let load = if cancelled_job.load(Ordering::Acquire) == id {
            Ok(None)
        } else {
            match source {
                ChunkSource::Procedural(seed) => {
                    Ok(Some(World::generate_chunk_load(seed, request)))
                }
                ChunkSource::Archive(archive) => {
                    archive.load_chunk(request).map(Some).map_err(|error| {
                        format!(
                            "could not read archived chunk {:?}: {error}",
                            request.coord()
                        )
                    })
                }
            }
        };
        let _ = completed.send(TaskCompletion { index, load });
    });
}

pub(super) fn run_generation_job(
    pool: &ThreadPool,
    job: GenerationJob,
    cancelled_job: &Arc<AtomicU64>,
    completed: &SyncSender<WorkerMessage>,
) -> bool {
    let id = job.id;
    let seed = job.seed;
    let archive = job.archive.clone();
    let source = archive
        .clone()
        .map_or(ChunkSource::Procedural(seed), ChunkSource::Archive);
    let request_count = job.requests.len();
    let task_window = (pool.current_num_threads() * TASKS_PER_WORKER)
        .min(COMPLETED_CHANNEL_CAPACITY)
        .min(request_count)
        .max(1);
    let (task_tx, task_rx) = mpsc::sync_channel::<TaskCompletion>(task_window);
    let (output_connected, cancelled, failed, next_output) = pool.in_place_scope(move |scope| {
        let mut output_connected = true;
        let mut cancelled = cancelled_job.load(Ordering::Acquire) == id;
        let mut failed = false;
        let mut next_request = 0;
        let mut next_output = 0;
        let mut active = 0;
        let mut buffered = 0;
        let mut prepared_until = 0;
        let mut ready: Vec<Option<WorldChunkLoad>> = std::iter::repeat_with(|| None)
            .take(request_count)
            .collect();
        if !cancelled && archive.is_none() {
            prepare_request_window(seed, &job.requests, &mut prepared_until, task_window);
            cancelled = cancelled_job.load(Ordering::Acquire) == id;
        }
        while !cancelled && active + buffered < task_window && next_request < request_count {
            spawn_generation_task(
                scope,
                task_tx.clone(),
                Arc::clone(cancelled_job),
                id,
                next_request,
                job.requests[next_request],
                source.clone(),
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
            } else {
                match task.load {
                    Ok(Some(load)) => {
                        ready[task.index] = Some(load);
                        buffered += 1;
                    }
                    Ok(None) => cancelled = true,
                    Err(error) => {
                        eprintln!("{error}");
                        failed = true;
                        cancelled = true;
                    }
                }
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
                    if archive.is_none() && next_request == prepared_until {
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
                        next_request,
                        job.requests[next_request],
                        source.clone(),
                    );
                    next_request += 1;
                    active += 1;
                }
            }
        }
        (output_connected, cancelled, failed, next_output)
    });

    if !output_connected {
        return false;
    }
    let outcome = if failed {
        GenerationOutcome::Failed
    } else if cancelled || next_output < request_count {
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
