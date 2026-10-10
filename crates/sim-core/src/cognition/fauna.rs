//! What an agent believes about animals (plan L3): whether a species is worth
//! hunting and whether it's dangerous. Bounded: one small belief per species.
//! Beliefs change only from evidence the agent has: being bitten (felt), seeing
//! someone bitten or an animal brought down, being warned, and family culture.

use crate::{AgentId, Species};

/// Evidence from being bitten oneself.
const FELT_EVIDENCE: u8 = 8;
/// Evidence ceiling for what can be learned by watching or being told.
const WATCHED_EVIDENCE_CAP: u8 = 5;
const CULTURE_EVIDENCE: u8 = 4;
/// Beliefs above this (out of 255) count.
const THRESHOLD: u8 = 64;

/// One belief about a species. 3 bytes.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub(crate) struct FaunaBelief {
    /// How worthwhile it seems to hunt (0–255).
    prey: u8,
    /// How dangerous it seems (0–255).
    danger: u8,
    evidence: u8,
}

/// A read-only belief, for tools and the viewer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FaunaView {
    pub species: Species,
    pub prey: u8,
    pub danger: u8,
    pub evidence: u8,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Fauna {
    beliefs: [FaunaBelief; Species::COUNT],
}

fn mix(mut key: u64) -> u64 {
    key = (key ^ (key >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    key = (key ^ (key >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    key ^ (key >> 31)
}

fn toward(current: u8, target: u8, weight: u8) -> u8 {
    let (current, target) = (i32::from(current), i32::from(target));
    let step = (target - current) / (i32::from(weight) + 1);
    let moved = if step == 0 && target != current {
        current + (target - current).signum()
    } else {
        current + step
    };
    moved.clamp(0, 255) as u8
}

impl Fauna {
    /// A founder's inherited lore. Both families hunt deer. One family (chosen by
    /// the seed) knows wolves are dangerous; the other has never met one.
    pub(crate) fn founding(seed: u64, founder: AgentId, family_size: u32) -> Self {
        let family = u64::from(founder.get() / family_size.max(1));
        let mut fauna = Self::default();
        fauna.beliefs[Species::Deer as usize] = FaunaBelief {
            prey: 200,
            danger: 0,
            evidence: CULTURE_EVIDENCE,
        };
        if (mix(seed ^ 0x574f_4c46) + family) % 2 == 0 {
            fauna.beliefs[Species::Wolf as usize] = FaunaBelief {
                prey: 0,
                danger: 200,
                evidence: CULTURE_EVIDENCE,
            };
        }
        fauna
    }

    pub(crate) fn dangerous(&self, species: Species) -> bool {
        let belief = self.beliefs[species as usize];
        belief.evidence > 0 && belief.danger > THRESHOLD
    }

    pub(crate) fn prey(&self, species: Species) -> bool {
        let belief = self.beliefs[species as usize];
        belief.evidence > 0 && belief.prey > THRESHOLD && belief.prey > belief.danger
    }

    /// It was bitten: now it knows.
    pub(crate) fn bitten(&mut self, species: Species) {
        let belief = &mut self.beliefs[species as usize];
        belief.danger = belief.danger.max(220);
        belief.evidence = belief
            .evidence
            .saturating_add(FELT_EVIDENCE)
            .max(FELT_EVIDENCE);
    }

    /// It saw someone bitten, or was warned about the species.
    pub(crate) fn heard_of_danger(&mut self, species: Species) {
        let belief = &mut self.beliefs[species as usize];
        if belief.evidence >= FELT_EVIDENCE {
            return;
        }
        belief.danger = toward(belief.danger, 180, belief.evidence / 2);
        belief.evidence = (belief.evidence + 1).min(WATCHED_EVIDENCE_CAP.max(belief.evidence));
    }

    /// It saw a person bring one down (so it can be hunted, and yields meat).
    pub(crate) fn saw_hunted(&mut self, species: Species) {
        let belief = &mut self.beliefs[species as usize];
        belief.prey = toward(belief.prey, 180, belief.evidence.min(WATCHED_EVIDENCE_CAP));
        belief.evidence = (belief.evidence + 1).min(WATCHED_EVIDENCE_CAP.max(belief.evidence));
    }

    pub(crate) fn views(&self) -> impl Iterator<Item = FaunaView> + '_ {
        Species::ALL
            .into_iter()
            .filter(|species| self.beliefs[*species as usize].evidence > 0)
            .map(|species| {
                let belief = self.beliefs[species as usize];
                FaunaView {
                    species,
                    prey: belief.prey,
                    danger: belief.danger,
                    evidence: belief.evidence,
                }
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn beliefs_are_compact() {
        assert_eq!(std::mem::size_of::<Fauna>(), 3 * Species::COUNT);
    }

    #[test]
    fn founders_hunt_deer_and_only_one_family_fears_wolves() {
        let first = Fauna::founding(1, AgentId::new(0), 8);
        let second = Fauna::founding(1, AgentId::new(8), 8);
        assert!(first.prey(Species::Deer) && second.prey(Species::Deer));
        assert_ne!(
            first.dangerous(Species::Wolf),
            second.dangerous(Species::Wolf)
        );
        assert!(
            !Fauna::default().prey(Species::Deer),
            "children know nothing"
        );
    }

    #[test]
    fn a_bite_or_a_warning_teaches_danger() {
        let mut fauna = Fauna::default();
        fauna.bitten(Species::Wolf);
        assert!(fauna.dangerous(Species::Wolf));
        let mut told = Fauna::default();
        told.heard_of_danger(Species::Wolf);
        assert!(told.dangerous(Species::Wolf));
        told.saw_hunted(Species::Deer);
        assert!(told.prey(Species::Deer));
    }
}
