use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;

use rayon::ThreadPoolBuilder;

use super::*;

#[test]
fn drainage_records_have_bounded_layouts() {
    assert_eq!(size_of::<DrainageSegment>(), 28);
    assert_eq!(size_of::<ChannelLink>(), 28);
    assert_eq!(size_of::<LakeDescriptor>(), 12);
    assert_eq!(size_of::<RiverSource>(), 8);
}

#[test]
fn strahler_order_increases_only_for_equal_order_confluences() {
    // 0 and 1 join at 2, then the order-2 branch meets one order-1 branch
    // at 4. The unequal confluence must remain order 2.
    let target = [2, 2, 4, 4, 5, NO_TARGET];
    let selected = [true, true, true, true, true, false];
    let order = [0, 1, 2, 3, 4, 5];
    let stream_orders = derive_stream_orders(&target, &selected, &order);
    assert_eq!(stream_orders, [1, 1, 2, 1, 2, 0]);
}

#[test]
fn arid_cells_do_not_create_perennial_runoff() {
    let elevation = [
        40_009, 40_008, 40_007, 40_006, 40_005, 40_004, 40_003, 40_002, 40_001,
    ];
    let moisture = [RUNOFF_MOISTURE_FLOOR; 9];
    let (_, accumulation) = route_flow(&elevation, &elevation, &moisture, 3, SKELETON_STEP);
    assert_eq!(accumulation, [0; 9]);

    let wet = [RUNOFF_MOISTURE_FLOOR + 9_600; 9];
    let (_, accumulation) = route_flow(&elevation, &elevation, &wet, 3, SKELETON_STEP);
    assert!(accumulation.into_iter().any(|flow| flow > 0));
}

#[test]
fn segment_intersection_includes_overlap_and_endpoint_touches() {
    let segment = |ax, ay, bx, by| DrainageSegment {
        ax,
        ay,
        bx,
        by,
        channel_id: 1,
        surface_a: 40_000,
        surface_b: 39_000,
        half_width: 1,
        stream_order: 1,
    };
    let horizontal = segment(0, 0, 10, 0);
    assert!(segments_intersect(horizontal, segment(5, 0, 15, 0)));
    assert!(segments_intersect(horizontal, segment(10, 0, 10, 5)));
    assert!(segments_intersect(horizontal, segment(5, -5, 5, 5)));
    assert!(!segments_intersect(horizontal, segment(11, 0, 15, 0)));
}

#[test]
fn retained_channels_do_not_cross_outside_shared_endpoints() {
    for seed in [1, 7, 42, 10_001] {
        let drainage = DrainageSkeleton::build(seed, SKELETON_STEP);
        let subdivisions = (drainage.step / NODE_STEP) as usize;
        let mut node_channels = BTreeMap::<u32, Vec<(u32, (i64, i64))>>::new();
        for link in &drainage.channels {
            let start = node_position(link.from as usize, drainage.grid, drainage.step);
            let end = node_position(link.to as usize, drainage.grid, drainage.step);
            node_channels.entry(link.from).or_default().push((
                link.channel_id,
                channel_point(seed, start, end, 0, subdivisions),
            ));
            node_channels.entry(link.to).or_default().push((
                link.channel_id,
                channel_point(seed, start, end, subdivisions, subdivisions),
            ));
        }
        let mut canonical_joins = BTreeSet::new();
        for node_channels in node_channels.into_values() {
            for (offset, &(left, point)) in node_channels.iter().enumerate() {
                for &(right, other_point) in &node_channels[offset + 1..] {
                    assert_eq!(point, other_point, "shared nodes must refine identically");
                    canonical_joins.insert((
                        left.min(right),
                        left.max(right),
                        point.0 as i32,
                        point.1 as i32,
                    ));
                }
            }
        }
        let mut bins = BTreeMap::<(i64, i64), Vec<usize>>::new();
        for (index, segment) in drainage.rivers.iter().enumerate() {
            let min_x = i64::from(segment.ax.min(segment.bx)).div_euclid(64);
            let max_x = i64::from(segment.ax.max(segment.bx)).div_euclid(64);
            let min_y = i64::from(segment.ay.min(segment.by)).div_euclid(64);
            let max_y = i64::from(segment.ay.max(segment.by)).div_euclid(64);
            for y in min_y..=max_y {
                for x in min_x..=max_x {
                    bins.entry((x, y)).or_default().push(index);
                }
            }
        }
        let mut compared = BTreeSet::new();
        for indices in bins.values() {
            for (offset, &left) in indices.iter().enumerate() {
                for &right in &indices[offset + 1..] {
                    let pair = (left.min(right), left.max(right));
                    if !compared.insert(pair) {
                        continue;
                    }
                    let left = drainage.rivers[pair.0];
                    let right = drainage.rivers[pair.1];
                    if !segments_intersect(left, right) {
                        continue;
                    }
                    let (shared_count, shared) = shared_endpoint(left, right);
                    let channel_pair = (
                        left.channel_id.min(right.channel_id),
                        left.channel_id.max(right.channel_id),
                        shared.0,
                        shared.1,
                    );
                    let allowed_join = shared_count == 1
                        && ((left.channel_id == right.channel_id && pair.1 == pair.0 + 1)
                            || canonical_joins.contains(&channel_pair));
                    if !allowed_join {
                        panic!(
                            "seed {seed} segments intersect outside their routed polyline: {left:?} and {right:?}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn skeleton_parallelism_preserves_exact_output() {
    let build = |workers| {
        ThreadPoolBuilder::new()
            .num_threads(workers)
            .build()
            .unwrap()
            .install(|| DrainageSkeleton::build(42, SKELETON_STEP))
    };
    let single = build(1);
    let parallel = build(4);
    assert_eq!(parallel.lake_depth, single.lake_depth);
    assert_eq!(parallel.fill_surface, single.fill_surface);
    assert_eq!(parallel.rivers, single.rivers);
    assert_eq!(parallel.channels, single.channels);
    assert_eq!(parallel.lakes, single.lakes);
}

#[test]
fn channels_continue_to_canonical_outlets_across_signed_region_seams() {
    let mut seam_kinds = [false; 4];
    let mut checked = 0;
    let mut observed_max = 0;
    let mut source_count = 0;
    for seed in [1, 7, 42, 10_001] {
        let drainage = DrainageSkeleton::build(seed, SKELETON_STEP);
        source_count += drainage.sources.len();
        assert!(!drainage.sources.is_empty());
        assert!(drainage.sources.len() <= MAX_RIVER_SOURCES);
        assert!(drainage.channels.len() < 600);
        for (offset, source) in drainage.sources.iter().enumerate() {
            assert!(drainage.channels.iter().any(|link| {
                link.from == source.node && u64::from(link.flow) >= RIVER_SOURCE_FLOW_THRESHOLD
            }));
            assert!(drainage.lakes.iter().any(|lake| {
                lake.id == source.lake_id && lake.outlet_node == source.node && !lake.terminal
            }));
            let point = node_position(source.node as usize, drainage.grid, drainage.step);
            for other in &drainage.sources[offset + 1..] {
                let other = node_position(other.node as usize, drainage.grid, drainage.step);
                let dx = point.0 - other.0;
                let dy = point.1 - other.1;
                assert!(
                    dx * dx + dy * dy >= RIVER_SOURCE_SPACING * RIVER_SOURCE_SPACING,
                    "river sources must remain spatially distinct"
                );
            }
        }
        assert!(
            drainage
                .rivers
                .iter()
                .all(|segment| segment.surface_b <= segment.surface_a)
        );
        let mut segments_per_chunk = BTreeMap::<(i64, i64), usize>::new();
        for segment in &drainage.rivers {
            let width = i64::from(segment.half_width) + FLOODPLAIN_RADIUS;
            let min_x = (i64::from(segment.ax.min(segment.bx)) - width)
                .div_euclid(crate::world::CHUNK_SIZE);
            let max_x = (i64::from(segment.ax.max(segment.bx)) + width)
                .div_euclid(crate::world::CHUNK_SIZE);
            let min_y = (i64::from(segment.ay.min(segment.by)) - width)
                .div_euclid(crate::world::CHUNK_SIZE);
            let max_y = (i64::from(segment.ay.max(segment.by)) + width)
                .div_euclid(crate::world::CHUNK_SIZE);
            for chunk_y in min_y..=max_y {
                for chunk_x in min_x..=max_x {
                    if (-512..512).contains(&chunk_x) && (-512..512).contains(&chunk_y) {
                        *segments_per_chunk.entry((chunk_x, chunk_y)).or_default() += 1;
                    }
                }
            }
        }
        let maximum = segments_per_chunk.values().copied().max().unwrap_or(0);
        observed_max = observed_max.max(maximum);
        assert!(
            maximum <= crate::worldgen::MAX_CHUNK_RIVERS,
            "seed {seed} needs {maximum} chunk river slots"
        );
        for link in &drainage.channels {
            checked += 1;
            assert_ne!(link.stream_order, 0);
            match link.outlet {
                ChannelOutlet::Continues => {
                    let downstream = drainage
                        .channels
                        .iter()
                        .find(|candidate| candidate.from == link.to)
                        .expect("declared continuation must exist");
                    assert_eq!(downstream.channel_id, link.channel_id);
                    assert_eq!(downstream.basin_id, link.basin_id);
                    assert!(downstream.stream_order >= link.stream_order);
                }
                ChannelOutlet::Confluence => assert!(
                    drainage.channels.iter().any(|candidate| {
                        candidate.from == link.to && candidate.basin_id == link.basin_id
                    }),
                    "tributary {} does not reach its canonical basin",
                    link.channel_id
                ),
                ChannelOutlet::Lake => {
                    assert_ne!(link.water_body_id, 0);
                    assert!(
                        drainage
                            .lakes
                            .iter()
                            .any(|lake| lake.id == link.water_body_id)
                    );
                }
                ChannelOutlet::Ocean | ChannelOutlet::WorldEdge => {}
                ChannelOutlet::TerminalBasin => {
                    assert!(is_border(link.to as usize, drainage.grid));
                }
            }
            let (from_x, from_y) = node_position(link.from as usize, drainage.grid, drainage.step);
            let (to_x, to_y) = node_position(link.to as usize, drainage.grid, drainage.step);
            if from_x.div_euclid(REGION_SIZE) != to_x.div_euclid(REGION_SIZE) {
                seam_kinds[usize::from(from_x.max(to_x) > 0)] = true;
            }
            if from_y.div_euclid(REGION_SIZE) != to_y.div_euclid(REGION_SIZE) {
                seam_kinds[2 + usize::from(from_y.max(to_y) > 0)] = true;
            }
        }
        assert!(drainage.lakes.iter().all(|lake| {
            lake.terminal || lake.outlet_node < (drainage.grid * drainage.grid) as u32
        }));
    }
    assert!(checked > 100, "too few canonical channel links: {checked}");
    assert!(source_count >= 16, "too few canonical lake-fed sources");
    assert!(
        observed_max <= crate::worldgen::MAX_CHUNK_RIVERS,
        "representative streams must fit the allocation-free chunk fast path"
    );
    assert!(
        seam_kinds.into_iter().all(|covered| covered),
        "major channels must cross positive and negative x/y region seams"
    );
}

#[test]
#[ignore = "release-only topology and footprint comparison"]
fn compare_candidate_skeleton_steps() {
    let seed = std::env::var("SIM_DRAINAGE_SEED").ok().map_or(1, |value| {
        value
            .parse::<u64>()
            .expect("SIM_DRAINAGE_SEED must be an integer")
    });
    let requested = std::env::var("SIM_DRAINAGE_STEP").ok().map(|value| {
        value
            .parse::<i64>()
            .expect("SIM_DRAINAGE_STEP must be an integer")
    });
    let stream_flow_threshold = std::env::var("SIM_RIVER_SOURCE_FLOW_THRESHOLD")
        .ok()
        .map_or(RIVER_SOURCE_FLOW_THRESHOLD, |value| {
            value
                .parse::<u64>()
                .expect("SIM_RIVER_SOURCE_FLOW_THRESHOLD must be a positive integer")
        });
    let steps: &[i64] = requested.as_ref().map_or(&[128, 256], std::slice::from_ref);
    for &step in steps {
        let started = Instant::now();
        let drainage = DrainageSkeleton::build_with_threshold(seed, step, stream_flow_threshold);
        println!(
            "seed={seed} step={step} source_flow_threshold={stream_flow_threshold} grid={} sources={} channels={} segments={} lakes={} retained_bytes={} scratch_upper_bound_bytes={} elapsed_ms={:.1}",
            drainage.grid,
            drainage.sources.len(),
            drainage.channels.len(),
            drainage.rivers.len(),
            drainage.lakes.len(),
            drainage.retained_bytes(),
            drainage.scratch_upper_bound_bytes(),
            started.elapsed().as_secs_f64() * 1_000.0
        );
        for source in &drainage.sources {
            let (x, y) = node_position(source.node as usize, drainage.grid, drainage.step);
            println!(
                "source lake={} node={} x={x} y={y}",
                source.lake_id, source.node
            );
        }
    }
}
