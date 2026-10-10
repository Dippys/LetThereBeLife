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

/// In a famine valley (5% of the food, nothing grows back, no game to hunt),
/// being able to ask for food keeps more of the band alive through the first
/// starvation wave: one more survivor at 600k ticks on each of seeds 7, 10, and
/// 12 (D-077, D-079). With regrowth or wildlife there's no famine, and nobody asks.
#[test]
#[ignore = "release-only: run with --release --ignored (part of scripts/validate)"]
fn asking_for_food_carries_more_of_a_famine_valley_through_the_first_starvation_wave() {
    let run = |seed: u64, helping: bool| {
        let mut config = valley(seed, 600_000);
        config.food_percent = 5;
        config.wildlife = false;
        config.regrowth = false;
        config.mind = PolicyOptions {
            helping,
            ..PolicyOptions::full()
        };
        run_study(config).expect("the seed has a valley")
    };
    let (mut helped, mut alone, mut gifts) = (0, 0, 0);
    for seed in [7, 10, 12] {
        let with_help = run(seed, true);
        let without = run(seed, false);
        assert_eq!(without.comms.summary().requests[0], 0);
        gifts += with_help.comms.summary().requests[2];
        helped += with_help.survivors;
        alone += without.survivors;
    }
    assert!(gifts > 0, "food changed hands");
    assert!(
        helped > alone,
        "survivors with helping {helped} vs without {alone}"
    );
}
