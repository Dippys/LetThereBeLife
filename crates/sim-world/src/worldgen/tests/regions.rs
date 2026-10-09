//! Regional cache, cross-region determinism, and parallelism tests.

use super::*;

#[test]
fn chunk_context_uses_a_safe_overflow_path_beyond_the_measured_fast_bound() {
    let mut context = ChunkContext {
        seed: PROBE_SEED,
        origin_x: 0,
        origin_y: 0,
        elevation: [38_000; 9],
        water_depth: [0; 9],
        temperature: [22_000; 9],
        moisture: [20_000; 9],
        roughness: [0; 9],
        rivers: [EMPTY_SEGMENT; MAX_CHUNK_RIVERS],
        river_len: 0,
        overflow_rivers: Vec::new(),
    };
    for channel_id in 1..=MAX_CHUNK_RIVERS as u32 + 1 {
        context.push_river(RiverSegment {
            ax: 0,
            ay: 0,
            bx: 32,
            by: 0,
            channel_id,
            surface_a: 38_000,
            surface_b: 37_000,
            half_width: 2,
            stream_order: 1,
        });
    }
    assert_eq!(context.river_len, MAX_CHUNK_RIVERS);
    assert_eq!(context.overflow_rivers.len(), 1);
    assert_eq!(context.river_segments().count(), MAX_CHUNK_RIVERS + 1);
}

#[test]
fn clearing_region_cache_does_not_change_chunk_output() {
    use crate::{ChunkCoord, World};

    let seed = 73;
    let target = ChunkCoord { x: -1, y: -1 };
    lock_region_cache().clear();
    let expected = World::generate_chunk_at(seed, target).expect("target is representable");
    lock_region_cache().clear();
    let regenerated = World::generate_chunk_at(seed, target).expect("target is representable");
    assert_eq!(regenerated, expected);
}

#[test]
fn cross_region_chunks_are_order_and_worker_count_independent() {
    use crate::{ChunkCoord, World};

    let seed = 0x4352_4f53_5352_4547;
    let coords = [
        ChunkCoord { x: -65, y: -1 },
        ChunkCoord { x: -64, y: -1 },
        ChunkCoord { x: -1, y: -65 },
        ChunkCoord { x: -1, y: -64 },
        ChunkCoord { x: 63, y: 0 },
        ChunkCoord { x: 64, y: 0 },
        ChunkCoord { x: 0, y: 63 },
        ChunkCoord { x: 0, y: 64 },
    ];
    let generate = |workers, ordered: Vec<ChunkCoord>| {
        ThreadPoolBuilder::new()
            .num_threads(workers)
            .build()
            .unwrap()
            .install(|| {
                let mut chunks: Vec<_> = ordered
                    .into_par_iter()
                    .map(|coord| {
                        (
                            coord,
                            World::generate_chunk_at(seed, coord)
                                .expect("test coordinate is inside the finite world"),
                        )
                    })
                    .collect();
                chunks.sort_unstable_by_key(|(coord, _)| (coord.x, coord.y));
                chunks
            })
    };

    let single = generate(1, coords.to_vec());
    let mut reversed = coords.to_vec();
    reversed.reverse();
    let parallel = generate(4, reversed);
    assert_eq!(parallel, single);
}

#[test]
fn concurrent_region_requests_share_one_build() {
    const WORKERS: usize = 8;
    let seed = 0x5348_4152_4544_4341;
    lock_region_cache().clear();
    let barrier = SyncArc::new(Barrier::new(WORKERS));
    let handles: Vec<_> = (0..WORKERS)
        .map(|_| {
            let barrier = SyncArc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                region(seed, 3, -4)
            })
        })
        .collect();
    let maps: Vec<_> = handles
        .into_iter()
        .map(|handle| handle.join().expect("region requester must not panic"))
        .collect();

    assert!(
        maps.iter().all(|map| Arc::ptr_eq(map, &maps[0])),
        "concurrent requests for one region must share its materialization"
    );
}

#[test]
fn regional_parallelism_preserves_exact_output_across_pool_sizes() {
    let build = |workers| {
        ThreadPoolBuilder::new()
            .num_threads(workers)
            .build()
            .unwrap()
            .install(|| RegionMap::build(0x5041_5241_4c4c_454c, -7, 7))
    };
    let single = build(1);
    let parallel = build(4);

    assert_eq!(parallel.elevation, single.elevation);
    assert_eq!(parallel.water_depth, single.water_depth);
    assert_eq!(parallel.temperature, single.temperature);
    assert_eq!(parallel.moisture, single.moisture);
    assert_eq!(parallel.roughness, single.roughness);
    assert_eq!(parallel.rivers, single.rivers);
}
