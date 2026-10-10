//! Engine side of wildlife: releasing animals, stepping them on their own
//! schedules, bites that wound people, and people hunting animals.

use crate::wildlife::{
    Animal, AnimalMode, AnimalView, Move, Senses, Species, WILDLIFE_TICKS, Wildlife, WildlifeEvent,
    chebyshev, decide, mix,
};
use crate::{
    AgentActivity, AgentId, Engine, HealthDiagnosticKind, PolicyFailureReason, Standability,
    WorldPosition, WorldRect,
};

/// Ticks a strike at an animal takes.
pub const HUNT_TICKS: u64 = 12;
/// A strike (a thrown spear or stone) reaches this far (Chebyshev cells).
pub const STRIKE_RANGE: i64 = 2;
/// Chance out of 100 that a lone hunter's strike lands; each other person near
/// the animal adds `HELPER_CHANCE` (hunting goes better together).
const STRIKE_CHANCE: u64 = 45;
const HELPER_CHANCE: u64 = 20;
/// Extra chance (out of 100) a strike lands with a cutting edge in hand.
const EDGE_CHANCE: u64 = 15;
/// One kill in this many breaks the blade used.
const BLADE_BREAK_ODDS: u64 = 6;
/// Each wound slows an animal's hurried step by this many ticks.
const WOUND_SLOWDOWN_TICKS: u64 = 5;
/// A thinned herd still breeds as if it had this many animals (stragglers
/// from beyond the area join it).
const MIN_BREEDERS: usize = 8;
/// How long someone knocked down by a wound lies before coming round (5 minutes).
pub(super) const WOUND_RECOVERY_TICKS: u64 = 18_000;

impl Engine {
    /// Scenario setup: releases `deer` deer in three herds and `wolves` wolves at
    /// spots chosen from the seed inside `area`, which also bounds where they
    /// roam. Returns how many animals were placed.
    pub fn release_wildlife(&mut self, area: WorldRect, deer: u16, wolves: u16) -> usize {
        self.wildlife = Wildlife {
            area: Some(area),
            next_birth: Species::ALL
                .map(|species| species.traits().birth_ticks / MIN_BREEDERS as u64),
            ..Wildlife::default()
        };
        let seed = self.config.seed;
        let herds = 3_u16;
        let mut placed = 0;
        let mut spot = 0_u64;
        let mut groups: Vec<(Species, u16)> = (0..herds)
            .map(|herd| (Species::Deer, deer / herds + u16::from(herd < deer % herds)))
            .collect();
        groups.extend((0..wolves).map(|_| (Species::Wolf, 1)));
        for (species, count) in groups {
            // A center somewhere standable in the area, then members around it.
            let center = loop {
                spot += 1;
                let roll = mix(seed ^ 0x5749_4c44 ^ spot.wrapping_mul(0x9e37_79b9));
                let width = (area.max.x - area.min.x).max(1) as u64;
                let height = (area.max.y - area.min.y).max(1) as u64;
                let candidate = WorldPosition {
                    x: area.min.x + (roll % width) as i64,
                    y: area.min.y + ((roll >> 32) % height) as i64,
                };
                if self.animal_can_stand(candidate) || spot > 10_000 {
                    break candidate;
                }
            };
            let mut ring = 0_i64;
            let mut members = 0;
            while members < count && ring < 12 {
                for dy in -ring..=ring {
                    for dx in -ring..=ring {
                        if members >= count || dx.abs().max(dy.abs()) != ring {
                            continue;
                        }
                        let cell = WorldPosition {
                            x: center.x + dx * 2,
                            y: center.y + dy * 2,
                        };
                        if self.animal_can_stand(cell) && self.spawn_animal(species, cell) {
                            members += 1;
                            placed += 1;
                        }
                    }
                }
                ring += 1;
            }
        }
        placed
    }

    /// Living animals, in id order.
    pub fn animal_views(&self) -> impl Iterator<Item = AnimalView> + '_ {
        self.wildlife.views()
    }

    /// Carcasses with meat left: where they lie and how much meat.
    pub fn carcass_views(&self) -> impl Iterator<Item = (WorldPosition, u8)> + '_ {
        self.wildlife
            .carcasses
            .iter()
            .map(|carcass| (carcass.position(), carcass.meat))
    }

    /// Living animals of `species`.
    pub fn animal_count(&self, species: Species) -> usize {
        self.wildlife.living(species)
    }

    /// What animals did and suffered during the latest tick.
    pub fn wildlife_events(&self) -> &[WildlifeEvent] {
        &self.wildlife_events
    }

    pub(super) fn spawn_animal(&mut self, species: Species, position: WorldPosition) -> bool {
        let (Ok(x), Ok(y)) = (i16::try_from(position.x), i16::try_from(position.y)) else {
            return false;
        };
        let now = self.time.ticks();
        let id = self.wildlife.animals.len() as u64;
        self.wildlife.animals.push(Animal {
            // Spread first moves out so the herd doesn't step in lockstep.
            next_act: now + mix(self.config.seed ^ id) % species.traits().calm_step_ticks,
            fed_until: 0,
            x,
            y,
            species,
            mode: AnimalMode::Grazing,
            wounds: 0,
            alive: true,
        });
        true
    }

    pub(super) fn animal_can_stand(&self, position: WorldPosition) -> bool {
        self.wildlife
            .area
            .is_some_and(|area| area.contains(position))
            && self.world.standability_at(position) == Ok(Standability::Standable)
            && self.structures.structure_at(position).is_none()
            && self.population.spatial().occupant(position).is_none()
    }

    /// Lets every animal that is due act, handles births, and rots carcasses.
    pub(super) fn step_wildlife(&mut self) {
        let now = self.time.ticks();
        if self.wildlife.area.is_none() || now % WILDLIFE_TICKS != 0 {
            return;
        }
        let mut nearby_people = Vec::new();
        for index in 0..self.wildlife.animals.len() {
            let animal = self.wildlife.animals[index];
            if !animal.alive || animal.next_act > now {
                continue;
            }
            let traits = animal.species.traits();
            let here = animal.position();
            let reach = traits.senses;
            let sensed = WorldRect {
                min: WorldPosition {
                    x: here.x - reach,
                    y: here.y - reach,
                },
                max: WorldPosition {
                    x: here.x + reach + 1,
                    y: here.y + reach + 1,
                },
            };
            nearby_people.clear();
            self.population
                .spatial()
                .agents_in(sensed, &mut nearby_people);
            let people: Vec<(AgentId, WorldPosition)> = nearby_people
                .iter()
                .filter_map(|&id| self.population.view(id))
                .filter(|view| !view.activity.is_terminal())
                .map(|view| (view.id, view.position))
                .collect();
            let animals: Vec<(usize, Species, WorldPosition)> = self
                .wildlife
                .animals
                .iter()
                .enumerate()
                .filter(|(_, other)| {
                    other.alive && chebyshev(here, other.position()) <= reach.max(traits.scent)
                })
                .map(|(other, animal)| (other, animal.species, animal.position()))
                .collect();
            let senses = Senses {
                people: &people,
                animals: &animals,
                now,
                seed: self.config.seed,
            };
            let choice = decide(index, animal, &senses, &|cell| self.animal_can_stand(cell));
            self.apply_animal_move(index, choice);
        }
        self.breed_wildlife();
        self.wildlife.rot(now);
    }

    fn apply_animal_move(&mut self, index: usize, choice: Move) {
        let now = self.time.ticks();
        let animal = self.wildlife.animals[index];
        let traits = animal.species.traits();
        let hurried =
            now + traits.hurried_step_ticks + u64::from(animal.wounds) * WOUND_SLOWDOWN_TICKS;
        let calm = now + traits.calm_step_ticks;
        match choice {
            Move::Step(cell, mode) => {
                let animal = &mut self.wildlife.animals[index];
                animal.x = cell.x as i16;
                animal.y = cell.y as i16;
                animal.mode = mode;
                animal.next_act = if mode == AnimalMode::Grazing {
                    calm
                } else {
                    hurried
                };
            }
            Move::Stay(mode) => {
                let animal = &mut self.wildlife.animals[index];
                animal.mode = mode;
                animal.next_act = if mode == AnimalMode::Fleeing {
                    hurried
                } else {
                    calm
                };
            }
            Move::BitePerson(agent) => {
                let position = animal.position();
                // One bite, then it loses interest for a while and wanders off.
                self.wildlife.animals[index].mode = AnimalMode::Resting;
                self.wildlife.animals[index].fed_until = now + Wildlife::bite_recovery();
                self.wildlife.animals[index].next_act = hurried;
                self.wound(agent, traits.bite);
                self.witness(WildlifeEvent::Bite {
                    animal: index as u32,
                    species: animal.species,
                    agent,
                    damage: traits.bite,
                    position,
                });
            }
            Move::AttackAnimal(target) => {
                self.wildlife.animals[index].mode = AnimalMode::Hunting;
                self.wildlife.animals[index].next_act = hurried;
                let prey = &mut self.wildlife.animals[target];
                prey.wounds = prey.wounds.saturating_add(1);
                prey.next_act = now;
                if prey.wounds >= prey.species.traits().toughness {
                    prey.alive = false;
                    let (species, position) = (prey.species, prey.position());
                    // The predator eats its fill and leaves the rest.
                    let meat = species.traits().meat.saturating_sub(4);
                    self.wildlife.leave_carcass(position, meat, now);
                    self.wildlife.animals[index].fed_until = now + Wildlife::fed_ticks();
                    self.witness(WildlifeEvent::Killed {
                        animal: target as u32,
                        species,
                        by: index as u32,
                        position,
                    });
                }
            }
        }
    }

    fn breed_wildlife(&mut self) {
        let now = self.time.ticks();
        for species in Species::ALL {
            let slot = species as usize;
            if now < self.wildlife.next_birth[slot] {
                continue;
            }
            let traits = species.traits();
            let living: Vec<usize> = self
                .wildlife
                .animals
                .iter()
                .enumerate()
                .filter(|(_, animal)| animal.alive && animal.species == species)
                .map(|(index, _)| index)
                .collect();
            // Every animal breeds, so a bigger herd recovers faster; a few
            // stragglers from beyond the area keep a thinned herd going.
            let breeders = living.len().max(MIN_BREEDERS) as u64;
            self.wildlife.next_birth[slot] = now + traits.birth_ticks / breeders;
            if living.len() < 2 {
                // Nearly gone: a newcomer wanders in from the edge of the area.
                self.immigrate(species, now);
                continue;
            }
            // Young are born outside winter.
            if living.len() >= usize::from(traits.max_population)
                || self.season() == crate::Season::Winter
            {
                continue;
            }
            let parent = living[(mix(self.config.seed ^ now) % living.len() as u64) as usize];
            let at = self.wildlife.animals[parent].position();
            if let Some(cell) = crate::wildlife::cardinal(at)
                .into_iter()
                .find(|cell| self.animal_can_stand(*cell))
                && self.spawn_animal(species, cell)
            {
                self.wildlife_events.push(WildlifeEvent::Born {
                    animal: (self.wildlife.animals.len() - 1) as u32,
                    species,
                    position: cell,
                });
            }
        }
    }

    /// Records what happened and lets everyone who saw it learn from it: the
    /// bitten agent knows the species bites, onlookers saw it happen, and anyone
    /// who saw a person bring an animal down learns it can be hunted.
    fn witness(&mut self, event: WildlifeEvent) {
        self.wildlife_events.push(event);
        if !self.policy_options.memory {
            return;
        }
        let (species, position, actor) = match event {
            WildlifeEvent::Bite {
                species,
                agent,
                position,
                ..
            } => {
                self.minds.get_mut(agent).fauna.bitten(species);
                (species, position, Some(agent))
            }
            WildlifeEvent::Struck {
                species,
                hunter,
                killed: true,
                position,
                ..
            } => {
                self.minds.get_mut(hunter).fauna.saw_hunted(species);
                (species, position, Some(hunter))
            }
            _ => return,
        };
        let mut onlookers = Vec::new();
        let reach = i64::from(crate::PHYSICAL_POLICY_RADIUS);
        self.population.spatial().agents_in(
            WorldRect {
                min: WorldPosition {
                    x: position.x - reach,
                    y: position.y - reach,
                },
                max: WorldPosition {
                    x: position.x + reach + 1,
                    y: position.y + reach + 1,
                },
            },
            &mut onlookers,
        );
        for onlooker in onlookers {
            let awake = self
                .population
                .view(onlooker)
                .is_some_and(|view| super::cognition::can_watch(view.activity));
            if Some(onlooker) == actor || !awake {
                continue;
            }
            let fauna = &mut self.minds.get_mut(onlooker).fauna;
            if matches!(event, WildlifeEvent::Bite { .. }) {
                fauna.heard_of_danger(species);
            } else {
                fauna.saw_hunted(species);
            }
        }
    }

    /// An animal of `species` arrives at a standable cell on the edge of the area.
    fn immigrate(&mut self, species: Species, now: u64) {
        let Some(area) = self.wildlife.area else {
            return;
        };
        for attempt in 0..64_u64 {
            let roll = mix(self.config.seed ^ now ^ attempt.wrapping_mul(0x2545_f491));
            let along = (roll >> 8) as i64;
            let width = (area.max.x - area.min.x).max(1);
            let height = (area.max.y - area.min.y).max(1);
            let cell = match roll % 4 {
                0 => WorldPosition {
                    x: area.min.x + along % width,
                    y: area.min.y + 1,
                },
                1 => WorldPosition {
                    x: area.min.x + along % width,
                    y: area.max.y - 2,
                },
                2 => WorldPosition {
                    x: area.min.x + 1,
                    y: area.min.y + along % height,
                },
                _ => WorldPosition {
                    x: area.max.x - 2,
                    y: area.min.y + along % height,
                },
            };
            if self.animal_can_stand(cell) && self.spawn_animal(species, cell) {
                self.wildlife_events.push(WildlifeEvent::Born {
                    animal: (self.wildlife.animals.len() - 1) as u32,
                    species,
                    position: cell,
                });
                return;
            }
        }
    }

    /// People knocked down by a wound come round once their time is up, weak but
    /// on their feet, unless their needs have taken over meanwhile.
    pub(super) fn revive_due(&mut self) {
        while let Some(&(due, agent)) = self.recovering.front() {
            if due > self.time {
                break;
            }
            self.recovering.pop_front();
            if let Some((before, after)) =
                self.population
                    .revive(&mut self.scheduler, self.time, agent)
            {
                self.health_diagnostics.push(crate::HealthDiagnostic {
                    agent,
                    at: self.time,
                    cause: None,
                    before,
                    after,
                    kind: HealthDiagnosticKind::Recovered,
                });
            }
        }
    }

    /// A wound from a bite or a fight: health falls at once, and the agent may
    /// be incapacitated or die of it.
    pub(super) fn wound(&mut self, agent: AgentId, amount: u16) {
        let Some(outcome) = self.population.injure(agent, amount, self.time) else {
            return;
        };
        match outcome.kind {
            HealthDiagnosticKind::Incapacitated => {
                self.cancel_construction(agent);
                self.population.incapacitate(self.time, agent);
                if let Some(due) = self.time.checked_add(WOUND_RECOVERY_TICKS) {
                    self.recovering.push_back((due, agent));
                }
            }
            HealthDiagnosticKind::Died => {
                self.cancel_construction(agent);
                if let Some(record) =
                    self.population
                        .finalize_death(self.time, agent, crate::DeathCause::Injury)
                {
                    self.death_records.push(record);
                }
            }
            HealthDiagnosticKind::Deteriorated => {
                // The pain wakes a sleeper and makes anyone stop and react.
                let sleeping = self.population.sleep_view(agent);
                let woke = match self.population.interrupt_for_policy_decision(
                    &mut self.scheduler,
                    self.time,
                    agent,
                    self.policy_active,
                ) {
                    Ok((_, interrupted)) => interrupted.is_some(),
                    Err(_) => {
                        sleeping.is_some()
                            && self
                                .population
                                .force_interrupt_sleep(self.time, agent)
                                .is_some()
                    }
                };
                if let Some(sleep) = sleeping
                    && woke
                {
                    self.sleep_diagnostics.push(crate::SleepDiagnostic {
                        sleep,
                        at: self.time,
                        kind: crate::SleepDiagnosticKind::Interrupted,
                        interruption: Some(crate::SleepInterruptionReason::Injury),
                    });
                }
            }
            HealthDiagnosticKind::StaleEvent | HealthDiagnosticKind::Recovered => {}
        }
        self.health_diagnostics.push(outcome);
    }

    /// A person strikes at an animal next to it (completing a `Hunt` action).
    /// Others standing next to the animal make the strike likelier to land.
    /// Animals that bite fight back.
    pub(super) fn apply_hunt(
        &mut self,
        hunter: AgentId,
        target: WorldPosition,
    ) -> Result<(), PolicyFailureReason> {
        let position = self
            .population
            .view(hunter)
            .ok_or(PolicyFailureReason::InconsistentState)?
            .position;
        let Some(index) = self
            .wildlife
            .animals
            .iter()
            .enumerate()
            .filter(|(_, animal)| {
                animal.alive && chebyshev(position, animal.position()) <= STRIKE_RANGE
            })
            .min_by_key(|(index, animal)| (chebyshev(target, animal.position()), *index))
            .map(|(index, _)| index)
        else {
            return Err(PolicyFailureReason::TargetUnavailable);
        };
        let animal = self.wildlife.animals[index];
        let at = animal.position();
        let helpers = self
            .population
            .views(usize::MAX)
            .filter(|view| {
                view.id != hunter
                    && !view.activity.is_terminal()
                    && view.activity != AgentActivity::Sleeping
                    && chebyshev(view.position, at) <= STRIKE_RANGE
            })
            .count() as u64;
        // A cutting edge makes a strike tell.
        let edge = self.population.inventory(hunter).and_then(|inventory| {
            crate::Material::ALL
                .into_iter()
                .find(|material| material.properties().cutting && inventory.amount(*material) > 0)
        });
        let chance =
            (STRIKE_CHANCE + HELPER_CHANCE * helpers + u64::from(edge.is_some()) * EDGE_CHANCE)
                .saturating_add_signed(self.strength(hunter))
                .clamp(5, 95);
        let roll = mix(self.config.seed
            ^ 0x4855_4e54
            ^ (u64::from(hunter.get()) << 40)
            ^ self.time.ticks());
        let landed = roll % 100 < chance;
        let traits = animal.species.traits();
        let mut killed = false;
        let now = self.time.ticks();
        {
            let struck = &mut self.wildlife.animals[index];
            struck.next_act = now;
            if landed {
                struck.wounds = struck.wounds.saturating_add(1);
                if struck.wounds >= traits.toughness {
                    struck.alive = false;
                    killed = true;
                }
            }
        }
        if killed {
            self.wildlife.leave_carcass(at, traits.meat, now);
            // Edges chip: now and then the blade breaks.
            if let Some(blade) = edge
                && (roll >> 16) % BLADE_BREAK_ODDS == 0
            {
                self.population.take(hunter, blade, 1);
            }
        } else if traits.bite > 0 {
            // A cornered predator bites back.
            self.wound(hunter, traits.bite);
            self.witness(WildlifeEvent::Bite {
                animal: index as u32,
                species: animal.species,
                agent: hunter,
                damage: traits.bite,
                position: at,
            });
        }
        self.witness(WildlifeEvent::Struck {
            animal: index as u32,
            species: animal.species,
            hunter,
            helpers: helpers as u8,
            killed,
            position: at,
        });
        Ok(())
    }
}
