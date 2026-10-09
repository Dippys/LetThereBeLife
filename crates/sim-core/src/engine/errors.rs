//! Typed mapping between movement, route, perception, sleep, action, and
//! build errors and the policy/route failure categories they report.

use crate::agent::ActionEffectError;
use crate::{
    BuildShelterError, MoveRequestError, MovementOutcomeKind, PerceptionError, PolicyFailureReason,
    RouteOutcomeKind, RouteRequestError, SleepRequestError,
};

pub(super) fn map_move_route_error(error: MoveRequestError) -> RouteRequestError {
    match error {
        MoveRequestError::PolicyControlled => RouteRequestError::PolicyControlled,
        MoveRequestError::MissingAgent => RouteRequestError::MissingAgent,
        MoveRequestError::DeadAgent => RouteRequestError::DeadAgent,
        MoveRequestError::InvalidStep => RouteRequestError::NoPath { expansions: 0 },
        MoveRequestError::Unloaded => RouteRequestError::Unloaded,
        MoveRequestError::OutsideWorld => RouteRequestError::OutsideWorld,
        MoveRequestError::OutsideActiveArea => RouteRequestError::OutsideActiveArea,
        MoveRequestError::Blocked(kind) => RouteRequestError::Blocked(kind),
        MoveRequestError::Occupied(agent) => RouteRequestError::Occupied(agent),
        MoveRequestError::BlockedByStructure(structure) => {
            RouteRequestError::BlockedByStructure(structure)
        }
        MoveRequestError::TimeOverflow => RouteRequestError::TimeOverflow,
        MoveRequestError::RescheduleLimit => RouteRequestError::RescheduleLimit,
        MoveRequestError::EventSequenceExhausted => RouteRequestError::EventSequenceExhausted,
    }
}

pub(super) fn map_move_route_failure_kind(error: MoveRequestError) -> RouteOutcomeKind {
    match map_move_route_error(error) {
        RouteRequestError::Occupied(agent) => RouteOutcomeKind::Occupied(agent),
        error => map_route_failure_kind(error),
    }
}

pub(super) fn map_route_failure_kind(error: RouteRequestError) -> RouteOutcomeKind {
    match error {
        RouteRequestError::NoPath { expansions } => RouteOutcomeKind::NoPath { expansions },
        RouteRequestError::BudgetExhausted { expansions } => {
            RouteOutcomeKind::BudgetExhausted { expansions }
        }
        RouteRequestError::Occupied(agent) => RouteOutcomeKind::Occupied(agent),
        RouteRequestError::Unloaded => RouteOutcomeKind::Unloaded,
        RouteRequestError::OutsideWorld => RouteOutcomeKind::OutsideWorld,
        RouteRequestError::OutsideActiveArea => RouteOutcomeKind::OutsideActiveArea,
        RouteRequestError::Blocked(kind) => RouteOutcomeKind::Blocked(kind),
        RouteRequestError::BlockedByStructure(structure) => {
            RouteOutcomeKind::BlockedByStructure(structure)
        }
        RouteRequestError::TimeOverflow => RouteOutcomeKind::TimeOverflow,
        RouteRequestError::RescheduleLimit => RouteOutcomeKind::RescheduleLimit,
        RouteRequestError::EventSequenceExhausted => RouteOutcomeKind::EventSequenceExhausted,
        RouteRequestError::PolicyControlled
        | RouteRequestError::MissingAgent
        | RouteRequestError::DeadAgent
        | RouteRequestError::AlreadyAtDestination
        | RouteRequestError::ZeroBudget
        | RouteRequestError::BudgetTooLarge { .. } => RouteOutcomeKind::InconsistentOccupancy,
    }
}

pub(super) fn map_movement_route_failure(kind: MovementOutcomeKind) -> RouteOutcomeKind {
    match kind {
        MovementOutcomeKind::Unloaded => RouteOutcomeKind::Unloaded,
        MovementOutcomeKind::OutsideWorld => RouteOutcomeKind::OutsideWorld,
        MovementOutcomeKind::OutsideActiveArea => RouteOutcomeKind::OutsideActiveArea,
        MovementOutcomeKind::Blocked(kind) => RouteOutcomeKind::Blocked(kind),
        MovementOutcomeKind::BlockedByStructure(structure) => {
            RouteOutcomeKind::BlockedByStructure(structure)
        }
        MovementOutcomeKind::Occupied(agent) => RouteOutcomeKind::Occupied(agent),
        MovementOutcomeKind::InconsistentOccupancy
        | MovementOutcomeKind::EventSequenceExhausted
        | MovementOutcomeKind::MissingAgent
        | MovementOutcomeKind::DeadAgent
        | MovementOutcomeKind::InvalidStep
        | MovementOutcomeKind::Moved
        | MovementOutcomeKind::StaleEvent => RouteOutcomeKind::InconsistentOccupancy,
    }
}

pub(super) fn perception_failure(error: PerceptionError) -> PolicyFailureReason {
    match error {
        PerceptionError::Unloaded => PolicyFailureReason::Unloaded,
        PerceptionError::OutsideWorld => PolicyFailureReason::OutsideWorld,
        PerceptionError::AreaOutsideActive => PolicyFailureReason::OutsideActiveArea,
        PerceptionError::MissingAgent
        | PerceptionError::DeadAgent
        | PerceptionError::RadiusTooLarge { .. }
        | PerceptionError::EmptyArea
        | PerceptionError::AreaTooLarge { .. }
        | PerceptionError::AllocationFailed => PolicyFailureReason::InconsistentState,
    }
}

pub(super) fn request_failure(error: RouteRequestError) -> PolicyFailureReason {
    match error {
        RouteRequestError::Occupied(_) => PolicyFailureReason::Occupied,
        RouteRequestError::NoPath { .. } => PolicyFailureReason::NoPath,
        RouteRequestError::BudgetExhausted { .. } => PolicyFailureReason::RouteBudgetExhausted,
        RouteRequestError::Unloaded => PolicyFailureReason::Unloaded,
        RouteRequestError::OutsideWorld => PolicyFailureReason::OutsideWorld,
        RouteRequestError::OutsideActiveArea => PolicyFailureReason::OutsideActiveArea,
        RouteRequestError::Blocked(_)
        | RouteRequestError::BlockedByStructure(_)
        | RouteRequestError::AlreadyAtDestination => PolicyFailureReason::TargetUnavailable,
        RouteRequestError::TimeOverflow => PolicyFailureReason::TimeOverflow,
        RouteRequestError::RescheduleLimit => PolicyFailureReason::RescheduleLimit,
        RouteRequestError::EventSequenceExhausted => PolicyFailureReason::EventSequenceExhausted,
        RouteRequestError::PolicyControlled
        | RouteRequestError::MissingAgent
        | RouteRequestError::DeadAgent
        | RouteRequestError::ZeroBudget
        | RouteRequestError::BudgetTooLarge { .. } => PolicyFailureReason::InconsistentState,
    }
}

pub(super) fn route_failure(kind: RouteOutcomeKind) -> PolicyFailureReason {
    match kind {
        RouteOutcomeKind::Occupied(_) => PolicyFailureReason::Occupied,
        RouteOutcomeKind::NoPath { .. } => PolicyFailureReason::NoPath,
        RouteOutcomeKind::BudgetExhausted { .. } => PolicyFailureReason::RouteBudgetExhausted,
        RouteOutcomeKind::Unloaded => PolicyFailureReason::Unloaded,
        RouteOutcomeKind::OutsideWorld => PolicyFailureReason::OutsideWorld,
        RouteOutcomeKind::OutsideActiveArea => PolicyFailureReason::OutsideActiveArea,
        RouteOutcomeKind::Blocked(_) | RouteOutcomeKind::BlockedByStructure(_) => {
            PolicyFailureReason::TargetUnavailable
        }
        RouteOutcomeKind::TimeOverflow => PolicyFailureReason::TimeOverflow,
        RouteOutcomeKind::RescheduleLimit => PolicyFailureReason::RescheduleLimit,
        RouteOutcomeKind::EventSequenceExhausted => PolicyFailureReason::EventSequenceExhausted,
        RouteOutcomeKind::InconsistentOccupancy | RouteOutcomeKind::Arrived => {
            PolicyFailureReason::InconsistentState
        }
    }
}

pub(super) fn move_failure(error: MoveRequestError) -> PolicyFailureReason {
    match error {
        MoveRequestError::Occupied(_) => PolicyFailureReason::Occupied,
        MoveRequestError::Unloaded => PolicyFailureReason::Unloaded,
        MoveRequestError::OutsideWorld => PolicyFailureReason::OutsideWorld,
        MoveRequestError::OutsideActiveArea => PolicyFailureReason::OutsideActiveArea,
        MoveRequestError::Blocked(_)
        | MoveRequestError::BlockedByStructure(_)
        | MoveRequestError::InvalidStep => PolicyFailureReason::TargetUnavailable,
        MoveRequestError::TimeOverflow => PolicyFailureReason::TimeOverflow,
        MoveRequestError::RescheduleLimit => PolicyFailureReason::RescheduleLimit,
        MoveRequestError::EventSequenceExhausted => PolicyFailureReason::EventSequenceExhausted,
        MoveRequestError::PolicyControlled
        | MoveRequestError::MissingAgent
        | MoveRequestError::DeadAgent => PolicyFailureReason::InconsistentState,
    }
}

pub(super) fn sleep_failure(error: SleepRequestError) -> PolicyFailureReason {
    match error {
        SleepRequestError::Water => PolicyFailureReason::SleepLocationWater,
        SleepRequestError::BlockingFeature => PolicyFailureReason::SleepLocationBlocked,
        SleepRequestError::Occupied(_) => PolicyFailureReason::SleepLocationOccupied,
        SleepRequestError::StructureOccupied(_) => PolicyFailureReason::SleepLocationOccupied,
        SleepRequestError::UnsafeExposure => PolicyFailureReason::SleepLocationUnsafe,
        SleepRequestError::OutsideActiveArea
        | SleepRequestError::OutsideWorld
        | SleepRequestError::Unloaded
        | SleepRequestError::NotAtLocation => PolicyFailureReason::SleepLocationUnavailable,
        SleepRequestError::TimeOverflow => PolicyFailureReason::TimeOverflow,
        SleepRequestError::RescheduleLimit => PolicyFailureReason::RescheduleLimit,
        SleepRequestError::EventSequenceExhausted => PolicyFailureReason::EventSequenceExhausted,
        SleepRequestError::MissingAgent
        | SleepRequestError::DeadAgent
        | SleepRequestError::PolicyControlled
        | SleepRequestError::AgentCommitted => PolicyFailureReason::InconsistentState,
    }
}

pub(super) fn action_effect_failure(error: ActionEffectError) -> PolicyFailureReason {
    match error {
        ActionEffectError::NoEdibleInventory => PolicyFailureReason::NoEdibleInventory,
        ActionEffectError::EventSequenceExhausted => PolicyFailureReason::EventSequenceExhausted,
    }
}

pub(super) fn map_build_schedule_error(error: MoveRequestError) -> BuildShelterError {
    match error {
        MoveRequestError::MissingAgent => BuildShelterError::MissingAgent,
        MoveRequestError::DeadAgent => BuildShelterError::DeadAgent,
        MoveRequestError::TimeOverflow => BuildShelterError::TimeOverflow,
        MoveRequestError::RescheduleLimit => BuildShelterError::RescheduleLimit,
        MoveRequestError::EventSequenceExhausted => BuildShelterError::EventSequenceExhausted,
        MoveRequestError::PolicyControlled
        | MoveRequestError::InvalidStep
        | MoveRequestError::Unloaded
        | MoveRequestError::OutsideWorld
        | MoveRequestError::OutsideActiveArea
        | MoveRequestError::Blocked(_)
        | MoveRequestError::Occupied(_)
        | MoveRequestError::BlockedByStructure(_) => BuildShelterError::AgentCommitted,
    }
}

pub(super) fn build_failure(error: BuildShelterError) -> PolicyFailureReason {
    match error {
        BuildShelterError::InsufficientMaterials => PolicyFailureReason::InsufficientMaterials,
        BuildShelterError::Occupied(_) | BuildShelterError::StructureOccupied(_) => {
            PolicyFailureReason::Occupied
        }
        BuildShelterError::Unloaded => PolicyFailureReason::Unloaded,
        BuildShelterError::OutsideWorld => PolicyFailureReason::OutsideWorld,
        BuildShelterError::OutsideActiveArea => PolicyFailureReason::OutsideActiveArea,
        BuildShelterError::TimeOverflow => PolicyFailureReason::TimeOverflow,
        BuildShelterError::RescheduleLimit => PolicyFailureReason::RescheduleLimit,
        BuildShelterError::EventSequenceExhausted => PolicyFailureReason::EventSequenceExhausted,
        BuildShelterError::Water
        | BuildShelterError::BlockingFeature
        | BuildShelterError::NotCardinallyAdjacent => PolicyFailureReason::BuildSiteInvalid,
        BuildShelterError::MissingAgent
        | BuildShelterError::DeadAgent
        | BuildShelterError::PolicyControlled
        | BuildShelterError::AgentCommitted
        | BuildShelterError::StructureLimit => PolicyFailureReason::InconsistentState,
    }
}
