use super::*;

#[test]
fn route_heuristic_floor_is_admissible_for_every_passable_surface() {
    for surface in [
        SurfaceType::Sand,
        SurfaceType::Soil,
        SurfaceType::Hill,
        SurfaceType::Rock,
        SurfaceType::SnowIce,
    ] {
        assert!(surface_traversal_cost(surface) >= MIN_TRAVERSAL_COST);
    }
}

#[test]
fn generation_is_deterministic() {
    assert_eq!(
        World::generate_square(42, 128),
        World::generate_square(42, 128)
    );
}

#[test]
fn seed_changes_generated_terrain() {
    let left = World::generate_square(1, 128);
    let right = World::generate_square(2, 128);
    assert_ne!(
        left.cells().collect::<Vec<_>>(),
        right.cells().collect::<Vec<_>>()
    );
}

#[test]
fn terrain_has_variation_and_sparse_features() {
    let world = World::generate_square(7, 256);
    let terrain: Vec<_> = world.cells().map(|(_, cell)| cell).collect();
    let features: Vec<_> = world.all_features().copied().collect();
    let first = terrain[0].classification();
    assert!(terrain.iter().any(|cell| cell.classification() != first));
    assert!(!features.is_empty());
    assert!(features.len() < terrain.len() / 4);
    assert!(features.iter().all(|feature| !matches!(
        world.cell(feature.position).unwrap().surface(),
        SurfaceType::DeepWater
            | SurfaceType::ShallowWater
            | SurfaceType::Sand
            | SurfaceType::SnowIce
    )));
    let first_feature = features[0];
    assert_eq!(
        world.base_resource_at(first_feature.position),
        Some(first_feature.base_resource())
    );
}

#[test]
fn terrain_records_keep_their_compact_layout() {
    assert_eq!(std::mem::size_of::<SurfaceType>(), 1);
    assert_eq!(std::mem::size_of::<BiomeType>(), 1);
    assert_eq!(std::mem::size_of::<TerrainClass>(), 1);
    assert_eq!(std::mem::size_of::<TerrainCell>(), 4);
    assert_eq!(std::mem::size_of::<ClimateSample>(), 4);
    assert_eq!(std::mem::size_of::<FeatureKind>(), 1);
    assert_eq!(std::mem::size_of::<ResourceKind>(), 1);
    assert_eq!(std::mem::size_of::<BaseResource>(), 4);
    assert_eq!(std::mem::size_of::<WaterSource>(), 1);
    assert_eq!(std::mem::size_of::<TraversalKind>(), 1);
    assert_eq!(std::mem::size_of::<Standability>(), 1);
    assert_eq!(std::mem::size_of::<TraversalStep>(), 8);
    assert_eq!(std::mem::size_of::<Feature>(), 24);
}

#[test]
fn physical_world_queries_are_explicit_and_derived_from_resident_base_data() {
    let mut world = World::new(3, WorldConfig::new(128, 64).unwrap());
    let coord = ChunkCoord { x: 0, y: 0 };
    let mut terrain = vec![
        TerrainCell::new(10_000, 80, SurfaceType::Soil, BiomeType::Grassland);
        (CHUNK_SIZE * CHUNK_SIZE) as usize
    ];
    let index = |x: usize, y: usize| y * CHUNK_SIZE as usize + x;
    terrain[index(1, 0)] = TerrainCell::new(10_000, 80, SurfaceType::ShallowWater, BiomeType::Lake);
    terrain[index(2, 0)] = TerrainCell::new(10_000, 80, SurfaceType::DeepWater, BiomeType::River);
    terrain[index(3, 0)] =
        TerrainCell::new(10_000, 80, SurfaceType::ShallowWater, BiomeType::Ocean);
    terrain[index(0, 1)] = TerrainCell::new(
        10_000 + MAX_TRAVERSABLE_ELEVATION_DELTA,
        80,
        SurfaceType::Hill,
        BiomeType::Alpine,
    );
    terrain[index(0, 2)] = TerrainCell::new(
        10_001 + 2 * MAX_TRAVERSABLE_ELEVATION_DELTA,
        80,
        SurfaceType::Hill,
        BiomeType::Alpine,
    );
    terrain[index(1, 1)] = terrain[index(0, 1)];
    terrain[index(2, 1)] = terrain[index(0, 1)];
    terrain[index(3, 1)] = terrain[index(0, 1)];
    let features = vec![
        Feature {
            position: WorldPosition { x: 1, y: 1 },
            kind: FeatureKind::Tree,
        },
        Feature {
            position: WorldPosition { x: 2, y: 1 },
            kind: FeatureKind::BerryBush,
        },
        Feature {
            position: WorldPosition { x: 3, y: 1 },
            kind: FeatureKind::Rock,
        },
    ];
    world
        .insert_chunks(vec![WorldChunk {
            coord,
            terrain,
            features,
        }])
        .unwrap();

    assert_eq!(world.water_at(WorldPosition { x: 0, y: 0 }), Ok(None));
    assert_eq!(
        world.standability_at(WorldPosition { x: 0, y: 0 }),
        Ok(Standability::Standable)
    );
    for (x, source, drinkable) in [
        (1, WaterSource::Lake, true),
        (2, WaterSource::River, true),
        (3, WaterSource::Ocean, false),
    ] {
        assert_eq!(world.water_at(WorldPosition { x, y: 0 }), Ok(Some(source)));
        assert_eq!(source.is_drinkable(), drinkable);
    }
    assert_eq!(
        world.standability_at(WorldPosition { x: 1, y: 0 }),
        Ok(Standability::BlockedByWater)
    );
    assert_eq!(
        world.standability_at(WorldPosition { x: 1, y: 1 }),
        Ok(Standability::BlockedByFeature)
    );
    assert_eq!(
        world.standability_at(WorldPosition { x: 2, y: 1 }),
        Ok(Standability::Standable)
    );

    let slope_limit = world
        .traversal_step(WorldPosition { x: 0, y: 0 }, WorldPosition { x: 0, y: 1 })
        .unwrap();
    assert_eq!(slope_limit.kind(), TraversalKind::Passable);
    assert_eq!(
        slope_limit.elevation_delta(),
        i32::from(MAX_TRAVERSABLE_ELEVATION_DELTA)
    );
    assert!(slope_limit.cost().is_some());
    assert_eq!(
        world
            .traversal_step(WorldPosition { x: 0, y: 1 }, WorldPosition { x: 0, y: 2 })
            .unwrap()
            .kind(),
        TraversalKind::BlockedBySlope
    );
    assert_eq!(
        world
            .traversal_step(WorldPosition { x: 0, y: 0 }, WorldPosition { x: 1, y: 0 })
            .unwrap()
            .kind(),
        TraversalKind::BlockedByWater
    );
    assert_eq!(
        world
            .traversal_step(WorldPosition { x: 2, y: 1 }, WorldPosition { x: 2, y: 0 })
            .unwrap()
            .kind(),
        TraversalKind::BlockedByWater
    );
    assert_eq!(
        world
            .traversal_step(WorldPosition { x: 3, y: 1 }, WorldPosition { x: 3, y: 0 })
            .unwrap()
            .kind(),
        TraversalKind::BlockedByWater
    );
    assert_eq!(
        world
            .traversal_step(WorldPosition { x: 0, y: 1 }, WorldPosition { x: 1, y: 1 })
            .unwrap()
            .kind(),
        TraversalKind::BlockedByFeature
    );
    assert!(
        world
            .traversal_step(WorldPosition { x: 1, y: 1 }, WorldPosition { x: 2, y: 1 })
            .unwrap()
            .is_passable()
    );
    assert_eq!(
        world
            .traversal_step(WorldPosition { x: 2, y: 1 }, WorldPosition { x: 3, y: 1 })
            .unwrap()
            .kind(),
        TraversalKind::BlockedByFeature
    );

    assert_eq!(world.resource_at(WorldPosition { x: 0, y: 0 }), Ok(None));
    assert_eq!(
        world.resource_at(WorldPosition { x: 2, y: 1 }),
        Ok(Some(FeatureKind::BerryBush.base_resource()))
    );
    let berry = world.feature_at(WorldPosition { x: 2, y: 1 }).unwrap();
    assert_eq!(berry.identity(), berry.position);

    assert_eq!(
        world.resource_at(WorldPosition { x: -1, y: 0 }),
        Err(WorldQueryError::Unloaded)
    );
    assert_eq!(
        world.water_at(WorldPosition {
            x: WORLD_HALF_EXTENT,
            y: 0
        }),
        Err(WorldQueryError::OutsideWorldBounds)
    );
    assert_eq!(
        world.traversal_step(WorldPosition { x: 0, y: 0 }, WorldPosition { x: 0, y: 0 }),
        Err(WorldQueryError::NonCardinalStep)
    );
    assert_eq!(
        world.traversal_step(WorldPosition { x: -1, y: 0 }, WorldPosition { x: -2, y: 0 }),
        Err(WorldQueryError::Unloaded)
    );
    assert_eq!(
        world.traversal_step(
            WorldPosition {
                x: WORLD_HALF_EXTENT,
                y: 0,
            },
            WorldPosition {
                x: WORLD_HALF_EXTENT - 1,
                y: 0,
            }
        ),
        Err(WorldQueryError::OutsideWorldBounds)
    );
}

#[test]
fn feature_resources_are_derived_without_mutating_generated_base() {
    let cases = [
        (FeatureKind::Tree, ResourceKind::Wood, 120),
        (FeatureKind::Rock, ResourceKind::Stone, 80),
        (FeatureKind::BerryBush, ResourceKind::Food, 12),
    ];
    for (kind, resource_kind, capacity) in cases {
        let feature = Feature {
            position: WorldPosition { x: -7, y: 11 },
            kind,
        };
        assert_eq!(
            feature.base_resource(),
            BaseResource {
                capacity,
                kind: resource_kind,
            }
        );
    }
}

#[test]
fn packed_terrain_classes_round_trip_every_public_semantic() {
    let surfaces = [
        SurfaceType::DeepWater,
        SurfaceType::ShallowWater,
        SurfaceType::Sand,
        SurfaceType::Soil,
        SurfaceType::Hill,
        SurfaceType::Rock,
        SurfaceType::SnowIce,
    ];
    let biomes = [
        BiomeType::Ocean,
        BiomeType::Lake,
        BiomeType::River,
        BiomeType::Beach,
        BiomeType::Desert,
        BiomeType::Grassland,
        BiomeType::Savanna,
        BiomeType::Forest,
        BiomeType::Wetland,
        BiomeType::Tundra,
        BiomeType::Alpine,
    ];
    let mut packed = BTreeSet::new();
    for surface in surfaces {
        for biome in biomes {
            let class = TerrainClass::new(surface, biome);
            assert_eq!(class.surface(), surface);
            assert_eq!(class.biome(), biome);
            assert!(packed.insert(class.packed()));
        }
    }
}

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
