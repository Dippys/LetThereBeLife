use super::*;
use crate::{
    Material, PHYSICAL_POLICY_RADIUS, PhysicalGoal, PolicyOptions, PolicyReason, Species,
    WildlifeEvent,
};

/// Two neighbours with full minds, and wildlife allowed across the whole area.
fn valley_pair() -> (Engine, WorldPosition, WorldPosition) {
    let mut engine = resident_engine(64);
    let (first, second) = standable_shelter_site(&engine);
    let area = engine.world().initial_bounds();
    engine
        .initialize_population(
            PopulationInit {
                active_area: area,
                population: 2,
            },
            &[first, second],
        )
        .unwrap();
    engine.policy_options = PolicyOptions::full();
    engine.wildlife.area = Some(area);
    (engine, first, second)
}

/// A standable cell near `around` that nobody occupies.
fn free_cell_near(engine: &Engine, around: WorldPosition, distance: i64) -> WorldPosition {
    for radius in distance..distance + 6 {
        for (dx, dy) in [(radius, 0), (-radius, 0), (0, radius), (0, -radius)] {
            let cell = WorldPosition {
                x: around.x + dx,
                y: around.y + dy,
            };
            if engine.animal_can_stand(cell) {
                return cell;
            }
        }
    }
    panic!("no free cell near {around:?}");
}

#[test]
fn released_wildlife_is_deterministic() {
    let release = || {
        let (mut engine, _, _) = valley_pair();
        let area = engine.world().initial_bounds();
        let placed = engine.release_wildlife(area, 9, 1);
        (placed, engine.animal_views().collect::<Vec<_>>())
    };
    let (placed, animals) = release();
    assert_eq!(placed, 10);
    assert_eq!(animals.len(), 10);
    assert_eq!(release().1, animals);
}

#[test]
fn a_hungry_wolf_bites_a_lone_person_and_everyone_who_saw_learns_to_fear_wolves() {
    let (mut engine, victim, _) = valley_pair();
    // Agent 1 is the onlooker; make sure neither starts out fearing wolves.
    engine.minds.set_founders(0);
    let wolf_at = free_cell_near(&engine, victim, 1);
    assert!(engine.spawn_animal(Species::Wolf, wolf_at));
    engine.wildlife.animals[0].next_act = 0;
    while engine.time.ticks() % crate::WILDLIFE_TICKS != 0 {
        engine.time = engine.time.checked_add(1).unwrap();
    }
    engine.step_wildlife();

    let bite = engine
        .wildlife_events()
        .iter()
        .find_map(|event| match *event {
            WildlifeEvent::Bite { agent, damage, .. } => Some((agent, damage)),
            _ => None,
        })
        .expect("the wolf bit someone");
    let after = engine.population.health_view(bite.0).unwrap().value;
    assert_eq!(after, crate::HEALTH_MAX - bite.1);
    for agent in [0, 1] {
        assert!(
            engine
                .minds
                .get_mut(AgentId::new(agent))
                .fauna
                .dangerous(Species::Wolf),
            "agent {agent} learned wolves are dangerous"
        );
    }
}

#[test]
fn hunting_brings_down_a_deer_and_leaves_meat_to_gather() {
    let (mut engine, hunter, _) = valley_pair();
    let deer_at = free_cell_near(&engine, hunter, 2);
    assert!(engine.spawn_animal(Species::Deer, deer_at));
    let mut kills = 0;
    for _ in 0..40 {
        engine.time = engine.time.checked_add(1).unwrap();
        if engine.apply_hunt(AgentId::new(0), deer_at).is_err() {
            break;
        }
        kills += engine
            .wildlife_events()
            .iter()
            .filter(|event| matches!(event, WildlifeEvent::Struck { killed: true, .. }))
            .count();
        engine.wildlife_events.clear();
        if kills > 0 {
            break;
        }
    }
    assert_eq!(kills, 1, "repeated strikes bring it down");
    assert_eq!(engine.animal_count(Species::Deer), 0);
    let meat = engine
        .perceive_physical(AgentId::new(0), PHYSICAL_POLICY_RADIUS)
        .unwrap()
        .resources
        .into_iter()
        .find(|resource| resource.resource.kind == Material::Meat)
        .expect("the carcass is visible as meat");
    assert_eq!(meat.position, deer_at);
    assert_eq!(
        engine
            .available_resource_at(deer_at)
            .unwrap()
            .map(|resource| resource.kind),
        Some(Material::Meat)
    );
}

#[test]
fn people_run_from_animals_they_fear_and_go_after_ones_they_hunt() {
    let (mut engine, origin, _) = valley_pair();
    let wolf_at = free_cell_near(&engine, origin, 3);
    assert!(engine.spawn_animal(Species::Wolf, wolf_at));
    engine
        .minds
        .get_mut(AgentId::new(0))
        .fauna
        .bitten(Species::Wolf);
    let agent = AgentId::new(0);
    let decide = |engine: &mut Engine| {
        let needs = engine.population.needs_view(agent, engine.time).unwrap();
        let inventory = engine.population.inventory(agent).unwrap();
        let perception = engine
            .perceive_physical(agent, PHYSICAL_POLICY_RADIUS)
            .unwrap();
        engine
            .deliberate_with_memory(agent, origin, needs, inventory, &perception)
            .0
    };
    let fleeing = decide(&mut engine);
    assert_eq!(fleeing.reason, PolicyReason::Fleeing);

    // Same spot with a deer instead: a diligent hunter closes in or strikes.
    engine.wildlife.animals.clear();
    let deer_at = free_cell_near(&engine, origin, 2);
    assert!(engine.spawn_animal(Species::Deer, deer_at));
    let mut hunted = false;
    for step in 0..20_u64 {
        engine.time = crate::SimTime::from_ticks(step * crate::PHYSICAL_POLICY_IDLE_RECHECK_TICKS);
        let choice = decide(&mut engine);
        if choice.reason == PolicyReason::Hunting {
            assert!(matches!(
                choice.goal,
                PhysicalGoal::Hunt | PhysicalGoal::Explore
            ));
            hunted = true;
            break;
        }
    }
    assert!(hunted, "some idle check turns into a hunt");
}
