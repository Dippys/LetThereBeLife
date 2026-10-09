//! Lake, river, channel, and wetland generation tests.

use super::*;

#[test]
fn adjacent_regions_share_canonical_water_and_crossing_channels() {
    let mut crossing_channels = 0;
    let mut wet_seam_nodes = 0;
    for seed in [PROBE_SEED, 42] {
        for seam_region in [-1_i64, 0, 1] {
            let left = RegionMap::build(seed, seam_region - 1, 0);
            let right = RegionMap::build(seed, seam_region, 0);
            let seam_x = seam_region * REGION_SIZE;
            for j in 0..GRID {
                let left_depth = left.water_depth[j * GRID + GRID - 1];
                let right_depth = right.water_depth[j * GRID];
                assert_eq!(left_depth, right_depth);
                wet_seam_nodes += usize::from(left_depth >= LAKE_MIN_DEPTH);
            }
            for segment in left.rivers.iter().filter(|segment| {
                i64::from(segment.ax.min(segment.bx)) < seam_x
                    && i64::from(segment.ax.max(segment.bx)) >= seam_x
            }) {
                assert!(right.rivers.contains(segment));
                crossing_channels += 1;
            }

            let top = RegionMap::build(seed, 0, seam_region - 1);
            let bottom = RegionMap::build(seed, 0, seam_region);
            let seam_y = seam_region * REGION_SIZE;
            for i in 0..GRID {
                let top_depth = top.water_depth[(GRID - 1) * GRID + i];
                let bottom_depth = bottom.water_depth[i];
                assert_eq!(top_depth, bottom_depth);
                wet_seam_nodes += usize::from(top_depth >= LAKE_MIN_DEPTH);
            }
            for segment in top.rivers.iter().filter(|segment| {
                i64::from(segment.ay.min(segment.by)) < seam_y
                    && i64::from(segment.ay.max(segment.by)) >= seam_y
            }) {
                assert!(bottom.rivers.contains(segment));
                crossing_channels += 1;
            }
        }
    }
    assert!(
        crossing_channels > 0,
        "canonical channels never crossed the tested region seams"
    );
    assert!(
        wet_seam_nodes > 0,
        "canonical lakes never reached the tested region seams"
    );
}

#[test]
fn flooded_lattice_nodes_render_as_water_without_surface_features() {
    let mut checked = 0;
    for seed in [PROBE_SEED, 42] {
        for region_y in -1..=1_i64 {
            for region_x in -1..=1_i64 {
                let map = RegionMap::build(seed, region_x, region_y);
                for (node, &depth) in map.water_depth.iter().enumerate() {
                    if depth < LAKE_MIN_DEPTH || checked >= 48 {
                        continue;
                    }
                    let x = region_x * REGION_SIZE + (node % GRID) as i64 * NODE_STEP;
                    let y = region_y * REGION_SIZE + (node / GRID) as i64 * NODE_STEP;
                    let origin_x = x.div_euclid(CHUNK_SIZE) * CHUNK_SIZE;
                    let origin_y = y.div_euclid(CHUNK_SIZE) * CHUNK_SIZE;
                    let context = ChunkContext::new(seed, origin_x, origin_y);
                    let (cell, feature) = context.generate(x, y);
                    assert!(matches!(
                        cell.surface(),
                        SurfaceType::DeepWater | SurfaceType::ShallowWater
                    ));
                    assert_eq!(cell.biome(), BiomeType::Lake);
                    assert!(feature.is_none(), "water node {node} emitted a feature");
                    checked += 1;
                }
            }
        }
    }
    assert!(
        checked >= 16,
        "expected representative flooded lattice nodes"
    );
}

#[test]
fn graded_mountain_river_core_wins_in_any_segment_order() {
    let core = RiverSegment {
        ax: 0,
        ay: 16,
        bx: 32,
        by: 16,
        channel_id: 1,
        surface_a: 42_000,
        surface_b: 40_000,
        half_width: 8,
        stream_order: 3,
    };
    let bank = RiverSegment {
        ax: 23,
        ay: 0,
        bx: 23,
        by: 32,
        channel_id: 2,
        surface_a: 41_000,
        surface_b: 41_000,
        half_width: 8,
        stream_order: 2,
    };

    for ordered in [[core, bank], [bank, core]] {
        let mut rivers = [EMPTY_SEGMENT; MAX_CHUNK_RIVERS];
        rivers[..ordered.len()].copy_from_slice(&ordered);
        let context = ChunkContext {
            seed: PROBE_SEED,
            origin_x: 0,
            origin_y: 0,
            elevation: [40_000; 9],
            water_depth: [0; 9],
            temperature: [20_000; 9],
            moisture: [20_000; 9],
            roughness: [0; 9],
            rivers,
            river_len: ordered.len(),
            overflow_rivers: Vec::new(),
        };

        let (cell, feature) = context.generate(16, 16);
        assert_eq!(cell.surface(), SurfaceType::DeepWater);
        assert_eq!(cell.biome(), BiomeType::River);
        assert_eq!(cell.elevation, 41_000);
        assert!(feature.is_none());
    }
}

#[test]
fn wetlands_require_low_slope_hydrologic_evidence() {
    let stream = RiverSegment {
        ax: 0,
        ay: 0,
        bx: 32,
        by: 0,
        channel_id: 1,
        surface_a: 38_000,
        surface_b: 37_000,
        half_width: 2,
        stream_order: 1,
    };
    let context = |elevation: [i32; 9], water_depth: [i32; 9], moisture| {
        let mut rivers = [EMPTY_SEGMENT; MAX_CHUNK_RIVERS];
        rivers[0] = stream;
        ChunkContext {
            seed: PROBE_SEED,
            origin_x: 0,
            origin_y: 0,
            elevation,
            water_depth,
            temperature: [22_000; 9],
            moisture: [moisture; 9],
            roughness: [0; 9],
            rivers,
            river_len: 1,
            overflow_rivers: Vec::new(),
        }
    };

    let floodplain = context([38_000; 9], [0; 9], 52_000).generate(16, 16).0;
    assert_eq!(floodplain.biome(), BiomeType::Wetland);

    let basin_edge = context([38_000; 9], [100; 9], 52_000).generate(32, 32).0;
    assert_eq!(basin_edge.biome(), BiomeType::Wetland);

    let steep = context(
        [
            38_000, 42_000, 46_000, 38_000, 42_000, 46_000, 38_000, 42_000, 46_000,
        ],
        [0; 9],
        52_000,
    )
    .generate(16, 16)
    .0;
    assert_ne!(steep.biome(), BiomeType::Wetland);

    let arid = context([38_000; 9], [0; 9], 8_000).generate(16, 16).0;
    assert_ne!(arid.biome(), BiomeType::Wetland);

    let mut no_river = context([38_000; 9], [0; 9], 52_000);
    no_river.river_len = 0;
    assert_ne!(no_river.generate(16, 16).0.biome(), BiomeType::Wetland);
}

#[test]
fn chunk_river_index_retains_every_intersecting_segment() {
    for (seed, region_x, region_y) in [(1, -1, -1), (7, 0, 0), (42, 1, -1)] {
        let map = RegionMap::build(seed, region_x, region_y);
        let mut coords = BTreeSet::new();
        for segment in &map.rivers {
            let influence = i64::from(segment.half_width) + FLOODPLAIN_RADIUS;
            let min_x = i64::from(segment.ax.min(segment.bx))
                .saturating_sub(influence)
                .div_euclid(CHUNK_SIZE);
            let max_x = i64::from(segment.ax.max(segment.bx))
                .saturating_add(influence)
                .div_euclid(CHUNK_SIZE);
            let min_y = i64::from(segment.ay.min(segment.by))
                .saturating_sub(influence)
                .div_euclid(CHUNK_SIZE);
            let max_y = i64::from(segment.ay.max(segment.by))
                .saturating_add(influence)
                .div_euclid(CHUNK_SIZE);
            for chunk_y in min_y..=max_y {
                for chunk_x in min_x..=max_x {
                    coords.insert((chunk_x, chunk_y));
                }
            }
        }

        for (chunk_x, chunk_y) in coords {
            let origin_x = chunk_x * CHUNK_SIZE;
            let origin_y = chunk_y * CHUNK_SIZE;
            if origin_x.div_euclid(REGION_SIZE) != region_x
                || origin_y.div_euclid(REGION_SIZE) != region_y
            {
                continue;
            }
            let expected: Vec<_> = map
                .rivers
                .iter()
                .copied()
                .filter(|segment| river_intersects_chunk(*segment, origin_x, origin_y))
                .collect();
            assert!(expected.len() <= MAX_CHUNK_RIVERS);
            let context = ChunkContext::new(seed, origin_x, origin_y);
            assert!(context.overflow_rivers.is_empty());
            assert_eq!(
                &context.rivers[..context.river_len],
                expected.as_slice(),
                "river index lost a segment in region {region_x},{region_y} chunk {chunk_x},{chunk_y}"
            );
        }
    }
}
