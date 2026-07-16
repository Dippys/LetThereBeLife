use std::{cmp::Ordering, collections::BinaryHeap};

use crate::{AgentId, NeedKind, SimTime, agent::CompactPosition, policy::PhysicalGoal};

/// Maximum number of due events one engine tick may apply.
pub(crate) const MAX_DUE_EVENTS_PER_TICK: usize = 4_096;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub(crate) enum EventClass {
    NeedThreshold = 0,
    HealthConsequence = 1,
    Wake = 2,
    ActionCompletion = 3,
    Movement = 4,
    Decision = 5,
}

impl EventClass {
    const fn rank(self) -> u8 {
        match self {
            Self::NeedThreshold => 0,
            Self::HealthConsequence => 1,
            Self::Wake => 2,
            Self::ActionCompletion => 3,
            Self::Movement => 4,
            Self::Decision => 5,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub(crate) struct ScheduledEvent {
    pub(crate) due: SimTime,
    pub(crate) sequence: u64,
    pub(crate) agent: AgentId,
    pub(crate) generation: u32,
    pub(crate) target: CompactPosition,
    pub(crate) need: NeedKind,
    pub(crate) goal: PhysicalGoal,
    pub(crate) class: EventClass,
}

impl Ord for ScheduledEvent {
    fn cmp(&self, other: &Self) -> Ordering {
        // Reverse the complete key so BinaryHeap behaves as a deterministic min-heap.
        other
            .due
            .cmp(&self.due)
            .then_with(|| other.class.rank().cmp(&self.class.rank()))
            .then_with(|| other.agent.cmp(&self.agent))
            .then_with(|| other.detail_rank().cmp(&self.detail_rank()))
            .then_with(|| other.sequence.cmp(&self.sequence))
    }
}

impl PartialOrd for ScheduledEvent {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl ScheduledEvent {
    const fn detail_rank(self) -> u8 {
        match self.class {
            EventClass::NeedThreshold | EventClass::HealthConsequence => self.need as u8,
            EventClass::Wake | EventClass::ActionCompletion | EventClass::Decision => {
                self.goal as u8
            }
            EventClass::Movement => 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScheduleError {
    SequenceExhausted,
}

#[derive(Debug, Default)]
pub(crate) struct Scheduler {
    events: BinaryHeap<ScheduledEvent>,
    next_sequence: u64,
    total_scheduled: u64,
    total_compacted: u64,
    peak_len: usize,
}

impl Scheduler {
    pub(crate) fn can_schedule(&self, count: u64) -> bool {
        self.next_sequence.checked_add(count).is_some()
    }

    #[cfg(test)]
    pub(crate) fn with_capacity(capacity: usize) -> Self {
        Self {
            events: BinaryHeap::with_capacity(capacity),
            next_sequence: 0,
            total_scheduled: 0,
            total_compacted: 0,
            peak_len: 0,
        }
    }

    pub(crate) fn try_with_capacity(capacity: usize) -> Result<Self, ()> {
        let mut events = BinaryHeap::new();
        events.try_reserve_exact(capacity).map_err(|_| ())?;
        Ok(Self {
            events,
            next_sequence: 0,
            total_scheduled: 0,
            total_compacted: 0,
            peak_len: 0,
        })
    }

    pub(crate) fn schedule_movement(
        &mut self,
        due: SimTime,
        agent: AgentId,
        generation: u32,
        target: CompactPosition,
    ) -> Result<u64, ScheduleError> {
        let sequence = self.next_sequence;
        self.next_sequence = self
            .next_sequence
            .checked_add(1)
            .ok_or(ScheduleError::SequenceExhausted)?;
        self.events.push(ScheduledEvent {
            due,
            sequence,
            agent,
            generation,
            target,
            need: NeedKind::Hunger,
            goal: PhysicalGoal::Wait,
            class: EventClass::Movement,
        });
        self.record_schedule();
        Ok(sequence)
    }

    pub(crate) fn schedule_need_threshold(
        &mut self,
        due: SimTime,
        agent: AgentId,
        generation: u32,
        need: NeedKind,
    ) -> Result<u64, ScheduleError> {
        let sequence = self.next_sequence;
        self.next_sequence = self
            .next_sequence
            .checked_add(1)
            .ok_or(ScheduleError::SequenceExhausted)?;
        self.events.push(ScheduledEvent {
            due,
            sequence,
            agent,
            generation,
            target: CompactPosition { x: 0, y: 0 },
            need,
            goal: PhysicalGoal::Wait,
            class: EventClass::NeedThreshold,
        });
        self.record_schedule();
        Ok(sequence)
    }

    pub(crate) fn schedule_health_consequence(
        &mut self,
        due: SimTime,
        agent: AgentId,
        generation: u32,
        need: NeedKind,
    ) -> Result<u64, ScheduleError> {
        let sequence = self.next_sequence;
        self.next_sequence = self
            .next_sequence
            .checked_add(1)
            .ok_or(ScheduleError::SequenceExhausted)?;
        self.events.push(ScheduledEvent {
            due,
            sequence,
            agent,
            generation,
            target: CompactPosition { x: 0, y: 0 },
            need,
            goal: PhysicalGoal::Incapacitated,
            class: EventClass::HealthConsequence,
        });
        self.record_schedule();
        Ok(sequence)
    }

    pub(crate) fn schedule_decision(
        &mut self,
        due: SimTime,
        agent: AgentId,
        generation: u32,
        goal: PhysicalGoal,
    ) -> Result<u64, ScheduleError> {
        self.schedule_policy(
            due,
            agent,
            generation,
            goal,
            CompactPosition { x: 0, y: 0 },
            EventClass::Decision,
        )
    }

    pub(crate) fn schedule_action_completion(
        &mut self,
        due: SimTime,
        agent: AgentId,
        generation: u32,
        goal: PhysicalGoal,
        target: CompactPosition,
    ) -> Result<u64, ScheduleError> {
        self.schedule_policy(
            due,
            agent,
            generation,
            goal,
            target,
            EventClass::ActionCompletion,
        )
    }

    pub(crate) fn schedule_wake(
        &mut self,
        due: SimTime,
        agent: AgentId,
        generation: u32,
        target: CompactPosition,
    ) -> Result<u64, ScheduleError> {
        self.schedule_policy(
            due,
            agent,
            generation,
            PhysicalGoal::Sleep,
            target,
            EventClass::Wake,
        )
    }

    fn schedule_policy(
        &mut self,
        due: SimTime,
        agent: AgentId,
        generation: u32,
        goal: PhysicalGoal,
        target: CompactPosition,
        class: EventClass,
    ) -> Result<u64, ScheduleError> {
        let sequence = self.next_sequence;
        self.next_sequence = self
            .next_sequence
            .checked_add(1)
            .ok_or(ScheduleError::SequenceExhausted)?;
        self.events.push(ScheduledEvent {
            due,
            sequence,
            agent,
            generation,
            target,
            need: NeedKind::Hunger,
            goal,
            class,
        });
        self.record_schedule();
        Ok(sequence)
    }

    fn record_schedule(&mut self) {
        self.total_scheduled = self.total_scheduled.saturating_add(1);
        self.peak_len = self.peak_len.max(self.events.len());
    }

    pub(crate) fn pop_due(&mut self, now: SimTime) -> Option<ScheduledEvent> {
        self.events
            .peek()
            .is_some_and(|event| event.due <= now)
            .then(|| self.events.pop())
            .flatten()
    }

    pub(crate) fn len(&self) -> usize {
        self.events.len()
    }

    pub(crate) fn has_due(&self, now: SimTime) -> bool {
        self.events.peek().is_some_and(|event| event.due <= now)
    }

    pub(crate) fn retain(&mut self, mut keep: impl FnMut(&ScheduledEvent) -> bool) {
        let before = self.events.len();
        self.events.retain(|event| keep(event));
        self.total_compacted = self
            .total_compacted
            .saturating_add((before - self.events.len()) as u64);
    }

    pub(crate) fn capacity(&self) -> usize {
        self.events.capacity()
    }

    pub(crate) const fn total_scheduled(&self) -> u64 {
        self.total_scheduled
    }

    pub(crate) const fn total_compacted(&self) -> u64 {
        self.total_compacted
    }

    pub(crate) const fn peak_len(&self) -> usize {
        self.peak_len
    }

    #[cfg(test)]
    pub(crate) fn exhaust_sequence(&mut self) {
        self.next_sequence = u64::MAX;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(due: u64, agent: u32, sequence: u64) -> ScheduledEvent {
        ScheduledEvent {
            due: SimTime::from_ticks(due),
            sequence,
            agent: AgentId::new(agent),
            generation: 1,
            target: CompactPosition { x: 0, y: 0 },
            need: NeedKind::Hunger,
            goal: PhysicalGoal::Wait,
            class: EventClass::Movement,
        }
    }

    #[test]
    fn event_order_is_time_class_agent_then_sequence() {
        let mut heap = BinaryHeap::new();
        for item in [
            event(4, 9, 0),
            event(3, 7, 8),
            event(3, 2, 9),
            event(3, 2, 4),
        ] {
            heap.push(item);
        }
        let keys: Vec<_> = std::iter::from_fn(|| heap.pop())
            .map(|item| (item.due.ticks(), item.agent.get(), item.sequence))
            .collect();
        assert_eq!(keys, [(3, 2, 4), (3, 2, 9), (3, 7, 8), (4, 9, 0)]);
    }

    #[test]
    fn due_extraction_never_returns_future_work() {
        let mut scheduler = Scheduler::default();
        scheduler
            .schedule_movement(
                SimTime::from_ticks(8),
                AgentId::new(0),
                1,
                CompactPosition { x: 0, y: 0 },
            )
            .unwrap();
        assert_eq!(scheduler.pop_due(SimTime::from_ticks(7)), None);
        assert!(scheduler.pop_due(SimTime::from_ticks(8)).is_some());
    }

    #[test]
    fn equal_time_need_priority_precedes_movement_and_ignores_insertion_order() {
        let mut scheduler = Scheduler::default();
        scheduler
            .schedule_movement(
                SimTime::from_ticks(8),
                AgentId::new(0),
                1,
                CompactPosition { x: 1, y: 0 },
            )
            .unwrap();
        for need in NeedKind::ALL.into_iter().rev() {
            scheduler
                .schedule_need_threshold(SimTime::from_ticks(8), AgentId::new(0), 0, need)
                .unwrap();
        }
        scheduler
            .schedule_health_consequence(
                SimTime::from_ticks(8),
                AgentId::new(0),
                0,
                NeedKind::Thirst,
            )
            .unwrap();
        let keys: Vec<_> = std::iter::from_fn(|| scheduler.pop_due(SimTime::from_ticks(8)))
            .map(|event| (event.class, event.need))
            .collect();
        assert_eq!(
            keys,
            [
                (EventClass::NeedThreshold, NeedKind::Hunger),
                (EventClass::NeedThreshold, NeedKind::Thirst),
                (EventClass::NeedThreshold, NeedKind::Rest),
                (EventClass::NeedThreshold, NeedKind::Exposure),
                (EventClass::HealthConsequence, NeedKind::Thirst),
                (EventClass::Movement, NeedKind::Hunger),
            ]
        );
    }

    #[test]
    fn equal_time_wake_completion_and_decision_have_explicit_boundaries() {
        let mut scheduler = Scheduler::default();
        scheduler
            .schedule_decision(
                SimTime::from_ticks(8),
                AgentId::new(0),
                1,
                PhysicalGoal::Wait,
            )
            .unwrap();
        scheduler
            .schedule_movement(
                SimTime::from_ticks(8),
                AgentId::new(0),
                1,
                CompactPosition { x: 1, y: 0 },
            )
            .unwrap();
        scheduler
            .schedule_wake(
                SimTime::from_ticks(8),
                AgentId::new(0),
                1,
                CompactPosition { x: 0, y: 0 },
            )
            .unwrap();
        scheduler
            .schedule_action_completion(
                SimTime::from_ticks(8),
                AgentId::new(0),
                1,
                PhysicalGoal::Drink,
                CompactPosition { x: 0, y: 0 },
            )
            .unwrap();
        scheduler
            .schedule_need_threshold(SimTime::from_ticks(8), AgentId::new(0), 0, NeedKind::Thirst)
            .unwrap();
        let classes: Vec<_> = std::iter::from_fn(|| scheduler.pop_due(SimTime::from_ticks(8)))
            .map(|event| event.class)
            .collect();
        assert_eq!(
            classes,
            [
                EventClass::NeedThreshold,
                EventClass::Wake,
                EventClass::ActionCompletion,
                EventClass::Movement,
                EventClass::Decision,
            ]
        );
    }

    #[test]
    fn exhausted_sequence_does_not_insert_an_event() {
        let mut scheduler = Scheduler {
            events: BinaryHeap::new(),
            next_sequence: u64::MAX,
            total_scheduled: 0,
            total_compacted: 0,
            peak_len: 0,
        };
        assert_eq!(
            scheduler.schedule_movement(
                SimTime::from_ticks(1),
                AgentId::new(0),
                1,
                CompactPosition { x: 0, y: 0 },
            ),
            Err(ScheduleError::SequenceExhausted)
        );
        assert_eq!(scheduler.len(), 0);
    }
}
