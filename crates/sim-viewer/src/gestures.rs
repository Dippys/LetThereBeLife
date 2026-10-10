//! Presentation-only log of recently completed gestures: a bounded, briefly displayed ring.

use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

use sim_core::{AgentId, Mime, SignalEvent, VocalForm, WorldPosition};

/// Recent gestures kept for drawing; older ones are dropped first.
pub const RECENT_GESTURE_CAPACITY: usize = 32;
/// Real time a completed gesture stays on the map.
pub const GESTURE_DISPLAY_TIME: Duration = Duration::from_millis(2_500);

/// What the viewer draws for one gesture: the public pointing line from the sender
/// to where watchers concluded the place is, and the word and mime that went with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GestureMark {
    pub id: u64,
    pub sender: AgentId,
    pub origin: WorldPosition,
    pub inferred_position: WorldPosition,
    pub word: Option<VocalForm>,
    pub mime: Mime,
    /// Shouted (warnings and calls to hunt).
    pub loud: bool,
}

impl From<&SignalEvent> for GestureMark {
    fn from(event: &SignalEvent) -> Self {
        Self {
            id: event.id,
            sender: event.signal.sender,
            origin: event.signal.origin,
            inferred_position: event.inferred_position,
            word: event.signal.vocal,
            mime: event.signal.mime,
            loud: event.signal.loud,
        }
    }
}

/// Engine diagnostics copied after each tick; never read back by the simulation.
#[derive(Debug, Default)]
pub struct GestureLog {
    recent: VecDeque<(GestureMark, Instant)>,
}

impl GestureLog {
    pub fn new() -> Self {
        Self {
            recent: VecDeque::with_capacity(RECENT_GESTURE_CAPACITY),
        }
    }

    /// Records one tick's completed gestures, shown from `now`.
    pub fn record(&mut self, events: impl IntoIterator<Item = GestureMark>, now: Instant) {
        for event in events {
            if self.recent.len() == RECENT_GESTURE_CAPACITY {
                self.recent.pop_front();
            }
            self.recent.push_back((event, now));
        }
    }

    /// Drops gestures shown for at least `GESTURE_DISPLAY_TIME`; returns whether any were dropped.
    pub fn expire(&mut self, now: Instant) -> bool {
        let before = self.recent.len();
        while self
            .recent
            .front()
            .is_some_and(|(_, shown)| now.saturating_duration_since(*shown) >= GESTURE_DISPLAY_TIME)
        {
            self.recent.pop_front();
        }
        self.recent.len() != before
    }

    /// Gestures currently on display, oldest first.
    pub fn recent(&self) -> impl Iterator<Item = &GestureMark> {
        self.recent.iter().map(|(event, _)| event)
    }

    pub fn is_empty(&self) -> bool {
        self.recent.is_empty()
    }

    /// Forgets everything (engine reset restarts gesture ids).
    pub fn clear(&mut self) {
        self.recent.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(id: u64) -> GestureMark {
        GestureMark {
            id,
            sender: AgentId::new(1),
            origin: WorldPosition { x: 0, y: 0 },
            inferred_position: WorldPosition { x: 40, y: 0 },
            word: Some(VocalForm(id as u8)),
            mime: Mime::Scoop,
            loud: false,
        }
    }

    #[test]
    fn ring_is_bounded_and_keeps_the_newest_gestures() {
        let mut log = GestureLog::new();
        let now = Instant::now();
        let events: Vec<_> = (0..RECENT_GESTURE_CAPACITY as u64 + 9).map(event).collect();
        log.record(events[..5].iter().copied(), now);
        log.record(events[5..].iter().copied(), now);
        assert_eq!(log.recent().count(), RECENT_GESTURE_CAPACITY);
        assert_eq!(log.recent().next().unwrap().id, 9);
    }

    #[test]
    fn gestures_expire_after_the_display_time() {
        let mut log = GestureLog::new();
        let start = Instant::now();
        log.record([event(1)], start);
        log.record([event(2)], start + Duration::from_secs(1));
        assert!(!log.expire(start + GESTURE_DISPLAY_TIME - Duration::from_millis(1)));
        assert!(log.expire(start + GESTURE_DISPLAY_TIME));
        assert_eq!(log.recent().map(|event| event.id).collect::<Vec<_>>(), [2]);
        assert!(log.expire(start + Duration::from_secs(10)));
        assert!(log.is_empty());
    }
}
