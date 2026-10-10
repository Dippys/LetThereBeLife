//! The fixed-tick event loop: drains due scheduler events and dispatches them
//! to needs, health, policy, action, and movement handlers.

use super::errors::map_movement_route_failure;
use crate::agent::MovementEnvironment;
use crate::scheduler::{EventClass, MAX_DUE_EVENTS_PER_TICK};
use crate::sleep::interruption_for_need;
use crate::{
    Engine, HealthDiagnosticKind, MovementOutcomeKind, NeedThresholdOutcomeKind, RouteOutcomeKind,
    SleepDiagnostic, SleepDiagnosticKind, TickOutcome,
};

impl Engine {
    /// Advances exactly one deterministic simulation tick.
    pub fn tick(&mut self) -> TickOutcome {
        if self.paused {
            return TickOutcome::Paused;
        }
        let Some(next_time) = self.time.checked_add(1) else {
            return TickOutcome::TimeExhausted;
        };
        self.time = next_time;
        self.movement_outcomes.clear();
        self.route_outcomes.clear();
        self.need_outcomes.clear();
        self.policy_diagnostics.clear();
        self.sleep_diagnostics.clear();
        self.structure_diagnostics.clear();
        self.health_diagnostics.clear();
        self.signal_events.clear();
        self.interpretation_events.clear();
        self.hint_outcomes.clear();
        self.meal_events.clear();
        self.lead_events.clear();
        self.wildlife_events.clear();
        self.lesson_events.clear();
        self.repair_events.clear();
        self.request_events.clear();
        let mut processed = 0_usize;
        while processed < MAX_DUE_EVENTS_PER_TICK {
            let Some(event) = self.scheduler.pop_due(self.time) else {
                break;
            };
            if event.class == EventClass::NeedThreshold {
                let outcome = self.population.apply_need_threshold(event);
                if outcome.outcome == NeedThresholdOutcomeKind::Reached {
                    let construction_cancelled = self.cancel_construction(outcome.agent);
                    let sleeping = self.population.sleep_view(outcome.agent);
                    let interruption = interruption_for_need(outcome.kind);
                    if construction_cancelled {
                        if self
                            .population
                            .interrupt_for_policy_decision(
                                &mut self.scheduler,
                                self.time,
                                outcome.agent,
                                self.policy_active,
                            )
                            .is_err()
                        {
                            self.population.force_settle_idle(self.time, outcome.agent);
                        }
                    } else if sleeping.is_some() && interruption.is_some() {
                        let result = self.population.interrupt_for_policy_decision(
                            &mut self.scheduler,
                            self.time,
                            outcome.agent,
                            self.policy_active,
                        );
                        let interrupted = match result {
                            Ok((_, interrupted)) => interrupted,
                            Err(_) => self
                                .population
                                .force_interrupt_sleep(self.time, outcome.agent),
                        };
                        if let (Some(sleep), Some(reason)) = (sleeping, interruption)
                            && interrupted.is_some()
                        {
                            self.sleep_diagnostics.push(SleepDiagnostic {
                                sleep,
                                at: self.time,
                                kind: SleepDiagnosticKind::Interrupted,
                                interruption: Some(reason),
                            });
                        }
                    } else if self.policy_active
                        && self
                            .population
                            .interrupt_for_policy_decision(
                                &mut self.scheduler,
                                self.time,
                                outcome.agent,
                                true,
                            )
                            .is_err()
                    {
                        self.population.force_settle_idle(self.time, outcome.agent);
                    }
                }
                self.need_outcomes.push(outcome);
                if outcome.outcome != NeedThresholdOutcomeKind::Reached {
                    self.runtime_counters.stale_events_processed += 1;
                }
                processed += 1;
                continue;
            }
            if event.class == EventClass::HealthConsequence {
                let outcome = self
                    .population
                    .apply_health_consequence(&mut self.scheduler, event);
                match outcome.kind {
                    HealthDiagnosticKind::Incapacitated => {
                        self.cancel_construction(outcome.agent);
                        self.population.incapacitate(outcome.at, outcome.agent);
                    }
                    HealthDiagnosticKind::Died => {
                        self.cancel_construction(outcome.agent);
                        if let Some(cause) = outcome.cause
                            && let Some(record) =
                                self.population
                                    .finalize_death(outcome.at, outcome.agent, cause)
                        {
                            self.death_records.push(record);
                        }
                    }
                    HealthDiagnosticKind::Deteriorated | HealthDiagnosticKind::StaleEvent => {}
                }
                self.health_diagnostics.push(outcome);
                if outcome.kind == HealthDiagnosticKind::StaleEvent {
                    self.runtime_counters.stale_events_processed += 1;
                }
                processed += 1;
                continue;
            }
            if event.class == EventClass::Decision {
                self.apply_policy_decision(event);
                processed += 1;
                continue;
            }
            if matches!(event.class, EventClass::Wake | EventClass::ActionCompletion) {
                self.apply_policy_action_completion(event);
                processed += 1;
                continue;
            }
            let outcome = self.population.apply_movement(
                &mut self.scheduler,
                MovementEnvironment {
                    world: &self.world,
                    spawned_objects: &self.spawned_objects,
                    structures: &self.structures,
                },
                event,
            );
            let agent = outcome.agent;
            let kind = outcome.kind;
            self.movement_outcomes.push(outcome);
            match kind {
                MovementOutcomeKind::Moved | MovementOutcomeKind::Occupied(_) => {
                    self.continue_route(agent);
                }
                MovementOutcomeKind::StaleEvent
                | MovementOutcomeKind::MissingAgent
                | MovementOutcomeKind::DeadAgent => {
                    self.runtime_counters.stale_events_processed += 1;
                }
                MovementOutcomeKind::InconsistentOccupancy => {
                    self.finish_route(agent, RouteOutcomeKind::InconsistentOccupancy);
                }
                MovementOutcomeKind::InvalidStep
                | MovementOutcomeKind::Unloaded
                | MovementOutcomeKind::OutsideWorld
                | MovementOutcomeKind::OutsideActiveArea
                | MovementOutcomeKind::Blocked(_)
                | MovementOutcomeKind::BlockedByStructure(_) => {
                    if self.population.route_request(agent).is_some() {
                        self.finish_route(agent, map_movement_route_failure(kind));
                    }
                }
                MovementOutcomeKind::EventSequenceExhausted => {
                    if self.population.route_request(agent).is_some() {
                        self.finish_route(agent, RouteOutcomeKind::EventSequenceExhausted);
                    }
                }
            }
            processed += 1;
        }
        let processed = processed as u16;
        self.runtime_counters.events_processed += u64::from(processed);
        self.runtime_counters.peak_events_processed_per_tick = self
            .runtime_counters
            .peak_events_processed_per_tick
            .max(processed);
        let due_backlog = self.scheduler.has_due(self.time);
        self.runtime_counters.due_backlog_ticks += u64::from(due_backlog);
        self.step_wildlife();
        // Autonomous agents re-schedule constantly; without this, stale events
        // were only pruned on manual commands and piled up during long runs.
        self.compact_scheduler_if_needed();
        TickOutcome::Advanced {
            time: self.time,
            processed_events: processed,
            due_backlog,
        }
    }
}
