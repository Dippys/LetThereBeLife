//! What an agent believes its actions need and bring about (docs/plans/THINKING.md):
//! a small cause-and-effect model learned from its own actions and from
//! watching others, over the facts it perceives about itself and what's at
//! hand. Plans are searched through this model, never through the world.

use super::habits::Choice;

/// What an agent perceives about its circumstances, in 16 bits.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Facts(pub u16);

impl Facts {
    pub const HUNGRY: u16 = 1;
    pub const THIRSTY: u16 = 1 << 1;
    pub const TIRED: u16 = 1 << 2;
    pub const COLD: u16 = 1 << 3;
    /// Drinkable water in view.
    pub const WATER_NEAR: u16 = 1 << 4;
    /// Something it believes is food, to pick or chop, in view.
    pub const FOOD_NEAR: u16 = 1 << 5;
    /// A burning fire in view.
    pub const FIRE: u16 = 1 << 6;
    /// A finished hut in view.
    pub const SHELTER: u16 = 1 << 7;
    /// A fire in view that is out or burning low.
    pub const DEAD_FIRE: u16 = 1 << 8;
    /// Wood to gather in view.
    pub const WOOD_NEAR: u16 = 1 << 9;
    /// Stone to gather in view.
    pub const STONE_NEAR: u16 = 1 << 10;
    pub const HAS_FOOD: u16 = 1 << 11;
    pub const HAS_WOOD: u16 = 1 << 12;
    pub const HAS_STONE: u16 = 1 << 13;
    pub const HAS_BLADE: u16 = 1 << 14;
    /// A hut in view with food stored in it.
    pub const STORE_NEAR: u16 = 1 << 15;

    /// The four needs, in `NeedKind` order.
    pub const NEEDS: [u16; 4] = [Self::HUNGRY, Self::THIRSTY, Self::TIRED, Self::COLD];

    pub const fn has(self, fact: u16) -> bool {
        self.0 & fact == fact
    }

    /// Plain-language names, for traces and the viewer.
    pub const fn name(fact: u16) -> &'static str {
        match fact {
            Self::HUNGRY => "hungry",
            Self::THIRSTY => "thirsty",
            Self::TIRED => "tired",
            Self::COLD => "cold",
            Self::WATER_NEAR => "water at hand",
            Self::FOOD_NEAR => "food at hand",
            Self::FIRE => "a fire burning",
            Self::SHELTER => "a hut at hand",
            Self::DEAD_FIRE => "a fire gone out",
            Self::WOOD_NEAR => "wood at hand",
            Self::STONE_NEAR => "stone at hand",
            Self::HAS_FOOD => "carrying food",
            Self::HAS_WOOD => "carrying wood",
            Self::HAS_STONE => "carrying stone",
            Self::HAS_BLADE => "carrying a blade",
            Self::STORE_NEAR => "stored food at hand",
            _ => "?",
        }
    }
}

/// The facts an action can bring about or need (not the needs it eases:
/// relief is learned apart). Being "at hand" comes from walking somewhere,
/// so plans reach those by going to a remembered place; carrying, a fire
/// burning, a hut built come from actions.
pub const EFFECTS: [u16; 12] = [
    Facts::WATER_NEAR,
    Facts::FOOD_NEAR,
    Facts::FIRE,
    Facts::SHELTER,
    Facts::DEAD_FIRE,
    Facts::WOOD_NEAR,
    Facts::STONE_NEAR,
    Facts::STORE_NEAR,
    Facts::HAS_FOOD,
    Facts::HAS_WOOD,
    Facts::HAS_STONE,
    Facts::HAS_BLADE,
];

/// Tries (high nibble) and how many of them held (low nibble).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct Odds(u8);

impl Odds {
    const fn tries(self) -> u8 {
        self.0 >> 4
    }

    const fn hits(self) -> u8 {
        self.0 & 0x0f
    }

    fn note(&mut self, hit: bool) {
        let (mut tries, mut hits) = (self.tries(), self.hits());
        if tries == 15 {
            // Keep the proportion, let new experience count.
            tries = 8;
            hits = hits.div_ceil(2);
        }
        tries += 1;
        hits += u8::from(hit);
        self.0 = tries << 4 | hits.min(tries);
    }

    /// Held every time, on enough evidence (3 or more): a coincidence soon
    /// fails once.
    const fn reliable(self) -> bool {
        self.hits() >= 3 && self.hits() == self.tries()
    }
}

/// What a kind of action is believed to need and do: 29 bytes.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[repr(C)]
struct Belief {
    /// What it brought about that didn't hold before (per `EFFECTS`).
    produces: [Odds; EFFECTS.len()],
    /// Per `EFFECTS`: evidence that the action needs it (it used it up, or
    /// couldn't be done without it, or others were seen using it).
    requires: [Odds; EFFECTS.len()],
    /// How much it eased each need (`NeedKind` order), hundredths of a threshold.
    relieves: [i8; 4],
    /// Average seconds it takes, in units of 4 seconds.
    duration: u8,
}

/// The action being tried, to judge once it's over.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[repr(C)]
struct Attempt {
    started: u32,
    facts: u16,
    /// `Choice as u8 + 1`; 0 when nothing is being tried.
    choice: u8,
    levels: [u8; 4],
}

/// One agent's cause-and-effect model: 29 bytes per kind of action plus the attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Model {
    beliefs: [Belief; Choice::COUNT],
    attempt: Attempt,
}

impl Default for Model {
    fn default() -> Self {
        Self {
            beliefs: [Belief::default(); Choice::COUNT],
            attempt: Attempt::default(),
        }
    }
}

/// A belief about one kind of action, as tools and the viewer see it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BeliefView {
    pub choice: Choice,
    /// Facts it reliably needs.
    pub needs: Facts,
    /// The facts it brought about, with how often (hits, tries).
    pub produces: Vec<(u16, u8, u8)>,
    /// How much it eased each need (hundredths of a threshold).
    pub relieves: [i8; 4],
    pub seconds: u32,
}

/// How far a need is past comfortable at the start and end of an attempt,
/// for judging relief: 0-255 in hundredths of the threshold.
pub(crate) fn need_level(value: u16, threshold: u16) -> u8 {
    (u32::from(value) * 100 / u32::from(threshold.max(1))).min(255) as u8
}

impl Model {
    /// Instincts every newborn and founder has: drinking at water eases thirst,
    /// eating what it carries eases hunger, picking food gets food, sleeping
    /// eases tiredness. Weak, so experience soon outweighs them.
    pub(crate) fn instinct() -> Self {
        let mut model = Self::default();
        let mut innate = |choice: Choice, needs: u16, makes: Option<u16>, eases: Option<usize>| {
            let belief = &mut model.beliefs[choice as usize];
            belief.duration = 4;
            for (odds, fact) in belief.requires.iter_mut().zip(EFFECTS) {
                if needs & fact != 0 {
                    *odds = Odds(3 << 4 | 3);
                }
            }
            if let Some(fact) = makes {
                let index = effect_index(fact).expect("an effect");
                belief.produces[index] = Odds(2 << 4 | 2);
            }
            if let Some(need) = eases {
                belief.relieves[need] = 20;
            }
        };
        innate(Choice::Drink, Facts::WATER_NEAR, None, Some(1));
        innate(Choice::Eat, Facts::HAS_FOOD, None, Some(0));
        innate(
            Choice::Forage,
            Facts::FOOD_NEAR,
            Some(Facts::HAS_FOOD),
            None,
        );
        innate(Choice::Sleep, 0, None, Some(2));
        innate(Choice::Gather, 0, Some(Facts::HAS_WOOD), None);
        model
    }

    /// What knowing a craft means for its actions, for those it hasn't yet
    /// learned anything about itself: keeping fire (a burning fire warms;
    /// fuel on a dead fire lights it), huts (wood makes a hut; a night in one
    /// warms), knapping (stone makes a blade).
    pub(crate) fn know_how(&mut self, fire: bool, huts: bool, knapping: bool) {
        let mut teach = |choice: Choice, needs: u16, makes: Option<u16>, eases: Option<usize>| {
            let belief = &mut self.beliefs[choice as usize];
            let untried = |odds: &[Odds]| odds.iter().all(|odds| odds.tries() == 0);
            if !untried(&belief.requires) || !untried(&belief.produces) {
                return;
            }
            for (odds, fact) in belief.requires.iter_mut().zip(EFFECTS) {
                if needs & fact != 0 {
                    *odds = Odds(3 << 4 | 3);
                }
            }
            if let Some(index) = makes.and_then(effect_index) {
                belief.produces[index] = Odds(3 << 4 | 3);
            }
            if let Some(need) = eases
                && belief.relieves[need] == 0
            {
                belief.relieves[need] = 20;
            }
        };
        if fire {
            teach(Choice::WarmUp, Facts::FIRE, None, Some(3));
            teach(
                Choice::TendFire,
                Facts::HAS_WOOD | Facts::DEAD_FIRE,
                Some(Facts::FIRE),
                None,
            );
        }
        if huts {
            teach(Choice::Build, Facts::HAS_WOOD, Some(Facts::SHELTER), None);
        }
        if knapping {
            teach(
                Choice::Craft,
                Facts::HAS_STONE,
                Some(Facts::HAS_BLADE),
                None,
            );
        }
    }

    /// It found it couldn't do `choice` for want of `fact`: that's needed.
    pub(crate) fn lacked(&mut self, choice: Choice, fact: u16) {
        if let Some(index) = effect_index(fact) {
            self.beliefs[choice as usize].requires[index].note(true);
        }
    }

    /// The agent chose `choice` with `facts` holding and its needs at
    /// `levels`: judges what it was trying before (if anything else), then
    /// starts judging this. Repeating the same choice in the same
    /// circumstances continues the attempt.
    pub(crate) fn decided(&mut self, choice: Choice, facts: Facts, levels: [u8; 4], now: u32) {
        let attempt = self.attempt;
        if attempt.choice == choice as u8 + 1 && attempt.facts == facts.0 {
            return;
        }
        if attempt.choice != 0 {
            self.learn(attempt, facts, levels, now);
        }
        self.attempt = Attempt {
            started: now,
            facts: facts.0,
            choice: choice as u8 + 1,
            levels,
        };
    }

    fn learn(&mut self, attempt: Attempt, after: Facts, levels: [u8; 4], now: u32) {
        let index = usize::from(attempt.choice - 1) % Choice::COUNT;
        let before = Facts(attempt.facts);
        let belief = &mut self.beliefs[index];
        for (odds, fact) in belief.produces.iter_mut().zip(EFFECTS) {
            if !before.has(fact) {
                odds.note(after.has(fact));
            }
        }
        // What it needs isn't learned from what happened to be around, but
        // from evidence: what it used up, what it found it couldn't do
        // without, and what it saw others use (see `used`, `lacked`, `saw`).
        for (need, eased) in belief.relieves.iter_mut().enumerate() {
            if !before.has(Facts::NEEDS[need]) {
                continue;
            }
            let relief = i32::from(attempt.levels[need]) - i32::from(levels[need]);
            let old = i32::from(*eased);
            *eased = (old + (relief.clamp(-127, 127) - old) / 4) as i8;
        }
        let seconds = (now.saturating_sub(attempt.started).min(1_020) / 4) as i32;
        let old = i32::from(belief.duration);
        belief.duration = if old == 0 {
            seconds
        } else {
            old + (seconds - old) / 4
        }
        .clamp(0, 255) as u8;
    }

    /// Doing `choice` used up what it carried of `fact` (some of it): felt in
    /// its own hands.
    pub(crate) fn used(&mut self, choice: Choice, fact: u16) {
        if let Some(index) = effect_index(fact) {
            self.beliefs[choice as usize].requires[index].note(true);
        }
    }

    /// Someone it watched do `choice`, working on or using up `needs`,
    /// brought `fact` about.
    pub(crate) fn saw(&mut self, choice: Choice, needs: u16, fact: u16) {
        let belief = &mut self.beliefs[choice as usize];
        for (odds, effect) in belief.requires.iter_mut().zip(EFFECTS) {
            if needs & effect != 0 {
                odds.note(true);
            }
        }
        if let Some(index) = effect_index(fact) {
            belief.produces[index].note(true);
        }
    }

    /// How often `choice` has brought `fact` about: (hits, tries).
    pub(crate) fn odds(&self, choice: Choice, fact: u16) -> (u8, u8) {
        effect_index(fact).map_or((0, 0), |index| {
            let odds = self.beliefs[choice as usize].produces[index];
            (odds.hits(), odds.tries())
        })
    }

    /// What `choice` is reliably believed to need.
    pub(crate) fn needs(&self, choice: Choice) -> Facts {
        let belief = &self.beliefs[choice as usize];
        Facts(
            belief
                .requires
                .iter()
                .zip(EFFECTS)
                .filter(|(odds, _)| odds.reliable())
                .fold(0, |mask, (_, fact)| mask | fact),
        )
    }

    /// How much `choice` has eased `need` (`NeedKind` index), on average.
    pub(crate) fn relief(&self, choice: Choice, need: usize) -> i8 {
        self.beliefs[choice as usize].relieves[need]
    }

    /// Average seconds `choice` takes.
    pub(crate) fn seconds(&self, choice: Choice) -> u32 {
        u32::from(self.beliefs[choice as usize].duration) * 4
    }

    pub(crate) fn views(&self) -> impl Iterator<Item = BeliefView> + '_ {
        Choice::ALL.into_iter().filter_map(move |choice| {
            let belief = &self.beliefs[choice as usize];
            let produces: Vec<(u16, u8, u8)> = belief
                .produces
                .iter()
                .zip(EFFECTS)
                .filter(|(odds, _)| odds.tries() > 0)
                .map(|(odds, fact)| (fact, odds.hits(), odds.tries()))
                .collect();
            let needs = self.needs(choice);
            (needs.0 != 0 || !produces.is_empty() || belief.relieves != [0; 4]).then(|| {
                BeliefView {
                    choice,
                    needs,
                    produces,
                    relieves: belief.relieves,
                    seconds: u32::from(belief.duration) * 4,
                }
            })
        })
    }
}

fn effect_index(fact: u16) -> Option<usize> {
    EFFECTS.iter().position(|&effect| effect == fact)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CALM: [u8; 4] = [0; 4];

    #[test]
    fn feeding_a_dead_fire_is_learned_to_need_wood_and_to_light_it() {
        let mut model = Model::default();
        let cold_by_dead_fire = Facts(Facts::COLD | Facts::DEAD_FIRE | Facts::HAS_WOOD);
        let burning = Facts(Facts::COLD | Facts::FIRE | Facts::HAS_WOOD);
        for round in 0..4 {
            // Sometimes it also carries stone; that turns out not to matter.
            let stone = if round % 2 == 0 { Facts::HAS_STONE } else { 0 };
            model.decided(
                Choice::TendFire,
                Facts(cold_by_dead_fire.0 | stone),
                [0, 0, 0, 120],
                round * 100,
            );
            model.used(Choice::TendFire, Facts::HAS_WOOD);
            model.decided(Choice::WarmUp, burning, [0, 0, 0, 120], round * 100 + 20);
            model.decided(Choice::Rest, Facts(Facts::FIRE), CALM, round * 100 + 80);
        }
        let needs = model.needs(Choice::TendFire);
        assert_eq!(
            needs,
            Facts(Facts::HAS_WOOD),
            "it used up wood, and nothing else"
        );
        // With no fire in sight there was nothing to feed, three times.
        for _ in 0..3 {
            model.lacked(Choice::TendFire, Facts::DEAD_FIRE);
        }
        let needs = model.needs(Choice::TendFire);
        assert!(needs.has(Facts::HAS_WOOD | Facts::DEAD_FIRE), "{needs:?}");
        assert!(!needs.has(Facts::HAS_STONE), "stone isn't needed");
        assert_eq!(model.odds(Choice::TendFire, Facts::FIRE), (4, 4));
        assert!(
            model.relief(Choice::WarmUp, 3) > 0,
            "warming up eased the cold"
        );
        assert_eq!(model.seconds(Choice::TendFire), 20);
        assert_eq!(std::mem::size_of::<Model>(), 29 * Choice::COUNT + 2 + 12);
    }

    #[test]
    fn watching_teaches_what_a_craft_makes_and_instinct_knows_drinking() {
        let mut model = Model::instinct();
        assert_eq!(model.needs(Choice::Drink), Facts(Facts::WATER_NEAR));
        assert!(model.relief(Choice::Drink, 1) > 0);
        assert_eq!(model.needs(Choice::Build), Facts(0));
        for _ in 0..3 {
            model.saw(Choice::Build, Facts::HAS_WOOD, Facts::SHELTER);
        }
        assert_eq!(model.needs(Choice::Build), Facts(Facts::HAS_WOOD));
        assert_eq!(model.odds(Choice::Build, Facts::SHELTER), (3, 3));
    }
}
