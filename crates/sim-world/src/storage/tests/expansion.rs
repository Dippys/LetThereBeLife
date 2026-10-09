//! Expansion generation budgets, chunk inspection, residency visits, and capacity tests.

use super::*;

#[test]
fn initial_area_rejects_unsafe_dimensions() {
    assert_eq!(WorldConfig::new(0, 10), Err(WorldConfigError::Empty));
    assert!(WorldConfig::new(4_096, 4_096).is_ok());
    assert!(matches!(
        WorldConfig::new(4_097, 4_096),
        Err(WorldConfigError::TooLarge { .. })
    ));
    assert!(matches!(
        WorldConfig::new(1, 16_777_216),
        Err(WorldConfigError::OutsideWorldBounds { .. })
    ));
    assert_eq!(MAX_INITIAL_CHUNKS, 262_144);
}

#[test]
fn generated_area_adds_deterministic_negative_world_space() {
    let bounds = WorldRect::from_inclusive_points(
        WorldPosition { x: -8, y: -6 },
        WorldPosition { x: -1, y: -1 },
    );
    let mut left = World::generate(33, WorldConfig::new(64, 64).unwrap());
    let mut right = World::generate(33, WorldConfig::new(64, 64).unwrap());
    left.generate_area(bounds).unwrap();
    right.generate_area(bounds).unwrap();
    assert_eq!(left, right);
    assert!(left.cell(WorldPosition { x: -4, y: -3 }).is_some());
}

#[test]
fn generated_area_accepts_large_selection_beyond_prior_cap() {
    let mut world = World::generate(1, WorldConfig::new(64, 64).unwrap());
    let bounds = WorldRect::from_inclusive_points(
        WorldPosition { x: 2_000, y: 2_000 },
        WorldPosition { x: 4_000, y: 4_000 },
    );
    world
        .generate_area(bounds)
        .expect("large selection now allowed");
    assert!(world.area_is_generated(bounds));
    assert!(world.cell(WorldPosition { x: 4_000, y: 4_000 }).is_some());
}

#[test]
fn generated_area_rejects_overflowing_selection() {
    let mut world = World::generate(1, WorldConfig::new(64, 64).unwrap());
    let bounds = WorldRect {
        min: WorldPosition {
            x: i64::MIN,
            y: i64::MIN,
        },
        max: WorldPosition {
            x: i64::MAX,
            y: i64::MAX,
        },
    };
    assert_eq!(
        world.generate_area(bounds),
        Err(GenerateAreaError::OutsideWorldBounds)
    );
}

#[test]
fn generated_area_rejects_more_than_large_chunk_budget() {
    let bounds = WorldRect {
        min: WorldPosition { x: 0, y: 0 },
        max: WorldPosition {
            x: 363 * CHUNK_SIZE,
            y: 363 * CHUNK_SIZE,
        },
    };
    assert!(matches!(
        World::generate_chunks_streaming(1, bounds),
        Err(GenerateAreaError::TooManyChunks { .. })
    ));
}

#[test]
fn generation_budget_accepts_limit_and_rejects_one_more() {
    let maximum = WorldRect {
        min: WorldPosition {
            x: -128 * CHUNK_SIZE,
            y: -128 * CHUNK_SIZE,
        },
        max: WorldPosition {
            x: 128 * CHUNK_SIZE,
            y: 128 * CHUNK_SIZE,
        },
    };
    assert!(World::generate_chunks_streaming(1, maximum).is_ok());

    let over = WorldRect {
        min: WorldPosition {
            x: -128 * CHUNK_SIZE,
            y: -128 * CHUNK_SIZE,
        },
        max: WorldPosition {
            x: 129 * CHUNK_SIZE,
            y: 128 * CHUNK_SIZE,
        },
    };
    assert_eq!(
        World::generate_chunks_streaming(1, over).map(|_| ()),
        Err(GenerateAreaError::TooManyChunks {
            requested: 257 * 256,
            maximum: MAX_CHUNKS_PER_GENERATION,
        })
    );
}

#[test]
fn generation_budget_counts_only_missing_chunks() {
    let world = World::generate(1, WorldConfig::new(64, 64).unwrap());
    let outside_initial = WorldRect {
        min: WorldPosition {
            x: CHUNK_SIZE,
            y: -128 * CHUNK_SIZE,
        },
        max: WorldPosition {
            x: 257 * CHUNK_SIZE,
            y: 128 * CHUNK_SIZE,
        },
    };
    assert_eq!(
        world.validate_generation_request(outside_initial),
        Ok(MAX_CHUNKS_PER_GENERATION)
    );
    let missing = world.missing_chunk_coords(outside_initial).unwrap();
    assert_eq!(missing.len(), MAX_CHUNKS_PER_GENERATION as usize);
    assert!(!missing.contains(&ChunkCoord { x: 0, y: 0 }));

    let requires_one_over = WorldRect {
        min: WorldPosition {
            x: CHUNK_SIZE,
            y: -128 * CHUNK_SIZE,
        },
        max: WorldPosition {
            x: 258 * CHUNK_SIZE,
            y: 128 * CHUNK_SIZE,
        },
    };
    let over_limit = GenerateAreaError::TooManyChunks {
        requested: MAX_CHUNKS_PER_GENERATION + 1,
        maximum: MAX_CHUNKS_PER_GENERATION,
    };
    assert_eq!(
        world.validate_generation_request(requires_one_over),
        Err(over_limit)
    );
    assert_eq!(
        world.missing_chunk_coords(requires_one_over),
        Err(over_limit)
    );
}

#[test]
fn missing_budget_counts_partial_initial_boundary_chunk() {
    let world = World::generate(1, WorldConfig::new(96, 64).unwrap());
    let bounds = WorldRect {
        min: WorldPosition { x: 64, y: 0 },
        max: WorldPosition { x: 128, y: 64 },
    };
    assert_eq!(world.validate_generation_request(bounds), Ok(1));
    assert_eq!(
        world.missing_chunk_coords(bounds).unwrap(),
        vec![ChunkCoord { x: 1, y: 0 }]
    );
}

#[test]
fn chunk_inspection_tracks_signed_boundaries_and_partial_initial_coverage() {
    let mut world = World::generate(1, WorldConfig::new(96, 64).unwrap());
    let cases = [
        (
            WorldPosition { x: -65, y: 0 },
            ChunkCoord { x: -2, y: 0 },
            ChunkLocalPosition { x: 63, y: 0 },
            ChunkPresence::Missing,
        ),
        (
            WorldPosition { x: -64, y: 0 },
            ChunkCoord { x: -1, y: 0 },
            ChunkLocalPosition { x: 0, y: 0 },
            ChunkPresence::PartialInitial,
        ),
        (
            WorldPosition { x: -1, y: 0 },
            ChunkCoord { x: -1, y: 0 },
            ChunkLocalPosition { x: 63, y: 0 },
            ChunkPresence::PartialInitial,
        ),
        (
            WorldPosition { x: 0, y: 0 },
            ChunkCoord { x: 0, y: 0 },
            ChunkLocalPosition { x: 0, y: 0 },
            ChunkPresence::PartialInitial,
        ),
        (
            WorldPosition { x: 47, y: 0 },
            ChunkCoord { x: 0, y: 0 },
            ChunkLocalPosition { x: 47, y: 0 },
            ChunkPresence::PartialInitial,
        ),
        (
            WorldPosition { x: 48, y: 0 },
            ChunkCoord { x: 0, y: 0 },
            ChunkLocalPosition { x: 48, y: 0 },
            ChunkPresence::PartialInitial,
        ),
        (
            WorldPosition { x: 64, y: 0 },
            ChunkCoord { x: 1, y: 0 },
            ChunkLocalPosition { x: 0, y: 0 },
            ChunkPresence::Missing,
        ),
    ];

    for (position, coord, local, presence) in cases {
        let inspection = world.inspect_chunk_at(position).unwrap();
        assert_eq!(inspection.coord, coord);
        assert_eq!(inspection.local, local);
        assert_eq!(inspection.presence, presence);
        assert!(inspection.bounds.contains(position));
    }

    world
        .generate_area(WorldRect {
            min: WorldPosition { x: 48, y: 0 },
            max: WorldPosition { x: 64, y: 32 },
        })
        .unwrap();
    assert_eq!(
        world
            .inspect_chunk_at(WorldPosition { x: 48, y: 0 })
            .unwrap()
            .presence,
        ChunkPresence::RetainedPartialInitial
    );
}

#[test]
fn chunk_inspection_rejects_an_unrepresentable_positive_edge() {
    let world = World::generate(1, WorldConfig::new(64, 64).unwrap());
    assert_eq!(
        world.inspect_chunk_at(WorldPosition { x: i64::MAX, y: 0 }),
        Err(GenerateAreaError::OutsideWorldBounds)
    );
}

#[test]
fn huge_world_aware_request_stops_at_missing_chunk_limit() {
    let world = World::generate(1, WorldConfig::new(64, 64).unwrap());
    let bounds = WorldRect {
        min: WorldPosition { x: 0, y: 0 },
        max: WorldPosition {
            x: 1_000_000_000,
            y: 1,
        },
    };
    assert_eq!(
        world.validate_generation_request(bounds),
        Err(GenerateAreaError::OutsideWorldBounds)
    );
}

#[test]
fn one_dimensional_extreme_range_is_rejected() {
    let bounds = WorldRect {
        min: WorldPosition { x: i64::MIN, y: 0 },
        max: WorldPosition { x: 0, y: 1 },
    };
    assert!(matches!(
        World::generate_chunks_streaming(1, bounds),
        Err(GenerateAreaError::OutsideWorldBounds)
    ));
}

#[test]
fn chunk_at_positive_coordinate_limit_is_rejected() {
    let bounds = WorldRect {
        min: WorldPosition {
            x: i64::MAX - 10,
            y: 0,
        },
        max: WorldPosition { x: i64::MAX, y: 1 },
    };
    assert_eq!(
        World::generate_chunks_streaming(1, bounds).map(|_| ()),
        Err(GenerateAreaError::OutsideWorldBounds)
    );
}

#[test]
fn chunk_outside_centered_world_is_rejected() {
    let coord = ChunkCoord {
        x: i64::MIN / CHUNK_SIZE,
        y: 0,
    };
    assert_eq!(
        World::generate_chunk_at(1, coord),
        Err(GenerateAreaError::OutsideWorldBounds)
    );
}

#[test]
fn generated_chunks_are_keyed_and_do_not_duplicate_initial_cells() {
    let mut world = World::generate(5, WorldConfig::new(64, 64).unwrap());
    let bounds = WorldRect::from_inclusive_points(
        WorldPosition { x: 60, y: 0 },
        WorldPosition { x: 70, y: 10 },
    );
    world.generate_area(bounds).unwrap();
    assert_eq!(world.revision(), 1);
    assert!(world.area_is_generated(bounds));
    assert_eq!(world.cells().count(), 11_264);
    assert!(world.cell(WorldPosition { x: 70, y: 10 }).is_some());
}

#[test]
fn generating_existing_chunks_does_not_advance_revision() {
    let mut world = World::generate(5, WorldConfig::new(64, 64).unwrap());
    let bounds = WorldRect::from_inclusive_points(
        WorldPosition { x: -10, y: -10 },
        WorldPosition { x: -1, y: -1 },
    );
    world.generate_area(bounds).unwrap();
    let revision = world.revision();
    world.generate_area(bounds).unwrap();
    assert_eq!(world.revision(), revision);
}

#[test]
fn missing_chunks_skip_initial_and_already_generated_areas() {
    let mut world = World::generate(5, WorldConfig::new(64, 64).unwrap());
    let first = WorldRect {
        min: WorldPosition { x: -64, y: 0 },
        max: WorldPosition { x: 128, y: 64 },
    };
    assert_eq!(
        world.missing_chunk_coords(first).unwrap(),
        vec![
            ChunkCoord { x: -1, y: 0 },
            ChunkCoord { x: 0, y: 0 },
            ChunkCoord { x: 1, y: 0 },
        ]
    );
    world.generate_area(first).unwrap();

    let extended = WorldRect {
        min: first.min,
        max: WorldPosition { x: 192, y: 64 },
    };
    assert_eq!(
        world.missing_chunk_coords(extended).unwrap(),
        vec![ChunkCoord { x: 2, y: 0 }]
    );
}

#[test]
fn stepped_cell_visit_samples_initial_and_generated_chunks() {
    let mut world = World::generate(5, WorldConfig::new(64, 64).unwrap());
    world
        .generate_area(WorldRect {
            min: WorldPosition { x: -64, y: 0 },
            max: WorldPosition { x: 0, y: 64 },
        })
        .unwrap();
    let mut positions = Vec::new();
    world.visit_cells_in_step(
        WorldRect {
            min: WorldPosition { x: -64, y: 0 },
            max: WorldPosition { x: 64, y: 64 },
        },
        4,
        |position, _| positions.push(position),
    );

    assert_eq!(positions.len(), 320);
    assert!(
        positions
            .iter()
            .all(|position| position.x.rem_euclid(4) == 0 && position.y.rem_euclid(4) == 0)
    );
}

#[test]
fn loaded_region_visit_reports_exact_clipped_coverage_in_chunk_order() {
    let mut world = World::generate(5, WorldConfig::new(96, 100).unwrap());
    world
        .generate_area(WorldRect {
            min: WorldPosition { x: 128, y: 0 },
            max: WorldPosition { x: 192, y: 64 },
        })
        .unwrap();
    let mut regions = Vec::new();
    world.visit_loaded_regions_in(
        WorldRect {
            min: WorldPosition { x: 0, y: -64 },
            max: WorldPosition { x: 192, y: 64 },
        },
        |coord, bounds| regions.push((coord, bounds)),
    );

    assert_eq!(
        regions,
        [
            (
                ChunkCoord { x: 0, y: -1 },
                WorldRect {
                    min: WorldPosition { x: 0, y: -50 },
                    max: WorldPosition { x: 48, y: 0 },
                },
            ),
            (
                ChunkCoord { x: 0, y: 0 },
                WorldRect {
                    min: WorldPosition { x: 0, y: 0 },
                    max: WorldPosition { x: 48, y: 50 },
                },
            ),
            (
                ChunkCoord { x: 2, y: 0 },
                WorldRect {
                    min: WorldPosition { x: 128, y: 0 },
                    max: WorldPosition { x: 192, y: 64 },
                },
            ),
        ]
    );
    let mut cell_count = 0;
    assert_eq!(
        world.visit_cells_in_chunk(ChunkCoord { x: 0, y: -1 }, |_, _| cell_count += 1),
        Some(regions[0].1)
    );
    assert_eq!(cell_count, 48 * 50);
    assert_eq!(
        world.visit_cells_in_chunk(ChunkCoord { x: 1, y: 0 }, |_, _| {}),
        None
    );
}

#[test]
fn generate_chunks_streaming_matches_batch_output() {
    let bounds = WorldRect::from_inclusive_points(
        WorldPosition { x: -64, y: -128 },
        WorldPosition { x: 200, y: -1 },
    );
    let batch = World::generate_chunks(11, bounds).expect("valid bounds");
    let streamed: Vec<_> = World::generate_chunks_streaming(11, bounds)
        .expect("valid bounds")
        .collect();
    assert_eq!(batch.len(), streamed.len());
    assert!(batch.iter().zip(streamed.iter()).all(|(a, b)| a == b));
}

#[test]
fn chunk_spans_visit_complete_region_groups_before_the_next_region() {
    let coords: Vec<_> = ChunkSpan {
        min: ChunkCoord { x: 0, y: 0 },
        max: ChunkCoord { x: 64, y: 1 },
        total: 130,
    }
    .coords()
    .collect();
    assert_eq!(coords[64], ChunkCoord { x: 0, y: 1 });
    assert_eq!(coords[128], ChunkCoord { x: 64, y: 0 });
}

#[test]
fn generated_chunk_store_enforces_total_capacity_without_allocating_the_limit() {
    let mut world = World::generate(1, WorldConfig::new(64, 64).unwrap());
    assert_eq!(world.ensure_chunk_capacity(MAX_GENERATED_CHUNKS), Ok(()));
    assert_eq!(
        world.ensure_chunk_capacity(MAX_GENERATED_CHUNKS + 1),
        Err(GenerateAreaError::WorldCapacity {
            requested: MAX_GENERATED_CHUNKS + 1,
            remaining: MAX_GENERATED_CHUNKS,
        })
    );

    assert_eq!(
        world.insert_chunks(vec![WorldChunk {
            coord: ChunkCoord { x: -1, y: 0 },
            terrain: Vec::new(),
            features: Vec::new(),
        }]),
        Ok(1)
    );
    assert_eq!(
        world.ensure_chunk_capacity(MAX_GENERATED_CHUNKS),
        Err(GenerateAreaError::WorldCapacity {
            requested: MAX_GENERATED_CHUNKS,
            remaining: MAX_GENERATED_CHUNKS - 1,
        })
    );
}
