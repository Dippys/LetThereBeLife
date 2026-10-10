//! Memory-driven goal selection. Same needs and actions as the reactive policy,
//! but the agent also uses its mental map: it travels to remembered places that
//! are out of view, explores directions it has not visited, tops up water and
//! food before wandering, never explores farther from known water than it could
//! walk back, and points out places to agents nearby. Personality tunes every
//! threshold (an all-average personality matches the original thresholds),
//! and sociable agents visit friends and stay with company.

use super::{
    exploration::{exploration_target, varied_exploration_heading},
    selection::{
        PolicySelection, candidate_available, most_urgent, nearest_build_site,
        nearest_resource_access, nearest_shelter_access, nearest_water_access, shelter_selection,
    },
};
use crate::{
    InventoryView, Material, NeedKind, PhysicalNeedsView, PhysicalPerception, WorldPosition,
    cognition::{LandmarkKind, MentalMap, Personality},
    policy::{
        ExplorationHeading, FoodValues, PHYSICAL_POLICY_IDLE_RECHECK_TICKS, PhysicalGoal,
        PolicyReason,
    },
    structures::SHELTER_WOOD_COST,
};

/// Personality-dependent thresholds for one decision. Average traits (128)
/// give: top up thirst at ~3,000 and hunger at ~3,500, keep 6 food, explore
/// until 2 water and 2 food places are known, take an excursion on ~1/3 of idle
/// checks, prepare shelter at exposure ~5,000, and roam half the walk-back range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Temperament {
    pub(crate) top_up_thirst: u16,
    pub(crate) top_up_hunger: u16,
    pub(crate) food_reserve: u8,
    pub(crate) curiosity_target: usize,
    /// Chance out of 256 that an idle check becomes an excursion.
    pub(crate) excursion_chance: u16,
    pub(crate) prepare_exposure: u16,
    /// Percent of the thirst headroom an explorer may spend getting away from water.
    pub(crate) leash_percent: u64,
    /// Chance out of 256 that a calm agent works (gathers, builds) instead of moving on.
    pub(crate) work_chance: u16,
    /// Chance out of 256 that a lonely agent goes looking for a friend.
    pub(crate) visit_chance: u16,
    /// Agents this sociable stay with company instead of wandering off.
    pub(crate) stays_with_company: bool,
    /// Chance out of 256 that an idle agent goes to look at a place someone
    /// pointed out nearby.
    pub(crate) nosy_chance: u16,
}

impl Temperament {
    pub(crate) fn of(personality: Personality) -> Self {
        let Personality {
            curiosity,
            caution,
            sociability,
            diligence,
        } = personality;
        Self {
            top_up_thirst: Personality::scale(caution, 4_000, 2_000) as u16,
            top_up_hunger: Personality::scale(caution, 4_500, 2_500) as u16,
            food_reserve: Personality::scale(caution, 3, 9) as u8,
            curiosity_target: Personality::scale(curiosity, 1, 3) as usize,
            excursion_chance: Personality::scale(curiosity, 30, 140) as u16,
            prepare_exposure: Personality::scale(caution, 6_000, 4_000) as u16,
            leash_percent: Personality::scale(caution, 65, 35) as u64,
            work_chance: Personality::scale(diligence, 0, 512).min(256) as u16,
            visit_chance: Personality::scale(sociability, 0, 200) as u16,
            stays_with_company: sociability >= 160,
            nosy_chance: Personality::scale(curiosity, 24, 120) as u16,
        }
    }
}

/// A remembered shelter this close counts as home: tired agents walk back to it,
/// and agents don't build another one.
pub const HOME_RANGE: u64 = 200;
/// A hearth is built only this close (cells) to the builder's home shelter.
const HEARTH_FROM_HOME: u64 = 16;
/// Tiredness (out of 10,000) at which an agent sleeps wherever it can.
const REST_CRITICAL: u16 = 8_800;
/// Idle agents go to look at places pointed out within this many cells.
const NOSY_RANGE: u64 = 48;
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
    pub(crate) personality: Personality,
    /// Someone awake is in view.
    pub(crate) company: bool,
    /// Where a friend was last seen, offered only when the agent is alone.
    pub(crate) friend_target: Option<WorldPosition>,
    /// Where someone it has something to tell (a correction) was last seen.
    pub(crate) seek: Option<WorldPosition>,
    /// For a child: whether to stay put or go back to where its parent was
    /// last seen. Children take no excursions.
    pub(crate) parent: Option<ParentInput>,
    /// Someone in view worth asking for food (offered only when the agent may ask).
    pub(crate) beg_target: Option<WorldPosition>,
    /// What the agent believes is worth eating.
    pub(crate) food: FoodValues,
    /// The nearest animal in view it believes is dangerous, if close.
    pub(crate) danger: Option<WorldPosition>,
    /// The nearest animal in view it believes is dangerous, at any distance.
    pub(crate) wary: Option<WorldPosition>,
    /// The nearest animal in view it believes is worth hunting.
    pub(crate) prey: Option<WorldPosition>,
    /// Where someone called it to come and hunt (when nothing is in view).
    pub(crate) quarry: Option<WorldPosition>,
    /// A dangerous animal in view worth warning the people nearby about.
    pub(crate) warn: Option<WorldPosition>,
    /// An animal in view worth calling the people nearby to hunt.
    pub(crate) recruit: Option<WorldPosition>,
    /// It believes a hearth would warm it (and knows how to build one).
    pub(crate) knows_hearths: bool,
    /// Where it stood at its previous decision (it won't step straight back).
    pub(crate) came_from: Option<WorldPosition>,
    /// Mourning someone close: stays near others, takes on no work or trips.
    pub(crate) grieving: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ParentInput {
    /// The parent is in view, or its whereabouts are unknown: stay near the band.
    Stay,
    /// Go back to where the parent was last seen.
    Return(WorldPosition),
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
        temperament: Temperament::of(mind.personality),
    };
    // Danger comes before everything else: warn the others unless it's already
    // close, then get away.
    let threat_close = mind
        .danger
        .is_some_and(|threat| origin.x.abs_diff(threat.x).max(origin.y.abs_diff(threat.y)) <= 3);
    if !threat_close && let Some(target) = mind.warn {
        return Deliberation::act(PhysicalGoal::Signal, target, PolicyReason::Warning);
    }
    if let Some(threat) = mind.danger
        && let Some(escape) = planner.flee_from(threat)
    {
        return escape;
    }
    let water_here = nearest_water_access(origin, perception);
    let food = mind.food;
    let food_here = nearest_resource_access(origin, perception, |kind| {
        food.is_food(kind) && inventory.can_add(kind)
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
        Some(NeedKind::Hunger) if food.carried(inventory) > 0 => {
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
            .or_else(|| planner.hunt(inventory))
            .or_else(|| planner.travel_to_food(PhysicalGoal::SeekFood))
            .or_else(|| {
                // Knowing no food anywhere, ask someone nearby before searching blind.
                mind.beg_target.map(|giver| {
                    Deliberation::act(PhysicalGoal::Signal, giver, PolicyReason::Begging)
                })
            })
            .or_else(|| planner.explore(PolicyReason::HungerThreshold, true))
            .unwrap_or_else(|| Deliberation::wait(origin, PolicyReason::HungerThreshold)),
        Some(NeedKind::Rest) => planner.rest(inventory),
        Some(NeedKind::Exposure) if mind.knows_hearths && planner.warm_up().is_some() => {
            planner.warm_up().expect("checked")
        }
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
                .or_else(|| {
                    mind.knows_hearths
                        .then(|| {
                            planner.travel_to_known(LandmarkKind::Hearth, PhysicalGoal::Explore)
                        })
                        .flatten()
                        .map(|trip| trip.with_reason(PolicyReason::Warming))
                })
                .or_else(|| planner.gather_known_wood(inventory))
                .unwrap_or_else(|| Deliberation::wait(origin, PolicyReason::ExposureThreshold))
        }
        None => planner.calm(inventory, water_here, food_here),
    }
}

struct Planner<'a> {
    temperament: Temperament,
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
        // Worn out with the weather mild, or about to collapse whatever the
        // weather: sleep here rather than trek home.
        let rest = self.needs.rest.value;
        let mild = !self.needs.exposure.threshold_reached;
        if ((rest >= REST_CRITICAL && mild) || rest >= crate::REST_COLLAPSE)
            && let Some(spot) = self.free_sleeping_spot()
        {
            return Deliberation::act(PhysicalGoal::Sleep, spot, reason);
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
            .filter(|structure| {
                structure.state == crate::StructureState::Complete
                    && structure.kind == crate::StructureKind::Shelter
            })
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
        let temperament = self.temperament;
        if self.needs.thirst.value >= temperament.top_up_thirst {
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
        let food = self.mind.food;
        if self.needs.hunger.value >= temperament.top_up_hunger && food.carried(inventory) > 0 {
            return Deliberation::act(PhysicalGoal::Eat, origin, PolicyReason::PrepareTrip);
        }
        if food.carried(inventory) < temperament.food_reserve {
            if let Some(target) = food_here {
                return Deliberation::act(
                    PhysicalGoal::SeekFood,
                    target,
                    PolicyReason::PrepareTrip,
                );
            }
            // Stock up from a remembered food place, seen or pointed out.
            if let Some(trip) = self.travel_to_food(PhysicalGoal::SeekFood) {
                return trip.with_reason(PolicyReason::PrepareTrip);
            }
        }
        // Answering a call to hunt comes before chores and chatter.
        if self.mind.quarry.is_some()
            && let Some(hunt) = self.hunt(inventory)
        {
            return hunt;
        }
        if let Some(place) = self.mind.share_target {
            return Deliberation::act(PhysicalGoal::Signal, place, PolicyReason::Sharing);
        }
        let grieving = self.mind.grieving;
        let works = !grieving && self.roll(1) < temperament.work_chance;
        // A bit chilly with a fire in view: warm up while it's easy.
        if self.mind.knows_hearths
            && self.needs.exposure.value >= temperament.prepare_exposure / 3
            && let Some(warm) = self.warm_up()
        {
            return warm.with_reason(PolicyReason::PrepareTrip);
        }
        // Hunting is work too, and prey in view is the best work there is.
        if works && let Some(hunt) = self.hunt(inventory) {
            return hunt;
        }
        // Someone who keeps fire and has none near home builds one.
        if works && let Some(hearth) = self.make_hearth(inventory) {
            return hearth;
        }
        let shelter_in_view = nearest_shelter_access(origin, self.perception).is_some();
        if !works {
            // Not in the mood for work: skip to friends, needed exploration, or rest.
        } else if !shelter_in_view && self.home_is_near() {
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
            if self.needs.exposure.value >= temperament.prepare_exposure
                && self.mind.map.seen_count(LandmarkKind::Shelter) == 0
                && let Some(wood) = self.gather_known_wood(inventory)
            {
                return wood.with_reason(PolicyReason::PrepareTrip);
            }
        }
        if let Some(ParentInput::Return(parent)) = self.mind.parent
            && let Some((waypoint, heading)) = self.waypoint_toward(parent)
        {
            return Deliberation {
                selection: PolicySelection {
                    goal: PhysicalGoal::Explore,
                    target: Some(waypoint),
                    reason: PolicyReason::Following,
                },
                heading: Some(heading),
            };
        }
        // Someone needs setting straight: go and find them.
        if let Some(person) = self.mind.seek
            && let Some((waypoint, heading)) = self.errand_toward(person)
        {
            return Deliberation {
                selection: PolicySelection {
                    goal: PhysicalGoal::Explore,
                    target: Some(waypoint),
                    reason: PolicyReason::Visiting,
                },
                heading: Some(heading),
            };
        }
        if let Some(friend) = self.mind.friend_target
            && (grieving || self.roll(2) < temperament.visit_chance)
            && let Some((waypoint, heading)) = self.errand_toward(friend)
        {
            return Deliberation {
                selection: PolicySelection {
                    goal: PhysicalGoal::Explore,
                    target: Some(waypoint),
                    reason: PolicyReason::Visiting,
                },
                heading: Some(heading),
            };
        }
        let map = self.mind.map;
        let food_places: usize = LandmarkKind::ALL
            .into_iter()
            .filter(|kind| {
                kind.material()
                    .is_some_and(|material| food.is_food(material))
            })
            .map(|kind| map.seen_count(kind))
            .sum();
        let curious = map.seen_count(LandmarkKind::Water) < temperament.curiosity_target
            || food_places < temperament.curiosity_target;
        // Lounging agents (no mood for work) don't take casual excursions either.
        let excursion = works
            && self.roll(3) < temperament.excursion_chance
            && !(self.mind.company && temperament.stays_with_company)
            && self.mind.parent.is_none();
        if curious || excursion {
            // Curiosity first checks what others have pointed out.
            if let Some(hint) = map.hint_to_check(self.needs.agent.get(), origin)
                && self.within_leash(hint)
                && let Some((waypoint, heading)) = self.errand_toward(hint)
            {
                return Deliberation {
                    selection: PolicySelection {
                        goal: PhysicalGoal::Explore,
                        target: Some(waypoint),
                        reason: PolicyReason::ToldPlace,
                    },
                    heading: Some(heading),
                };
            }
            if let Some(explore) = self.explore(PolicyReason::NoUrgentNeed, true) {
                return explore;
            }
        }
        // Nothing to do: go and see what someone pointed out close by.
        if !grieving
            && let Some(hint) = map.hint_to_check(self.needs.agent.get(), origin)
            && manhattan(origin, hint) <= NOSY_RANGE
            && self.roll(4) < temperament.nosy_chance
            && self.within_leash(hint)
            && let Some((waypoint, heading)) = self.errand_toward(hint)
        {
            return Deliberation {
                selection: PolicySelection {
                    goal: PhysicalGoal::Explore,
                    target: Some(waypoint),
                    reason: PolicyReason::ToldPlace,
                },
                heading: Some(heading),
            };
        }
        Deliberation::wait(origin, PolicyReason::NoUrgentNeed)
    }

    /// Warms up at the hearth in view: right away if it's beside the agent,
    /// otherwise walks over.
    fn warm_up(&self) -> Option<Deliberation> {
        let origin = self.origin;
        let hearth = self
            .perception
            .structures
            .iter()
            .filter(|structure| {
                structure.state == crate::StructureState::Complete
                    && structure.kind == crate::StructureKind::Hearth
            })
            .min_by_key(|structure| (manhattan(origin, structure.position), structure.id.get()))?
            .position;
        if origin.x.abs_diff(hearth.x).max(origin.y.abs_diff(hearth.y)) <= 1 {
            return Some(Deliberation::act(
                PhysicalGoal::WarmUp,
                origin,
                PolicyReason::Warming,
            ));
        }
        let (waypoint, heading) = self.waypoint_toward(hearth)?;
        Some(Deliberation {
            selection: PolicySelection {
                goal: PhysicalGoal::Explore,
                target: Some(waypoint),
                reason: PolicyReason::Warming,
            },
            heading: Some(heading),
        })
    }

    /// Builds a hearth near home if it keeps fire and knows of none close by,
    /// gathering stone and wood for it first.
    fn make_hearth(&self, inventory: InventoryView) -> Option<Deliberation> {
        let map = self.mind.map;
        let at_home = map
            .nearest_seen_distance(LandmarkKind::Shelter, self.origin)
            .is_some_and(|distance| distance <= HEARTH_FROM_HOME);
        if !self.mind.knows_hearths
            || !at_home
            || map.remembers_near(LandmarkKind::Hearth, self.origin, HOME_RANGE)
        {
            return None;
        }
        let kind = crate::StructureKind::Hearth;
        if kind.affordable(inventory) {
            let site = nearest_build_site(self.origin, self.perception)?;
            return Some(Deliberation::act(
                PhysicalGoal::BuildHearth,
                site,
                PolicyReason::NoUrgentNeed,
            ));
        }
        let missing = kind
            .cost()
            .into_iter()
            .find(|&(material, amount)| inventory.amount(material) < amount)?
            .0;
        if let Some(target) = nearest_resource_access(self.origin, self.perception, |found| {
            found == missing && inventory.can_add(found)
        }) {
            return Some(Deliberation::act(
                PhysicalGoal::GatherMaterial,
                target,
                PolicyReason::HearthMaterials,
            ));
        }
        LandmarkKind::of_material(missing)
            .and_then(|place| self.travel_to_known(place, PhysicalGoal::GatherMaterial))
            .map(|trip| trip.with_reason(PolicyReason::HearthMaterials))
    }

    /// Moves to the reachable cell in view farthest from `threat`.
    fn flee_from(&self, threat: WorldPosition) -> Option<Deliberation> {
        let origin = self.origin;
        let (_, cell) = self
            .perception
            .reachable_cells
            .iter()
            .copied()
            .filter(|&cell| cell != origin && candidate_available(origin, self.perception, cell))
            .map(|cell| (manhattan(cell, threat), cell))
            .filter(|&(distance, _)| distance > manhattan(origin, threat))
            .max_by_key(|&(distance, cell)| (distance, -cell.y, -cell.x))?;
        Some(Deliberation {
            selection: PolicySelection {
                goal: PhysicalGoal::Explore,
                target: Some(cell),
                reason: PolicyReason::Fleeing,
            },
            heading: Some(heading_toward(origin, cell)),
        })
    }

    /// Strikes at the prey in view if it's in reach, else closes in. Never
    /// chases beyond the range it could walk back to water from, nor while thirsty.
    fn hunt(&self, inventory: InventoryView) -> Option<Deliberation> {
        let prey = self.mind.prey.or(self.mind.quarry)?;
        // Nobody goes after more meat while carrying some or with a carcass in view.
        let meat_in_view = self
            .perception
            .resources
            .iter()
            .any(|resource| resource.resource.kind == crate::Material::Meat);
        if inventory.amount(crate::Material::Meat) > 0 || meat_in_view {
            return None;
        }
        // A chase is hard work: not while thirsty, worn out, or cold, and never
        // beyond the range it could walk back to water from.
        let needs = self.needs;
        let worn = |level: crate::NeedLevelView| {
            u32::from(level.value) * 4 >= u32::from(level.threshold) * 3
        };
        if self.needs.thirst.value >= self.temperament.top_up_thirst
            || worn(needs.rest)
            || worn(needs.exposure)
            || !self.within_leash(prey)
        {
            return None;
        }
        // Hunting goes better together: call the others first.
        if let Some(target) = self.mind.recruit {
            return Some(Deliberation::act(
                PhysicalGoal::Signal,
                target,
                PolicyReason::Recruiting,
            ));
        }
        let origin = self.origin;
        if origin.x.abs_diff(prey.x).max(origin.y.abs_diff(prey.y)) <= crate::STRIKE_RANGE as u64 {
            return Some(Deliberation::act(
                PhysicalGoal::Hunt,
                prey,
                PolicyReason::Hunting,
            ));
        }
        let (waypoint, heading) = self.waypoint_toward(prey)?;
        Some(Deliberation {
            selection: PolicySelection {
                goal: PhysicalGoal::Explore,
                target: Some(waypoint),
                reason: PolicyReason::Hunting,
            },
            heading: Some(heading),
        })
    }

    /// A deterministic 0–255 roll for this agent and idle-check window.
    fn roll(&self, salt: u64) -> u16 {
        let mut key = (u64::from(self.needs.agent.get()) << 32)
            ^ (self.needs.at.ticks() / PHYSICAL_POLICY_IDLE_RECHECK_TICKS)
            ^ salt.wrapping_mul(0x9e37_79b9_7f4a_7c15);
        key = (key ^ (key >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        key = (key ^ (key >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        ((key ^ (key >> 31)) & 0xff) as u16
    }

    /// Head for the best remembered place of `kind`, one visible waypoint at a time.
    fn travel_to_known(&self, kind: LandmarkKind, goal: PhysicalGoal) -> Option<Deliberation> {
        self.travel_to_best(&[kind], goal)
    }

    /// Head for the best remembered place of anything the agent considers food.
    fn travel_to_food(&self, goal: PhysicalGoal) -> Option<Deliberation> {
        let food = self.mind.food;
        let kinds: Vec<LandmarkKind> = LandmarkKind::ALL
            .into_iter()
            .filter(|kind| {
                kind.material()
                    .is_some_and(|material| food.is_food(material))
            })
            .collect();
        self.travel_to_best(&kinds, goal)
    }

    /// Head for the best remembered place among `kinds`. When the need behind the
    /// trip is already urgent, a place it has seen itself beats any hint: there's
    /// no time to search.
    fn travel_to_best(&self, kinds: &[LandmarkKind], goal: PhysicalGoal) -> Option<Deliberation> {
        let now = (self.needs.at.ticks() / 60) as u32;
        let urgent = match goal {
            PhysicalGoal::SeekWater => self.needs.thirst.threshold_reached,
            PhysicalGoal::SeekFood => self.needs.hunger.threshold_reached,
            _ => false,
        };
        let best = |seen_only: bool| {
            kinds
                .iter()
                .filter_map(|&kind| {
                    self.mind.map.recall_scored(
                        kind,
                        self.needs.agent.get(),
                        self.origin,
                        now,
                        seen_only,
                    )
                })
                .min_by_key(|&(score, destination, _)| (score, destination.y, destination.x))
        };
        let (_, destination, source) = urgent
            .then(|| best(true))
            .flatten()
            .or_else(|| best(false))?;
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
        (inventory.amount(Material::Wood) < SHELTER_WOOD_COST && inventory.can_add(Material::Wood))
            .then(|| self.travel_to_known(LandmarkKind::Wood, PhysicalGoal::GatherMaterial))
            .flatten()
            .map(|travel| travel.with_reason(PolicyReason::ShelterMaterials))
    }

    /// The reachable cell in view closest to `destination`, or a detour around
    /// whatever blocks the direct line.
    /// A waypoint for an errand that can wait (a visit, a look at a tip): none
    /// when it would lead toward a dangerous animal in view.
    fn errand_toward(
        &self,
        destination: WorldPosition,
    ) -> Option<(WorldPosition, ExplorationHeading)> {
        let (waypoint, heading) = self.waypoint_toward(destination)?;
        let closer =
            |threat: WorldPosition| manhattan(waypoint, threat) < manhattan(self.origin, threat);
        (!self.mind.wary.is_some_and(closer)).then_some((waypoint, heading))
    }

    fn waypoint_toward(
        &self,
        destination: WorldPosition,
    ) -> Option<(WorldPosition, ExplorationHeading)> {
        let origin = self.origin;
        let heading = heading_toward(origin, destination);
        let current = manhattan(origin, destination);
        let back = self.mind.came_from;
        let direct = self
            .perception
            .reachable_cells
            .iter()
            .copied()
            .filter(|&cell| {
                cell != origin
                    && back.is_none_or(|back| manhattan(cell, back) > 1)
                    && candidate_available(origin, self.perception, cell)
            })
            .map(|cell| (manhattan(cell, destination), cell))
            .filter(|&(distance, _)| distance < current)
            .min_by_key(|&(distance, cell)| (distance, cell.y, cell.x));
        match direct {
            Some((_, cell)) => Some((cell, heading)),
            None => exploration_target(origin, self.perception, heading.rotated(2)),
        }
    }

    /// Whether going to `place` keeps the agent within walking range of known water.
    fn within_leash(&self, place: WorldPosition) -> bool {
        let thirst = self.needs.thirst;
        let headroom = u64::from(thirst.threshold.saturating_sub(thirst.value));
        let leash = (headroom * self.temperament.leash_percent / 100).max(MIN_LEASH);
        self.mind
            .map
            .nearest_seen_distance(LandmarkKind::Water, place)
            .is_none_or(|distance| distance <= leash)
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
            let headroom = u64::from(thirst.threshold.saturating_sub(thirst.value));
            let leash = (headroom * self.temperament.leash_percent / 100).max(MIN_LEASH);
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
    /// Labels the decision with its purpose, except that a trip based on someone's
    /// tip keeps `ToldPlace`, so logs can see hints being acted on.
    fn with_reason(mut self, reason: PolicyReason) -> Self {
        if self.selection.reason != PolicyReason::ToldPlace {
            self.selection.reason = reason;
        }
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
