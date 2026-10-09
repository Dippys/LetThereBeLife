//! Public movement and route scheduling results, errors, and event outcomes.

use std::{error::Error, fmt};

use crate::{
    TraversalKind, WorldPosition,
    agent::{AgentId, EventId, SimTime},
    structures::StructureId,
};

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
