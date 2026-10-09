use super::map::Landmark;
use super::*;
use crate::{
    BaseResource, PerceivedResource, PerceivedWater, PhysicalPerception, ResourceKind, WaterSource,
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
                kind: ResourceKind::Food,
            },
        })
        .collect();
    perception
}

fn landmarks(map: &MentalMap) -> Vec<LandmarkView> {
    map.views().collect()
}

#[test]
fn mental_map_layout_is_compact() {
    assert_eq!(size_of::<Landmark>(), 12);
    assert_eq!(
        size_of::<MentalMap>(),
        12 * LANDMARK_SLOTS + 4 * VISITED_TILE_SLOTS + 16
    );
}

#[test]
fn seeing_water_remembers_it_and_repeated_sightings_merge() {
    let mut map = MentalMap::default();
    let origin = at(100, 100);
    map.observe(
        7,
        origin,
        &with_water(view_around(origin), &[at(104, 100)]),
        10,
    );
    map.observe(
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
    map.observe(1, at(3, 0), &with_water(view_around(at(3, 0)), &[lake]), 1);
    // Walk far away: nothing in view, but the lake is still remembered.
    let far = at(300, 40);
    map.observe(1, far, &view_around(far), 2);
    let (destination, source) = map.recall(LandmarkKind::Water, 1, far).expect("remembered");
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
    map.observe(
        1,
        at(52, 50),
        &with_food(view_around(at(52, 50)), &[bush]),
        1,
    );
    assert_eq!(map.seen_count(LandmarkKind::Food), 1);
    // The bush was eaten: the same spot is in view with no food anywhere.
    map.observe(1, at(51, 50), &view_around(at(51, 50)), 2);
    assert_eq!(map.seen_count(LandmarkKind::Food), 0);
}

#[test]
fn slots_are_bounded_per_kind_and_keep_the_freshest() {
    let mut map = MentalMap::default();
    for index in 0..10_i64 {
        let spot = at(index * 100, 0);
        map.observe(
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
    assert!(map.remember_told(LandmarkKind::Food, at(200, 0), 10, 5));
    let told = landmarks(&map)[0];
    assert_eq!(told.source, LandmarkSource::Told);
    assert_eq!(told.search_radius, 40);
    // Hearing about the same area again reinforces instead of duplicating.
    assert!(map.remember_told(LandmarkKind::Food, at(210, 4), 10, 6));
    assert_eq!(landmarks(&map).len(), 1);
    assert!(landmarks(&map)[0].confidence > told.confidence);
    // Seeing food there replaces the hint with a precise memory.
    let spot = at(215, 3);
    map.observe(1, spot, &with_food(view_around(spot), &[spot]), 7);
    let after = landmarks(&map);
    assert_eq!(after.len(), 1);
    assert_eq!(after[0].source, LandmarkSource::Seen);
    // Hearsay about a place it has seen adds nothing.
    assert!(!map.remember_told(LandmarkKind::Food, at(212, 0), 3, 8));
}

#[test]
fn a_hint_that_keeps_turning_up_empty_is_eventually_forgotten() {
    let mut map = MentalMap::default();
    map.remember_told(LandmarkKind::Water, at(0, 0), 4, 1);
    for step in 0..20 {
        let Some((probe, _)) = map.recall(LandmarkKind::Water, 3, at(0, 0)) else {
            return;
        };
        map.observe(3, probe, &view_around(probe), 2 + step);
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
    map.observe(1, at(48, 0), &view_around(at(48, 0)), 1);
    let heading = map.novel_heading(origin, ExplorationHeading::East).unwrap();
    assert_ne!(heading, ExplorationHeading::East);
}

#[test]
fn sharing_rotates_through_places_outside_the_view() {
    let mut map = MentalMap::default();
    for spot in [at(0, 0), at(200, 0)] {
        map.observe(1, spot, &with_water(view_around(spot), &[spot]), 1);
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
