use sim_core::{
    AgentId, Engine, EngineCommand, EngineConfig, MoveRequestError, PhysicalGoal,
    PolicyActivationError, PolicyDiagnosticKind, PolicyFailureReason, PopulationInit, RouteRequest,
    RouteRequestError, Standability, WaterSource, WorldConfig, WorldPosition, WorldRect,
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
                if engine.world().standability_at(access) == Ok(Standability::Standable) {
                    return access;
                }
            }
        }
    }
    panic!("seeded world should expose resident drinkable-water access");
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
        seed: 1,
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

fn run_to(engine: &mut Engine, tick: u64) {
    while engine.snapshot().tick < tick {
        engine.tick();
    }
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
    assert_eq!(engine.snapshot().scheduled_event_count, 4);
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
fn thirst_selects_drink_deterministically_then_uses_positive_backoff() {
    let (base, bounds) = river_engine();
    let position = water_access(&base, bounds);
    let (replay_base, replay_bounds) = river_engine();
    let mut first = active_engine(base, bounds, position);
    let mut replay = active_engine(replay_base, replay_bounds, position);
    first.activate_physical_policy().unwrap();
    replay.activate_physical_policy().unwrap();

    run_to(&mut first, 90_001);
    run_to(&mut replay, 90_001);
    assert_eq!(first.policy_diagnostics(), replay.policy_diagnostics());
    assert!(first.policy_diagnostics().iter().any(|diagnostic| {
        diagnostic.kind == PolicyDiagnosticKind::ActionStarted
            && diagnostic.goal == PhysicalGoal::Drink
            && diagnostic.target == Some(position)
    }));
    let committed = first.physical_policy(AgentId::new(0)).unwrap();
    assert_eq!(committed.goal, PhysicalGoal::Drink);
    assert!(committed.committed);

    run_to(&mut first, 90_061);
    assert!(first.policy_diagnostics().iter().any(|diagnostic| {
        diagnostic.kind == PolicyDiagnosticKind::ActionDeferred
            && diagnostic.failure == Some(PolicyFailureReason::DeferredToLaterSlice)
    }));
    assert!(first.policy_diagnostics().iter().any(|diagnostic| {
        diagnostic.kind == PolicyDiagnosticKind::RetryScheduled
            && diagnostic.failure == Some(PolicyFailureReason::DeferredToLaterSlice)
    }));
    assert!(!first.physical_policy(AgentId::new(0)).unwrap().committed);
    assert!(!first.policy_diagnostics().iter().any(|diagnostic| {
        diagnostic.kind == PolicyDiagnosticKind::StaleEvent && diagnostic.at.ticks() == 90_061
    }));
}

#[test]
fn missing_perceived_target_reconsiders_without_a_same_time_loop() {
    let base = resident_engine();
    let position = standable_position(&base);
    let mut engine = single_cell_engine(position);
    engine.activate_physical_policy().unwrap();
    run_to(&mut engine, 90_001);
    assert!(engine.policy_diagnostics().iter().any(|diagnostic| {
        diagnostic.goal == PhysicalGoal::SeekWater
            && diagnostic.kind == PolicyDiagnosticKind::RetryScheduled
            && diagnostic.failure == Some(PolicyFailureReason::NoPerceivedTarget)
    }));
    let snapshot = engine.snapshot();
    assert!(!engine.policy_diagnostics().is_empty());
    assert!(snapshot.scheduled_event_count >= 1);
}
