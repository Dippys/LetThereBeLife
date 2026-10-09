//! Autonomous physical policy: activation, decision events, applying a
//! selected goal, and bounded retry scheduling.

use super::errors::{
    build_failure, move_failure, perception_failure, request_failure, sleep_failure,
};
use crate::agent::MovementEnvironment;
use crate::policy::{PolicyAction, PolicySelection, retry_delay, select_with_exploration};
use crate::scheduler::{self};
use crate::{
    AgentId, Engine, ExplorationHeading, PHYSICAL_POLICY_ACTION_TICKS,
    PHYSICAL_POLICY_IDLE_RECHECK_TICKS, PHYSICAL_POLICY_RADIUS, PHYSICAL_POLICY_ROUTE_BUDGET,
    PhysicalGoal, PolicyActivationError, PolicyDiagnostic, PolicyDiagnosticKind,
    PolicyFailureReason, PolicyOptions, PolicyReason, RouteRequest, SleepDiagnostic,
    SleepDiagnosticKind, WorldPosition,
};

impl Engine {
    /// Activates autonomous physical decisions after population initialization.
    pub fn activate_physical_policy(&mut self) -> Result<(), PolicyActivationError> {
        self.activate_physical_policy_with_options(PolicyOptions::default())
    }

    /// Activates autonomous physical decisions with bounded deterministic wandering
    /// whenever the local perception window contains no actionable objective.
    pub fn activate_physical_policy_with_exploration(
        &mut self,
    ) -> Result<(), PolicyActivationError> {
        self.activate_physical_policy_with_options(PolicyOptions {
            exploration: true,
            ..PolicyOptions::default()
        })
    }

    /// Activates autonomous physical decisions with explicit cognitive features.
    /// `memory` implies exploration; `sharing` requires `memory` and is ignored without it.
    pub fn activate_physical_policy_with_options(
        &mut self,
        options: PolicyOptions,
    ) -> Result<(), PolicyActivationError> {
        if !self.population.is_initialized() {
            return Err(PolicyActivationError::PopulationNotInitialized);
        }
        if self.policy_active {
            return Err(PolicyActivationError::AlreadyActive);
        }
        if let Some(agent) = self.population.first_non_idle_agent() {
            return Err(PolicyActivationError::AgentCommitted { agent });
        }
        let due = self
            .time
            .checked_add(1)
            .ok_or(PolicyActivationError::TimeOverflow)?;
        if !self.scheduler.can_schedule(self.population.len() as u64) {
            return Err(PolicyActivationError::EventSequenceExhausted);
        }
        self.population
            .activate_policy(&mut self.scheduler, due)
            .map_err(|_| PolicyActivationError::EventSequenceExhausted)?;
        self.policy_active = true;
        self.policy_options = PolicyOptions {
            exploration: options.exploration || options.memory,
            memory: options.memory,
            sharing: options.memory && options.sharing,
        };
        Ok(())
    }

    pub(super) fn apply_policy_decision(&mut self, event: scheduler::ScheduledEvent) {
        if !self.population.policy_event_is_current(event) {
            self.runtime_counters.stale_events_processed += 1;
            self.policy_diagnostics.push(PolicyDiagnostic {
                agent: event.agent,
                at: self.time,
                goal: event.goal,
                target: None,
                reason: PolicyReason::Retry,
                kind: PolicyDiagnosticKind::StaleEvent,
                failure: None,
            });
            return;
        }
        let Some((view, needs, inventory)) = self.population.policy_context(event.agent, self.time)
        else {
            return;
        };
        let perception = match self.perceive_physical(event.agent, PHYSICAL_POLICY_RADIUS) {
            Ok(perception) => perception,
            Err(error) => {
                self.schedule_policy_retry(
                    event.agent,
                    event.goal,
                    None,
                    PolicyReason::Retry,
                    perception_failure(error),
                );
                return;
            }
        };
        self.runtime_counters.policy_perception_queries += 1;
        let width = (perception.area.max.x - perception.area.min.x) as u64;
        let height = (perception.area.max.y - perception.area.min.y) as u64;
        self.runtime_counters.policy_perceived_cells += width * height;
        let (selection, selected_heading) = if self.policy_options.memory {
            self.deliberate_with_memory(event.agent, view.position, needs, inventory, &perception)
        } else {
            let exploration_heading = self
                .policy_options
                .exploration
                .then(|| self.population.exploration_heading(event.agent))
                .flatten();
            select_with_exploration(
                view.position,
                needs,
                inventory,
                &perception,
                exploration_heading,
            )
        };
        self.policy_diagnostics.push(PolicyDiagnostic {
            agent: event.agent,
            at: self.time,
            goal: selection.goal,
            target: selection.target,
            reason: selection.reason,
            kind: PolicyDiagnosticKind::Selected,
            failure: None,
        });
        self.apply_policy_selection(event.agent, view.position, selection, selected_heading);
    }

    fn apply_policy_selection(
        &mut self,
        agent: AgentId,
        origin: WorldPosition,
        selection: PolicySelection,
        exploration_heading: Option<ExplorationHeading>,
    ) {
        let Some(target) = selection.target else {
            self.schedule_policy_retry(
                agent,
                selection.goal,
                None,
                selection.reason,
                if selection.goal == PhysicalGoal::SeekShelter {
                    PolicyFailureReason::DeferredToLaterSlice
                } else {
                    PolicyFailureReason::NoPerceivedTarget
                },
            );
            return;
        };
        if selection.goal == PhysicalGoal::Signal {
            self.start_signal(agent, target, selection.reason);
            return;
        }
        if selection.goal == PhysicalGoal::BuildShelter {
            match self.start_shelter_build(agent, target, selection.reason) {
                Ok(structure) => self.policy_diagnostics.push(PolicyDiagnostic {
                    agent,
                    at: self.time,
                    goal: selection.goal,
                    target: Some(structure.position),
                    reason: selection.reason,
                    kind: PolicyDiagnosticKind::ActionStarted,
                    failure: None,
                }),
                Err(error) => self.schedule_policy_retry(
                    agent,
                    selection.goal,
                    Some(target),
                    selection.reason,
                    build_failure(error),
                ),
            }
            return;
        }
        if selection.goal == PhysicalGoal::Wait {
            self.population.clear_route(agent);
            if let Err(error) = self.population.schedule_policy_decision(
                &mut self.scheduler,
                self.time,
                agent,
                PHYSICAL_POLICY_IDLE_RECHECK_TICKS,
                PolicyReason::NoUrgentNeed,
                false,
            ) {
                self.schedule_policy_retry(
                    agent,
                    selection.goal,
                    Some(target),
                    selection.reason,
                    move_failure(error),
                );
            }
            return;
        }
        if target == origin {
            self.population.clear_route(agent);
            let action_goal = match selection.goal {
                PhysicalGoal::SeekWater => PhysicalGoal::Drink,
                PhysicalGoal::SeekFood => PhysicalGoal::GatherMaterial,
                goal => goal,
            };
            if action_goal == PhysicalGoal::Sleep {
                let quality = match self.population.validate_sleep_location(
                    MovementEnvironment {
                        world: &self.world,
                        spawned_objects: &self.spawned_objects,
                        structures: &self.structures,
                    },
                    self.time,
                    agent,
                    target,
                    self.structures.is_sheltered_access(target),
                    self.structures.structure_at(target),
                ) {
                    Ok(quality) => quality,
                    Err(error) => {
                        self.schedule_policy_retry(
                            agent,
                            action_goal,
                            Some(target),
                            selection.reason,
                            sleep_failure(error),
                        );
                        return;
                    }
                };
                match self.population.schedule_sleep(
                    &mut self.scheduler,
                    self.time,
                    agent,
                    target,
                    quality,
                    selection.reason,
                ) {
                    Ok(sleep) => {
                        self.policy_diagnostics.push(PolicyDiagnostic {
                            agent,
                            at: self.time,
                            goal: action_goal,
                            target: Some(target),
                            reason: selection.reason,
                            kind: PolicyDiagnosticKind::ActionStarted,
                            failure: None,
                        });
                        self.sleep_diagnostics.push(SleepDiagnostic {
                            sleep,
                            at: self.time,
                            kind: SleepDiagnosticKind::Started,
                            interruption: None,
                        });
                    }
                    Err(error) => self.schedule_policy_retry(
                        agent,
                        action_goal,
                        Some(target),
                        selection.reason,
                        sleep_failure(error),
                    ),
                }
                return;
            }
            match self.population.schedule_policy_action(
                &mut self.scheduler,
                self.time,
                agent,
                PolicyAction {
                    goal: action_goal,
                    target,
                    reason: selection.reason,
                    duration: PHYSICAL_POLICY_ACTION_TICKS,
                },
            ) {
                Ok(_) => self.policy_diagnostics.push(PolicyDiagnostic {
                    agent,
                    at: self.time,
                    goal: action_goal,
                    target: Some(target),
                    reason: selection.reason,
                    kind: PolicyDiagnosticKind::ActionStarted,
                    failure: None,
                }),
                Err(error) => self.schedule_policy_retry(
                    agent,
                    action_goal,
                    Some(target),
                    selection.reason,
                    move_failure(error),
                ),
            }
            return;
        }
        match self.schedule_route(
            agent,
            RouteRequest {
                destination: target,
                max_expansions: PHYSICAL_POLICY_ROUTE_BUDGET,
            },
        ) {
            Ok(_) => {
                self.population.commit_policy_route(
                    agent,
                    selection.goal,
                    target,
                    selection.reason,
                    exploration_heading,
                );
                self.policy_diagnostics.push(PolicyDiagnostic {
                    agent,
                    at: self.time,
                    goal: selection.goal,
                    target: Some(target),
                    reason: selection.reason,
                    kind: PolicyDiagnosticKind::RouteScheduled,
                    failure: None,
                });
            }
            Err(error) => self.schedule_policy_retry(
                agent,
                selection.goal,
                Some(target),
                selection.reason,
                request_failure(error),
            ),
        }
    }

    pub(super) fn schedule_policy_retry(
        &mut self,
        agent: AgentId,
        goal: PhysicalGoal,
        target: Option<WorldPosition>,
        reason: PolicyReason,
        failure: PolicyFailureReason,
    ) {
        self.population.clear_route(agent);
        self.population.record_policy_retry(agent, goal, target);
        let retry_depth = self.population.policy_retries(agent);
        let delay = retry_delay(retry_depth);
        self.runtime_counters.policy_retries += 1;
        self.runtime_counters.peak_policy_retry_depth = self
            .runtime_counters
            .peak_policy_retry_depth
            .max(retry_depth.saturating_add(1));
        let scheduling_failure = self
            .population
            .schedule_policy_decision(
                &mut self.scheduler,
                self.time,
                agent,
                delay,
                PolicyReason::Retry,
                true,
            )
            .err()
            .map(move_failure);
        self.policy_diagnostics.push(PolicyDiagnostic {
            agent,
            at: self.time,
            goal,
            target,
            reason,
            kind: PolicyDiagnosticKind::RetryScheduled,
            failure: scheduling_failure.or(Some(failure)),
        });
    }
}
