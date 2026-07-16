use std::{collections::BTreeSet, error::Error, fmt};

use crate::{
    BaseResource, NeedKind, NeedQueryError, NeedThresholdEventOutcome, NeedThresholdOutcomeKind,
    PhysicalNeedsView, Standability, TraversalKind, WORLD_GENERATION_BOUNDS, WaterSource, World,
    WorldPosition, WorldQueryError, WorldRect,
    health::{
        DeathCause, DeathRecord, HealthDiagnostic, HealthDiagnosticKind, HealthState, HealthView,
    },
    needs::NeedState,
    policy::{
        PhysicalGoal, PhysicalPolicyView, PolicyAction, PolicyPhase, PolicyReason, PolicyState,
    },
    resources::{FOOD_CONSUMPTION, InventoryView, ResourceDeltas},
    routing::{RouteRequest, RouteRequestError},
    scheduler::{EventClass, ScheduleError, ScheduledEvent, Scheduler},
    sleep::{SleepQuality, SleepRequestError, SleepState, SleepView},
    spatial::{SpatialIndex, TransferError},
    structures::{
        BuildShelterError, SHELTER_WOOD_COST, StructureId, StructureStore, StructureView,
    },
};

pub const MAX_POPULATION: u32 = 10_000_000;

/// Stable dense population identity. IDs are allocated from zero in spawn order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct AgentId(u32);

impl AgentId {
    /// Reconstructs an opaque handle from its stable numeric value.
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    pub const fn get(self) -> u32 {
        self.0
    }
}

impl fmt::Display for AgentId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

/// Authoritative simulation time measured in fixed engine ticks.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct SimTime(u64);

impl SimTime {
    pub const ZERO: Self = Self(0);

    pub const fn from_ticks(ticks: u64) -> Self {
        Self(ticks)
    }

    pub const fn ticks(self) -> u64 {
        self.0
    }

    pub(crate) const fn checked_add(self, ticks: u64) -> Option<Self> {
        match self.0.checked_add(ticks) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum AgentActivity {
    Idle = 0,
    Moving = 1,
    Gathering = 2,
    Building = 3,
    Sleeping = 4,
    Incapacitated = 5,
    Dead = 6,
}

impl AgentActivity {
    pub(crate) const fn is_terminal(self) -> bool {
        matches!(self, Self::Incapacitated | Self::Dead)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgentView {
    pub id: AgentId,
    pub position: WorldPosition,
    pub activity: AgentActivity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(C)]
pub(crate) struct CompactPosition {
    pub(crate) x: i16,
    pub(crate) y: i16,
}

impl CompactPosition {
    pub(crate) fn checked(position: WorldPosition) -> Option<Self> {
        Some(Self {
            x: i16::try_from(position.x).ok()?,
            y: i16::try_from(position.y).ok()?,
        })
    }

    pub(crate) fn world(self) -> WorldPosition {
        WorldPosition {
            x: i64::from(self.x),
            y: i64::from(self.y),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub(crate) struct AgentRecord {
    position: CompactPosition,
    activity: AgentActivity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpawnInvalidReason {
    Water,
    BlockingFeature,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PopulationInitError {
    AlreadyInitialized,
    EmptyPopulation,
    PopulationTooLarge {
        requested: u32,
        maximum: u32,
    },
    RequestedPositionsExceedPopulation {
        positions: usize,
        population: u32,
    },
    EmptyActiveArea,
    ActiveAreaOutsideWorld,
    IncompleteResidency,
    RequestedPositionOutsideArea {
        position: WorldPosition,
    },
    DuplicatePosition {
        position: WorldPosition,
    },
    InvalidSpawn {
        position: WorldPosition,
        reason: SpawnInvalidReason,
    },
    InsufficientValidSpawnCells {
        requested: u32,
        found: u32,
    },
    AllocationFailed,
    EventSequenceExhausted,
}

impl fmt::Display for PopulationInitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AlreadyInitialized => formatter.write_str("population is already initialized"),
            Self::EmptyPopulation => {
                formatter.write_str("population must contain at least one agent")
            }
            Self::PopulationTooLarge { requested, maximum } => {
                write!(
                    formatter,
                    "population {requested} exceeds the {maximum}-agent limit"
                )
            }
            Self::RequestedPositionsExceedPopulation {
                positions,
                population,
            } => write!(
                formatter,
                "{positions} requested positions exceed population {population}"
            ),
            Self::EmptyActiveArea => formatter.write_str("active simulation area is empty"),
            Self::ActiveAreaOutsideWorld => {
                formatter.write_str("active simulation area is outside the world envelope")
            }
            Self::IncompleteResidency => {
                formatter.write_str("active simulation area is not completely resident")
            }
            Self::RequestedPositionOutsideArea { position } => write!(
                formatter,
                "requested position ({}, {}) is outside the active area",
                position.x, position.y
            ),
            Self::DuplicatePosition { position } => write!(
                formatter,
                "requested position ({}, {}) is duplicated",
                position.x, position.y
            ),
            Self::InvalidSpawn { position, reason } => write!(
                formatter,
                "requested position ({}, {}) is not standable: {reason:?}",
                position.x, position.y
            ),
            Self::InsufficientValidSpawnCells { requested, found } => write!(
                formatter,
                "active area has only {found} valid spawn cells for {requested} agents"
            ),
            Self::AllocationFailed => formatter.write_str("population allocation failed"),
            Self::EventSequenceExhausted => {
                formatter.write_str("event sequence exhausted during population initialization")
            }
        }
    }
}

impl Error for PopulationInitError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PopulationInit {
    pub active_area: WorldRect,
    pub population: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PopulationInitOutcome {
    pub first_id: AgentId,
    pub count: u32,
    pub active_area: WorldRect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct EventId(u64);

impl EventId {
    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MovementScheduled {
    pub event: EventId,
    pub completes_at: SimTime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RouteScheduled {
    pub first_event: EventId,
    pub first_completion: SimTime,
    pub destination: WorldPosition,
    pub expansions: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoveRequestError {
    PolicyControlled,
    MissingAgent,
    DeadAgent,
    InvalidStep,
    Unloaded,
    OutsideWorld,
    OutsideActiveArea,
    Blocked(TraversalKind),
    Occupied(AgentId),
    BlockedByStructure(StructureId),
    TimeOverflow,
    RescheduleLimit,
    EventSequenceExhausted,
}

impl fmt::Display for MoveRequestError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "movement request failed: {self:?}")
    }
}

impl Error for MoveRequestError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MovementOutcomeKind {
    Moved,
    StaleEvent,
    MissingAgent,
    DeadAgent,
    InvalidStep,
    Unloaded,
    OutsideWorld,
    OutsideActiveArea,
    Blocked(TraversalKind),
    Occupied(AgentId),
    BlockedByStructure(StructureId),
    InconsistentOccupancy,
    EventSequenceExhausted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MovementEventOutcome {
    pub event: EventId,
    pub agent: AgentId,
    pub due: SimTime,
    pub from: Option<WorldPosition>,
    pub target: WorldPosition,
    pub kind: MovementOutcomeKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteOutcomeKind {
    Arrived,
    Occupied(AgentId),
    NoPath { expansions: u16 },
    BudgetExhausted { expansions: u16 },
    Unloaded,
    OutsideWorld,
    OutsideActiveArea,
    Blocked(TraversalKind),
    BlockedByStructure(StructureId),
    TimeOverflow,
    RescheduleLimit,
    EventSequenceExhausted,
    InconsistentOccupancy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RouteEventOutcome {
    pub agent: AgentId,
    pub at: SimTime,
    pub destination: WorldPosition,
    pub kind: RouteOutcomeKind,
}

pub const MAX_PERCEPTION_RADIUS: u8 = 31;
pub const MAX_PERCEPTION_CELLS: u16 = 3_969;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PerceivedWater {
    pub position: WorldPosition,
    pub source: WaterSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PerceivedResource {
    pub position: WorldPosition,
    pub resource: BaseResource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhysicalPerception {
    pub area: WorldRect,
    pub agents: Vec<AgentView>,
    pub drinkable_water: Vec<PerceivedWater>,
    pub resources: Vec<PerceivedResource>,
    pub structures: Vec<StructureView>,
    pub traversable_cells: Vec<WorldPosition>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PerceptionError {
    MissingAgent,
    DeadAgent,
    RadiusTooLarge { requested: u8, maximum: u8 },
    EmptyArea,
    AreaOutsideActive,
    AreaTooLarge { requested: u64, maximum: u16 },
    Unloaded,
    OutsideWorld,
    AllocationFailed,
}

impl fmt::Display for PerceptionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "physical perception failed: {self:?}")
    }
}

impl Error for PerceptionError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub(crate) struct RouteState {
    destination: CompactPosition,
    max_expansions: u16,
}

#[derive(Clone, Copy)]
pub(crate) struct MovementEnvironment<'a> {
    pub(crate) world: &'a World,
    pub(crate) structures: &'a StructureStore,
}

#[derive(Debug, Default)]
pub(crate) struct Population {
    records: Vec<AgentRecord>,
    movement_generations: Vec<u32>,
    routes: Vec<Option<RouteState>>,
    needs: Vec<NeedState>,
    policies: Vec<PolicyState>,
    inventories: Vec<InventoryView>,
    sleeps: Vec<SleepState>,
    health: Vec<HealthState>,
    spatial: SpatialIndex,
    living_count: u32,
    active_count: u32,
    active_area: Option<WorldRect>,
    initialized: bool,
}

impl Population {
    pub(crate) fn initialize(
        &mut self,
        world: &World,
        now: SimTime,
        init: PopulationInit,
        requested_positions: &[WorldPosition],
    ) -> Result<PopulationInitOutcome, PopulationInitError> {
        if self.initialized {
            return Err(PopulationInitError::AlreadyInitialized);
        }
        if init.population == 0 {
            return Err(PopulationInitError::EmptyPopulation);
        }
        if init.population > MAX_POPULATION {
            return Err(PopulationInitError::PopulationTooLarge {
                requested: init.population,
                maximum: MAX_POPULATION,
            });
        }
        if requested_positions.len() > init.population as usize {
            return Err(PopulationInitError::RequestedPositionsExceedPopulation {
                positions: requested_positions.len(),
                population: init.population,
            });
        }
        if init.active_area.max.x <= init.active_area.min.x
            || init.active_area.max.y <= init.active_area.min.y
        {
            return Err(PopulationInitError::EmptyActiveArea);
        }
        if !WORLD_GENERATION_BOUNDS.contains_rect(init.active_area) {
            return Err(PopulationInitError::ActiveAreaOutsideWorld);
        }
        if !world.area_is_generated(init.active_area) {
            return Err(PopulationInitError::IncompleteResidency);
        }

        let capacity = init.population as usize;
        let mut positions = Vec::new();
        positions
            .try_reserve_exact(capacity)
            .map_err(|_| PopulationInitError::AllocationFailed)?;
        let mut occupied = BTreeSet::new();
        for &position in requested_positions {
            if !init.active_area.contains(position) {
                return Err(PopulationInitError::RequestedPositionOutsideArea { position });
            }
            let compact = CompactPosition::checked(position)
                .ok_or(PopulationInitError::ActiveAreaOutsideWorld)?;
            if !occupied.insert(compact) {
                return Err(PopulationInitError::DuplicatePosition { position });
            }
            if let Some(reason) = invalid_spawn_reason(world, position)? {
                return Err(PopulationInitError::InvalidSpawn { position, reason });
            }
            positions.push(compact);
        }

        'rows: for y in init.active_area.min.y..init.active_area.max.y {
            for x in init.active_area.min.x..init.active_area.max.x {
                if positions.len() == capacity {
                    break 'rows;
                }
                let position = WorldPosition { x, y };
                let compact = CompactPosition::checked(position)
                    .ok_or(PopulationInitError::ActiveAreaOutsideWorld)?;
                if occupied.contains(&compact) || invalid_spawn_reason(world, position)?.is_some() {
                    continue;
                }
                occupied.insert(compact);
                positions.push(compact);
            }
        }
        if positions.len() != capacity {
            return Err(PopulationInitError::InsufficientValidSpawnCells {
                requested: init.population,
                found: positions.len() as u32,
            });
        }

        let mut records = Vec::new();
        records
            .try_reserve_exact(capacity)
            .map_err(|_| PopulationInitError::AllocationFailed)?;
        records.extend(positions.into_iter().map(|position| AgentRecord {
            position,
            activity: AgentActivity::Idle,
        }));
        let mut movement_generations = Vec::new();
        movement_generations
            .try_reserve_exact(capacity)
            .map_err(|_| PopulationInitError::AllocationFailed)?;
        movement_generations.resize(capacity, 0);
        let mut routes = Vec::new();
        routes
            .try_reserve_exact(capacity)
            .map_err(|_| PopulationInitError::AllocationFailed)?;
        routes.resize(capacity, None);
        let mut needs = Vec::new();
        needs
            .try_reserve_exact(capacity)
            .map_err(|_| PopulationInitError::AllocationFailed)?;
        needs.resize(capacity, NeedState::new(now));
        let mut policies = Vec::new();
        policies
            .try_reserve_exact(capacity)
            .map_err(|_| PopulationInitError::AllocationFailed)?;
        policies.resize(capacity, PolicyState::default());
        let mut inventories = Vec::new();
        inventories
            .try_reserve_exact(capacity)
            .map_err(|_| PopulationInitError::AllocationFailed)?;
        inventories.resize(capacity, InventoryView::default());
        let mut sleeps = Vec::new();
        sleeps
            .try_reserve_exact(capacity)
            .map_err(|_| PopulationInitError::AllocationFailed)?;
        sleeps.resize(capacity, SleepState::default());
        let mut health = Vec::new();
        health
            .try_reserve_exact(capacity)
            .map_err(|_| PopulationInitError::AllocationFailed)?;
        health.resize(capacity, HealthState::default());
        let spatial = SpatialIndex::from_positions(
            records
                .iter()
                .enumerate()
                .map(|(raw, record)| (AgentId(raw as u32), record.position.world())),
        );

        self.records = records;
        self.movement_generations = movement_generations;
        self.routes = routes;
        self.needs = needs;
        self.policies = policies;
        self.inventories = inventories;
        self.sleeps = sleeps;
        self.health = health;
        self.spatial = spatial;
        self.living_count = init.population;
        self.active_count = init.population;
        self.active_area = Some(init.active_area);
        self.initialized = true;
        Ok(PopulationInitOutcome {
            first_id: AgentId(0),
            count: init.population,
            active_area: init.active_area,
        })
    }

    pub(crate) fn schedule_movement(
        &mut self,
        scheduler: &mut Scheduler,
        environment: MovementEnvironment<'_>,
        now: SimTime,
        agent: AgentId,
        target: WorldPosition,
    ) -> Result<MovementScheduled, MoveRequestError> {
        let scheduled = self.schedule_step(scheduler, environment, now, agent, target)?;
        self.routes[agent.0 as usize] = None;
        Ok(scheduled)
    }

    fn schedule_step(
        &mut self,
        scheduler: &mut Scheduler,
        environment: MovementEnvironment<'_>,
        now: SimTime,
        agent: AgentId,
        target: WorldPosition,
    ) -> Result<MovementScheduled, MoveRequestError> {
        let index = agent.0 as usize;
        let record = self
            .records
            .get(index)
            .ok_or(MoveRequestError::MissingAgent)?;
        if record.activity.is_terminal() {
            return Err(MoveRequestError::DeadAgent);
        }
        let activity_changes = record.activity != AgentActivity::Moving;
        if !scheduler.can_schedule(if activity_changes { 6 } else { 1 }) {
            return Err(MoveRequestError::EventSequenceExhausted);
        }
        if !WORLD_GENERATION_BOUNDS.contains(target) {
            return Err(MoveRequestError::OutsideWorld);
        }
        if !self
            .active_area
            .is_some_and(|active_area| active_area.contains(target))
        {
            return Err(MoveRequestError::OutsideActiveArea);
        }
        if let Some(occupant) = self.spatial.occupant(target)
            && occupant != agent
        {
            return Err(MoveRequestError::Occupied(occupant));
        }
        if let Some(structure) = environment.structures.structure_at(target) {
            return Err(MoveRequestError::BlockedByStructure(structure));
        }
        let step = environment
            .world
            .traversal_step(record.position.world(), target)
            .map_err(map_query_error)?;
        let cost = step.cost().ok_or(MoveRequestError::Blocked(step.kind()))?;
        let due = now
            .checked_add(u64::from(cost))
            .ok_or(MoveRequestError::TimeOverflow)?;
        let generation = self.movement_generations[index]
            .checked_add(1)
            .ok_or(MoveRequestError::RescheduleLimit)?;
        let compact_target =
            CompactPosition::checked(target).ok_or(MoveRequestError::OutsideWorld)?;
        let sequence = scheduler
            .schedule_movement(due, agent, generation, compact_target)
            .map_err(|error| match error {
                ScheduleError::SequenceExhausted => MoveRequestError::EventSequenceExhausted,
            })?;
        self.movement_generations[index] = generation;
        if activity_changes {
            self.transition_activity(scheduler, now, agent, AgentActivity::Moving)
                .expect("event sequence capacity was prechecked");
        }
        Ok(MovementScheduled {
            event: EventId(sequence),
            completes_at: due,
        })
    }

    pub(crate) fn schedule_route_step(
        &mut self,
        scheduler: &mut Scheduler,
        environment: MovementEnvironment<'_>,
        now: SimTime,
        agent: AgentId,
        request: RouteRequest,
        target: WorldPosition,
    ) -> Result<MovementScheduled, MoveRequestError> {
        let destination =
            CompactPosition::checked(request.destination).ok_or(MoveRequestError::OutsideWorld)?;
        let scheduled = self.schedule_step(scheduler, environment, now, agent, target)?;
        self.routes[agent.0 as usize] = Some(RouteState {
            destination,
            max_expansions: request.max_expansions,
        });
        Ok(scheduled)
    }

    pub(crate) fn apply_movement(
        &mut self,
        scheduler: &mut Scheduler,
        environment: MovementEnvironment<'_>,
        event: ScheduledEvent,
    ) -> MovementEventOutcome {
        let target = event.target.world();
        let index = event.agent.0 as usize;
        let Some(record) = self.records.get_mut(index) else {
            return movement_outcome(event, None, target, MovementOutcomeKind::MissingAgent);
        };
        let from = record.position.world();
        if record.activity.is_terminal() {
            return movement_outcome(event, Some(from), target, MovementOutcomeKind::DeadAgent);
        }
        if self.movement_generations[index] != event.generation
            || record.activity != AgentActivity::Moving
        {
            return movement_outcome(event, Some(from), target, MovementOutcomeKind::StaleEvent);
        }
        if !scheduler.can_schedule(5) {
            self.settle_activity_without_events(event.due, event.agent, AgentActivity::Idle);
            return movement_outcome(
                event,
                Some(from),
                target,
                MovementOutcomeKind::EventSequenceExhausted,
            );
        }
        if !WORLD_GENERATION_BOUNDS.contains(target) {
            let outcome =
                movement_outcome(event, Some(from), target, MovementOutcomeKind::OutsideWorld);
            self.transition_activity(scheduler, event.due, event.agent, AgentActivity::Idle)
                .expect("event sequence capacity was prechecked");
            return outcome;
        }
        if !self
            .active_area
            .is_some_and(|active_area| active_area.contains(target))
        {
            let outcome = movement_outcome(
                event,
                Some(from),
                target,
                MovementOutcomeKind::OutsideActiveArea,
            );
            self.transition_activity(scheduler, event.due, event.agent, AgentActivity::Idle)
                .expect("event sequence capacity was prechecked");
            return outcome;
        }
        let kind = if let Some(structure) = environment.structures.structure_at(target) {
            MovementOutcomeKind::BlockedByStructure(structure)
        } else {
            match environment.world.traversal_step(from, target) {
                Ok(step) if step.is_passable() => {
                    match self.spatial.transfer(event.agent, from, target) {
                        Ok(()) => {
                            record.position = event.target;
                            MovementOutcomeKind::Moved
                        }
                        Err(TransferError::Occupied(occupant)) => {
                            MovementOutcomeKind::Occupied(occupant)
                        }
                        Err(TransferError::SourceMismatch) => {
                            MovementOutcomeKind::InconsistentOccupancy
                        }
                    }
                }
                Ok(step) => MovementOutcomeKind::Blocked(step.kind()),
                Err(error) => map_event_query_error(error),
            }
        };
        let outcome = movement_outcome(event, Some(from), target, kind);
        let route_continues = self.routes[index].is_some()
            && matches!(
                kind,
                MovementOutcomeKind::Moved | MovementOutcomeKind::Occupied(_)
            );
        if !route_continues {
            self.transition_activity(scheduler, event.due, event.agent, AgentActivity::Idle)
                .expect("event sequence capacity was prechecked");
        }
        outcome
    }

    pub(crate) fn finish_route_activity(
        &mut self,
        scheduler: &mut Scheduler,
        now: SimTime,
        agent: AgentId,
    ) -> Result<(), MoveRequestError> {
        let Some(record) = self.records.get(agent.0 as usize) else {
            return Err(MoveRequestError::MissingAgent);
        };
        if record.activity == AgentActivity::Idle || record.activity.is_terminal() {
            return Ok(());
        }
        match self.transition_activity(scheduler, now, agent, AgentActivity::Idle) {
            Ok(()) => Ok(()),
            Err(error) => {
                self.settle_activity_without_events(now, agent, AgentActivity::Idle);
                Err(error)
            }
        }
    }

    pub(crate) fn initialize_need_events(
        &mut self,
        scheduler: &mut Scheduler,
        now: SimTime,
    ) -> Result<(), ScheduleError> {
        for index in 0..self.needs.len() {
            let agent = AgentId(index as u32);
            let state = self.needs[index];
            self.schedule_need_thresholds(scheduler, agent, state, now)?;
            self.reschedule_health(scheduler, agent, state, now)?;
        }
        Ok(())
    }

    pub(crate) fn apply_health_consequence(
        &mut self,
        scheduler: &mut Scheduler,
        event: ScheduledEvent,
    ) -> HealthDiagnostic {
        let index = event.agent.0 as usize;
        let Some(health) = self.health.get_mut(index) else {
            return HealthDiagnostic {
                agent: event.agent,
                at: event.due,
                cause: None,
                before: 0,
                after: 0,
                kind: HealthDiagnosticKind::StaleEvent,
            };
        };
        let outcome = health.apply(event.generation, self.needs[index], event.due, event.agent);
        if outcome.kind != HealthDiagnosticKind::Died
            && outcome.kind != HealthDiagnosticKind::StaleEvent
            && let Some(due) = health.schedule_next_interval(event.due)
        {
            let _ = scheduler.schedule_health_consequence(
                due,
                event.agent,
                health.generation(),
                event.need,
            );
        }
        outcome
    }

    pub(crate) fn health_view(&self, agent: AgentId) -> Option<HealthView> {
        self.health
            .get(agent.0 as usize)
            .copied()
            .map(|state| state.view(agent))
    }

    pub(crate) fn living_count(&self) -> usize {
        self.living_count as usize
    }

    pub(crate) fn active_count(&self) -> usize {
        self.active_count as usize
    }

    pub(crate) fn incapacitate(&mut self, now: SimTime, agent: AgentId) {
        let index = agent.0 as usize;
        if !self
            .records
            .get(index)
            .is_some_and(|record| record.activity != AgentActivity::Dead)
        {
            return;
        }
        if self.records[index].activity == AgentActivity::Incapacitated {
            return;
        }
        self.routes[index] = None;
        self.movement_generations[index] = self.movement_generations[index].wrapping_add(1);
        self.policies[index].phase = PolicyPhase::Dormant;
        self.policies[index].generation = self.policies[index].generation.wrapping_add(1);
        self.sleeps[index] = SleepState::default();
        self.active_count = self.active_count.saturating_sub(1);
        self.settle_activity_without_events(now, agent, AgentActivity::Incapacitated);
    }

    pub(crate) fn finalize_death(
        &mut self,
        at: SimTime,
        agent: AgentId,
        cause: DeathCause,
    ) -> Option<DeathRecord> {
        let index = agent.0 as usize;
        let record = self.records.get(index)?;
        if record.activity == AgentActivity::Dead {
            return None;
        }
        let position = record.position.world();
        let was_active = !record.activity.is_terminal();
        self.routes[index] = None;
        self.movement_generations[index] = self.movement_generations[index].wrapping_add(1);
        self.policies[index].phase = PolicyPhase::Dormant;
        self.policies[index].generation = self.policies[index].generation.wrapping_add(1);
        self.sleeps[index] = SleepState::default();
        self.spatial.remove(agent, position);
        self.living_count = self.living_count.saturating_sub(1);
        if was_active {
            self.active_count = self.active_count.saturating_sub(1);
        }
        self.settle_activity_without_events(at, agent, AgentActivity::Dead);
        Some(DeathRecord {
            agent,
            cause,
            at,
            position,
        })
    }

    pub(crate) fn needs_view(
        &self,
        agent: AgentId,
        now: SimTime,
    ) -> Result<PhysicalNeedsView, NeedQueryError> {
        let record = self
            .records
            .get(agent.0 as usize)
            .ok_or(NeedQueryError::MissingAgent)?;
        if record.activity.is_terminal() {
            return Err(NeedQueryError::DeadAgent);
        }
        Ok(self.needs[agent.0 as usize].view(agent, now))
    }

    pub(crate) fn apply_need_threshold(
        &mut self,
        event: ScheduledEvent,
    ) -> NeedThresholdEventOutcome {
        let index = event.agent.0 as usize;
        let Some(record) = self.records.get(index) else {
            return need_outcome(event, None, NeedThresholdOutcomeKind::MissingAgent);
        };
        if record.activity.is_terminal() {
            return need_outcome(event, None, NeedThresholdOutcomeKind::DeadAgent);
        }
        let (outcome, value) =
            self.needs[index].apply_threshold(event.generation, event.need, event.due);
        need_outcome(event, Some(value), outcome)
    }

    pub(crate) fn policy_view(&self, agent: AgentId) -> Option<PhysicalPolicyView> {
        self.policies
            .get(agent.0 as usize)
            .copied()
            .map(|state| state.view(agent))
    }

    pub(crate) fn first_non_idle_agent(&self) -> Option<AgentId> {
        self.records
            .iter()
            .position(|record| record.activity != AgentActivity::Idle)
            .map(|index| AgentId(index as u32))
    }

    pub(crate) fn activate_policy(
        &mut self,
        scheduler: &mut Scheduler,
        due: SimTime,
    ) -> Result<(), ScheduleError> {
        for (index, state) in self.policies.iter_mut().enumerate() {
            let generation = state
                .next_generation()
                .ok_or(ScheduleError::SequenceExhausted)?;
            state.phase = PolicyPhase::DecisionPending;
            state.goal = PhysicalGoal::Wait;
            state.reason = PolicyReason::InitialDecision;
            scheduler.schedule_decision(
                due,
                AgentId(index as u32),
                generation,
                PhysicalGoal::Wait,
            )?;
        }
        Ok(())
    }

    pub(crate) fn policy_event_is_current(&self, event: ScheduledEvent) -> bool {
        let index = event.agent.0 as usize;
        self.records
            .get(index)
            .is_some_and(|record| !record.activity.is_terminal())
            && self.policies[index].event_is_current(event.generation)
    }

    pub(crate) fn policy_context(
        &self,
        agent: AgentId,
        now: SimTime,
    ) -> Option<(AgentView, PhysicalNeedsView, InventoryView)> {
        let view = self.view(agent)?;
        (!view.activity.is_terminal()).then(|| {
            (
                view,
                self.needs[agent.0 as usize].view(agent, now),
                self.inventories[agent.0 as usize],
            )
        })
    }

    pub(crate) fn commit_policy_route(
        &mut self,
        agent: AgentId,
        goal: PhysicalGoal,
        target: WorldPosition,
        reason: PolicyReason,
    ) {
        let state = &mut self.policies[agent.0 as usize];
        state.goal = goal;
        state.target =
            CompactPosition::checked(target).expect("policy target is inside active area");
        state.reason = reason;
        state.phase = PolicyPhase::Routing;
    }

    pub(crate) fn policy_commitment(
        &self,
        agent: AgentId,
    ) -> Option<(PhysicalGoal, WorldPosition, PolicyReason)> {
        let state = *self.policies.get(agent.0 as usize)?;
        matches!(state.phase, PolicyPhase::Routing | PolicyPhase::Acting)
            .then(|| (state.goal, state.target.world(), state.reason))
    }

    pub(crate) fn schedule_policy_decision(
        &mut self,
        scheduler: &mut Scheduler,
        now: SimTime,
        agent: AgentId,
        delay: u64,
        reason: PolicyReason,
        retry: bool,
    ) -> Result<SimTime, MoveRequestError> {
        let due = now
            .checked_add(delay)
            .ok_or(MoveRequestError::TimeOverflow)?;
        if !scheduler.can_schedule(1) {
            return Err(MoveRequestError::EventSequenceExhausted);
        }
        let state = self
            .policies
            .get_mut(agent.0 as usize)
            .ok_or(MoveRequestError::MissingAgent)?;
        let generation = state
            .next_generation()
            .ok_or(MoveRequestError::RescheduleLimit)?;
        scheduler
            .schedule_decision(due, agent, generation, state.goal)
            .map_err(|_| MoveRequestError::EventSequenceExhausted)?;
        state.reason = reason;
        state.phase = if retry {
            state.retries = state.retries.saturating_add(1);
            PolicyPhase::Backoff
        } else {
            PolicyPhase::DecisionPending
        };
        Ok(due)
    }

    pub(crate) fn interrupt_for_policy_decision(
        &mut self,
        scheduler: &mut Scheduler,
        now: SimTime,
        agent: AgentId,
        schedule_decision: bool,
    ) -> Result<(SimTime, Option<SleepState>), MoveRequestError> {
        let index = agent.0 as usize;
        let record = self
            .records
            .get(index)
            .ok_or(MoveRequestError::MissingAgent)?;
        if record.activity.is_terminal() {
            return Err(MoveRequestError::DeadAgent);
        }
        let interrupted_sleep = self.sleeps[index].is_active().then_some(self.sleeps[index]);
        let due = if schedule_decision {
            now.checked_add(1).ok_or(MoveRequestError::TimeOverflow)?
        } else {
            now
        };
        let movement_generation = if record.activity == AgentActivity::Moving {
            self.movement_generations[index]
                .checked_add(1)
                .ok_or(MoveRequestError::RescheduleLimit)?
        } else {
            self.movement_generations[index]
        };
        if schedule_decision && self.policies[index].generation == u32::MAX {
            return Err(MoveRequestError::RescheduleLimit);
        }
        let transition_events = if self.needs[index].requires_transition(AgentActivity::Idle) {
            5
        } else {
            0
        };
        if !scheduler.can_schedule(transition_events + u64::from(schedule_decision)) {
            return Err(MoveRequestError::EventSequenceExhausted);
        }
        if record.activity == AgentActivity::Moving {
            self.movement_generations[index] = movement_generation;
            self.routes[index] = None;
        }
        self.transition_activity(scheduler, now, agent, AgentActivity::Idle)
            .expect("event sequence capacity was prechecked");
        self.sleeps[index] = SleepState::default();
        if schedule_decision {
            self.schedule_policy_decision(scheduler, now, agent, 1, PolicyReason::Retry, false)?;
        } else {
            self.policies[index].phase = PolicyPhase::Dormant;
        }
        Ok((due, interrupted_sleep))
    }

    pub(crate) fn force_interrupt_sleep(
        &mut self,
        now: SimTime,
        agent: AgentId,
    ) -> Option<SleepState> {
        let index = agent.0 as usize;
        let state = self
            .sleeps
            .get(index)
            .copied()?
            .is_active()
            .then_some(self.sleeps[index])?;
        self.settle_activity_without_events(now, agent, AgentActivity::Idle);
        self.policies[index].phase = PolicyPhase::Dormant;
        self.sleeps[index] = SleepState::default();
        Some(state)
    }

    pub(crate) fn force_settle_idle(&mut self, now: SimTime, agent: AgentId) {
        let index = agent.0 as usize;
        if index >= self.records.len() || self.records[index].activity.is_terminal() {
            return;
        }
        self.settle_activity_without_events(now, agent, AgentActivity::Idle);
        self.policies[index].phase = PolicyPhase::Dormant;
        self.sleeps[index] = SleepState::default();
    }

    pub(crate) fn validate_sleep_location(
        &self,
        world: &World,
        now: SimTime,
        agent: AgentId,
        position: WorldPosition,
        sheltered: bool,
        structure: Option<StructureId>,
    ) -> Result<SleepQuality, SleepRequestError> {
        let index = agent.0 as usize;
        let record = self
            .records
            .get(index)
            .ok_or(SleepRequestError::MissingAgent)?;
        if record.activity.is_terminal() {
            return Err(SleepRequestError::DeadAgent);
        }
        if record.activity != AgentActivity::Idle {
            return Err(SleepRequestError::AgentCommitted);
        }
        if !self
            .active_area
            .expect("initialized population")
            .contains(position)
        {
            return Err(SleepRequestError::OutsideActiveArea);
        }
        match world.standability_at(position) {
            Ok(Standability::Standable) => {}
            Ok(Standability::BlockedByWater) => return Err(SleepRequestError::Water),
            Ok(Standability::BlockedByFeature) => {
                return Err(SleepRequestError::BlockingFeature);
            }
            Err(WorldQueryError::Unloaded) => return Err(SleepRequestError::Unloaded),
            Err(WorldQueryError::OutsideWorldBounds) => {
                return Err(SleepRequestError::OutsideWorld);
            }
            Err(WorldQueryError::NonCardinalStep) => unreachable!("standing queries have no step"),
        }
        if let Some(occupant) = self.spatial.occupant(position)
            && occupant != agent
        {
            return Err(SleepRequestError::Occupied(occupant));
        }
        if let Some(structure) = structure {
            return Err(SleepRequestError::StructureOccupied(structure));
        }
        if !sheltered
            && self.needs[index]
                .view(agent, now)
                .exposure
                .threshold_reached
        {
            return Err(SleepRequestError::UnsafeExposure);
        }
        if record.position.world() != position {
            return Err(SleepRequestError::NotAtLocation);
        }
        Ok(if sheltered {
            SleepQuality::Sheltered
        } else {
            SleepQuality::OpenGround
        })
    }

    pub(crate) fn schedule_sleep(
        &mut self,
        scheduler: &mut Scheduler,
        now: SimTime,
        agent: AgentId,
        position: WorldPosition,
        quality: SleepQuality,
        reason: PolicyReason,
    ) -> Result<SleepView, SleepRequestError> {
        let index = agent.0 as usize;
        let compact = CompactPosition::checked(position).ok_or(SleepRequestError::OutsideWorld)?;
        let due = self.needs[index]
            .sleep_recovery_due_for(quality, now, reason == PolicyReason::ExposureThreshold)
            .ok_or(SleepRequestError::TimeOverflow)?;
        if self.policies[index].generation == u32::MAX {
            return Err(SleepRequestError::RescheduleLimit);
        }
        let transition_events = if self.needs[index].requires_transition(AgentActivity::Sleeping) {
            5
        } else {
            0
        };
        if !scheduler.can_schedule(transition_events + 1) {
            return Err(SleepRequestError::EventSequenceExhausted);
        }
        if self.needs[index].transition_sleep(quality, now) {
            let state = self.needs[index];
            self.schedule_need_thresholds(scheduler, agent, state, now)
                .expect("event sequence capacity was prechecked");
            self.reschedule_health(scheduler, agent, state, now)
                .expect("event sequence capacity was prechecked");
        }
        self.records[index].activity = AgentActivity::Sleeping;
        let state = &mut self.policies[index];
        let generation = state
            .next_generation()
            .expect("policy generation was prechecked");
        scheduler
            .schedule_wake(due, agent, generation, compact)
            .expect("event sequence capacity was prechecked");
        state.goal = PhysicalGoal::Sleep;
        state.target = compact;
        state.reason = reason;
        state.phase = PolicyPhase::Acting;
        state.retries = 0;
        self.sleeps[index] = SleepState::active(now, due, quality);
        Ok(self.sleep_view(agent).expect("sleep was just activated"))
    }

    pub(crate) fn sleep_view(&self, agent: AgentId) -> Option<SleepView> {
        let index = agent.0 as usize;
        let state = *self.sleeps.get(index)?;
        state.is_active().then(|| SleepView {
            agent,
            position: self.records[index].position.world(),
            started_at: state.started_at,
            planned_wake: state.planned_wake,
            quality: state.quality,
        })
    }

    pub(crate) fn finish_sleep(&mut self, agent: AgentId) -> Option<SleepView> {
        let view = self.sleep_view(agent)?;
        self.sleeps[agent.0 as usize] = SleepState::default();
        Some(view)
    }

    pub(crate) fn schedule_policy_action(
        &mut self,
        scheduler: &mut Scheduler,
        now: SimTime,
        agent: AgentId,
        action: PolicyAction,
    ) -> Result<SimTime, MoveRequestError> {
        let due = now
            .checked_add(action.duration)
            .ok_or(MoveRequestError::TimeOverflow)?;
        let activity = match action.goal {
            PhysicalGoal::Sleep => AgentActivity::Sleeping,
            PhysicalGoal::GatherMaterial => AgentActivity::Gathering,
            PhysicalGoal::BuildShelter => AgentActivity::Building,
            PhysicalGoal::SeekWater
            | PhysicalGoal::SeekFood
            | PhysicalGoal::Drink
            | PhysicalGoal::Eat
            | PhysicalGoal::SeekShelter
            | PhysicalGoal::Wait => AgentActivity::Idle,
            PhysicalGoal::Incapacitated => AgentActivity::Incapacitated,
        };
        if !scheduler.can_schedule(
            if self.needs[agent.0 as usize].requires_transition(activity) {
                6
            } else {
                1
            },
        ) {
            return Err(MoveRequestError::EventSequenceExhausted);
        }
        self.transition_activity(scheduler, now, agent, activity)?;
        let state = &mut self.policies[agent.0 as usize];
        let generation = state
            .next_generation()
            .ok_or(MoveRequestError::RescheduleLimit)?;
        let compact =
            CompactPosition::checked(action.target).ok_or(MoveRequestError::OutsideWorld)?;
        scheduler
            .schedule_action_completion(due, agent, generation, action.goal, compact)
            .map_err(|_| MoveRequestError::EventSequenceExhausted)?;
        state.goal = action.goal;
        state.target = compact;
        state.reason = action.reason;
        state.phase = PolicyPhase::Acting;
        state.retries = 0;
        Ok(due)
    }

    pub(crate) fn complete_policy_action(
        &mut self,
        scheduler: &mut Scheduler,
        event: ScheduledEvent,
    ) -> Result<Option<(PhysicalGoal, WorldPosition, PolicyReason)>, MoveRequestError> {
        if !self.policy_event_is_current(event) {
            return Ok(None);
        }
        let index = event.agent.0 as usize;
        let state = self.policies[index];
        if let Err(error) =
            self.transition_activity(scheduler, event.due, event.agent, AgentActivity::Idle)
        {
            self.settle_activity_without_events(event.due, event.agent, AgentActivity::Idle);
            self.policies[index].phase = PolicyPhase::Dormant;
            return Err(error);
        }
        self.policies[index].phase = PolicyPhase::Dormant;
        Ok(Some((state.goal, state.target.world(), state.reason)))
    }

    pub(crate) fn inventory(&self, agent: AgentId) -> Option<InventoryView> {
        let index = agent.0 as usize;
        self.records
            .get(index)
            .is_some_and(|record| !record.activity.is_terminal())
            .then(|| self.inventories[index])
    }

    pub(crate) fn can_build_shelter(&self, agent: AgentId) -> bool {
        self.inventory(agent)
            .is_some_and(|inventory| inventory.wood >= SHELTER_WOOD_COST)
    }

    pub(crate) fn consume_shelter_materials(
        &mut self,
        agent: AgentId,
    ) -> Result<(), BuildShelterError> {
        let inventory = self
            .inventories
            .get_mut(agent.0 as usize)
            .ok_or(BuildShelterError::MissingAgent)?;
        if inventory.wood < SHELTER_WOOD_COST {
            return Err(BuildShelterError::InsufficientMaterials);
        }
        inventory.wood -= SHELTER_WOOD_COST;
        Ok(())
    }

    pub(crate) fn refund_shelter_materials(&mut self, agent: AgentId) {
        let inventory = &mut self.inventories[agent.0 as usize];
        inventory.wood = inventory.wood.saturating_add(SHELTER_WOOD_COST);
    }

    pub(crate) fn add_inventory(
        &mut self,
        agent: AgentId,
        kind: crate::ResourceKind,
        amount: u8,
    ) -> u8 {
        let inventory = &mut self.inventories[agent.0 as usize];
        let accepted = inventory.remaining_capacity(kind).min(amount);
        let slot = match kind {
            crate::ResourceKind::Food => &mut inventory.food,
            crate::ResourceKind::Wood => &mut inventory.wood,
            crate::ResourceKind::Stone => &mut inventory.stone,
        };
        *slot += accepted;
        accepted
    }

    pub(crate) fn apply_need_relief(
        &mut self,
        scheduler: &mut Scheduler,
        now: SimTime,
        agent: AgentId,
        kind: NeedKind,
        amount: u16,
        consume_food: bool,
    ) -> Result<(), ActionEffectError> {
        let index = agent.0 as usize;
        if consume_food && self.inventories[index].food < FOOD_CONSUMPTION {
            return Err(ActionEffectError::NoEdibleInventory);
        }
        if !scheduler.can_schedule(5) {
            return Err(ActionEffectError::EventSequenceExhausted);
        }
        let mut next = self.needs[index];
        next.relieve(kind, amount, now);
        self.schedule_need_thresholds(scheduler, agent, next, now)
            .map_err(|_| ActionEffectError::EventSequenceExhausted)?;
        self.reschedule_health(scheduler, agent, next, now)
            .map_err(|_| ActionEffectError::EventSequenceExhausted)?;
        if consume_food {
            self.inventories[index].food -= FOOD_CONSUMPTION;
        }
        self.needs[index] = next;
        Ok(())
    }

    pub(crate) fn policy_retries(&self, agent: AgentId) -> u8 {
        self.policies
            .get(agent.0 as usize)
            .map_or(0, |state| state.retries)
    }

    fn transition_activity(
        &mut self,
        scheduler: &mut Scheduler,
        now: SimTime,
        agent: AgentId,
        activity: AgentActivity,
    ) -> Result<(), MoveRequestError> {
        let index = agent.0 as usize;
        if self.needs[index].requires_transition(activity) && !scheduler.can_schedule(5) {
            return Err(MoveRequestError::EventSequenceExhausted);
        }
        if self.needs[index].transition(activity, now) {
            let state = self.needs[index];
            self.schedule_need_thresholds(scheduler, agent, state, now)
                .map_err(|_| MoveRequestError::EventSequenceExhausted)?;
            self.reschedule_health(scheduler, agent, state, now)
                .map_err(|_| MoveRequestError::EventSequenceExhausted)?;
        }
        self.records[index].activity = activity;
        Ok(())
    }

    fn settle_activity_without_events(
        &mut self,
        now: SimTime,
        agent: AgentId,
        activity: AgentActivity,
    ) {
        let index = agent.0 as usize;
        self.needs[index].transition(activity, now);
        self.records[index].activity = activity;
    }

    fn schedule_need_thresholds(
        &self,
        scheduler: &mut Scheduler,
        agent: AgentId,
        state: NeedState,
        now: SimTime,
    ) -> Result<(), ScheduleError> {
        for kind in NeedKind::ALL {
            if let Some(due) = state.threshold_due(kind, now) {
                scheduler.schedule_need_threshold(due, agent, state.generation(), kind)?;
            }
        }
        Ok(())
    }

    fn reschedule_health(
        &mut self,
        scheduler: &mut Scheduler,
        agent: AgentId,
        needs: NeedState,
        now: SimTime,
    ) -> Result<(), ScheduleError> {
        let health = &mut self.health[agent.0 as usize];
        if let Some((due, need)) = health.reschedule(needs, now) {
            scheduler.schedule_health_consequence(due, agent, health.generation(), need)?;
        }
        Ok(())
    }

    pub(crate) fn views(&self, limit: usize) -> impl Iterator<Item = AgentView> + '_ {
        self.records
            .iter()
            .take(limit)
            .enumerate()
            .map(|(index, record)| AgentView {
                id: AgentId(index as u32),
                position: record.position.world(),
                activity: record.activity,
            })
    }

    pub(crate) fn view(&self, agent: AgentId) -> Option<AgentView> {
        let record = self.records.get(agent.0 as usize)?;
        Some(AgentView {
            id: agent,
            position: record.position.world(),
            activity: record.activity,
        })
    }

    pub(crate) fn route_context(
        &self,
        agent: AgentId,
    ) -> Result<(WorldPosition, WorldRect), RouteRequestError> {
        let record = self
            .records
            .get(agent.0 as usize)
            .ok_or(RouteRequestError::MissingAgent)?;
        if record.activity.is_terminal() {
            return Err(RouteRequestError::DeadAgent);
        }
        Ok((
            record.position.world(),
            self.active_area.expect("initialized population"),
        ))
    }

    pub(crate) fn route_request(&self, agent: AgentId) -> Option<RouteRequest> {
        self.routes
            .get(agent.0 as usize)
            .copied()
            .flatten()
            .map(|route| RouteRequest {
                destination: route.destination.world(),
                max_expansions: route.max_expansions,
            })
    }

    pub(crate) fn clear_route(&mut self, agent: AgentId) {
        if let Some(route) = self.routes.get_mut(agent.0 as usize) {
            *route = None;
        }
    }

    pub(crate) fn spatial(&self) -> &SpatialIndex {
        &self.spatial
    }

    pub(crate) fn active_area(&self) -> Option<WorldRect> {
        self.active_area
    }

    pub(crate) fn perceive(
        &self,
        world: &World,
        resource_deltas: &ResourceDeltas,
        structures: &StructureStore,
        agent: AgentId,
        radius: u8,
    ) -> Result<PhysicalPerception, PerceptionError> {
        if radius > MAX_PERCEPTION_RADIUS {
            return Err(PerceptionError::RadiusTooLarge {
                requested: radius,
                maximum: MAX_PERCEPTION_RADIUS,
            });
        }
        let view = self.view(agent).ok_or(PerceptionError::MissingAgent)?;
        if view.activity.is_terminal() {
            return Err(PerceptionError::DeadAgent);
        }
        let active = self.active_area.expect("initialized population");
        let radius = i64::from(radius);
        let requested = WorldRect {
            min: WorldPosition {
                x: view.position.x - radius,
                y: view.position.y - radius,
            },
            max: WorldPosition {
                x: view.position.x + radius + 1,
                y: view.position.y + radius + 1,
            },
        };
        let area = active
            .intersection(requested)
            .ok_or(PerceptionError::OutsideWorld)?;
        self.perceive_area(world, resource_deltas, structures, agent, area)
    }

    pub(crate) fn perceive_area(
        &self,
        world: &World,
        resource_deltas: &ResourceDeltas,
        structures: &StructureStore,
        agent: AgentId,
        area: WorldRect,
    ) -> Result<PhysicalPerception, PerceptionError> {
        let view = self.view(agent).ok_or(PerceptionError::MissingAgent)?;
        if view.activity.is_terminal() {
            return Err(PerceptionError::DeadAgent);
        }
        if area.max.x <= area.min.x || area.max.y <= area.min.y {
            return Err(PerceptionError::EmptyArea);
        }
        if !self
            .active_area
            .expect("initialized population")
            .contains_rect(area)
        {
            return Err(PerceptionError::AreaOutsideActive);
        }
        let width = u64::try_from(area.max.x - area.min.x)
            .map_err(|_| PerceptionError::AreaOutsideActive)?;
        let height = u64::try_from(area.max.y - area.min.y)
            .map_err(|_| PerceptionError::AreaOutsideActive)?;
        let requested = width
            .checked_mul(height)
            .ok_or(PerceptionError::AreaTooLarge {
                requested: u64::MAX,
                maximum: MAX_PERCEPTION_CELLS,
            })?;
        if requested > u64::from(MAX_PERCEPTION_CELLS) {
            return Err(PerceptionError::AreaTooLarge {
                requested,
                maximum: MAX_PERCEPTION_CELLS,
            });
        }
        let cell_count = requested as usize;
        let mut agent_ids = Vec::new();
        agent_ids
            .try_reserve(self.len().min(cell_count))
            .map_err(|_| PerceptionError::AllocationFailed)?;
        self.spatial.agents_in(area, &mut agent_ids);
        let mut agents = Vec::new();
        agents
            .try_reserve(agent_ids.len())
            .map_err(|_| PerceptionError::AllocationFailed)?;
        agents.extend(agent_ids.into_iter().filter_map(|id| self.view(id)));
        let mut drinkable_water = Vec::new();
        let mut resources = Vec::new();
        let mut perceived_structures = Vec::new();
        perceived_structures
            .try_reserve(structures.len().min(cell_count))
            .map_err(|_| PerceptionError::AllocationFailed)?;
        structures.push_views_in(area, &mut perceived_structures);
        let mut traversable_cells = Vec::new();
        traversable_cells
            .try_reserve(cell_count)
            .map_err(|_| PerceptionError::AllocationFailed)?;
        for y in area.min.y..area.max.y {
            for x in area.min.x..area.max.x {
                let position = WorldPosition { x, y };
                match world
                    .standability_at(position)
                    .map_err(map_perception_query_error)?
                {
                    Standability::Standable if structures.structure_at(position).is_none() => {
                        traversable_cells.push(position)
                    }
                    Standability::Standable => {}
                    Standability::BlockedByWater | Standability::BlockedByFeature => {}
                }
                if let Some(source) = world
                    .water_at(position)
                    .map_err(map_perception_query_error)?
                    && source.is_drinkable()
                {
                    try_push(&mut drinkable_water, PerceivedWater { position, source })?;
                }
                if let Some(resource) = resource_deltas
                    .resource_at(world, position)
                    .map_err(map_perception_query_error)?
                {
                    try_push(&mut resources, PerceivedResource { position, resource })?;
                }
            }
        }
        Ok(PhysicalPerception {
            area,
            agents,
            drinkable_water,
            resources,
            structures: perceived_structures,
            traversable_cells,
        })
    }

    pub(crate) fn len(&self) -> usize {
        self.records.len()
    }

    pub(crate) fn is_initialized(&self) -> bool {
        self.initialized
    }

    pub(crate) fn event_is_current(&self, event: &ScheduledEvent) -> bool {
        let index = event.agent.0 as usize;
        self.records
            .get(index)
            .is_some_and(|record| match event.class {
                EventClass::Movement => {
                    record.activity == AgentActivity::Moving
                        && self.movement_generations[index] == event.generation
                }
                EventClass::NeedThreshold => {
                    !record.activity.is_terminal()
                        && self.needs[index].event_is_current(event.generation, event.need)
                }
                EventClass::HealthConsequence => {
                    record.activity != AgentActivity::Dead
                        && self.health[index].event_is_current(event.generation)
                }
                EventClass::Decision | EventClass::Wake | EventClass::ActionCompletion => {
                    !record.activity.is_terminal()
                        && self.policies[index].event_is_current(event.generation)
                }
            })
    }

    #[cfg(test)]
    pub(crate) fn capacities(&self) -> (usize, usize, usize, usize, usize, usize) {
        (
            self.records.capacity(),
            self.movement_generations.capacity(),
            self.routes.capacity(),
            self.needs.capacity(),
            self.spatial.retained_entry_capacity(),
            self.spatial.bucket_count(),
        )
    }

    #[cfg(test)]
    pub(crate) fn inventory_capacity(&self) -> usize {
        self.inventories.capacity()
    }

    #[cfg(test)]
    pub(crate) fn sleep_capacity(&self) -> usize {
        self.sleeps.capacity()
    }

    #[cfg(test)]
    pub(crate) fn set_need_value_for_test(
        &mut self,
        agent: AgentId,
        kind: NeedKind,
        value: u16,
        now: SimTime,
    ) {
        self.needs[agent.0 as usize].set_value_for_test(kind, value, now);
    }

    #[cfg(test)]
    pub(crate) fn prepare_health_consequence_for_test(
        &mut self,
        scheduler: &mut Scheduler,
        agent: AgentId,
        value: u16,
        now: SimTime,
    ) {
        let index = agent.0 as usize;
        self.health[index].set_value_for_test(value);
        let needs = self.needs[index];
        self.reschedule_health(scheduler, agent, needs, now)
            .unwrap();
    }

    #[cfg(test)]
    pub(crate) fn mark_dead(&mut self, agent: AgentId) {
        self.records[agent.0 as usize].activity = AgentActivity::Dead;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ActionEffectError {
    NoEdibleInventory,
    EventSequenceExhausted,
}

fn invalid_spawn_reason(
    world: &World,
    position: WorldPosition,
) -> Result<Option<SpawnInvalidReason>, PopulationInitError> {
    match world
        .standability_at(position)
        .map_err(|_| PopulationInitError::IncompleteResidency)?
    {
        Standability::Standable => Ok(None),
        Standability::BlockedByWater => Ok(Some(SpawnInvalidReason::Water)),
        Standability::BlockedByFeature => Ok(Some(SpawnInvalidReason::BlockingFeature)),
    }
}

fn map_query_error(error: WorldQueryError) -> MoveRequestError {
    match error {
        WorldQueryError::OutsideWorldBounds => MoveRequestError::OutsideWorld,
        WorldQueryError::Unloaded => MoveRequestError::Unloaded,
        WorldQueryError::NonCardinalStep => MoveRequestError::InvalidStep,
    }
}

fn map_event_query_error(error: WorldQueryError) -> MovementOutcomeKind {
    match error {
        WorldQueryError::OutsideWorldBounds => MovementOutcomeKind::OutsideWorld,
        WorldQueryError::Unloaded => MovementOutcomeKind::Unloaded,
        WorldQueryError::NonCardinalStep => MovementOutcomeKind::InvalidStep,
    }
}

fn map_perception_query_error(error: WorldQueryError) -> PerceptionError {
    match error {
        WorldQueryError::OutsideWorldBounds => PerceptionError::OutsideWorld,
        WorldQueryError::Unloaded => PerceptionError::Unloaded,
        WorldQueryError::NonCardinalStep => unreachable!("point queries are not movement steps"),
    }
}

fn try_push<T>(values: &mut Vec<T>, value: T) -> Result<(), PerceptionError> {
    if values.len() == values.capacity() {
        values
            .try_reserve(1)
            .map_err(|_| PerceptionError::AllocationFailed)?;
    }
    values.push(value);
    Ok(())
}

fn movement_outcome(
    event: ScheduledEvent,
    from: Option<WorldPosition>,
    target: WorldPosition,
    kind: MovementOutcomeKind,
) -> MovementEventOutcome {
    MovementEventOutcome {
        event: EventId(event.sequence),
        agent: event.agent,
        due: event.due,
        from,
        target,
        kind,
    }
}

fn need_outcome(
    event: ScheduledEvent,
    value: Option<u16>,
    outcome: NeedThresholdOutcomeKind,
) -> NeedThresholdEventOutcome {
    NeedThresholdEventOutcome {
        event: EventId(event.sequence),
        agent: event.agent,
        due: event.due,
        kind: event.need,
        value,
        outcome,
    }
}

#[cfg(test)]
mod tests {
    use std::mem::{align_of, size_of};

    use super::*;

    #[test]
    fn foundational_agent_records_remain_compact() {
        assert_eq!(size_of::<AgentId>(), 4);
        assert_eq!(align_of::<AgentId>(), 4);
        assert_eq!(size_of::<CompactPosition>(), 4);
        assert_eq!(size_of::<AgentActivity>(), 1);
        assert_eq!(size_of::<AgentRecord>(), 6);
        assert_eq!(size_of::<RouteState>(), 6);
        assert_eq!(size_of::<Option<RouteState>>(), 8);
        assert_eq!(size_of::<ScheduledEvent>(), 32);
    }
}
