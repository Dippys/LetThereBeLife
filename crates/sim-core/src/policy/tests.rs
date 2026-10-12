//! Policy state layout tests.

use std::mem::{align_of, size_of};

use super::state::PolicyNavigation;
use super::*;

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
