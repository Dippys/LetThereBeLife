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

/// In famine valleys (5% of the food, nothing grows back, no game to hunt),
/// being able to ask for food keeps more of the band on its feet. The effect is
/// small and comes from the few valleys where people actually ask (D-077,
/// D-079, D-090), so this runs every livable valley among seeds 1-40 (20, in
/// parallel) with and without helping and compares the people standing at the
/// end (alive and not collapsed): 365 against 361 when this was written.
#[test]
#[ignore = "release-only: run with --release --ignored (part of scripts/validate)"]
fn asking_for_food_keeps_more_of_famine_valleys_on_their_feet() {
    let run = |seed: u64, helping: bool| {
        let mut config = valley(seed, 600_000);
        config.food_percent = 5;
        config.wildlife = false;
        config.regrowth = false;
        config.mind = PolicyOptions {
            helping,
            ..PolicyOptions::full()
        };
        run_study(config).ok()
    };
    let seeds: Vec<u64> = (1..=40).collect();
    let threads = std::thread::available_parallelism().map_or(4, usize::from);
    let results: Vec<(u32, u32, u64, u64)> = std::thread::scope(|scope| {
        let workers: Vec<_> = seeds
            .chunks(seeds.len().div_ceil(threads))
            .map(|chunk| {
                scope.spawn(move || {
                    chunk
                        .iter()
                        .filter_map(|&seed| {
                            let with_help = run(seed, true)?;
                            let without = run(seed, false)?;
                            Some((
                                with_help.survivors - with_help.collapsed,
                                without.survivors - without.collapsed,
                                with_help.comms.summary().requests[2],
                                without.comms.summary().requests[0],
                            ))
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        workers
            .into_iter()
            .flat_map(|worker| worker.join().expect("study thread"))
            .collect()
    });
    assert_eq!(
        results.len(),
        20,
        "the livable valleys among the seeds changed"
    );
    let helped: u32 = results.iter().map(|result| result.0).sum();
    let alone: u32 = results.iter().map(|result| result.1).sum();
    let gifts: u64 = results.iter().map(|result| result.2).sum();
    let asked_alone: u64 = results.iter().map(|result| result.3).sum();
    assert_eq!(asked_alone, 0, "without helping nobody asks");
    assert!(gifts > 0, "food changed hands");
    assert!(
        helped > alone,
        "people standing with helping {helped} vs without {alone}"
    );
}
