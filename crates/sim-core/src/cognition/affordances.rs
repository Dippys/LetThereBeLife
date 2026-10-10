//! What an agent believes materials are good for (plan L2). Bounded: one small
//! belief per material. Beliefs change only from evidence the agent has: what it
//! felt after eating something, seeing someone eat (and retch, which is
//! visible), being handed something to eat, and its family's food culture.
//! Nothing here reads a material's real properties except `felt`, which is the
//! agent's own body telling it what happened.

use crate::{AgentId, Material, MaterialProperties};

/// Need points per belief unit (beliefs are 0–255, so up to ~8,000 points).
pub const BELIEF_UNIT: u16 = 32;
/// Evidence from eating something oneself. Stronger than anything else.
const FELT_EVIDENCE: u8 = 8;
/// Evidence ceiling for what can be learned by watching others.
const WATCHED_EVIDENCE_CAP: u8 = 5;
/// How much a family's culture is held: firm, but not as firm as experience.
const CULTURE_EVIDENCE: u8 = 4;

/// One belief about a material. 3 bytes.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub(crate) struct Belief {
    /// Expected hunger relief per unit, in `BELIEF_UNIT`s.
    feeds: u8,
    /// Expected sickness per unit, in `BELIEF_UNIT`s.
    sickens: u8,
    /// How much evidence backs this (0 = never thought about it).
    evidence: u8,
}

/// A read-only belief, for tools and the viewer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AffordanceView {
    pub material: Material,
    /// Expected hunger relief per unit (need points).
    pub feeds: u16,
    /// Expected sickness per unit (need points).
    pub sickens: u16,
    pub evidence: u8,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Affordances {
    beliefs: [Belief; Material::COUNT],
}

const fn units(points: u16) -> u8 {
    let units = points / BELIEF_UNIT;
    if units > 255 { 255 } else { units as u8 }
}

fn mix(mut key: u64) -> u64 {
    key = (key ^ (key >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    key = (key ^ (key >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    key ^ (key >> 31)
}

/// Moves `current` a fraction `1 / (evidence + 1)` of the way to `target`.
fn toward(current: u8, target: u8, evidence: u8) -> u8 {
    let (current, target) = (i32::from(current), i32::from(target));
    let step = (target - current) / (i32::from(evidence) + 1);
    let moved = if step == 0 && target != current {
        current + (target - current).signum()
    } else {
        current + step
    };
    moved.clamp(0, 255) as u8
}

impl Affordances {
    /// A founder's inherited food culture. Everyone in the band knows berries feed
    /// you and that wood and stone don't. Bitter berries divide the families: in
    /// one, they're taken for food; in the other, for something that makes you
    /// sick (which family is which depends on the seed). A few founders never
    /// learned either.
    pub(crate) fn founding(seed: u64, founder: AgentId, family_size: u32) -> Self {
        let family = u64::from(founder.get() / family_size.max(1));
        let mut affordances = Self::default();
        let known = |feeds: u16, sickens: u16| Belief {
            feeds: units(feeds),
            sickens: units(sickens),
            evidence: CULTURE_EVIDENCE,
        };
        affordances.beliefs[Material::Berries as usize] = known(3_800, 0);
        affordances.beliefs[Material::Wood as usize] = known(0, 0);
        affordances.beliefs[Material::Stone as usize] = known(0, 0);
        let family_eats_bitter = (mix(seed ^ 0x4249_5454_4552) + family) % 2 == 0;
        let ignorant = mix(seed ^ 0x49_474e ^ (u64::from(founder.get()) << 8)) % 8 == 0;
        if !ignorant {
            affordances.beliefs[Material::Bitterberries as usize] = if family_eats_bitter {
                Belief {
                    evidence: CULTURE_EVIDENCE / 2,
                    ..known(2_000, 0)
                }
            } else {
                Belief {
                    evidence: CULTURE_EVIDENCE / 2,
                    ..known(800, 2_500)
                }
            };
        }
        affordances
    }

    /// Whether the agent has any belief about `material` at all.
    pub(crate) fn knows(&self, material: Material) -> bool {
        self.beliefs[material as usize].evidence > 0
    }

    /// Expected net worth of eating one unit, in belief units: what it feeds
    /// minus twice what it sickens (being sick is worse than being hungry).
    /// `None` when the agent has no idea.
    pub(crate) fn food_value(&self, material: Material) -> Option<i16> {
        let belief = self.beliefs[material as usize];
        (belief.evidence > 0).then(|| i16::from(belief.feeds) - 2 * i16::from(belief.sickens))
    }

    /// `food_value` for every material.
    pub(crate) fn food_values(&self) -> [Option<i16>; Material::COUNT] {
        Material::ALL.map(|material| self.food_value(material))
    }

    /// The agent ate `material` and felt what it does.
    pub(crate) fn felt(&mut self, material: Material, properties: MaterialProperties) {
        let belief = &mut self.beliefs[material as usize];
        belief.feeds = units(properties.nutrition);
        belief.sickens = units(properties.toxicity);
        belief.evidence = belief
            .evidence
            .saturating_add(FELT_EVIDENCE)
            .max(FELT_EVIDENCE);
    }

    /// The agent watched someone eat `material` (and saw whether they retched).
    /// People eat what they think is food, so eating suggests it feeds; retching
    /// says it sickens.
    pub(crate) fn saw_eaten(&mut self, material: Material, retched: bool) {
        let belief = &mut self.beliefs[material as usize];
        if belief.evidence >= FELT_EVIDENCE {
            // Its own stomach outranks what it saw happen to someone else.
            return;
        }
        let weight = belief.evidence;
        belief.feeds = toward(belief.feeds, units(2_400), weight);
        if retched {
            belief.sickens = toward(belief.sickens, units(2_400), weight / 2);
        }
        belief.evidence = (belief.evidence + 1).min(WATCHED_EVIDENCE_CAP.max(belief.evidence));
    }

    pub(crate) fn views(&self) -> impl Iterator<Item = AffordanceView> + '_ {
        Material::ALL
            .into_iter()
            .filter(|material| self.knows(*material))
            .map(|material| {
                let belief = self.beliefs[material as usize];
                AffordanceView {
                    material,
                    feeds: u16::from(belief.feeds) * BELIEF_UNIT,
                    sickens: u16::from(belief.sickens) * BELIEF_UNIT,
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
        assert_eq!(std::mem::size_of::<Belief>(), 3);
        assert_eq!(std::mem::size_of::<Affordances>(), 3 * Material::COUNT);
    }

    #[test]
    fn children_know_nothing_and_founders_know_berries_feed_them() {
        let child = Affordances::default();
        assert_eq!(child.food_value(Material::Berries), None);
        let founder = Affordances::founding(1, AgentId::new(0), 8);
        assert!(founder.food_value(Material::Berries).unwrap() > 100);
        assert_eq!(founder.food_value(Material::Stone), Some(0));
    }

    #[test]
    fn the_two_families_disagree_about_bitter_berries() {
        let values: Vec<_> = [0, 8]
            .map(|id| {
                (0..8)
                    .filter_map(|member| {
                        Affordances::founding(1, AgentId::new(id + member), 8)
                            .food_value(Material::Bitterberries)
                    })
                    .map(i32::from)
                    .sum::<i32>()
            })
            .into();
        assert!(
            (values[0] > 0) != (values[1] > 0),
            "one family eats them, the other avoids them: {values:?}"
        );
    }

    #[test]
    fn eating_teaches_the_truth_and_outranks_hearsay() {
        let mut belief = Affordances::founding(1, AgentId::new(0), 8);
        belief.felt(
            Material::Bitterberries,
            Material::Bitterberries.properties(),
        );
        assert!(belief.food_value(Material::Bitterberries).unwrap() < 0);
        // Watching someone happily eat them doesn't undo what it felt.
        belief.saw_eaten(Material::Bitterberries, false);
        assert!(belief.food_value(Material::Bitterberries).unwrap() < 0);
    }

    #[test]
    fn watching_others_eat_and_retch_teaches_a_child() {
        let mut child = Affordances::default();
        for _ in 0..3 {
            child.saw_eaten(Material::Berries, false);
        }
        assert!(child.food_value(Material::Berries).unwrap() > 0);
        for _ in 0..3 {
            child.saw_eaten(Material::Bitterberries, true);
        }
        assert!(child.food_value(Material::Bitterberries).unwrap() < 0);
    }
}
