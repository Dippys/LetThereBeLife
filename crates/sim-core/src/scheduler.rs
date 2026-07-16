use std::{cmp::Ordering, collections::BinaryHeap};

use crate::{AgentId, SimTime, agent::CompactPosition};

/// Maximum number of due events one engine tick may apply.
pub(crate) const MAX_DUE_EVENTS_PER_TICK: usize = 4_096;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub(crate) enum EventClass {
    Movement = 0,
}

impl EventClass {
    const fn rank(self) -> u8 {
        match self {
            Self::Movement => 0,
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
            .then_with(|| other.sequence.cmp(&self.sequence))
    }
}

impl PartialOrd for ScheduledEvent {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
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
}

impl Scheduler {
    #[cfg(test)]
    pub(crate) fn with_capacity(capacity: usize) -> Self {
        Self {
            events: BinaryHeap::with_capacity(capacity),
            next_sequence: 0,
        }
    }

    pub(crate) fn try_with_capacity(capacity: usize) -> Result<Self, ()> {
        let mut events = BinaryHeap::new();
        events.try_reserve_exact(capacity).map_err(|_| ())?;
        Ok(Self {
            events,
            next_sequence: 0,
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
            class: EventClass::Movement,
        });
        Ok(sequence)
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
        self.events.retain(|event| keep(event));
    }

    #[cfg(test)]
    pub(crate) fn capacity(&self) -> usize {
        self.events.capacity()
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
    fn exhausted_sequence_does_not_insert_an_event() {
        let mut scheduler = Scheduler {
            events: BinaryHeap::new(),
            next_sequence: u64::MAX,
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
