use super::*;
use crate::{
    GestureTopic, LandmarkKind, LandmarkSource, PerceivedWater, PhysicalPerception, PolicyOptions,
};

fn two_neighbours() -> (Engine, WorldPosition, WorldPosition) {
    let mut engine = resident_engine(64);
    let (sender, watcher) = standable_shelter_site(&engine);
    let area = engine.world().initial_bounds();
    engine
        .initialize_population(
            PopulationInit {
                active_area: area,
                population: 2,
            },
            &[sender, watcher],
        )
        .unwrap();
    (engine, sender, watcher)
}

/// Gives `agent` a first-hand memory of water at `place` without walking there.
fn remember_water(engine: &mut Engine, agent: AgentId, place: WorldPosition) {
    let perception = PhysicalPerception {
        area: WorldRect {
            min: WorldPosition {
                x: place.x - 8,
                y: place.y - 8,
            },
            max: WorldPosition {
                x: place.x + 9,
                y: place.y + 9,
            },
        },
        agents: Vec::new(),
        claimed_targets: Vec::new(),
        drinkable_water: vec![PerceivedWater {
            position: place,
            source: WaterSource::Lake,
        }],
        resources: Vec::new(),
        structures: Vec::new(),
        traversable_cells: Vec::new(),
        reachable_cells: Vec::new(),
        reserved_cells: Vec::new(),
        spent_resources: Vec::new(),
        animals: Vec::new(),
    };
    engine
        .minds
        .get_mut(agent)
        .map
        .observe(agent.get(), place, &perception, 0, &mut |_| {});
}

#[test]
fn a_gesture_gives_watchers_a_rough_hint_not_the_exact_place() {
    let (mut engine, sender, _) = two_neighbours();
    let lake = WorldPosition {
        x: sender.x + 150,
        y: sender.y - 40,
    };
    remember_water(&mut engine, AgentId::new(0), lake);
    engine.apply_signal(AgentId::new(0), lake).unwrap();

    let events = engine.signal_events();
    assert_eq!(events.len(), 1);
    assert_eq!((events[0].watchers, events[0].informed), (1, 1));
    assert_eq!(
        events[0].intent.place, lake,
        "the log keeps the private intent"
    );
    let readings = engine.interpretation_events();
    assert_eq!(readings.len(), 1, "one reading per watcher");
    assert_eq!(readings[0].signal, events[0].id);
    assert_eq!(readings[0].receiver, AgentId::new(1));
    assert!(readings[0].changed);
    assert_eq!(
        events[0].intent.topic,
        GestureTopic::Place(LandmarkKind::Water)
    );
    assert_eq!(events[0].signal.mime, crate::Mime::Scoop);

    let watcher = engine.mental_map(AgentId::new(1)).unwrap();
    assert_eq!(watcher.landmarks.len(), 1);
    let hint = watcher.landmarks[0];
    assert_eq!(
        (hint.kind, hint.source),
        (LandmarkKind::Water, LandmarkSource::Told)
    );
    assert!(hint.confidence < 255 && hint.search_radius > 0);
    let error = hint
        .position
        .x
        .abs_diff(lake.x)
        .max(hint.position.y.abs_diff(lake.y));
    assert!(
        error <= u64::from(hint.search_radius),
        "the real lake lies inside the search area"
    );
}

#[test]
fn sleeping_agents_do_not_see_gestures() {
    let (mut engine, sender, watcher) = two_neighbours();
    let lake = WorldPosition {
        x: sender.x + 60,
        y: sender.y,
    };
    remember_water(&mut engine, AgentId::new(0), lake);
    engine
        .request_sleep(AgentId::new(1), watcher)
        .expect("watcher can sleep where it stands");
    engine.apply_signal(AgentId::new(0), lake).unwrap();
    assert_eq!(engine.signal_events()[0].watchers, 0);
    assert!(
        engine
            .mental_map(AgentId::new(1))
            .is_none_or(|map| map.landmarks.is_empty())
    );
}

#[test]
fn agents_cannot_point_at_places_they_do_not_remember() {
    let (mut engine, sender, _) = two_neighbours();
    let nowhere = WorldPosition {
        x: sender.x + 50,
        y: sender.y,
    };
    assert_eq!(
        engine.apply_signal(AgentId::new(0), nowhere),
        Err(PolicyFailureReason::TargetUnavailable)
    );
    assert!(engine.signal_events().is_empty());
}

fn social_neighbours() -> (Engine, WorldPosition) {
    let (mut engine, sender, _) = two_neighbours();
    engine.policy_options = PolicyOptions::full();
    (engine, sender)
}

#[test]
fn watchers_remember_who_pointed_and_weigh_hints_by_trust() {
    let (mut engine, sender) = social_neighbours();
    let lake = WorldPosition {
        x: sender.x + 120,
        y: sender.y,
    };
    remember_water(&mut engine, AgentId::new(0), lake);
    engine.apply_signal(AgentId::new(0), lake).unwrap();
    let watcher = engine.mental_map(AgentId::new(1)).unwrap();
    assert_eq!(watcher.acquaintances.len(), 1);
    assert_eq!(watcher.acquaintances[0].agent, AgentId::new(0));
    let stranger_hint = watcher.landmarks[0].confidence;
    let reading = engine.interpretation_events()[0];
    assert_eq!(stranger_hint, reading.confidence);
    // Confidence = trust in a stranger, scaled by how sure the reading was.
    let (_, probability) = reading.reading.best();
    assert_eq!(
        u16::from(stranger_hint),
        u16::from(crate::cognition::told_confidence(crate::DEFAULT_TRUST)) * u16::from(probability)
            / 255
    );

    // A distrusted teller's hint about another place carries less weight.
    let mind = engine.minds.get_mut(AgentId::new(1));
    let slot = mind.social.slot_of(AgentId::new(0)).unwrap();
    for _ in 0..3 {
        mind.social.hint_checked(slot, false);
    }
    let pond = WorldPosition {
        x: sender.x - 120,
        y: sender.y,
    };
    remember_water(&mut engine, AgentId::new(0), pond);
    engine.apply_signal(AgentId::new(0), pond).unwrap();
    let doubted = engine
        .mental_map(AgentId::new(1))
        .unwrap()
        .landmarks
        .into_iter()
        .find(|place| place.position.x < sender.x)
        .expect("second hint stored");
    assert!(doubted.confidence < stranger_hint);
}

#[test]
fn explored_gestures_mark_ground_for_watchers() {
    let (mut engine, sender) = social_neighbours();
    let far = WorldPosition {
        x: sender.x + 300,
        y: sender.y + 300,
    };
    let perception = engine.perceive_physical(AgentId::new(0), 8).unwrap();
    let mut far_view = perception.clone();
    far_view.area = WorldRect {
        min: WorldPosition {
            x: far.x - 8,
            y: far.y - 8,
        },
        max: WorldPosition {
            x: far.x + 9,
            y: far.y + 9,
        },
    };
    far_view.agents.clear();
    engine
        .minds
        .get_mut(AgentId::new(0))
        .map
        .observe(0, far, &far_view, 0, &mut |_| {});
    let (marker, _) = engine
        .minds
        .get(AgentId::new(0))
        .unwrap()
        .map
        .shareable(perception.area)
        .expect("far explored tile is shareable");
    let before = engine
        .mental_map(AgentId::new(1))
        .map_or(0, |map| map.explored_tiles);
    engine.apply_signal(AgentId::new(0), marker).unwrap();
    assert_eq!(
        engine.signal_events()[0].intent.topic,
        GestureTopic::Explored
    );
    assert_eq!(
        engine.mental_map(AgentId::new(1)).unwrap().explored_tiles,
        before + 1
    );
}

#[test]
fn personalities_are_stable_per_agent_and_average_without_social() {
    let (mut engine, _, _) = two_neighbours();
    let innate = engine.personality(AgentId::new(0)).unwrap();
    assert_eq!(
        innate,
        crate::Personality::of(engine.config.seed, AgentId::new(0))
    );
    engine.policy_options = PolicyOptions {
        social: false,
        ..PolicyOptions::full()
    };
    remember_water(
        &mut engine,
        AgentId::new(0),
        WorldPosition { x: 500, y: 500 },
    );
    assert_eq!(
        engine.mental_map(AgentId::new(0)).unwrap().personality,
        crate::Personality::AVERAGE
    );
}

#[test]
fn receivers_depend_only_on_the_public_signal() {
    // World A: the sender really remembers a lake and points at it.
    let (mut informed_world, sender, _) = two_neighbours();
    let lake = WorldPosition {
        x: sender.x + 90,
        y: sender.y + 20,
    };
    remember_water(&mut informed_world, AgentId::new(0), lake);
    informed_world.apply_signal(AgentId::new(0), lake).unwrap();
    let public = informed_world.signal_events()[0].signal;

    // World B: the sender knows nothing; the same public signal is delivered.
    let (mut blank_world, _, _) = two_neighbours();
    let delivery = blank_world.deliver(0, &public).unwrap();
    assert_eq!(delivery.watchers, 1);

    assert_eq!(
        informed_world.mental_map(AgentId::new(1)),
        blank_world.mental_map(AgentId::new(1)),
        "identical public signals must produce identical beliefs, whatever the sender meant"
    );
    assert_eq!(
        informed_world.interpretation_events(),
        blank_world.interpretation_events()
    );
}

#[test]
fn the_sender_looks_as_urgent_as_its_needs() {
    let (mut engine, sender, _) = two_neighbours();
    let lake = WorldPosition {
        x: sender.x + 40,
        y: sender.y,
    };
    remember_water(&mut engine, AgentId::new(0), lake);
    engine.apply_signal(AgentId::new(0), lake).unwrap();
    let calm = engine.signal_events()[0].signal.tone.urgency;
    engine.population.set_need_value_for_test(
        AgentId::new(0),
        crate::NeedKind::Thirst,
        6_000,
        engine.time,
    );
    engine.apply_signal(AgentId::new(0), lake).unwrap();
    let thirsty = engine.signal_events()[1].signal.tone.urgency;
    assert!(calm < 32, "a fresh agent looks calm ({calm})");
    assert!(
        thirsty >= 128,
        "an agent at its thirst threshold looks urgent ({thirsty})"
    );
}

#[test]
fn watchers_learn_words_from_the_mime_they_come_with() {
    let (mut informed_world, sender, _) = two_neighbours();
    let lake = WorldPosition {
        x: sender.x + 90,
        y: sender.y,
    };
    remember_water(&mut informed_world, AgentId::new(0), lake);
    informed_world.apply_signal(AgentId::new(0), lake).unwrap();
    let mut public = informed_world.signal_events()[0].signal;
    // Pick a word the watcher has no reading for at all.
    let watcher_words: Vec<_> = informed_world
        .mental_map(AgentId::new(1))
        .unwrap()
        .lexicon
        .iter()
        .map(|entry| entry.form)
        .collect();
    let fresh = (0..crate::VOCAL_FORMS)
        .map(crate::VocalForm)
        .find(|form| !watcher_words.contains(form))
        .expect("16 slots can't hold all 32 forms");
    public.vocal = Some(fresh);

    let (mut world, _, _) = two_neighbours();
    world.deliver(0, &public).unwrap();
    let reading = world.interpretation_events()[0];
    assert_eq!(reading.heard, Some(fresh));
    assert_eq!(
        reading.word_reading, None,
        "the word meant nothing to them before"
    );
    let learned = world
        .mental_map(AgentId::new(1))
        .unwrap()
        .lexicon
        .into_iter()
        .find(|entry| entry.form == fresh)
        .expect("the word is now in the lexicon");
    assert_eq!(
        learned.concept,
        crate::Concept::Water,
        "learned from the scooping mime"
    );
}

#[test]
fn founders_inherit_the_same_lexicons_on_replay() {
    let (mut first, sender, _) = two_neighbours();
    let (mut second, _, _) = two_neighbours();
    let lake = WorldPosition {
        x: sender.x + 60,
        y: sender.y,
    };
    remember_water(&mut first, AgentId::new(0), lake);
    remember_water(&mut second, AgentId::new(0), lake);
    let words = |engine: &Engine| engine.mental_map(AgentId::new(0)).unwrap().lexicon;
    assert_eq!(words(&first), words(&second));
    assert!(
        !words(&first).is_empty(),
        "founders start with a proto-language"
    );
}

#[test]
fn a_word_the_listener_holds_differently_causes_a_believable_misreading() {
    let (mut world, sender, _) = two_neighbours();
    let lake = WorldPosition {
        x: sender.x + 70,
        y: sender.y,
    };
    remember_water(&mut world, AgentId::new(0), lake);
    world.apply_signal(AgentId::new(0), lake).unwrap();
    let mut public = world.signal_events()[0].signal;
    assert_eq!(public.mime, crate::Mime::Scoop);
    // The sender says the word that, to this listener, firmly means FOOD.
    let (mut listener_world, _, _) = two_neighbours();
    listener_world.minds.get_mut(AgentId::new(1));
    let food_word = listener_world
        .mental_map(AgentId::new(1))
        .unwrap()
        .lexicon
        .into_iter()
        .filter(|entry| entry.concept == crate::Concept::Berries)
        .max_by_key(|entry| i32::from(entry.positive) - i32::from(entry.contradictory))
        .expect("founders have a word for food")
        .form;
    public.vocal = Some(food_word);
    listener_world.deliver(0, &public).unwrap();
    let reading = listener_world.interpretation_events()[0];
    assert_eq!(
        reading.understood,
        GestureTopic::Place(LandmarkKind::Berries)
    );
    assert!(reading.reading.reasons.ambiguous_mime);
    assert!(reading.reading.reasons.word_disagrees);
    assert_eq!(
        reading.reading.runner_up().map(|(c, _)| c),
        Some(crate::Concept::Water)
    );
}

/// The word this agent firmly links to `concept`.
fn word_for(engine: &Engine, agent: AgentId, concept: crate::Concept) -> crate::VocalForm {
    engine
        .mental_map(agent)
        .unwrap()
        .lexicon
        .into_iter()
        .filter(|entry| entry.concept == concept)
        .max_by_key(|entry| i32::from(entry.positive) - i32::from(entry.contradictory))
        .expect("founders have a word for it")
        .form
}

#[test]
fn an_unsure_listener_asks_and_the_speaker_repairs() {
    let (mut engine, sender, _) = two_neighbours();
    let lake = WorldPosition {
        x: sender.x + 70,
        y: sender.y,
    };
    remember_water(&mut engine, AgentId::new(0), lake);
    engine.minds.get_mut(AgentId::new(1));
    // The speaker says, for water, the word the listener firmly reads as food.
    let confusing = word_for(&engine, AgentId::new(1), crate::Concept::Berries);
    engine
        .minds
        .get_mut(AgentId::new(0))
        .lexicon
        .reinforce(confusing, crate::Concept::Water, 60);
    // A thirsty listener weighs water too, so it ends up unsure (the word says
    // food; the mime and its thirst lean water) and asks.
    engine.population.set_need_value_for_test(
        AgentId::new(1),
        crate::NeedKind::Thirst,
        6_000,
        engine.time,
    );
    engine.apply_signal(AgentId::new(0), lake).unwrap();

    let repair = engine
        .repair_events()
        .first()
        .copied()
        .expect("the listener asked");
    assert_eq!(repair.listener, AgentId::new(1));
    assert_eq!(repair.guess, crate::Concept::Berries);
    assert_eq!(
        repair.response,
        crate::RepairResponse::Repaired(crate::Concept::Water)
    );
    let listener = engine.mental_map(AgentId::new(1)).unwrap();
    assert!(
        listener
            .landmarks
            .iter()
            .any(|place| place.kind == LandmarkKind::Water && place.source == LandmarkSource::Told),
        "after the repair the listener believes there is water there"
    );
    assert!(
        !listener
            .landmarks
            .iter()
            .any(|place| place.kind == LandmarkKind::Berries),
        "and dropped the misread food hint"
    );
    let causes: Vec<_> = engine
        .lesson_events()
        .iter()
        .map(|lesson| lesson.cause)
        .collect();
    assert!(causes.contains(&crate::LessonCause::Repair));
}

#[test]
fn a_correction_teaches_the_speaker_its_word_was_misheard() {
    let (mut engine, sender, _) = two_neighbours();
    let pond = WorldPosition {
        x: sender.x + 50,
        y: sender.y + 10,
    };
    engine.minds.get_mut(AgentId::new(0));
    let form = word_for(&engine, AgentId::new(0), crate::Concept::Water);
    // Agent 1 misread agent 0's word for water as food and found water instead.
    engine
        .minds
        .get_mut(AgentId::new(1))
        .dialogue
        .plan_correction(crate::cognition::PendingCorrection {
            speaker: AgentId::new(0),
            form,
            misread: crate::Concept::Berries,
            actual: crate::Concept::Water,
            place: pond,
            since: 0,
        });
    engine.apply_signal(AgentId::new(1), pond).unwrap();

    let event = engine.signal_events()[0];
    assert_eq!(event.intent.effect, crate::DesiredEffect::Correct);
    assert_eq!(event.signal.vocal, Some(form));
    assert_eq!(event.signal.negated, Some(crate::Mime::PickAndChew));
    let speaker_lesson = engine
        .lesson_events()
        .iter()
        .find(|lesson| lesson.agent == AgentId::new(0))
        .expect("the speaker saw the correction");
    assert_eq!(speaker_lesson.cause, crate::LessonCause::Correction);
    assert_eq!(speaker_lesson.use_worked, Some(false));
    let entry = engine
        .mental_map(AgentId::new(0))
        .unwrap()
        .lexicon
        .into_iter()
        .find(|entry| entry.form == form && entry.concept == crate::Concept::Water)
        .unwrap();
    assert_eq!(entry.failures, 1, "the speaker counts that use as misheard");
}

#[test]
fn a_correction_waits_while_the_speaker_is_away() {
    let (mut engine, sender, _) = two_neighbours();
    let pond = WorldPosition {
        x: sender.x + 50,
        y: sender.y,
    };
    engine
        .minds
        .get_mut(AgentId::new(1))
        .dialogue
        .plan_correction(crate::cognition::PendingCorrection {
            speaker: AgentId::new(7),
            form: crate::VocalForm(3),
            misread: crate::Concept::Berries,
            actual: crate::Concept::Water,
            place: pond,
            since: 0,
        });
    assert_eq!(
        engine.apply_signal(AgentId::new(1), pond),
        Err(PolicyFailureReason::TargetUnavailable)
    );
    assert!(
        engine
            .minds
            .get(AgentId::new(1))
            .unwrap()
            .dialogue
            .correction(1)
            .is_some(),
        "kept for the next meeting"
    );
}
