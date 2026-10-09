//! Public-API checks for agent memory: what agents see becomes private,
//! persistent knowledge, and memory-driven behavior stays deterministic.

use sim_core::{
    AgentId, Engine, EngineCommand, EngineConfig, LandmarkKind, LandmarkSource, PolicyOptions,
    PopulationInit, Standability, WaterSource, WorldConfig, WorldPosition, WorldRect,
};

/// A seed-1 area containing a lake or river, with every cell resident.
fn watered_engine() -> (Engine, WorldRect) {
    let bounds = WorldRect {
        min: WorldPosition {
            x: -16_128,
            y: -16_896,
        },
        max: WorldPosition {
            x: -15_104,
            y: -15_872,
        },
    };
    let mut engine = Engine::new(EngineConfig {
        seed: 1,
        ticks_per_second: 60,
        world: WorldConfig::new(64, 64).unwrap(),
    });
    engine.command(EngineCommand::GenerateWorldArea(bounds));
    (engine, bounds)
}

fn first_water_access(engine: &Engine, bounds: WorldRect) -> WorldPosition {
    for y in bounds.min.y + 16..bounds.max.y - 16 {
        for x in bounds.min.x + 16..bounds.max.x - 16 {
            let water = WorldPosition { x, y };
            if !engine
                .world()
                .water_at(water)
                .is_ok_and(|source| source.is_some_and(WaterSource::is_drinkable))
            {
                continue;
            }
            let access = WorldPosition { x, y: y - 1 };
            if engine.world().standability_at(access) == Ok(Standability::Standable) {
                return access;
            }
        }
    }
    panic!("seeded area should contain drinkable water");
}

fn thinking_engine() -> (Engine, WorldPosition) {
    let (mut engine, bounds) = watered_engine();
    let spawn = first_water_access(&engine, bounds);
    engine
        .initialize_population(
            PopulationInit {
                active_area: bounds,
                population: 1,
            },
            &[spawn],
        )
        .unwrap();
    engine
        .activate_physical_policy_with_options(PolicyOptions::full())
        .unwrap();
    (engine, spawn)
}

#[test]
fn agents_remember_water_they_have_seen() {
    let (mut engine, spawn) = thinking_engine();
    assert!(
        engine.mental_map(AgentId::new(0)).is_none(),
        "no mind before the first decision"
    );
    engine.tick();
    let map = engine
        .mental_map(AgentId::new(0))
        .expect("first decision creates a mind");
    let water = map
        .landmarks
        .iter()
        .find(|place| place.kind == LandmarkKind::Water)
        .expect("water beside the spawn is remembered");
    assert_eq!(water.source, LandmarkSource::Seen);
    assert!(water.position.x.abs_diff(spawn.x) + water.position.y.abs_diff(spawn.y) <= 16);
    assert!(map.explored_tiles >= 1);
}

#[test]
fn memory_driven_runs_replay_identically() {
    let (mut first, _) = thinking_engine();
    let (mut second, _) = thinking_engine();
    for _ in 0..60_000 {
        first.tick();
        second.tick();
    }
    assert_eq!(
        first.agent_views(1).collect::<Vec<_>>(),
        second.agent_views(1).collect::<Vec<_>>()
    );
    assert_eq!(
        first.mental_map(AgentId::new(0)),
        second.mental_map(AgentId::new(0))
    );
}

#[test]
fn legacy_activation_keeps_agents_mindless() {
    let (mut engine, bounds) = watered_engine();
    let spawn = first_water_access(&engine, bounds);
    engine
        .initialize_population(
            PopulationInit {
                active_area: bounds,
                population: 1,
            },
            &[spawn],
        )
        .unwrap();
    engine.activate_physical_policy_with_exploration().unwrap();
    for _ in 0..1_000 {
        engine.tick();
    }
    assert!(!engine.policy_options().memory);
    assert!(engine.mental_map(AgentId::new(0)).is_none());
}
