//! Personality: a compact trait vector per agent (the spec's "trait vector
//! rather than a class hierarchy"). Traits don't add new behaviors; they tune
//! how strongly each agent leans toward behaviors it already has.

use crate::AgentId;

const PERSONALITY_SEED: u64 = 0x5045_5253_4f4e_414c;

/// Four traits, each 0 (low) to 255 (high). Derived deterministically from the
/// world seed and the agent's id, so a replay always gets the same people.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Personality {
    /// Explores more often and farther; settles for less knowledge before exploring.
    pub curiosity: u8,
    /// Tops up water and food earlier, stays closer to known water, prepares shelter sooner.
    pub caution: u8,
    /// Seeks out and stays with others, points out places more often, visits friends.
    pub sociability: u8,
    /// Gathers and builds when nothing is pressing instead of resting.
    pub diligence: u8,
}

impl Personality {
    /// Everyone exactly average; used where a personality-neutral rule is needed.
    pub const AVERAGE: Self = Self {
        curiosity: 128,
        caution: 128,
        sociability: 128,
        diligence: 128,
    };

    /// The agent's innate personality. Each trait averages two random bytes, so
    /// most agents are moderate and extremes are rare.
    pub fn of(seed: u64, agent: AgentId) -> Self {
        let mut key = seed ^ PERSONALITY_SEED ^ (u64::from(agent.get()) << 17);
        key = (key ^ (key >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        key = (key ^ (key >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        key ^= key >> 31;
        let trait_at = |index: u32| {
            let pair = (key >> (index * 16)) as u16;
            ((u16::from(pair as u8) + (pair >> 8)) / 2) as u8
        };
        Self {
            curiosity: trait_at(0),
            caution: trait_at(1),
            sociability: trait_at(2),
            diligence: trait_at(3),
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
    }

    #[test]
    fn scale_maps_both_directions() {
        assert_eq!(Personality::scale(0, 10, 20), 10);
        assert_eq!(Personality::scale(255, 10, 20), 20);
        assert_eq!(Personality::scale(255, 4_000, 2_000), 2_000);
    }
}
