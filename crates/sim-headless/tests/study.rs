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

#[test]
fn social_minds_keep_company_and_sociability_matters() {
    let run = |mind: PolicyOptions| {
        let mut config = StudyConfig::new(1, 20, 120_000);
        config.spawn = StudySpawn::Groups;
        config.mind = mind;
        run_study(config).expect("seed 1 has land for every group")
    };
    let sharing = run(PolicyOptions {
        social: false,
        ..PolicyOptions::full()
    });
    let social = run(PolicyOptions::full());
    assert!(
        social.company_percent > sharing.company_percent,
        "company {}% vs {}%",
        social.company_percent,
        sharing.company_percent
    );
    let (_, _, loners, sociable) = social
        .trait_effects
        .iter()
        .copied()
        .find(|(name, ..)| *name == "sociability")
        .expect("sociability is measured");
    assert!(
        sociable > loners,
        "sociable {sociable}% vs loners {loners}%"
    );
}

#[test]
fn valley_communication_log_is_consistent() {
    let mut config = StudyConfig::new(1, sim_headless::VALLEY_POPULATION, 80_000);
    config.spawn = StudySpawn::Valley;
    let report = run_study(config).expect("seed 1 has a valley");
    let summary = report.comms.summary();
    assert!(summary.exchanges > 0, "the band talks");
    assert!(summary.informed <= summary.receptions);
    assert!(summary.acted <= summary.informed);
    for exchange in report.comms.exchanges() {
        let signal = exchange.signal;
        let error = signal
            .inferred_position
            .x
            .abs_diff(signal.intent.place.x)
            .max(signal.inferred_position.y.abs_diff(signal.intent.place.y));
        assert!(
            error <= u64::from(signal.search_radius),
            "the real place lies inside the search area watchers infer"
        );
        for reception in &exchange.receptions {
            assert_eq!(reception.interpretation.signal, signal.id);
            assert_ne!(reception.interpretation.receiver, signal.signal.sender);
        }
    }
    assert!(summary.worded[0] + summary.worded[1] > 0, "the band speaks");
    let [start, end] = report.vocabulary_agreement;
    assert!(
        start >= 50,
        "founders mostly share a proto-language ({start}%)"
    );
    assert!(
        end >= start,
        "talking doesn't make words diverge ({start}% -> {end}%)"
    );
    let story = sim_headless::explain(&report, 0);
    assert!(story.starts_with("explain agent 0:"));
}

#[test]
fn valley_misunderstandings_happen_for_recorded_reasons_and_are_acted_on() {
    let mut config = StudyConfig::new(1, sim_headless::VALLEY_POPULATION, 200_000);
    config.spawn = StudySpawn::Valley;
    let report = run_study(config).expect("seed 1 has a valley");
    let summary = report.comms.summary();
    assert!(report.hint_decisions > 0, "agents act on what they're told");
    assert!(
        summary.misread > 0,
        "two family dialects produce misreadings"
    );
    assert!(
        summary.misread * 4 < summary.receptions,
        "but not chaos: {} of {}",
        summary.misread,
        summary.receptions
    );
    let explained = summary.misread_reasons.iter().sum::<u64>();
    assert!(
        explained >= summary.misread,
        "every misreading has a recorded reason"
    );
}

#[test]
fn apart_families_start_in_neighboring_valleys() {
    let mut config = StudyConfig::new(1, sim_headless::VALLEY_POPULATION, 6_000);
    config.spawn = StudySpawn::Apart;
    let report = run_study(config).expect("seed 1 has two neighboring valleys");
    assert!(
        report.families.camp_distance > 1_000,
        "a valley apart: camps {} cells apart",
        report.families.camp_distance
    );
    assert_eq!(report.survivors, sim_headless::VALLEY_POPULATION);
    let mut other = StudyConfig::new(1, 8, 6_000);
    other.spawn = StudySpawn::Apart;
    assert!(run_study(other).is_err(), "only the standard band");
}
