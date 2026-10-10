use sim_core::{
    AgentId, Engine, EngineCommand, EngineConfig, FeatureKind, Material, MovementOutcomeKind,
    PopulationInit, SpawnKind, SpawnObjectError, Standability, WaterSource, WorldConfig,
    WorldPosition, WorldRect,
};

fn resident_engine() -> Engine {
    let mut engine = Engine::new(EngineConfig {
        seed: 42,
        ticks_per_second: 60,
        world: WorldConfig::new(128, 128).unwrap(),
    });
    engine.materialize_initial_area().unwrap();
    engine
}

fn clear_cross(engine: &Engine) -> WorldPosition {
    let bounds = engine.world().initial_bounds();
    for y in bounds.min.y + 4..bounds.max.y - 4 {
        for x in bounds.min.x + 4..bounds.max.x - 4 {
            let center = WorldPosition { x, y };
            let cells = [
                center,
                WorldPosition { x, y: y - 1 },
                WorldPosition { x: x - 1, y },
                WorldPosition { x: x + 1, y },
                WorldPosition { x, y: y + 1 },
            ];
            if cells.into_iter().all(|position| {
                engine.world().standability_at(position) == Ok(Standability::Standable)
                    && engine.world().resource_at(position) == Ok(None)
            }) {
                return center;
            }
        }
    }
    panic!("seeded resident world should contain a clear five-cell cross");
}

fn active_area(center: WorldPosition) -> WorldRect {
    WorldRect {
        min: WorldPosition {
            x: center.x - 4,
            y: center.y - 4,
        },
        max: WorldPosition {
            x: center.x + 5,
            y: center.y + 5,
        },
    }
}

#[test]
fn spawned_resources_and_fresh_water_are_authoritative_agent_facts() {
    let mut engine = resident_engine();
    let center = clear_cross(&engine);
    let tree = WorldPosition {
        x: center.x + 1,
        y: center.y,
    };
    let berries = WorldPosition {
        x: center.x,
        y: center.y + 1,
    };
    let water = WorldPosition {
        x: center.x - 1,
        y: center.y,
    };
    engine.spawn_object(SpawnKind::Tree, tree).unwrap();
    engine.spawn_object(SpawnKind::BerryBush, berries).unwrap();
    engine.spawn_object(SpawnKind::Water, water).unwrap();
    engine
        .initialize_population(
            PopulationInit {
                active_area: active_area(center),
                population: 1,
            },
            &[center],
        )
        .unwrap();

    let perception = engine.perceive_physical(AgentId::new(0), 2).unwrap();
    assert!(
        perception
            .resources
            .iter()
            .any(|fact| { fact.position == tree && fact.resource.kind == Material::Wood })
    );
    assert!(
        perception
            .resources
            .iter()
            .any(|fact| { fact.position == berries && fact.resource.kind == Material::Berries })
    );
    assert!(
        perception
            .drinkable_water
            .iter()
            .any(|fact| { fact.position == water && fact.source == WaterSource::Lake })
    );
    assert_eq!(
        engine.physical_standability_at(tree),
        Ok(Standability::Standable)
    );
    assert_eq!(
        engine.physical_standability_at(water),
        Ok(Standability::BlockedByWater)
    );
    assert_eq!(
        engine.physical_standability_at(berries),
        Ok(Standability::Standable)
    );
}

#[test]
fn spawned_trees_and_rocks_do_not_block_in_flight_movement() {
    let mut engine = resident_engine();
    let from = clear_cross(&engine);
    let target = WorldPosition {
        x: from.x + 1,
        y: from.y,
    };
    engine
        .initialize_population(
            PopulationInit {
                active_area: active_area(from),
                population: 1,
            },
            &[from],
        )
        .unwrap();
    let scheduled = engine.request_move(AgentId::new(0), target).unwrap();
    engine.spawn_object(SpawnKind::Rock, target).unwrap();
    while engine.snapshot().tick < scheduled.completes_at.ticks() {
        engine.tick();
    }
    assert!(
        engine
            .movement_outcomes()
            .iter()
            .any(|outcome| outcome.kind == MovementOutcomeKind::Moved)
    );
    assert_eq!(engine.agent_views(1).next().unwrap().position, target);
}

#[test]
fn placement_rejects_conflicts_and_reset_clears_the_sparse_layer() {
    let mut engine = resident_engine();
    let generated_feature = engine
        .world()
        .features_in(engine.world().initial_bounds())
        .find(|feature| matches!(feature.kind, FeatureKind::Tree | FeatureKind::Rock))
        .expect("seeded resident world should contain a tree or rock")
        .position;
    assert_eq!(
        engine.spawn_object(SpawnKind::Water, generated_feature),
        Err(SpawnObjectError::BlockedByFeature)
    );
    let center = clear_cross(&engine);
    engine.spawn_object(SpawnKind::Tree, center).unwrap();
    assert_eq!(
        engine.spawn_object(SpawnKind::Water, center),
        Err(SpawnObjectError::ExistingObject)
    );
    assert_eq!(engine.spawned_object_count(), 1);
    assert_eq!(engine.spawned_object_views().count(), 1);
    engine.command(EngineCommand::Reset);
    assert_eq!(engine.spawned_object_count(), 0);
    assert_eq!(
        engine.physical_standability_at(center),
        Ok(Standability::Standable)
    );
}
