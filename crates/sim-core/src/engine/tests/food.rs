use super::*;
use crate::{InventoryView, Material, NeedKind, PolicyOptions};

/// Two neighbours with full minds. Agent 0 carries `food`; agent 1 watches.
fn eater_and_watcher(food: &[(Material, u8)]) -> Engine {
    let mut engine = resident_engine(64);
    let (eater, watcher) = standable_shelter_site(&engine);
    let area = engine.world().initial_bounds();
    engine
        .initialize_population(
            PopulationInit {
                active_area: area,
                population: 2,
            },
            &[eater, watcher],
        )
        .unwrap();
    engine
        .set_initial_inventory(AgentId::new(0), InventoryView::of(food))
        .unwrap();
    engine.policy_options = PolicyOptions::full();
    engine
}

fn value(engine: &mut Engine, agent: u32, material: Material) -> Option<i16> {
    engine
        .minds
        .get_mut(AgentId::new(agent))
        .affordances
        .food_value(material)
}

#[test]
fn bitter_berries_make_the_eater_sick_and_everyone_watching_learns() {
    let mut engine = eater_and_watcher(&[(Material::Bitterberries, 2)]);
    // The eater was raised to think they're fine; the watcher has no idea.
    engine.minds.set_founders(0);
    engine.minds.get_mut(AgentId::new(0)).affordances =
        crate::cognition::Affordances::founding(engine.config.seed, AgentId::new(0), 8);
    if value(&mut engine, 0, Material::Bitterberries).is_none_or(|value| value <= 0) {
        // This seed's first family avoids them; the test needs a believer.
        engine.minds.get_mut(AgentId::new(0)).affordances =
            crate::cognition::Affordances::founding(engine.config.seed, AgentId::new(8), 8);
    }
    assert!(value(&mut engine, 0, Material::Bitterberries).unwrap() > 0);
    engine.population.set_need_value_for_test(
        AgentId::new(0),
        NeedKind::Hunger,
        6_000,
        engine.time,
    );
    let before = engine
        .population
        .needs_view(AgentId::new(0), engine.time)
        .unwrap();

    engine.apply_eat(AgentId::new(0)).unwrap();

    let after = engine
        .population
        .needs_view(AgentId::new(0), engine.time)
        .unwrap();
    assert!(after.hunger.value < before.hunger.value);
    assert!(
        after.thirst.value >= before.thirst.value + 2_000,
        "retching dehydrates"
    );
    assert!(after.rest.value >= before.rest.value + 2_000, "and drains");
    let meal = engine.meal_events()[0];
    assert_eq!(meal.material, Material::Bitterberries);
    assert!(meal.retched);
    assert_eq!(meal.watchers, 1);
    assert!(
        value(&mut engine, 0, Material::Bitterberries).unwrap() < 0,
        "it felt it"
    );
    assert!(
        value(&mut engine, 1, Material::Bitterberries).unwrap() < 0,
        "the watcher saw it retch"
    );
}

#[test]
fn an_agent_eats_what_it_believes_is_best() {
    let mut engine = eater_and_watcher(&[(Material::Bitterberries, 1), (Material::Berries, 1)]);
    engine.apply_eat(AgentId::new(0)).unwrap();
    assert_eq!(engine.meal_events()[0].material, Material::Berries);
}

#[test]
fn a_hungry_child_tastes_what_it_has_never_tried_and_a_full_one_does_not() {
    let mut engine = eater_and_watcher(&[(Material::Stone, 1)]);
    engine.minds.set_founders(0);
    assert_eq!(
        engine
            .food_values(AgentId::new(0))
            .best_carried(engine.population.inventory(AgentId::new(0)).unwrap()),
        None
    );
    engine.population.set_need_value_for_test(
        AgentId::new(0),
        NeedKind::Hunger,
        9_000,
        engine.time,
    );
    engine.apply_eat(AgentId::new(0)).unwrap();
    let meal = engine.meal_events()[0];
    assert_eq!(meal.material, Material::Stone);
    assert!(meal.first_taste);
    // Stone fed it nothing; it won't try that again.
    assert_eq!(value(&mut engine, 0, Material::Stone), Some(0));
}

#[test]
fn legacy_agents_without_minds_eat_by_real_properties() {
    let mut engine = eater_and_watcher(&[(Material::Bitterberries, 1), (Material::Berries, 1)]);
    engine.policy_options = PolicyOptions::default();
    engine.apply_eat(AgentId::new(0)).unwrap();
    assert_eq!(
        engine
            .population
            .inventory(AgentId::new(0))
            .unwrap()
            .amount(Material::Berries),
        0
    );
    assert!(
        engine.meal_events().is_empty(),
        "no minds, nothing to learn or log"
    );
}

#[test]
fn picked_bushes_grow_back_over_time_and_stone_does_not() {
    let engine = resident_engine(64);
    let mut deltas = crate::resources::ResourceDeltas::default();
    let mut regrowth = |material: Material| {
        let position = engine
            .world()
            .all_features()
            .find(|feature| feature.base_resource().kind == material)
            .map(|feature| feature.position)?;
        while deltas
            .gather(engine.world(), position, 255)
            .unwrap()
            .is_some()
        {}
        let period = material.properties().regrow_seconds;
        deltas.advance(crate::SimTime::from_ticks(
            u64::from(period.max(1)) * 60 * 2,
        ));
        let left = deltas
            .resource_at(engine.world(), position)
            .unwrap()
            .map_or(0, |resource| resource.capacity);
        deltas.advance(crate::SimTime::ZERO);
        Some(left)
    };
    if let Some(berries) = regrowth(Material::Berries) {
        assert_eq!(berries, 2, "two regrowth periods, two berries");
    }
    if let Some(stone) = regrowth(Material::Stone) {
        assert_eq!(stone, 0);
    }
}
