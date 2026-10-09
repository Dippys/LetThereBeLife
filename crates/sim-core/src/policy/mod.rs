//! Physical survival policy: public goal, reason, diagnostic, and view types,
//! plus the compact per-agent state (`state`), goal selection (`selection`),
//! and bounded exploration (`exploration`).

mod deliberate;
mod exploration;
mod selection;
mod state;

pub use deliberate::{
    CURIOSITY_TARGET, EXCURSION_EVERY, FOOD_RESERVE, HOME_RANGE, PREPARE_EXPOSURE, TOP_UP_HUNGER,
    TOP_UP_THIRST,
};
pub(crate) use deliberate::{MindInput, deliberate};
#[cfg(test)]
pub(crate) use selection::select;
pub(crate) use selection::{PolicyAction, PolicySelection, select_with_exploration};
pub(crate) use state::{PolicyPhase, PolicyState};

use std::{error::Error, fmt};

use crate::{AgentId, SimTime, WorldPosition};

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
    /// Point out a remembered place to agents nearby.
    Signal = 11,
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
    /// Heading for a place the agent saw earlier.
    RememberedPlace,
    /// Heading for a place someone pointed out.
    ToldPlace,
    /// Drinking, eating, or stocking up before needs become urgent.
    PrepareTrip,
    /// Pointing out a place to someone nearby.
    Sharing,
    /// Turning back toward known water before straying out of range.
    Returning,
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

    pub(crate) const fn rotated(self, offset: i8) -> Self {
        Self::from_rank((self as i8).wrapping_add(offset) as u8)
    }

    pub(crate) const fn delta(self) -> (i64, i64) {
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
mod tests;
