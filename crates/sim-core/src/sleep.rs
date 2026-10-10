use std::{error::Error, fmt};

use crate::{AgentId, SimTime, StructureId, WorldPosition};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum SleepQuality {
    OpenGround = 0,
    Sheltered = 1,
}

impl SleepQuality {
    pub(crate) const fn rest_recovery_per_period(self) -> u8 {
        match self {
            Self::OpenGround => 8,
            Self::Sheltered => 12,
        }
    }

    pub(crate) const fn exposure_rate_per_period(self) -> i8 {
        match self {
            Self::OpenGround => 2,
            Self::Sheltered => -4,
        }
    }
}

/// Tiredness (out of 10,000) past which an agent falls asleep wherever it is,
/// cold or not.
pub const REST_COLLAPSE: u16 = 9_200;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum SleepInterruptionReason {
    Hunger,
    Thirst,
    Exposure,
    /// Bitten in its sleep.
    Injury,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum SleepDiagnosticKind {
    Started,
    Woke,
    Interrupted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SleepView {
    pub agent: AgentId,
    pub position: WorldPosition,
    pub started_at: SimTime,
    pub planned_wake: SimTime,
    pub quality: SleepQuality,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SleepDiagnostic {
    pub sleep: SleepView,
    pub at: SimTime,
    pub kind: SleepDiagnosticKind,
    pub interruption: Option<SleepInterruptionReason>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SleepRequestError {
    MissingAgent,
    DeadAgent,
    PolicyControlled,
    AgentCommitted,
    OutsideActiveArea,
    OutsideWorld,
    Unloaded,
    Water,
    BlockingFeature,
    Occupied(AgentId),
    StructureOccupied(StructureId),
    UnsafeExposure,
    NotAtLocation,
    TimeOverflow,
    RescheduleLimit,
    EventSequenceExhausted,
}

impl fmt::Display for SleepRequestError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "sleep request failed: {self:?}")
    }
}

impl Error for SleepRequestError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub(crate) struct SleepState {
    pub(crate) started_at: SimTime,
    pub(crate) planned_wake: SimTime,
    pub(crate) quality: SleepQuality,
    active: bool,
}

impl Default for SleepState {
    fn default() -> Self {
        Self {
            started_at: SimTime::ZERO,
            planned_wake: SimTime::ZERO,
            quality: SleepQuality::OpenGround,
            active: false,
        }
    }
}

impl SleepState {
    pub(crate) const fn active(
        started_at: SimTime,
        planned_wake: SimTime,
        quality: SleepQuality,
    ) -> Self {
        Self {
            started_at,
            planned_wake,
            quality,
            active: true,
        }
    }

    pub(crate) const fn is_active(self) -> bool {
        self.active
    }
}

pub(crate) const fn interruption_for_need(
    need: crate::NeedKind,
) -> Option<SleepInterruptionReason> {
    match need {
        crate::NeedKind::Hunger => Some(SleepInterruptionReason::Hunger),
        crate::NeedKind::Thirst => Some(SleepInterruptionReason::Thirst),
        crate::NeedKind::Exposure => Some(SleepInterruptionReason::Exposure),
        crate::NeedKind::Rest => None,
    }
}

#[cfg(test)]
mod tests {
    use std::mem::{align_of, size_of};

    use super::*;

    #[test]
    fn sleep_state_is_fixed_and_pointer_free() {
        assert_eq!(size_of::<SleepState>(), 24);
        assert_eq!(align_of::<SleepState>(), 8);
        assert_eq!(size_of::<SleepQuality>(), 1);
        assert_eq!(size_of::<SleepInterruptionReason>(), 1);
    }
}
