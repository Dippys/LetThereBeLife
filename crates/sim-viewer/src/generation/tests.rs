//! Generation pool ordering, cancellation, backpressure, and pager tests.

use super::*;
use sim_core::{ChunkCoord, World, WorldConfig, WorldPosition, WorldRect};
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
        archive: None,
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
    let mut pager = ChunkPager::new(bounds, WorldPosition { x: 0, y: 0 }).unwrap();
    let world = World::new(1, WorldConfig::new(64, 64).unwrap());
    let first = pager.next_requests(&world).unwrap().unwrap();
    assert!(first.len() <= (PAGE_CHUNKS * PAGE_CHUNKS) as usize);
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
    assert!(second.len() <= (PAGE_CHUNKS * PAGE_CHUNKS) as usize);
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
                archive: None,
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
            archive: None,
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
            archive: None,
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
            archive: None,
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
