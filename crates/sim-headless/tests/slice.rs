//! The rest of the vertical slice (plan M7), release-only:
//! `cargo test --release -p sim-headless --test slice -- --ignored` (part of scripts/validate).

use sim_core::PolicyOptions;
use sim_headless::{StudyConfig, StudySpawn, VALLEY_POPULATION, run_study};

fn valley(seed: u64, ticks: u64) -> StudyConfig {
    let mut config = StudyConfig::new(seed, VALLEY_POPULATION, ticks);
    config.spawn = StudySpawn::Valley;
    config
}

/// Children start with no words and learn only from what they observe.
#[test]
#[ignore = "release-only: run with --release --ignored (part of scripts/validate)"]
fn children_pick_up_most_of_the_band_s_words_from_observation() {
    let report = run_study(valley(1, 600_000)).expect("seed 1 has a valley");
    let (children, matching) = report.children_vocabulary.expect("the valley has children");
    assert_eq!(children, 4);
    assert!(
        matching >= 60,
        "children say the band's word for only {matching}% of places"
    );
}

/// In a valley stripped to 5% of its food, being able to ask for food keeps
/// more of the band alive through the first starvation wave (seed 7: 14 vs 12
/// survivors at 600k ticks). Food doesn't regrow, so over longer runs helping
/// mostly evens out who starves rather than adding meals (see DECISIONS D-077).
#[test]
#[ignore = "release-only: run with --release --ignored (part of scripts/validate)"]
fn asking_for_food_carries_more_of_a_scarce_valley_through_the_first_famine() {
    let run = |helping: bool| {
        let mut config = valley(7, 600_000);
        config.food_percent = 5;
        config.mind = PolicyOptions {
            helping,
            ..PolicyOptions::full()
        };
        run_study(config).expect("seed 7 has a valley")
    };
    let helped = run(true);
    let alone = run(false);
    let [asked, _, gave, _, _] = helped.comms.summary().requests;
    assert!(asked > 0 && gave > 0, "asked {asked}, gave {gave}");
    assert_eq!(alone.comms.summary().requests[0], 0);
    assert!(
        helped.survivors > alone.survivors,
        "survivors with helping {} vs without {}",
        helped.survivors,
        alone.survivors
    );
}
