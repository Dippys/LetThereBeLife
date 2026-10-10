use super::*;
use crate::{
    Concept, InventoryView, NeedKind, PHYSICAL_POLICY_RADIUS, PhysicalGoal, PolicyOptions,
    PolicyReason, RequestResponse,
};

/// Two neighbours with average personalities and helping enabled; agent 0 is
/// starving and carries nothing, agent 1 carries `food`.
fn hungry_and_holding(food: u8) -> Engine {
    let mut engine = resident_engine(64);
    let (asker, giver) = standable_shelter_site(&engine);
    let area = engine.world().initial_bounds();
    engine
        .initialize_population(
            PopulationInit {
                active_area: area,
                population: 2,
            },
            &[asker, giver],
        )
        .unwrap();
    engine
        .set_initial_inventory(
            AgentId::new(1),
            InventoryView::of(&[(crate::Material::Berries, food)]),
        )
        .unwrap();
    engine.policy_options = PolicyOptions {
        social: false,
        ..PolicyOptions::full()
    };
    engine.population.set_need_value_for_test(
        AgentId::new(0),
        NeedKind::Hunger,
        9_000,
        engine.time,
    );
    engine
}

fn ask(engine: &mut Engine) -> crate::RequestEvent {
    let asker = AgentId::new(0);
    let perception = engine
        .perceive_physical(asker, PHYSICAL_POLICY_RADIUS)
        .unwrap();
    let (giver, place) = engine
        .beg_target(asker, &perception)
        .expect("someone to ask");
    engine
        .minds
        .get_mut(asker)
        .dialogue
        .plan_request(giver, place);
    engine.apply_signal(asker, place).unwrap();
    engine.request_events()[0]
}

#[test]
fn a_hungry_agent_with_no_food_chooses_to_ask_someone_in_view() {
    let mut engine = hungry_and_holding(8);
    let asker = AgentId::new(0);
    let view = engine.population.view(asker).unwrap();
    let needs = engine.population.needs_view(asker, engine.time).unwrap();
    let inventory = engine.population.inventory(asker).unwrap();
    let perception = engine
        .perceive_physical(asker, PHYSICAL_POLICY_RADIUS)
        .unwrap();
    let (selection, _) =
        engine.deliberate_with_memory(asker, view.position, needs, inventory, &perception);
    assert_eq!(selection.goal, PhysicalGoal::Signal);
    assert_eq!(selection.reason, PolicyReason::Begging);
}

#[test]
fn a_friend_with_food_hands_over_a_meal() {
    let mut engine = hungry_and_holding(8);
    let request = ask(&mut engine);
    assert_eq!(request.response, RequestResponse::Gave);
    assert_eq!(request.read_as, Concept::Berries);
    assert_eq!(
        engine
            .population
            .inventory(AgentId::new(0))
            .unwrap()
            .amount(crate::Material::Berries),
        1
    );
    assert_eq!(
        engine
            .population
            .inventory(AgentId::new(1))
            .unwrap()
            .amount(crate::Material::Berries),
        7
    );
    let signal = engine.signal_events()[0];
    assert_eq!(signal.signal.addressee, Some(AgentId::new(1)));
    assert_eq!(signal.intent.effect, crate::DesiredEffect::Request);
    // The asker waits before asking again.
    let perception = engine
        .perceive_physical(AgentId::new(0), PHYSICAL_POLICY_RADIUS)
        .unwrap();
    assert_eq!(engine.beg_target(AgentId::new(0), &perception), None);
}

#[test]
fn empty_hands_give_nothing_and_givers_keep_what_they_need() {
    let mut engine = hungry_and_holding(0);
    assert_eq!(ask(&mut engine).response, RequestResponse::NothingToGive);

    // Someone who isn't the asker's parent keeps its last meal...
    let mut engine = hungry_and_holding(1);
    assert_eq!(ask(&mut engine).response, RequestResponse::Refused);

    // ...and a hungry giver keeps its food even with plenty.
    let mut engine = hungry_and_holding(8);
    engine.population.set_need_value_for_test(
        AgentId::new(1),
        NeedKind::Hunger,
        9_000,
        engine.time,
    );
    assert_eq!(ask(&mut engine).response, RequestResponse::Refused);
    assert_eq!(
        engine
            .population
            .inventory(AgentId::new(1))
            .unwrap()
            .amount(crate::Material::Berries),
        8
    );
}

#[test]
fn a_parent_feeds_its_child_and_a_misread_request_is_repaired() {
    let mut engine = hungry_and_holding(1);
    // Agent 0 is a child with no words or food beliefs; agent 1, its parent,
    // has its family's.
    engine.minds.set_founders(0);
    engine.minds.get_mut(AgentId::new(1)).affordances =
        crate::cognition::Affordances::founding(engine.config.seed, AgentId::new(1), 8);
    assert!(engine.bond(AgentId::new(0), AgentId::new(1)));
    // The child has no words, and the parent is thirsty: it first takes the
    // eating mime for a request about water.
    engine.population.set_need_value_for_test(
        AgentId::new(1),
        NeedKind::Thirst,
        9_000,
        engine.time,
    );
    let request = ask(&mut engine);
    assert_eq!(request.response, RequestResponse::Gave);
    assert_eq!(request.read_as, Concept::Water);
    assert_eq!(engine.repair_events().len(), 1);
}

#[test]
fn without_helping_nobody_asks() {
    let mut engine = hungry_and_holding(8);
    engine.policy_options.helping = false;
    let perception = engine
        .perceive_physical(AgentId::new(0), PHYSICAL_POLICY_RADIUS)
        .unwrap();
    assert_eq!(engine.beg_target(AgentId::new(0), &perception), None);
}
