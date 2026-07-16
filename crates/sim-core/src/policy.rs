use std::{error::Error, fmt};

use crate::{
    AgentId, InventoryView, NeedKind, PhysicalNeedsView, PhysicalPerception, ResourceKind, SimTime,
    WorldPosition,
    agent::CompactPosition,
    structures::{SHELTER_WOOD_COST, StructureState},
};

pub const PHYSICAL_POLICY_RADIUS: u8 = 8;
pub const PHYSICAL_POLICY_ROUTE_BUDGET: u16 = 256;
pub const PHYSICAL_POLICY_IDLE_RECHECK_TICKS: u64 = 600;
pub const PHYSICAL_POLICY_ACTION_TICKS: u64 = 60;
pub const PHYSICAL_POLICY_MAX_BACKOFF_TICKS: u64 = 1_920;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyActivationError {
    PopulationNotInitialized,
    AlreadyActive,
    AgentCommitted { agent: AgentId },
    TimeOverflow,
    EventSequenceExhausted,
}

impl fmt::Display for PolicyActivationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "physical policy activation failed: {self:?}")
    }
}

impl Error for PolicyActivationError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum PhysicalGoal {
    SeekWater = 0,
    SeekFood = 1,
    GatherMaterial = 2,
    Eat = 3,
    Drink = 4,
    Sleep = 5,
    SeekShelter = 6,
    BuildShelter = 7,
    Wait = 8,
    Incapacitated = 9,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PolicyReason {
    InitialDecision,
    ThirstThreshold,
    HungerThreshold,
    RestThreshold,
    ExposureThreshold,
    NoUrgentNeed,
    RouteArrived,
    ActionCompleted,
    ShelterMaterials,
    Retry,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PolicyFailureReason {
    NoPerceivedTarget,
    Unloaded,
    OutsideWorld,
    OutsideActiveArea,
    Occupied,
    NoPath,
    RouteBudgetExhausted,
    TargetUnavailable,
    ResourceDepleted,
    InventoryFull,
    NoEdibleInventory,
    InvalidWaterAccess,
    SleepLocationWater,
    SleepLocationBlocked,
    SleepLocationOccupied,
    SleepLocationUnsafe,
    SleepLocationUnavailable,
    BuildSiteInvalid,
    InsufficientMaterials,
    TimeOverflow,
    RescheduleLimit,
    EventSequenceExhausted,
    InconsistentState,
    DeferredToLaterSlice,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PolicyDiagnosticKind {
    Selected,
    RouteScheduled,
    ActionStarted,
    ActionCompleted,
    ActionDeferred,
    RetryScheduled,
    StaleEvent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PolicyDiagnostic {
    pub agent: AgentId,
    pub at: SimTime,
    pub goal: PhysicalGoal,
    pub target: Option<WorldPosition>,
    pub reason: PolicyReason,
    pub kind: PolicyDiagnosticKind,
    pub failure: Option<PolicyFailureReason>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhysicalPolicyView {
    pub agent: AgentId,
    pub goal: PhysicalGoal,
    pub target: Option<WorldPosition>,
    pub committed: bool,
    pub retry_count: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum PolicyPhase {
    Dormant,
    DecisionPending,
    Routing,
    Acting,
    Backoff,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub(crate) struct PolicyState {
    pub(crate) target: CompactPosition,
    pub(crate) generation: u32,
    pub(crate) goal: PhysicalGoal,
    pub(crate) phase: PolicyPhase,
    pub(crate) retries: u8,
    pub(crate) reason: PolicyReason,
}

impl Default for PolicyState {
    fn default() -> Self {
        Self {
            target: CompactPosition { x: 0, y: 0 },
            generation: 0,
            goal: PhysicalGoal::Wait,
            phase: PolicyPhase::Dormant,
            retries: 0,
            reason: PolicyReason::InitialDecision,
        }
    }
}

impl PolicyState {
    pub(crate) fn view(self, agent: AgentId) -> PhysicalPolicyView {
        PhysicalPolicyView {
            agent,
            goal: self.goal,
            target: matches!(self.phase, PolicyPhase::Routing | PolicyPhase::Acting)
                .then(|| self.target.world()),
            committed: matches!(self.phase, PolicyPhase::Routing | PolicyPhase::Acting),
            retry_count: self.retries,
        }
    }

    pub(crate) fn next_generation(&mut self) -> Option<u32> {
        self.generation = self.generation.checked_add(1)?;
        Some(self.generation)
    }

    pub(crate) const fn event_is_current(self, generation: u32) -> bool {
        self.generation == generation
            && matches!(
                self.phase,
                PolicyPhase::DecisionPending | PolicyPhase::Backoff | PolicyPhase::Acting
            )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PolicySelection {
    pub(crate) goal: PhysicalGoal,
    pub(crate) target: Option<WorldPosition>,
    pub(crate) reason: PolicyReason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PolicyAction {
    pub(crate) goal: PhysicalGoal,
    pub(crate) target: WorldPosition,
    pub(crate) reason: PolicyReason,
    pub(crate) duration: u64,
}

pub(crate) fn select(
    origin: WorldPosition,
    needs: PhysicalNeedsView,
    inventory: InventoryView,
    perception: &PhysicalPerception,
) -> PolicySelection {
    let urgent = NeedKind::ALL
        .into_iter()
        .filter_map(|kind| {
            let level = match kind {
                NeedKind::Hunger => needs.hunger,
                NeedKind::Thirst => needs.thirst,
                NeedKind::Rest => needs.rest,
                NeedKind::Exposure => needs.exposure,
            };
            level.threshold_reached.then_some((
                urgency_score(level.value, level.threshold),
                urgency_tie_rank(kind),
                kind,
            ))
        })
        .max_by_key(|&(score, tie, _)| (score, tie));

    match urgent.map(|(_, _, kind)| kind) {
        Some(NeedKind::Thirst) => PolicySelection {
            goal: PhysicalGoal::SeekWater,
            target: nearest_water_access(origin, perception),
            reason: PolicyReason::ThirstThreshold,
        },
        Some(NeedKind::Hunger) if inventory.food > 0 => PolicySelection {
            goal: PhysicalGoal::Eat,
            target: Some(origin),
            reason: PolicyReason::HungerThreshold,
        },
        Some(NeedKind::Hunger) => PolicySelection {
            goal: PhysicalGoal::SeekFood,
            target: nearest_resource_access(origin, perception, |kind| {
                kind == ResourceKind::Food && inventory.can_add(kind)
            }),
            reason: PolicyReason::HungerThreshold,
        },
        Some(NeedKind::Rest) => PolicySelection {
            goal: PhysicalGoal::Sleep,
            target: nearest_shelter_access(origin, perception).or(Some(origin)),
            reason: PolicyReason::RestThreshold,
        },
        Some(NeedKind::Exposure) => shelter_selection(
            origin,
            inventory,
            perception,
            PolicyReason::ExposureThreshold,
        ),
        None => shelter_selection(origin, inventory, perception, PolicyReason::NoUrgentNeed),
    }
}

fn shelter_selection(
    origin: WorldPosition,
    inventory: InventoryView,
    perception: &PhysicalPerception,
    reason: PolicyReason,
) -> PolicySelection {
    if let Some(access) = nearest_shelter_access(origin, perception) {
        if reason == PolicyReason::ExposureThreshold {
            return PolicySelection {
                goal: if access == origin {
                    PhysicalGoal::Sleep
                } else {
                    PhysicalGoal::SeekShelter
                },
                target: Some(access),
                reason,
            };
        }
        return nearest_resource_access(origin, perception, |kind| inventory.can_add(kind)).map_or(
            PolicySelection {
                goal: PhysicalGoal::Wait,
                target: Some(origin),
                reason,
            },
            |target| PolicySelection {
                goal: PhysicalGoal::GatherMaterial,
                target: Some(target),
                reason,
            },
        );
    }

    if inventory.wood >= SHELTER_WOOD_COST {
        if let Some(site) = nearest_build_site(origin, perception) {
            return PolicySelection {
                goal: PhysicalGoal::BuildShelter,
                target: Some(site),
                reason,
            };
        }
    }

    let needs_wood = inventory.wood < SHELTER_WOOD_COST;
    let material = nearest_resource_access(origin, perception, |kind| {
        inventory.can_add(kind) && needs_wood && kind == ResourceKind::Wood
    });
    let fallback = (reason == PolicyReason::NoUrgentNeed)
        .then(|| nearest_resource_access(origin, perception, |kind| inventory.can_add(kind)))
        .flatten();
    material.or(fallback).map_or(
        PolicySelection {
            goal: PhysicalGoal::Wait,
            target: Some(origin),
            reason,
        },
        |target| PolicySelection {
            goal: PhysicalGoal::GatherMaterial,
            target: Some(target),
            reason: if material.is_some() {
                PolicyReason::ShelterMaterials
            } else {
                reason
            },
        },
    )
}

fn nearest_shelter_access(
    origin: WorldPosition,
    perception: &PhysicalPerception,
) -> Option<WorldPosition> {
    perception
        .structures
        .iter()
        .filter(|structure| structure.state == StructureState::Complete)
        .flat_map(|structure| cardinal_neighbors(structure.position))
        .filter(|candidate| traversable(perception, *candidate))
        .filter(|candidate| {
            *candidate == origin
                || !perception
                    .agents
                    .iter()
                    .any(|agent| agent.position == *candidate)
        })
        .min_by_key(|candidate| target_key(origin, *candidate))
}

fn nearest_build_site(
    origin: WorldPosition,
    perception: &PhysicalPerception,
) -> Option<WorldPosition> {
    cardinal_neighbors(origin)
        .filter(|candidate| traversable(perception, *candidate))
        .filter(|candidate| {
            !perception
                .agents
                .iter()
                .any(|agent| agent.position == *candidate)
        })
        .min_by_key(|candidate| (candidate.y, candidate.x))
}

const fn urgency_score(value: u16, threshold: u16) -> u32 {
    (value as u32).saturating_mul(10_000) / threshold as u32
}

const fn urgency_tie_rank(kind: NeedKind) -> u8 {
    match kind {
        NeedKind::Thirst => 3,
        NeedKind::Exposure => 2,
        NeedKind::Hunger => 1,
        NeedKind::Rest => 0,
    }
}

fn nearest_water_access(
    origin: WorldPosition,
    perception: &PhysicalPerception,
) -> Option<WorldPosition> {
    perception
        .drinkable_water
        .iter()
        .flat_map(|water| cardinal_neighbors(water.position))
        .filter(|candidate| traversable(perception, *candidate))
        .min_by_key(|candidate| target_key(origin, *candidate))
}

fn nearest_resource_access(
    origin: WorldPosition,
    perception: &PhysicalPerception,
    accepts: impl Fn(ResourceKind) -> bool,
) -> Option<WorldPosition> {
    perception
        .resources
        .iter()
        .filter(|resource| accepts(resource.resource.kind))
        .flat_map(|resource| {
            std::iter::once(resource.position).chain(cardinal_neighbors(resource.position))
        })
        .filter(|candidate| traversable(perception, *candidate))
        .min_by_key(|candidate| target_key(origin, *candidate))
}

fn cardinal_neighbors(position: WorldPosition) -> std::array::IntoIter<WorldPosition, 4> {
    [
        WorldPosition {
            x: position.x,
            y: position.y - 1,
        },
        WorldPosition {
            x: position.x - 1,
            y: position.y,
        },
        WorldPosition {
            x: position.x + 1,
            y: position.y,
        },
        WorldPosition {
            x: position.x,
            y: position.y + 1,
        },
    ]
    .into_iter()
}

fn traversable(perception: &PhysicalPerception, position: WorldPosition) -> bool {
    perception
        .traversable_cells
        .binary_search_by_key(&(position.y, position.x), |candidate| {
            (candidate.y, candidate.x)
        })
        .is_ok()
}

fn target_key(origin: WorldPosition, target: WorldPosition) -> (u64, i64, i64) {
    (
        origin.x.abs_diff(target.x) + origin.y.abs_diff(target.y),
        target.y,
        target.x,
    )
}

pub(crate) const fn retry_delay(retries: u8) -> u64 {
    let shift = if retries > 5 { 5 } else { retries };
    let delay = 60_u64 << shift;
    if delay > PHYSICAL_POLICY_MAX_BACKOFF_TICKS {
        PHYSICAL_POLICY_MAX_BACKOFF_TICKS
    } else {
        delay
    }
}

#[cfg(test)]
mod tests {
    use std::mem::{align_of, size_of};

    use super::*;
    use crate::{
        BaseResource, NeedLevelView, PerceivedResource, PerceivedWater, WaterSource, WorldRect,
    };

    #[test]
    fn policy_state_has_a_fixed_pointer_free_layout() {
        assert_eq!(size_of::<PolicyState>(), 12);
        assert_eq!(align_of::<PolicyState>(), 4);
        assert_eq!(size_of::<PolicyDiagnostic>(), 40);
        assert_eq!(align_of::<PolicyDiagnostic>(), 8);
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
    }
}
