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
    InventoryView, Material, Method, NeedKind, PhysicalNeedsView, PhysicalPerception,
    WorldPosition,
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
            ..
        } = personality;
        // A good wayfinder dares go farther from water and still find the way back.
        let wayfinding = personality.talent(crate::Activity::Wayfinding);
        Self {
            top_up_thirst: Personality::scale(caution, 4_000, 2_000) as u16,
            top_up_hunger: Personality::scale(caution, 4_500, 2_500) as u16,
            food_reserve: Personality::scale(caution, 3, 9) as u8,
            curiosity_target: Personality::scale(curiosity, 1, 3) as usize,
            excursion_chance: Personality::scale(curiosity, 30, 140) as u16,
            prepare_exposure: Personality::scale(caution, 6_000, 4_000) as u16,
            leash_percent: (Personality::scale(caution, 65, 35)
                + Personality::scale(wayfinding, -10, 10)) as u64,
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
/// A hearth is built only this close (cells) to the builder's home, so it
/// warms the hut (within `FIRE_WARMTH_REACH`).
const HEARTH_FROM_HOME: u64 = 4;
/// Tiredness (out of 10,000) at which an agent sleeps wherever it can.
const REST_CRITICAL: u16 = 8_800;
/// How much more an option listed one place earlier is worth, by instinct.
const INSTINCT_STEP: i32 = 3;
/// Kinds of choice tried fewer times than this may get a curiosity bonus.
const CURIOUS_TRIES: u8 = 3;
const CURIOSITY_BONUS: i32 = 8;
/// Huts closer together than this aren't built: one home is enough nearby.
pub(crate) const HUT_SPACING: u64 = 96;
/// Carrying at least this much, a hut in view is worth stopping at to unload.
const STORE_LOAD: u8 = 9;
/// Food carried before a stocking trip home (more than is kept back on storing).
const STOCK_LOAD: u8 = 7;
/// Raw meat carried that's worth a walk to a remembered hearth to cook.
const COOKING_TRIP_MEAT: u8 = 3;
/// Condition (out of 255) below which a home needs mending.
const WORN: u8 = 192;
/// Meat, or berries, carried that's worth smoking to keep.
const SMOKE_MEAT: u8 = 2;
const SMOKE_BERRIES: u8 = 4;
/// Curiosity is divided by this for the chance (out of 256) an idle check
/// becomes an experiment.
const EXPERIMENT_RARITY: u16 = 64;
/// Wood a basket or a spear takes.
const TOOL_WOOD: u8 = 2;
/// Planting happens this close to home (cells), with this much food on
/// hand, while fewer than this many food bushes are in view.
const PLANTING_RANGE: u64 = 8;
const PLANTING_SPARE: u8 = 3;
const FEW_BUSHES: usize = 4;
/// Food in the home hut that's stock enough for winter.
const STOCKED_FOOD: u32 = 24;
/// A fire with less than this much burning left (seconds) is worth feeding.
pub(crate) const LOW_FUEL_SECONDS: u32 = 20 * 60;
/// Idle agents go to look at places pointed out within this many cells.
const NOSY_RANGE: u64 = 48;
/// Exploration never shrinks the round-trip range below this many cells.
const MIN_LEASH: u64 = 24;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Deliberation {
    pub(crate) selection: PolicySelection,
    /// New persistent exploration heading when the agent explores or travels.
    pub(crate) heading: Option<ExplorationHeading>,
    /// A trip to a place out of view (remembered, pointed out, or home),
    /// whatever it's for: the cause-and-effect model counts it as travel.
    pub(crate) trip: bool,
    /// Where a walk is ultimately headed (the waypoint is only the next leg):
    /// the engine gives up on it, and avoids it a while, once the agent stops
    /// getting any closer.
    pub(crate) toward: Option<WorldPosition>,
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
            trip: false,
            toward: None,
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
    /// What it knows of making things, and what it has tried.
    pub(crate) crafts: crate::cognition::Crafts,
    /// Where it stood at its previous decision (it won't step straight back).
    pub(crate) came_from: Option<WorldPosition>,
    /// Mourning someone close: stays near others, takes on no work or trips.
    pub(crate) grieving: bool,
    /// Knows how to knap a blade from stone.
    pub(crate) knows_knapping: bool,
    /// Knows how to build a hut.
    pub(crate) knows_huts: bool,
    /// Fit for heavy work (an adult, not pregnant, not worn out or starving):
    /// building and chopping wood.
    pub(crate) able: bool,
    /// The hut it last put food away in.
    pub(crate) stored_food: Option<WorldPosition>,
    /// The hut it lives in.
    pub(crate) home: Option<WorldPosition>,
    /// Huts it believes are other families' (it saw them sleep there).
    pub(crate) others_huts: &'a [WorldPosition],
    /// Late autumn after a hungry winter: carry food home and put it away.
    pub(crate) stocking: bool,
    /// Knows seeds grow, and it's a growing season.
    pub(crate) plants: bool,
    /// A body in view it would bury.
    pub(crate) body: Option<WorldPosition>,
    /// Old enough (and able) to hunt.
    pub(crate) hunts: bool,
    /// A place it recently couldn't get to, avoided for now.
    pub(crate) avoid: Option<WorldPosition>,
    /// Something to get ready for a hard season it remembers coming.
    pub(crate) ready: Option<u16>,
    /// Its habits and situation, when choices are learned (phase 2).
    pub(crate) habits: Option<(&'a crate::cognition::Habits, crate::cognition::Situation)>,
    /// Its cause-and-effect model, and the needs it meets by planning (a bit
    /// per `NeedKind`).
    pub(crate) model: &'a crate::cognition::Model,
    pub(crate) plans: u8,
    /// What it perceives of its circumstances now.
    pub(crate) facts: crate::cognition::Facts,
    /// What it's set on doing; updated as it plans, follows and revises.
    pub(crate) intention: &'a std::cell::Cell<crate::cognition::Intention>,
    /// Set when a plan step couldn't be done for want of something: what it
    /// tried, and what it lacked.
    pub(crate) lacked: &'a std::cell::Cell<Option<(crate::cognition::Choice, u16)>>,
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
    // Only what it can actually gather: a child can't fell a tree to taste it.
    let food_here = nearest_resource_access(origin, perception, |kind| {
        food.is_food(kind)
            && inventory.can_add(kind)
            && (mind.able || kind.properties().handling != crate::Handling::Chop)
    });
    let urgent = most_urgent(needs);
    // An intention is done once its need has eased (with nothing pressing,
    // plans that failed are forgiven), or what it was getting ready holds or
    // no longer matters.
    let done = match mind.intention.get().aim() {
        Some(crate::cognition::Goal::Need(need)) => {
            urgent.is_none() || !mind.facts.has(crate::cognition::Facts::NEEDS[need])
        }
        Some(crate::cognition::Goal::Ready(fact)) => mind.facts.has(fact) || urgent.is_some(),
        None => urgent.is_none(),
    };
    if done {
        mind.intention.set(crate::cognition::Intention::default());
    }
    let instinctive = instinct(&planner, urgent, inventory, water_here, food_here);
    // About to collapse, it sleeps where it stands (a reflex, not a plan).
    let collapsing = needs.rest.value >= crate::REST_COLLAPSE;
    // Nothing pressing, and a hard season coming that it remembers: get ready.
    if urgent.is_none()
        && mind.plans & PLANS_READY != 0
        && let Some(fact) = mind.ready
        && let Some(readying) = planner.planned(
            crate::cognition::Goal::Ready(fact),
            inventory,
            water_here,
            food_here,
        )
    {
        return readying;
    }
    // Nothing to do: work toward having what has proved useful.
    if urgent.is_none()
        && mind.plans & PLANS_IDLE != 0
        && instinctive.selection.goal == PhysicalGoal::Wait
        && let Some(fact) = planner.most_useful_missing()
        && let Some(working) = planner.planned(
            crate::cognition::Goal::Ready(fact),
            inventory,
            water_here,
            food_here,
        )
    {
        return working;
    }
    let Some(need) = urgent.filter(|&need| {
        mind.plans & (1 << need as u8) != 0 && !(need == NeedKind::Rest && collapsing)
    }) else {
        return instinctive;
    };
    // What it would do anyway has worked for it here before: no need to
    // think it through (and a plan it held gives way to the opportunity).
    if planner.trusts(&instinctive) {
        mind.intention.set(crate::cognition::Intention::default());
        return instinctive;
    }
    planner
        .planned(
            crate::cognition::Goal::Need(need as usize),
            inventory,
            water_here,
            food_here,
        )
        .unwrap_or(instinctive)
}

/// What it does by instinct and habit for its most pressing need, if any.
fn instinct(
    planner: &Planner<'_>,
    urgent: Option<NeedKind>,
    inventory: InventoryView,
    water_here: Option<WorldPosition>,
    food_here: Option<WorldPosition>,
) -> Deliberation {
    let origin = planner.origin;
    let mind = planner.mind;
    let food = mind.food;
    match urgent {
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
        Some(NeedKind::Hunger)
            if inventory.total() >= inventory.capacity()
                && let Some(drop) = planner.make_room(inventory) =>
        {
            drop
        }
        Some(NeedKind::Hunger) => food_here
            .map(|target| {
                Deliberation::act(
                    PhysicalGoal::SeekFood,
                    target,
                    PolicyReason::HungerThreshold,
                )
            })
            .or_else(|| planner.fetch_food())
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
        Some(NeedKind::Rest) if mind.habits.is_some() => planner.learned_rest(inventory),
        Some(NeedKind::Exposure) if mind.habits.is_some() => planner.learned_warmth(inventory),
        Some(NeedKind::Rest) => planner.rest(inventory),
        Some(NeedKind::Exposure) if mind.knows_hearths && planner.warm_up().is_some() => {
            planner.warm_up().expect("checked")
        }
        // Cold with only a dead fire in view: get it going again.
        Some(NeedKind::Exposure)
            if mind.knows_hearths && planner.tend_fire(inventory).is_some() =>
        {
            planner.tend_fire(inventory).expect("checked")
        }
        Some(NeedKind::Exposure) => {
            let reactive = planner.shelter(inventory, PolicyReason::ExposureThreshold);
            if reactive.goal != PhysicalGoal::Wait {
                return Deliberation {
                    selection: reactive,
                    heading: None,
                    trip: false,
                    toward: None,
                };
            }
            planner
                .travel_to_known(LandmarkKind::SHELTER, PhysicalGoal::SeekShelter)
                .or_else(|| {
                    mind.knows_hearths
                        .then(|| {
                            planner.travel_to_known(LandmarkKind::HEARTH, PhysicalGoal::Explore)
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
                self.travel_to_known(LandmarkKind::SHELTER, PhysicalGoal::SeekShelter)
        {
            return home.with_reason(reason);
        }
        if !self.needs.exposure.threshold_reached
            && let Some(spot) = self.free_sleeping_spot()
        {
            return Deliberation::act(PhysicalGoal::Sleep, spot, reason);
        }
        // Too exposed to sleep in the open: a shelter is the only way to rest.
        let reactive = self.shelter(inventory, PolicyReason::ExposureThreshold);
        if reactive.goal != PhysicalGoal::Wait {
            return Deliberation {
                selection: PolicySelection { reason, ..reactive },
                heading: None,
                trip: false,
                toward: None,
            };
        }
        self.travel_to_known(LandmarkKind::SHELTER, PhysicalGoal::SeekShelter)
            .or_else(|| self.gather_known_wood(inventory))
            .or_else(|| self.explore(reason, false))
            .unwrap_or_else(|| Deliberation::wait(self.origin, reason))
    }

    /// Somewhere to shelter, or what to do toward building one, but only a
    /// hut it may build: it must know how, be fit for the work, and have no
    /// hut of its own within reach already.
    fn shelter(&self, inventory: InventoryView, reason: PolicyReason) -> PolicySelection {
        let selection = shelter_selection(self.origin, inventory, self.perception, reason);
        let for_a_hut = selection.goal == PhysicalGoal::BuildShelter
            || selection.reason == PolicyReason::ShelterMaterials;
        // Idle gathering only of what it has a use for.
        let unwanted = selection.goal == PhysicalGoal::GatherMaterial
            && !for_a_hut
            && selection.target.is_some_and(|target| {
                self.perception
                    .resources
                    .iter()
                    .filter(|resource| {
                        manhattan(resource.position, target) <= 1 && resource.resource.capacity > 0
                    })
                    .all(|resource| {
                        !self.wants(resource.resource.kind, inventory)
                            || !self.can_handle(resource.resource.kind)
                    })
            });
        if (for_a_hut && !self.may_build_hut()) || unwanted {
            return PolicySelection {
                goal: PhysicalGoal::Wait,
                target: Some(self.origin),
                reason,
            };
        }
        selection
    }

    /// It may build a hut if it knows how, is fit for the work, and has no
    /// home (or its home is full, or it's freezing far from it), where there's
    /// no hut close by or every hut in view is full. Merely away on a trip, it
    /// sleeps rough until its attachment wears off.
    fn may_build_hut(&self) -> bool {
        let huts_full = self.perception.structures.iter().any(|structure| {
            structure.kind == crate::StructureKind::Shelter
                && structure.state == crate::StructureState::Complete
        }) && self.sleepable_shelter_access().is_none();
        let homeless = self.mind.home.is_none_or(|home| {
            let full = huts_full
                && self
                    .perception
                    .structures
                    .iter()
                    .any(|structure| structure.position == home);
            full || (self.needs.exposure.threshold_reached
                && manhattan(home, self.origin) > HOME_RANGE)
        });
        self.mind.knows_huts
            && self.mind.able
            && homeless
            && (huts_full
                || self
                    .mind
                    .map
                    .nearest_seen_distance(LandmarkKind::SHELTER, self.origin)
                    .is_none_or(|distance| distance > HUT_SPACING))
    }

    /// A full load while short of food or of the wood for its hut: set some down.
    fn make_room(&self, inventory: InventoryView) -> Option<Deliberation> {
        let full = inventory.total() >= inventory.capacity();
        let food = self.mind.food;
        let short_of_food = food.carried(inventory) < self.temperament.food_reserve;
        let short_of_wood =
            self.may_build_hut() && inventory.amount(Material::Wood) < SHELTER_WOOD_COST;
        (full && self.has_spare(inventory) && (short_of_food || short_of_wood))
            .then(|| Deliberation::act(PhysicalGoal::Drop, self.origin, PolicyReason::Unloading))
    }

    /// Something it could set down: a thing it has no use for, or more food
    /// than it keeps (the same limits setting things down uses).
    fn has_spare(&self, inventory: InventoryView) -> bool {
        let food = self.mind.food;
        let short_of_wood =
            self.may_build_hut() && inventory.amount(Material::Wood) < SHELTER_WOOD_COST;
        inventory.carried().any(|(kind, _)| {
            !kind.properties().worn() && !food.is_food(kind) && !self.keeps_all(kind, inventory)
        }) || food.carried(inventory) > if short_of_wood { 2 } else { 4 }
    }

    /// Whether setting things down would keep all it carries of `kind`.
    fn keeps_all(&self, kind: Material, inventory: InventoryView) -> bool {
        let have = inventory.amount(kind);
        let fire = self.mind.knows_hearths;
        let builds = self.may_build_hut();
        let short_of_wood = builds && inventory.amount(Material::Wood) < SHELTER_WOOD_COST;
        let keep = match kind {
            Material::Wood if builds => SHELTER_WOOD_COST,
            Material::Wood if fire => crate::HEARTH_WOOD_COST + 1,
            Material::Stone if short_of_wood => 0,
            Material::Stone if fire => crate::HEARTH_STONE_COST,
            Material::Stone => u8::from(self.mind.knows_knapping),
            _ => 0,
        };
        have <= keep
    }

    /// Whether it has a use for more `kind`: a few meals' worth of food, wood
    /// for a hut it may build or a fire it keeps, stone for a hearth or a blade.
    fn wants(&self, kind: Material, inventory: InventoryView) -> bool {
        let food = self.mind.food;
        if food.is_food(kind) {
            return food.carried(inventory) < self.temperament.food_reserve;
        }
        let have = inventory.amount(kind);
        let fire = self.mind.knows_hearths;
        match kind {
            Material::Wood => {
                let for_hut = if self.may_build_hut() {
                    SHELTER_WOOD_COST
                } else {
                    0
                };
                let for_fire = if fire { crate::HEARTH_WOOD_COST + 1 } else { 0 };
                // Wood to mend a worn home.
                let for_repair = if self
                    .home_in_view()
                    .is_some_and(|home| !home.stone && home.condition < WORN)
                {
                    crate::REPAIR_COST
                } else {
                    0
                };
                have < for_hut.max(for_fire).max(for_repair)
            }
            Material::Stone => {
                let for_hearth = if fire { crate::HEARTH_STONE_COST } else { 0 };
                let for_blade = u8::from(self.mind.knows_knapping);
                // Stone for walling its hut, once it knows how.
                let for_walls = if self
                    .mind
                    .crafts
                    .knows_design(crate::cognition::Design::StoneWalls)
                    && self.home_in_view().is_some_and(|home| !home.stone)
                {
                    crate::STONE_HUT_STONE
                } else {
                    0
                };
                have < for_hearth.max(for_blade).max(for_walls)
            }
            _ => false,
        }
    }

    /// Whether it's fit to gather `kind`: chopping wood is heavy work; anyone
    /// can pick things up.
    fn can_handle(&self, kind: Material) -> bool {
        self.mind.able || kind.properties().handling != crate::Handling::Chop
    }

    /// The nearest finished hut in view, and whether the agent stands beside it.
    /// The nearest place in view to keep food (a storage pit before a hut)
    /// whose store suits `wanted`.
    fn hut_in_view(&self, wanted: impl Fn(InventoryView) -> bool) -> Option<WorldPosition> {
        let origin = self.origin;
        self.perception
            .structures
            .iter()
            .filter(|structure| {
                structure.kind.stores()
                    && structure.state == crate::StructureState::Complete
                    && !self.theirs(structure.position)
                    && wanted(structure.stored)
            })
            .min_by_key(|structure| {
                (
                    structure.kind != crate::StructureKind::Pit,
                    manhattan(origin, structure.position),
                    structure.id.get(),
                )
            })
            .map(|structure| structure.position)
    }

    /// Its home hut, if it's in view.
    fn home_in_view(&self) -> Option<&crate::StructureView> {
        let home = self.mind.home?;
        self.perception.structures.iter().find(|structure| {
            structure.position == home
                && structure.kind == crate::StructureKind::Shelter
                && structure.state == crate::StructureState::Complete
        })
    }

    /// A finished structure of `kind` in view within `reach` of `near`.
    fn structure_near(&self, kind: crate::StructureKind, near: WorldPosition, reach: u64) -> bool {
        self.perception
            .structures
            .iter()
            .any(|structure| structure.kind == kind && manhattan(structure.position, near) <= reach)
    }

    /// Its home, worn to half, mended with wood it carries, or stone (always
    /// for a stone hut).
    fn repair(&self, inventory: InventoryView) -> Option<Deliberation> {
        let home = self.home_in_view()?;
        let wood = inventory.amount(Material::Wood) >= crate::REPAIR_COST;
        let stone = inventory.amount(Material::Stone) >= crate::REPAIR_COST;
        let can_mend = if home.stone { stone } else { wood || stone };
        (home.condition < WORN && can_mend)
            .then(|| self.at_hut(home.position, PhysicalGoal::Repair, PolicyReason::Repairing))
            .flatten()
    }

    /// Building something it knows by home: stone walls for its hut, a drying
    /// rack, a storage pit. Without knowing the design, a curious person with
    /// the materials sometimes tries it anyway (`trying`): that's how designs
    /// are found.
    fn build_by_home(&self, inventory: InventoryView, trying: bool) -> Option<Deliberation> {
        use crate::cognition::Design;
        let crafts = self.mind.crafts;
        let home = self.home_in_view()?;
        if manhattan(home.position, self.origin) > PLANTING_RANGE || !self.mind.able {
            return None;
        }
        let reason = if trying {
            PolicyReason::Experimenting
        } else {
            PolicyReason::Building
        };
        let fits = |design: Design| crafts.knows_design(design) != trying;
        if fits(Design::StoneWalls)
            && !home.stone
            && inventory.amount(Material::Stone) >= crate::STONE_HUT_STONE
        {
            return self.at_hut(home.position, PhysicalGoal::WallInStone, reason);
        }
        let food = self.mind.food.carried(inventory);
        // Never on a hut's doorstep: those are where people sleep.
        let site = || {
            nearest_build_site(self.origin, self.perception).filter(|&site| {
                !self.perception.structures.iter().any(|structure| {
                    structure.kind == crate::StructureKind::Shelter
                        && manhattan(structure.position, site) <= 2
                })
            })
        };
        if fits(Design::Rack)
            && inventory.amount(Material::Wood) >= crate::RACK_WOOD_COST
            && food >= 2
            && !self.structure_near(crate::StructureKind::Rack, home.position, PLANTING_RANGE)
            && let Some(site) = site()
        {
            return Some(Deliberation::act(PhysicalGoal::BuildRack, site, reason));
        }
        let digs = inventory
            .carried()
            .any(|(material, _)| material.properties().digs);
        if fits(Design::Pit)
            && digs
            && !self.structure_near(crate::StructureKind::Pit, home.position, PLANTING_RANGE)
            && let Some(site) = site()
        {
            return Some(Deliberation::act(PhysicalGoal::DigPit, site, reason));
        }
        None
    }

    /// A drying rack in view that dries now (not in winter).
    fn rack_in_view(&self) -> Option<WorldPosition> {
        if crate::Season::at(self.needs.at) == crate::Season::Winter {
            return None;
        }
        self.perception
            .structures
            .iter()
            .filter(|structure| {
                structure.kind == crate::StructureKind::Rack
                    && structure.state == crate::StructureState::Complete
            })
            .min_by_key(|structure| {
                (
                    manhattan(structure.position, self.origin),
                    structure.id.get(),
                )
            })
            .map(|structure| structure.position)
    }

    /// The nearest fire burning in view.
    fn fire_in_view(&self) -> Option<WorldPosition> {
        let now = (self.needs.at.ticks() / 60) as u32;
        self.perception
            .structures
            .iter()
            .filter(|structure| structure.kind.burns() && structure.working(now))
            .min_by_key(|structure| {
                (
                    manhattan(structure.position, self.origin),
                    structure.position.y,
                    structure.position.x,
                )
            })
            .map(|structure| structure.position)
    }

    /// Food it knows how to keep: pounds pemmican from dried meat and berries
    /// it carries, or smokes surplus meat or berries at a fire in view.
    fn preserve(&self, inventory: InventoryView) -> Option<Deliberation> {
        let crafts = self.mind.crafts;
        let knows = |output: Material| {
            crate::RECIPES
                .iter()
                .enumerate()
                .any(|(index, recipe)| recipe.output == output && crafts.knows(index))
        };
        if knows(Material::Pemmican)
            && inventory.amount(Material::DriedMeat) > 0
            && inventory.amount(Material::DriedBerries) > 0
        {
            return Some(Deliberation::act(
                PhysicalGoal::Craft,
                self.origin,
                PolicyReason::Crafting,
            ));
        }
        let surplus = (knows(Material::DriedMeat)
            && inventory.amount(Material::Meat) >= SMOKE_MEAT)
            || (knows(Material::DriedBerries)
                && inventory.amount(Material::Berries) >= SMOKE_BERRIES);
        if !surplus {
            return None;
        }
        self.at_hut(
            self.fire_in_view()?,
            PhysicalGoal::Cook,
            PolicyReason::Preserving,
        )
    }

    /// Now and then someone curious tries something new with what it carries:
    /// by hand, with an edge, or at a fire in view.
    fn experiment(&self, inventory: InventoryView) -> Option<Deliberation> {
        let chance = u16::from(self.mind.personality.curiosity) / EXPERIMENT_RARITY;
        // Only with food to spare: a try can spoil what goes into it.
        if self.roll(0x4558_5045) >= chance
            || self.mind.food.carried(inventory) < self.temperament.food_reserve
        {
            return None;
        }
        let carried: Vec<Material> = inventory.carried().map(|(material, _)| material).collect();
        let crafts = self.mind.crafts;
        let roll = u64::from(self.roll(0x5452_5920));
        if let Some(build) = self.build_by_home(inventory, true) {
            return Some(build);
        }
        if let Some(rack) = self.rack_in_view()
            && crafts.untried(&carried, &[Method::Rack], roll).is_some()
        {
            return self.at_hut(rack, PhysicalGoal::Cook, PolicyReason::Experimenting);
        }
        if let Some(fire) = self.fire_in_view()
            && crafts
                .untried(&carried, &[Method::Fire, Method::Smoke], roll)
                .is_some()
        {
            return self.at_hut(fire, PhysicalGoal::Cook, PolicyReason::Experimenting);
        }
        let edge = inventory
            .carried()
            .any(|(material, _)| material.properties().cutting);
        let methods: &[Method] = if edge {
            &[Method::Hand, Method::Edge]
        } else {
            &[Method::Hand]
        };
        crafts.untried(&carried, methods, roll).map(|_| {
            Deliberation::act(
                PhysicalGoal::Craft,
                self.origin,
                PolicyReason::Experimenting,
            )
        })
    }

    /// Someone who keeps fire, carrying raw meat: cooks it at a fire burning
    /// in view, or with enough meat to be worth the walk, heads for a hearth
    /// it remembers.
    fn cook(&self, inventory: InventoryView) -> Option<Deliberation> {
        let meat = inventory.amount(Material::Meat);
        if !self.mind.knows_hearths || meat == 0 {
            return None;
        }
        match self.fire_in_view() {
            Some(fire) => self.at_hut(fire, PhysicalGoal::Cook, PolicyReason::Cooking),
            None if meat >= COOKING_TRIP_MEAT => self
                .travel_to_known(LandmarkKind::HEARTH, PhysicalGoal::Explore)
                .map(|trip| trip.with_reason(PolicyReason::Cooking)),
            None => None,
        }
    }

    /// The hut at `hut` belongs to another family, as far as it knows.
    fn theirs(&self, hut: WorldPosition) -> bool {
        self.mind.others_huts.contains(&hut)
    }

    /// Late autumn after a hungry winter, with food enough on it: take it
    /// home and put it away (unless home is far, or full of food already).
    fn stock_home(&self, inventory: InventoryView) -> Option<Deliberation> {
        let home = self.mind.home?;
        let food = self.mind.food;
        if !self.mind.stocking
            || food.carried(inventory) < STOCK_LOAD
            || manhattan(home, self.origin) > HOME_RANGE
        {
            return None;
        }
        let stocked = self.perception.structures.iter().any(|structure| {
            structure.position == home
                && structure
                    .stored
                    .carried()
                    .filter(|(material, _)| food.is_food(*material))
                    .map(|(_, amount)| u32::from(amount))
                    .sum::<u32>()
                    >= STOCKED_FOOD
        });
        if stocked {
            return None;
        }
        let trip = self.at_hut(home, PhysicalGoal::Store, PolicyReason::Storing)?;
        Some(Deliberation {
            toward: Some(home),
            ..trip
        })
    }

    /// Near home with berries to spare and few bushes in view: plant one on
    /// a free cell beside it (open ground, nothing growing or built there).
    fn plant(&self, inventory: InventoryView) -> Option<Deliberation> {
        let home = self.mind.home?;
        let food = self.mind.food;
        if !self.mind.plants
            || manhattan(home, self.origin) > PLANTING_RANGE
            || food.carried(inventory) < PLANTING_SPARE
        {
            return None;
        }
        let bushes = self
            .perception
            .resources
            .iter()
            .chain(&self.perception.spent_resources)
            .filter(|found| {
                food.is_food(found.resource.kind)
                    && found.resource.kind.properties().regrow_seconds > 0
            })
            .count();
        if bushes >= FEW_BUSHES {
            return None;
        }
        let taken = |cell: WorldPosition| {
            self.perception
                .resources
                .iter()
                .chain(&self.perception.spent_resources)
                .any(|found| found.position == cell)
                || self
                    .perception
                    .structures
                    .iter()
                    .any(|structure| structure.position == cell)
        };
        let spot = [(0, -1), (-1, 0), (1, 0), (0, 1)]
            .map(|(dx, dy)| WorldPosition {
                x: self.origin.x + dx,
                y: self.origin.y + dy,
            })
            .into_iter()
            .find(|&cell| {
                candidate_available(self.origin, self.perception, cell) && !taken(cell)
            })?;
        Some(Deliberation::act(
            PhysicalGoal::Plant,
            spot,
            PolicyReason::Planting,
        ))
    }

    /// Goes to `hut` and does `goal` there (beside it), for `reason`.
    fn at_hut(
        &self,
        hut: WorldPosition,
        goal: PhysicalGoal,
        reason: PolicyReason,
    ) -> Option<Deliberation> {
        let origin = self.origin;
        if origin.x.abs_diff(hut.x).max(origin.y.abs_diff(hut.y)) <= 1 {
            return Some(Deliberation::act(goal, hut, reason));
        }
        if self.mind.avoid == Some(hut) {
            return None;
        }
        let (waypoint, heading) = self.waypoint_toward(hut)?;
        Some(Deliberation {
            selection: PolicySelection {
                goal: PhysicalGoal::Explore,
                target: Some(waypoint),
                reason,
            },
            heading: Some(heading),
            trip: false,
            toward: Some(hut),
        })
    }

    /// Carrying a heavy load with a hut in view: put what it doesn't need away.
    fn store(&self, inventory: InventoryView) -> Option<Deliberation> {
        if inventory.total() < STORE_LOAD {
            return None;
        }
        let hut = self.hut_in_view(|stored| stored.total() < crate::HUT_STORE_CAPACITY)?;
        self.at_hut(hut, PhysicalGoal::Store, PolicyReason::Storing)
    }

    /// Hungry: food put away in a hut in view, or in the hut it last left food in.
    fn fetch_food(&self) -> Option<Deliberation> {
        let food = self.mind.food;
        let has_food =
            |stored: InventoryView| stored.carried().any(|(material, _)| food.is_food(material));
        if let Some(hut) = self.hut_in_view(has_food) {
            return self.at_hut(hut, PhysicalGoal::Fetch, PolicyReason::Fetching);
        }
        let hut = self.mind.stored_food?;
        let trip = self.at_hut(hut, PhysicalGoal::Fetch, PolicyReason::Fetching)?;
        Some(Deliberation {
            trip: trip.heading.is_some(),
            ..trip
        })
    }

    /// Works toward `goal` by its intention: follows the step it's on, or
    /// forms a plan from its own beliefs if it has none for this goal. A step
    /// it can't take (and what it lacked) is noted, and it thinks again once.
    /// None when it can think of no plan: instinct then decides.
    fn planned(
        &self,
        goal: crate::cognition::Goal,
        inventory: InventoryView,
        water_here: Option<WorldPosition>,
        food_here: Option<WorldPosition>,
    ) -> Option<Deliberation> {
        use crate::cognition::{Abilities, Goal, Intention, plan, plan_ready};
        let cell = self.mind.intention;
        let mut intention = cell.get();
        let now = (self.needs.at.ticks() / 60) as u32;
        if intention.aim() != Some(goal) && intention.revisions == 0 {
            intention = Intention::default();
        }
        // Plans for this keep failing: leave it to instinct until it's over.
        if intention.aim().is_none() && intention.revisions >= GIVE_UP_PLANNING {
            cell.set(intention);
            return None;
        }
        // Each place it could go is probed once.
        let probed: std::cell::Cell<[Option<Option<u32>>; 16]> = std::cell::Cell::new([None; 16]);
        let can = |choice: crate::cognition::Choice| self.can(choice);
        let worth = |choice: crate::cognition::Choice, facts: u16| self.worth_in(choice, facts);
        let can_reach = |fact: u16| {
            let index = fact.trailing_zeros() as usize % 16;
            let mut known = probed.get();
            let seconds = *known[index].get_or_insert_with(|| {
                let there = self.go_for(fact)?.toward?;
                // About a second a cell, with detours.
                Some(manhattan(self.origin, there).min(u64::from(u32::MAX)) as u32)
            });
            probed.set(known);
            seconds
        };
        let abilities = Abilities {
            can: &can,
            can_reach: &can_reach,
            worth: &worth,
        };
        let think = |revisions: u8| match goal {
            Goal::Need(need) => plan(
                self.mind.model,
                self.mind.facts,
                need,
                &abilities,
                revisions,
                now,
            ),
            Goal::Ready(fact) => plan_ready(
                self.mind.model,
                self.mind.facts,
                fact,
                &abilities,
                revisions,
                now,
            ),
        };
        // Something much better than the plan it holds has turned up: think again.
        if intention.aim().is_some()
            && let Some(fresh) = think(intention.revisions)
            && intention.outdone_by(&fresh)
        {
            intention = fresh;
        }
        for _ in 0..2 {
            if intention.aim().is_none() {
                let Some(fresh) = think(intention.revisions) else {
                    break;
                };
                intention = fresh;
            }
            intention.advance(self.mind.facts);
            let Some(step) = intention.current() else {
                intention = Intention::default();
                continue;
            };
            let choice = step.choice()?;
            let reason = match goal {
                Goal::Need(need) => need_reason(need),
                Goal::Ready(_) => PolicyReason::NoUrgentNeed,
            };
            let done = match self.carry_out(
                choice,
                step.fact(),
                reason,
                inventory,
                water_here,
                food_here,
            ) {
                Ok(deliberation) if intention.persist() => {
                    cell.set(intention);
                    return Some(deliberation);
                }
                Ok(_) => None,
                Err(lacked) => lacked,
            };
            self.mind.lacked.set(done.map(|fact| (choice, fact)));
            // That plan failed: think again from what it now believes.
            intention = Intention::none(intention.revisions.saturating_add(1));
        }
        cell.set(intention);
        None
    }

    /// Whether experience vouches for `option` here: it has worked for it in
    /// this situation, often enough to act on without thinking it through.
    fn trusts(&self, option: &Deliberation) -> bool {
        let Some((habits, situation)) = self.mind.habits else {
            return false;
        };
        let choice = crate::cognition::Choice::of(option.selection.goal, option.selection.reason);
        let (value, tries) = habits.value(situation, choice);
        tries >= TRUSTED_TRIES
            && i32::from(value) + habits.foresight(situation, choice) >= TRUSTED_VALUE
    }

    /// What `choice` has been worth in the situation `facts` describe (this
    /// season), by experience: relief in the same units as the model's.
    fn worth_in(&self, choice: crate::cognition::Choice, facts: u16) -> Option<i32> {
        use crate::cognition::{Facts, Situation};
        let (habits, now) = self.mind.habits?;
        let facts = Facts(facts);
        let situation = Situation::new(
            Facts::NEEDS.map(|need| facts.has(need)),
            now.season(),
            facts.has(Facts::SHELTER),
            facts.has(Facts::FIRE),
        );
        let (value, tries) = habits.value(situation, choice);
        (tries >= TRUSTED_TRIES).then_some(i32::from(value) * 4)
    }

    /// The thing it lacks that has been most useful to it, by its own
    /// beliefs: what is needed by actions that eased its needs, or that bring
    /// about what such actions need (wood feeds a fire, at which warming up
    /// helps), two steps deep.
    fn most_useful_missing(&self) -> Option<u16> {
        use crate::cognition::Facts;
        [
            Facts::HAS_FOOD,
            Facts::HAS_WOOD,
            Facts::HAS_STONE,
            Facts::HAS_BLADE,
            Facts::STORE_NEAR,
        ]
        .into_iter()
        .filter(|&fact| !self.mind.facts.has(fact))
        .map(|fact| (self.usefulness(fact, 2), fact))
        .filter(|&(worth, _)| worth > 0)
        .max_by_key(|&(worth, fact)| (worth, std::cmp::Reverse(fact)))
        .map(|(_, fact)| fact)
    }

    /// How useful having `fact` has been: the most an action needing it has
    /// eased its needs, or (`depth` deep) what it brought about was worth.
    fn usefulness(&self, fact: u16, depth: u8) -> i32 {
        use crate::cognition::{Choice, MADE};
        let model = self.mind.model;
        Choice::ALL
            .into_iter()
            .filter(|&choice| self.can(choice) && model.needs(choice).has(fact))
            .map(|choice| {
                let eases: i32 = (0..4)
                    .map(|need| i32::from(model.relief(choice, need)).max(0))
                    .sum();
                let leads = if depth == 0 {
                    0
                } else {
                    MADE.into_iter()
                        .filter(|&made| {
                            let (hits, tries) = model.odds(choice, made);
                            hits >= 2 && hits * 2 >= tries
                        })
                        .map(|made| self.usefulness(made, depth - 1) / 2)
                        .max()
                        .unwrap_or(0)
                };
                eases.max(leads)
            })
            .max()
            .unwrap_or(0)
    }

    /// Whether it knows how to do `choice` as a step of a plan.
    fn can(&self, choice: crate::cognition::Choice) -> bool {
        use crate::cognition::Choice;
        match choice {
            Choice::Build => self.may_build_hut(),
            Choice::TendFire | Choice::WarmUp => self.mind.knows_hearths,
            Choice::Craft => self.mind.knows_knapping,
            Choice::Drink
            | Choice::Eat
            | Choice::Forage
            | Choice::Gather
            | Choice::Store
            | Choice::Fetch
            | Choice::Sleep => true,
            Choice::Hunt
            | Choice::Travel
            | Choice::Company
            | Choice::Explore
            | Choice::Flee
            | Choice::Unload
            | Choice::Rest => false,
        }
    }

    /// A trip to a remembered place where `fact` would hold.
    fn go_for(&self, fact: u16) -> Option<Deliberation> {
        use crate::cognition::Facts;
        let trip = match fact {
            Facts::WATER_NEAR => self.travel_to_known(LandmarkKind::Water, PhysicalGoal::SeekWater),
            Facts::FOOD_NEAR => self.travel_to_food(PhysicalGoal::SeekFood),
            Facts::WOOD_NEAR => {
                self.travel_to_known(LandmarkKind::WOOD, PhysicalGoal::GatherMaterial)
            }
            Facts::STONE_NEAR => {
                self.travel_to_known(LandmarkKind::STONE, PhysicalGoal::GatherMaterial)
            }
            Facts::FIRE | Facts::DEAD_FIRE => self
                .travel_to_known(LandmarkKind::HEARTH, PhysicalGoal::Explore)
                .map(|trip| trip.with_reason(PolicyReason::Warming)),
            Facts::SHELTER => self
                .go_home(PolicyReason::ExposureThreshold)
                .filter(|home| home.trip)
                .or_else(|| self.travel_to_known(LandmarkKind::SHELTER, PhysicalGoal::SeekShelter)),
            Facts::STORE_NEAR => self.fetch_food().filter(|fetch| fetch.trip),
            _ => None,
        }?;
        Some(Deliberation { trip: true, ..trip })
    }

    /// Does `choice` toward `fact` (or, with no fact, the last step that
    /// eases the need), or says what it lacked.
    fn carry_out(
        &self,
        choice: crate::cognition::Choice,
        fact: Option<u16>,
        reason: PolicyReason,
        inventory: InventoryView,
        water_here: Option<WorldPosition>,
        food_here: Option<WorldPosition>,
    ) -> Result<Deliberation, Option<u16>> {
        use crate::cognition::{Choice, Facts};
        let origin = self.origin;
        let has = |fact: u16| self.mind.facts.has(fact);
        let lacking = |fact: u16| if has(fact) { None } else { Some(fact) };
        match choice {
            Choice::Travel => {
                let trip = fact.and_then(|fact| self.go_for(fact)).ok_or(None)?;
                // There already, and what it came for isn't: the plan was
                // wrong about this place; think again from what it sees.
                match trip.toward {
                    Some(there) if manhattan(origin, there) <= ARRIVED => Err(None),
                    _ => Ok(trip),
                }
            }
            Choice::Gather => {
                let material = match fact {
                    Some(Facts::HAS_STONE) => Material::Stone,
                    _ => Material::Wood,
                };
                nearest_resource_access(origin, self.perception, |kind| {
                    kind == material && inventory.can_add(kind) && self.can_handle(kind)
                })
                .map(|target| Deliberation::act(PhysicalGoal::GatherMaterial, target, reason))
                .ok_or(Some(if material == Material::Stone {
                    Facts::STONE_NEAR
                } else {
                    Facts::WOOD_NEAR
                }))
            }
            Choice::Forage => food_here
                .map(|target| Deliberation::act(PhysicalGoal::SeekFood, target, reason))
                .ok_or(Some(Facts::FOOD_NEAR)),
            Choice::Drink => water_here
                .map(|target| Deliberation::act(PhysicalGoal::SeekWater, target, reason))
                .ok_or(Some(Facts::WATER_NEAR)),
            Choice::Eat => (self.mind.food.carried(inventory) > 0)
                .then(|| Deliberation::act(PhysicalGoal::Eat, origin, reason))
                .ok_or(Some(Facts::HAS_FOOD)),
            Choice::TendFire => self
                .tend_fire(inventory)
                .ok_or_else(|| lacking(Facts::HAS_WOOD).or(lacking(Facts::DEAD_FIRE))),
            Choice::WarmUp => self.warm_up().ok_or(Some(Facts::FIRE)),
            Choice::Sleep => self
                .go_home(reason)
                .filter(|home| !home.trip)
                .or_else(|| {
                    self.sleepable_shelter_access()
                        .map(|access| Deliberation::act(PhysicalGoal::Sleep, access, reason))
                })
                .or_else(|| {
                    (!self.needs.exposure.threshold_reached)
                        .then(|| self.free_sleeping_spot())
                        .flatten()
                        .map(|spot| Deliberation::act(PhysicalGoal::Sleep, spot, reason))
                })
                .ok_or(Some(Facts::SHELTER)),
            Choice::Build => {
                let selection = self.shelter(inventory, reason);
                (selection.goal == PhysicalGoal::BuildShelter)
                    .then_some(Deliberation {
                        selection,
                        heading: None,
                        trip: false,
                        toward: None,
                    })
                    .ok_or(lacking(Facts::HAS_WOOD))
            }
            Choice::Fetch => self.fetch_food().ok_or(Some(Facts::STORE_NEAR)),
            Choice::Store => self.store(inventory).ok_or(Some(Facts::SHELTER)),
            Choice::Craft => self.make_tool(inventory).ok_or(lacking(Facts::HAS_STONE)),
            _ => Err(None),
        }
    }

    /// Cold: what it could do about it, in its instinctive order, picked by
    /// what has worked for it before.
    fn learned_warmth(&self, inventory: InventoryView) -> Deliberation {
        let reason = PolicyReason::ExposureThreshold;
        let mut options = Vec::new();
        options.extend(self.go_home(reason));
        if self.mind.knows_hearths {
            options.extend(self.warm_up());
            options.extend(self.tend_fire(inventory));
        }
        let reactive = self.shelter(inventory, reason);
        if reactive.goal != PhysicalGoal::Wait {
            options.push(Deliberation {
                selection: reactive,
                heading: None,
                trip: false,
                toward: None,
            });
        }
        options.extend(self.travel_to_known(LandmarkKind::SHELTER, PhysicalGoal::SeekShelter));
        if self.mind.knows_hearths {
            options.extend(
                self.travel_to_known(LandmarkKind::HEARTH, PhysicalGoal::Explore)
                    .map(|trip| trip.with_reason(PolicyReason::Warming)),
            );
        }
        options.extend(self.gather_known_wood(inventory));
        options.extend(self.explore(reason, true));
        self.pick(options)
            .unwrap_or_else(|| Deliberation::wait(self.origin, reason))
    }

    /// Tired: about to collapse, it sleeps where it stands (a reflex);
    /// otherwise it picks among what it could do by what has worked before.
    fn learned_rest(&self, inventory: InventoryView) -> Deliberation {
        let reason = PolicyReason::RestThreshold;
        if self.needs.rest.value >= crate::REST_COLLAPSE
            && let Some(spot) = self.free_sleeping_spot()
        {
            return Deliberation::act(PhysicalGoal::Sleep, spot, reason);
        }
        let mut options = Vec::new();
        options.extend(self.go_home(reason));
        options.extend(
            self.sleepable_shelter_access()
                .map(|access| Deliberation::act(PhysicalGoal::Sleep, access, reason)),
        );
        options.extend(
            self.travel_to_known(LandmarkKind::SHELTER, PhysicalGoal::SeekShelter)
                .map(|home| home.with_reason(reason)),
        );
        if !self.needs.exposure.threshold_reached {
            options.extend(
                self.free_sleeping_spot()
                    .map(|spot| Deliberation::act(PhysicalGoal::Sleep, spot, reason)),
            );
        }
        let reactive = self.shelter(inventory, PolicyReason::ExposureThreshold);
        if reactive.goal != PhysicalGoal::Wait {
            options.push(Deliberation {
                selection: PolicySelection { reason, ..reactive },
                heading: None,
                trip: false,
                toward: None,
            });
        }
        options.extend(self.gather_known_wood(inventory));
        options.extend(self.explore(reason, false));
        self.pick(options)
            .unwrap_or_else(|| Deliberation::wait(self.origin, reason))
    }

    /// The option worth most: an instinctive lean toward those listed first,
    /// plus how that kind of choice has gone in this situation and what it
    /// expects it to lead to, plus a curiosity bonus for kinds it has hardly tried. Ties go to the earlier.
    fn pick(&self, options: Vec<Deliberation>) -> Option<Deliberation> {
        let (habits, situation) = self.mind.habits?;
        let count = options.len() as i32;
        let curiosity = i32::from(self.mind.personality.curiosity);
        options
            .into_iter()
            .enumerate()
            .map(|(rank, option)| {
                let choice =
                    crate::cognition::Choice::of(option.selection.goal, option.selection.reason);
                let (value, tries) = habits.value(situation, choice);
                let instinct = INSTINCT_STEP * (count - rank as i32);
                let novel = tries < CURIOUS_TRIES
                    && i32::from(self.roll(0x4355_5249 ^ choice as u64)) < curiosity / 2;
                let foresight = habits.foresight(situation, choice);
                let score = instinct
                    + i32::from(value)
                    + foresight
                    + if novel { CURIOSITY_BONUS } else { 0 };
                (score, -(rank as i32), option)
            })
            .max_by_key(|&(score, rank, _)| (score, rank))
            .map(|(_, _, option)| option)
    }

    fn home_is_near(&self) -> bool {
        self.mind
            .map
            .nearest_seen_distance(LandmarkKind::SHELTER, self.origin)
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

    /// Its home within reach: sleep under its roof, or head back to it.
    fn go_home(&self, reason: PolicyReason) -> Option<Deliberation> {
        let home = self.mind.home?;
        if manhattan(home, self.origin) > HOME_RANGE {
            return None;
        }
        let in_view = self
            .perception
            .structures
            .iter()
            .any(|structure| structure.position == home);
        let spot = in_view
            .then(|| {
                [(0, -1), (-1, 0), (1, 0), (0, 1)]
                    .map(|(dx, dy)| WorldPosition {
                        x: home.x + dx,
                        y: home.y + dy,
                    })
                    .into_iter()
                    .filter(|&cell| self.can_sleep_on(cell))
                    .min_by_key(|&cell| (manhattan(self.origin, cell), cell.y, cell.x))
            })
            .flatten();
        if let Some(spot) = spot {
            if spot == self.origin {
                return Some(Deliberation::act(PhysicalGoal::Sleep, spot, reason));
            }
            let (waypoint, heading) = self.waypoint_toward(spot)?;
            return Some(Deliberation {
                selection: PolicySelection {
                    goal: PhysicalGoal::SeekShelter,
                    target: Some(waypoint),
                    reason,
                },
                heading: Some(heading),
                trip: false,
                toward: Some(home),
            });
        }
        if in_view {
            // Every place under its roof is taken.
            return None;
        }
        if self.mind.avoid == Some(home) {
            return None;
        }
        let (waypoint, heading) = self.waypoint_toward(home)?;
        Some(Deliberation {
            selection: PolicySelection {
                goal: PhysicalGoal::SeekShelter,
                target: Some(waypoint),
                reason,
            },
            heading: Some(heading),
            trip: true,
            toward: Some(home),
        })
    }

    fn sleepable_shelter_access(&self) -> Option<WorldPosition> {
        self.perception
            .structures
            .iter()
            .filter(|structure| {
                structure.state == crate::StructureState::Complete
                    && structure.kind == crate::StructureKind::Shelter
                    // Another family's hut only when freezing.
                    && (!self.theirs(structure.position) || self.needs.exposure.threshold_reached)
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
        // Full up but short of something it needs: set down what it doesn't.
        if let Some(drop) = self.make_room(inventory) {
            return drop;
        }
        // Stocking up needs room to carry it; a full load goes to its use first.
        let stocking = self.mind.stocking && self.mind.home.is_some();
        if (food.carried(inventory) < temperament.food_reserve
            || (stocking && food.carried(inventory) < STOCK_LOAD))
            && inventory.can_add(Material::Berries)
        {
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
        // The dead come before chores.
        if let Some(body) = self.mind.body
            && let Some(burial) = self.at_hut(body, PhysicalGoal::Bury, PolicyReason::Burying)
        {
            return burial;
        }
        // Food it knows how to keep is kept: smoked or pounded into pemmican.
        if let Some(keep) = self.preserve(inventory) {
            return keep;
        }
        // Raw meat goes bad fast: cook it at a fire in view.
        if let Some(cook) = self.cook(inventory) {
            return cook;
        }
        // Getting ready for winter: food home.
        if let Some(stock) = self.stock_home(inventory) {
            return stock;
        }
        // Loaded down with a hut in view: put the surplus away.
        if let Some(store) = self.store(inventory) {
            return store;
        }
        // A worn home is mended before it falls down.
        if let Some(mend) = self.repair(inventory) {
            return mend;
        }
        // What it knows how to build by home: stone walls, a rack, a pit.
        if works && let Some(build) = self.build_by_home(inventory, false) {
            return build;
        }
        // Someone who keeps fire feeds one that's burning low.
        if works && let Some(tend) = self.tend_fire(inventory) {
            return tend;
        }
        // Someone who can knap and has no edge makes one from a carried stone.
        if works && let Some(craft) = self.make_tool(inventory) {
            return craft;
        }
        // The curious try new things with what they carry.
        if works && let Some(trial) = self.experiment(inventory) {
            return trial;
        }
        // Hunting is work too, and prey in view is the best work there is.
        if works && let Some(hunt) = self.hunt(inventory) {
            return hunt;
        }
        // Someone who keeps fire and has none near home builds one.
        if works && let Some(hearth) = self.make_hearth(inventory) {
            return hearth;
        }
        // Someone who knows seeds grow plants berries by home, where few bushes are.
        if works && let Some(plant) = self.plant(inventory) {
            return plant;
        }
        let shelter_in_view = nearest_shelter_access(origin, self.perception).is_some();
        if !works {
            // Not in the mood for work: skip to friends, needed exploration, or rest.
        } else if !shelter_in_view && self.home_is_near() {
            // Already has a home nearby: gather what's around instead of building another.
            if let Some(target) = nearest_resource_access(origin, self.perception, |kind| {
                inventory.can_add(kind) && self.can_handle(kind) && self.wants(kind, inventory)
            }) {
                return Deliberation::act(
                    PhysicalGoal::GatherMaterial,
                    target,
                    PolicyReason::NoUrgentNeed,
                );
            }
        } else {
            let reactive = self.shelter(inventory, PolicyReason::NoUrgentNeed);
            if reactive.goal != PhysicalGoal::Wait {
                return Deliberation {
                    selection: reactive,
                    heading: None,
                    trip: false,
                    toward: None,
                };
            }
            // Someone who means to build a hut fetches the wood for it from
            // trees it remembers, in good time rather than once it's cold.
            if let Some(wood) = self.gather_known_wood(inventory) {
                return wood.with_reason(PolicyReason::ShelterMaterials);
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
                trip: false,
                toward: None,
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
                trip: false,
                toward: None,
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
                trip: false,
                toward: None,
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
                    trip: false,
                    toward: None,
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
                trip: false,
                toward: None,
            };
        }
        Deliberation::wait(origin, PolicyReason::NoUrgentNeed)
    }

    /// Warms up at the hearth in view: right away if it's beside the agent,
    /// otherwise walks over.
    /// Whether it stands beside `position`, or can reach a cell that is.
    fn can_get_beside(&self, position: WorldPosition) -> bool {
        if self.mind.avoid == Some(position) {
            return false;
        }
        let beside = |cell: WorldPosition| {
            cell != position && cell.x.abs_diff(position.x).max(cell.y.abs_diff(position.y)) <= 1
        };
        beside(self.origin)
            || self
                .perception
                .reachable_cells
                .iter()
                .any(|&cell| beside(cell))
    }

    fn warm_up(&self) -> Option<Deliberation> {
        let origin = self.origin;
        let now = (self.needs.at.ticks() / 60) as u32;
        let hearth = self
            .perception
            .structures
            .iter()
            .filter(|structure| {
                structure.kind.burns()
                    && structure.working(now)
                    && self.can_get_beside(structure.position)
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
            trip: false,
            toward: Some(hearth),
        })
    }

    /// Makes something it knows how to make when it has none with that use
    /// (a blade when it carries no cutting edge) and carries what it takes.
    fn make_tool(&self, inventory: InventoryView) -> Option<Deliberation> {
        let has_edge = crate::Material::ALL
            .into_iter()
            .any(|material| material.properties().cutting && inventory.amount(material) > 0);
        // Any tool it knows how to put together, doesn't have, and carries
        // the parts for (an axe, a stone spear, a cloak, cord, a digging stick).
        let crafts = self.mind.crafts;
        let can_assemble = crate::RECIPES.iter().enumerate().any(|(index, recipe)| {
            crafts.knows(index)
                && recipe.output.properties().nutrition == 0
                && inventory.amount(recipe.output) == 0
                && match recipe.method {
                    Method::Hand => true,
                    Method::Edge => has_edge,
                    Method::Fire | Method::Smoke | Method::Rack => false,
                }
                && recipe
                    .inputs
                    .iter()
                    .all(|&(input, count)| inventory.amount(input) >= count)
        });
        if can_assemble {
            return Some(Deliberation::act(
                PhysicalGoal::Craft,
                self.origin,
                PolicyReason::Crafting,
            ));
        }
        if !self.mind.knows_knapping {
            return None;
        }
        if has_edge {
            // With an edge in hand: a basket, and for a hunter a spear, from
            // wood it carries.
            let has = |wanted: fn(crate::MaterialProperties) -> bool| {
                inventory
                    .carried()
                    .any(|(material, _)| wanted(material.properties()))
            };
            let lacks = !has(|properties| properties.room > 0)
                || (self.mind.hunts && !has(|properties| properties.strike > 0));
            return (lacks && inventory.amount(Material::Wood) >= TOOL_WOOD).then(|| {
                Deliberation::act(PhysicalGoal::Craft, self.origin, PolicyReason::Crafting)
            });
        }
        let (input, amount) = crate::Material::ALL
            .into_iter()
            .filter(|material| material.properties().cutting)
            .find_map(|material| material.made_from())?;
        if inventory.amount(input) >= amount {
            return Some(Deliberation::act(
                PhysicalGoal::Craft,
                self.origin,
                PolicyReason::Crafting,
            ));
        }
        // Picks up the stone for it when some is in view.
        nearest_resource_access(self.origin, self.perception, |found| {
            found == input && inventory.can_add(found)
        })
        .map(|target| {
            Deliberation::act(PhysicalGoal::GatherMaterial, target, PolicyReason::Crafting)
        })
    }

    /// Feeds a fire in view that is out or burning low, if it knows fire and
    /// carries something that burns: right away if beside it, otherwise walks over.
    fn tend_fire(&self, inventory: InventoryView) -> Option<Deliberation> {
        if !self.mind.knows_hearths
            || !crate::Material::ALL.into_iter().any(|material| {
                material.properties().fuel_seconds > 0 && inventory.amount(material) > 0
            })
        {
            return None;
        }
        let now = (self.needs.at.ticks() / 60) as u32;
        let origin = self.origin;
        let fire = self
            .perception
            .structures
            .iter()
            .filter(|structure| {
                structure.kind.burns()
                    && structure.state == crate::StructureState::Complete
                    && structure.fuel_until < now + LOW_FUEL_SECONDS
                    && self.can_get_beside(structure.position)
            })
            .min_by_key(|structure| (manhattan(origin, structure.position), structure.id.get()))?
            .position;
        if origin.x.abs_diff(fire.x).max(origin.y.abs_diff(fire.y)) <= 1 {
            return Some(Deliberation::act(
                PhysicalGoal::TendFire,
                origin,
                PolicyReason::Tending,
            ));
        }
        let (waypoint, heading) = self.waypoint_toward(fire)?;
        Some(Deliberation {
            selection: PolicySelection {
                goal: PhysicalGoal::Explore,
                target: Some(waypoint),
                reason: PolicyReason::Tending,
            },
            heading: Some(heading),
            trip: false,
            toward: Some(fire),
        })
    }

    /// Builds a hearth beside its home if it keeps fire and knows of none
    /// close by, gathering stone and wood for it first.
    fn make_hearth(&self, inventory: InventoryView) -> Option<Deliberation> {
        let map = self.mind.map;
        let home = self.mind.home?;
        if !self.mind.knows_hearths
            || manhattan(home, self.origin) > HOME_RANGE
            || map.remembers_near(LandmarkKind::HEARTH, home, HOME_RANGE)
        {
            return None;
        }
        let kind = crate::StructureKind::Hearth;
        if kind.affordable(inventory) {
            if manhattan(home, self.origin) > HEARTH_FROM_HOME {
                let (waypoint, heading) = self.waypoint_toward(home)?;
                return Some(Deliberation {
                    selection: PolicySelection {
                        goal: PhysicalGoal::Explore,
                        target: Some(waypoint),
                        reason: PolicyReason::HearthMaterials,
                    },
                    heading: Some(heading),
                    trip: false,
                    toward: None,
                });
            }
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
        if !self.can_handle(missing) {
            return None;
        }
        // No room for it: set down what it doesn't need first.
        if !inventory.can_add(missing) {
            return self.has_spare(inventory).then(|| {
                Deliberation::act(PhysicalGoal::Drop, self.origin, PolicyReason::Unloading)
            });
        }
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
            .filter(|&cell| {
                cell != origin
                    && manhattan(cell, threat) > manhattan(origin, threat)
                    && candidate_available(origin, self.perception, cell)
            })
            .map(|cell| (manhattan(cell, threat), cell))
            .max_by_key(|&(distance, cell)| (distance, -cell.y, -cell.x))?;
        Some(Deliberation {
            selection: PolicySelection {
                goal: PhysicalGoal::Explore,
                target: Some(cell),
                reason: PolicyReason::Fleeing,
            },
            heading: Some(heading_toward(origin, cell)),
            trip: false,
            toward: None,
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
            trip: false,
            toward: None,
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
        if self.mind.avoid == Some(destination) {
            return None;
        }
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
            trip: true,
            toward: Some(destination),
        })
    }

    fn gather_known_wood(&self, inventory: InventoryView) -> Option<Deliberation> {
        (self.may_build_hut()
            && inventory.amount(Material::Wood) < SHELTER_WOOD_COST
            && inventory.can_add(Material::Wood))
        .then(|| self.travel_to_known(LandmarkKind::WOOD, PhysicalGoal::GatherMaterial))
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
                    && manhattan(cell, destination) < current
                    && candidate_available(origin, self.perception, cell)
            })
            .map(|cell| (manhattan(cell, destination), cell))
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
                trip: false,
                toward: None,
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
            trip: false,
            toward: None,
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

/// The `MindInput::plans` bit for getting ready for hard seasons.
pub(crate) const PLANS_READY: u8 = 1 << 4;
/// The `MindInput::plans` bit for working, when idle, toward having what has
/// proved useful.
pub(crate) const PLANS_IDLE: u8 = 1 << 5;

/// Cells from a trip's destination that count as there.
const ARRIVED: u64 = 2;

/// Tries in a situation, and learned value there, for experience to vouch
/// for an option without thinking it through.
const TRUSTED_TRIES: u8 = 2;
const TRUSTED_VALUE: i32 = 3;

/// Plans given up for one need before instinct takes over until it eases.
const GIVE_UP_PLANNING: u8 = 2;

/// The policy reason a plan step for `need` (`NeedKind` index) shows.
const fn need_reason(need: usize) -> PolicyReason {
    match need {
        0 => PolicyReason::HungerThreshold,
        1 => PolicyReason::ThirstThreshold,
        2 => PolicyReason::RestThreshold,
        _ => PolicyReason::ExposureThreshold,
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
