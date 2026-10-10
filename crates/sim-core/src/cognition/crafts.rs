//! What an agent knows about things people make: whether a hearth warms you
//! (and so is worth building), and how to knap a stone blade. Knowing it is enough
//! to build one: a hearth is a ring of stones around burning wood, plain to see.
//! The belief changes only from evidence: warming up at one (felt), watching
//! someone warm their hands at one, and family lore.

use crate::AgentId;

const FELT_EVIDENCE: u8 = 8;
const WATCHED_EVIDENCE_CAP: u8 = 5;
const CULTURE_EVIDENCE: u8 = 4;

/// 3 bytes.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub(crate) struct Crafts {
    /// How strongly it believes hearths warm (0–255).
    hearth: u8,
    evidence: u8,
    /// Knapping it has seen or done (0 = it doesn't know how).
    knapping: u8,
}

fn mix(mut key: u64) -> u64 {
    key = (key ^ (key >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    key = (key ^ (key >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    key ^ (key >> 31)
}

impl Crafts {
    /// One founding family (chosen by the seed) keeps fire; the other doesn't.
    pub(crate) fn founding(seed: u64, founder: AgentId, family_size: u32) -> Self {
        let family = u64::from(founder.get() / family_size.max(1));
        // One founding family keeps fire, the other knaps blades.
        if (mix(seed ^ 0x4649_5245) + family) % 2 == 0 {
            Self {
                hearth: 200,
                evidence: CULTURE_EVIDENCE,
                knapping: 0,
            }
        } else {
            Self {
                knapping: CULTURE_EVIDENCE,
                ..Self::default()
            }
        }
    }

    /// Knows how to knap a blade from stone.
    pub(crate) const fn knows_knapping(self) -> bool {
        self.knapping > 0
    }

    /// It knapped a blade, or watched someone do it.
    pub(crate) fn saw_knapping(&mut self) {
        self.knapping = self.knapping.saturating_add(1);
    }

    /// Believes a hearth would warm it (worth seeking out and building).
    pub(crate) const fn knows_hearths(self) -> bool {
        self.evidence > 0 && self.hearth > 64
    }

    /// It warmed up at a hearth.
    pub(crate) fn warmed(&mut self) {
        self.hearth = self.hearth.max(230);
        self.evidence = self
            .evidence
            .saturating_add(FELT_EVIDENCE)
            .max(FELT_EVIDENCE);
    }

    /// It watched someone warm their hands at a hearth.
    pub(crate) fn saw_warming(&mut self) {
        if self.evidence >= FELT_EVIDENCE {
            return;
        }
        let step = (180 - i32::from(self.hearth)) / (i32::from(self.evidence) + 1);
        self.hearth = (i32::from(self.hearth) + step.max(1)).clamp(0, 255) as u8;
        self.evidence = (self.evidence + 1).min(WATCHED_EVIDENCE_CAP.max(self.evidence));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_family_keeps_fire_and_others_learn_by_watching() {
        let first = Crafts::founding(1, AgentId::new(0), 8);
        let second = Crafts::founding(1, AgentId::new(8), 8);
        assert_ne!(first.knows_hearths(), second.knows_hearths());
        let mut child = Crafts::default();
        assert!(!child.knows_hearths());
        child.saw_warming();
        assert!(child.knows_hearths());
        assert_ne!(
            first.knows_knapping(),
            second.knows_knapping(),
            "the other family knaps"
        );
        child.saw_knapping();
        assert!(child.knows_knapping());
        assert_eq!(std::mem::size_of::<Crafts>(), 3);
    }
}
