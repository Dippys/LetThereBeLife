//! Plants that die, are felled, and spread (D-111).

use super::*;

#[test]
fn a_felled_tree_is_gone_for_good() {
    let mut engine = resident_engine(64);
    let tree = engine
        .world()
        .all_features()
        .find(|feature| feature.kind == crate::FeatureKind::Tree)
        .map(|feature| feature.position)
        .expect("a tree in the resident area");
    while engine
        .resource_deltas
        .gather(&engine.world, tree, 255)
        .unwrap()
        .is_some()
    {}
    engine
        .resource_deltas
        .advance(crate::SimTime::from_ticks(crate::TICKS_PER_YEAR * 10));
    assert_eq!(engine.available_resource_at(tree), Ok(None), "no regrowth");
    assert_eq!(engine.dead_plants().collect::<Vec<_>>(), [tree]);
}

#[test]
fn a_seedling_bush_fruits_once_it_has_grown_and_a_sapling_falls_to_one_chop() {
    let mut engine = resident_engine(64);
    let (bush, sapling) = {
        let bounds = engine.world().initial_bounds();
        let mut open = (bounds.min.y..=bounds.max.y)
            .flat_map(|y| (bounds.min.x..=bounds.max.x).map(move |x| WorldPosition { x, y }))
            .filter(|&cell| engine.can_sprout_at(cell));
        (open.next().unwrap(), open.next().unwrap())
    };
    engine.spawned_objects.plant(bush, SpawnKind::BerryBush, 0);
    engine.spawned_objects.plant(sapling, SpawnKind::Tree, 2);
    assert_eq!(engine.available_resource_at(bush), Ok(None));
    engine
        .spawned_objects
        .advance(crate::season::SEASON_TICKS as u32 / 60);
    assert!(
        engine
            .available_resource_at(bush)
            .unwrap()
            .is_some_and(|berries| berries.capacity > 0),
        "a growing season later it bears fruit"
    );
    assert_eq!(
        engine.spawned_objects.gather(sapling, 4),
        Some((Material::Wood, 2))
    );
    assert!(engine.spawned_object_at(sapling).is_none(), "felled");
}

#[test]
fn each_spring_old_plants_die_and_new_ones_take_root_nearby() {
    let mut engine = resident_engine(128);
    // Plants live near people: one person stands in the middle.
    let bounds = engine.world().initial_bounds();
    let middle = (bounds.min.y..=bounds.max.y)
        .flat_map(|y| (bounds.min.x..=bounds.max.x).map(move |x| WorldPosition { x, y }))
        .filter(|&cell| engine.can_sprout_at(cell))
        .min_by_key(|cell| {
            cell.x.abs_diff((bounds.min.x + bounds.max.x) / 2)
                + cell.y.abs_diff((bounds.min.y + bounds.max.y) / 2)
        })
        .unwrap();
    engine
        .initialize_population(
            PopulationInit {
                active_area: bounds,
                population: 1,
            },
            &[middle],
        )
        .unwrap();
    let before = engine.plant_census();
    engine.time = crate::SimTime::from_ticks(crate::TICKS_PER_YEAR);
    engine.grow_plants();
    let (sprouted, died) = engine.plant_counts();
    assert!(sprouted > 0 && died > 0, "sprouted {sprouted}, died {died}");
    let after = engine.plant_census();
    assert_eq!(
        after.0 + after.1,
        before.0 + before.1 + sprouted - died,
        "every plant is counted once"
    );
    for object in engine.spawned_object_views() {
        let near = engine.world().all_features().any(|feature| {
            feature.position.x.abs_diff(object.position.x) <= 5
                && feature.position.y.abs_diff(object.position.y) <= 5
        });
        assert!(near, "seedlings fall near a plant");
    }
}
