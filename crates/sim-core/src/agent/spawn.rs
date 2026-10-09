//! Population initialization and spawn request types and their errors.

use std::{error::Error, fmt};

use crate::{WorldPosition, WorldRect, agent::AgentId, structures::StructureId};

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
pub enum AgentSpawnError {
    PopulationNotInitialized,
    PopulationFull,
    OutsideActiveArea,
    OutsideWorld,
    Unloaded,
    InvalidSpawn(SpawnInvalidReason),
    Occupied(AgentId),
    BlockedByStructure(StructureId),
    AllocationFailed,
    TimeOverflow,
    EventSequenceExhausted,
}

impl fmt::Display for AgentSpawnError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "agent spawn failed: {self:?}")
    }
}

impl Error for AgentSpawnError {}

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
