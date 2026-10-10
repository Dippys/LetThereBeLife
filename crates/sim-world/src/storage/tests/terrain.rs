//! Terrain layout, classification, determinism, and physical point-query tests.

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
    assert_eq!(std::mem::size_of::<Material>(), 1);
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
        Ok(Standability::Standable)
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
    // Lakes and rivers can be waded or swum, slowly; the sea can't be entered.
    for (from, to) in [((0, 0), (1, 0)), ((2, 1), (2, 0))] {
        let step = world
            .traversal_step(
                WorldPosition {
                    x: from.0,
                    y: from.1,
                },
                WorldPosition { x: to.0, y: to.1 },
            )
            .unwrap();
        assert_eq!(step.kind(), TraversalKind::Passable, "{from:?} -> {to:?}");
        assert!(step.cost() > Some(crate::MIN_TRAVERSAL_COST), "slow");
    }
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
        TraversalKind::Passable
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
        TraversalKind::Passable
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
        (FeatureKind::Tree, Material::Wood, 120),
        (FeatureKind::Rock, Material::Stone, 80),
        (FeatureKind::BerryBush, Material::Berries, 12),
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
