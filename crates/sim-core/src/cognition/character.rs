//! What life has made of a person (docs/plans/FEELINGS.md): skill in each
//! activity, grown by practice and faded by disuse, and how far its traits
//! have drifted from those it was born with.

use super::personality::{Activity, Personality};

/// How far life can move a trait from the one a person was born with.
pub const DRIFT_LIMIT: i8 = 40;

/// How many times as often each activity is done as a hunt: frequent work
/// teaches less per go, so skill reflects years of it, not a busy week.
const PRACTICE_RATE: [u32; Activity::COUNT] = [8, 1, 1, 4, 1, 1, 2, 32];

/// 24 bytes: a fine-grained skill per activity and a drift per trait.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Character {
    /// Skill in 1/256ths, so frequent practice can add a little at a time.
    skills: [u16; Activity::COUNT],
    drift: [i8; 8],
}

impl Character {
    /// A founder starts with years of practice behind it: skill matching its
    /// talent, so an average founder is average at everything.
    pub(crate) fn seasoned(personality: &Personality) -> Self {
        Self {
            skills: personality.talents.map(|talent| u16::from(talent) << 8),
            drift: [0; 8],
        }
    }

    pub(crate) const fn skill(&self, activity: Activity) -> u8 {
        (self.skills[activity as usize] >> 8) as u8
    }

    pub(crate) fn skills(&self) -> [u8; Activity::COUNT] {
        self.skills.map(|skill| (skill >> 8) as u8)
    }

    /// Did `activity` once: skill grows, quickly at first and more slowly as
    /// it nears mastery, faster with talent.
    pub(crate) fn practise(&mut self, activity: Activity, talent: u8) {
        let skill = &mut self.skills[activity as usize];
        let gain = (u32::from(u16::MAX - *skill) * (u32::from(talent) + 64)
            / (4_096 * PRACTICE_RATE[activity as usize]))
            .max(1);
        *skill = (u32::from(*skill) + gain).min(u32::from(u16::MAX)) as u16;
    }

    /// A year passes: unpractised skill fades (practice keeps it up).
    pub(crate) fn fade(&mut self) {
        for skill in &mut self.skills {
            *skill -= *skill / 8;
        }
    }

    /// Life moves trait `index` (in `Personality::traits` order) by `amount`,
    /// never further than `DRIFT_LIMIT` from where it started.
    pub(crate) fn nudge(&mut self, index: usize, amount: i8) {
        let drift = &mut self.drift[index];
        *drift = drift
            .saturating_add(amount)
            .clamp(-DRIFT_LIMIT, DRIFT_LIMIT);
    }

    /// `innate` as life has changed it.
    pub(crate) fn shape(&self, innate: Personality) -> Personality {
        let traits = innate.traits();
        innate.with_traits(std::array::from_fn(|index| {
            (i16::from(traits[index]) + i16::from(self.drift[index])).clamp(0, 255) as u8
        }))
    }

    /// How good it is at `activity`: talent counts, practice counts double.
    pub(crate) fn ability(&self, activity: Activity, talent: u8) -> u8 {
        ((u16::from(talent) + 2 * u16::from(self.skill(activity))) / 3) as u8
    }
}

/// `base` scaled by ability: 150% for the worst, 100% at 128, 70% for the best.
pub(crate) fn by_ability(base: u64, ability: u8) -> u64 {
    let percent = if ability < 128 {
        150 - u64::from(ability) * 50 / 128
    } else {
        100 - (u64::from(ability) - 128) * 30 / 127
    };
    base * percent / 100
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn practice_makes_better_faster_with_talent_and_unused_skill_fades() {
        let mut gifted = Character::default();
        let mut plodder = Character::default();
        for _ in 0..20 {
            gifted.practise(Activity::Hunting, 230);
            plodder.practise(Activity::Hunting, 30);
        }
        assert!(gifted.skill(Activity::Hunting) > plodder.skill(Activity::Hunting));
        assert!(plodder.skill(Activity::Hunting) > 0);
        let before = gifted.skill(Activity::Hunting);
        gifted.fade();
        assert!(gifted.skill(Activity::Hunting) < before);
        assert!(gifted.ability(Activity::Hunting, 230) > plodder.ability(Activity::Hunting, 30));
        assert_eq!(by_ability(300, 128), 300);
        assert!(by_ability(300, 255) < 300 && by_ability(300, 0) > 300);
    }

    #[test]
    fn traits_drift_with_life_but_only_so_far() {
        let mut life = Character::default();
        for _ in 0..100 {
            life.nudge(4, -3);
        }
        let shaped = life.shape(Personality::AVERAGE);
        assert_eq!(shaped.boldness, 128 - DRIFT_LIMIT as u8);
        assert_eq!(shaped.curiosity, 128);
        assert_eq!(std::mem::size_of::<Character>(), 24);
    }
}
