//! Explicit movement and route requests, route planning, step continuation,
//! and terminal route outcomes.

use super::errors::{
    map_move_route_error, map_move_route_failure_kind, map_route_failure_kind, route_failure,
};
use crate::agent::MovementEnvironment;
use crate::routing::RouteEnvironment;
use crate::{
    AgentId, Engine, MoveRequestError, MovementScheduled, PolicyDiagnostic, PolicyDiagnosticKind,
    PolicyFailureReason, PolicyReason, RouteEventOutcome, RouteOutcomeKind, RouteRequest,
    RouteRequestError, RouteScheduled, WorldPosition,
};

impl Engine {
    pub fn request_move(
        &mut self,
        agent: AgentId,
        target: WorldPosition,
    ) -> Result<MovementScheduled, MoveRequestError> {
        if self.policy_active {
            return Err(MoveRequestError::PolicyControlled);
        }
        self.compact_scheduler_if_needed();
        self.population.schedule_movement(
            &mut self.scheduler,
            MovementEnvironment {
                world: &self.world,
                spawned_objects: &self.spawned_objects,
                structures: &self.structures,
            },
            self.time,
            agent,
            target,
        )
    }

    /// Plans a bounded deterministic minimum-travel-time local route and schedules its first step.
    pub fn request_route(
        &mut self,
        agent: AgentId,
        request: RouteRequest,
    ) -> Result<RouteScheduled, RouteRequestError> {
        if self.policy_active {
            return Err(RouteRequestError::PolicyControlled);
        }
        self.schedule_route(agent, request)
    }

    pub(super) fn schedule_route(
        &mut self,
        agent: AgentId,
        request: RouteRequest,
    ) -> Result<RouteScheduled, RouteRequestError> {
        self.compact_scheduler_if_needed();
        let (origin, active_area) = self.population.route_context(agent)?;
        self.runtime_counters.route_plans += 1;
        let plan = self.route_planner.plan(
            RouteEnvironment {
                world: &self.world,
                spawned_objects: &self.spawned_objects,
                structures: &self.structures,
                active_area,
            },
            origin,
            request,
        );
        let expansions = match &plan {
            Ok(plan) => plan.expansions,
            Err(
                RouteRequestError::NoPath { expansions }
                | RouteRequestError::BudgetExhausted { expansions },
            ) => *expansions,
            Err(_) => 0,
        };
        self.runtime_counters.route_expansions += u64::from(expansions);
        let plan = plan?;
        let scheduled = self
            .population
            .schedule_route_step(
                &mut self.scheduler,
                MovementEnvironment {
                    world: &self.world,
                    spawned_objects: &self.spawned_objects,
                    structures: &self.structures,
                },
                self.time,
                agent,
                request,
                plan.next,
            )
            .map_err(map_move_route_error)?;
        Ok(RouteScheduled {
            first_event: scheduled.event,
            first_completion: scheduled.completes_at,
            destination: request.destination,
            expansions: plan.expansions,
        })
    }

    pub(super) fn continue_route(&mut self, agent: AgentId) {
        self.compact_scheduler_if_needed();
        let Some(request) = self.population.route_request(agent) else {
            return;
        };
        let Ok((origin, active_area)) = self.population.route_context(agent) else {
            self.finish_route(agent, RouteOutcomeKind::InconsistentOccupancy);
            return;
        };
        if origin == request.destination {
            self.finish_route(agent, RouteOutcomeKind::Arrived);
            return;
        }
        match self.route_planner.plan(
            RouteEnvironment {
                world: &self.world,
                spawned_objects: &self.spawned_objects,
                structures: &self.structures,
                active_area,
            },
            origin,
            request,
        ) {
            Ok(plan) => {
                if let Err(error) = self.population.schedule_route_step(
                    &mut self.scheduler,
                    MovementEnvironment {
                        world: &self.world,
                        spawned_objects: &self.spawned_objects,
                        structures: &self.structures,
                    },
                    self.time,
                    agent,
                    request,
                    plan.next,
                ) {
                    self.finish_route(agent, map_move_route_failure_kind(error));
                }
            }
            Err(error) => self.finish_route(agent, map_route_failure_kind(error)),
        }
    }

    pub(super) fn finish_route(&mut self, agent: AgentId, kind: RouteOutcomeKind) {
        let Some(request) = self.population.route_request(agent) else {
            return;
        };
        let kind =
            match self
                .population
                .finish_route_activity(&mut self.scheduler, self.time, agent)
            {
                Ok(()) => kind,
                Err(MoveRequestError::RescheduleLimit) => RouteOutcomeKind::RescheduleLimit,
                Err(MoveRequestError::EventSequenceExhausted) => {
                    RouteOutcomeKind::EventSequenceExhausted
                }
                Err(_) => RouteOutcomeKind::InconsistentOccupancy,
            };
        self.population.clear_route(agent);
        self.route_outcomes.push(RouteEventOutcome {
            agent,
            at: self.time,
            destination: request.destination,
            kind,
        });
        if self.policy_active
            && let Some((goal, target, reason)) = self.population.policy_commitment(agent)
        {
            if kind == RouteOutcomeKind::Arrived {
                if self
                    .population
                    .schedule_policy_decision(
                        &mut self.scheduler,
                        self.time,
                        agent,
                        1,
                        PolicyReason::RouteArrived,
                        false,
                    )
                    .is_err()
                {
                    self.policy_diagnostics.push(PolicyDiagnostic {
                        agent,
                        at: self.time,
                        goal,
                        target: Some(target),
                        reason,
                        kind: PolicyDiagnosticKind::RetryScheduled,
                        failure: Some(PolicyFailureReason::EventSequenceExhausted),
                    });
                }
            } else {
                self.schedule_policy_retry(agent, goal, Some(target), reason, route_failure(kind));
            }
        }
    }
}
