//! Pregnancy, carried babies, and children taking their first steps.
//!
//! A baby is carried and nursed by its mother until `WEANING_AGE`; until then
//! it is a record on her, not yet a person in the world. At weaning it becomes
//! a child beside her who knows its parents and siblings and starts with no
//! words or lore.

use std::collections::BTreeMap;

use crate::life::{ADULT_AGE, Life, SECONDS_PER_YEAR, Sex, WEANING_AGE};
use crate::wildlife::mix;
use crate::{AgentActivity, AgentId, Engine, NeedKind, Personality, WorldPosition};

/// Women can conceive between `ADULT_AGE` and this age; men stay fertile longer.
const LAST_FERTILE_AGE: u32 = 45;
const LAST_FATHER_AGE: u32 = 65;
/// A pregnancy lasts about nine months.
const PREGNANCY_SECONDS: i32 = (SECONDS_PER_YEAR * 3 / 4) as i32;
/// After a child starts walking, its mother waits about this long before the next.
const BIRTH_GAP_SECONDS: i32 = SECONDS_PER_YEAR as i32;
/// One in this many decisions a fertile couple spends together brings a pregnancy.
const CONCEPTION_ODDS: u64 = 150;
/// How often pregnancies and babies are looked after.
pub(crate) const FAMILY_CHECK_TICKS: u64 = 600;
/// Extra hunger a nursing mother takes on per check.
const NURSING_HUNGER: u16 = 20;
/// Fewer adults than this, and newcomers may arrive.
const NEWCOMERS_BELOW_ADULTS: usize = 4;
/// At most one arrival in this many years.
const NEWCOMER_GAP_YEARS: u64 = 5;
/// Most a child's trait strays from the average of its parents'.
const TRAIT_VARIATION: i32 = 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Pregnancy {
    conceived: i32,
    father: AgentId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Baby {
    born: i32,
    sex: Sex,
    father: AgentId,
    personality: Personality,
}

/// Mothers' pregnancies and carried babies, and when each last weaned a child.
#[derive(Debug, Default)]
pub(crate) struct Families {
    pregnancies: BTreeMap<AgentId, Pregnancy>,
    babies: BTreeMap<AgentId, Baby>,
    last_weaned: BTreeMap<AgentId, i32>,
    /// The year newcomers last arrived.
    last_arrival: Option<u64>,
}

/// Something that happened in a family (latest tick, for logs and tools).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FamilyEvent {
    Conceived {
        mother: AgentId,
        father: AgentId,
    },
    Born {
        mother: AgentId,
        father: AgentId,
        sex: Sex,
    },
    /// A carried child took its first steps and is now a person of its own.
    Walking {
        mother: AgentId,
        child: AgentId,
    },
    /// A pregnancy or a carried baby was lost with its mother.
    Lost {
        mother: AgentId,
    },
    /// A couple from elsewhere arrived, bringing their own words and lore.
    Arrived {
        woman: AgentId,
        man: AgentId,
    },
}

/// What a mother carries, for views.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MotherhoodView {
    pub pregnant: bool,
    /// The carried baby's sex and age in years.
    pub baby: Option<(Sex, u32)>,
}

impl Engine {
    /// A couple together in view: she may conceive if both are of age, she is
    /// well fed and rested, and she carries no child and weaned none recently.
    pub(super) fn try_conceive(&mut self, mother: AgentId, father: AgentId) {
        let now = self.life_seconds() as i32;
        let (her, him) = (self.life_of(mother), self.life_of(father));
        if her.sex != Sex::Female || him.sex != Sex::Male {
            return;
        }
        let (her_age, his_age) = (self.age_of(mother), self.age_of(father));
        let families = &self.families;
        if !(ADULT_AGE..=LAST_FERTILE_AGE).contains(&her_age)
            || !(ADULT_AGE..=LAST_FATHER_AGE).contains(&his_age)
            || families.pregnancies.contains_key(&mother)
            || families.babies.contains_key(&mother)
            || families
                .last_weaned
                .get(&mother)
                .is_some_and(|&weaned| now - weaned < BIRTH_GAP_SECONDS)
        {
            return;
        }
        let Ok(needs) = self.population.needs_view(mother, self.time) else {
            return;
        };
        if needs.hunger.value * 2 > needs.hunger.threshold || needs.rest.threshold_reached {
            return;
        }
        let roll =
            mix(self.config.seed ^ 0x4255_5254 ^ u64::from(mother.get()) << 32 ^ self.time.ticks());
        if roll % CONCEPTION_ODDS != 0 {
            return;
        }
        self.families.pregnancies.insert(
            mother,
            Pregnancy {
                conceived: now,
                father,
            },
        );
        self.family_events
            .push(FamilyEvent::Conceived { mother, father });
    }

    /// Every `FAMILY_CHECK_TICKS`: births, nursing, weaning, and losses.
    pub(super) fn tend_families(&mut self) {
        if self.time.ticks() % FAMILY_CHECK_TICKS != 0 {
            return;
        }
        let now = self.life_seconds() as i32;
        let alive = |engine: &Engine, agent: AgentId| {
            engine
                .population
                .view(agent)
                .is_some_and(|view| view.activity != AgentActivity::Dead)
        };
        let pregnancies: Vec<(AgentId, Pregnancy)> = self
            .families
            .pregnancies
            .iter()
            .map(|(&mother, &pregnancy)| (mother, pregnancy))
            .collect();
        for (mother, pregnancy) in pregnancies {
            if !alive(self, mother) {
                self.families.pregnancies.remove(&mother);
                self.family_events.push(FamilyEvent::Lost { mother });
                continue;
            }
            if now - pregnancy.conceived < PREGNANCY_SECONDS {
                continue;
            }
            self.families.pregnancies.remove(&mother);
            let roll = mix(self.config.seed
                ^ 0x4249_5254
                ^ u64::from(mother.get()) << 32
                ^ self.time.ticks());
            let sex = if roll & 1 == 0 {
                Sex::Female
            } else {
                Sex::Male
            };
            let personality = blend(
                self.innate_personality(mother),
                self.innate_personality(pregnancy.father),
                roll >> 1,
            );
            self.families.babies.insert(
                mother,
                Baby {
                    born: now,
                    sex,
                    father: pregnancy.father,
                    personality,
                },
            );
            self.family_events.push(FamilyEvent::Born {
                mother,
                father: pregnancy.father,
                sex,
            });
        }

        let babies: Vec<(AgentId, Baby)> = self
            .families
            .babies
            .iter()
            .map(|(&mother, &baby)| (mother, baby))
            .collect();
        for (mother, baby) in babies {
            if !alive(self, mother) {
                self.families.babies.remove(&mother);
                self.family_events.push(FamilyEvent::Lost { mother });
                continue;
            }
            let _ = self.population.worsen_need(
                &mut self.scheduler,
                self.time,
                mother,
                NeedKind::Hunger,
                NURSING_HUNGER,
            );
            if (now - baby.born) / SECONDS_PER_YEAR as i32 >= WEANING_AGE as i32
                && let Some(child) = self.wean(mother, baby)
            {
                self.families.babies.remove(&mother);
                self.families.last_weaned.insert(mother, now);
                self.family_events
                    .push(FamilyEvent::Walking { mother, child });
            }
        }
    }

    /// The carried child becomes a person beside its mother. `None` if there's
    /// no free cell next to her (it tries again at the next check).
    fn wean(&mut self, mother: AgentId, baby: Baby) -> Option<AgentId> {
        let at = self.population.view(mother)?.position;
        let child = [
            (1, 0),
            (-1, 0),
            (0, 1),
            (0, -1),
            (1, 1),
            (-1, -1),
            (1, -1),
            (-1, 1),
        ]
        .into_iter()
        .map(|(dx, dy)| WorldPosition {
            x: at.x + dx,
            y: at.y + dy,
        })
        .find_map(|cell| self.spawn_agent(cell).ok())?;
        // Born into the band: no inherited words or lore.
        if self.minds.is_founder(child) {
            self.minds.set_founders(child.get());
        }
        let index = child.get() as usize;
        if self.lives.len() <= index {
            self.lives.resize(index + 1, None);
        }
        self.lives[index] = Some(Life {
            born: baby.born,
            sex: baby.sex,
            inherited: Some(baby.personality),
        });
        // Bonded to the father first, so the mother is the parent it follows.
        if self
            .population
            .view(baby.father)
            .is_some_and(|view| view.activity != AgentActivity::Dead)
        {
            self.bond(child, baby.father);
        }
        self.bond(child, mother);
        Some(child)
    }

    /// Once a year: if the band has dwindled, a couple from elsewhere wanders
    /// in at the edge of the area, as partners with their own family's lore.
    pub(super) fn welcome_newcomers(&mut self, year: u64) {
        let adults = self
            .population
            .views(usize::MAX)
            .filter(|view| {
                view.activity != AgentActivity::Dead && self.age_of(view.id) >= ADULT_AGE
            })
            .count();
        if adults >= NEWCOMERS_BELOW_ADULTS
            || self
                .families
                .last_arrival
                .is_some_and(|last| year < last + NEWCOMER_GAP_YEARS)
        {
            return;
        }
        let Some(area) = self.population.active_area() else {
            return;
        };
        for attempt in 0..64_u64 {
            let roll = mix(self.config.seed ^ 0x4e45_5743 ^ year << 8 ^ attempt);
            let along = (roll >> 8) as i64;
            let width = (area.max.x - area.min.x).max(4);
            let height = (area.max.y - area.min.y).max(4);
            let cell = match roll % 4 {
                0 => WorldPosition {
                    x: area.min.x + 1 + along % (width - 2),
                    y: area.min.y + 1,
                },
                1 => WorldPosition {
                    x: area.min.x + 1 + along % (width - 2),
                    y: area.max.y - 2,
                },
                2 => WorldPosition {
                    x: area.min.x + 1,
                    y: area.min.y + 1 + along % (height - 2),
                },
                _ => WorldPosition {
                    x: area.max.x - 2,
                    y: area.min.y + 1 + along % (height - 2),
                },
            };
            let standable = |engine: &Engine, at: WorldPosition| {
                engine.physical_standability_at(at) == Ok(crate::Standability::Standable)
            };
            let Some(beside) = [(1, 0), (-1, 0), (0, 1), (0, -1)]
                .into_iter()
                .map(|(dx, dy)| WorldPosition {
                    x: cell.x + dx,
                    y: cell.y + dy,
                })
                .find(|&beside| standable(self, cell) && standable(self, beside))
            else {
                continue;
            };
            let Ok(woman) = self.spawn_agent(cell) else {
                continue;
            };
            let Ok(man) = self.spawn_agent(beside) else {
                // She came alone after all.
                self.arrive_alone(woman);
                return;
            };
            self.arrive_as_couple(woman, man);
            self.families.last_arrival = Some(year);
            self.family_events.push(FamilyEvent::Arrived { woman, man });
            return;
        }
    }

    fn arrive_alone(&mut self, woman: AgentId) {
        self.minds.add_newcomer(woman);
        let index = woman.get() as usize;
        if self.lives.len() <= index {
            self.lives.resize(index + 1, None);
        }
        let starting = Life::starting(self.config.seed, woman, false);
        self.lives[index] = Some(Life {
            sex: Sex::Female,
            ..starting
        });
    }

    fn arrive_as_couple(&mut self, woman: AgentId, man: AgentId) {
        let now = crate::cognition::belief_seconds(self.time);
        for (agent, sex) in [(woman, Sex::Female), (man, Sex::Male)] {
            self.minds.add_newcomer(agent);
            let index = agent.get() as usize;
            if self.lives.len() <= index {
                self.lives.resize(index + 1, None);
            }
            let starting = Life::starting(self.config.seed, agent, false);
            self.lives[index] = Some(Life { sex, ..starting });
        }
        for (from, to) in [(woman, man), (man, woman)] {
            let Some(at) = self.population.view(to).map(|view| view.position) else {
                continue;
            };
            let social = &mut self.minds.get_mut(from).social;
            if let Some(slot) = social.bond(to, at, now) {
                social.set_tie(slot, crate::Tie::Partner);
            }
        }
    }

    /// Family happenings from the latest tick (for logs and tools).
    pub fn family_events(&self) -> &[FamilyEvent] {
        &self.family_events
    }

    /// Whether `agent` is pregnant or carrying a baby.
    pub fn motherhood(&self, agent: AgentId) -> Option<MotherhoodView> {
        let pregnant = self.families.pregnancies.contains_key(&agent);
        let baby = self.families.babies.get(&agent).map(|baby| {
            let age = (self.life_seconds() as i32 - baby.born) / SECONDS_PER_YEAR as i32;
            (baby.sex, age.max(0) as u32)
        });
        (pregnant || baby.is_some()).then_some(MotherhoodView { pregnant, baby })
    }
}

/// Each trait is the average of the parents' plus up to `TRAIT_VARIATION` either way.
fn blend(mother: Personality, father: Personality, roll: u64) -> Personality {
    let trait_of = |a: u8, b: u8, index: u32| {
        let spread = (2 * TRAIT_VARIATION + 1) as u64;
        let variation = ((roll >> (index * 8)) % spread) as i32 - TRAIT_VARIATION;
        ((i32::from(a) + i32::from(b)) / 2 + variation).clamp(0, 255) as u8
    };
    Personality {
        curiosity: trait_of(mother.curiosity, father.curiosity, 0),
        caution: trait_of(mother.caution, father.caution, 1),
        sociability: trait_of(mother.sociability, father.sociability, 2),
        diligence: trait_of(mother.diligence, father.diligence, 3),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn children_take_after_both_parents() {
        let low = Personality {
            curiosity: 40,
            caution: 40,
            sociability: 40,
            diligence: 40,
        };
        let high = Personality {
            curiosity: 220,
            caution: 220,
            sociability: 220,
            diligence: 220,
        };
        for roll in 0..50 {
            let child = blend(low, high, mix(roll));
            for value in [
                child.curiosity,
                child.caution,
                child.sociability,
                child.diligence,
            ] {
                assert!((100..=160).contains(&value), "{value}");
            }
        }
    }
}
