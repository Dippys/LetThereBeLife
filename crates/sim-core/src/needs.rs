use crate::{AgentActivity, AgentId, EventId, SimTime, SleepQuality};

pub const NEED_MAX: u16 = 10_000;
pub const NEED_RATE_PERIOD_TICKS: u16 = 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum NeedKind {
    Hunger = 0,
    Thirst = 1,
    Rest = 2,
    Exposure = 3,
}

impl NeedKind {
    pub const ALL: [Self; 4] = [Self::Hunger, Self::Thirst, Self::Rest, Self::Exposure];

    pub(crate) const fn index(self) -> usize {
        self as usize
    }

    pub(crate) const fn mask(self) -> u8 {
        1 << self.index()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NeedLevelView {
    pub value: u16,
    pub rate_per_period: i16,
    pub threshold: u16,
    pub threshold_reached: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NeedThreshold {
    pub kind: NeedKind,
    pub due: SimTime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhysicalNeedsView {
    pub agent: AgentId,
    pub at: SimTime,
    pub hunger: NeedLevelView,
    pub thirst: NeedLevelView,
    pub rest: NeedLevelView,
    pub exposure: NeedLevelView,
    pub next_threshold: Option<NeedThreshold>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NeedQueryError {
    MissingAgent,
    DeadAgent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NeedThresholdOutcomeKind {
    Reached,
    StaleEvent,
    MissingAgent,
    DeadAgent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NeedThresholdEventOutcome {
    pub event: EventId,
    pub agent: AgentId,
    pub due: SimTime,
    pub kind: NeedKind,
    pub value: Option<u16>,
    pub outcome: NeedThresholdOutcomeKind,
}

// Deficit units gained per 60 fixed simulation ticks. Exposure is provisional physical
// pressure; Slice 6 shelter and Slice 7 climate consequences will change it.
const ACTIVITY_RATES: [[i8; 4]; 5] = [
    [2, 4, 1, 0],  // Idle
    [3, 6, 3, 1],  // Moving
    [4, 7, 4, 1],  // Gathering
    [5, 8, 5, 1],  // Building
    [1, 2, -8, 2], // Sleeping without shelter input
];

const THRESHOLDS: [u16; 4] = [7_000, 6_000, 8_000, 7_000];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub(crate) struct NeedState {
    reference_time: SimTime,
    values: [u16; 4],
    generation: u32,
    rates: [i8; 4],
    remainders: [u8; 4],
    crossed: u8,
}

impl NeedState {
    pub(crate) const fn new(reference_time: SimTime) -> Self {
        Self {
            reference_time,
            values: [0; 4],
            generation: 0,
            rates: ACTIVITY_RATES[AgentActivity::Idle as usize],
            remainders: [0; 4],
            crossed: 0,
        }
    }

    pub(crate) const fn generation(self) -> u32 {
        self.generation
    }

    pub(crate) fn requires_transition(self, activity: AgentActivity) -> bool {
        self.rates != rates_for(activity)
    }

    pub(crate) fn event_is_current(self, generation: u32, kind: NeedKind) -> bool {
        self.generation == generation && self.crossed & kind.mask() == 0
    }

    pub(crate) fn transition(&mut self, activity: AgentActivity, now: SimTime) -> bool {
        self.transition_rates(rates_for(activity), now)
    }

    pub(crate) fn transition_sleep(&mut self, quality: SleepQuality, now: SimTime) -> bool {
        self.transition_rates(sleep_rates(quality), now)
    }

    fn transition_rates(&mut self, new_rates: [i8; 4], now: SimTime) -> bool {
        if self.rates == new_rates {
            return false;
        }
        self.rebase(now);
        self.rates = new_rates;
        self.generation = self.generation.wrapping_add(1);
        self.crossed &= crossed_mask(self.values);
        true
    }

    pub(crate) fn sleep_recovery_due(self, quality: SleepQuality, now: SimTime) -> Option<SimTime> {
        let remaining = self.numerator(NeedKind::Rest, now);
        let recovery = u128::from(quality.rest_recovery_per_period());
        let elapsed = remaining.div_ceil(recovery).max(1);
        now.checked_add(u64::try_from(elapsed).ok()?)
    }

    pub(crate) fn threshold_due(self, kind: NeedKind, now: SimTime) -> Option<SimTime> {
        if self.crossed & kind.mask() != 0 {
            return None;
        }
        let current = self.numerator(kind, now);
        let threshold = u128::from(threshold(kind)) * u128::from(NEED_RATE_PERIOD_TICKS);
        if current >= threshold {
            return Some(now);
        }
        let rate = i64::from(self.rates[kind.index()]);
        if rate <= 0 {
            return None;
        }
        let numerator = threshold - current;
        let rate = rate as u128;
        let elapsed = numerator.div_ceil(rate);
        now.checked_add(u64::try_from(elapsed).ok()?)
    }

    pub(crate) fn view(self, agent: AgentId, now: SimTime) -> PhysicalNeedsView {
        let level = |kind| NeedLevelView {
            value: self.value(kind, now),
            rate_per_period: i16::from(self.rates[kind.index()]),
            threshold: threshold(kind),
            threshold_reached: self.crossed & kind.mask() != 0
                || self.value(kind, now) >= threshold(kind),
        };
        let next_threshold = NeedKind::ALL
            .into_iter()
            .filter_map(|kind| {
                self.threshold_due(kind, now)
                    .map(|due| NeedThreshold { kind, due })
            })
            .min_by_key(|next| (next.due, next.kind));
        PhysicalNeedsView {
            agent,
            at: now,
            hunger: level(NeedKind::Hunger),
            thirst: level(NeedKind::Thirst),
            rest: level(NeedKind::Rest),
            exposure: level(NeedKind::Exposure),
            next_threshold,
        }
    }

    pub(crate) fn apply_threshold(
        &mut self,
        generation: u32,
        kind: NeedKind,
        due: SimTime,
    ) -> (NeedThresholdOutcomeKind, u16) {
        let value = self.value(kind, due);
        if !self.event_is_current(generation, kind) || value < threshold(kind) {
            return (NeedThresholdOutcomeKind::StaleEvent, value);
        }
        self.crossed |= kind.mask();
        (NeedThresholdOutcomeKind::Reached, value)
    }

    pub(crate) fn relieve(&mut self, kind: NeedKind, amount: u16, now: SimTime) {
        self.rebase(now);
        let index = kind.index();
        let numerator = u32::from(self.values[index]) * u32::from(NEED_RATE_PERIOD_TICKS)
            + u32::from(self.remainders[index]);
        let relieved =
            numerator.saturating_sub(u32::from(amount) * u32::from(NEED_RATE_PERIOD_TICKS));
        self.values[index] = (relieved / u32::from(NEED_RATE_PERIOD_TICKS)) as u16;
        self.remainders[index] = (relieved % u32::from(NEED_RATE_PERIOD_TICKS)) as u8;
        self.generation = self.generation.wrapping_add(1);
        self.crossed = crossed_mask(self.values);
    }

    fn value(self, kind: NeedKind, now: SimTime) -> u16 {
        (self.numerator(kind, now) / u128::from(NEED_RATE_PERIOD_TICKS)) as u16
    }

    fn numerator(self, kind: NeedKind, now: SimTime) -> u128 {
        let elapsed = now.ticks().saturating_sub(self.reference_time.ticks());
        let base = i128::from(self.values[kind.index()]) * i128::from(NEED_RATE_PERIOD_TICKS)
            + i128::from(self.remainders[kind.index()]);
        let change = i128::from(self.rates[kind.index()]) * i128::from(elapsed);
        (base + change).clamp(0, i128::from(NEED_MAX) * i128::from(NEED_RATE_PERIOD_TICKS)) as u128
    }

    fn rebase(&mut self, now: SimTime) {
        for kind in NeedKind::ALL {
            let numerator = self.numerator(kind, now);
            self.values[kind.index()] = (numerator / u128::from(NEED_RATE_PERIOD_TICKS)) as u16;
            self.remainders[kind.index()] = (numerator % u128::from(NEED_RATE_PERIOD_TICKS)) as u8;
        }
        self.reference_time = now;
    }

    #[cfg(test)]
    pub(crate) fn set_value_for_test(&mut self, kind: NeedKind, value: u16, now: SimTime) {
        self.rebase(now);
        self.values[kind.index()] = value.min(NEED_MAX);
        self.remainders[kind.index()] = 0;
        self.crossed = crossed_mask(self.values);
    }
}

const fn rates_for(activity: AgentActivity) -> [i8; 4] {
    match activity {
        AgentActivity::Idle => ACTIVITY_RATES[0],
        AgentActivity::Moving => ACTIVITY_RATES[1],
        AgentActivity::Gathering => ACTIVITY_RATES[2],
        AgentActivity::Building => ACTIVITY_RATES[3],
        AgentActivity::Sleeping => sleep_rates(SleepQuality::OpenGround),
        AgentActivity::Dead => [0; 4],
    }
}

const fn sleep_rates(quality: SleepQuality) -> [i8; 4] {
    let mut rates = ACTIVITY_RATES[4];
    rates[NeedKind::Rest as usize] = -(quality.rest_recovery_per_period() as i8);
    rates
}

const fn threshold(kind: NeedKind) -> u16 {
    THRESHOLDS[kind.index()]
}

fn crossed_mask(values: [u16; 4]) -> u8 {
    NeedKind::ALL.into_iter().fold(0, |mask, kind| {
        mask | if values[kind.index()] >= threshold(kind) {
            kind.mask()
        } else {
            0
        }
    })
}

#[cfg(test)]
mod tests {
    use std::mem::{align_of, size_of};

    use super::*;

    #[test]
    fn compact_state_and_exact_interpolation_are_fixed() {
        assert_eq!(size_of::<NeedState>(), 32);
        assert_eq!(align_of::<NeedState>(), 8);
        let state = NeedState::new(SimTime::from_ticks(10));
        let view = state.view(AgentId::new(0), SimTime::from_ticks(70));
        assert_eq!(view.hunger.value, 2);
        assert_eq!(view.thirst.value, 4);
        assert_eq!(view.rest.value, 1);
        assert_eq!(view.exposure.value, 0);
    }

    #[test]
    fn prediction_uses_ceiling_and_saturation_is_chunking_independent() {
        let state = NeedState::new(SimTime::ZERO);
        assert_eq!(
            state.threshold_due(NeedKind::Thirst, SimTime::ZERO),
            Some(SimTime::from_ticks(90_000))
        );
        let direct = state.view(AgentId::new(0), SimTime::from_ticks(900_000));
        for checkpoint in [60, 6_000, 90_000, 900_000] {
            let stepped = state.view(AgentId::new(0), SimTime::from_ticks(checkpoint));
            assert_eq!(
                stepped.thirst.value,
                ((4_u64 * checkpoint / 60).min(u64::from(NEED_MAX))) as u16
            );
        }
        assert_eq!(direct.thirst.value, NEED_MAX);
    }

    #[test]
    fn activity_transition_rebases_and_sleep_recovers_rest() {
        let mut state = NeedState::new(SimTime::ZERO);
        assert!(state.transition(AgentActivity::Moving, SimTime::from_ticks(61)));
        let moving = state.view(AgentId::new(0), SimTime::from_ticks(121));
        assert_eq!(moving.hunger.value, 5);
        assert_eq!(moving.rest.value, 4);
        state.values[NeedKind::Rest.index()] = 100;
        state.reference_time = SimTime::from_ticks(121);
        state.remainders[NeedKind::Rest.index()] = 0;
        state.transition(AgentActivity::Sleeping, SimTime::from_ticks(121));
        assert_eq!(
            state
                .view(AgentId::new(0), SimTime::from_ticks(721))
                .rest
                .value,
            20
        );
        assert_eq!(
            state
                .view(AgentId::new(0), SimTime::from_ticks(10_000))
                .rest
                .value,
            0
        );
    }

    #[test]
    fn sleep_recovery_due_is_exact_for_quality_and_tick_batching() {
        let mut state = NeedState::new(SimTime::ZERO);
        state.values[NeedKind::Rest.index()] = 8_000;
        let open_due = state
            .sleep_recovery_due(SleepQuality::OpenGround, SimTime::ZERO)
            .unwrap();
        let sheltered_due = state
            .sleep_recovery_due(SleepQuality::Sheltered, SimTime::ZERO)
            .unwrap();
        assert_eq!(open_due, SimTime::from_ticks(60_000));
        assert_eq!(sheltered_due, SimTime::from_ticks(40_000));

        state.transition_sleep(SleepQuality::OpenGround, SimTime::ZERO);
        assert_eq!(state.view(AgentId::new(0), open_due).rest.value, 0);
        assert_eq!(
            state
                .view(AgentId::new(0), SimTime::from_ticks(open_due.ticks() - 1))
                .rest
                .value,
            0
        );
        assert_eq!(
            state.sleep_recovery_due(
                SleepQuality::OpenGround,
                SimTime::from_ticks(open_due.ticks() - 1),
            ),
            Some(open_due)
        );
    }

    #[test]
    fn provisional_activity_profiles_and_prediction_edges_are_explicit() {
        let mut state = NeedState::new(SimTime::ZERO);
        for (activity, expected) in [
            (AgentActivity::Moving, [3, 6, 3, 1]),
            (AgentActivity::Gathering, [4, 7, 4, 1]),
            (AgentActivity::Building, [5, 8, 5, 1]),
            (AgentActivity::Sleeping, [1, 2, -8, 2]),
        ] {
            state.transition(activity, state.reference_time);
            assert_eq!(state.rates, expected);
        }
        assert_eq!(
            NeedState::new(SimTime::ZERO).threshold_due(NeedKind::Exposure, SimTime::ZERO),
            None
        );
        state.reference_time = SimTime::from_ticks(u64::MAX - 1);
        state.values = [0; 4];
        state.remainders = [0; 4];
        state.rates = ACTIVITY_RATES[0];
        assert_eq!(
            state.threshold_due(NeedKind::Thirst, SimTime::from_ticks(u64::MAX - 1)),
            None
        );
    }

    #[test]
    fn activity_change_cannot_swallow_an_unprocessed_crossing() {
        let due = SimTime::from_ticks(90_000);
        let mut state = NeedState::new(SimTime::ZERO);
        state.transition(AgentActivity::Moving, due);
        assert_eq!(state.threshold_due(NeedKind::Thirst, due), Some(due));
        let generation = state.generation();
        assert_eq!(
            state.apply_threshold(generation, NeedKind::Thirst, due).0,
            NeedThresholdOutcomeKind::Reached
        );
        state.transition(AgentActivity::Idle, due);
        assert_eq!(state.threshold_due(NeedKind::Thirst, due), None);
    }

    #[test]
    fn generation_wrap_is_safe_for_bounded_stale_retention() {
        let mut state = NeedState::new(SimTime::ZERO);
        state.generation = u32::MAX;
        assert!(state.requires_transition(AgentActivity::Moving));
        assert!(state.transition(AgentActivity::Moving, SimTime::from_ticks(60)));
        assert_eq!(state.generation, 0);
        assert_eq!(state.reference_time, SimTime::from_ticks(60));
    }

    #[test]
    fn relief_preserves_exact_fractional_progress_and_changes_only_one_need() {
        let mut state = NeedState::new(SimTime::ZERO);
        state.values[NeedKind::Hunger.index()] = 10;
        let before = state.view(AgentId::new(0), SimTime::from_ticks(1));
        state.relieve(NeedKind::Hunger, 1, SimTime::from_ticks(1));
        assert_eq!(state.remainders[NeedKind::Hunger.index()], 2);
        let after = state.view(AgentId::new(0), SimTime::from_ticks(1));
        assert_eq!(after.hunger.value, before.hunger.value - 1);
        assert_eq!(after.thirst.value, before.thirst.value);
        assert_eq!(after.rest.value, before.rest.value);
        assert_eq!(after.exposure.value, before.exposure.value);
    }
}
