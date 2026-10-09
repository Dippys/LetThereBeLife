//! Bootstrap-area materialization, clipping, streaming, and payload validation tests.

use super::*;

#[test]
fn centered_world_envelope_matches_the_raw_terrain_budget() {
    assert_eq!(
        WORLD_GENERATION_BOUNDS.min,
        WorldPosition {
            x: -32_768,
            y: -32_768
        }
    );
    assert_eq!(
        WORLD_GENERATION_BOUNDS.max,
        WorldPosition {
            x: 32_768,
            y: 32_768
        }
    );
    assert_eq!(MAX_GENERATED_CHUNKS, 1_048_576);
    assert_eq!(MAX_GENERATED_CELLS, 4_294_967_296);
    assert_eq!(MAX_GENERATED_TERRAIN_BYTES, 16 * 1_024 * 1_024 * 1_024);

    assert!(World::generate_chunk_at(1, ChunkCoord { x: -512, y: -512 }).is_ok());
    assert!(World::generate_chunk_at(1, ChunkCoord { x: 511, y: 511 }).is_ok());
    assert_eq!(
        World::generate_chunk_at(1, ChunkCoord { x: 512, y: 0 }),
        Err(GenerateAreaError::OutsideWorldBounds)
    );
}

#[test]
fn initial_area_matches_independently_generated_chunks() {
    let seed = 19;
    let world = World::generate(seed, WorldConfig::new(128, 128).unwrap());
    for coord in [
        ChunkCoord { x: -1, y: -1 },
        ChunkCoord { x: -1, y: 0 },
        ChunkCoord { x: 0, y: -1 },
        ChunkCoord { x: 0, y: 0 },
    ] {
        let chunk = World::generate_chunk_at(seed, coord).unwrap();
        let origin = chunk_origin(coord);
        for (index, cell) in chunk.terrain().iter().enumerate() {
            let position = WorldPosition {
                x: origin.x + index as i64 % CHUNK_SIZE,
                y: origin.y + index as i64 / CHUNK_SIZE,
            };
            assert_eq!(world.cell(position), Some(*cell));
        }
        let expected_features: Vec<_> = world
            .all_features()
            .filter(|feature| chunk_coord(feature.position) == coord)
            .copied()
            .collect();
        assert_eq!(chunk.features(), expected_features);
    }
}

#[test]
fn region_major_initial_generation_matches_chunks_and_keeps_feature_order() {
    let seed = 23;
    let width = (REGION_SIZE + CHUNK_SIZE) as u32;
    let world = World::generate(seed, WorldConfig::new(width, 128).unwrap());
    for coord in [ChunkCoord { x: -1, y: -1 }, ChunkCoord { x: 0, y: 0 }] {
        let chunk = World::generate_chunk_at(seed, coord).expect("coordinate is valid");
        let origin = chunk_origin(coord);
        for (index, cell) in chunk.terrain().iter().enumerate() {
            let position = WorldPosition {
                x: origin.x + index as i64 % CHUNK_SIZE,
                y: origin.y + index as i64 / CHUNK_SIZE,
            };
            assert_eq!(world.cell(position), Some(*cell));
        }
    }
    assert!(
        world
            .all_features()
            .all(|feature| world.feature_at(feature.position) == Some(*feature))
    );
}

#[test]
fn chunk_generator_validates_inputs_and_matches_chunk_output() {
    let seed = 19;
    let coord = ChunkCoord { x: -1, y: 64 };
    let sampler = ChunkGenerator::new(seed, coord).expect("coordinate is representable");
    let chunk = World::generate_chunk_at(seed, coord).expect("coordinate is representable");
    for local in [
        ChunkLocalPosition { x: 0, y: 0 },
        ChunkLocalPosition { x: 31, y: 48 },
        ChunkLocalPosition { x: 63, y: 63 },
    ] {
        let sampled = sampler.sample(local).expect("local coordinate is valid");
        let index = usize::from(local.y) * CHUNK_SIZE as usize + usize::from(local.x);
        assert_eq!(sampled.terrain, chunk.terrain()[index]);
        let origin = chunk_origin(coord);
        let position = WorldPosition {
            x: origin.x + i64::from(local.x),
            y: origin.y + i64::from(local.y),
        };
        assert_eq!(
            sampled.feature,
            chunk
                .features()
                .iter()
                .find(|feature| feature.position == position)
                .map(|feature| feature.kind)
        );
    }
    assert!(sampler.sample(ChunkLocalPosition { x: 64, y: 0 }).is_none());
    assert!(matches!(
        ChunkGenerator::new(seed, ChunkCoord { x: i64::MAX, y: 0 }),
        Err(GenerateAreaError::TooLarge)
    ));
}

#[test]
fn cell_rejects_out_of_bounds_positions() {
    let world = World::generate_square(1, 16);
    assert!(world.cell(WorldPosition { x: 7, y: 7 }).is_some());
    assert!(world.cell(WorldPosition { x: 8, y: 0 }).is_none());
}

#[test]
fn sparse_features_are_sorted_and_addressable() {
    let world = World::generate_square(7, 256);
    let feature = world
        .all_features()
        .next()
        .copied()
        .expect("generated world has a sparse feature");
    assert_eq!(world.feature_at(feature.position), Some(feature));
    assert_eq!(
        world.feature_at(WorldPosition {
            x: i64::from(world.width()),
            y: 0
        }),
        None
    );
}

#[test]
fn configured_initial_area_can_be_rectangular() {
    let config = WorldConfig::new(96, 64).unwrap();
    let world = World::generate(3, config);
    assert_eq!((world.width(), world.height()), (96, 64));
    assert_eq!(world.cells().count(), 96 * 64);
}

#[test]
fn resident_iterators_use_documented_tile_then_local_order() {
    let world = World::generate(3, WorldConfig::new(128, 128).unwrap());
    let positions: Vec<_> = world.cells().map(|(position, _)| position).collect();

    assert_eq!(positions[0], WorldPosition { x: -64, y: -64 });
    assert_eq!(positions[1], WorldPosition { x: -63, y: -64 });
    assert_eq!(positions[64], WorldPosition { x: -64, y: -63 });
    assert_eq!(positions[4_095], WorldPosition { x: -1, y: -1 });
    assert_eq!(positions[4_096], WorldPosition { x: -64, y: 0 });
    assert_eq!(positions[8_192], WorldPosition { x: 0, y: -64 });

    let features: Vec<_> = world.all_features().copied().collect();
    assert!(features.windows(2).all(|pair| {
        let left_chunk = ChunkCoord::from_world_position(pair[0].position);
        let right_chunk = ChunkCoord::from_world_position(pair[1].position);
        left_chunk < right_chunk
            || (left_chunk == right_chunk
                && (pair[0].position.y < pair[1].position.y
                    || (pair[0].position.y == pair[1].position.y
                        && pair[0].position.x <= pair[1].position.x)))
    }));
}

#[test]
fn deferred_world_declares_bootstrap_without_claiming_loaded_cells() {
    let world = World::new(3, WorldConfig::new(96, 100).unwrap());
    let bounds = world.initial_bounds();

    assert_eq!(world.loaded_chunk_count(), 0);
    assert_eq!(world.cells().count(), 0);
    assert_eq!(world.all_features().count(), 0);
    assert!(!world.area_is_generated(bounds));
    assert_eq!(world.cell(WorldPosition { x: 0, y: 0 }), None);
    assert_eq!(world.loaded_bounds_at(WorldPosition { x: 0, y: 0 }), None);
    assert_eq!(
        world
            .inspect_chunk_at(WorldPosition { x: 0, y: 0 })
            .unwrap()
            .presence,
        ChunkPresence::PartialInitialUnloaded
    );
    assert_eq!(
        world
            .inspect_chunk_at(WorldPosition { x: 47, y: 49 })
            .unwrap()
            .presence,
        ChunkPresence::PartialInitialUnloaded
    );
    assert_eq!(world.missing_chunk_load_requests(bounds).unwrap().len(), 4);
}

#[test]
fn streamed_bootstrap_matches_eager_content_for_aligned_and_clipped_worlds() {
    for config in [
        WorldConfig::new(128, 128).unwrap(),
        WorldConfig::new(96, 100).unwrap(),
    ] {
        let eager = World::generate(41, config);
        let mut streamed = World::new(41, config);
        let loads = streamed
            .missing_chunk_load_requests(streamed.initial_bounds())
            .unwrap()
            .into_iter()
            .rev()
            .map(|request| World::generate_chunk_load(41, request))
            .collect();

        assert_eq!(
            streamed.insert_chunk_loads(loads),
            Ok(eager.loaded_chunk_count())
        );
        assert_eq!(
            streamed.cells().collect::<Vec<_>>(),
            eager.cells().collect::<Vec<_>>()
        );
        assert_eq!(
            streamed.all_features().copied().collect::<Vec<_>>(),
            eager.all_features().copied().collect::<Vec<_>>()
        );
        assert!(streamed.area_is_generated(streamed.initial_bounds()));
    }
}

#[test]
fn clipped_bootstrap_generation_matches_the_corresponding_full_chunk_subset() {
    let seed = 41;
    let mut world = World::new(seed, WorldConfig::new(96, 64).unwrap());
    let request = world
        .missing_chunk_load_requests(world.initial_bounds())
        .unwrap()
        .into_iter()
        .find(|request| request.coord() == ChunkCoord { x: 0, y: 0 })
        .unwrap();
    let full_chunk = World::generate_chunk_at(seed, request.coord()).unwrap();
    let full_bounds = request.coord().bounds().unwrap();

    world
        .insert_chunk_loads(vec![World::generate_chunk_load(seed, request)])
        .unwrap();

    for y in request.bounds().min.y..request.bounds().max.y {
        for x in request.bounds().min.x..request.bounds().max.x {
            let index = ((y - full_bounds.min.y) * CHUNK_SIZE + x - full_bounds.min.x) as usize;
            assert_eq!(
                world.cell(WorldPosition { x, y }),
                Some(full_chunk.terrain()[index])
            );
        }
    }
    let expected_features: Vec<_> = full_chunk
        .features()
        .iter()
        .filter(|feature| request.bounds().contains(feature.position))
        .copied()
        .collect();
    assert_eq!(
        world.all_features().copied().collect::<Vec<_>>(),
        expected_features
    );
}

#[test]
fn eager_bootstrap_materialization_batches_without_changing_content() {
    let config = WorldConfig::new(128, 64).unwrap();
    let expected = World::generate(41, config);
    let mut world = World::new(41, config);

    world.materialize_initial_area_in_batches(1).unwrap();

    assert_eq!(world.loaded_chunk_count(), 4);
    assert_eq!(world.revision(), 4);
    assert_eq!(
        world.cells().collect::<Vec<_>>(),
        expected.cells().collect::<Vec<_>>()
    );
    assert_eq!(
        world.all_features().copied().collect::<Vec<_>>(),
        expected.all_features().copied().collect::<Vec<_>>()
    );
}

#[test]
fn clipped_bootstrap_tile_hides_its_fringe_until_promoted_to_an_expansion() {
    let mut world = World::new(7, WorldConfig::new(96, 64).unwrap());
    let bootstrap = world
        .missing_chunk_load_requests(world.initial_bounds())
        .unwrap()
        .into_iter()
        .map(|request| World::generate_chunk_load(7, request))
        .collect();
    world.insert_chunk_loads(bootstrap).unwrap();

    let bootstrap_bounds = WorldRect {
        min: WorldPosition { x: 0, y: 0 },
        max: WorldPosition { x: 48, y: 32 },
    };
    assert!(world.cell(WorldPosition { x: 47, y: 31 }).is_some());
    assert_eq!(world.cell(WorldPosition { x: 48, y: 0 }), None);
    assert_eq!(
        world.loaded_bounds_at(WorldPosition { x: 47, y: 0 }),
        Some(bootstrap_bounds)
    );
    assert_eq!(world.loaded_bounds_at(WorldPosition { x: 48, y: 0 }), None);
    assert_eq!(world.generated_chunk_count(), 0);

    let expansion_bounds = WorldRect {
        min: WorldPosition { x: 48, y: 0 },
        max: WorldPosition { x: 64, y: 32 },
    };
    let expansion = world
        .missing_chunk_load_requests(expansion_bounds)
        .unwrap()
        .into_iter()
        .map(|request| World::generate_chunk_load(7, request))
        .collect();
    assert_eq!(world.insert_chunk_loads(expansion), Ok(1));

    assert!(world.cell(WorldPosition { x: 48, y: 0 }).is_some());
    assert_eq!(
        world.loaded_bounds_at(WorldPosition { x: 48, y: 0 }),
        Some(ChunkCoord { x: 0, y: 0 }.bounds().unwrap())
    );
    assert_eq!(world.generated_chunk_count(), 1);
    assert_eq!(
        world
            .inspect_chunk_at(WorldPosition { x: 48, y: 0 })
            .unwrap()
            .presence,
        ChunkPresence::RetainedPartialInitial
    );
}

#[test]
fn opaque_loads_reject_wrong_seed_or_incompatible_bootstrap_coverage() {
    let config = WorldConfig::new(96, 64).unwrap();
    let mut world = World::new(7, config);
    let request = world
        .missing_chunk_load_requests(world.initial_bounds())
        .unwrap()
        .into_iter()
        .next()
        .unwrap();
    let wrong_seed = World::generate_chunk_load(8, request);

    assert_eq!(
        world.insert_chunk_loads(vec![wrong_seed]),
        Err(GenerateAreaError::SeedMismatch {
            expected: 7,
            received: 8,
        })
    );
    assert_eq!(world.loaded_chunk_count(), 0);
    assert_eq!(world.revision(), 0);

    let source = World::new(7, WorldConfig::new(128, 64).unwrap());
    let incompatible_request = source
        .missing_chunk_load_requests(source.initial_bounds())
        .unwrap()
        .into_iter()
        .find(|request| request.coord() == ChunkCoord { x: -1, y: 0 })
        .unwrap();
    let incompatible = World::generate_chunk_load(7, incompatible_request);

    assert_eq!(
        world.insert_chunk_loads(vec![incompatible]),
        Err(GenerateAreaError::InvalidChunkLoad)
    );
    assert_eq!(world.loaded_chunk_count(), 0);
    assert_eq!(world.revision(), 0);
}

#[test]
fn overlapping_initial_areas_generate_identical_cells() {
    let small = World::generate(11, WorldConfig::new(96, 64).unwrap());
    let large = World::generate(11, WorldConfig::new(128, 96).unwrap());
    for position in [
        WorldPosition { x: 0, y: 0 },
        WorldPosition { x: 31, y: 31 },
        WorldPosition { x: 47, y: 31 },
    ] {
        assert_eq!(small.cell(position), large.cell(position));
        assert_eq!(small.feature_at(position), large.feature_at(position));
    }
}
