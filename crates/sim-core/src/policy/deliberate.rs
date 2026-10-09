//! Memory-driven goal selection. Same needs and actions as the reactive policy,
//! but the agent also uses its mental map: it travels to remembered places that
//! are out of view, explores directions it has not visited, tops up water and
//! food before wandering, never explores farther from known water than it could
//! walk back, and points out places to agents nearby.

use super::{
    exploration::{exploration_target, varied_exploration_heading},
    selection::{
        PolicySelection, candidate_available, most_urgent, nearest_resource_access,
        nearest_shelter_access, nearest_water_access, shelter_selection,
    },
};
use crate::{
    InventoryView, NeedKind, PhysicalNeedsView, PhysicalPerception, ResourceKind, WorldPosition,
    cognition::{LandmarkKind, MentalMap},
    policy::{ExplorationHeading, PHYSICAL_POLICY_IDLE_RECHECK_TICKS, PhysicalGoal, PolicyReason},
    structures::SHELTER_WOOD_COST,
};

/// Below their thresholds, agents still drink above this thirst before wandering.
pub const TOP_UP_THIRST: u16 = 3_000;
/// Below its threshold, agents still eat carried food above this hunger.
pub const TOP_UP_HUNGER: u16 = 3_500;
/// Agents keep at least this much food in hand when food is in view.
pub const FOOD_RESERVE: u8 = 6;
/// Agents keep exploring until they know this many water and food places.
pub const CURIOSITY_TARGET: usize = 2;
/// Agents that know enough still take an excursion on one in this many idle checks.
pub const EXCURSION_EVERY: u64 = 3;
/// Without a known shelter, agents fetch wood for one once exposure passes this.
pub const PREPARE_EXPOSURE: u16 = 5_000;
/// A remembered shelter this close counts as home: tired agents walk back to it,
/// and agents don't build another one.
pub const HOME_RANGE: u64 = 200;
/// Exploration never shrinks the round-trip range below this many cells.
const MIN_LEASH: u64 = 24;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Deliberation {
    pub(crate) selection: PolicySelection,
    /// New persistent exploration heading when the agent explores or travels.
    pub(crate) heading: Option<ExplorationHeading>,
}

impl Deliberation {
    fn act(goal: PhysicalGoal, target: WorldPosition, reason: PolicyReason) -> Self {
        Self {
            selection: PolicySelection {
                goal,
                target: Some(target),
                reason,
            },
            heading: None,
        }
    }

    fn wait(origin: WorldPosition, reason: PolicyReason) -> Self {
        Self::act(PhysicalGoal::Wait, origin, reason)
    }
}

pub(crate) struct MindInput<'a> {
    pub(crate) map: &'a MentalMap,
    pub(crate) heading: ExplorationHeading,
    /// A remembered place worth pointing out, when the agent may gesture now
    /// and someone awake is watching.
    pub(crate) share_target: Option<WorldPosition>,
    /// Next spiral-search corner while the agent knows no water.
    pub(crate) search_target: Option<WorldPosition>,
}

pub(crate) fn deliberate(
    origin: WorldPosition,
    needs: PhysicalNeedsView,
    inventory: InventoryView,
    perception: &PhysicalPerception,
    mind: MindInput<'_>,
) -> Deliberation {
    let planner = Planner {
        origin,
        needs,
        perception,
        mind: &mind,
    };
    let water_here = nearest_water_access(origin, perception);
    let food_here = nearest_resource_access(origin, perception, |kind| {
        kind == ResourceKind::Food && inventory.can_add(kind)
    });
    match most_urgent(needs) {
        Some(NeedKind::Thirst) => water_here
            .map(|target| {
                Deliberation::act(
                    PhysicalGoal::SeekWater,
                    target,
                    PolicyReason::ThirstThreshold,
                )
            })
            .or_else(|| planner.travel_to_known(LandmarkKind::Water, PhysicalGoal::SeekWater))
            .or_else(|| planner.explore(PolicyReason::ThirstThreshold, false))
            .unwrap_or_else(|| Deliberation::wait(origin, PolicyReason::ThirstThreshold)),
        Some(NeedKind::Hunger) if inventory.food > 0 => {
            Deliberation::act(PhysicalGoal::Eat, origin, PolicyReason::HungerThreshold)
        }
        Some(NeedKind::Hunger) => food_here
            .map(|target| {
                Deliberation::act(
                    PhysicalGoal::SeekFood,
                    target,
                    PolicyReason::HungerThreshold,
                )
            })
            .or_else(|| planner.travel_to_known(LandmarkKind::Food, PhysicalGoal::SeekFood))
            .or_else(|| planner.explore(PolicyReason::HungerThreshold, true))
            .unwrap_or_else(|| Deliberation::wait(origin, PolicyReason::HungerThreshold)),
        Some(NeedKind::Rest) => planner.rest(inventory),
        Some(NeedKind::Exposure) => {
            let reactive = shelter_selection(
                origin,
                inventory,
                perception,
                PolicyReason::ExposureThreshold,
            );
            if reactive.goal != PhysicalGoal::Wait {
                return Deliberation {
                    selection: reactive,
                    heading: None,
                };
            }
            planner
                .travel_to_known(LandmarkKind::Shelter, PhysicalGoal::SeekShelter)
                .or_else(|| planner.gather_known_wood(inventory))
                .unwrap_or_else(|| Deliberation::wait(origin, PolicyReason::ExposureThreshold))
        }
        None => planner.calm(inventory, water_here, food_here),
    }
}

struct Planner<'a> {
    origin: WorldPosition,
    needs: PhysicalNeedsView,
    perception: &'a PhysicalPerception,
    mind: &'a MindInput<'a>,
}

impl Planner<'_> {
    /// Tired: sleep in a shelter if one is in view; otherwise on free open
    /// ground when exposure allows it; otherwise get to, build, or gather for a
    /// shelter, because sleeping in the open is refused once exposure is high.
    fn rest(&self, inventory: InventoryView) -> Deliberation {
        let reason = PolicyReason::RestThreshold;
        if let Some(access) = self.sleepable_shelter_access() {
            return Deliberation::act(PhysicalGoal::Sleep, access, reason);
        }
        if self.home_is_near()
            && let Some(home) =
                self.travel_to_known(LandmarkKind::Shelter, PhysicalGoal::SeekShelter)
        {
            return home.with_reason(reason);
        }
        if !self.needs.exposure.threshold_reached
            && let Some(spot) = self.free_sleeping_spot()
        {
            return Deliberation::act(PhysicalGoal::Sleep, spot, reason);
        }
        // Too exposed to sleep in the open: a shelter is the only way to rest.
        let reactive = shelter_selection(
            self.origin,
            inventory,
            self.perception,
            PolicyReason::ExposureThreshold,
        );
        if reactive.goal != PhysicalGoal::Wait {
            return Deliberation {
                selection: PolicySelection { reason, ..reactive },
                heading: None,
            };
        }
        self.travel_to_known(LandmarkKind::Shelter, PhysicalGoal::SeekShelter)
            .or_else(|| self.gather_known_wood(inventory))
            .or_else(|| self.explore(reason, false))
            .unwrap_or_else(|| Deliberation::wait(self.origin, reason))
    }

    fn home_is_near(&self) -> bool {
        self.mind
            .map
            .nearest_seen_distance(LandmarkKind::Shelter, self.origin)
            .is_some_and(|distance| distance <= HOME_RANGE)
    }

    /// Sleep needs a cell to itself and off any structure footprint (an agent
    /// can end up standing where someone later built).
    fn can_sleep_on(&self, cell: WorldPosition) -> bool {
        let me = self.needs.agent;
        candidate_available(self.origin, self.perception, cell)
            && self
                .perception
                .reserved_cells
                .binary_search_by_key(&(cell.y, cell.x), |reserved| (reserved.y, reserved.x))
                .is_err()
            && !self
                .perception
                .agents
                .iter()
                .any(|other| other.id != me && other.position == cell)
            && !self
                .perception
                .structures
                .iter()
                .any(|structure| structure.position == cell)
    }

    fn sleepable_shelter_access(&self) -> Option<WorldPosition> {
        self.perception
            .structures
            .iter()
            .filter(|structure| structure.state == crate::StructureState::Complete)
            .flat_map(|structure| {
                let at = structure.position;
                [(0, -1), (-1, 0), (1, 0), (0, 1)].map(|(dx, dy)| WorldPosition {
                    x: at.x + dx,
                    y: at.y + dy,
                })
            })
            .filter(|&cell| self.can_sleep_on(cell))
            .min_by_key(|&cell| (manhattan(self.origin, cell), cell.y, cell.x))
    }

    /// Where the agent stands if it can sleep there, else the nearest such cell in view.
    fn free_sleeping_spot(&self) -> Option<WorldPosition> {
        if self.can_sleep_on(self.origin) {
            return Some(self.origin);
        }
        self.perception
            .reachable_cells
            .iter()
            .copied()
            .filter(|&cell| self.can_sleep_on(cell))
            .min_by_key(|&cell| (manhattan(self.origin, cell), cell.y, cell.x))
    }

    /// No need is pressing: prepare, share, work, explore, or rest.
    fn calm(
        &self,
        inventory: InventoryView,
        water_here: Option<WorldPosition>,
        food_here: Option<WorldPosition>,
    ) -> Deliberation {
        let origin = self.origin;
        if self.needs.thirst.value >= TOP_UP_THIRST {
            if let Some(target) = water_here {
                return Deliberation::act(
                    PhysicalGoal::SeekWater,
                    target,
                    PolicyReason::PrepareTrip,
                );
            }
            if let Some(travel) = self.travel_to_known(LandmarkKind::Water, PhysicalGoal::SeekWater)
            {
                return travel.with_reason(PolicyReason::PrepareTrip);
            }
        }
        if self.needs.hunger.value >= TOP_UP_HUNGER && inventory.food > 0 {
            return Deliberation::act(PhysicalGoal::Eat, origin, PolicyReason::PrepareTrip);
        }
        if inventory.food < FOOD_RESERVE
            && let Some(target) = food_here
        {
            return Deliberation::act(PhysicalGoal::SeekFood, target, PolicyReason::PrepareTrip);
        }
        if let Some(place) = self.mind.share_target {
            return Deliberation::act(PhysicalGoal::Signal, place, PolicyReason::Sharing);
        }
        let shelter_in_view = nearest_shelter_access(origin, self.perception).is_some();
        if !shelter_in_view && self.home_is_near() {
            // Already has a home nearby: gather what's around instead of building another.
            if let Some(target) =
                nearest_resource_access(origin, self.perception, |kind| inventory.can_add(kind))
            {
                return Deliberation::act(
                    PhysicalGoal::GatherMaterial,
                    target,
                    PolicyReason::NoUrgentNeed,
                );
            }
        } else {
            let reactive = shelter_selection(
                origin,
                inventory,
                self.perception,
                PolicyReason::NoUrgentNeed,
            );
            if reactive.goal != PhysicalGoal::Wait {
                return Deliberation {
                    selection: reactive,
                    heading: None,
                };
            }
            if self.needs.exposure.value >= PREPARE_EXPOSURE
                && self.mind.map.seen_count(LandmarkKind::Shelter) == 0
                && let Some(wood) = self.gather_known_wood(inventory)
            {
                return wood.with_reason(PolicyReason::PrepareTrip);
            }
        }
        let map = self.mind.map;
        let curious = map.seen_count(LandmarkKind::Water) < CURIOSITY_TARGET
            || map.seen_count(LandmarkKind::Food) < CURIOSITY_TARGET;
        let excursion = (self.needs.at.ticks() / PHYSICAL_POLICY_IDLE_RECHECK_TICKS
            + u64::from(self.needs.agent.get()))
            % EXCURSION_EVERY
            == 0;
        if (curious || excursion)
            && let Some(explore) = self.explore(PolicyReason::NoUrgentNeed, true)
        {
            return explore;
        }
        Deliberation::wait(origin, PolicyReason::NoUrgentNeed)
    }

    /// Head for the best remembered place of `kind`, one visible waypoint at a time.
    fn travel_to_known(&self, kind: LandmarkKind, goal: PhysicalGoal) -> Option<Deliberation> {
        let (destination, source) =
            self.mind
                .map
                .recall(kind, self.needs.agent.get(), self.origin)?;
        let reason = match source {
            crate::LandmarkSource::Seen => PolicyReason::RememberedPlace,
            crate::LandmarkSource::Told => PolicyReason::ToldPlace,
        };
        let (waypoint, heading) = self.waypoint_toward(destination)?;
        Some(Deliberation {
            selection: PolicySelection {
                goal,
                target: Some(waypoint),
                reason,
            },
            heading: Some(heading),
        })
    }

    fn gather_known_wood(&self, inventory: InventoryView) -> Option<Deliberation> {
        (inventory.wood < SHELTER_WOOD_COST && inventory.can_add(ResourceKind::Wood))
            .then(|| self.travel_to_known(LandmarkKind::Wood, PhysicalGoal::GatherMaterial))
            .flatten()
            .map(|travel| travel.with_reason(PolicyReason::ShelterMaterials))
    }

    /// The reachable cell in view closest to `destination`, or a detour around
    /// whatever blocks the direct line.
    fn waypoint_toward(
        &self,
        destination: WorldPosition,
    ) -> Option<(WorldPosition, ExplorationHeading)> {
        let origin = self.origin;
        let heading = heading_toward(origin, destination);
        let current = manhattan(origin, destination);
        let direct = self
            .perception
            .reachable_cells
            .iter()
            .copied()
            .filter(|&cell| cell != origin && candidate_available(origin, self.perception, cell))
            .map(|cell| (manhattan(cell, destination), cell))
            .filter(|&(distance, _)| distance < current)
            .min_by_key(|&(distance, cell)| (distance, cell.y, cell.x));
        match direct {
            Some((_, cell)) => Some((cell, heading)),
            None => exploration_target(origin, self.perception, heading.rotated(2)),
        }
    }

    /// Wander toward unexplored ground. With `leashed`, stay within the range the
    /// agent could still walk back to remembered water before getting thirsty.
    fn explore(&self, reason: PolicyReason, leashed: bool) -> Option<Deliberation> {
        let origin = self.origin;
        if let Some(corner) = self.mind.search_target {
            let (waypoint, heading) = self.waypoint_toward(corner)?;
            return Some(Deliberation {
                selection: PolicySelection {
                    goal: PhysicalGoal::Explore,
                    target: Some(waypoint),
                    reason,
                },
                heading: Some(heading),
            });
        }
        let map = self.mind.map;
        let preferred = map
            .novel_heading(origin, self.mind.heading)
            .unwrap_or_else(|| {
                varied_exploration_heading(self.needs.agent, origin, self.mind.heading)
            });
        let (target, heading) = exploration_target(origin, self.perception, preferred)?;
        if leashed {
            let thirst = self.needs.thirst;
            let leash =
                (u64::from(thirst.threshold.saturating_sub(thirst.value)) / 2).max(MIN_LEASH);
            let from_target = map.nearest_seen_distance(LandmarkKind::Water, target);
            let from_here = map.nearest_seen_distance(LandmarkKind::Water, origin);
            if let (Some(from_target), Some(from_here)) = (from_target, from_here)
                && from_target > leash
                && from_target >= from_here
            {
                return self
                    .travel_to_known(LandmarkKind::Water, PhysicalGoal::Explore)
                    .map(|travel| travel.with_reason(PolicyReason::Returning));
            }
        }
        Some(Deliberation {
            selection: PolicySelection {
                goal: PhysicalGoal::Explore,
                target: Some(target),
                reason,
            },
            heading: Some(heading),
        })
    }
}

impl Deliberation {
    fn with_reason(mut self, reason: PolicyReason) -> Self {
        self.selection.reason = reason;
        self
    }
}

fn manhattan(left: WorldPosition, right: WorldPosition) -> u64 {
    left.x.abs_diff(right.x) + left.y.abs_diff(right.y)
}

/// The 8-way direction that best matches the vector to `destination`.
pub(crate) fn heading_toward(
    origin: WorldPosition,
    destination: WorldPosition,
) -> ExplorationHeading {
    let (dx, dy) = (destination.x - origin.x, destination.y - origin.y);
    let (ax, ay) = (dx.unsigned_abs(), dy.unsigned_abs());
    let horizontal = if ax > 2 * ay { Some(dx > 0) } else { None };
    let vertical = if ay > 2 * ax { Some(dy > 0) } else { None };
    match (horizontal, vertical, dx > 0, dy > 0) {
        (Some(true), _, _, _) => ExplorationHeading::East,
        (Some(false), _, _, _) => ExplorationHeading::West,
        (_, Some(true), _, _) => ExplorationHeading::South,
        (_, Some(false), _, _) => ExplorationHeading::North,
        (None, None, true, true) => ExplorationHeading::SouthEast,
        (None, None, true, false) => ExplorationHeading::NorthEast,
        (None, None, false, true) => ExplorationHeading::SouthWest,
        (None, None, false, false) => ExplorationHeading::NorthWest,
    }
}
