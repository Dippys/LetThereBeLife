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
    Explore = 10,
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
    pub reason: PolicyReason,
    pub target: Option<WorldPosition>,
    pub committed: bool,
    pub retry_count: u8,
    pub exploration_heading: ExplorationHeading,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ExplorationHeading {
    North = 0,
    NorthEast = 1,
    East = 2,
    SouthEast = 3,
    South = 4,
    SouthWest = 5,
    West = 6,
    NorthWest = 7,
}

impl ExplorationHeading {
    const fn from_rank(rank: u8) -> Self {
        match rank & 7 {
            0 => Self::North,
            1 => Self::NorthEast,
            2 => Self::East,
            3 => Self::SouthEast,
            4 => Self::South,
            5 => Self::SouthWest,
            6 => Self::West,
            _ => Self::NorthWest,
        }
    }

    const fn rotated(self, offset: i8) -> Self {
        Self::from_rank((self as i8).wrapping_add(offset) as u8)
    }

    const fn delta(self) -> (i64, i64) {
        match self {
            Self::North => (0, -1),
            Self::NorthEast => (1, -1),
            Self::East => (1, 0),
            Self::SouthEast => (1, 1),
            Self::South => (0, 1),
            Self::SouthWest => (-1, 1),
            Self::West => (-1, 0),
            Self::NorthWest => (-1, -1),
        }
    }
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
#[repr(transparent)]
pub(crate) struct PolicyNavigation(u8);

impl PolicyNavigation {
    const PHASE_MASK: u8 = 0b111;
    const HEADING_SHIFT: u8 = 3;

    const fn new(phase: PolicyPhase, heading: ExplorationHeading) -> Self {
        Self((phase as u8) | ((heading as u8) << Self::HEADING_SHIFT))
    }

    pub(crate) const fn phase(self) -> PolicyPhase {
        match self.0 & Self::PHASE_MASK {
            0 => PolicyPhase::Dormant,
            1 => PolicyPhase::DecisionPending,
            2 => PolicyPhase::Routing,
            3 => PolicyPhase::Acting,
            _ => PolicyPhase::Backoff,
        }
    }

    pub(crate) const fn heading(self) -> ExplorationHeading {
        ExplorationHeading::from_rank(self.0 >> Self::HEADING_SHIFT)
    }

    pub(crate) fn set_phase(&mut self, phase: PolicyPhase) {
        self.0 = (self.0 & !Self::PHASE_MASK) | phase as u8;
    }

    pub(crate) fn set_heading(&mut self, heading: ExplorationHeading) {
        self.0 = (self.0 & Self::PHASE_MASK) | ((heading as u8) << Self::HEADING_SHIFT);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub(crate) struct PolicyState {
    pub(crate) target: CompactPosition,
    pub(crate) generation: u32,
    pub(crate) navigation: PolicyNavigation,
    pub(crate) goal: PhysicalGoal,
    pub(crate) retries: u8,
    pub(crate) reason: PolicyReason,
}

impl Default for PolicyState {
    fn default() -> Self {
        Self {
            target: CompactPosition { x: 0, y: 0 },
            generation: 0,
            navigation: PolicyNavigation::new(PolicyPhase::Dormant, ExplorationHeading::North),
            goal: PhysicalGoal::Wait,
            retries: 0,
            reason: PolicyReason::InitialDecision,
        }
    }
}

impl PolicyState {
    pub(crate) fn for_agent(agent: AgentId) -> Self {
        let mut state = Self::default();
        let mut key = agent.get().wrapping_mul(0x9e37_79b9);
        key ^= key >> 16;
        state
            .navigation
            .set_heading(ExplorationHeading::from_rank(key as u8));
        state
    }

    pub(crate) const fn phase(self) -> PolicyPhase {
        self.navigation.phase()
    }

    pub(crate) fn set_phase(&mut self, phase: PolicyPhase) {
        self.navigation.set_phase(phase);
    }

    pub(crate) const fn exploration_heading(self) -> ExplorationHeading {
        self.navigation.heading()
    }

    pub(crate) fn set_exploration_heading(&mut self, heading: ExplorationHeading) {
        self.navigation.set_heading(heading);
    }

    pub(crate) fn view(self, agent: AgentId) -> PhysicalPolicyView {
        let phase = self.phase();
        PhysicalPolicyView {
            agent,
            goal: self.goal,
            reason: self.reason,
            target: matches!(phase, PolicyPhase::Routing | PolicyPhase::Acting)
                .then(|| self.target.world()),
            committed: matches!(phase, PolicyPhase::Routing | PolicyPhase::Acting),
            retry_count: self.retries,
            exploration_heading: self.exploration_heading(),
        }
    }

    pub(crate) fn next_generation(&mut self) -> Option<u32> {
        self.generation = self.generation.checked_add(1)?;
        Some(self.generation)
    }

    pub(crate) const fn event_is_current(self, generation: u32) -> bool {
        self.generation == generation
            && matches!(
                self.phase(),
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

#[cfg(test)]
pub(crate) fn select(
    origin: WorldPosition,
    needs: PhysicalNeedsView,
    inventory: InventoryView,
    perception: &PhysicalPerception,
) -> PolicySelection {
    select_with_exploration(origin, needs, inventory, perception, None).0
}

pub(crate) fn select_with_exploration(
    origin: WorldPosition,
    needs: PhysicalNeedsView,
    inventory: InventoryView,
    perception: &PhysicalPerception,
    exploration_heading: Option<ExplorationHeading>,
) -> (PolicySelection, Option<ExplorationHeading>) {
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

    let selection = match urgent.map(|(_, _, kind)| kind) {
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
    };
    let Some(heading) = exploration_heading else {
        return (selection, None);
    };
    if (selection.target.is_some() && selection.goal != PhysicalGoal::Wait)
        || (selection.goal == PhysicalGoal::Wait && is_safe_anchor(origin, perception))
    {
        return (selection, None);
    }
    let heading = varied_exploration_heading(needs.agent, origin, heading);
    exploration_target(origin, perception, heading).map_or(
        (selection, None),
        |(target, heading)| {
            (
                PolicySelection {
                    goal: PhysicalGoal::Explore,
                    target: Some(target),
                    reason: selection.reason,
                },
                Some(heading),
            )
        },
    )
}

fn is_safe_anchor(origin: WorldPosition, perception: &PhysicalPerception) -> bool {
    perception
        .drinkable_water
        .iter()
        .any(|water| origin.x.abs_diff(water.position.x) + origin.y.abs_diff(water.position.y) <= 1)
        || nearest_shelter_access(origin, perception) == Some(origin)
}

fn varied_exploration_heading(
    agent: AgentId,
    origin: WorldPosition,
    heading: ExplorationHeading,
) -> ExplorationHeading {
    let mut key = u64::from(agent.get()) ^ (origin.x as u64).rotate_left(17);
    key ^= (origin.y as u64).rotate_left(41);
    key = (key ^ (key >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    key = (key ^ (key >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    key ^= key >> 31;
    let turn = match key >> 61 {
        0 => -2,
        1 | 2 => -1,
        3..=5 => 0,
        6 => 1,
        _ => 2,
    };
    heading.rotated(turn)
}

fn exploration_target(
    origin: WorldPosition,
    perception: &PhysicalPerception,
    heading: ExplorationHeading,
) -> Option<(WorldPosition, ExplorationHeading)> {
    [0_i8, 1, -1, 2, -2, 3, -3, 4].into_iter().find_map(|turn| {
        let heading = heading.rotated(turn);
        let (heading_x, heading_y) = heading.delta();
        perception
            .reachable_cells
            .iter()
            .copied()
            .filter(|candidate| *candidate != origin)
            .filter(|candidate| candidate_available(origin, perception, *candidate))
            .filter_map(|candidate| {
                let dx = candidate.x - origin.x;
                let dy = candidate.y - origin.y;
                let projection = dx * heading_x + dy * heading_y;
                (projection > 0).then(|| {
                    let lateral = (dx * heading_y - dy * heading_x).unsigned_abs();
                    let distance = dx.unsigned_abs() + dy.unsigned_abs();
                    (candidate, projection, lateral, distance)
                })
            })
            .max_by_key(|(candidate, projection, lateral, distance)| {
                (
                    *projection,
                    u64::MAX - *lateral,
                    *distance,
                    candidate.y,
                    candidate.x,
                )
            })
            .map(|(target, _, _, _)| (target, heading))
    })
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
        .filter(|candidate| candidate_available(origin, perception, *candidate))
        .min_by_key(|candidate| target_key(origin, *candidate))
}

fn nearest_build_site(
    origin: WorldPosition,
    perception: &PhysicalPerception,
) -> Option<WorldPosition> {
    cardinal_neighbors(origin)
        .filter(|candidate| candidate_available(origin, perception, *candidate))
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
        .filter(|candidate| candidate_available(origin, perception, *candidate))
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
        .filter(|candidate| candidate_available(origin, perception, *candidate))
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
        .reachable_cells
        .binary_search_by_key(&(position.y, position.x), |candidate| {
            (candidate.y, candidate.x)
        })
        .is_ok()
}

fn candidate_available(
    origin: WorldPosition,
    perception: &PhysicalPerception,
    position: WorldPosition,
) -> bool {
    traversable(perception, position)
        && (position == origin
            || (!perception
                .agents
                .iter()
                .any(|agent| agent.position == position)
                && perception
                    .claimed_targets
                    .binary_search_by_key(&(position.y, position.x), |target| (target.y, target.x))
                    .is_err()))
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
}
