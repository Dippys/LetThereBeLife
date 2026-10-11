//! Public-API autonomous gathering, shelter construction, and reset scenario.

use sim_core::{
    AgentId, Engine, EngineCommand, EngineConfig, Material, PopulationInit, Standability,
    StructureState, WorldConfig, WorldPosition,
};

fn engine_and_spawn() -> (Engine, WorldPosition) {
    let mut engine = Engine::new(EngineConfig {
        seed: 42,
        ticks_per_second: 60,
        world: WorldConfig::new(128, 128).unwrap(),
    });
    engine.materialize_initial_area().unwrap();
    let bounds = engine.world().initial_bounds();
    let mut wood = Vec::new();
    for y in bounds.min.y..bounds.max.y {
        for x in bounds.min.x..bounds.max.x {
            let position = WorldPosition { x, y };
            match engine.world().resource_at(position).unwrap() {
                Some(resource) if resource.kind == Material::Wood => wood.push(position),
                _ => {}
            }
        }
    }
    let spawn = (bounds.min.y..bounds.max.y)
        .flat_map(|y| (bounds.min.x..bounds.max.x).map(move |x| WorldPosition { x, y }))
        .find(|position| {
            engine.world().standability_at(*position) == Ok(Standability::Standable)
                && wood
                    .iter()
                    .any(|resource| chebyshev(*position, *resource) <= 7)
        })
        .expect("seeded active area should expose nearby timber");
    (engine, spawn)
}

fn chebyshev(left: WorldPosition, right: WorldPosition) -> u64 {
    left.x.abs_diff(right.x).max(left.y.abs_diff(right.y))
}

#[test]
fn autonomous_policy_gathers_builds_and_reset_clears_authoritative_shelter() {
    let (mut engine, spawn) = engine_and_spawn();
    let active_area = engine.world().initial_bounds();
    engine
        .initialize_population(
            PopulationInit {
                active_area,
                population: 1,
            },
            &[spawn],
        )
        .unwrap();
    engine.activate_physical_policy().unwrap();

    for _ in 0..20_000 + sim_core::SHELTER_BUILD_TICKS {
        engine.tick();
        if engine
            .structure_views(1)
            .any(|structure| structure.state == StructureState::Complete)
        {
            break;
        }
    }
    let shelter = engine
        .structure_views(1)
        .find(|structure| structure.state == StructureState::Complete)
        .expect("physical policy should gather the compact recipe and complete a shelter");
    assert_eq!(engine.snapshot().structure_count, 1);
    assert_eq!(shelter.builder, None);
    assert!(
        engine
            .perceive_physical(AgentId::new(0), 8)
            .unwrap()
            .structures
            .iter()
            .any(|structure| structure.id == shelter.id)
    );

    engine.command(EngineCommand::Reset);
    assert_eq!(engine.snapshot().structure_count, 0);
    assert_eq!(engine.structure_views(1).next(), None);
}
