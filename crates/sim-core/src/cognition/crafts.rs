//! What an agent knows about things people make: whether a hearth warms you
//! (and so is worth building), how to knap a stone blade, and how to build a hut. Knowing it is enough
//! to build one: a hearth is a ring of stones around burning wood, plain to see.
//! The belief changes only from evidence: warming up at one (felt), watching
//! someone warm their hands at one, and family lore.

use crate::{AgentId, Kinds, Material, Method, RECIPES, Recipe, canonical};

const FELT_EVIDENCE: u8 = 8;
const WATCHED_EVIDENCE_CAP: u8 = 5;
const CULTURE_EVIDENCE: u8 = 4;

/// Ways of building beyond huts and hearths, learned by trying and watching.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum Design {
    /// Walling a hut in stone.
    StoneWalls = 1,
    /// A drying rack.
    Rack = 2,
    /// A storage pit.
    Pit = 4,
}

impl Design {
    /// The design a structure of `kind` shows (if it's one of these).
    pub(crate) const fn of(kind: crate::StructureKind) -> Option<Self> {
        match kind {
            crate::StructureKind::Rack => Some(Self::Rack),
            crate::StructureKind::Pit => Some(Self::Pit),
            crate::StructureKind::Shelter | crate::StructureKind::Hearth => None,
        }
    }
}

/// Combinations a person remembers trying that made nothing.
pub const TRIED_SLOTS: usize = 16;

/// A combination to try or make: up to three kinds of input (a set) and the
/// method, packed in 32 bits (bit 31 set when it holds one).
#[derive(
    Debug,
    Default,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    serde::Serialize,
    serde::Deserialize,
)]
pub(crate) struct Attempt(u32);

impl Attempt {
    const NONE: u32 = 0x1F;

    pub(crate) fn new(kinds: Kinds, method: Method) -> Self {
        let kinds = canonical(kinds);
        let mut bits = 1 << 31 | (method as u32) << 15;
        for (index, kind) in kinds.iter().enumerate() {
            bits |= kind.map_or(Self::NONE, |material| material as u32) << (index * 5);
        }
        Self(bits)
    }

    pub(crate) const fn is_some(self) -> bool {
        self.0 & 1 << 31 != 0
    }

    pub(crate) fn kinds(self) -> Kinds {
        std::array::from_fn(|index| {
            let kind = (self.0 >> (index * 5)) & 0x1F;
            (kind != Self::NONE).then(|| Material::ALL[kind as usize % Material::COUNT])
        })
    }

    pub(crate) fn method(self) -> Method {
        Method::ALL[((self.0 >> 15) & 0x7) as usize % Method::COUNT]
    }

    /// The recipe physics has for this combination, if any.
    pub(crate) fn recipe(self) -> Option<(usize, Recipe)> {
        Recipe::find(self.kinds(), self.method())
    }
}

/// 80 bytes.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[repr(C)]
pub(crate) struct Crafts {
    /// Recipes it knows (a bit per index in `RECIPES`).
    recipes: u16,
    /// Combinations it tried that made nothing (oldest replaced first).
    tried: [Attempt; TRIED_SLOTS],
    /// What it's trying right now, judged when the work is done.
    pending: Attempt,
    /// Building designs it knows beyond huts and hearths (`Design` bits).
    designs: u8,
    /// How strongly it believes hearths warm (0–255).
    hearth: u8,
    evidence: u8,
    /// Knapping it has seen or done (0 = it doesn't know how).
    knapping: u8,
    /// Hut-building it has seen or done (0 = it doesn't know how).
    huts: u8,
    /// Seeds it has seen grow or watched planted (0 = it doesn't know).
    planting: u8,
    /// Sickness by a rotting body it has felt, or burials it has watched.
    burial: u8,
    /// The last new thing it learned to make (`Material` + 1; 0 for none).
    newest: u8,
}

/// One of `options`, by `roll`.
fn pick(options: &[Attempt], roll: u64) -> Option<Attempt> {
    (!options.is_empty()).then(|| options[(roll % options.len() as u64) as usize])
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
        // Both founding families build huts (children must learn it); one keeps
        // fire, the other knaps blades.
        if (mix(seed ^ 0x4649_5245) + family) % 2 == 0 {
            Self {
                hearth: 200,
                evidence: CULTURE_EVIDENCE,
                huts: CULTURE_EVIDENCE,
                ..Self::default()
            }
        } else {
            Self {
                knapping: CULTURE_EVIDENCE,
                huts: CULTURE_EVIDENCE,
                ..Self::default()
            }
        }
    }

    /// Knows how to knap a blade from stone.
    pub(crate) const fn knows_knapping(self) -> bool {
        self.knapping > 0
    }

    /// Knows how to build a hut.
    pub(crate) const fn knows_huts(self) -> bool {
        self.huts > 0
    }

    /// It built a hut, or watched one go up.
    pub(crate) fn saw_building(&mut self) {
        self.huts = self.huts.saturating_add(1);
    }

    /// Believes the dead are better buried.
    pub(crate) const fn knows_burial(self) -> bool {
        self.burial > 0
    }

    /// It fell sick by a rotting body, or watched someone bury one.
    pub(crate) fn saw_burial(&mut self) {
        self.burial = self.burial.saturating_add(1);
    }

    /// Knows that berries pushed into the ground grow into a bush.
    pub(crate) const fn knows_planting(self) -> bool {
        self.planting > 0
    }

    /// It saw a bush come up where it ate, planted one, or watched it done.
    pub(crate) fn saw_planting(&mut self) {
        self.planting = self.planting.saturating_add(1);
    }

    /// It knapped a blade, or watched someone do it.
    pub(crate) fn saw_knapping(&mut self) {
        self.knapping = self.knapping.saturating_add(1);
    }

    /// Believes a hearth would warm it (worth seeking out and building).
    pub(crate) const fn knows_hearths(self) -> bool {
        self.evidence > 0 && self.hearth > 64
    }

    /// Knows recipe `index` (in `RECIPES`). Knapping lore covers what's made
    /// by hand or edge from stone and wood; keeping fire covers roasting.
    pub(crate) fn knows(self, index: usize) -> bool {
        let lore = match RECIPES[index].output {
            Material::Blade | Material::Basket | Material::Spear => self.knows_knapping(),
            Material::Roast => self.knows_hearths(),
            _ => false,
        };
        lore || self.recipes & 1 << index != 0
    }

    /// It made recipe `index`, or watched it made.
    pub(crate) fn learn(&mut self, index: usize) {
        if !self.knows(index) {
            self.newest = RECIPES[index].output as u8 + 1;
        }
        self.recipes |= 1 << index;
    }

    /// The last new thing it learned to make, the one it's keenest to use.
    pub(crate) fn newest(self) -> Option<Material> {
        (self.newest > 0).then(|| Material::ALL[usize::from(self.newest - 1) % Material::COUNT])
    }

    pub(crate) const fn knows_design(self, design: Design) -> bool {
        self.designs & design as u8 != 0
    }

    /// It built `design`, or watched it built.
    pub(crate) fn learn_design(&mut self, design: Design) {
        self.designs |= design as u8;
    }

    /// Whether it remembers trying `attempt` for nothing.
    pub(crate) fn tried(self, attempt: Attempt) -> bool {
        self.tried.contains(&attempt)
    }

    /// It tried `attempt` and nothing came of it.
    pub(crate) fn failed(&mut self, attempt: Attempt) {
        self.tried.rotate_right(1);
        self.tried[0] = attempt;
    }

    pub(crate) fn set_pending(&mut self, attempt: Attempt) {
        self.pending = attempt;
    }

    pub(crate) fn take_pending(&mut self) -> Attempt {
        std::mem::take(&mut self.pending)
    }

    /// Something worth trying with `carried` kinds by one of `methods`: a
    /// combination it hasn't tried and doesn't already know makes something.
    /// `roll` picks among them.
    pub(crate) fn untried(
        self,
        carried: &[Material],
        methods: &[Method],
        roll: u64,
    ) -> Option<Attempt> {
        pick(&self.options(carried, methods, &[]), roll)
    }

    /// As `untried`, among the combinations that use everything in `with`
    /// (an edge is used by working with it)
    /// and, of those, the ones with the fewest kinds of thing: a tinkerer
    /// tries the simple things first.
    pub(crate) fn untried_with(
        self,
        carried: &[Material],
        methods: &[Method],
        with: &[Material],
        roll: u64,
    ) -> Option<Attempt> {
        let options = self.options(carried, methods, with);
        let size = |attempt: &Attempt| attempt.kinds().iter().flatten().count();
        let fewest = options.iter().map(size).min()?;
        let simplest: Vec<Attempt> = options
            .into_iter()
            .filter(|attempt| size(attempt) == fewest)
            .collect();
        pick(&simplest, roll)
    }

    /// Combinations of `carried` using everything in `with`, by any of
    /// `methods`, that it hasn't tried and doesn't know make something.
    fn options(self, carried: &[Material], methods: &[Method], with: &[Material]) -> Vec<Attempt> {
        let mut options = Vec::new();
        let count = carried.len();
        for first in 0..count {
            for second in first..=count {
                for third in second..=count {
                    // `count` stands for "nothing more"; repeats collapse.
                    let pick = |index: usize| carried.get(index).copied();
                    let kinds = [
                        pick(first),
                        (second > first).then(|| pick(second)).flatten(),
                        (third > second).then(|| pick(third)).flatten(),
                    ];
                    if second == count && third != count {
                        continue;
                    }
                    for &method in methods {
                        // Working with an edge uses the edge.
                        let uses = |kind: &Material| {
                            kinds.contains(&Some(*kind))
                                || method == Method::Edge && kind.properties().cutting
                        };
                        if !with.iter().all(uses) {
                            continue;
                        }
                        let attempt = Attempt::new(kinds, method);
                        let known = attempt.recipe().is_some_and(|(index, _)| self.knows(index));
                        if !known && !self.tried(attempt) && !options.contains(&attempt) {
                            options.push(attempt);
                        }
                    }
                }
            }
        }
        options
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
    fn trying_finds_what_physics_allows_and_remembers_what_failed() {
        let mut crafts = Crafts::default();
        let carried = [Material::Meat, Material::Stone];
        let methods = [Method::Hand, Method::Smoke];
        let mut found = None;
        for roll in 0..20 {
            let Some(attempt) = crafts.untried(&carried, &methods, roll) else {
                break;
            };
            match attempt.recipe() {
                Some((index, recipe)) => {
                    crafts.learn(index);
                    found = Some(recipe.output);
                }
                None => crafts.failed(attempt),
            }
        }
        assert!(crafts.knows(4) || found.is_some(), "smoking meat was found");
        let smoked = Attempt::new([Some(Material::Meat), None, None], Method::Smoke);
        assert_eq!(smoked.kinds(), [Some(Material::Meat), None, None]);
        assert_eq!(smoked.method(), Method::Smoke);
        assert_eq!(
            smoked.recipe().map(|(_, recipe)| recipe.output),
            Some(Material::DriedMeat)
        );
        let nothing = Attempt::new(
            [Some(Material::Meat), Some(Material::Stone), None],
            Method::Hand,
        );
        let axe = Attempt::new(
            [
                Some(Material::Cord),
                Some(Material::Wood),
                Some(Material::Blade),
            ],
            Method::Hand,
        );
        assert_eq!(
            axe.recipe().map(|(_, recipe)| recipe.output),
            Some(Material::Axe)
        );
        crafts.failed(nothing);
        assert!(crafts.tried(nothing));
    }

    #[test]
    fn a_tinkerer_starts_simple_from_what_it_has_and_its_newest_thing() {
        let mut crafts = Crafts::default();
        let carried = [Material::Wood, Material::Blade, Material::Hide];
        let methods = [Method::Hand, Method::Edge];
        // After a better cutter: the edge alone first, worked on one thing.
        let first = crafts
            .untried_with(&carried, &methods, &[Material::Blade], 0)
            .unwrap();
        assert_eq!(first.kinds().iter().flatten().count(), 1);
        let mut cord = None;
        for roll in 0..20 {
            let Some(attempt) = crafts.untried_with(&carried, &methods, &[Material::Hide], roll)
            else {
                break;
            };
            match attempt.recipe() {
                Some((index, recipe)) => {
                    crafts.learn(index);
                    cord = Some(recipe.output);
                    break;
                }
                None => crafts.failed(attempt),
            }
        }
        assert_eq!(cord, Some(Material::Cord), "cutting the hide makes cord");
        assert_eq!(crafts.newest(), Some(Material::Cord));
        // With cord, the edge and wood, the axe is among the first tries.
        let carried = [Material::Wood, Material::Blade, Material::Cord];
        let with = [Material::Blade, Material::Cord];
        let mut tries = 0;
        while let Some(attempt) = crafts.untried_with(&carried, &methods, &with, tries) {
            tries += 1;
            if attempt.recipe().is_some() {
                break;
            }
            crafts.failed(attempt);
        }
        assert!(tries <= 6, "found within {tries} tries");
    }

    #[test]
    fn one_family_keeps_fire_and_others_learn_by_watching() {
        assert_eq!(std::mem::size_of::<Crafts>(), 80);
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
        assert!(
            first.knows_huts() && second.knows_huts(),
            "every founder builds huts"
        );
        assert!(!child.knows_huts(), "children have to learn it");
        child.saw_building();
        assert!(child.knows_huts());
        assert!(!child.knows_planting(), "nobody starts out farming");
        child.saw_planting();
        assert!(child.knows_planting());
        assert!(!child.knows_burial());
        child.saw_burial();
        assert!(child.knows_burial());
        assert_eq!(std::mem::size_of::<Crafts>(), 80);
    }
}
