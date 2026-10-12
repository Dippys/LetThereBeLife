//! Personality: a compact trait vector per agent (the spec's "trait vector
//! rather than a class hierarchy"). Traits don't add new behaviors; they tune
//! how strongly each agent leans toward behaviors it already has.

use crate::AgentId;

const PERSONALITY_SEED: u64 = 0x5045_5253_4f4e_414c;

/// Things a person can be better or worse at (docs/plans/FEELINGS.md).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
#[repr(u8)]
pub enum Activity {
    /// Picking berries and other food.
    Foraging,
    Hunting,
    /// Knapping, spears, baskets, hides.
    Crafting,
    /// Huts and hearths, and chopping wood or breaking stone for them.
    Building,
    Cooking,
    Farming,
    /// Finding the way out and back.
    Wayfinding,
    /// Picking up words.
    Words,
}

impl Activity {
    pub const COUNT: usize = 8;
    pub const ALL: [Self; Self::COUNT] = [
        Self::Foraging,
        Self::Hunting,
        Self::Crafting,
        Self::Building,
        Self::Cooking,
        Self::Farming,
        Self::Wayfinding,
        Self::Words,
    ];
}

/// Eight traits and eight talents, each 0 (low) to 255 (high). Founders' come
/// from the life seed; children's are a blend of their parents'.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct Personality {
    /// Explores more often and farther; settles for less knowledge before exploring.
    pub curiosity: u8,
    /// Tops up water and food earlier, stays closer to known water, prepares shelter sooner.
    pub caution: u8,
    /// Seeks out and stays with others, points out places more often, visits friends.
    pub sociability: u8,
    /// Gathers and builds when nothing is pressing instead of resting.
    pub diligence: u8,
    /// Frightened less easily, and calms sooner.
    pub boldness: u8,
    /// Grateful and fond of people readily, slow to anger.
    pub warmth: u8,
    /// Angered easily and strongly.
    pub temper: u8,
    /// Feelings fade faster; worries less.
    pub steadiness: u8,
    /// Inborn knack for each `Activity`.
    pub talents: [u8; Activity::COUNT],
}

impl Personality {
    /// Everyone exactly average; used where a personality-neutral rule is needed.
    pub const AVERAGE: Self = Self {
        curiosity: 128,
        caution: 128,
        sociability: 128,
        diligence: 128,
        boldness: 128,
        warmth: 128,
        temper: 128,
        steadiness: 128,
        talents: [128; Activity::COUNT],
    };

    /// The agent's innate personality. Each trait and talent averages two
    /// random bytes, so most people are moderate and extremes are rare.
    pub fn of(seed: u64, agent: AgentId) -> Self {
        let draw = |salt: u64| {
            let mut key = seed ^ PERSONALITY_SEED ^ salt ^ (u64::from(agent.get()) << 17);
            key = (key ^ (key >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
            key = (key ^ (key >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
            key ^ (key >> 31)
        };
        let values = [
            draw(0),
            draw(0x7472_6169_7473),
            draw(0x7461_6c65_6e74),
            draw(0x6b6e_6163_6b73),
        ];
        let at = |index: usize| {
            let pair = (values[index / 4] >> ((index % 4) * 16)) as u16;
            ((u16::from(pair as u8) + (pair >> 8)) / 2) as u8
        };
        Self {
            curiosity: at(0),
            caution: at(1),
            sociability: at(2),
            diligence: at(3),
            boldness: at(4),
            warmth: at(5),
            temper: at(6),
            steadiness: at(7),
            talents: std::array::from_fn(|index| at(8 + index)),
        }
    }

    pub const fn talent(&self, activity: Activity) -> u8 {
        self.talents[activity as usize]
    }

    /// The eight traits in order (curiosity, caution, sociability, diligence,
    /// boldness, warmth, temper, steadiness).
    pub const fn traits(&self) -> [u8; 8] {
        [
            self.curiosity,
            self.caution,
            self.sociability,
            self.diligence,
            self.boldness,
            self.warmth,
            self.temper,
            self.steadiness,
        ]
    }

    /// This personality with its traits replaced by `traits` (same order).
    pub const fn with_traits(self, traits: [u8; 8]) -> Self {
        Self {
            curiosity: traits[0],
            caution: traits[1],
            sociability: traits[2],
            diligence: traits[3],
            boldness: traits[4],
            warmth: traits[5],
            temper: traits[6],
            steadiness: traits[7],
            ..self
        }
    }

    /// Linearly maps a trait onto `[low, high]` (works for decreasing ranges too).
    pub(crate) fn scale(value: u8, low: i64, high: i64) -> i64 {
        low + (high - low) * i64::from(value) / 255
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn personalities_are_stable_and_varied() {
        let people: Vec<_> = (0..200)
            .map(|id| Personality::of(1, AgentId::new(id)))
            .collect();
        assert_eq!(people[17], Personality::of(1, AgentId::new(17)));
        assert_ne!(people[17], Personality::of(2, AgentId::new(17)));
        let curious = people.iter().filter(|p| p.curiosity > 170).count();
        let incurious = people.iter().filter(|p| p.curiosity < 85).count();
        let middling = people
            .iter()
            .filter(|p| (85..=170).contains(&p.curiosity))
            .count();
        assert!(curious > 0 && incurious > 0, "extremes exist");
        assert!(middling > curious + incurious, "most people are moderate");
        let person = people[17];
        assert_ne!(
            person.talents,
            [person.talents[0]; Activity::COUNT],
            "uneven talents"
        );
        assert_eq!(person.with_traits(person.traits()), person);
    }

    #[test]
    fn scale_maps_both_directions() {
        assert_eq!(Personality::scale(0, 10, 20), 10);
        assert_eq!(Personality::scale(255, 10, 20), 20);
        assert_eq!(Personality::scale(255, 4_000, 2_000), 2_000);
    }
}
