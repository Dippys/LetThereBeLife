//! What an agent has learned about its own choices (docs/plans/LEARNING.md):
//! how well each kind of choice has gone in each kind of situation, judged by
//! how much discomfort it felt before and after, and what each kind of choice
//! tends to bring about (a fire burning, a shelter at hand), so it can see
//! that one step leads to another that helps.

use crate::{PhysicalGoal, PolicyReason};

/// Habits kept per agent.
pub const HABIT_SLOTS: usize = 32;

/// How the agent is, in a byte: which needs are past their thresholds, the
/// season, and whether a shelter or a fire is in view.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Situation(pub u8);

impl Situation {
    pub const HUNGRY: u8 = 1;
    pub const THIRSTY: u8 = 2;
    pub const TIRED: u8 = 4;
    pub const COLD: u8 = 8;
    pub const SHELTER_IN_VIEW: u8 = 64;
    pub const FIRE_IN_VIEW: u8 = 128;

    pub(crate) fn new(
        needs: [bool; 4],
        season: crate::Season,
        shelter_in_view: bool,
        fire_in_view: bool,
    ) -> Self {
        let [hungry, thirsty, tired, cold] = needs;
        let flag = |on: bool, bit: u8| if on { bit } else { 0 };
        let mut bits = flag(hungry, Self::HUNGRY)
            | flag(thirsty, Self::THIRSTY)
            | flag(tired, Self::TIRED)
            | flag(cold, Self::COLD);
        bits |= (season as u8) << 4;
        bits |= flag(shelter_in_view, Self::SHELTER_IN_VIEW);
        bits |= flag(fire_in_view, Self::FIRE_IN_VIEW);
        Self(bits)
    }

    pub const fn has(self, flag: u8) -> bool {
        self.0 & flag != 0
    }

    pub const fn season(self) -> crate::Season {
        crate::Season::ALL[((self.0 >> 4) & 3) as usize]
    }
}

/// The kinds of choice an agent learns about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum Choice {
    Drink,
    Eat,
    /// Going for food, or picking it.
    Forage,
    /// Gathering wood, stone, or anything not eaten.
    Gather,
    Store,
    Fetch,
    Sleep,
    WarmUp,
    TendFire,
    Build,
    Craft,
    Hunt,
    /// Heading for a remembered or pointed-out place.
    Travel,
    /// Visiting, following, asking, pointing things out.
    Company,
    Explore,
    Flee,
    Unload,
    Rest,
}

impl Choice {
    pub const COUNT: usize = 18;
    pub const ALL: [Self; Self::COUNT] = [
        Self::Drink,
        Self::Eat,
        Self::Forage,
        Self::Gather,
        Self::Store,
        Self::Fetch,
        Self::Sleep,
        Self::WarmUp,
        Self::TendFire,
        Self::Build,
        Self::Craft,
        Self::Hunt,
        Self::Travel,
        Self::Company,
        Self::Explore,
        Self::Flee,
        Self::Unload,
        Self::Rest,
    ];

    /// The kind of choice a policy selection amounts to.
    pub(crate) const fn of(goal: PhysicalGoal, reason: PolicyReason) -> Self {
        match goal {
            PhysicalGoal::SeekWater | PhysicalGoal::Drink => Self::Drink,
            PhysicalGoal::Eat => Self::Eat,
            PhysicalGoal::SeekFood => Self::Forage,
            PhysicalGoal::GatherMaterial => match reason {
                PolicyReason::HungerThreshold | PolicyReason::PrepareTrip => Self::Forage,
                _ => Self::Gather,
            },
            PhysicalGoal::Store => Self::Store,
            PhysicalGoal::Fetch => Self::Fetch,
            PhysicalGoal::Sleep | PhysicalGoal::SeekShelter => Self::Sleep,
            PhysicalGoal::WarmUp => Self::WarmUp,
            PhysicalGoal::TendFire => Self::TendFire,
            PhysicalGoal::BuildShelter | PhysicalGoal::BuildHearth => Self::Build,
            PhysicalGoal::Craft => Self::Craft,
            PhysicalGoal::Hunt => Self::Hunt,
            PhysicalGoal::Drop => Self::Unload,
            PhysicalGoal::Signal => Self::Company,
            PhysicalGoal::Explore => match reason {
                // Walking over to a fire is part of feeding it or warming at it.
                PolicyReason::Tending => Self::TendFire,
                PolicyReason::Warming => Self::WarmUp,
                PolicyReason::RememberedPlace
                | PolicyReason::ToldPlace
                | PolicyReason::Fetching
                | PolicyReason::Storing
                | PolicyReason::HearthMaterials
                | PolicyReason::ShelterMaterials => Self::Travel,
                PolicyReason::Visiting | PolicyReason::Following | PolicyReason::Begging => {
                    Self::Company
                }
                PolicyReason::Fleeing => Self::Flee,
                PolicyReason::Hunting | PolicyReason::Recruiting => Self::Hunt,
                _ => Self::Explore,
            },
            PhysicalGoal::Wait | PhysicalGoal::Incapacitated => Self::Rest,
        }
    }
}

/// One learned habit: 4 bytes.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[repr(C)]
struct Habit {
    situation: u8,
    /// `Choice as u8 + 1`; 0 marks a free slot.
    choice: u8,
    /// How much relief it brought, on average (positive is good).
    value: i8,
    tries: u8,
}

/// A habit as tools and the viewer see it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HabitView {
    pub situation: Situation,
    pub choice: Choice,
    pub value: i8,
    pub tries: u8,
}

/// Changes in its circumstances a choice can bring about, as its situation
/// shows them.
pub const EFFECTS: [u8; 2] = [Situation::FIRE_IN_VIEW, Situation::SHELTER_IN_VIEW];

/// How often a kind of choice brought something about: of the `tries` that
/// started without it, `hits` ended with it.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[repr(C)]
struct Effect {
    hits: u8,
    tries: u8,
}

/// What a kind of choice tends to bring about, as tools and the viewer see it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EffectView {
    pub choice: Choice,
    /// The situation flag it brought about (one of [`EFFECTS`]).
    pub effect: u8,
    pub hits: u8,
    pub tries: u8,
}

/// The decision being tried, to judge once the next decision comes.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[repr(C)]
pub(crate) struct Trial {
    situation: u8,
    /// `Choice as u8 + 1`; 0 when nothing is being tried.
    choice: u8,
    discomfort: u16,
}

/// 128 bytes of habits, the trial in progress, and 72 bytes of what choices
/// bring about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Habits {
    habits: [Habit; HABIT_SLOTS],
    trial: Trial,
    effects: [[Effect; EFFECTS.len()]; Choice::COUNT],
}

impl Default for Habits {
    fn default() -> Self {
        Self {
            habits: [Habit::default(); HABIT_SLOTS],
            trial: Trial::default(),
            effects: [[Effect::default(); EFFECTS.len()]; Choice::COUNT],
        }
    }
}

/// Scale from discomfort points to a habit's value.
const RELIEF_SCALE: i32 = 4;

impl Habits {
    /// Judges the choice being tried by how discomfort changed since, then
    /// starts trying `choice` in `situation`.
    /// Re-deciding the same thing in the same situation (each step of a walk,
    /// a retry) continues the trial: it's judged only when something else is chosen.
    pub(crate) fn decided(&mut self, situation: Situation, choice: Choice, discomfort: u16) {
        if self.trial.choice == choice as u8 + 1 && self.trial.situation == situation.0 {
            return;
        }
        if self.trial.choice != 0 {
            let relief = (i32::from(self.trial.discomfort) - i32::from(discomfort)) / RELIEF_SCALE;
            self.learn(self.trial.situation, self.trial.choice, relief);
            self.notice(self.trial.situation, self.trial.choice, situation.0);
        }
        self.trial = Trial {
            situation: situation.0,
            choice: choice as u8 + 1,
            discomfort,
        };
    }

    fn learn(&mut self, situation: u8, choice: u8, relief: i32) {
        let relief = relief.clamp(i32::from(i8::MIN), i32::from(i8::MAX));
        let slot = self
            .habits
            .iter()
            .position(|habit| habit.situation == situation && habit.choice == choice)
            .or_else(|| self.habits.iter().position(|habit| habit.choice == 0))
            .unwrap_or_else(|| {
                // Full: replace the habit with the least experience behind it.
                (0..HABIT_SLOTS)
                    .min_by_key(|&slot| (self.habits[slot].tries, slot))
                    .unwrap_or(0)
            });
        let habit = &mut self.habits[slot];
        if habit.situation != situation || habit.choice != choice {
            *habit = Habit {
                situation,
                choice,
                value: relief as i8,
                tries: 1,
            };
            return;
        }
        // Recent experience counts most: a moving average over about 4 tries.
        let value = i32::from(habit.value);
        habit.value = (value + (relief - value) / 4) as i8;
        habit.tries = habit.tries.saturating_add(1);
    }

    /// Notes what the choice tried from `before` brought about by `after`.
    fn notice(&mut self, before: u8, choice: u8, after: u8) {
        let effects = &mut self.effects[usize::from(choice - 1) % Choice::COUNT];
        for (effect, flag) in effects.iter_mut().zip(EFFECTS) {
            if before & flag != 0 {
                continue;
            }
            if effect.tries == u8::MAX {
                // Keep the proportion, let new experience count.
                effect.tries /= 2;
                effect.hits /= 2;
            }
            effect.tries += 1;
            effect.hits += u8::from(after & flag != 0);
        }
    }

    /// What it expects `choice` to lead to in `situation`: for each thing the
    /// choice has brought about before, how much better it has fared once
    /// that was so, by how often the choice brought it about. Halved, as the
    /// benefit comes a step later.
    pub(crate) fn foresight(&self, situation: Situation, choice: Choice) -> i32 {
        let effects = &self.effects[choice as usize];
        effects
            .iter()
            .zip(EFFECTS)
            .filter(|&(effect, flag)| effect.hits > 0 && !situation.has(flag))
            .map(|(effect, flag)| {
                let gain = self.worth(Situation(situation.0 | flag)) - self.worth(situation);
                gain.max(0) * i32::from(effect.hits) / (i32::from(effect.tries) + 1) / 2
            })
            .sum()
    }

    /// How well it can do in `situation`: the best it has fared there.
    fn worth(&self, situation: Situation) -> i32 {
        self.habits
            .iter()
            .filter(|habit| habit.choice != 0 && habit.situation == situation.0)
            .map(|habit| i32::from(habit.value))
            .max()
            .unwrap_or(0)
            .max(0)
    }

    /// How `choice` has gone in `situation`: its value and how often it was tried.
    pub(crate) fn value(&self, situation: Situation, choice: Choice) -> (i8, u8) {
        self.habits
            .iter()
            .find(|habit| habit.situation == situation.0 && habit.choice == choice as u8 + 1)
            .map_or((0, 0), |habit| (habit.value, habit.tries))
    }

    pub(crate) fn views(&self) -> impl Iterator<Item = HabitView> + '_ {
        self.habits
            .iter()
            .filter(|habit| habit.choice != 0)
            .map(|habit| HabitView {
                situation: Situation(habit.situation),
                choice: Choice::ALL[usize::from(habit.choice - 1) % Choice::COUNT],
                value: habit.value,
                tries: habit.tries,
            })
    }

    pub(crate) fn effect_views(&self) -> impl Iterator<Item = EffectView> + '_ {
        Choice::ALL.into_iter().flat_map(move |choice| {
            self.effects[choice as usize]
                .iter()
                .zip(EFFECTS)
                .filter(|(effect, _)| effect.tries > 0)
                .map(move |(effect, flag)| EffectView {
                    choice,
                    effect: flag,
                    hits: effect.hits,
                    tries: effect.tries,
                })
        })
    }
}

/// How uncomfortable the agent is: for each need past half its threshold, how
/// far past, in hundredths of the threshold.
pub(crate) fn discomfort(levels: [(u16, u16); 4]) -> u16 {
    levels
        .into_iter()
        .map(|(value, threshold)| {
            let comfortable = threshold / 2;
            u32::from(value.saturating_sub(comfortable)) * 100 / u32::from(threshold.max(1))
        })
        .sum::<u32>()
        .min(u32::from(u16::MAX)) as u16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_choice_that_eases_discomfort_is_valued_and_one_that_doesnt_is_not() {
        let mut habits = Habits::default();
        let cold = Situation::new(
            [false, false, false, true],
            crate::Season::Winter,
            true,
            false,
        );
        for _ in 0..6 {
            // Sleeping takes the agent from 200 to 120; wandering off then
            // brings it back up to 160; resting a while, up again to 200.
            habits.decided(cold, Choice::Sleep, 200);
            habits.decided(cold, Choice::Explore, 120);
            habits.decided(cold, Choice::Rest, 160);
        }
        let value = |choice: Choice| {
            habits
                .views()
                .find(|habit| habit.situation == cold && habit.choice == choice)
                .map(|habit| habit.value)
        };
        assert!(value(Choice::Sleep) > Some(10), "sleeping eased it");
        assert!(
            value(Choice::Explore) < Some(0),
            "wandering off made it worse"
        );
        assert!(cold.has(Situation::COLD) && cold.has(Situation::SHELTER_IN_VIEW));
        assert_eq!(cold.season(), crate::Season::Winter);
        assert_eq!(
            std::mem::size_of::<Habits>(),
            4 * HABIT_SLOTS + 4 + 2 * EFFECTS.len() * Choice::COUNT
        );
    }

    #[test]
    fn a_step_that_brings_about_what_helps_is_worth_taking() {
        let mut habits = Habits::default();
        let cold = Situation::new(
            [false, false, false, true],
            crate::Season::Winter,
            false,
            false,
        );
        let by_fire = Situation(cold.0 | Situation::FIRE_IN_VIEW);
        for _ in 0..6 {
            // Feeding the dead fire eases nothing by itself, but gets it
            // burning; warming at it then eases the cold. Wandering off does neither.
            habits.decided(cold, Choice::TendFire, 200);
            habits.decided(by_fire, Choice::WarmUp, 200);
            habits.decided(cold, Choice::Explore, 120);
            habits.decided(cold, Choice::Rest, 200);
        }
        assert!(
            habits.value(cold, Choice::TendFire).0 <= 0,
            "no relief by itself"
        );
        assert!(
            habits.foresight(cold, Choice::TendFire) > 4,
            "it leads to a fire that warms"
        );
        assert_eq!(habits.foresight(cold, Choice::Explore), 0);
        assert_eq!(
            habits.foresight(by_fire, Choice::TendFire),
            0,
            "already burning: nothing more to bring about"
        );
        let fire = habits
            .effect_views()
            .find(|view| view.choice == Choice::TendFire && view.effect == Situation::FIRE_IN_VIEW)
            .expect("noticed");
        assert_eq!((fire.hits, fire.tries), (6, 6));
    }

    #[test]
    fn discomfort_counts_only_needs_past_comfortable() {
        assert_eq!(discomfort([(0, 7_000); 4]), 0);
        assert_eq!(
            discomfort([(7_000, 7_000), (0, 6_000), (0, 6_000), (0, 7_000)]),
            50
        );
    }
}
