//! Engine construction, commands, reset, determinism, terrain residency, and population setup tests.

use super::*;

#[test]
fn additive_spawn_expands_across_contiguous_resident_terrain() {
    let mut engine = resident_engine(128);
    let initial = engine.world().initial_bounds();
    let left = WorldRect {
        min: initial.min,
        max: WorldPosition {
            x: 0,
            y: initial.max.y,
        },
    };
    let standable_in = |bounds: WorldRect| {
        (bounds.min.y..bounds.max.y)
            .flat_map(|y| (bounds.min.x..bounds.max.x).map(move |x| WorldPosition { x, y }))
            .find(|position| {
                engine.world().standability_at(*position) == Ok(Standability::Standable)
            })
            .unwrap()
    };
    let first = standable_in(left);
    let second = standable_in(WorldRect {
        min: WorldPosition {
            x: 0,
            y: initial.min.y,
        },
        max: initial.max,
    });
    engine
        .initialize_population(
            PopulationInit {
                active_area: left,
                population: 1,
            },
            &[first],
        )
        .unwrap();
    engine.activate_physical_policy_with_exploration().unwrap();

    assert_eq!(engine.spawn_agent(second), Ok(AgentId::new(1)));
    assert_eq!(engine.population.active_area(), Some(initial));
    assert_eq!(engine.snapshot().agent_count, 2);
    assert_eq!(engine.agent_views(2).last().unwrap().position, second);
}

#[test]
fn identical_inputs_produce_identical_snapshots() {
    let mut left = Engine::default();
    let mut right = Engine::default();
    for _ in 0..1_000 {
        left.tick();
        right.tick();
    }
    assert_eq!(left.snapshot(), right.snapshot());
}

#[test]
fn pause_prevents_time_advancing() {
    let mut engine = Engine::default();
    engine.command(EngineCommand::SetPaused(true));
    engine.tick();
    assert_eq!(engine.snapshot().tick, 0);
}

#[test]
fn reset_restores_runtime_state_but_preserves_config() {
    let config = EngineConfig {
        seed: 42,
        ticks_per_second: 20,
        ..EngineConfig::default()
    };
    let mut engine = Engine::new(config);
    engine.tick();
    engine.command(EngineCommand::SetSpeed(8.0));
    engine.command(EngineCommand::Reset);
    assert_eq!(engine.config(), config);
    assert_eq!(engine.snapshot().tick, 0);
    assert_eq!(engine.snapshot().speed, 1.0);
}

#[test]
fn simulation_speed_accepts_the_viewer_ceiling_and_caps_above_it() {
    let mut engine = Engine::default();
    assert_eq!(
        engine.command(EngineCommand::SetSpeed(4096.0)),
        EngineCommandOutcome::Applied
    );
    assert_eq!(engine.snapshot().speed, MAX_SIMULATION_SPEED);
    engine.command(EngineCommand::SetSpeed(8192.0));
    assert_eq!(engine.snapshot().speed, MAX_SIMULATION_SPEED);
}

#[test]
fn equal_seeds_generate_equal_worlds() {
    let left = Engine::new(EngineConfig {
        seed: 99,
        ..EngineConfig::default()
    });
    let right = Engine::new(EngineConfig {
        seed: 99,
        ..EngineConfig::default()
    });

    assert_eq!(left.world(), right.world());
}

#[test]
fn engine_construction_is_deferred_and_headless_materialization_is_explicit() {
    let config = EngineConfig {
        seed: 99,
        world: WorldConfig::new(96, 64).unwrap(),
        ..EngineConfig::default()
    };
    let mut engine = Engine::new(config);

    assert_eq!(engine.world().loaded_chunk_count(), 0);
    assert!(
        !engine
            .world()
            .area_is_generated(engine.world().initial_bounds())
    );
    engine.materialize_initial_area().unwrap();
    let eager = World::generate(config.seed, config.world);
    assert_eq!(
        engine.world().cells().collect::<Vec<_>>(),
        eager.cells().collect::<Vec<_>>()
    );
    assert_eq!(
        engine.world().all_features().copied().collect::<Vec<_>>(),
        eager.all_features().copied().collect::<Vec<_>>()
    );
}

#[test]
fn terrain_materialization_does_not_change_fixed_tick_progression() {
    let config = EngineConfig {
        seed: 99,
        world: WorldConfig::new(96, 64).unwrap(),
        ..EngineConfig::default()
    };
    let mut unloaded = Engine::new(config);
    let mut resident = Engine::new(config);
    let loads = resident
        .world()
        .missing_chunk_load_requests(resident.world().initial_bounds())
        .unwrap()
        .into_iter()
        .map(|request| World::generate_chunk_load(config.seed, request))
        .collect();

    assert_eq!(resident.apply_world_chunk_loads(loads), Ok(4));
    for _ in 0..600 {
        unloaded.tick();
        resident.tick();
    }

    assert_eq!(unloaded.snapshot(), resident.snapshot());
    assert_eq!(unloaded.snapshot().tick, 600);
}

#[test]
fn generate_initial_area_command_uses_bootstrap_batching() {
    let config = EngineConfig {
        seed: 99,
        world: WorldConfig::new(128, 64).unwrap(),
        ..EngineConfig::default()
    };
    let mut engine = Engine::new(config);
    let initial = engine.world().initial_bounds();

    engine.command(EngineCommand::GenerateWorldArea(initial));

    assert!(engine.world().area_is_generated(initial));
    assert_eq!(engine.world().loaded_chunk_count(), 4);
}

#[test]
fn applied_chunks_remain_engine_owned_and_deduplicated() {
    let config = EngineConfig {
        seed: 9,
        world: WorldConfig::new(64, 64).unwrap(),
        ..EngineConfig::default()
    };
    let mut engine = Engine::new(config);
    let coord = ChunkCoord { x: -1, y: 0 };
    let chunk = World::generate_chunk_at(config.seed, coord).unwrap();

    assert_eq!(engine.apply_world_chunks(vec![chunk.clone()]), Ok(1));
    let revision = engine.world().revision();
    assert_eq!(
        engine
            .world()
            .inspect_chunk_at(WorldPosition { x: -1, y: 0 })
            .unwrap()
            .presence,
        ChunkPresence::RetainedPartialInitial
    );
    assert_eq!(engine.apply_world_chunks(vec![chunk]), Ok(0));
    assert_eq!(engine.world().revision(), revision);
}

#[test]
fn population_initialization_is_explicit_atomic_and_deterministic() {
    let config = EngineConfig {
        seed: 42,
        world: WorldConfig::new(64, 64).unwrap(),
        ..EngineConfig::default()
    };
    let mut engine = Engine::new(config);
    let init = PopulationInit {
        active_area: engine.world().initial_bounds(),
        population: 2,
    };
    assert_eq!(
        engine.initialize_population(init, &[]),
        Err(PopulationInitError::IncompleteResidency)
    );
    assert_eq!(engine.snapshot().agent_count, 0);

    engine.materialize_initial_area().unwrap();
    let pair = standable_steps(&engine, 1)[0];
    let blocked = engine
        .world()
        .cells()
        .map(|(position, _)| position)
        .find(|&position| {
            position != pair.0
                && position != pair.1
                && engine.world().standability_at(position) == Ok(Standability::Standable)
        })
        .expect("seeded test area should contain another standable spawn");
    engine.spawn_object(SpawnKind::Water, blocked).unwrap();
    assert_eq!(
        engine.initialize_population(init, &[blocked]),
        Err(PopulationInitError::InvalidSpawn {
            position: blocked,
            reason: SpawnInvalidReason::Water,
        })
    );
    assert_eq!(engine.snapshot().agent_count, 0);
    assert_eq!(
        engine.initialize_population(init, &[pair.0, pair.0]),
        Err(PopulationInitError::DuplicatePosition { position: pair.0 })
    );
    assert_eq!(engine.snapshot().agent_count, 0);

    let one_cell = WorldRect {
        min: pair.0,
        max: WorldPosition {
            x: pair.0.x + 1,
            y: pair.0.y + 1,
        },
    };
    assert_eq!(
        engine.initialize_population(
            PopulationInit {
                active_area: one_cell,
                population: 2,
            },
            &[],
        ),
        Err(PopulationInitError::InsufficientValidSpawnCells {
            requested: 2,
            found: 1,
        })
    );
    assert_eq!(engine.snapshot().agent_count, 0);

    let outcome = engine
        .initialize_population(init, &[pair.0, pair.1])
        .unwrap();
    assert_eq!(outcome.first_id, AgentId::new(0));
    assert_eq!(
        engine.agent_views(10).collect::<Vec<_>>(),
        [
            AgentView {
                id: AgentId::new(0),
                position: pair.0,
                activity: AgentActivity::Idle,
            },
            AgentView {
                id: AgentId::new(1),
                position: pair.1,
                activity: AgentActivity::Idle,
            },
        ]
    );
}

#[test]
fn reset_clears_population_scheduler_and_id_sequence_but_keeps_residency() {
    let mut engine = resident_engine(64);
    let pair = standable_steps(&engine, 1)[0];
    let init = PopulationInit {
        active_area: engine.world().initial_bounds(),
        population: 1,
    };
    engine.initialize_population(init, &[pair.0]).unwrap();
    engine.request_move(AgentId::new(0), pair.1).unwrap();
    let chunks = engine.world().loaded_chunk_count();
    engine.command(EngineCommand::Reset);
    assert_eq!(engine.snapshot().tick, 0);
    assert_eq!(engine.snapshot().agent_count, 0);
    assert_eq!(engine.snapshot().scheduled_event_count, 0);
    assert_eq!(engine.world().loaded_chunk_count(), chunks);
    assert_eq!(
        engine
            .initialize_population(init, &[pair.0])
            .unwrap()
            .first_id,
        AgentId::new(0)
    );
}
