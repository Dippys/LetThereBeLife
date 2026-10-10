//! The project's definition of success (InitialDocumentation/16_IMPLEMENTATION_ROADMAP.md):
//!
//! > An agent misunderstands a signal for a believable reason, acts on that
//! > misunderstanding, and both participants update future behavior using only
//! > observable evidence.
//!
//! Complete episodes are rare per run (about one in seven valleys) and any
//! change to behavior reshuffles which runs have them, so this does not pin one.
//! It runs the standard band (two families, with children) for 1.2M ticks in
//! every livable valley among seeds 1-78 (40 valleys, in parallel) and requires
//! at least `MIN_EPISODES` complete episodes in total, each traced through
//! gesture ids by the communication log: the listener must act on its reading
//! before it learns better, and the speaker's lesson must concern the meaning
//! at stake. When this was written the valleys held 7 (DECISIONS D-082).
//! Release-only: `cargo test --release -p sim-headless --test success -- --ignored`.

use sim_core::{DesiredEffect, LessonCause};
use sim_headless::{StudyConfig, StudyReport, StudySpawn, SuccessEpisode, run_study};

/// Seeds scanned for valleys (40 of them are livable).
const SEEDS: std::ops::RangeInclusive<u64> = 1..=78;
/// Well under the 7 measured, so ordinary behavior changes don't break it.
const MIN_EPISODES: usize = 2;
const TICKS: u64 = 1_200_000;

#[test]
#[ignore = "release-only: run with --release --ignored (part of scripts/validate)"]
fn agents_misunderstand_act_and_both_sides_learn_from_what_they_observe() {
    let seeds: Vec<u64> = SEEDS.collect();
    let threads = std::thread::available_parallelism().map_or(4, usize::from);
    let reports: Vec<StudyReport> = std::thread::scope(|scope| {
        let workers: Vec<_> = seeds
            .chunks(seeds.len().div_ceil(threads))
            .map(|chunk| {
                scope.spawn(move || {
                    chunk
                        .iter()
                        .filter_map(|&seed| {
                            let mut config =
                                StudyConfig::new(seed, sim_headless::VALLEY_POPULATION, TICKS);
                            config.spawn = StudySpawn::Valley;
                            run_study(config).ok()
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
        reports.len(),
        40,
        "the livable valleys among the seeds changed"
    );

    let mut total = 0;
    for report in &reports {
        let episodes = report.comms.success_episodes();
        total += episodes.len();
        for episode in &episodes {
            check(report, *episode);
        }
        if let Some(first) = episodes.first() {
            println!(
                "seed {}: {} episode(s); first:\n{}",
                report.config.seed,
                episodes.len(),
                report.comms.describe_episode(*first)
            );
        }
    }
    assert!(
        total >= MIN_EPISODES,
        "only {total} complete misunderstanding episodes emerged in {} valleys",
        reports.len()
    );
}

fn check(report: &StudyReport, episode: SuccessEpisode) {
    let exchange = &report.comms.exchanges()[episode.exchange];
    let reception = exchange
        .receptions
        .iter()
        .find(|reception| reception.interpretation.receiver == episode.listener)
        .unwrap();
    let reading = reception.interpretation;
    // 1. A misunderstanding: the listener's reading differs from the private intent.
    assert_eq!(exchange.signal.intent.effect, DesiredEffect::Inform);
    assert_ne!(reading.understood, exchange.signal.intent.topic);
    // ...for a believable, recorded reason.
    let reasons = reading.reading.reasons;
    assert!(
        reasons.ambiguous_mime
            || reasons.unknown_word
            || reasons.word_disagrees
            || reasons.need_bias
            || reasons.memory_bias
    );
    // 2. The listener acted on it.
    assert!(episode.acted_at >= exchange.signal.at.ticks());
    // 3. Both updated from something they observed, in that order.
    assert_eq!(episode.listener_lesson.agent, episode.listener);
    assert_eq!(episode.listener_lesson.cause, LessonCause::Consequence);
    assert_eq!(episode.speaker_lesson.agent, exchange.signal.signal.sender);
    assert!(matches!(
        episode.speaker_lesson.cause,
        LessonCause::Correction | LessonCause::Usage | LessonCause::Consequence
    ));
    assert!(episode.speaker_lesson.at >= episode.listener_lesson.at);
    assert!(!report.comms.describe_episode(episode).is_empty());
}
