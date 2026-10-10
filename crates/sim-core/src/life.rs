//! Sex, age, and life stages. A year is one full turn of the seasons (four
//! simulated hours), starting each spring.

use crate::{AgentId, wildlife::mix};

/// Simulated seconds in one year: four seasons.
pub const SECONDS_PER_YEAR: i64 = 4 * crate::SEASON_SECONDS as i64;
/// Ticks in one year.
pub const TICKS_PER_YEAR: u64 = SECONDS_PER_YEAR as u64 * 60;
/// A month is a twelfth of a year (three to a season).
pub const MONTH_SECONDS: i64 = SECONDS_PER_YEAR / 12;
/// Up to this age a child is carried and nursed.
pub const WEANING_AGE: u32 = 3;
/// Adulthood begins at this age.
pub const ADULT_AGE: u32 = 15;
/// Frailty sets in from this age.
pub const ELDER_AGE: u32 = 50;
/// Children this young don't hunt.
pub const HUNTING_AGE: u32 = 12;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum Sex {
    Female = 0,
    Male = 1,
}

/// What someone is called: a two-syllable sound (1,024 of them), separate
/// from the sounds used as words.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct Name(pub u16);

impl Name {
    pub const COUNT: u16 = 1_024;

    /// A pronounceable rendering, such as "Tavo".
    pub fn spoken(self) -> String {
        const CONSONANTS: [char; 8] = ['T', 'M', 'R', 'S', 'N', 'K', 'L', 'D'];
        const LATER: [char; 8] = ['t', 'm', 'r', 's', 'n', 'k', 'l', 'd'];
        const VOWELS: [char; 4] = ['a', 'i', 'o', 'e'];
        let id = usize::from(self.0 % Self::COUNT);
        let mut name = String::with_capacity(4);
        name.push(CONSONANTS[id & 7]);
        name.push(VOWELS[(id >> 3) & 3]);
        name.push(LATER[(id >> 5) & 7]);
        name.push(VOWELS[(id >> 8) & 3]);
        name
    }

    pub(crate) fn from_roll(roll: u64) -> Self {
        Self((roll % u64::from(Self::COUNT)) as u16)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum LifeStage {
    /// Carried and nursed.
    Baby,
    Child,
    Adult,
    /// Growing frail.
    Elder,
}

impl LifeStage {
    pub const fn of(age: u32) -> Self {
        if age < WEANING_AGE {
            Self::Baby
        } else if age < ADULT_AGE {
            Self::Child
        } else if age < ELDER_AGE {
            Self::Adult
        } else {
            Self::Elder
        }
    }
}

/// Who someone is in body: sex and when they were born.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Life {
    /// Simulated second of birth (negative for people older than the run).
    pub(crate) born: i32,
    pub(crate) sex: Sex,
    /// For someone born during the run: the personality they got from their
    /// parents (others' come from the seed).
    pub(crate) inherited: Option<crate::Personality>,
    /// What their mother (or, for those present from the start, their family) named them.
    pub(crate) name: Name,
}

const _: () = assert!(size_of::<Life>() == 12);

impl Life {
    /// Someone present when the run starts: a founder is an adult of 18-39, a
    /// child of the band 4-10. Sex and age come from the seed.
    pub(crate) fn starting(seed: u64, agent: AgentId, child: bool) -> Self {
        let roll = mix(seed ^ 0x4c49_4645 ^ u64::from(agent.get()).wrapping_mul(0x9e37_79b9));
        let sex = if roll & 1 == 0 {
            Sex::Female
        } else {
            Sex::Male
        };
        let age = if child {
            4 + (roll >> 8) % 7
        } else {
            18 + (roll >> 8) % 22
        };
        let offset = (roll >> 16) % SECONDS_PER_YEAR as u64;
        Self {
            born: -((age as i64 * SECONDS_PER_YEAR + offset as i64) as i32),
            sex,
            inherited: None,
            name: Name::from_roll(roll >> 28),
        }
    }

    /// Whole years of age at simulated second `now`.
    pub(crate) fn age(self, now: i64) -> u32 {
        ((now - i64::from(self.born)).max(0) / SECONDS_PER_YEAR) as u32
    }

    pub(crate) fn view(self, now: i64) -> LifeView {
        let age = self.age(now);
        LifeView {
            name: self.name,
            sex: self.sex,
            age,
            stage: LifeStage::of(age),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LifeView {
    pub name: Name,
    pub sex: Sex,
    /// Whole years.
    pub age: u32,
    pub stage: LifeStage,
}

/// Chance out of 1,000 that someone of `age` dies of old age in the coming
/// year: nothing before `ELDER_AGE`, then rising with the square of the years
/// past it (about 4% at 60, 16% at 70, 36% at 80).
pub(crate) fn old_age_risk(age: u32) -> u64 {
    let past = u64::from(age.saturating_sub(ELDER_AGE));
    (past * past * 4 / 10).min(1_000)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starting_ages_and_sexes_are_deterministic_and_mixed() {
        let founders: Vec<Life> = (0..40)
            .map(|id| Life::starting(7, AgentId::new(id), false))
            .collect();
        assert_eq!(
            founders,
            (0..40)
                .map(|id| Life::starting(7, AgentId::new(id), false))
                .collect::<Vec<_>>()
        );
        assert!(founders.iter().all(|life| (18..40).contains(&life.age(0))));
        let women = founders
            .iter()
            .filter(|life| life.sex == Sex::Female)
            .count();
        assert!((10..=30).contains(&women), "{women} women of 40");
        let child = Life::starting(7, AgentId::new(3), true);
        assert!((4..=10).contains(&child.age(0)));
        assert_eq!(child.view(0).stage, LifeStage::Child);
    }

    #[test]
    fn people_age_one_year_per_simulated_hour() {
        let life = Life {
            born: 0,
            sex: Sex::Male,
            inherited: None,
            name: Name(0),
        };
        assert_eq!(life.age(SECONDS_PER_YEAR * 2 - 1), 1);
        assert_eq!(life.view(SECONDS_PER_YEAR * 2).stage, LifeStage::Baby);
        assert_eq!(life.view(SECONDS_PER_YEAR * 15).stage, LifeStage::Adult);
        assert_eq!(life.view(SECONDS_PER_YEAR * 50).stage, LifeStage::Elder);
    }

    #[test]
    fn names_are_pronounceable_and_varied() {
        assert_eq!(Name(0).spoken(), "Tata");
        let names: std::collections::BTreeSet<String> =
            (0..Name::COUNT).map(|id| Name(id).spoken()).collect();
        assert_eq!(names.len(), usize::from(Name::COUNT));
    }

    #[test]
    fn old_age_risk_rises_after_fifty() {
        assert_eq!(old_age_risk(49), 0);
        assert_eq!(old_age_risk(60), 40);
        assert_eq!(old_age_risk(70), 160);
        assert_eq!(old_age_risk(110), 1_000);
    }
}
