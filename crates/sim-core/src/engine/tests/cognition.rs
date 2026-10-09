use super::*;
use crate::{LandmarkKind, LandmarkSource, PerceivedWater, PhysicalPerception};

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
        .observe(agent.get(), place, &perception, 0);
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
    assert_eq!(events[0].kind, LandmarkKind::Water);

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
