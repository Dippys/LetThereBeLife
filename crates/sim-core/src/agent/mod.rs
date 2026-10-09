//! Agent identity, simulation time, compact per-agent records, and the public
//! movement, spawn, and perception types. The `Population` store lives in
//! [`population`].

mod movement;
mod perception;
mod population;
mod spawn;

pub use movement::{
    MoveRequestError, MovementEventOutcome, MovementOutcomeKind, MovementScheduled,
    RouteEventOutcome, RouteOutcomeKind, RouteScheduled,
};
pub use perception::{
    MAX_PERCEPTION_CELLS, MAX_PERCEPTION_RADIUS, PerceivedResource, PerceivedWater,
    PerceptionError, PhysicalPerception,
};
#[cfg(test)]
pub(crate) use population::RouteState;
pub(crate) use population::{MovementEnvironment, Population};
pub use spawn::{
    AgentSpawnError, PopulationInit, PopulationInitError, PopulationInitOutcome, SpawnInvalidReason,
};

use std::fmt;

use crate::WorldPosition;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct EventId(u64);

impl EventId {
    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ActionEffectError {
    NoEdibleInventory,
    EventSequenceExhausted,
}

#[cfg(test)]
mod tests;
