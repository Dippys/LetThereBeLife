//! Sparse relationships: each agent remembers a few others it has met — how
//! familiar they are, how much it trusts their hints, and where it last saw
//! them. No all-pairs matrix; strangers beyond the slots are simply unknown.

use crate::{AgentId, WorldPosition, WorldRect};

/// Acquaintances remembered per agent.
pub const ACQUAINTANCE_SLOTS: usize = 6;
/// Familiarity gained each time the agent decides with the other in view.
const FAMILIARITY_PER_SIGHTING: u8 = 2;
/// Trust before any hint from this person has been checked.
pub const DEFAULT_TRUST: u8 = 128;
/// Trust gained when one of their hints turns out right, and lost when one doesn't.
const TRUST_CONFIRMED: u8 = 32;
const TRUST_REFUTED: u8 = 48;
/// Familiarity needed before an agent counts someone as a friend worth visiting.
pub const FRIEND_FAMILIARITY: u8 = 24;

const EMPTY: u32 = u32::MAX;

/// One remembered person. Exactly 16 bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
struct Acquaintance {
    agent: u32,
    /// Simulated second they were last seen.
    last_seen: u32,
    x: i16,
    y: i16,
    familiarity: u8,
    trust: u8,
    /// Whether `x, y` is still where the agent expects to find them.
    position_known: bool,
    _reserved: u8,
}

impl Default for Acquaintance {
    fn default() -> Self {
        Self {
            agent: EMPTY,
            last_seen: 0,
            x: 0,
            y: 0,
            familiarity: 0,
            trust: DEFAULT_TRUST,
            position_known: false,
            _reserved: 0,
        }
    }
}

impl Acquaintance {
    fn position(self) -> WorldPosition {
        WorldPosition {
            x: i64::from(self.x),
            y: i64::from(self.y),
        }
    }
}

/// A read-only copy of one relationship.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AcquaintanceView {
    pub agent: AgentId,
    pub familiarity: u8,
    pub trust: u8,
    /// Where they were last seen, if the agent hasn't since found that spot empty.
    pub last_seen_position: Option<WorldPosition>,
    pub last_seen_second: u32,
}

/// What `notice` did to the slots.
pub(crate) struct Noticed {
    pub(crate) slot: u8,
    /// A previous acquaintance was forgotten to make room; hints they gave lose their source.
    pub(crate) evicted: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub(crate) struct SocialMemory {
    slots: [Acquaintance; ACQUAINTANCE_SLOTS],
}

impl Default for SocialMemory {
    fn default() -> Self {
        Self {
            slots: [Acquaintance::default(); ACQUAINTANCE_SLOTS],
        }
    }
}

impl SocialMemory {
    /// Records seeing `other` at `position`. Strangers take an empty slot or
    /// replace the least familiar (then least recently seen) acquaintance.
    pub(crate) fn notice(
        &mut self,
        other: AgentId,
        position: WorldPosition,
        now: u32,
    ) -> Option<Noticed> {
        let (x, y) = (
            i16::try_from(position.x).ok()?,
            i16::try_from(position.y).ok()?,
        );
        let raw = other.get();
        let (slot, evicted) = match self.slot_of(other) {
            Some(slot) => (slot, false),
            None => {
                let slot = (0..ACQUAINTANCE_SLOTS)
                    .min_by_key(|&slot| {
                        let known = self.slots[slot];
                        (
                            known.agent != EMPTY,
                            known.familiarity,
                            known.last_seen,
                            slot,
                        )
                    })
                    .expect("at least one slot");
                let evicted = self.slots[slot].agent != EMPTY;
                self.slots[slot] = Acquaintance {
                    agent: raw,
                    ..Acquaintance::default()
                };
                (slot as u8, evicted)
            }
        };
        let known = &mut self.slots[usize::from(slot)];
        known.familiarity = known.familiarity.saturating_add(FAMILIARITY_PER_SIGHTING);
        known.last_seen = now;
        known.x = x;
        known.y = y;
        known.position_known = true;
        Some(Noticed { slot, evicted })
    }

    pub(crate) fn slot_of(&self, other: AgentId) -> Option<u8> {
        self.slots
            .iter()
            .position(|known| known.agent == other.get())
            .map(|slot| slot as u8)
    }

    pub(crate) fn agent_in(&self, slot: u8) -> Option<AgentId> {
        let known = self.slots[usize::from(slot)];
        (known.agent != EMPTY).then(|| AgentId::new(known.agent))
    }

    pub(crate) fn trust(&self, slot: u8) -> u8 {
        self.slots[usize::from(slot)].trust
    }

    /// Adjusts trust in the source of a hint after it was checked.
    pub(crate) fn hint_checked(&mut self, slot: u8, confirmed: bool) {
        let known = &mut self.slots[usize::from(slot)];
        known.trust = if confirmed {
            known.trust.saturating_add(TRUST_CONFIRMED)
        } else {
            known.trust.saturating_sub(TRUST_REFUTED)
        };
    }

    /// Forgets where acquaintances were if that spot is in view and they aren't.
    pub(crate) fn update_whereabouts(&mut self, view: WorldRect, present: impl Fn(u32) -> bool) {
        for known in &mut self.slots {
            if known.agent != EMPTY
                && known.position_known
                && contains(view, known.position())
                && !present(known.agent)
            {
                known.position_known = false;
            }
        }
    }

    /// The most familiar friend whose whereabouts are known and fresher than
    /// `max_age` seconds.
    pub(crate) fn friend_to_visit(&self, now: u32, max_age: u32) -> Option<WorldPosition> {
        self.slots
            .iter()
            .filter(|known| {
                known.agent != EMPTY
                    && known.position_known
                    && known.familiarity >= FRIEND_FAMILIARITY
                    && now.saturating_sub(known.last_seen) <= max_age
            })
            .max_by_key(|known| (known.familiarity, known.last_seen, u32::MAX - known.agent))
            .map(|known| known.position())
    }

    pub(crate) fn views(&self) -> impl Iterator<Item = AcquaintanceView> + '_ {
        self.slots
            .iter()
            .filter(|known| known.agent != EMPTY)
            .map(|known| AcquaintanceView {
                agent: AgentId::new(known.agent),
                familiarity: known.familiarity,
                trust: known.trust,
                last_seen_position: known.position_known.then(|| known.position()),
                last_seen_second: known.last_seen,
            })
    }
}

fn contains(area: WorldRect, position: WorldPosition) -> bool {
    position.x >= area.min.x
        && position.x < area.max.x
        && position.y >= area.min.y
        && position.y < area.max.y
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(x: i64, y: i64) -> WorldPosition {
        WorldPosition { x, y }
    }

    #[test]
    fn relationship_layout_is_compact() {
        assert_eq!(size_of::<Acquaintance>(), 16);
        assert_eq!(size_of::<SocialMemory>(), 16 * ACQUAINTANCE_SLOTS);
    }

    #[test]
    fn repeated_sightings_build_familiarity_and_track_whereabouts() {
        let mut social = SocialMemory::default();
        for second in 0..20 {
            social.notice(AgentId::new(9), at(second, 5), second as u32);
        }
        let friend = social.views().next().unwrap();
        assert_eq!(friend.familiarity, 40);
        assert_eq!(friend.trust, DEFAULT_TRUST);
        assert_eq!(friend.last_seen_position, Some(at(19, 5)));
        assert_eq!(social.friend_to_visit(25, 60), Some(at(19, 5)));
        assert_eq!(social.friend_to_visit(500, 60), None, "too long ago");
    }

    #[test]
    fn the_least_familiar_acquaintance_is_forgotten_first() {
        let mut social = SocialMemory::default();
        for id in 0..ACQUAINTANCE_SLOTS as u32 {
            for _ in 0..=id {
                social.notice(AgentId::new(id), at(0, 0), 1);
            }
        }
        let noticed = social.notice(AgentId::new(99), at(0, 0), 2).unwrap();
        assert!(noticed.evicted);
        assert!(
            social.slot_of(AgentId::new(0)).is_none(),
            "agent 0 was least familiar"
        );
        assert!(social.slot_of(AgentId::new(99)).is_some());
    }

    #[test]
    fn trust_follows_how_hints_turn_out() {
        let mut social = SocialMemory::default();
        let slot = social.notice(AgentId::new(3), at(0, 0), 0).unwrap().slot;
        social.hint_checked(slot, true);
        assert_eq!(social.trust(slot), DEFAULT_TRUST + 32);
        social.hint_checked(slot, false);
        social.hint_checked(slot, false);
        assert_eq!(social.trust(slot), DEFAULT_TRUST + 32 - 96);
    }

    #[test]
    fn finding_a_friend_absent_forgets_where_they_were() {
        let mut social = SocialMemory::default();
        for _ in 0..20 {
            social.notice(AgentId::new(4), at(10, 10), 1);
        }
        let view = WorldRect {
            min: at(0, 0),
            max: at(20, 20),
        };
        social.update_whereabouts(view, |_| false);
        assert_eq!(social.friend_to_visit(2, 100), None);
    }
}
