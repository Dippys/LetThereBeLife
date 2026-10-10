use super::*;
use crate::{
    Concept, GestureTopic, LessonCause, PHYSICAL_POLICY_RADIUS, PolicyOptions, PolicyReason,
    Species, cognition::Fauna,
};

/// Speaker (agent 0) has been bitten by wolves; the listener (agent 1) hunts
/// deer and has never met a wolf. A wolf stands a few cells from the speaker.
fn wolf_in_sight() -> (Engine, WorldPosition) {
    let mut engine = resident_engine(64);
    let (speaker, listener) = standable_shelter_site(&engine);
    let area = engine.world().initial_bounds();
    engine
        .initialize_population(
            PopulationInit {
                active_area: area,
                population: 2,
            },
            &[speaker, listener],
        )
        .unwrap();
    engine.policy_options = PolicyOptions::full();
    engine.wildlife.area = Some(area);
    let wolf = (11..16)
        .flat_map(|distance| {
            [(distance, 0), (-distance, 0), (0, distance), (0, -distance)].map(|(dx, dy)| {
                WorldPosition {
                    x: speaker.x + dx,
                    y: speaker.y + dy,
                }
            })
        })
        .find(|cell| engine.animal_can_stand(*cell))
        .expect("room for a wolf");
    assert!(engine.spawn_animal(Species::Wolf, wolf));
    engine
        .minds
        .get_mut(AgentId::new(0))
        .fauna
        .bitten(Species::Wolf);
    let listener = &mut engine.minds.get_mut(AgentId::new(1)).fauna;
    *listener = Fauna::default();
    listener.saw_hunted(Species::Deer);
    (engine, wolf)
}

fn decide(engine: &mut Engine, agent: AgentId) -> crate::policy::PolicySelection {
    let view = engine.population.view(agent).unwrap();
    let needs = engine.population.needs_view(agent, engine.time).unwrap();
    let inventory = engine.population.inventory(agent).unwrap();
    let perception = engine
        .perceive_physical(agent, PHYSICAL_POLICY_RADIUS)
        .unwrap();
    engine
        .deliberate_with_memory(agent, view.position, needs, inventory, &perception)
        .0
}

fn word(engine: &mut Engine, agent: u32, concept: Concept) -> crate::VocalForm {
    engine
        .minds
        .get_mut(AgentId::new(agent))
        .lexicon
        .produce(concept)
        .expect("founders have a word")
}

#[test]
fn a_warning_makes_a_listener_who_knows_the_word_run() {
    let (mut engine, wolf) = wolf_in_sight();
    let choice = decide(&mut engine, AgentId::new(0));
    assert_eq!(choice.reason, PolicyReason::Warning);
    assert_eq!(choice.target, Some(wolf));
    // The listener takes the speaker's wolf word to mean wolf.
    let form = word(&mut engine, 0, Concept::Wolf);
    engine
        .minds
        .get_mut(AgentId::new(1))
        .lexicon
        .reinforce(form, Concept::Wolf, 60);
    engine.apply_signal(AgentId::new(0), wolf).unwrap();

    let signal = engine.signal_events()[0];
    assert_eq!(signal.intent.topic, GestureTopic::Animal(Species::Wolf));
    assert_eq!(signal.signal.mime, crate::Mime::Snarl);
    assert!(signal.signal.loud);
    let reading = engine.interpretation_events()[0];
    assert_eq!(reading.understood, GestureTopic::Animal(Species::Wolf));
    let listener = engine.minds.get_mut(AgentId::new(1));
    assert!(
        listener.fauna.dangerous(Species::Wolf),
        "the snarl taught it"
    );
    assert!(listener.dialogue.alarm.is_some());
    assert_eq!(
        decide(&mut engine, AgentId::new(1)).reason,
        PolicyReason::Fleeing
    );
    assert_eq!(engine.lead_events().len(), 1, "it acted on the warning");
}

#[test]
fn a_wolf_misread_as_deer_is_hunted_found_out_and_corrected() {
    let (mut engine, wolf) = wolf_in_sight();
    assert_eq!(
        decide(&mut engine, AgentId::new(0)).reason,
        PolicyReason::Warning
    );
    // In the listener's dialect the speaker's word for wolf means deer.
    let form = word(&mut engine, 0, Concept::Wolf);
    engine
        .minds
        .get_mut(AgentId::new(1))
        .lexicon
        .reinforce(form, Concept::Deer, 60);
    engine.apply_signal(AgentId::new(0), wolf).unwrap();

    // 1. A believable misunderstanding: the word outweighed the snarl.
    let reading = engine.interpretation_events()[0];
    assert_eq!(reading.understood, GestureTopic::Animal(Species::Deer));
    assert!(reading.reading.reasons.word_disagrees);
    // 2. Acted on: it goes after the "deer".
    assert!(
        engine
            .minds
            .get_mut(AgentId::new(1))
            .dialogue
            .quarry
            .is_some()
    );
    let choice = decide(&mut engine, AgentId::new(1));
    assert_eq!(choice.reason, PolicyReason::Hunting);
    assert_eq!(engine.lead_events().len(), 1);
    // 3. Closing in, it gets near enough to see a wolf where it expected a deer:
    // the word must mean wolf.
    let listener_at = engine.population.view(AgentId::new(1)).unwrap().position;
    let place = engine
        .minds
        .get_mut(AgentId::new(1))
        .dialogue
        .quarry
        .unwrap()
        .place();
    let step = |from: i64, to: i64| from + (to - from).clamp(-6, 6);
    let toward = WorldPosition {
        x: step(listener_at.x, place.x),
        y: step(listener_at.y, place.y),
    };
    let _ = wolf;
    engine.wildlife.animals[0].x = toward.x as i16;
    engine.wildlife.animals[0].y = toward.y as i16;
    decide(&mut engine, AgentId::new(1));
    let lesson = engine
        .lesson_events()
        .iter()
        .find(|lesson| lesson.agent == AgentId::new(1) && lesson.cause == LessonCause::Consequence)
        .copied()
        .expect("it learned from what it found");
    assert_eq!(lesson.form, form);
    assert_eq!(lesson.strengthened, Some(Concept::Wolf));
    assert_eq!(lesson.weakened, Some(Concept::Deer));
    // 4. It tells the speaker "not deer, wolf"; the speaker counts its word as misheard.
    let correction = engine
        .minds
        .get_mut(AgentId::new(1))
        .dialogue
        .correction(crate::cognition::belief_seconds(engine.time))
        .expect("a correction is planned");
    let from = engine.population.view(AgentId::new(1)).unwrap().position;
    engine
        .apply_correction(AgentId::new(1), from, correction)
        .unwrap();
    let speaker_lesson = engine
        .lesson_events()
        .iter()
        .find(|lesson| lesson.agent == AgentId::new(0) && lesson.cause == LessonCause::Correction)
        .copied()
        .expect("the speaker learned from the correction");
    assert_eq!(speaker_lesson.form, form);
    assert_eq!(speaker_lesson.use_worked, Some(false));
}
