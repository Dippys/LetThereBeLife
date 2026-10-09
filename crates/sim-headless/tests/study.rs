//! The behavior study is a measurement tool, but two properties are guarded:
//! it is deterministic, and agents with memory outlive mindless ones in the
//! default seed's savanna.

use sim_core::PolicyOptions;
use sim_headless::{StudyConfig, StudySpawn, run_study};

fn study(mind: PolicyOptions) -> sim_headless::StudyReport {
    let mut config = StudyConfig::new(1, 10, 150_000);
    config.spawn = StudySpawn::Groups;
    config.mind = mind;
    run_study(config).expect("seed 1 has land for every group")
}

#[test]
fn studies_are_deterministic() {
    assert_eq!(study(PolicyOptions::full()), study(PolicyOptions::full()));
}

#[test]
fn memory_outlives_reactive_agents() {
    let legacy = study(PolicyOptions {
        exploration: true,
        ..PolicyOptions::default()
    });
    let full = study(PolicyOptions::full());
    assert!(
        full.survivors > legacy.survivors,
        "memory {} vs legacy {} survivors",
        full.survivors,
        legacy.survivors
    );
    assert!(full.mean_known_places > 0);
}
