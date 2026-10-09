use sim_core::{EngineCommand, EngineCommandOutcome, WorldConfig};
use sim_headless::{ScenarioConfig, ScenarioRunner};

fn fast_config(population: u32) -> ScenarioConfig {
    let mut config = ScenarioConfig::canonical(population);
    config.engine.seed = 42;
    config.engine.world = WorldConfig::new(512, 512).unwrap();
    config.driver_ticks = 1_200;
    config
}

#[test]
fn twenty_and_one_hundred_agent_reports_are_repeatable_and_batch_stable() {
    for population in [20, 100] {
        let config = fast_config(population);
        let first = ScenarioRunner::new(config).unwrap().run(1).unwrap();
        let second = ScenarioRunner::new(config).unwrap().run(137).unwrap();
        assert_eq!(first, second);
        assert_eq!(first.to_string(), second.to_string());
        assert_eq!(first.final_tick, config.driver_ticks);
        assert_eq!(first.soak.invariant_violations, 0);
        assert_eq!(first.capacity.occupancy_entries, population as usize);
    }
}

#[test]
fn accepted_command_diverges_and_reset_replays_exactly() {
    let config = fast_config(20);
    let mut runner = ScenarioRunner::new(config).unwrap();
    runner
        .advance_driver_steps(config.driver_ticks, 73)
        .unwrap();
    let first = runner.report();

    runner.reset_for_replay().unwrap();
    runner
        .advance_driver_steps(config.driver_ticks, 211)
        .unwrap();
    assert_eq!(runner.report(), first);

    runner.reset_for_replay().unwrap();
    assert_eq!(
        runner.command(EngineCommand::SetPaused(true)),
        EngineCommandOutcome::Applied
    );
    runner.advance_driver_steps(1, 1).unwrap();
    assert_eq!(
        runner.command(EngineCommand::SetPaused(false)),
        EngineCommandOutcome::Applied
    );
    runner
        .advance_driver_steps(config.driver_ticks - 1, 97)
        .unwrap();
    let divergent = runner.report();
    assert_eq!(divergent.final_tick, first.final_tick - 1);
    assert_ne!(divergent.semantic_hash, first.semantic_hash);
}

#[test]
#[ignore = "canonical 600,000-tick release scenario"]
fn canonical_twenty_agent_survival_report_is_stable() {
    let config = ScenarioConfig::canonical(20);
    let first = ScenarioRunner::new(config).unwrap().run(10_000).unwrap();
    let second = ScenarioRunner::new(config).unwrap().run(10_000).unwrap();
    eprintln!("{first:#?}");
    assert_eq!(first, second);
    assert!(first.living_agents > 0);
    assert!(first.actions.gathers > 0);
    assert!(first.actions.eats > 0);
    assert!(first.actions.drinks > 0);
    assert!(first.actions.sleep_starts > 0);
    assert!(first.actions.shelter_completions > 0);
    assert!(first.deaths.dehydration > 0);
    assert_eq!(first.soak.invariant_violations, 0);
}

#[test]
#[ignore = "canonical 600,000-tick release soak"]
fn canonical_one_hundred_agent_soak_is_bounded_and_repeatable() {
    let config = ScenarioConfig::canonical(100);
    let first = ScenarioRunner::new(config).unwrap().run(10_000).unwrap();
    let second = ScenarioRunner::new(config).unwrap().run(10_000).unwrap();
    eprintln!("{first:#?}");
    assert_eq!(first, second);
    assert_eq!(first.soak.invariant_violations, 0);
    assert_eq!(first.capacity.agent_records, 100);
    assert!(first.work.peak_scheduled_events < 2_000);
    assert!(first.work.peak_policy_retry_depth < 128);
    assert!(first.work.events_processed <= first.work.events_scheduled);
    assert!(first.work.route_expansions <= first.work.route_plans * 256);
}
