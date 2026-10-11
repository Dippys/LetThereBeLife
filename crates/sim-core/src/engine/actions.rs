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

/// Food kept on hand when putting things away in a hut.
const KEPT_FOOD: u8 = 2;
/// Most food taken out of a hut at once.
const FETCHED_FOOD: u8 = 6;
/// Cold and tiredness one step through water adds (more where it's deep).
const SWIM_CHILL: u16 = 120;
const SWIM_EFFORT: u16 = 60;

/// One use in this many wears a blade out.
const BLADE_WEAR_ODDS: u64 = 6;

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
                let healing = self.sleep_healing(event.agent);
                let sleep = self.population.finish_sleep(event.agent, healing);
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
            PhysicalGoal::BuildShelter | PhysicalGoal::BuildHearth => {
                self.apply_build_completion(event.agent)
            }
            PhysicalGoal::WarmUp => self.apply_warm_up(event.agent),
            PhysicalGoal::TendFire => self.apply_tend_fire(event.agent),
            PhysicalGoal::Craft => self.apply_craft(event.agent),
            PhysicalGoal::Store => self.apply_store(event.agent, target),
            PhysicalGoal::Fetch => self.apply_fetch(event.agent, target),
            PhysicalGoal::Drop => self.apply_drop(event.agent),
            PhysicalGoal::Signal => self.apply_signal(event.agent, target),
            PhysicalGoal::Hunt => self.apply_hunt(event.agent, target),
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

    /// Warms up by a hearth: the cold eases a lot. It learns hearths warm (it
    /// felt it), and anyone watching sees it warming its hands.
    pub(super) fn apply_warm_up(&mut self, agent: AgentId) -> Result<(), PolicyFailureReason> {
        let position = self
            .population
            .view(agent)
            .ok_or(PolicyFailureReason::InconsistentState)?
            .position;
        let now = crate::cognition::belief_seconds(self.time);
        if !self.structures.fire_beside(position, now) {
            return Err(PolicyFailureReason::TargetUnavailable);
        }
        self.population
            .apply_need_relief(
                &mut self.scheduler,
                self.time,
                agent,
                NeedKind::Exposure,
                crate::HEARTH_WARMTH,
            )
            .map_err(action_effect_failure)?;
        if self.policy_options.memory {
            self.minds.get_mut(agent).crafts.warmed();
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
            for watcher in watchers {
                self.minds.get_mut(watcher).crafts.saw_warming();
            }
        }
        Ok(())
    }

    /// A step into a lake or river chills and tires: twice as much where it's
    /// deep enough to swim.
    pub(super) fn feel_the_water(&mut self, agent: AgentId) {
        let Some(position) = self.population.view(agent).map(|view| view.position) else {
            return;
        };
        let Some(cell) = self.world.cell(position) else {
            return;
        };
        let depth = match cell.surface() {
            crate::SurfaceType::ShallowWater => 1,
            crate::SurfaceType::DeepWater => 2,
            _ if self
                .spawned_objects
                .at(position)
                .is_some_and(|object| object.kind == crate::SpawnKind::Water) =>
            {
                1
            }
            _ => return,
        };
        for (need, amount) in [
            (NeedKind::Exposure, SWIM_CHILL * depth),
            (NeedKind::Rest, SWIM_EFFORT * depth),
        ] {
            let _ =
                self.population
                    .worsen_need(&mut self.scheduler, self.time, agent, need, amount);
        }
    }

    /// Puts one unit of something that burns on the fire beside the agent,
    /// relighting it if it was out. Anyone watching sees fire being kept.
    pub(super) fn apply_tend_fire(&mut self, agent: AgentId) -> Result<(), PolicyFailureReason> {
        let position = self
            .population
            .view(agent)
            .ok_or(PolicyFailureReason::InconsistentState)?
            .position;
        let fire = self
            .structures
            .hearth_position_beside(position)
            .ok_or(PolicyFailureReason::TargetUnavailable)?;
        let inventory = self
            .population
            .inventory(agent)
            .ok_or(PolicyFailureReason::InconsistentState)?;
        let fuel = crate::Material::ALL
            .into_iter()
            .find(|material| {
                material.properties().fuel_seconds > 0 && inventory.amount(*material) > 0
            })
            .ok_or(PolicyFailureReason::TargetUnavailable)?;
        let now = crate::cognition::belief_seconds(self.time);
        let was_burning = self.structures.fire_beside(position, now);
        if self.population.take(agent, fuel, 1) == 0
            || !self
                .structures
                .add_fuel(fire, fuel.properties().fuel_seconds, now)
        {
            return Err(PolicyFailureReason::TargetUnavailable);
        }
        self.fire_events.push(crate::FireEvent {
            agent,
            at: fire,
            relit: !was_burning,
        });
        if self.policy_options.memory {
            self.minds.get_mut(agent).crafts.saw_warming();
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
            for watcher in watchers {
                self.minds.get_mut(watcher).crafts.saw_warming();
            }
        }
        Ok(())
    }

    /// Puts everything carried into the hut at `hut` (beside the agent), except a
    /// little food and any tool, as far as there's room.
    pub(super) fn apply_store(
        &mut self,
        agent: AgentId,
        hut: WorldPosition,
    ) -> Result<(), PolicyFailureReason> {
        let id = self.hut_beside(agent, hut)?;
        let inventory = self
            .population
            .inventory(agent)
            .ok_or(PolicyFailureReason::InconsistentState)?;
        let food = self.food_values(agent);
        let mut kept_food = 0_u8;
        let mut offered = crate::InventoryView::default();
        for (material, amount) in inventory.carried() {
            let keep = if material.properties().cutting {
                amount
            } else if food.is_food(material) {
                let keep = amount.min(KEPT_FOOD.saturating_sub(kept_food));
                kept_food += keep;
                keep
            } else {
                0
            };
            offered.items[material as usize] = amount - keep;
        }
        let moved = self.structures.deposit(id, offered);
        if moved.total() == 0 {
            return Err(PolicyFailureReason::TargetUnavailable);
        }
        for (material, amount) in moved.carried() {
            self.population.take(agent, material, amount);
        }
        self.stored += u64::from(moved.total());
        if moved.carried().any(|(material, _)| food.is_food(material)) {
            self.minds.get_mut(agent).stored_food = crate::agent::CompactPosition::checked(hut);
        }
        Ok(())
    }

    /// Sets down whatever it has no use for, to make room: it keeps any tool,
    /// the wood for a hut it can build (or a fire it keeps), stone for a
    /// hearth or a blade, and a little food (less while a hut is waiting on wood).
    pub(super) fn apply_drop(&mut self, agent: AgentId) -> Result<(), PolicyFailureReason> {
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
        let crafts = self
            .minds
            .get(agent)
            .map(|mind| mind.crafts)
            .unwrap_or_default();
        // Hut wood is worth keeping only for someone who can build one and has
        // no hut nearby already.
        let homeless = self.minds.get(agent).is_none_or(|mind| {
            mind.map
                .nearest_seen_distance(crate::cognition::LandmarkKind::SHELTER, position)
                .is_none_or(|distance| distance > crate::policy::HUT_SPACING)
        });
        let builds_huts = crafts.knows_huts() && self.fit_for_heavy_work(agent) && homeless;
        let wood_for_hut =
            builds_huts && inventory.amount(Material::Wood) < crate::SHELTER_WOOD_COST;
        let keep_wood = if builds_huts {
            crate::SHELTER_WOOD_COST
        } else if crafts.knows_hearths() {
            crate::HEARTH_WOOD_COST + 1
        } else {
            0
        };
        let keep_stone = if wood_for_hut {
            0
        } else if crafts.knows_hearths() {
            crate::HEARTH_STONE_COST
        } else {
            u8::from(crafts.knows_knapping())
        };
        let mut food_left = if wood_for_hut {
            KEPT_FOOD
        } else {
            2 * KEPT_FOOD
        };
        let mut dropped = 0_u8;
        for (material, amount) in inventory.carried() {
            let keep = if material.properties().cutting {
                amount
            } else if food.is_food(material) {
                let keep = amount.min(food_left);
                food_left -= keep;
                keep
            } else {
                match material {
                    Material::Wood => amount.min(keep_wood),
                    Material::Stone => amount.min(keep_stone),
                    _ => 0,
                }
            };
            dropped += self.population.take(agent, material, amount - keep);
        }
        if dropped == 0 {
            return Err(PolicyFailureReason::TargetUnavailable);
        }
        Ok(())
    }

    /// Takes food out of the hut at `hut` (beside the agent): what it thinks is
    /// food, as much as it can carry up to a few meals.
    pub(super) fn apply_fetch(
        &mut self,
        agent: AgentId,
        hut: WorldPosition,
    ) -> Result<(), PolicyFailureReason> {
        let id = self.hut_beside(agent, hut)?;
        let food = self.food_values(agent);
        let mut room = self
            .population
            .inventory(agent)
            .ok_or(PolicyFailureReason::InconsistentState)?
            .remaining_capacity(Material::Berries)
            .min(FETCHED_FOOD);
        let stored = self
            .structures
            .view(id)
            .map(|view| view.stored)
            .unwrap_or_default();
        let mut taken = 0_u8;
        for (material, _) in stored
            .carried()
            .filter(|(material, _)| food.is_food(*material))
        {
            let got = self.structures.withdraw(id, material, room);
            self.population.add_inventory(agent, material, got);
            room -= got;
            taken += got;
        }
        let left = self.structures.view(id).is_some_and(|view| {
            view.stored
                .carried()
                .any(|(material, _)| food.is_food(material))
        });
        if !left {
            let mind = self.minds.get_mut(agent);
            if mind.stored_food == crate::agent::CompactPosition::checked(hut) {
                mind.stored_food = None;
            }
        }
        if taken == 0 {
            return Err(PolicyFailureReason::TargetUnavailable);
        }
        self.fetched += u64::from(taken);
        Ok(())
    }

    /// The finished hut at `hut`, if the agent stands within a cell of it.
    fn hut_beside(
        &self,
        agent: AgentId,
        hut: WorldPosition,
    ) -> Result<crate::StructureId, PolicyFailureReason> {
        let position = self
            .population
            .view(agent)
            .ok_or(PolicyFailureReason::InconsistentState)?
            .position;
        if position.x.abs_diff(hut.x).max(position.y.abs_diff(hut.y)) > 1 {
            return Err(PolicyFailureReason::TargetUnavailable);
        }
        let id = self
            .structures
            .structure_at(hut)
            .ok_or(PolicyFailureReason::TargetUnavailable)?;
        self.structures
            .view(id)
            .filter(|view| {
                view.kind == crate::StructureKind::Shelter
                    && view.state == crate::StructureState::Complete
            })
            .map(|_| id)
            .ok_or(PolicyFailureReason::TargetUnavailable)
    }

    /// Makes the first thing the agent knows how to make from what it carries
    /// (a blade from a stone). Anyone watching sees how it's done.
    pub(super) fn apply_craft(&mut self, agent: AgentId) -> Result<(), PolicyFailureReason> {
        if !self
            .minds
            .get(agent)
            .is_some_and(|mind| mind.crafts.knows_knapping())
        {
            return Err(PolicyFailureReason::TargetUnavailable);
        }
        let inventory = self
            .population
            .inventory(agent)
            .ok_or(PolicyFailureReason::InconsistentState)?;
        let (made, (input, amount)) = crate::Material::ALL
            .into_iter()
            .filter_map(|material| Some((material, material.properties().made_from?)))
            .find(|&(made, (input, amount))| {
                inventory.amount(input) >= amount && inventory.can_add(made)
            })
            .ok_or(PolicyFailureReason::TargetUnavailable)?;
        if self.population.take(agent, input, amount) < amount {
            return Err(PolicyFailureReason::InconsistentState);
        }
        self.population.add_inventory(agent, made, 1);
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
        if self.policy_options.memory {
            self.minds.get_mut(agent).crafts.saw_knapping();
            for &watcher in &watchers {
                self.minds.get_mut(watcher).crafts.saw_knapping();
            }
        }
        self.craft_events.push(crate::CraftEvent {
            agent,
            made,
            watchers: watchers.len() as u16,
        });
        Ok(())
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
        // Felling and chopping wood takes a grown body; anyone can pick things up.
        let heavy_work = self.fit_for_heavy_work(agent);
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
                        && (heavy_work
                            || resource.kind.properties().handling != crate::Handling::Chop)
                        && (reason != PolicyReason::HearthMaterials
                            || matches!(resource.kind, Material::Stone | Material::Wood))
                        && (reason != PolicyReason::Crafting
                            || Material::ALL.into_iter().any(|made| {
                                made.properties()
                                    .made_from
                                    .is_some_and(|(input, _)| input == resource.kind)
                            }))
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
        // A cutting edge doubles what one go at chopping or carving yields,
        // and wears with use.
        let edge = Material::ALL
            .into_iter()
            .find(|material| material.properties().cutting && inventory.amount(*material) > 0);
        let cuts = matches!(
            resource.kind.properties().handling,
            crate::Handling::Chop | crate::Handling::Carve
        );
        let edge = edge.filter(|_| cuts);
        let maximum = inventory
            .remaining_capacity(resource.kind)
            .min(GATHER_YIELD * if edge.is_some() { 2 } else { 1 });
        let gathered = if resource.kind == Material::Meat {
            self.wildlife.butcher(resource_position, maximum)
        } else if self.spawned_objects.at(resource_position).is_some() {
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
        if let Some(blade) = edge
            && crate::wildlife::mix(self.config.seed ^ self.time.ticks() ^ u64::from(agent.get()))
                % BLADE_WEAR_ODDS
                == 0
        {
            self.population.take(agent, blade, 1);
        }
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
