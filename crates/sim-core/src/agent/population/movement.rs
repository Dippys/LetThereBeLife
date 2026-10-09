//! Movement and route-step scheduling, movement event application, and
//! per-agent route state.

use super::{MovementEnvironment, Population, RouteState};
use crate::{
    WORLD_GENERATION_BOUNDS, WorldPosition, WorldQueryError, WorldRect,
    agent::{
        AgentActivity, AgentId, CompactPosition, EventId, MoveRequestError, MovementEventOutcome,
        MovementOutcomeKind, MovementScheduled, SimTime,
    },
    routing::{RouteRequest, RouteRequestError},
    scheduler::{ScheduleError, ScheduledEvent, Scheduler},
    spatial::TransferError,
};

impl Population {
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
        if let Some(structure) = environment.structures.structure_at(target) {
            return Err(MoveRequestError::BlockedByStructure(structure));
        }
        let step = environment
            .spawned_objects
            .traversal_step(environment.world, record.position.world(), target)
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
            match environment
                .spawned_objects
                .traversal_step(environment.world, from, target)
            {
                Ok(step) if step.is_passable() => {
                    match self.spatial.transfer(event.agent, from, target) {
                        Ok(()) => {
                            record.position = event.target;
                            MovementOutcomeKind::Moved
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
        let route_continues = self.routes[index].is_some() && kind == MovementOutcomeKind::Moved;
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
