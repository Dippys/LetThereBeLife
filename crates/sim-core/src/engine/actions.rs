//! Timed physical actions: sleep requests and action/wake completion with
//! drinking, eating, and gathering effects.

use super::errors::{action_effect_failure, move_failure};
use crate::agent::MovementEnvironment;
use crate::scheduler::{self};
use crate::{
    AgentId, DRINK_THIRST_RELIEF, Engine, GATHER_YIELD, Material, NeedKind, PhysicalGoal,
    PolicyDiagnostic, PolicyDiagnosticKind, PolicyFailureReason, PolicyReason, SHELTER_WOOD_COST,
    SleepDiagnostic, SleepDiagnosticKind, SleepRequestError, SleepView, WorldPosition,
    WorldQueryError,
};

impl Engine {
    /// Starts one explicit sleep intent at the agent's current physical location.
    pub fn request_sleep(
        &mut self,
        agent: AgentId,
        position: WorldPosition,
    ) -> Result<SleepView, SleepRequestError> {
        if self.policy_active {
            return Err(SleepRequestError::PolicyControlled);
        }
        let quality = self.population.validate_sleep_location(
            MovementEnvironment {
                world: &self.world,
                spawned_objects: &self.spawned_objects,
                structures: &self.structures,
            },
            self.time,
            agent,
            position,
            self.structures.is_sheltered_access(position),
            self.structures.structure_at(position),
        )?;
        self.compact_scheduler_if_needed();
        let sleep = self.population.schedule_sleep(
            &mut self.scheduler,
            self.time,
            agent,
            position,
            quality,
            PolicyReason::RestThreshold,
        )?;
        self.sleep_diagnostics.push(SleepDiagnostic {
            sleep,
            at: self.time,
            kind: SleepDiagnosticKind::Started,
            interruption: None,
        });
        Ok(sleep)
    }

    pub(super) fn apply_policy_action_completion(&mut self, event: scheduler::ScheduledEvent) {
        let completion = self
            .population
            .complete_policy_action(&mut self.scheduler, event);
        let Some((goal, target, reason)) = (match completion {
            Ok(completion) => completion,
            Err(error) => {
                if event.goal == PhysicalGoal::BuildShelter {
                    self.cancel_construction(event.agent);
                }
                self.policy_diagnostics.push(PolicyDiagnostic {
                    agent: event.agent,
                    at: self.time,
                    goal: event.goal,
                    target: Some(event.target.world()),
                    reason: PolicyReason::Retry,
                    kind: PolicyDiagnosticKind::ActionCompleted,
                    failure: Some(move_failure(error)),
                });
                return;
            }
        }) else {
            self.runtime_counters.stale_events_processed += 1;
            self.policy_diagnostics.push(PolicyDiagnostic {
                agent: event.agent,
                at: self.time,
                goal: event.goal,
                target: Some(event.target.world()),
                reason: PolicyReason::Retry,
                kind: PolicyDiagnosticKind::StaleEvent,
                failure: None,
            });
            return;
        };
        let result = match goal {
            PhysicalGoal::Drink => self.apply_drink(event.agent, target),
            PhysicalGoal::Eat => self.apply_eat(event.agent),
            PhysicalGoal::GatherMaterial => self.apply_gather(event.agent, reason),
            PhysicalGoal::Sleep => {
                let sleep = self.population.finish_sleep(event.agent);
                if let Some(sleep) = sleep {
                    self.sleep_diagnostics.push(SleepDiagnostic {
                        sleep,
                        at: self.time,
                        kind: SleepDiagnosticKind::Woke,
                        interruption: None,
                    });
                    Ok(())
                } else {
                    Err(PolicyFailureReason::InconsistentState)
                }
            }
            PhysicalGoal::BuildShelter => self.apply_build_completion(event.agent),
            PhysicalGoal::Signal => self.apply_signal(event.agent, target),
            PhysicalGoal::SeekShelter | PhysicalGoal::Incapacitated => {
                Err(PolicyFailureReason::DeferredToLaterSlice)
            }
            PhysicalGoal::SeekWater
            | PhysicalGoal::SeekFood
            | PhysicalGoal::Explore
            | PhysicalGoal::Wait => Err(PolicyFailureReason::InconsistentState),
        };
        match result {
            Ok(()) => {
                self.policy_diagnostics.push(PolicyDiagnostic {
                    agent: event.agent,
                    at: self.time,
                    goal,
                    target: Some(target),
                    reason,
                    kind: PolicyDiagnosticKind::ActionCompleted,
                    failure: None,
                });
                if self.policy_active
                    && let Err(error) = self.population.schedule_policy_decision(
                        &mut self.scheduler,
                        self.time,
                        event.agent,
                        1,
                        PolicyReason::ActionCompleted,
                        false,
                    )
                {
                    self.schedule_policy_retry(
                        event.agent,
                        goal,
                        Some(target),
                        PolicyReason::Retry,
                        move_failure(error),
                    );
                }
            }
            Err(failure) => {
                let deferred = failure == PolicyFailureReason::DeferredToLaterSlice;
                self.policy_diagnostics.push(PolicyDiagnostic {
                    agent: event.agent,
                    at: self.time,
                    goal,
                    target: Some(target),
                    reason,
                    kind: if deferred {
                        PolicyDiagnosticKind::ActionDeferred
                    } else {
                        PolicyDiagnosticKind::ActionCompleted
                    },
                    failure: Some(failure),
                });
                if self.policy_active {
                    self.schedule_policy_retry(
                        event.agent,
                        goal,
                        Some(target),
                        PolicyReason::Retry,
                        failure,
                    );
                }
            }
        }
    }

    pub(super) fn apply_drink(
        &mut self,
        agent: AgentId,
        target: WorldPosition,
    ) -> Result<(), PolicyFailureReason> {
        let position = self
            .population
            .view(agent)
            .ok_or(PolicyFailureReason::InconsistentState)?
            .position;
        if position != target || !self.has_adjacent_drinkable_water(position)? {
            return Err(PolicyFailureReason::InvalidWaterAccess);
        }
        self.population
            .apply_need_relief(
                &mut self.scheduler,
                self.time,
                agent,
                NeedKind::Thirst,
                DRINK_THIRST_RELIEF,
            )
            .map_err(action_effect_failure)
    }

    /// Eats one unit of whatever carried material the agent most wants to eat.
    /// The agent feels what it really does; anyone watching sees it eat, and
    /// sees it retch if it was sickening.
    pub(super) fn apply_eat(&mut self, agent: AgentId) -> Result<(), PolicyFailureReason> {
        let inventory = self
            .population
            .inventory(agent)
            .ok_or(PolicyFailureReason::InconsistentState)?;
        let material = self
            .food_values(agent)
            .best_carried(inventory)
            .ok_or(PolicyFailureReason::NoEdibleInventory)?;
        let properties = self
            .population
            .eat(&mut self.scheduler, self.time, agent, material)
            .map_err(action_effect_failure)?;
        if !self.policy_options.memory {
            return Ok(());
        }
        let retched = properties.toxicity > 0;
        let eater = self.minds.get_mut(agent);
        let first_taste = !eater.affordances.knows(material);
        eater.affordances.felt(material, properties);
        let watchers: Vec<AgentId> = self
            .perceive_physical(agent, crate::PHYSICAL_POLICY_RADIUS)
            .map(|perception| {
                perception
                    .agents
                    .iter()
                    .filter(|other| {
                        other.id != agent && super::cognition::can_watch(other.activity)
                    })
                    .map(|other| other.id)
                    .collect()
            })
            .unwrap_or_default();
        for watcher in &watchers {
            self.minds
                .get_mut(*watcher)
                .affordances
                .saw_eaten(material, retched);
        }
        self.meal_events.push(crate::MealEvent {
            agent,
            at: self.time,
            material,
            retched,
            first_taste,
            watchers: watchers.len() as u16,
        });
        Ok(())
    }

    fn apply_gather(
        &mut self,
        agent: AgentId,
        reason: PolicyReason,
    ) -> Result<(), PolicyFailureReason> {
        let position = self
            .population
            .view(agent)
            .ok_or(PolicyFailureReason::InconsistentState)?
            .position;
        let inventory = self
            .population
            .inventory(agent)
            .ok_or(PolicyFailureReason::InconsistentState)?;
        let food = self.food_values(agent);
        let candidate = [
            WorldPosition {
                x: position.x,
                y: position.y - 1,
            },
            WorldPosition {
                x: position.x - 1,
                y: position.y,
            },
            position,
            WorldPosition {
                x: position.x + 1,
                y: position.y,
            },
            WorldPosition {
                x: position.x,
                y: position.y + 1,
            },
        ]
        .into_iter()
        .filter_map(|candidate| {
            self.available_resource_at(candidate)
                .ok()
                .flatten()
                .filter(|resource| {
                    inventory.can_add(resource.kind)
                        && (reason != PolicyReason::HungerThreshold || food.is_food(resource.kind))
                        && (reason != PolicyReason::ShelterMaterials
                            || (resource.kind == Material::Wood
                                && inventory.amount(Material::Wood) < SHELTER_WOOD_COST))
                })
                .map(|resource| (candidate, resource))
        })
        .min_by_key(|(candidate, resource)| {
            (
                position.x.abs_diff(candidate.x) + position.y.abs_diff(candidate.y),
                candidate.y,
                candidate.x,
                resource.kind as u8,
            )
        });
        let Some((resource_position, resource)) = candidate else {
            return Err(
                if Material::ALL
                    .into_iter()
                    .all(|kind| !inventory.can_add(kind))
                {
                    PolicyFailureReason::InventoryFull
                } else {
                    PolicyFailureReason::ResourceDepleted
                },
            );
        };
        let maximum = inventory
            .remaining_capacity(resource.kind)
            .min(GATHER_YIELD);
        let gathered = if self.spawned_objects.at(resource_position).is_some() {
            self.spawned_objects.gather(resource_position, maximum)
        } else {
            self.resource_deltas
                .gather(&self.world, resource_position, maximum)
                .map_err(|_| PolicyFailureReason::TargetUnavailable)?
        };
        let Some((kind, gathered)) = gathered else {
            return Err(PolicyFailureReason::ResourceDepleted);
        };
        let accepted = self.population.add_inventory(agent, kind, gathered);
        debug_assert_eq!(accepted, gathered);
        Ok(())
    }

    pub(super) fn has_adjacent_drinkable_water(
        &self,
        position: WorldPosition,
    ) -> Result<bool, PolicyFailureReason> {
        let mut failure = None;
        for candidate in [
            position,
            WorldPosition {
                x: position.x,
                y: position.y - 1,
            },
            WorldPosition {
                x: position.x - 1,
                y: position.y,
            },
            WorldPosition {
                x: position.x + 1,
                y: position.y,
            },
            WorldPosition {
                x: position.x,
                y: position.y + 1,
            },
        ] {
            match self.spawned_objects.water_at(&self.world, candidate) {
                Ok(Some(source)) if source.is_drinkable() => return Ok(true),
                Ok(_) => {}
                Err(WorldQueryError::Unloaded) => failure = Some(PolicyFailureReason::Unloaded),
                Err(WorldQueryError::OutsideWorldBounds) => {
                    failure.get_or_insert(PolicyFailureReason::OutsideWorld);
                }
                Err(WorldQueryError::NonCardinalStep) => {
                    return Err(PolicyFailureReason::InconsistentState);
                }
            }
        }
        failure.map_or(Ok(false), Err)
    }
}
