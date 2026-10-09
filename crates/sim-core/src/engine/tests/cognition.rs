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
    };
    engine
        .minds
        .get_mut(agent)
        .map
        .observe(agent.get(), place, &perception, 0, &mut |_, _| {});
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
    assert_eq!(events[0].topic, GestureTopic::Place(LandmarkKind::Water));

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
    assert_eq!(
        stranger_hint,
        crate::cognition::told_confidence(crate::DEFAULT_TRUST)
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
        .observe(0, far, &far_view, 0, &mut |_, _| {});
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
    assert_eq!(engine.signal_events()[0].topic, GestureTopic::Explored);
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
