//! Terrain summary cache, sampling step, and cache-sync tests.

use std::time::Instant;

use sim_core::{BiomeType, ChunkCoord, World, WorldConfig, WorldPosition, WorldRect};

use crate::render::colors::terrain_color;
use crate::render::gpu::Instance;
use crate::render::summary::{
    CacheSyncAction, FOREST_VISUAL, GRASS_VISUAL, LAKE_VISUAL, OCEAN_SHALLOW, RIVER_VISUAL,
    SNOW_VISUAL, SummaryAccumulator, VisualSample, WorldSummaryCache, build_world_instances,
    cache_sync_action, terrain_sample_step,
};

#[test]
fn coarse_view_reduces_terrain_instances() {
    let world = World::generate(1, WorldConfig::new(64, 64).unwrap());
    let bounds = world.initial_bounds();
    let (full, _) = build_world_instances(&world, bounds, 1);
    let (coarse, _) = build_world_instances(&world, bounds, 4);

    assert_eq!(full.len(), 4_096);
    assert!((256..=512).contains(&coarse.len()));
    assert_eq!(
        coarse
            .iter()
            .filter(|instance| instance.size == [4.0, 4.0])
            .count(),
        256
    );
}

#[test]
fn coarse_summary_preserves_unaligned_features_as_density_markers() {
    let world = World::generate(42, WorldConfig::new(128, 128).unwrap());
    let bounds = world.initial_bounds();
    let (_, exact) = build_world_instances(&world, bounds, 1);
    assert!(!exact.is_empty(), "probe must contain generated features");
    assert!(exact.iter().any(|instance| {
        instance.position[0] as i64 % 64 != 0 || instance.position[1] as i64 % 64 != 0
    }));

    let (_, coarse) = build_world_instances(&world, bounds, 64);
    assert!(!coarse.is_empty());
    assert!(coarse.len() <= 4, "one marker per coarse block");
}

#[test]
fn summary_priority_preserves_water_coasts_and_mountain_minorities() {
    let detail = |base: usize, minority: usize| {
        let mut summary = SummaryAccumulator::default();
        summary.visuals[base].count = 100;
        summary.visuals[minority].count = 1;
        (summary.base_visual(), summary.detail_visual(base))
    };
    assert_eq!(
        detail(GRASS_VISUAL, RIVER_VISUAL),
        (Some(GRASS_VISUAL), Some(RIVER_VISUAL))
    );
    assert_eq!(
        detail(GRASS_VISUAL, LAKE_VISUAL),
        (Some(GRASS_VISUAL), Some(LAKE_VISUAL))
    );
    assert_eq!(
        detail(GRASS_VISUAL, OCEAN_SHALLOW),
        (Some(GRASS_VISUAL), Some(OCEAN_SHALLOW))
    );
    assert_eq!(
        detail(FOREST_VISUAL, SNOW_VISUAL),
        (Some(FOREST_VISUAL), Some(SNOW_VISUAL))
    );
}

#[test]
fn feature_summary_marker_area_increases_with_density() {
    let world = World::generate(1, WorldConfig::new(64, 64).unwrap());
    let position = WorldPosition { x: 0, y: 0 };
    let cell = world.cell(position).expect("origin is resident");
    let block = WorldRect {
        min: position,
        max: WorldPosition { x: 64, y: 64 },
    };
    let marker = |count| {
        let mut summary = SummaryAccumulator::default();
        summary.observe_cell(position, position, cell);
        summary.feature_counts[0] = count;
        let mut terrain = Vec::new();
        let mut features = Vec::new();
        summary.instances(block, position, &mut terrain, &mut features);
        features[0]
    };
    let sparse = marker(1);
    let dense = marker(100);
    assert!(dense.size[0] > sparse.size[0]);
    assert!(dense.size[1] > sparse.size[1]);
}

#[test]
fn coarse_summary_preserves_a_major_river_that_misses_block_origins() {
    let mut world = World::generate(1, WorldConfig::new(64, 64).unwrap());
    let river_probe = WorldRect {
        min: WorldPosition {
            x: -14_592,
            y: -14_976,
        },
        max: WorldPosition {
            x: -14_336,
            y: -14_720,
        },
    };
    world.generate_area(river_probe).unwrap();
    let mut river_colors = Vec::new();
    let mut has_unaligned_river = false;
    world.visit_cells_in(river_probe, |position, cell| {
        if cell.biome() == BiomeType::River {
            river_colors.push(terrain_color(cell));
            has_unaligned_river |= position.x.rem_euclid(64) != 0 || position.y.rem_euclid(64) != 0;
        }
    });
    river_colors.sort_unstable();
    river_colors.dedup();
    assert!(
        has_unaligned_river,
        "canonical probe must contain an unaligned river"
    );

    let (coarse, _) = build_world_instances(&world, river_probe, 64);
    assert!(
        coarse
            .iter()
            .any(|instance| river_colors.binary_search(&instance.color).is_ok()),
        "minority river color disappeared from coarse summaries"
    );
}

#[test]
fn summary_cache_retains_only_the_active_step_and_resident_margin() {
    let mut world = World::generate(7, WorldConfig::new(128, 64).unwrap());
    let distant = WorldRect {
        min: WorldPosition { x: 512, y: 0 },
        max: WorldPosition { x: 576, y: 64 },
    };
    world.generate_area(distant).unwrap();
    let mut cache = WorldSummaryCache::default();
    let initial = world.initial_bounds();
    let _ = cache.sync(&world, None, initial, 16, None);
    assert_eq!(cache.step, 16);
    assert_eq!(cache.chunks.len(), 4);
    assert!(cache.logical_bytes() > 0);

    let _ = cache.sync(&world, None, distant, 16, None);
    assert_eq!(cache.chunks.len(), 1);
    assert!(cache.chunks.contains_key(&ChunkCoord { x: 8, y: 0 }));

    let _ = cache.sync(&world, None, distant, 32, None);
    assert_eq!(cache.step, 32);
    assert_eq!(cache.chunks.len(), 1);

    let mut repeated = WorldSummaryCache::default();
    assert_eq!(
        cache.sync(&world, None, distant, 32, Some(distant)),
        repeated.sync(&world, None, distant, 32, None),
        "parallel summary construction and change-bound invalidation must be deterministic"
    );
}

#[test]
#[ignore = "release-only renderer summary measurement"]
fn release_summary_cache_measurement() {
    let side = std::env::var("SIM_SUMMARY_BENCH_SIZE")
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(1_024);
    let world = World::generate(1, WorldConfig::new(side, side).unwrap());
    let bounds = world.initial_bounds();
    for step in [2, 4, 8, 16, 32, 64] {
        let mut cache = WorldSummaryCache::default();
        let started = Instant::now();
        let (terrain, features) = cache.sync(&world, None, bounds, step, None);
        let elapsed = started.elapsed();
        println!(
            "summary-bench side={side} step={step} chunks={} cache_bytes={} terrain_instances={} feature_instances={} gpu_instance_bytes={} build_ms={:.3}",
            cache.chunks.len(),
            cache.logical_bytes(),
            terrain.len(),
            features.len(),
            (terrain.len() + features.len()) * size_of::<Instance>(),
            elapsed.as_secs_f64() * 1_000.0,
        );
    }
}

#[test]
fn sample_step_targets_two_pixel_blocks_and_chunk_divisors() {
    assert_eq!(size_of::<VisualSample>(), 12);
    assert_eq!(size_of::<SummaryAccumulator>(), 188);
    assert_eq!(terrain_sample_step(4.0), 1);
    assert_eq!(terrain_sample_step(1.1), 2);
    assert_eq!(terrain_sample_step(0.5), 4);
    assert_eq!(terrain_sample_step(0.01), 64);
}

#[test]
fn coarse_blocks_follow_exact_loaded_tile_coverage() {
    let initial = World::generate(1, WorldConfig::new(96, 64).unwrap());
    let (initial_instances, _) = build_world_instances(
        &initial,
        WorldRect {
            min: WorldPosition { x: 0, y: 0 },
            max: WorldPosition { x: 128, y: 64 },
        },
        64,
    );
    assert!(
        initial_instances
            .iter()
            .all(|instance| instance.position[0] + instance.size[0] <= 96.0)
    );

    let mut boundary = World::generate(1, WorldConfig::new(96, 64).unwrap());
    let boundary_bounds = WorldRect {
        min: WorldPosition { x: 96, y: 0 },
        max: WorldPosition { x: 128, y: 64 },
    };
    boundary.generate_area(boundary_bounds).unwrap();
    let (boundary_instances, _) = build_world_instances(
        &boundary,
        WorldRect {
            min: WorldPosition { x: 0, y: 0 },
            max: boundary_bounds.max,
        },
        64,
    );
    assert!(
        boundary_instances
            .iter()
            .any(|instance| { instance.position == [64.0, 0.0] && instance.size == [64.0, 64.0] })
    );

    let mut corner = World::generate(1, WorldConfig::new(96, 100).unwrap());
    let corner_bounds = WorldRect {
        min: WorldPosition { x: 96, y: 64 },
        max: WorldPosition { x: 128, y: 128 },
    };
    corner.generate_area(corner_bounds).unwrap();
    let (corner_instances, _) = build_world_instances(
        &corner,
        WorldRect {
            min: WorldPosition { x: 0, y: 0 },
            max: corner_bounds.max,
        },
        64,
    );
    assert!(
        corner_instances
            .iter()
            .any(|instance| { instance.position == [64.0, 64.0] && instance.size == [64.0, 64.0] })
    );

    let mut expanded = World::generate(1, WorldConfig::new(64, 64).unwrap());
    let generated_bounds = WorldRect {
        min: WorldPosition { x: 128, y: 0 },
        max: WorldPosition { x: 192, y: 64 },
    };
    expanded.generate_area(generated_bounds).unwrap();
    let (generated_instances, _) = build_world_instances(&expanded, generated_bounds, 4);
    assert!(generated_instances.iter().all(|instance| {
        instance.position[0] >= 128.0
            && instance.position[0] + instance.size[0] <= 192.0
            && instance.position[1] + instance.size[1] <= 64.0
    }));
    assert!(
        generated_instances
            .iter()
            .any(|instance| instance.position[0] == 188.0 && instance.size[0] == 4.0)
    );
}

#[test]
fn unloaded_bootstrap_has_no_terrain_instances() {
    let world = World::new(1, WorldConfig::new(64, 64).unwrap());
    let (terrain, features) = build_world_instances(&world, world.initial_bounds(), 1);

    assert!(terrain.is_empty());
    assert!(features.is_empty());
}

#[test]
fn cache_sync_rebuilds_streamed_visible_work_without_offscreen_uploads() {
    assert_eq!(
        cache_sync_action(true, true, true, false, true),
        CacheSyncAction::Skip
    );
    assert_eq!(
        cache_sync_action(true, true, true, true, true),
        CacheSyncAction::Rebuild
    );
    assert_eq!(
        cache_sync_action(true, true, true, false, false),
        CacheSyncAction::AdvanceRevision
    );
    assert_eq!(
        cache_sync_action(false, true, false, false, false),
        CacheSyncAction::Rebuild
    );
}
