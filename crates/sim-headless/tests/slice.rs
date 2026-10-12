//! The rest of the vertical slice (plan M7), release-only:
//! `cargo test --release -p sim-headless --test slice -- --ignored` (part of scripts/validate).

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
/// people ask each other for food and food changes hands. Whether that keeps more of the band on its feet is no longer
/// asserted: since storage and huts (D-102) these valleys are mild and the
/// few gifts change survival by 1-3 people either way (D-110, docs/STATUS.md).
#[test]
#[ignore = "release-only: run with --release --ignored (part of scripts/validate)"]
fn people_in_famine_valleys_ask_for_food_and_get_some() {
    let results = across_valleys(1..=40, |seed| {
        let mut config = valley(seed, 600_000);
        config.food_percent = 5;
        config.wildlife = false;
        config.regrowth = false;
        Some(run_study(config).ok()?.comms.summary().requests[2])
    });
    assert_eq!(
        results.len(),
        20,
        "the livable valleys among the seeds changed"
    );
    let gifts: u64 = results.iter().sum();
    assert!(gifts > 0, "food changed hands");
}

/// People settle in a hut they come back to rather than building wherever
/// they get cold (D-109): over two years most have a home, and the huts that
/// went up are a few per person, not dozens.
#[test]
#[ignore = "release-only: run with --release --ignored (part of scripts/validate)"]
fn people_keep_homes_instead_of_building_wherever_they_are() {
    let results = across_valleys(1..=16, |seed| {
        let report = run_study(valley(seed, 2 * sim_core::TICKS_PER_YEAR)).ok()?;
        Some((
            report.wildlife.huts,
            report.wildlife.housed,
            report.survivors,
        ))
    });
    assert_eq!(results.len(), 10, "the valleys among the seeds changed");
    let built: u64 = results.iter().map(|&(huts, _, _)| huts).sum();
    let housed: u64 = results.iter().map(|&(_, housed, _)| housed).sum();
    let living: u64 = results
        .iter()
        .map(|&(_, _, survivors)| survivors as u64)
        .sum();
    assert!(
        housed * 2 >= living,
        "only {housed} of {living} people have a home"
    );
    assert!(
        built <= 2 * living,
        "{built} huts went up for {living} people"
    );
}

/// Talking brings the band's words together: averaged over every valley among
/// seeds 1-16, agreement on each place word is higher after 80,000 ticks than
/// at the start (one valley alone can dip by a point, D-112).
#[test]
#[ignore = "release-only: run with --release --ignored (part of scripts/validate)"]
fn talking_brings_the_band_s_words_together() {
    let results = across_valleys(1..=16, |seed| {
        Some(run_study(valley(seed, 80_000)).ok()?.vocabulary_agreement)
    });
    assert_eq!(results.len(), 10, "the valleys among the seeds changed");
    let start: u64 = results.iter().map(|[start, _]| start).sum();
    let end: u64 = results.iter().map(|[_, end]| end).sum();
    assert!(
        end > start,
        "words diverge: summed agreement {start}% -> {end}%"
    );
}
