//! Public-API autonomous policy activation, drink selection, backoff, and target reconsideration.

use sim_core::{
    AgentId, Engine, EngineCommand, EngineConfig, MoveRequestError, PhysicalGoal,
    PolicyActivationError, PolicyDiagnosticKind, PopulationInit, RouteRequest, RouteRequestError,
    Standability, WaterSource, WorldConfig, WorldPosition, WorldRect,
};

fn resident_engine() -> Engine {
    let mut engine = Engine::new(EngineConfig {
        world_seed: 42,
        life_seed: 42,
        ticks_per_second: 60,
        world: WorldConfig::new(128, 128).unwrap(),
    });
    engine.materialize_initial_area().unwrap();
    engine
}

fn water_access(engine: &Engine, bounds: WorldRect) -> WorldPosition {
    for y in bounds.min.y..bounds.max.y {
        for x in bounds.min.x..bounds.max.x {
            let water = WorldPosition { x, y };
            if !engine
                .world()
                .water_at(water)
                .is_ok_and(|source| matches!(source, Some(WaterSource::Lake | WaterSource::River)))
            {
                continue;
            }
            for (dx, dy) in [(0, -1), (-1, 0), (1, 0), (0, 1)] {
                let access = WorldPosition {
                    x: x + dx,
                    y: y + dy,
                };
                if engine.world().standability_at(access) == Ok(Standability::Standable)
                    && nothing_to_gather_near(engine, access)
                {
                    return access;
                }
            }
        }
    }
    panic!("seeded world should expose resident drinkable-water access");
}

/// Keeps thirst tests independent of shore vegetation: an agent with trees or
/// berries in view would go gathering before it gets thirsty.
fn nothing_to_gather_near(engine: &Engine, position: WorldPosition) -> bool {
    let radius = i64::from(sim_core::PHYSICAL_POLICY_RADIUS);
    (position.y - radius..=position.y + radius).all(|y| {
        (position.x - radius..=position.x + radius)
            .all(|x| engine.world().feature_at(WorldPosition { x, y }).is_none())
    })
}

fn river_engine() -> (Engine, WorldRect) {
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
        world_seed: 1,
        life_seed: 1,
        ticks_per_second: 60,
        world: WorldConfig::new(64, 64).unwrap(),
    });
    engine.command(EngineCommand::GenerateWorldArea(bounds));
    (engine, bounds)
}

fn single_cell_engine(position: WorldPosition) -> Engine {
    let mut engine = resident_engine();
    engine
        .initialize_population(
            PopulationInit {
                active_area: WorldRect {
                    min: position,
                    max: WorldPosition {
                        x: position.x + 1,
                        y: position.y + 1,
                    },
                },
                population: 1,
            },
            &[position],
        )
        .unwrap();
    engine
}

fn standable_position(engine: &Engine) -> WorldPosition {
    let bounds = engine.world().initial_bounds();
    for y in bounds.min.y..bounds.max.y {
        for x in bounds.min.x..bounds.max.x {
            let position = WorldPosition { x, y };
            if engine.world().standability_at(position) == Ok(Standability::Standable) {
                return position;
            }
        }
    }
    panic!("seeded world should contain a standable position");
}

fn standable_step(engine: &Engine) -> (WorldPosition, WorldPosition) {
    let bounds = engine.world().initial_bounds();
    for y in bounds.min.y..bounds.max.y {
        for x in bounds.min.x..bounds.max.x {
            let from = WorldPosition { x, y };
            for (dx, dy) in [(0, -1), (-1, 0), (1, 0), (0, 1)] {
                let target = WorldPosition {
                    x: x + dx,
                    y: y + dy,
                };
                if engine
                    .world()
                    .traversal_step(from, target)
                    .is_ok_and(|step| step.is_passable())
                {
                    return (from, target);
                }
            }
        }
    }
    panic!("seeded world should contain a standable step");
}

fn active_engine(mut engine: Engine, active_area: WorldRect, position: WorldPosition) -> Engine {
    engine
        .initialize_population(
            PopulationInit {
                active_area,
                population: 1,
            },
            &[position],
        )
        .unwrap();
    engine
}

#[test]
fn policy_activation_is_explicit_and_initial_wait_is_one_commitment() {
    let mut empty = resident_engine();
    assert_eq!(
        empty.activate_physical_policy(),
        Err(PolicyActivationError::PopulationNotInitialized)
    );
    let base = resident_engine();
    let position = standable_position(&base);
    let mut engine = single_cell_engine(position);
    engine.activate_physical_policy().unwrap();
    assert_eq!(
        engine.activate_physical_policy(),
        Err(PolicyActivationError::AlreadyActive)
    );
    assert_eq!(
        engine.request_move(AgentId::new(0), position),
        Err(MoveRequestError::PolicyControlled)
    );
    assert_eq!(
        engine.request_route(
            AgentId::new(0),
            RouteRequest {
                destination: position,
                max_expansions: 1,
            },
        ),
        Err(RouteRequestError::PolicyControlled)
    );
    engine.tick();
    assert_eq!(engine.policy_diagnostics().len(), 1);
    assert_eq!(
        engine.policy_diagnostics()[0].kind,
        PolicyDiagnosticKind::Selected
    );
    assert_eq!(engine.policy_diagnostics()[0].goal, PhysicalGoal::Wait);
    let view = engine.physical_policy(AgentId::new(0)).unwrap();
    assert_eq!(view.goal, PhysicalGoal::Wait);
    assert!(!view.committed);
    assert_eq!(engine.snapshot().scheduled_event_count, 5);
}

#[test]
fn policy_activation_rejects_an_existing_manual_commitment() {
    let base = resident_engine();
    let (from, target) = standable_step(&base);
    let active_area = base.world().initial_bounds();
    let mut engine = active_engine(base, active_area, from);
    engine.request_move(AgentId::new(0), target).unwrap();
    assert_eq!(
        engine.activate_physical_policy(),
        Err(PolicyActivationError::AgentCommitted {
            agent: AgentId::new(0)
        })
    );
}

#[test]
fn a_thirsty_agent_beside_water_drinks_and_replays_identically() {
    let (base, bounds) = river_engine();
    let position = water_access(&base, bounds);
    let (replay_base, replay_bounds) = river_engine();
    let mut first = active_engine(base, bounds, position);
    let mut replay = active_engine(replay_base, replay_bounds, position);
    first.activate_physical_policy().unwrap();
    replay.activate_physical_policy().unwrap();
    let mut drank = false;
    for _ in 0..100_000 {
        first.tick();
        replay.tick();
        assert_eq!(first.policy_diagnostics(), replay.policy_diagnostics());
        drank |= first.policy_diagnostics().iter().any(|diagnostic| {
            diagnostic.kind == PolicyDiagnosticKind::ActionCompleted
                && diagnostic.goal == PhysicalGoal::Drink
                && diagnostic.failure.is_none()
        });
    }
    assert!(drank, "it drank");
    assert!(
        !first
            .physical_needs(AgentId::new(0))
            .unwrap()
            .thirst
            .threshold_reached
    );
}

#[test]
fn an_agent_with_nowhere_to_go_never_decides_twice_in_one_tick() {
    let base = resident_engine();
    let position = standable_position(&base);
    let mut engine = single_cell_engine(position);
    engine.activate_physical_policy().unwrap();
    for _ in 0..100_000 {
        engine.tick();
        let selected = engine
            .policy_diagnostics()
            .iter()
            .filter(|diagnostic| diagnostic.kind == PolicyDiagnosticKind::Selected)
            .count();
        assert!(selected <= 1, "at most one decision per tick");
    }
    assert!(engine.snapshot().scheduled_event_count >= 1);
}
