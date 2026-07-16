use std::{collections::BTreeSet, error::Error, fmt};

use crate::{
    Standability, TraversalKind, WORLD_GENERATION_BOUNDS, World, WorldPosition, WorldQueryError,
    WorldRect,
    scheduler::{ScheduleError, ScheduledEvent, Scheduler},
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
    Idle,
    Moving,
    Dead,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgentView {
    pub id: AgentId,
    pub position: WorldPosition,
    pub activity: AgentActivity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(C)]
pub(crate) struct CompactPosition {
    pub(crate) x: i16,
    pub(crate) y: i16,
}

impl CompactPosition {
    fn checked(position: WorldPosition) -> Option<Self> {
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
pub enum MoveRequestError {
    MissingAgent,
    DeadAgent,
    InvalidStep,
    Unloaded,
    OutsideWorld,
    OutsideActiveArea,
    Blocked(TraversalKind),
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

#[derive(Debug, Default)]
pub(crate) struct Population {
    records: Vec<AgentRecord>,
    movement_generations: Vec<u32>,
    active_area: Option<WorldRect>,
    initialized: bool,
}

impl Population {
    pub(crate) fn initialize(
        &mut self,
        world: &World,
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

        self.records = records;
        self.movement_generations = movement_generations;
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
        world: &World,
        now: SimTime,
        agent: AgentId,
        target: WorldPosition,
    ) -> Result<MovementScheduled, MoveRequestError> {
        let index = agent.0 as usize;
        let record = self
            .records
            .get(index)
            .ok_or(MoveRequestError::MissingAgent)?;
        if record.activity == AgentActivity::Dead {
            return Err(MoveRequestError::DeadAgent);
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
        let step = world
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
        self.records[index].activity = AgentActivity::Moving;
        Ok(MovementScheduled {
            event: EventId(sequence),
            completes_at: due,
        })
    }

    pub(crate) fn apply_movement(
        &mut self,
        world: &World,
        event: ScheduledEvent,
    ) -> MovementEventOutcome {
        let target = event.target.world();
        let index = event.agent.0 as usize;
        let Some(record) = self.records.get_mut(index) else {
            return movement_outcome(event, None, target, MovementOutcomeKind::MissingAgent);
        };
        let from = record.position.world();
        if record.activity == AgentActivity::Dead {
            return movement_outcome(event, Some(from), target, MovementOutcomeKind::DeadAgent);
        }
        if self.movement_generations[index] != event.generation
            || record.activity != AgentActivity::Moving
        {
            return movement_outcome(event, Some(from), target, MovementOutcomeKind::StaleEvent);
        }
        if !WORLD_GENERATION_BOUNDS.contains(target) {
            record.activity = AgentActivity::Idle;
            return movement_outcome(event, Some(from), target, MovementOutcomeKind::OutsideWorld);
        }
        if !self
            .active_area
            .is_some_and(|active_area| active_area.contains(target))
        {
            record.activity = AgentActivity::Idle;
            return movement_outcome(
                event,
                Some(from),
                target,
                MovementOutcomeKind::OutsideActiveArea,
            );
        }
        let kind = match world.traversal_step(from, target) {
            Ok(step) if step.is_passable() => {
                record.position = event.target;
                record.activity = AgentActivity::Idle;
                MovementOutcomeKind::Moved
            }
            Ok(step) => {
                record.activity = AgentActivity::Idle;
                MovementOutcomeKind::Blocked(step.kind())
            }
            Err(error) => {
                record.activity = AgentActivity::Idle;
                map_event_query_error(error)
            }
        };
        movement_outcome(event, Some(from), target, kind)
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

    pub(crate) fn len(&self) -> usize {
        self.records.len()
    }

    pub(crate) fn is_initialized(&self) -> bool {
        self.initialized
    }

    pub(crate) fn event_is_current(&self, event: &ScheduledEvent) -> bool {
        let index = event.agent.0 as usize;
        self.records.get(index).is_some_and(|record| {
            record.activity == AgentActivity::Moving
                && self.movement_generations[index] == event.generation
        })
    }

    #[cfg(test)]
    pub(crate) fn capacities(&self) -> (usize, usize) {
        (
            self.records.capacity(),
            self.movement_generations.capacity(),
        )
    }

    #[cfg(test)]
    pub(crate) fn mark_dead(&mut self, agent: AgentId) {
        self.records[agent.0 as usize].activity = AgentActivity::Dead;
    }
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
        assert_eq!(size_of::<ScheduledEvent>(), 32);
    }
}
