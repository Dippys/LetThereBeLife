//! The project's definition of success (InitialDocumentation/16_IMPLEMENTATION_ROADMAP.md):
//!
//! > An agent misunderstands a signal for a believable reason, acts on that
//! > misunderstanding, and both participants update future behavior using only
//! > observable evidence.
//!
//! This runs the spec's first slice, two families of 8 adults in the seed 1
//! valley (without the M7 children, whose readiness to ask repairs most
//! misunderstandings before they're acted on; see DECISIONS D-077), and requires
//! at least one complete episode, traced through gesture ids by the
//! communication log. Release-only (about 7 s):
//! `cargo test --release -p sim-headless --test success -- --ignored`.

use sim_core::{DesiredEffect, LessonCause};
use sim_headless::{StudyConfig, StudySpawn, run_study};

#[test]
#[ignore = "release-only: run with --release --ignored (part of scripts/validate)"]
fn an_agent_misunderstands_acts_and_both_sides_learn_from_what_they_observe() {
    let mut config = StudyConfig::new(1, sim_core::VALLEY_BAND as u32, 1_200_000);
    config.spawn = StudySpawn::Valley;
    let report = run_study(config).expect("seed 1 has a valley");
    let episodes = report.comms.success_episodes();
    assert!(
        !episodes.is_empty(),
        "no complete misunderstanding episode emerged"
    );

    for episode in &episodes {
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
            LessonCause::Correction | LessonCause::Usage
        ));
        assert!(episode.speaker_lesson.at >= episode.listener_lesson.at);
        assert!(!report.comms.describe_episode(*episode).is_empty());
    }
    println!("{}", report.comms.describe_episode(episodes[0]));
}
