//! Public-API sleep scenarios: planned wake, urgent interruption, and location validation.

use sim_core::{
    AgentActivity, AgentId, Engine, EngineCommand, EngineConfig, FeatureKind, PopulationInit,
    SleepDiagnosticKind, SleepInterruptionReason, SleepQuality, SleepRequestError, Standability,
    WorldConfig, WorldPosition, WorldRect,
};

fn engine_with_positions(count: u32) -> (Engine, Vec<WorldPosition>) {
    let mut engine = Engine::new(EngineConfig {
        seed: 42,
        ticks_per_second: 60,
        world: WorldConfig::new(128, 128).unwrap(),
    });
    engine.materialize_initial_area().unwrap();
    let bounds = engine.world().initial_bounds();
    let mut positions = Vec::new();
    'rows: for y in bounds.min.y..bounds.max.y {
        for x in bounds.min.x..bounds.max.x {
            let position = WorldPosition { x, y };
            if engine.world().standability_at(position) == Ok(Standability::Standable) {
                positions.push(position);
                if positions.len() == count as usize {
                    break 'rows;
                }
            }
        }
    }
    engine
        .initialize_population(
            PopulationInit {
                active_area: bounds,
                population: count,
            },
            &positions,
        )
        .unwrap();
    (engine, positions)
}

fn run_to(engine: &mut Engine, tick: u64) {
    while engine.snapshot().tick < tick {
        engine.tick();
    }
}

fn river_engine_with_positions(count: u32) -> (Engine, Vec<WorldPosition>) {
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
    let mut positions = Vec::new();
    'rows: for y in bounds.min.y..bounds.max.y {
        for x in bounds.min.x..bounds.max.x {
            let position = WorldPosition { x, y };
            if engine.world().standability_at(position) == Ok(Standability::Standable) {
                positions.push(position);
                if positions.len() == count as usize {
                    break 'rows;
                }
            }
        }
    }
    engine
        .initialize_population(
            PopulationInit {
                active_area: bounds,
                population: count,
            },
            &positions,
        )
        .unwrap();
    (engine, positions)
}

#[test]
fn planned_wake_recovers_rest_analytically_and_pause_preserves_it() {
    let (mut engine, positions) = engine_with_positions(1);
    run_to(&mut engine, 60);
    let before = engine.physical_needs(AgentId::new(0)).unwrap().rest.value;
    let sleep = engine.request_sleep(AgentId::new(0), positions[0]).unwrap();
    assert_eq!(sleep.quality, SleepQuality::OpenGround);
    assert_eq!(sleep.started_at.ticks(), 60);
    assert_eq!(sleep.planned_wake.ticks(), 68);
    assert_eq!(engine.sleep(AgentId::new(0)), Some(sleep));

    engine.command(EngineCommand::SetPaused(true));
    for _ in 0..100 {
        engine.tick();
    }
    assert_eq!(engine.snapshot().tick, 60);
    assert_eq!(engine.sleep(AgentId::new(0)), Some(sleep));
    engine.command(EngineCommand::SetPaused(false));
    run_to(&mut engine, sleep.planned_wake.ticks());

    assert_eq!(engine.sleep(AgentId::new(0)), None);
    assert_eq!(
        engine.physical_needs(AgentId::new(0)).unwrap().rest.value,
        0
    );
    assert!(before > 0);
    assert_eq!(
        engine.agent_views(1).next().unwrap().activity,
        AgentActivity::Idle
    );
    assert_eq!(engine.sleep_diagnostics().len(), 1);
    assert_eq!(
        engine.sleep_diagnostics()[0].kind,
        SleepDiagnosticKind::Woke
    );
}

#[test]
fn urgent_thirst_interrupts_once_and_leaves_the_planned_wake_stale() {
    let (mut engine, positions) = engine_with_positions(1);
    run_to(&mut engine, 89_990);
    let sleep = engine.request_sleep(AgentId::new(0), positions[0]).unwrap();
    assert!(sleep.planned_wake.ticks() > 90_000);

    run_to(&mut engine, 90_010);
    assert_eq!(engine.sleep(AgentId::new(0)), None);
    assert_eq!(engine.sleep_diagnostics().len(), 1);
    assert_eq!(
        engine.sleep_diagnostics()[0].kind,
        SleepDiagnosticKind::Interrupted
    );
    assert_eq!(
        engine.sleep_diagnostics()[0].interruption,
        Some(SleepInterruptionReason::Thirst)
    );
    assert_eq!(
        engine.agent_views(1).next().unwrap().activity,
        AgentActivity::Idle
    );

    run_to(&mut engine, sleep.planned_wake.ticks());
    assert_eq!(engine.sleep(AgentId::new(0)), None);
    assert_eq!(
        engine.agent_views(1).next().unwrap().activity,
        AgentActivity::Idle
    );
    assert!(engine.sleep_diagnostics().is_empty());
}

#[test]
fn sleep_location_validation_accepts_self_and_rejects_other_occupants_and_water() {
    let (mut engine, positions) = river_engine_with_positions(2);
    assert_eq!(
        engine.request_sleep(AgentId::new(0), positions[1]),
        Err(SleepRequestError::Occupied(AgentId::new(1)))
    );

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
    let water = (bounds.min.y..bounds.max.y)
        .flat_map(|y| (bounds.min.x..bounds.max.x).map(move |x| WorldPosition { x, y }))
        .find(|&position| {
            engine.world().standability_at(position) == Ok(Standability::BlockedByWater)
        })
        .expect("seeded resident area should contain water");
    assert_eq!(
        engine.request_sleep(AgentId::new(0), water),
        Err(SleepRequestError::Water)
    );
    let exclusive_feature = (bounds.min.y..bounds.max.y)
        .flat_map(|y| (bounds.min.x..bounds.max.x).map(move |x| WorldPosition { x, y }))
        .find(|&position| {
            engine.world().feature_at(position).is_some_and(|feature| {
                matches!(feature.kind, FeatureKind::Tree | FeatureKind::Rock)
            })
        })
        .expect("seeded resident area should contain an exclusive natural feature");
    assert_eq!(
        engine.request_sleep(AgentId::new(0), exclusive_feature),
        Err(SleepRequestError::BlockingFeature)
    );

    run_to(&mut engine, 60);
    assert!(engine.request_sleep(AgentId::new(0), positions[0]).is_ok());
    assert_eq!(
        engine.request_sleep(AgentId::new(0), positions[0]),
        Err(SleepRequestError::AgentCommitted)
    );
    engine.command(EngineCommand::Reset);
    assert_eq!(engine.sleep(AgentId::new(0)), None);
}
