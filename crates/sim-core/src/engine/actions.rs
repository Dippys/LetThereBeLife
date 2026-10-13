//! Timed physical actions: sleep requests and action/wake completion with
//! drinking, eating, and gathering effects.

use super::errors::{action_effect_failure, move_failure};
use crate::agent::MovementEnvironment;
use crate::scheduler::{self};
use crate::{
    AgentId, DRINK_THIRST_RELIEF, Engine, GATHER_YIELD, Material, NeedKind, PhysicalGoal,
    PolicyDiagnostic, PolicyDiagnosticKind, PolicyFailureReason, PolicyReason, SHELTER_WOOD_COST,
    SleepDiagnostic, SleepDiagnosticKind, SleepQuality, SleepRequestError, SleepView,
    WorldPosition, WorldQueryError,
};

/// Pride at something made with others watching, plus this much per watcher.
const PROUD_BASE: u8 = 40;
const PROUD_PER_WATCHER: u8 = 20;
/// Anger at a taker seen at one's own family's hut, or at anyone's.
const ROBBED_ANGER: u8 = 96;
const WITNESS_ANGER: u8 = 24;
/// Shame at being seen taking.
const CAUGHT_SHAME: u8 = 90;
/// Food handed over by someone who backs down; shame and fear it feels; how
/// much of the anger that settles.
const CONFRONT_FOOD: u8 = 2;
const CONFRONT_SHAME: u8 = 90;
const CONFRONT_FEAR: u8 = 80;
const CONFRONT_SETTLED: u8 = 96;
/// Frustration at a try that made nothing.
const FAILED_TRY: u8 = 24;
/// Health a scuffle costs each side.
const SCUFFLE_WOUND: u16 = 800;
/// Most batches smoked at one sitting.
const SMOKING_BATCH: u8 = 6;

/// Food kept on hand when putting things away in a hut.
const KEPT_FOOD: u8 = 2;
/// Most food taken out of a hut at once.
const FETCHED_FOOD: u8 = 6;
/// Cold and tiredness one step through water adds (more where it's deep).
const SWIM_CHILL: u16 = 120;
const SWIM_EFFORT: u16 = 60;

impl Engine {
    /// Sleeping under a roof with a burning fire close by, or in a stone
    /// hut, warms faster.
    pub(super) fn by_the_fire(
        &self,
        quality: SleepQuality,
        position: WorldPosition,
    ) -> SleepQuality {
        let now = crate::cognition::belief_seconds(self.time);
        // A stone hut keeps as warm as a hut by a fire.
        if quality == SleepQuality::Sheltered
            && (self.structures.stone_beside(position)
                || self
                    .structures
                    .fire_within(position, now, crate::FIRE_WARMTH_REACH))
        {
            SleepQuality::Hearthside
        } else {
            quality
        }
    }

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
        let quality = self.by_the_fire(quality, position);
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
            PhysicalGoal::BuildShelter
            | PhysicalGoal::BuildHearth
            | PhysicalGoal::BuildRack
            | PhysicalGoal::DigPit => self.apply_build_completion(event.agent),
            PhysicalGoal::Repair => self.apply_repair(event.agent, target),
            PhysicalGoal::WallInStone => self.apply_wall_in_stone(event.agent, target),
            PhysicalGoal::Confront => self.apply_confront(event.agent, target),
            PhysicalGoal::WarmUp => self.apply_warm_up(event.agent),
            PhysicalGoal::TendFire => self.apply_tend_fire(event.agent),
            PhysicalGoal::Craft => self.apply_craft(event.agent),
            PhysicalGoal::Plant => self.apply_plant(event.agent, target),
            PhysicalGoal::Bury => self.apply_bury(event.agent, target),
            PhysicalGoal::Cook => self.apply_cook(event.agent, target),
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
        self.minds.get_mut(agent).model.used(
            crate::cognition::Choice::TendFire,
            crate::cognition::Facts::HAS_WOOD,
        );

        self.fire_events.push(crate::FireEvent {
            agent,
            at: fire,
            relit: !was_burning,
        });
        self.minds.get_mut(agent).crafts.saw_warming();
        let relit = !was_burning;
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
            let mind = self.minds.get_mut(watcher);
            mind.crafts.saw_warming();
            // Seen fed back to life, a fire shows what fuel does for it.
            if relit {
                mind.model.saw(
                    crate::cognition::Choice::TendFire,
                    crate::cognition::Facts::HAS_WOOD | crate::cognition::Facts::DEAD_FIRE,
                    crate::cognition::Facts::FIRE,
                );
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
            let keep = if material.properties().worn() {
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
            let keep = if material.properties().worn() {
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
        // Food taken from a hut it knows is another family's: whoever sees it
        // is angry with the taker (the hut's own people most), and a taker
        // who's seen is ashamed.
        let theirs = self.minds.get(agent).is_some_and(|mind| {
            mind.social.others_huts().any(|other| other == hut)
                && mind.home.hut() != Some(hut)
                && mind.social.family_hut() != Some(hut)
        });
        if theirs {
            self.thefts += 1;
            let watchers = self.watchers_of(agent);
            self.life_events.push(crate::LifeEvent::Took {
                agent,
                seen: !watchers.is_empty(),
            });
            if !watchers.is_empty() {
                self.thefts_seen += 1;
                self.minds
                    .get_mut(agent)
                    .feelings
                    .feel(crate::Feeling::Shame, CAUGHT_SHAME);
            }
            for watcher in watchers {
                let temper = u32::from(self.personality_in_use(watcher).temper);
                let mind = self.minds.get_mut(watcher);
                let own = mind.home.hut() == Some(hut) || mind.social.family_hut() == Some(hut);
                let anger = if own { ROBBED_ANGER } else { WITNESS_ANGER };
                let anger = (u32::from(anger) * (64 + temper) / 192).min(255) as u8;
                if let Some(slot) = mind.social.slot_of(agent) {
                    mind.social.provoke(slot, anger);
                }
            }
        }
        Ok(())
    }

    /// Faces down whoever stands at `target` that it's furious with. The
    /// bolder and angrier of the two carries it: the other backs down
    /// (hands over food, ashamed and frightened), or stands its ground and it
    /// comes to a scuffle that hurts both a little but kills nobody.
    pub(super) fn apply_confront(
        &mut self,
        agent: AgentId,
        target: WorldPosition,
    ) -> Result<(), PolicyFailureReason> {
        let other = self
            .perceive_physical(agent, crate::PHYSICAL_POLICY_RADIUS)
            .map_err(|_| PolicyFailureReason::TargetUnavailable)?
            .agents
            .iter()
            .find(|seen| {
                seen.id != agent
                    && seen
                        .position
                        .x
                        .abs_diff(target.x)
                        .max(seen.position.y.abs_diff(target.y))
                        <= 1
                    && super::cognition::can_watch(seen.activity)
            })
            .map(|seen| seen.id)
            .ok_or(PolicyFailureReason::TargetUnavailable)?;
        self.confrontations += 1;
        let now = crate::cognition::belief_seconds(self.time);
        for who in [agent, other] {
            self.minds.get_mut(who).confronted_at = now;
        }
        let anger_of = |engine: &Self, from: AgentId, at: AgentId| {
            engine.minds.get(from).map_or(0, |mind| {
                mind.social
                    .slot_of(at)
                    .map_or(0, |slot| mind.social.anger(slot))
            })
        };
        let roll = crate::wildlife::mix(
            self.config.life_seed ^ 0x4655_5259 ^ self.time.ticks() ^ u64::from(agent.get()) << 32,
        );
        let force = |engine: &Self, who: AgentId, at: AgentId| {
            u32::from(engine.personality_in_use(who).boldness)
                + u32::from(anger_of(engine, who, at)) / 2
        };
        let mine = force(self, agent, other) + (roll % 64) as u32;
        let theirs = force(self, other, agent) + ((roll >> 8) % 64) as u32;
        self.life_events.push(crate::LifeEvent::StandOff {
            agent,
            other,
            backed_down: mine > theirs,
        });
        if mine > theirs {
            // It backs down.
            self.backed_down += 1;
            let food = self.food_values(other);
            let given = self
                .population
                .inventory(other)
                .map_or(Vec::new(), |inventory| {
                    inventory
                        .carried()
                        .filter(|(material, _)| food.is_food(*material))
                        .map(|(material, count)| (material, count.min(CONFRONT_FOOD)))
                        .take(1)
                        .collect()
                });
            for (material, count) in given {
                let taken = self.population.take(other, material, count);
                let kept = self.population.add_inventory(agent, material, taken);
                if kept < taken {
                    self.population.add_inventory(other, material, taken - kept);
                }
            }
            let mind = self.minds.get_mut(other);
            mind.feelings.feel(crate::Feeling::Shame, CONFRONT_SHAME);
            mind.feelings.feel(crate::Feeling::Fear, CONFRONT_FEAR);
            let mind = self.minds.get_mut(agent);
            if let Some(slot) = mind.social.slot_of(other) {
                mind.social.soothe(slot, CONFRONT_SETTLED);
            }
        } else {
            // A scuffle: both hurt a little and shaken, and it clears the air
            // (both half as angry after); nobody is hurt past walking away.
            self.scuffles += 1;
            for (who, at) in [(agent, other), (other, agent)] {
                let health = self.health(who).map_or(0, |health| health.value);
                if health > crate::HEALTH_INCAPACITATION_THRESHOLD + 2 * SCUFFLE_WOUND {
                    self.wound(who, SCUFFLE_WOUND);
                }
                let mind = self.minds.get_mut(who);
                mind.feelings.feel(crate::Feeling::Fear, CONFRONT_FEAR / 2);
                if let Some(slot) = mind.social.slot_of(at) {
                    let anger = mind.social.anger(slot);
                    mind.social.soothe(slot, anger / 2);
                }
            }
        }
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
            .filter(|view| view.kind.stores() && view.state == crate::StructureState::Complete)
            .map(|_| id)
            .ok_or(PolicyFailureReason::TargetUnavailable)
    }

    /// Makes something by hand or with an edge: what it set out to try (an
    /// experiment), or else the first thing it knows how to make and wants
    /// (a blade, a basket, a spear, pemmican). Anyone watching sees how.
    pub(super) fn apply_craft(&mut self, agent: AgentId) -> Result<(), PolicyFailureReason> {
        let inventory = self
            .population
            .inventory(agent)
            .ok_or(PolicyFailureReason::InconsistentState)?;
        let has_edge = inventory
            .carried()
            .any(|(material, _)| material.properties().cutting);
        let attempt = self.minds.get_mut(agent).crafts.take_pending();
        if attempt.is_some() {
            // A project's known recipe is simply made; anything else is a try.
            if let Some((index, recipe)) = attempt.recipe().filter(|(index, _)| {
                self.minds
                    .get(agent)
                    .is_some_and(|mind| mind.crafts.knows(*index))
            }) {
                let ready = recipe
                    .inputs
                    .iter()
                    .all(|&(input, count)| inventory.amount(input) >= count)
                    && inventory.can_add(recipe.output);
                if !ready {
                    return Err(PolicyFailureReason::InsufficientMaterials);
                }
                return self.make(agent, index, recipe);
            }
            return self.try_attempt(agent, attempt);
        }
        let crafts = self
            .minds
            .get(agent)
            .map(|mind| mind.crafts)
            .unwrap_or_default();
        // The first thing it can make that it doesn't have yet, or pemmican
        // from dried meat and berries.
        let (index, recipe) = crate::RECIPES
            .iter()
            .copied()
            .enumerate()
            .filter(|(index, recipe)| {
                crafts.knows(*index)
                    && match recipe.method {
                        crate::Method::Hand => true,
                        crate::Method::Edge => has_edge,
                        crate::Method::Fire | crate::Method::Smoke | crate::Method::Rack => false,
                    }
                    && (recipe.output.properties().nutrition > 0
                        || inventory.amount(recipe.output) == 0)
                    && recipe
                        .inputs
                        .iter()
                        .all(|&(input, count)| inventory.amount(input) >= count)
                    && inventory.can_add(recipe.output)
            })
            .min_by_key(|(_, recipe)| {
                // Food first: it's what would otherwise go bad.
                (
                    recipe.output.properties().nutrition == 0,
                    recipe.output as u8,
                )
            })
            .ok_or(PolicyFailureReason::TargetUnavailable)?;
        self.make(agent, index, recipe)
    }

    /// Turns the inputs of recipe `index` it carries into what it makes, and
    /// lets anyone watching learn how.
    pub(super) fn make(
        &mut self,
        agent: AgentId,
        index: usize,
        recipe: crate::Recipe,
    ) -> Result<(), PolicyFailureReason> {
        for (input, count) in recipe.inputs {
            if count > 0 && self.population.take(agent, input, count) < count {
                return Err(PolicyFailureReason::InconsistentState);
            }
        }
        let made = recipe.output;
        if made == Material::Blade {
            self.minds.get_mut(agent).model.used(
                crate::cognition::Choice::Craft,
                crate::cognition::Facts::HAS_STONE,
            );
        }
        self.population.add_inventory(agent, made, recipe.makes);
        let watchers = self.watchers_of(agent);
        for &who in std::iter::once(&agent).chain(&watchers) {
            let mind = self.minds.get_mut(who);
            mind.crafts.learn(index);
            if made == Material::Blade {
                mind.crafts.saw_knapping();
            }
            // Made food is expected to feed as what it was made from did.
            if made.properties().nutrition > 0 {
                mind.affordances.saw_cooked(recipe.inputs[0].0, made);
            }
        }
        for &watcher in &watchers {
            if made == Material::Blade {
                self.minds.get_mut(watcher).model.saw(
                    crate::cognition::Choice::Craft,
                    crate::cognition::Facts::HAS_STONE,
                    crate::cognition::Facts::HAS_BLADE,
                );
            }
        }
        self.name_made(agent, crate::Concept::Material(made), &watchers);
        self.proud_of_work(agent, watchers.len());
        self.craft_events.push(crate::CraftEvent {
            agent,
            made,
            watchers: watchers.len() as u16,
        });
        Ok(())
    }

    /// Something made or built while others watched: pride, more the more
    /// who saw.
    pub(super) fn proud_of_work(&mut self, agent: AgentId, watchers: usize) {
        if watchers > 0 {
            let pride = PROUD_BASE + PROUD_PER_WATCHER * watchers.min(5) as u8;
            self.minds
                .get_mut(agent)
                .feelings
                .feel(crate::Feeling::Pride, pride);
        }
    }

    /// Tries a combination: if physics has a recipe for it (and it carries
    /// enough), it's made and learned; otherwise one of each input is spoiled
    /// and it remembers not to try that again.
    fn try_attempt(
        &mut self,
        agent: AgentId,
        attempt: crate::cognition::Attempt,
    ) -> Result<(), PolicyFailureReason> {
        self.experiments += 1;
        let inventory = self
            .population
            .inventory(agent)
            .ok_or(PolicyFailureReason::InconsistentState)?;
        if let Some((index, recipe)) = attempt.recipe()
            && recipe
                .inputs
                .iter()
                .all(|&(input, count)| inventory.amount(input) >= count)
        {
            // It works, but there's no room to hold what it makes: it puts it
            // off, none the wiser.
            if !inventory.can_add(recipe.output) {
                return Err(PolicyFailureReason::InventoryFull);
            }
            self.discoveries += 1;
            self.life_events.push(crate::LifeEvent::Discovered {
                agent,
                made: recipe.output,
            });
            return self.make(agent, index, recipe);
        }
        // Food tried is spoiled, and what was cut with an edge; things put
        // together by hand come apart again, and tools come through.
        let cut = attempt.method() == crate::Method::Edge;
        for kind in attempt.kinds().into_iter().flatten() {
            let properties = kind.properties();
            if !properties.worn() && (cut || properties.nutrition > 0) {
                self.population.take(agent, kind, 1);
            }
        }
        let mind = self.minds.get_mut(agent);
        mind.crafts.failed(attempt);
        mind.feelings.feel(crate::Feeling::Frustration, FAILED_TRY);
        Ok(())
    }

    /// `agent` used `tool` once: about one use in its wear odds breaks it.
    pub(super) fn wear(&mut self, agent: AgentId, tool: Material) {
        let odds = u64::from(tool.properties().wear_odds);
        if odds > 0
            && crate::wildlife::mix(
                self.config.life_seed
                    ^ 0x5745_4152
                    ^ self.time.ticks()
                    ^ u64::from(agent.get()) << 32,
            ) % odds
                == 0
        {
            self.population.take(agent, tool, 1);
        }
    }

    /// Mends the structure at `target` (beside it) with wood, or stone for a
    /// stone hut.
    pub(super) fn apply_repair(
        &mut self,
        agent: AgentId,
        target: WorldPosition,
    ) -> Result<(), PolicyFailureReason> {
        let id = self
            .structures
            .structure_at(target)
            .ok_or(PolicyFailureReason::TargetUnavailable)?;
        let view = self
            .structures
            .view(id)
            .ok_or(PolicyFailureReason::TargetUnavailable)?;
        // Wood mends a hut; a stone hut, or anyone out of wood, uses stone,
        // and whoever packs stones into a wall finds out they hold.
        let wood = self
            .population
            .inventory(agent)
            .is_some_and(|inventory| inventory.amount(Material::Wood) >= crate::REPAIR_COST);
        let material = if view.stone || !wood {
            Material::Stone
        } else {
            Material::Wood
        };
        if self.population.take(agent, material, crate::REPAIR_COST) < crate::REPAIR_COST {
            return Err(PolicyFailureReason::InsufficientMaterials);
        }
        self.structures.repair(id);
        self.practise(agent, crate::Activity::Building);
        if material == Material::Stone && view.kind == crate::StructureKind::Shelter {
            let watchers = self.watchers_of(agent);
            for who in std::iter::once(agent).chain(watchers) {
                self.minds
                    .get_mut(who)
                    .crafts
                    .learn_design(crate::cognition::Design::StoneWalls);
            }
        }
        Ok(())
    }

    /// Walls the hut at `target` (beside it) in stone. Whoever does it or
    /// watches knows how from then on.
    pub(super) fn apply_wall_in_stone(
        &mut self,
        agent: AgentId,
        target: WorldPosition,
    ) -> Result<(), PolicyFailureReason> {
        let id = self
            .structures
            .structure_at(target)
            .ok_or(PolicyFailureReason::TargetUnavailable)?;
        let stone = crate::STONE_HUT_STONE;
        if self
            .population
            .inventory(agent)
            .is_none_or(|inventory| inventory.amount(Material::Stone) < stone)
        {
            return Err(PolicyFailureReason::InsufficientMaterials);
        }
        if !self.structures.wall_in_stone(id) {
            return Err(PolicyFailureReason::TargetUnavailable);
        }
        self.population.take(agent, Material::Stone, stone);
        self.practise(agent, crate::Activity::Building);
        self.stone_huts += 1;
        self.life_events.push(crate::LifeEvent::Walled { agent });
        let watchers = self.watchers_of(agent);
        for who in std::iter::once(agent).chain(watchers) {
            self.minds
                .get_mut(who)
                .crafts
                .learn_design(crate::cognition::Design::StoneWalls);
        }
        Ok(())
    }

    /// Whether `place` is a finished drying rack that dries now (not in winter).
    pub(super) fn drying_rack(&self, place: Option<crate::StructureView>) -> bool {
        place.is_some_and(|rack| {
            rack.kind == crate::StructureKind::Rack
                && rack.state == crate::StructureState::Complete
                && self.season() != crate::Season::Winter
        })
    }

    /// A pending recipe it knows is made in a batch; anything else is tried.
    fn try_or_make(
        &mut self,
        agent: AgentId,
        attempt: crate::cognition::Attempt,
    ) -> Result<(), PolicyFailureReason> {
        let known = attempt.recipe().filter(|(index, _)| {
            self.minds
                .get(agent)
                .is_some_and(|mind| mind.crafts.knows(*index))
        });
        let Some((index, recipe)) = known else {
            return self.try_attempt(agent, attempt);
        };
        let mut made = 0;
        while made < SMOKING_BATCH
            && self.population.inventory(agent).is_some_and(|inventory| {
                recipe
                    .inputs
                    .iter()
                    .all(|&(input, count)| inventory.amount(input) >= count)
            })
        {
            self.make(agent, index, recipe)?;
            made += 1;
        }
        if made > 0 {
            Ok(())
        } else {
            Err(PolicyFailureReason::InsufficientMaterials)
        }
    }

    /// Who is awake and close enough to see `agent` at work.
    pub(super) fn watchers_of(&self, agent: AgentId) -> Vec<AgentId> {
        self.perceive_physical(agent, crate::PHYSICAL_POLICY_RADIUS)
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
            .unwrap_or_default()
    }

    /// Berries eaten on open ground (no bush near) may drop a seed beside the
    /// eater, which comes up as a bush. The eater remembers where it ate.
    fn drop_seeds(&mut self, agent: AgentId, material: crate::Material) {
        let Some(bush) = bush_of(material) else {
            return;
        };
        let Some(position) = self.population.view(agent).map(|view| view.position) else {
            return;
        };
        let bush_near = (-SEED_CLEARANCE..=SEED_CLEARANCE).any(|dy| {
            (-SEED_CLEARANCE..=SEED_CLEARANCE).any(|dx| {
                let at = WorldPosition {
                    x: position.x + dx,
                    y: position.y + dy,
                };
                self.world
                    .feature_at(at)
                    .is_some_and(|feature| bush_of(feature.kind.base_resource().kind).is_some())
                    || self.spawned_objects.at(at).is_some_and(|object| {
                        object
                            .kind
                            .feature()
                            .is_some_and(|kind| bush_of(kind.base_resource().kind).is_some())
                    })
            })
        });
        if bush_near {
            return;
        }
        let now = (self.time.ticks() / 60) as u32;
        self.minds.get_mut(agent).sown.ate_at(position, now);
        let draw = crate::wildlife::mix(
            self.config.life_seed
                ^ 0x5345_4544_5345_4544
                ^ self.time.ticks()
                ^ u64::from(agent.get()) << 40,
        );
        // Seeds take only in spring.
        if draw % SEED_ODDS != 0 || self.season() != crate::Season::Spring {
            return;
        }
        let turn = (draw >> 8) as usize % 4;
        let spots = [(0, -1), (1, 0), (0, 1), (-1, 0)].map(|(dx, dy)| WorldPosition {
            x: position.x + dx,
            y: position.y + dy,
        });
        if let Some(spot) = (0..4)
            .map(|step| spots[(turn + step) % 4])
            .find(|&spot| self.can_sprout_at(spot))
            && let Some(sprout) = crate::SpawnKind::growing(bush)
        {
            self.spawned_objects.sow(spot, sprout, next_season(now));
            self.plants_sprouted += 1;
        }
    }

    /// Cooks the raw meat it carries over the burning fire at `target` (beside
    /// it). It and anyone watching expect cooked meat to feed as meat did.
    pub(super) fn apply_cook(
        &mut self,
        agent: AgentId,
        target: WorldPosition,
    ) -> Result<(), PolicyFailureReason> {
        let position = self
            .population
            .view(agent)
            .ok_or(PolicyFailureReason::InconsistentState)?
            .position;
        let now = (self.time.ticks() / 60) as u32;
        let place = self
            .structures
            .structure_at(target)
            .and_then(|id| self.structures.view(id));
        let burning = place.is_some_and(|fire| fire.kind.burns() && fire.working(now));
        let rack = self.drying_rack(place);
        if !(burning || rack)
            || position
                .x
                .abs_diff(target.x)
                .max(position.y.abs_diff(target.y))
                > 1
        {
            return Err(PolicyFailureReason::TargetUnavailable);
        }
        // Smoking or drying food it knows how to keep, or trying something
        // at the fire or the rack.
        let attempt = self.minds.get_mut(agent).crafts.take_pending();
        let fits = |method: crate::Method| {
            if method == crate::Method::Rack {
                rack
            } else {
                burning
            }
        };
        if attempt.is_some() && !fits(attempt.method()) {
            return Err(PolicyFailureReason::TargetUnavailable);
        }
        if !burning {
            // Nothing to roast on a rack.
            if attempt.is_some() {
                return self.try_or_make(agent, attempt);
            }
            return Err(PolicyFailureReason::TargetUnavailable);
        }
        if attempt.is_some() {
            return self.try_or_make(agent, attempt);
        }
        let cooked = self.population.cook(agent, Material::Meat, Material::Roast);
        if cooked == 0 {
            return Err(PolicyFailureReason::InsufficientMaterials);
        }
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
        for &who in std::iter::once(&agent).chain(&watchers) {
            self.minds
                .get_mut(who)
                .affordances
                .saw_cooked(Material::Meat, Material::Roast);
        }
        self.name_made(agent, crate::Concept::Material(Material::Roast), &watchers);
        self.craft_events.push(crate::CraftEvent {
            agent,
            made: Material::Roast,
            watchers: watchers.len() as u16,
        });
        Ok(())
    }

    /// Pushes a carried berry into the ground at `target` (beside the agent),
    /// where it comes up as a bush. Anyone watching sees how it's done.
    pub(super) fn apply_plant(
        &mut self,
        agent: AgentId,
        target: WorldPosition,
    ) -> Result<(), PolicyFailureReason> {
        if !self
            .minds
            .get(agent)
            .is_some_and(|mind| mind.crafts.knows_planting())
        {
            return Err(PolicyFailureReason::TargetUnavailable);
        }
        let position = self
            .population
            .view(agent)
            .ok_or(PolicyFailureReason::InconsistentState)?
            .position;
        if position.x.abs_diff(target.x) + position.y.abs_diff(target.y) != 1
            || !self.can_sprout_at(target)
        {
            return Err(PolicyFailureReason::TargetUnavailable);
        }
        let inventory = self
            .population
            .inventory(agent)
            .ok_or(PolicyFailureReason::InconsistentState)?;
        // It plants what it eats.
        let food = self.food_values(agent);
        let (seed, bush) = crate::Material::ALL
            .into_iter()
            .filter(|&material| food.is_food(material) && inventory.amount(material) > 0)
            .find_map(|material| Some((material, bush_of(material)?)))
            .ok_or(PolicyFailureReason::InsufficientMaterials)?;
        let sprout =
            crate::SpawnKind::growing(bush).ok_or(PolicyFailureReason::InconsistentState)?;
        if self.population.take(agent, seed, 1) < 1 {
            return Err(PolicyFailureReason::InconsistentState);
        }
        let now = (self.time.ticks() / 60) as u32;
        self.spawned_objects.sow(target, sprout, next_season(now));
        self.plants_sprouted += 1;
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
        for &watcher in &watchers {
            self.minds.get_mut(watcher).crafts.saw_planting();
        }
        self.farm_events.push(crate::FarmEvent {
            agent,
            at: target,
            planted: true,
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
        let retched = properties.toxicity > 0;
        self.drop_seeds(agent, material);
        let eater = self.minds.get_mut(agent);
        eater.model.used(
            crate::cognition::Choice::Eat,
            crate::cognition::Facts::HAS_FOOD,
        );
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
                                made.made_from()
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
        // The best edge it carries: an axe before a hand-held flake.
        let edge = Material::ALL
            .into_iter()
            .filter(|material| material.properties().cutting && inventory.amount(*material) > 0)
            .max_by_key(|material| material.properties().chop);
        let cuts = matches!(
            resource.kind.properties().handling,
            crate::Handling::Chop | crate::Handling::Carve
        );
        let edge = edge.filter(|_| cuts);
        // Carving, it takes the skin first if it has none to wear: easily with
        // an edge, and one time in a few tearing it off by hand.
        let skins = edge.is_some()
            || crate::wildlife::mix(
                self.config.life_seed
                    ^ 0x4849_4445
                    ^ self.time.ticks()
                    ^ u64::from(agent.get()) << 32,
            ) % HAND_SKINNING_ODDS
                == 0;
        if resource.kind == Material::Meat
            && skins
            && inventory.amount(Material::Hide) == 0
            && inventory.can_add(Material::Hide)
            && self.wildlife.skin(resource_position)
        {
            self.population.add_inventory(agent, Material::Hide, 1);
            self.dress_for_weather(agent);
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
            self.name_made(agent, crate::Concept::Material(Material::Hide), &watchers);
        }
        let inventory = self
            .population
            .inventory(agent)
            .ok_or(PolicyFailureReason::InconsistentState)?;
        // A practised forager picks more at a time; a practised builder
        // chops and breaks more.
        let activity = if self.food_values(agent).is_food(resource.kind) {
            crate::Activity::Foraging
        } else {
            crate::Activity::Building
        };
        let knack = u64::from(GATHER_YIELD) * 100
            / crate::cognition::by_ability(100, self.ability(agent, activity)).max(1);
        self.practise(agent, activity);
        let maximum = inventory
            .remaining_capacity(resource.kind)
            .min(knack as u8 * edge.map_or(1, |edge| edge.properties().chop.max(1)));
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
        if let Some(tool) = edge {
            self.wear(agent, tool);
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

/// Eating on open ground drops a seed that comes up with odds 1 in this.
const SEED_ODDS: u64 = 4;
/// No bush within this many cells counts as open ground.
const SEED_CLEARANCE: i64 = 2;

/// The bush whose fruit `material` is, if it is one.
fn bush_of(material: crate::Material) -> Option<crate::FeatureKind> {
    [
        crate::FeatureKind::BerryBush,
        crate::FeatureKind::BitterBush,
    ]
    .into_iter()
    .find(|bush| bush.base_resource().kind == material)
}

/// Without an edge, a hide comes off by hand with odds 1 in this per go at a carcass.
const HAND_SKINNING_ODDS: u64 = 3;

/// The simulated second the season after the one at `now` begins.
fn next_season(now: u32) -> u32 {
    let season = crate::SEASON_SECONDS as u32;
    (now / season + 1) * season
}
