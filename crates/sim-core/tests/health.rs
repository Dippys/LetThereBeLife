//! Public-API prolonged-thirst death record and reset boundary scenario.

use sim_core::{
    AgentActivity, AgentId, DeathCause, Engine, EngineCommand, EngineConfig, HealthStatus,
    MoveRequestError, PerceptionError, PopulationInit, WorldConfig, WorldPosition,
};

fn initialized_engine() -> Engine {
    let mut engine = Engine::new(EngineConfig {
        seed: 42,
        ticks_per_second: 60,
        world: WorldConfig::new(128, 128).unwrap(),
    });
    engine.materialize_initial_area().unwrap();
    engine
        .initialize_population(
            PopulationInit {
                active_area: engine.world().initial_bounds(),
                population: 1,
            },
            &[],
        )
        .unwrap();
    engine
}

#[test]
fn prolonged_unmet_thirst_has_one_replayable_terminal_record_and_reset_boundary() {
    let mut left = initialized_engine();
    let mut right = initialized_engine();
    for _ in 0..121_800 {
        left.tick();
        right.tick();
    }

    assert_eq!(left.snapshot(), right.snapshot());
    assert_eq!(left.death_records(), right.death_records());
    let death = left.death_records()[0];
    assert_eq!(death.agent, AgentId::new(0));
    assert_eq!(death.cause, DeathCause::Dehydration);
    assert_eq!(death.at.ticks(), 121_800);
    assert_eq!(left.snapshot().agent_count, 1);
    assert_eq!(left.snapshot().living_agent_count, 0);
    assert_eq!(left.snapshot().death_count, 1);
    assert_eq!(
        left.agent_views(1).next().unwrap().activity,
        AgentActivity::Dead
    );
    assert_eq!(
        left.health(AgentId::new(0)).unwrap().status,
        HealthStatus::Dead
    );
    assert_eq!(
        left.request_move(AgentId::new(0), WorldPosition { x: 0, y: 0 }),
        Err(MoveRequestError::DeadAgent)
    );
    assert_eq!(
        left.perceive_physical(AgentId::new(0), 1),
        Err(PerceptionError::DeadAgent)
    );

    for _ in 0..1_200 {
        left.tick();
    }
    assert_eq!(left.death_records(), &[death]);

    left.command(EngineCommand::Reset);
    assert_eq!(left.snapshot().agent_count, 0);
    assert_eq!(left.snapshot().living_agent_count, 0);
    assert_eq!(left.snapshot().death_count, 0);
    assert!(left.death_records().is_empty());
}
