//! Policy state layout and goal-selection tests.

use std::mem::{align_of, size_of};

use super::state::PolicyNavigation;
use super::*;
use crate::{
    BaseResource, InventoryView, NeedLevelView, PerceivedResource, PerceivedWater,
    PhysicalNeedsView, PhysicalPerception, ResourceKind, WaterSource, WorldRect,
};

#[test]
fn policy_state_has_a_fixed_pointer_free_layout() {
    assert_eq!(size_of::<PolicyState>(), 12);
    assert_eq!(align_of::<PolicyState>(), 4);
    assert_eq!(size_of::<PolicyNavigation>(), 1);
    assert_eq!(size_of::<ExplorationHeading>(), 1);
    assert_eq!(size_of::<PolicyDiagnostic>(), 40);
    assert_eq!(align_of::<PolicyDiagnostic>(), 8);

    let mut state = PolicyState::for_agent(AgentId::new(7));
    state.set_phase(PolicyPhase::Routing);
    let heading = state.exploration_heading();
    state.set_phase(PolicyPhase::Backoff);
    assert_eq!(state.exploration_heading(), heading);
    state.set_exploration_heading(ExplorationHeading::SouthWest);
    state.reason = PolicyReason::HungerThreshold;
    assert_eq!(state.phase(), PolicyPhase::Backoff);
    assert_eq!(
        state.view(AgentId::new(7)).reason,
        PolicyReason::HungerThreshold
    );
}

#[test]
fn retry_is_positive_and_bounded() {
    assert_eq!(retry_delay(0), 60);
    assert_eq!(retry_delay(1), 120);
    assert_eq!(retry_delay(u8::MAX), PHYSICAL_POLICY_MAX_BACKOFF_TICKS);
}

fn level(value: u16, threshold: u16) -> NeedLevelView {
    NeedLevelView {
        value,
        rate_per_period: 1,
        threshold,
        threshold_reached: value >= threshold,
    }
}

fn needs(hunger: u16, thirst: u16, rest: u16, exposure: u16) -> PhysicalNeedsView {
    PhysicalNeedsView {
        agent: AgentId::new(0),
        at: SimTime::ZERO,
        hunger: level(hunger, 7_000),
        thirst: level(thirst, 6_000),
        rest: level(rest, 8_000),
        exposure: level(exposure, 7_000),
        next_threshold: None,
    }
}

fn perception() -> PhysicalPerception {
    PhysicalPerception {
        area: WorldRect {
            min: WorldPosition { x: -4, y: -4 },
            max: WorldPosition { x: 5, y: 5 },
        },
        agents: Vec::new(),
        claimed_targets: Vec::new(),
        drinkable_water: vec![PerceivedWater {
            position: WorldPosition { x: 2, y: 0 },
            source: WaterSource::Lake,
        }],
        resources: vec![PerceivedResource {
            position: WorldPosition { x: 0, y: 2 },
            resource: BaseResource {
                capacity: 12,
                kind: ResourceKind::Food,
            },
        }],
        structures: Vec::new(),
        traversable_cells: vec![
            WorldPosition { x: 0, y: 0 },
            WorldPosition { x: 1, y: 0 },
            WorldPosition { x: 0, y: 2 },
        ],
        reachable_cells: vec![
            WorldPosition { x: 0, y: 0 },
            WorldPosition { x: 1, y: 0 },
            WorldPosition { x: 0, y: 2 },
        ],
    }
}

#[test]
fn urgent_selection_uses_normalized_integer_score_and_explicit_ties() {
    let origin = WorldPosition { x: 0, y: 0 };
    let facts = perception();
    assert_eq!(
        select(
            origin,
            needs(7_000, 6_000, 0, 0),
            InventoryView::default(),
            &facts,
        )
        .goal,
        PhysicalGoal::SeekWater
    );
    assert_eq!(
        select(
            origin,
            needs(8_400, 6_000, 0, 0),
            InventoryView::default(),
            &facts,
        )
        .goal,
        PhysicalGoal::SeekFood
    );
}

#[test]
fn irrelevant_candidates_do_not_reorder_the_selected_water_access() {
    let origin = WorldPosition { x: 0, y: 0 };
    let mut facts = perception();
    let selected = select(
        origin,
        needs(0, 6_000, 0, 0),
        InventoryView::default(),
        &facts,
    );
    facts.resources.push(PerceivedResource {
        position: WorldPosition { x: -3, y: -3 },
        resource: BaseResource {
            capacity: 120,
            kind: ResourceKind::Wood,
        },
    });
    assert_eq!(
        select(
            origin,
            needs(0, 6_000, 0, 0),
            InventoryView::default(),
            &facts,
        ),
        selected
    );
    assert_eq!(selected.target, Some(WorldPosition { x: 1, y: 0 }));
}

#[test]
fn disconnected_objective_access_is_not_selected_for_repeated_failure() {
    let origin = WorldPosition { x: 0, y: 0 };
    let mut facts = perception();
    facts.reachable_cells = vec![origin];

    let (selection, _) = select_with_exploration(
        origin,
        needs(0, 6_000, 0, 0),
        InventoryView::default(),
        &facts,
        Some(ExplorationHeading::North),
    );

    assert_eq!(selection.goal, PhysicalGoal::SeekWater);
    assert_eq!(selection.target, None);
}

#[test]
fn occupied_and_claimed_objective_cells_select_distinct_fallbacks() {
    let origin = WorldPosition { x: 0, y: 0 };
    let mut facts = perception();
    facts.traversable_cells.extend([
        WorldPosition { x: 2, y: -1 },
        WorldPosition { x: 3, y: 0 },
        WorldPosition { x: 2, y: 1 },
        WorldPosition { x: 0, y: 1 },
        WorldPosition { x: -1, y: 2 },
        WorldPosition { x: 1, y: 2 },
        WorldPosition { x: 0, y: 3 },
    ]);
    facts
        .traversable_cells
        .sort_unstable_by_key(|position| (position.y, position.x));
    facts.reachable_cells = facts.traversable_cells.clone();
    facts.agents.push(crate::AgentView {
        id: AgentId::new(1),
        position: WorldPosition { x: 1, y: 0 },
        activity: crate::AgentActivity::Moving,
    });
    facts.claimed_targets = vec![WorldPosition { x: 2, y: -1 }];

    let water = select(
        origin,
        needs(0, 6_000, 0, 0),
        InventoryView::default(),
        &facts,
    );
    assert_eq!(water.goal, PhysicalGoal::SeekWater);
    assert_eq!(water.target, Some(WorldPosition { x: 3, y: 0 }));

    facts.agents[0].position = WorldPosition { x: 0, y: 1 };
    facts.claimed_targets = vec![WorldPosition { x: 0, y: 2 }];
    let resource = select(origin, needs(0, 0, 0, 0), InventoryView::default(), &facts);
    assert_eq!(resource.goal, PhysicalGoal::GatherMaterial);
    assert_eq!(resource.target, Some(WorldPosition { x: -1, y: 2 }));
}

#[test]
fn carried_food_turns_hunger_into_eating_and_idle_agents_gather_capacity() {
    let origin = WorldPosition { x: 0, y: 0 };
    let facts = perception();
    let carrying_food = InventoryView {
        food: 1,
        ..InventoryView::default()
    };
    assert_eq!(
        select(origin, needs(7_000, 0, 0, 0), carrying_food, &facts).goal,
        PhysicalGoal::Eat
    );
    assert_eq!(
        select(origin, needs(0, 0, 0, 0), InventoryView::default(), &facts,).goal,
        PhysicalGoal::GatherMaterial
    );
    let full = InventoryView {
        food: crate::INVENTORY_CAPACITY_PER_KIND,
        wood: crate::INVENTORY_CAPACITY_PER_KIND,
        stone: crate::INVENTORY_CAPACITY_PER_KIND,
    };
    assert_eq!(
        select(origin, needs(0, 0, 0, 0), full, &facts).goal,
        PhysicalGoal::BuildShelter
    );
}

#[test]
fn every_planned_physical_goal_has_a_stable_compact_discriminant() {
    assert_eq!(size_of::<PhysicalGoal>(), 1);
    assert_eq!(PhysicalGoal::SeekWater as u8, 0);
    assert_eq!(PhysicalGoal::SeekFood as u8, 1);
    assert_eq!(PhysicalGoal::GatherMaterial as u8, 2);
    assert_eq!(PhysicalGoal::Eat as u8, 3);
    assert_eq!(PhysicalGoal::Drink as u8, 4);
    assert_eq!(PhysicalGoal::Sleep as u8, 5);
    assert_eq!(PhysicalGoal::SeekShelter as u8, 6);
    assert_eq!(PhysicalGoal::BuildShelter as u8, 7);
    assert_eq!(PhysicalGoal::Wait as u8, 8);
    assert_eq!(PhysicalGoal::Incapacitated as u8, 9);
    assert_eq!(PhysicalGoal::Explore as u8, 10);
}

#[test]
fn agents_explore_when_no_local_objective_exists() {
    let origin = WorldPosition { x: 0, y: 0 };
    let mut facts = perception();
    facts.resources.clear();
    facts.drinkable_water.clear();
    facts.area = WorldRect {
        min: WorldPosition { x: -8, y: -8 },
        max: WorldPosition { x: 9, y: 9 },
    };
    facts.traversable_cells = (-8..=8)
        .flat_map(|y| (-8..=8).map(move |x| WorldPosition { x, y }))
        .collect();
    facts.reachable_cells = facts.traversable_cells.clone();
    let (selection, heading) = select_with_exploration(
        origin,
        needs(0, 0, 0, 0),
        InventoryView::default(),
        &facts,
        Some(ExplorationHeading::NorthEast),
    );
    assert_eq!(selection.goal, PhysicalGoal::Explore);
    assert_ne!(selection.target, Some(origin));
    let heading = heading.unwrap();
    let first_target = selection.target.unwrap();

    facts.area = WorldRect {
        min: WorldPosition {
            x: first_target.x - 8,
            y: first_target.y - 8,
        },
        max: WorldPosition {
            x: first_target.x + 9,
            y: first_target.y + 9,
        },
    };
    facts.traversable_cells = (facts.area.min.y..facts.area.max.y)
        .flat_map(|y| (facts.area.min.x..facts.area.max.x).map(move |x| WorldPosition { x, y }))
        .collect();
    facts.reachable_cells = facts.traversable_cells.clone();
    let (continued, _) = select_with_exploration(
        first_target,
        needs(0, 0, 0, 0),
        InventoryView::default(),
        &facts,
        Some(heading),
    );
    let continued_target = continued.target.unwrap();
    let first_dx = first_target.x - origin.x;
    let first_dy = first_target.y - origin.y;
    let next_dx = continued_target.x - first_target.x;
    let next_dy = continued_target.y - first_target.y;
    assert!(first_dx * next_dx + first_dy * next_dy >= 0);
    assert_ne!(continued_target, origin);

    let (urgent, _) = select_with_exploration(
        origin,
        needs(0, 6_000, 0, 0),
        InventoryView::default(),
        &facts,
        Some(ExplorationHeading::NorthEast),
    );
    assert_eq!(urgent.goal, PhysicalGoal::Explore);
    assert_eq!(urgent.reason, PolicyReason::ThirstThreshold);
}

#[test]
fn safe_water_access_is_not_abandoned_by_optional_exploration() {
    let origin = WorldPosition { x: 1, y: 0 };
    let mut facts = perception();
    facts.resources.clear();
    facts.reachable_cells.push(origin);
    facts
        .reachable_cells
        .sort_unstable_by_key(|position| (position.y, position.x));

    let (selection, heading) = select_with_exploration(
        origin,
        needs(0, 0, 0, 0),
        InventoryView::default(),
        &facts,
        Some(ExplorationHeading::East),
    );

    assert_eq!(selection.goal, PhysicalGoal::Wait);
    assert_eq!(selection.target, Some(origin));
    assert_eq!(heading, None);
}
