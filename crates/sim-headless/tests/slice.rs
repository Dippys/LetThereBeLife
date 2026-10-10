//! The rest of the vertical slice (plan M7), release-only:
//! `cargo test --release -p sim-headless --test slice -- --ignored` (part of scripts/validate).

use sim_core::PolicyOptions;
use sim_headless::{StudyConfig, StudySpawn, VALLEY_POPULATION, run_study};

fn valley(seed: u64, ticks: u64) -> StudyConfig {
    let mut config = StudyConfig::new(seed, VALLEY_POPULATION, ticks);
    config.spawn = StudySpawn::Valley;
    config
}

/// Children start with no words and learn only from what they observe. One
/// valley's 4 children swing this by tens of points, so it averages every
/// valley among seeds 1-44 (21; mean 75% when this was written, D-095).
#[test]
#[ignore = "release-only: run with --release --ignored (part of scripts/validate)"]
fn children_pick_up_most_of_the_band_s_words_from_observation() {
    let results = across_valleys(1..=44, |seed| {
        run_study(valley(seed, 600_000)).ok()?.children_vocabulary
    });
    assert_eq!(results.len(), 21, "the valleys among the seeds changed");
    assert!(
        results.iter().all(|&(children, _)| children == 4),
        "every valley's founders have 4 children"
    );
    let mean = results.iter().map(|&(_, matching)| matching).sum::<u64>() / results.len() as u64;
    assert!(
        mean >= 60,
        "children say the band's word for only {mean}% of places on average"
    );
}

/// Runs `study` for each seed on all cores, keeping the valleys it returns.
fn across_valleys<T: Send>(
    seeds: std::ops::RangeInclusive<u64>,
    study: impl Fn(u64) -> Option<T> + Sync,
) -> Vec<T> {
    let seeds: Vec<u64> = seeds.collect();
    let threads = std::thread::available_parallelism().map_or(4, usize::from);
    let study = &study;
    std::thread::scope(|scope| {
        let workers: Vec<_> = seeds
            .chunks(seeds.len().div_ceil(threads))
            .map(|chunk| {
                scope.spawn(move || {
                    chunk
                        .iter()
                        .filter_map(|&seed| study(seed))
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        workers
            .into_iter()
            .flat_map(|worker| worker.join().expect("study thread"))
            .collect()
    })
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
    let results = across_valleys(1..=40, |seed| {
        let with_help = run(seed, true)?;
        let without = run(seed, false)?;
        Some((
            with_help.survivors - with_help.collapsed,
            without.survivors - without.collapsed,
            with_help.comms.summary().requests[2],
            without.comms.summary().requests[0],
        ))
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
