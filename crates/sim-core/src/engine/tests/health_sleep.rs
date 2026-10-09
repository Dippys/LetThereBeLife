//! Lethal and terminal health consequences and sleep-request rejection tests.

use super::*;

#[test]
fn lethal_health_consequence_releases_the_cell_for_other_agents() {
    let mut engine = resident_engine(64);
    let (from, target) = standable_steps(&engine, 1)[0];
    engine
        .initialize_population(
            PopulationInit {
                active_area: engine.world().initial_bounds(),
                population: 2,
            },
            &[from, target],
        )
        .unwrap();
    engine.population.set_need_value_for_test(
        AgentId::new(0),
        NeedKind::Thirst,
        8_000,
        SimTime::ZERO,
    );
    engine.request_move(AgentId::new(0), target).unwrap();
    engine.population.prepare_health_consequence_for_test(
        &mut engine.scheduler,
        AgentId::new(0),
        HEALTH_INCAPACITATION_THRESHOLD,
        SimTime::ZERO,
    );

    engine.tick();

    assert_eq!(engine.death_records().len(), 1);
    assert_eq!(engine.death_records()[0].cause, DeathCause::Dehydration);
    assert_eq!(engine.death_records()[0].at, SimTime::ZERO);
    assert_eq!(engine.population.spatial().occupant(from), None);
    assert_eq!(
        engine.population.spatial().occupant(target),
        Some(AgentId::new(1))
    );
    assert_eq!(engine.snapshot().living_agent_count, 1);
    assert_eq!(
        engine.health_diagnostics().last().unwrap().kind,
        HealthDiagnosticKind::Died
    );

    let movement = engine.request_move(AgentId::new(1), from).unwrap();
    while engine.snapshot().tick < movement.completes_at.ticks() {
        engine.tick();
    }
    assert_eq!(engine.agent_views(2).nth(1).unwrap().position, from);
}

#[test]
fn terminal_health_cancels_construction_and_makes_completion_stale() {
    let mut engine = resident_engine(64);
    let (access, site) = standable_shelter_site(&engine);
    engine
        .initialize_population(
            PopulationInit {
                active_area: engine.world().initial_bounds(),
                population: 1,
            },
            &[access],
        )
        .unwrap();
    engine
        .population
        .add_inventory(AgentId::new(0), ResourceKind::Wood, SHELTER_WOOD_COST);
    let build = engine.request_build_shelter(AgentId::new(0), site).unwrap();
    engine.population.set_need_value_for_test(
        AgentId::new(0),
        NeedKind::Thirst,
        8_000,
        SimTime::ZERO,
    );
    engine.population.prepare_health_consequence_for_test(
        &mut engine.scheduler,
        AgentId::new(0),
        HEALTH_INCAPACITATION_THRESHOLD,
        SimTime::ZERO,
    );

    engine.tick();
    assert_eq!(engine.snapshot().structure_count, 0);
    assert!(engine.structure_diagnostics().iter().any(|diagnostic| {
        diagnostic.structure.id == build.id && diagnostic.kind == StructureDiagnosticKind::Cancelled
    }));
    while engine.snapshot().tick <= build.completes_at.ticks() {
        engine.tick();
    }
    assert_eq!(engine.snapshot().structure_count, 0);
    assert_eq!(engine.death_records().len(), 1);
}

#[test]
fn sleep_rejects_unsafe_exposure_and_sequence_exhaustion_without_partial_state() {
    let mut engine = resident_engine(128);
    let position = standable_steps(&engine, 1)[0].0;
    engine
        .initialize_population(
            PopulationInit {
                active_area: engine.world().initial_bounds(),
                population: 1,
            },
            &[position],
        )
        .unwrap();
    engine.population.set_need_value_for_test(
        AgentId::new(0),
        NeedKind::Exposure,
        7_000,
        engine.time,
    );
    assert_eq!(
        engine.request_sleep(AgentId::new(0), position),
        Err(SleepRequestError::UnsafeExposure)
    );
    assert_eq!(engine.sleep(AgentId::new(0)), None);
    assert_eq!(
        engine.agent_views(1).next().unwrap().activity,
        AgentActivity::Idle
    );

    engine
        .population
        .set_need_value_for_test(AgentId::new(0), NeedKind::Exposure, 0, engine.time);
    engine.scheduler.exhaust_sequence();
    let before = engine.physical_needs(AgentId::new(0)).unwrap();
    assert_eq!(
        engine.request_sleep(AgentId::new(0), position),
        Err(SleepRequestError::EventSequenceExhausted)
    );
    assert_eq!(engine.sleep(AgentId::new(0)), None);
    assert_eq!(engine.physical_needs(AgentId::new(0)).unwrap(), before);
    assert_eq!(
        engine.agent_views(1).next().unwrap().activity,
        AgentActivity::Idle
    );
}
