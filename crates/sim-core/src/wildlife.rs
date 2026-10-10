//! Wildlife (plan L3): cheap creatures without minds. Each species is a row of
//! traits (how far it senses, how fast it moves, whether it runs from people,
//! whether and how hard it bites, what it hunts, how much meat it leaves), and
//! one shared rule set turns traits into behavior. Animals act on their own
//! schedule (`next_act`), so nothing scans them every tick, and every random
//! choice is a hash of the seed, the animal, and the time.

use crate::{AgentId, Material, WorldPosition, WorldRect};

/// Deer and wolves released into the vertical-slice valley.
pub const VALLEY_DEER: u16 = 24;
pub const VALLEY_WOLVES: u16 = 2;
/// Wildlife is checked every this many ticks; animals act when they're due.
pub const WILDLIFE_TICKS: u64 = 3;
/// Carcasses spoil after this many ticks (meat that's carried keeps).
pub const CARCASS_TICKS: u64 = 60 * 60 * 20;
/// A wolf that just ate leaves deer alone this long.
const FED_TICKS: u64 = 60 * 60 * 30;
/// A biting wolf backs off this long before it can bite again (a bite is a
/// warning more than a hunt).
const BITE_RECOVERY_TICKS: u64 = 60 * 120;
/// People this close together scare wolves off.
const CROWD_RADIUS: i64 = 4;
/// This many people within `CROWD_RADIUS` count as a crowd.
const CROWD_SIZE: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum Species {
    Deer,
    Wolf,
}

/// What a species is like. Behavior depends only on these.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpeciesTraits {
    /// How far it notices people and prey (Chebyshev cells).
    pub senses: i64,
    /// Ticks per step while calm and while in a hurry.
    pub calm_step_ticks: u64,
    pub hurried_step_ticks: u64,
    /// Runs from people who come near.
    pub shy: bool,
    /// Health a bite takes from a person (0 = harmless).
    pub bite: u16,
    /// Successful hits it takes to bring one down.
    pub toughness: u8,
    /// Meat a kill leaves.
    pub meat: u8,
    /// What it hunts, if anything.
    pub preys_on: Option<Species>,
    /// It never breeds past this many.
    pub max_population: u16,
    /// Ticks between births while below the cap.
    pub birth_ticks: u64,
}

impl Species {
    pub const COUNT: usize = 2;
    pub const ALL: [Self; Self::COUNT] = [Self::Deer, Self::Wolf];

    pub const fn traits(self) -> SpeciesTraits {
        match self {
            Self::Deer => SpeciesTraits {
                senses: 4,
                calm_step_ticks: 150,
                hurried_step_ticks: 13,
                shy: true,
                bite: 0,
                toughness: 2,
                meat: 16,
                preys_on: None,
                max_population: 40,
                birth_ticks: 60 * 60 * 15,
            },
            Self::Wolf => SpeciesTraits {
                senses: 10,
                calm_step_ticks: 90,
                hurried_step_ticks: 9,
                shy: false,
                bite: 1_500,
                toughness: 4,
                meat: 6,
                preys_on: Some(Self::Deer),
                max_population: 4,
                birth_ticks: 60 * 60 * 120,
            },
        }
    }
}

/// What an animal is doing, as anyone watching can see.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum AnimalMode {
    Grazing,
    Fleeing,
    Hunting,
    Resting,
}

/// A read-only animal, for perception, tools, and the viewer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnimalView {
    pub id: u32,
    pub species: Species,
    pub position: WorldPosition,
    pub mode: AnimalMode,
    pub wounds: u8,
}

/// Something that happened to or because of an animal (latest tick, for logs,
/// tools, and what watchers see).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WildlifeEvent {
    /// An animal bit a person.
    Bite {
        animal: u32,
        species: Species,
        agent: AgentId,
        damage: u16,
        position: WorldPosition,
    },
    /// A person struck an animal (`killed` if it went down).
    Struck {
        animal: u32,
        species: Species,
        hunter: AgentId,
        helpers: u8,
        killed: bool,
        position: WorldPosition,
    },
    /// A predator brought down prey.
    Killed {
        animal: u32,
        species: Species,
        by: u32,
        position: WorldPosition,
    },
    Born {
        animal: u32,
        species: Species,
        position: WorldPosition,
    },
}

/// One animal. 24 bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub(crate) struct Animal {
    pub(crate) next_act: u64,
    pub(crate) fed_until: u64,
    pub(crate) x: i16,
    pub(crate) y: i16,
    pub(crate) species: Species,
    pub(crate) mode: AnimalMode,
    pub(crate) wounds: u8,
    pub(crate) alive: bool,
}

impl Animal {
    pub(crate) fn position(self) -> WorldPosition {
        WorldPosition {
            x: i64::from(self.x),
            y: i64::from(self.y),
        }
    }
}

/// Meat lying where an animal fell. Spoils after `CARCASS_TICKS`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub(crate) struct Carcass {
    pub(crate) spoils_at: u64,
    pub(crate) x: i16,
    pub(crate) y: i16,
    pub(crate) meat: u8,
}

impl Carcass {
    pub(crate) fn position(self) -> WorldPosition {
        WorldPosition {
            x: i64::from(self.x),
            y: i64::from(self.y),
        }
    }
}

/// All animals and carcasses. Dead animals keep their slot (ids stay stable).
#[derive(Debug, Default)]
pub(crate) struct Wildlife {
    pub(crate) animals: Vec<Animal>,
    pub(crate) carcasses: Vec<Carcass>,
    /// Where animals may roam (none = no wildlife).
    pub(crate) area: Option<WorldRect>,
    /// Next tick each species may breed.
    pub(crate) next_birth: [u64; Species::COUNT],
}

pub(crate) fn mix(mut key: u64) -> u64 {
    key = (key ^ (key >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    key = (key ^ (key >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    key ^ (key >> 31)
}

pub(crate) fn chebyshev(left: WorldPosition, right: WorldPosition) -> i64 {
    (left.x - right.x).abs().max((left.y - right.y).abs())
}

pub(crate) const fn cardinal(position: WorldPosition) -> [WorldPosition; 4] {
    [
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
    ]
}

/// What an animal decides to do this turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Move {
    /// Step to this cell (already checked standable).
    Step(WorldPosition, AnimalMode),
    /// Attack the person or animal next to it.
    BitePerson(AgentId),
    AttackAnimal(usize),
    Stay(AnimalMode),
}

/// What an animal senses around it, gathered by the engine.
pub(crate) struct Senses<'a> {
    pub(crate) people: &'a [(AgentId, WorldPosition)],
    /// Living animals nearby: (index, species, position).
    pub(crate) animals: &'a [(usize, Species, WorldPosition)],
    pub(crate) now: u64,
    pub(crate) seed: u64,
}

/// The shared rule set. `standable` says whether a cell can be stepped on.
pub(crate) fn decide(
    index: usize,
    animal: Animal,
    senses: &Senses<'_>,
    standable: &dyn Fn(WorldPosition) -> bool,
) -> Move {
    let traits = animal.species.traits();
    let here = animal.position();
    let near = |position: WorldPosition, radius: i64| chebyshev(here, position) <= radius;
    let nearest_person = senses
        .people
        .iter()
        .filter(|(_, position)| near(*position, traits.senses))
        .min_by_key(|(id, position)| (chebyshev(here, *position), id.get()))
        .copied();
    let crowd = |position: WorldPosition| {
        senses
            .people
            .iter()
            .filter(|(_, other)| chebyshev(position, *other) <= CROWD_RADIUS)
            .count()
            >= CROWD_SIZE
    };
    let roll = mix(senses.seed ^ (index as u64).wrapping_mul(0x9e37_79b9) ^ senses.now);

    // Shy animals run from people; predators run from crowds.
    let threat = nearest_person.filter(|(_, position)| {
        (traits.shy && near(*position, traits.senses)) || (!traits.shy && crowd(*position))
    });
    if let Some((_, from)) = threat {
        return step_away(here, from, standable).map_or(Move::Stay(AnimalMode::Fleeing), |cell| {
            Move::Step(cell, AnimalMode::Fleeing)
        });
    }

    let hungry = senses.now >= animal.fed_until;
    if let Some(prey_species) = traits.preys_on
        && hungry
    {
        // Prey first, then a person on their own.
        let prey = senses
            .animals
            .iter()
            .filter(|(other, species, position)| {
                *other != index && *species == prey_species && near(*position, traits.senses)
            })
            .min_by_key(|(other, _, position)| (chebyshev(here, *position), *other))
            .copied();
        if let Some((target, _, position)) = prey {
            if chebyshev(here, position) <= 1 {
                return Move::AttackAnimal(target);
            }
            return step_toward(here, position, standable)
                .map_or(Move::Stay(AnimalMode::Hunting), |cell| {
                    Move::Step(cell, AnimalMode::Hunting)
                });
        }
        if traits.bite > 0
            && let Some((person, position)) = nearest_person
        {
            if chebyshev(here, position) <= 1 {
                return Move::BitePerson(person);
            }
            return step_toward(here, position, standable)
                .map_or(Move::Stay(AnimalMode::Hunting), |cell| {
                    Move::Step(cell, AnimalMode::Hunting)
                });
        }
    }

    // Calm: drift, mostly toward others of its kind.
    if roll % 3 != 0 {
        return Move::Stay(if hungry {
            AnimalMode::Grazing
        } else {
            AnimalMode::Resting
        });
    }
    let kin: Vec<WorldPosition> = senses
        .animals
        .iter()
        .filter(|(other, species, _)| *other != index && *species == animal.species)
        .map(|(_, _, position)| *position)
        .collect();
    let target = if kin.is_empty() || roll % 2 == 0 {
        let [dx, dy] = [((roll >> 8) % 3) as i64 - 1, ((roll >> 16) % 3) as i64 - 1];
        WorldPosition {
            x: here.x + dx * 4,
            y: here.y + dy * 4,
        }
    } else {
        let count = kin.len() as i64;
        WorldPosition {
            x: kin.iter().map(|position| position.x).sum::<i64>() / count,
            y: kin.iter().map(|position| position.y).sum::<i64>() / count,
        }
    };
    if chebyshev(here, target) <= 1 {
        return Move::Stay(AnimalMode::Grazing);
    }
    step_toward(here, target, standable).map_or(Move::Stay(AnimalMode::Grazing), |cell| {
        Move::Step(cell, AnimalMode::Grazing)
    })
}

fn step_toward(
    here: WorldPosition,
    target: WorldPosition,
    standable: &dyn Fn(WorldPosition) -> bool,
) -> Option<WorldPosition> {
    let current = chebyshev(here, target) * 4 + manhattan(here, target);
    cardinal(here)
        .into_iter()
        .filter(|cell| standable(*cell))
        .map(|cell| (chebyshev(cell, target) * 4 + manhattan(cell, target), cell))
        .filter(|&(score, _)| score < current)
        .min_by_key(|&(score, cell)| (score, cell.y, cell.x))
        .map(|(_, cell)| cell)
}

fn step_away(
    here: WorldPosition,
    from: WorldPosition,
    standable: &dyn Fn(WorldPosition) -> bool,
) -> Option<WorldPosition> {
    // Straight away first (Chebyshev distance), then sideways.
    let current = (chebyshev(here, from), manhattan(here, from));
    cardinal(here)
        .into_iter()
        .filter(|cell| standable(*cell))
        .map(|cell| ((chebyshev(cell, from), manhattan(cell, from)), cell))
        .filter(|&(distance, _)| distance >= current)
        .max_by_key(|&(distance, cell)| (distance, -cell.y, -cell.x))
        .map(|(_, cell)| cell)
}

fn manhattan(left: WorldPosition, right: WorldPosition) -> i64 {
    (left.x - right.x).abs() + (left.y - right.y).abs()
}

impl Wildlife {
    pub(crate) fn views(&self) -> impl Iterator<Item = AnimalView> + '_ {
        self.animals
            .iter()
            .enumerate()
            .filter(|(_, animal)| animal.alive)
            .map(|(id, animal)| AnimalView {
                id: id as u32,
                species: animal.species,
                position: animal.position(),
                mode: animal.mode,
                wounds: animal.wounds,
            })
    }

    pub(crate) fn living(&self, species: Species) -> usize {
        self.animals
            .iter()
            .filter(|animal| animal.alive && animal.species == species)
            .count()
    }

    /// Meat on the carcass at `position`, if any is left.
    pub(crate) fn carcass_meat(&self, position: WorldPosition) -> Option<u8> {
        self.carcasses
            .iter()
            .find(|carcass| carcass.position() == position && carcass.meat > 0)
            .map(|carcass| carcass.meat)
    }

    /// Takes up to `maximum` meat from the carcass at `position`.
    pub(crate) fn butcher(
        &mut self,
        position: WorldPosition,
        maximum: u8,
    ) -> Option<(Material, u8)> {
        let carcass = self
            .carcasses
            .iter_mut()
            .find(|carcass| carcass.position() == position && carcass.meat > 0)?;
        let taken = carcass.meat.min(maximum);
        carcass.meat -= taken;
        Some((Material::Meat, taken))
    }

    /// Leaves a carcass where an animal fell (merging with one already there).
    pub(crate) fn leave_carcass(&mut self, position: WorldPosition, meat: u8, now: u64) {
        let (Ok(x), Ok(y)) = (i16::try_from(position.x), i16::try_from(position.y)) else {
            return;
        };
        if let Some(carcass) = self
            .carcasses
            .iter_mut()
            .find(|carcass| carcass.position() == position)
        {
            carcass.meat = carcass.meat.saturating_add(meat);
            carcass.spoils_at = now + CARCASS_TICKS;
            return;
        }
        self.carcasses.push(Carcass {
            spoils_at: now + CARCASS_TICKS,
            x,
            y,
            meat,
        });
    }

    /// Drops spoiled or emptied carcasses.
    pub(crate) fn rot(&mut self, now: u64) {
        self.carcasses
            .retain(|carcass| carcass.meat > 0 && carcass.spoils_at > now);
    }

    pub(crate) fn bite_recovery() -> u64 {
        BITE_RECOVERY_TICKS
    }

    pub(crate) fn fed_ticks() -> u64 {
        FED_TICKS
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(x: i64, y: i64) -> WorldPosition {
        WorldPosition { x, y }
    }

    fn animal(species: Species, position: WorldPosition) -> Animal {
        Animal {
            next_act: 0,
            fed_until: 0,
            x: position.x as i16,
            y: position.y as i16,
            species,
            mode: AnimalMode::Grazing,
            wounds: 0,
            alive: true,
        }
    }

    fn senses<'a>(
        people: &'a [(AgentId, WorldPosition)],
        animals: &'a [(usize, Species, WorldPosition)],
    ) -> Senses<'a> {
        Senses {
            people,
            animals,
            now: 1_000,
            seed: 1,
        }
    }

    const OPEN: &dyn Fn(WorldPosition) -> bool = &|_| true;

    #[test]
    fn animals_are_compact() {
        assert_eq!(std::mem::size_of::<Animal>(), 24);
        assert_eq!(std::mem::size_of::<Carcass>(), 16);
    }

    #[test]
    fn deer_run_from_people_and_wolves_run_from_crowds() {
        let people = [(AgentId::new(0), at(3, 0))];
        let deer = decide(
            0,
            animal(Species::Deer, at(0, 0)),
            &senses(&people, &[]),
            OPEN,
        );
        assert_eq!(deer, Move::Step(at(-1, 0), AnimalMode::Fleeing));

        let crowd = [
            (AgentId::new(0), at(3, 0)),
            (AgentId::new(1), at(3, 1)),
            (AgentId::new(2), at(4, 0)),
        ];
        let wolf = decide(
            0,
            animal(Species::Wolf, at(0, 0)),
            &senses(&crowd, &[]),
            OPEN,
        );
        assert!(matches!(wolf, Move::Step(_, AnimalMode::Fleeing)));
    }

    #[test]
    fn a_hungry_wolf_hunts_deer_first_then_a_lone_person() {
        let wolf = animal(Species::Wolf, at(0, 0));
        let deer = [(1, Species::Deer, at(5, 0))];
        let lone = [(AgentId::new(0), at(-4, 0))];
        assert_eq!(
            decide(0, wolf, &senses(&lone, &deer), OPEN),
            Move::Step(at(1, 0), AnimalMode::Hunting)
        );
        assert_eq!(
            decide(0, wolf, &senses(&lone, &[]), OPEN),
            Move::Step(at(-1, 0), AnimalMode::Hunting)
        );
        let beside = [(AgentId::new(0), at(1, 0))];
        assert_eq!(
            decide(0, wolf, &senses(&beside, &[]), OPEN),
            Move::BitePerson(AgentId::new(0))
        );
        let fed = Animal {
            fed_until: 5_000,
            ..wolf
        };
        assert!(matches!(
            decide(0, fed, &senses(&lone, &deer), OPEN),
            Move::Stay(_) | Move::Step(_, AnimalMode::Grazing)
        ));
    }

    #[test]
    fn carcasses_can_be_butchered_and_spoil() {
        let mut wildlife = Wildlife::default();
        wildlife.leave_carcass(at(2, 2), 16, 0);
        assert_eq!(wildlife.butcher(at(2, 2), 4), Some((Material::Meat, 4)));
        assert_eq!(wildlife.carcass_meat(at(2, 2)), Some(12));
        wildlife.rot(CARCASS_TICKS);
        assert_eq!(wildlife.carcass_meat(at(2, 2)), None);
    }
}
