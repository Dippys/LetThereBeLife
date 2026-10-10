use crate::{AgentId, NeedKind, SimTime, WorldPosition, needs::NeedState};

pub const HEALTH_MAX: u16 = 10_000;
pub const HEALTH_CONSEQUENCE_INTERVAL_TICKS: u64 = 600;
pub const HEALTH_INCAPACITATION_THRESHOLD: u16 = 2_500;
/// Health a full sleep restores.
pub const SLEEP_HEALING: u16 = 1_500;
const FLAG_SCHEDULED: u8 = 1;
const FLAG_DETERIORATING: u8 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum HealthStatus {
    Healthy = 0,
    Incapacitated = 1,
    Dead = 2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum DeathCause {
    Dehydration = 0,
    Exposure = 1,
    Starvation = 2,
    Exhaustion = 3,
    /// Bitten or wounded.
    Injury = 4,
}

impl DeathCause {
    pub const COUNT: usize = 5;

    /// Causes that come from a need staying severe, in tie-break order.
    pub(crate) const NEEDS: [Self; 4] = [
        Self::Dehydration,
        Self::Exposure,
        Self::Starvation,
        Self::Exhaustion,
    ];

    pub(crate) const fn need(self) -> NeedKind {
        match self {
            Self::Dehydration => NeedKind::Thirst,
            Self::Exposure => NeedKind::Exposure,
            Self::Starvation => NeedKind::Hunger,
            Self::Exhaustion | Self::Injury => NeedKind::Rest,
        }
    }

    pub(crate) const fn severe_threshold(self) -> u16 {
        match self {
            Self::Dehydration => 8_000,
            Self::Exposure => 8_500,
            Self::Starvation => 9_000,
            Self::Exhaustion => 9_500,
            Self::Injury => u16::MAX,
        }
    }

    pub(crate) const fn damage(self) -> u16 {
        match self {
            Self::Dehydration => 2_500,
            Self::Exposure => 2_000,
            Self::Starvation | Self::Exhaustion => 1_000,
            Self::Injury => 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HealthView {
    pub agent: AgentId,
    pub value: u16,
    pub status: HealthStatus,
    pub next_consequence: Option<SimTime>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeathRecord {
    pub agent: AgentId,
    pub cause: DeathCause,
    pub at: SimTime,
    pub position: WorldPosition,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealthDiagnosticKind {
    Deteriorated,
    Incapacitated,
    Died,
    StaleEvent,
    /// Came round after being knocked down by a wound.
    Recovered,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HealthDiagnostic {
    pub agent: AgentId,
    pub at: SimTime,
    pub cause: Option<DeathCause>,
    pub before: u16,
    pub after: u16,
    pub kind: HealthDiagnosticKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub(crate) struct HealthState {
    next_consequence: SimTime,
    generation: u32,
    value: u16,
    status: HealthStatus,
    flags: u8,
}

impl Default for HealthState {
    fn default() -> Self {
        Self {
            next_consequence: SimTime::ZERO,
            generation: 0,
            value: HEALTH_MAX,
            status: HealthStatus::Healthy,
            flags: 0,
        }
    }
}

impl HealthState {
    pub(crate) const fn generation(self) -> u32 {
        self.generation
    }

    #[cfg(test)]
    pub(crate) const fn status(self) -> HealthStatus {
        self.status
    }

    #[cfg(test)]
    pub(crate) fn set_value_for_test(&mut self, value: u16) {
        self.value = value.min(HEALTH_MAX);
    }

    pub(crate) fn view(self, agent: AgentId) -> HealthView {
        HealthView {
            agent,
            value: self.value,
            status: self.status,
            next_consequence: self.scheduled().then_some(self.next_consequence),
        }
    }

    pub(crate) fn reschedule(
        &mut self,
        needs: NeedState,
        now: SimTime,
    ) -> Option<(SimTime, NeedKind)> {
        if self.status == HealthStatus::Dead {
            self.flags = 0;
            return None;
        }
        let Some(next) = earliest_consequence(needs, now) else {
            self.generation = self.generation.wrapping_add(1);
            self.flags = 0;
            return None;
        };
        let preserve_cadence = next.0 == now
            && self.scheduled()
            && self.deteriorating()
            && self.next_consequence > now;
        let due = if preserve_cadence {
            self.next_consequence
        } else {
            next.0
        };
        self.generation = self.generation.wrapping_add(1);
        self.next_consequence = due;
        self.flags = FLAG_SCHEDULED
            | if preserve_cadence {
                FLAG_DETERIORATING
            } else {
                0
            };
        Some((due, next.1.need()))
    }

    pub(crate) fn event_is_current(self, generation: u32) -> bool {
        self.status != HealthStatus::Dead && self.scheduled() && self.generation == generation
    }

    pub(crate) fn apply(
        &mut self,
        generation: u32,
        needs: NeedState,
        due: SimTime,
        agent: AgentId,
    ) -> HealthDiagnostic {
        let before = self.value;
        if !self.event_is_current(generation) || self.next_consequence != due {
            return stale(agent, due, before);
        }
        self.flags &= !FLAG_SCHEDULED;
        let cause = DeathCause::NEEDS
            .into_iter()
            .find(|cause| needs.value_at(cause.need(), due) >= cause.severe_threshold());
        let Some(cause) = cause else {
            return stale(agent, due, before);
        };
        self.value = self.value.saturating_sub(cause.damage());
        let kind = if self.value == 0 {
            self.status = HealthStatus::Dead;
            HealthDiagnosticKind::Died
        } else if self.value <= HEALTH_INCAPACITATION_THRESHOLD
            && self.status == HealthStatus::Healthy
        {
            self.status = HealthStatus::Incapacitated;
            HealthDiagnosticKind::Incapacitated
        } else {
            HealthDiagnosticKind::Deteriorated
        };
        HealthDiagnostic {
            agent,
            at: due,
            cause: Some(cause),
            before,
            after: self.value,
            kind,
        }
    }

    /// Rest heals: health rises by `amount` (never past the maximum), but only
    /// for someone who is up and about.
    pub(crate) fn heal(&mut self, amount: u16) {
        if self.status == HealthStatus::Healthy {
            self.value = self.value.saturating_add(amount).min(HEALTH_MAX);
        }
    }

    /// Comes round after a wound knocked it down: back on its feet with health
    /// just above collapsing. Returns `(before, after)`, or `None` if it isn't
    /// lying wounded.
    pub(crate) fn revive(&mut self) -> Option<(u16, u16)> {
        if self.status != HealthStatus::Incapacitated {
            return None;
        }
        let before = self.value;
        self.value = self
            .value
            .max(HEALTH_INCAPACITATION_THRESHOLD + crate::SLEEP_HEALING);
        self.status = HealthStatus::Healthy;
        Some((before, self.value))
    }

    /// A wound: health falls by `amount` at once.
    pub(crate) fn injure(&mut self, amount: u16, agent: AgentId, at: SimTime) -> HealthDiagnostic {
        let before = self.value;
        if self.status == HealthStatus::Dead {
            return stale(agent, at, before);
        }
        self.value = self.value.saturating_sub(amount);
        let kind = if self.value == 0 {
            self.status = HealthStatus::Dead;
            self.flags = 0;
            HealthDiagnosticKind::Died
        } else if self.value <= HEALTH_INCAPACITATION_THRESHOLD
            && self.status == HealthStatus::Healthy
        {
            self.status = HealthStatus::Incapacitated;
            HealthDiagnosticKind::Incapacitated
        } else {
            HealthDiagnosticKind::Deteriorated
        };
        HealthDiagnostic {
            agent,
            at,
            cause: Some(DeathCause::Injury),
            before,
            after: self.value,
            kind,
        }
    }

    pub(crate) fn schedule_next_interval(&mut self, due: SimTime) -> Option<SimTime> {
        if self.status == HealthStatus::Dead {
            return None;
        }
        let next = due.checked_add(HEALTH_CONSEQUENCE_INTERVAL_TICKS)?;
        self.generation = self.generation.wrapping_add(1);
        self.next_consequence = next;
        self.flags |= FLAG_SCHEDULED | FLAG_DETERIORATING;
        Some(next)
    }

    const fn scheduled(self) -> bool {
        self.flags & FLAG_SCHEDULED != 0
    }

    const fn deteriorating(self) -> bool {
        self.flags & FLAG_DETERIORATING != 0
    }
}

fn stale(agent: AgentId, at: SimTime, value: u16) -> HealthDiagnostic {
    HealthDiagnostic {
        agent,
        at,
        cause: None,
        before: value,
        after: value,
        kind: HealthDiagnosticKind::StaleEvent,
    }
}

fn earliest_consequence(needs: NeedState, now: SimTime) -> Option<(SimTime, DeathCause)> {
    DeathCause::NEEDS
        .into_iter()
        .filter_map(|cause| {
            needs
                .due_at_value(cause.need(), cause.severe_threshold(), now)
                .map(|due| (due, cause))
        })
        .min_by_key(|(due, cause)| (*due, *cause))
}

#[cfg(test)]
mod tests {
    use std::{
        mem::{align_of, size_of},
        time::Instant,
    };

    use super::*;
    use crate::scheduler::Scheduler;

    #[test]
    fn health_state_is_compact_and_cause_priority_is_explicit() {
        assert_eq!(size_of::<HealthState>(), 16);
        assert_eq!(align_of::<HealthState>(), 8);
        assert_eq!(DeathCause::NEEDS[0], DeathCause::Dehydration);
        assert_eq!(DeathCause::NEEDS[1], DeathCause::Exposure);
    }

    #[test]
    fn repeated_dehydration_incapacitates_then_kills_once() {
        let mut needs = NeedState::new(SimTime::ZERO);
        needs.set_value_for_test(NeedKind::Thirst, 8_000, SimTime::ZERO);
        let mut health = HealthState::default();
        let (mut due, _) = health.reschedule(needs, SimTime::ZERO).unwrap();
        for expected in [7_500, 5_000, 2_500, 0] {
            let outcome = health.apply(health.generation(), needs, due, AgentId::new(0));
            assert_eq!(outcome.after, expected);
            if expected > 0 {
                due = health.schedule_next_interval(due).unwrap();
            }
        }
        assert_eq!(health.status(), HealthStatus::Dead);
    }

    #[test]
    fn simultaneous_severity_uses_physical_cause_precedence() {
        let mut needs = NeedState::new(SimTime::ZERO);
        needs.set_value_for_test(NeedKind::Hunger, 9_000, SimTime::ZERO);
        needs.set_value_for_test(NeedKind::Thirst, 8_000, SimTime::ZERO);
        needs.set_value_for_test(NeedKind::Rest, 9_500, SimTime::ZERO);
        needs.set_value_for_test(NeedKind::Exposure, 8_500, SimTime::ZERO);
        let mut health = HealthState::default();
        let (due, _) = health.reschedule(needs, SimTime::ZERO).unwrap();
        let outcome = health.apply(health.generation(), needs, due, AgentId::new(0));
        assert_eq!(outcome.cause, Some(DeathCause::Dehydration));
        assert_eq!(outcome.after, 7_500);
    }

    #[test]
    fn relief_ends_the_old_deterioration_cadence() {
        let mut needs = NeedState::new(SimTime::ZERO);
        needs.set_value_for_test(NeedKind::Thirst, 8_000, SimTime::ZERO);
        let mut health = HealthState::default();
        let (due, _) = health.reschedule(needs, SimTime::ZERO).unwrap();
        health.apply(health.generation(), needs, due, AgentId::new(0));
        assert_eq!(
            health.schedule_next_interval(due),
            Some(SimTime::from_ticks(600))
        );

        needs.set_value_for_test(NeedKind::Thirst, 0, SimTime::from_ticks(1));
        let projected = health.reschedule(needs, SimTime::from_ticks(1)).unwrap().0;
        assert!(projected > SimTime::from_ticks(600));
        needs.set_value_for_test(NeedKind::Thirst, 8_000, SimTime::from_ticks(2));
        assert_eq!(
            health.reschedule(needs, SimTime::from_ticks(2)).unwrap().0,
            SimTime::from_ticks(2)
        );
    }

    #[test]
    #[ignore = "release-only Slice 7 health-state and consequence-event measurement"]
    fn release_physical_agent_slice_seven_measurement() {
        assert!(
            !std::hint::black_box(cfg!(debug_assertions)),
            "run this harness with --release"
        );
        println!(
            "population\thealth_state_size\thealth_capacity\tevent_size\tscheduler_capacity\tretained_logical_bytes\tschedule_ns\tdue_extract_ns"
        );
        for population in [20_usize, 100, 10_000] {
            let mut states = Vec::with_capacity(population);
            states.resize(population, HealthState::default());
            let mut needs = NeedState::new(SimTime::ZERO);
            needs.set_value_for_test(NeedKind::Thirst, 8_000, SimTime::ZERO);
            let mut scheduler = Scheduler::with_capacity(population);
            let started = Instant::now();
            for (raw, state) in states.iter_mut().enumerate() {
                let (due, need) = state.reschedule(needs, SimTime::ZERO).unwrap();
                scheduler
                    .schedule_health_consequence(
                        due,
                        AgentId::new(raw as u32),
                        state.generation(),
                        need,
                    )
                    .unwrap();
            }
            let schedule_ns = started.elapsed().as_nanos();
            let started = Instant::now();
            while scheduler.pop_due(SimTime::ZERO).is_some() {}
            let due_ns = started.elapsed().as_nanos();
            println!(
                "{population}\t{}\t{}\t{}\t{}\t{}\t{schedule_ns}\t{due_ns}",
                size_of::<HealthState>(),
                states.capacity(),
                size_of::<crate::scheduler::ScheduledEvent>(),
                population,
                states.capacity() * size_of::<HealthState>()
                    + population * size_of::<crate::scheduler::ScheduledEvent>(),
            );
        }
    }
}
