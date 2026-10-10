use super::map::{HintSource, Landmark};
use super::*;
use crate::{
    BaseResource, Material, PerceivedResource, PerceivedWater, PhysicalPerception, WaterSource,
    WorldRect, policy::ExplorationHeading,
};

fn at(x: i64, y: i64) -> WorldPosition {
    WorldPosition { x, y }
}

fn view_around(center: WorldPosition) -> PhysicalPerception {
    PhysicalPerception {
        area: WorldRect {
            min: at(center.x - 8, center.y - 8),
            max: at(center.x + 9, center.y + 9),
        },
        agents: Vec::new(),
        claimed_targets: Vec::new(),
        drinkable_water: Vec::new(),
        resources: Vec::new(),
        structures: Vec::new(),
        traversable_cells: Vec::new(),
        reachable_cells: Vec::new(),
        reserved_cells: Vec::new(),
        spent_resources: Vec::new(),
        animals: Vec::new(),
    }
}

fn with_water(mut perception: PhysicalPerception, cells: &[WorldPosition]) -> PhysicalPerception {
    perception.drinkable_water = cells
        .iter()
        .map(|&position| PerceivedWater {
            position,
            source: WaterSource::Lake,
        })
        .collect();
    perception
}

fn with_food(mut perception: PhysicalPerception, cells: &[WorldPosition]) -> PhysicalPerception {
    perception.resources = cells
        .iter()
        .map(|&position| PerceivedResource {
            position,
            resource: BaseResource {
                capacity: 12,
                kind: Material::Berries,
            },
        })
        .collect();
    perception
}

fn observe(
    map: &mut MentalMap,
    agent: u32,
    origin: WorldPosition,
    perception: &PhysicalPerception,
    now: u32,
) {
    map.observe(agent, origin, perception, now, &mut |_| {});
}

fn landmarks(map: &MentalMap) -> Vec<LandmarkView> {
    map.views().collect()
}

#[test]
fn mental_map_layout_is_compact() {
    assert_eq!(size_of::<Landmark>(), 16);
    assert_eq!(
        size_of::<MentalMap>(),
        16 * LANDMARK_SLOTS + 4 * VISITED_TILE_SLOTS + 20
    );
}

#[test]
fn mind_layout_is_bounded() {
    assert_eq!(size_of::<super::PendingCorrection>(), 32);
    assert_eq!(size_of::<super::Dialogue>(), 112);
    // Map 372, social 96, lexicon 192, dialogue 112, beliefs 15 + 6 + 2,
    // child flag, parent id, grief timer, and padding.
    assert_eq!(size_of::<MentalMap>(), 372);
    assert_eq!(size_of::<super::Mind>(), 808);
}

#[test]
fn seeing_water_remembers_it_and_repeated_sightings_merge() {
    let mut map = MentalMap::default();
    let origin = at(100, 100);
    observe(
        &mut map,
        7,
        origin,
        &with_water(view_around(origin), &[at(104, 100)]),
        10,
    );
    observe(
        &mut map,
        7,
        origin,
        &with_water(view_around(origin), &[at(105, 101)]),
        20,
    );
    let remembered = landmarks(&map);
    assert_eq!(
        remembered.len(),
        1,
        "nearby sightings merge into one memory"
    );
    assert_eq!(remembered[0].kind, LandmarkKind::Water);
    assert_eq!(remembered[0].source, LandmarkSource::Seen);
    assert_eq!(remembered[0].seen_second, 20);
}

#[test]
fn memories_persist_out_of_view_and_are_recalled() {
    let mut map = MentalMap::default();
    let lake = at(0, 0);
    observe(
        &mut map,
        1,
        at(3, 0),
        &with_water(view_around(at(3, 0)), &[lake]),
        1,
    );
    // Walk far away: nothing in view, but the lake is still remembered.
    let far = at(300, 40);
    observe(&mut map, 1, far, &view_around(far), 2);
    let (destination, source) = map
        .recall(LandmarkKind::Water, 1, far, 0)
        .expect("remembered");
    assert_eq!((destination, source), (lake, LandmarkSource::Seen));
    assert_eq!(
        map.nearest_seen_distance(LandmarkKind::Water, far),
        Some(340)
    );
}

#[test]
fn looking_at_an_empty_remembered_place_forgets_it() {
    let mut map = MentalMap::default();
    let bush = at(50, 50);
    observe(
        &mut map,
        1,
        at(52, 50),
        &with_food(view_around(at(52, 50)), &[bush]),
        1,
    );
    assert_eq!(map.seen_count(LandmarkKind::Berries), 1);
    // The bush was eaten: the same spot is in view with no food anywhere.
    observe(&mut map, 1, at(51, 50), &view_around(at(51, 50)), 2);
    assert_eq!(map.seen_count(LandmarkKind::Berries), 0);
}

#[test]
fn slots_are_bounded_per_kind_and_keep_the_freshest() {
    let mut map = MentalMap::default();
    for index in 0..10_i64 {
        let spot = at(index * 100, 0);
        observe(
            &mut map,
            1,
            spot,
            &with_water(view_around(spot), &[spot]),
            index as u32,
        );
    }
    let water: Vec<_> = landmarks(&map)
        .into_iter()
        .filter(|landmark| landmark.kind == LandmarkKind::Water)
        .collect();
    assert_eq!(water.len(), 4);
    assert!(water.iter().all(|landmark| landmark.seen_second >= 6));
}

#[test]
fn hearsay_is_stored_with_uncertainty_and_never_overrides_first_hand_memory() {
    let mut map = MentalMap::default();
    assert!(map.remember_told(
        LandmarkKind::Berries,
        at(200, 0),
        10,
        5,
        HintSource::anonymous(),
        128
    ));
    let told = landmarks(&map)[0];
    assert_eq!(told.source, LandmarkSource::Told);
    assert_eq!(told.search_radius, 40);
    // Hearing about the same area again reinforces instead of duplicating.
    assert!(map.remember_told(
        LandmarkKind::Berries,
        at(210, 4),
        10,
        6,
        HintSource::anonymous(),
        128
    ));
    assert_eq!(landmarks(&map).len(), 1);
    assert!(landmarks(&map)[0].confidence > told.confidence);
    // Seeing food there replaces the hint with a precise memory.
    let spot = at(215, 3);
    observe(&mut map, 1, spot, &with_food(view_around(spot), &[spot]), 7);
    let after = landmarks(&map);
    assert_eq!(after.len(), 1);
    assert_eq!(after[0].source, LandmarkSource::Seen);
    // Hearsay about a place it has seen adds nothing.
    assert!(!map.remember_told(
        LandmarkKind::Berries,
        at(212, 0),
        3,
        8,
        HintSource::anonymous(),
        128
    ));
}

#[test]
fn a_hint_that_keeps_turning_up_empty_is_eventually_forgotten() {
    let mut map = MentalMap::default();
    map.remember_told(
        LandmarkKind::Water,
        at(0, 0),
        4,
        1,
        HintSource::anonymous(),
        128,
    );
    for step in 0..20 {
        let Some((probe, _)) = map.recall(LandmarkKind::Water, 3, at(0, 0), 0) else {
            return;
        };
        observe(&mut map, 3, probe, &view_around(probe), 2 + step);
    }
    panic!("hint should be forgotten after repeated empty searches");
}

#[test]
fn exploration_prefers_unvisited_directions() {
    let mut map = MentalMap::default();
    let origin = at(0, 0);
    assert_eq!(
        map.novel_heading(origin, ExplorationHeading::East),
        Some(ExplorationHeading::East)
    );
    // Having been east already, exploration turns.
    observe(&mut map, 1, at(48, 0), &view_around(at(48, 0)), 1);
    let heading = map.novel_heading(origin, ExplorationHeading::East).unwrap();
    assert_ne!(heading, ExplorationHeading::East);
}

#[test]
fn sharing_rotates_through_places_outside_the_view() {
    let mut map = MentalMap::default();
    for spot in [at(0, 0), at(200, 0)] {
        observe(
            &mut map,
            1,
            spot,
            &with_water(view_around(spot), &[spot]),
            1,
        );
    }
    let here = view_around(at(0, 0)).area;
    let (first, rank) = map.shareable(here).expect("far lake is shareable");
    assert_eq!(
        first,
        at(200, 0),
        "the lake in view is not worth pointing at"
    );
    map.mark_shared(5, rank);
    assert!(!map.share_ready(30, SHARE_COOLDOWN_SECONDS));
    assert!(map.share_ready(65, SHARE_COOLDOWN_SECONDS));
    let elsewhere = view_around(at(1_000, 1_000)).area;
    let (first, first_rank) = map.shareable(elsewhere).unwrap();
    map.mark_shared(70, first_rank);
    let (second, _) = map.shareable(elsewhere).unwrap();
    assert_ne!(first, second, "the next encounter shares a different place");
}

#[test]
fn hint_outcomes_are_reported_to_the_teller() {
    let mut map = MentalMap::default();
    map.remember_told(
        LandmarkKind::Water,
        at(0, 0),
        4,
        1,
        HintSource::from_teller(2),
        144,
    );
    let mut outcomes = Vec::new();
    let spot = at(3, 2);
    map.observe(
        1,
        spot,
        &with_water(view_around(spot), &[spot]),
        2,
        &mut |check| {
            outcomes.push((check.teller.unwrap_or(u8::MAX), check.confirmed));
        },
    );
    assert_eq!(
        outcomes,
        vec![(2, true)],
        "seeing the place confirms the hint"
    );

    let mut map = MentalMap::default();
    map.remember_told(
        LandmarkKind::Water,
        at(0, 0),
        4,
        1,
        HintSource::from_teller(5),
        144,
    );
    let mut outcomes = Vec::new();
    for step in 0..20 {
        let Some((probe, _)) = map.recall(LandmarkKind::Water, 3, at(0, 0), 0) else {
            break;
        };
        map.observe(3, probe, &view_around(probe), 2 + step, &mut |check| {
            outcomes.push((check.teller.unwrap_or(u8::MAX), check.confirmed));
        });
    }
    assert_eq!(
        outcomes,
        vec![(5, false)],
        "abandoning the hint refutes it once"
    );
}

#[test]
fn forgetting_a_teller_detaches_their_hints() {
    let mut map = MentalMap::default();
    map.remember_told(
        LandmarkKind::Berries,
        at(0, 0),
        4,
        1,
        HintSource::from_teller(1),
        144,
    );
    map.forget_teller(1);
    let mut outcomes = Vec::new();
    let spot = at(2, 2);
    map.observe(
        1,
        spot,
        &with_food(view_around(spot), &[spot]),
        2,
        &mut |check| {
            outcomes.push((check.teller.unwrap_or(u8::MAX), check.confirmed));
        },
    );
    assert_eq!(
        outcomes,
        vec![(u8::MAX, true)],
        "the check is still reported, but its teller is no longer known"
    );
}

#[test]
fn explored_ground_can_be_shared_and_marked() {
    let mut map = MentalMap::default();
    observe(&mut map, 1, at(500, 500), &view_around(at(500, 500)), 1);
    let here = view_around(at(0, 0)).area;
    let (marker, _) = map
        .shareable(here)
        .expect("explored tile far away is shareable");
    assert!(map.is_explored_marker(marker));
    let mut watcher = MentalMap::default();
    assert!(
        watcher.record_visit(marker),
        "the watcher marks that ground explored"
    );
    assert!(!watcher.record_visit(marker));
}

#[test]
fn a_fresh_close_hint_beats_a_stale_far_food_sighting() {
    let mut map = MentalMap::default();
    let far = at(400, 0);
    observe(&mut map, 1, far, &with_food(view_around(far), &[far]), 0);
    // Much later, someone points out food nearby.
    let now = 4_000;
    map.remember_told(
        LandmarkKind::Berries,
        at(40, 0),
        4,
        now,
        HintSource::anonymous(),
        120,
    );
    let (destination, source) = map.recall(LandmarkKind::Berries, 1, at(0, 0), now).unwrap();
    assert_eq!(source, LandmarkSource::Told);
    assert_eq!(destination, at(40, 0));
    // Right after the sighting, the same hint would not have won.
    let (_, early) = map.recall(LandmarkKind::Berries, 1, at(380, 0), 1).unwrap();
    assert_eq!(early, LandmarkSource::Seen);
}

#[test]
fn a_fresh_hint_can_displace_a_stale_food_memory_when_slots_are_full() {
    let mut map = MentalMap::default();
    for index in 0..3_i64 {
        let spot = at(index * 100, 0);
        observe(&mut map, 1, spot, &with_food(view_around(spot), &[spot]), 0);
    }
    assert!(map.remember_told(
        LandmarkKind::Berries,
        at(900, 900),
        4,
        5_000,
        HintSource::anonymous(),
        140
    ));
    assert!(
        landmarks(&map)
            .iter()
            .any(|place| place.source == LandmarkSource::Told)
    );
}

#[test]
fn curious_agents_pick_the_most_promising_unchecked_hint() {
    let mut map = MentalMap::default();
    map.remember_told(
        LandmarkKind::Water,
        at(300, 0),
        4,
        1,
        HintSource::anonymous(),
        200,
    );
    map.remember_told(
        LandmarkKind::Berries,
        at(30, 0),
        4,
        1,
        HintSource::anonymous(),
        200,
    );
    assert_eq!(map.hint_to_check(1, at(0, 0)), Some(at(30, 0)));
}

fn bush(position: WorldPosition, kind: Material) -> PerceivedResource {
    PerceivedResource {
        position,
        resource: BaseResource { capacity: 12, kind },
    }
}

/// A berries hint said with a word, where the listener also weighed bitter berries.
fn worded_berries_hint(map: &mut MentalMap, spot: WorldPosition) {
    let source = HintSource {
        form: Some(VocalForm(4)),
        alternative: Some(Concept::Bitterberries),
        ..HintSource::from_teller(1)
    };
    assert!(map.remember_told(LandmarkKind::Berries, spot, 3, 1, source, 144));
}

#[test]
fn a_worded_hint_is_judged_by_what_stands_at_the_spot() {
    let spot = at(0, 0);
    // A berry bush 10 cells away, known first-hand, doesn't make the hint old news.
    let mut map = MentalMap::default();
    observe(
        &mut map,
        1,
        at(10, 0),
        &with_food(view_around(at(10, 0)), &[at(10, 0)]),
        1,
    );
    worded_berries_hint(&mut map, spot);

    // Seen from afar, berries elsewhere in view don't settle it.
    let far = at(8, 0);
    let mut checks = Vec::new();
    let mut view = view_around(far);
    view.resources = vec![
        bush(at(10, 0), Material::Berries),
        bush(at(1, 0), Material::Bitterberries),
    ];
    map.observe(1, far, &view, 2, &mut |check| checks.push(check));
    assert!(checks.is_empty(), "{checks:?}");

    // Up close, the bitter bush stands closest to the spot: the word was misread.
    let near = at(3, 0);
    let mut view = view_around(near);
    view.resources = vec![
        bush(at(3, 1), Material::Berries),
        bush(at(1, 0), Material::Bitterberries),
    ];
    map.observe(1, near, &view, 3, &mut |check| checks.push(check));
    assert_eq!(checks.len(), 1);
    assert!(!checks[0].confirmed);
    assert_eq!(checks[0].alternative, Some(Concept::Bitterberries));
    assert_eq!(checks[0].form, Some(VocalForm(4)));

    // With berries closest to the spot, the same hint is confirmed.
    let mut map = MentalMap::default();
    worded_berries_hint(&mut map, spot);
    let mut checks = Vec::new();
    let mut view = view_around(near);
    view.resources = vec![
        bush(at(0, 1), Material::Berries),
        bush(at(2, 0), Material::Bitterberries),
    ];
    map.observe(1, near, &view, 3, &mut |check| checks.push(check));
    assert_eq!(checks.len(), 1);
    assert!(checks[0].confirmed);
}
