//! What an agent has learned about its own choices (docs/plans/LEARNING.md):
//! how well each kind of choice has gone in each kind of situation, judged by
//! how much discomfort it felt before and after. Phase 1 only learns; the
//! scripted policy still decides.

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
                PolicyReason::RememberedPlace
                | PolicyReason::ToldPlace
                | PolicyReason::Fetching
                | PolicyReason::Storing
                | PolicyReason::Tending
                | PolicyReason::Warming
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
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
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

/// The decision being tried, to judge once the next decision comes.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub(crate) struct Trial {
    situation: u8,
    /// `Choice as u8 + 1`; 0 when nothing is being tried.
    choice: u8,
    discomfort: u16,
}

/// 128 bytes of habits plus the trial in progress.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Habits {
    habits: [Habit; HABIT_SLOTS],
    trial: Trial,
}

impl Default for Habits {
    fn default() -> Self {
        Self {
            habits: [Habit::default(); HABIT_SLOTS],
            trial: Trial::default(),
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
        assert_eq!(std::mem::size_of::<Habits>(), 4 * HABIT_SLOTS + 4);
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
