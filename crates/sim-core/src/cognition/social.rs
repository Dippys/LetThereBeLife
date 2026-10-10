//! Sparse relationships: each agent remembers a few others it has met — how
//! familiar they are, how much it trusts their hints, and where it last saw
//! them. No all-pairs matrix; strangers beyond the slots are simply unknown.

use crate::{AgentId, WorldPosition, WorldRect};

/// Acquaintances remembered per agent.
pub const ACQUAINTANCE_SLOTS: usize = 12;
/// Familiarity gained each time the agent decides with the other in view.
const FAMILIARITY_PER_SIGHTING: u8 = 2;
/// Trust before any hint from this person has been checked.
pub const DEFAULT_TRUST: u8 = 128;
/// Trust gained when one of their hints turns out right, and lost when one doesn't.
const TRUST_CONFIRMED: u8 = 32;
const TRUST_REFUTED: u8 = 48;
/// Trust gained in someone who gave food when asked.
const TRUST_HELPED: u8 = 24;
/// Trust lost in someone who refused while holding food.
const TRUST_REFUSED: u8 = 8;
/// Trust a child starts with in its parent.
pub const BOND_TRUST: u8 = 220;
/// Familiarity needed before an agent counts someone as a friend worth visiting.
pub const FRIEND_FAMILIARITY: u8 = 24;
/// Below this trust someone is held in contempt: avoided, refused, not believed.
pub const DISTRUST: u8 = 64;
/// Most favours one remembers owing someone.
const MAX_OWED: u8 = 15;
const TIE_BITS: u8 = 0x07;
const RAISED_TOGETHER: u8 = 0x08;

/// How someone is related to the agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Tie {
    Parent = 1,
    Child = 2,
    Sibling = 3,
    Partner = 4,
}

impl Tie {
    const fn from_bits(bits: u8) -> Option<Self> {
        match bits {
            1 => Some(Self::Parent),
            2 => Some(Self::Child),
            3 => Some(Self::Sibling),
            4 => Some(Self::Partner),
            _ => None,
        }
    }

    /// Family by blood (partners are family by choice).
    pub const fn is_kin(self) -> bool {
        !matches!(self, Self::Partner)
    }
}

const EMPTY: u32 = u32::MAX;

/// One remembered person. Exactly 20 bytes.
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
    /// Bits 0-2: how they're related (`Tie`, 0 = not); bit 3: they were
    /// children together; bits 4-7: favours the agent owes them.
    ties: u8,
    /// What the agent calls them (`NO_NAME` if it doesn't know).
    name: u16,
    /// How many times in a row it has heard them called something else.
    name_doubt: u8,
    _reserved: u8,
}

const NO_NAME: u16 = u16::MAX;
/// Hearing someone called another name this many times running changes one's mind.
const NAME_DOUBTS: u8 = 2;

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
            ties: 0,
            name: NO_NAME,
            name_doubt: 0,
            _reserved: 0,
        }
    }
}

impl Acquaintance {
    const fn tie(self) -> Option<Tie> {
        Tie::from_bits(self.ties & TIE_BITS)
    }

    const fn owed(self) -> u8 {
        self.ties >> 4
    }

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
    pub tie: Option<Tie>,
    /// Favours the agent owes them.
    pub owed: u8,
    /// What the agent calls them, if it knows.
    pub name: Option<crate::Name>,
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
                            // Family is never forgotten to make room.
                            known.tie().is_some(),
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

    /// Starts a close bond (a child and its parent): high familiarity and trust.
    pub(crate) fn bond(&mut self, other: AgentId, position: WorldPosition, now: u32) -> Option<u8> {
        let noticed = self.notice(other, position, now)?;
        let known = &mut self.slots[usize::from(noticed.slot)];
        known.familiarity = u8::MAX;
        known.trust = BOND_TRUST;
        Some(noticed.slot)
    }

    /// Where `other` is expected to be, if known.
    pub(crate) fn whereabouts(&self, other: AgentId) -> Option<WorldPosition> {
        let known = &self.slots[usize::from(self.slot_of(other)?)];
        known.position_known.then(|| known.position())
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

    /// Adjusts trust in someone who was asked for help: up when they gave, down a
    /// little when they refused while holding food.
    pub(crate) fn helped(&mut self, slot: u8, gave: bool) {
        let known = &mut self.slots[usize::from(slot)];
        known.trust = if gave {
            known.trust.saturating_add(TRUST_HELPED)
        } else {
            known.trust.saturating_sub(TRUST_REFUSED)
        };
    }

    /// Records how `slot` is related to the agent.
    pub(crate) fn set_tie(&mut self, slot: u8, tie: Tie) {
        let known = &mut self.slots[usize::from(slot)];
        known.ties = (known.ties & !TIE_BITS) | tie as u8;
    }

    /// Ends a partnership (they drifted apart).
    pub(crate) fn clear_partner(&mut self, slot: u8) {
        let known = &mut self.slots[usize::from(slot)];
        if known.tie() == Some(Tie::Partner) {
            known.ties &= !TIE_BITS;
        }
    }

    /// The agent's partner and the slot that holds them, if any.
    pub(crate) fn partner(&self) -> Option<(u8, AgentId)> {
        self.slots
            .iter()
            .position(|known| known.agent != EMPTY && known.tie() == Some(Tie::Partner))
            .map(|slot| (slot as u8, AgentId::new(self.slots[slot].agent)))
    }

    /// They were children together (which rules them out as a partner).
    pub(crate) fn mark_raised_together(&mut self, slot: u8) {
        self.slots[usize::from(slot)].ties |= RAISED_TOGETHER;
    }

    pub(crate) fn raised_together(&self, slot: u8) -> bool {
        self.slots[usize::from(slot)].ties & RAISED_TOGETHER != 0
    }

    /// Hears `slot` called `name`: learns it if it knew no name, keeps its own
    /// if it hears the same, and switches after hearing another name twice
    /// running (so a misheard name gets set right).
    pub(crate) fn learn_name(&mut self, slot: u8, name: crate::Name) {
        let known = &mut self.slots[usize::from(slot)];
        if known.name == NO_NAME || known.name == name.0 {
            known.name = name.0;
            known.name_doubt = 0;
            return;
        }
        known.name_doubt += 1;
        if known.name_doubt >= NAME_DOUBTS {
            known.name = name.0;
            known.name_doubt = 0;
        }
    }

    pub(crate) fn name(&self, slot: u8) -> Option<crate::Name> {
        let name = self.slots[usize::from(slot)].name;
        (name != NO_NAME).then_some(crate::Name(name))
    }

    /// Whether the agent already calls anyone `name`.
    pub(crate) fn knows_name(&self, name: crate::Name) -> bool {
        self.slots
            .iter()
            .any(|known| known.agent != EMPTY && known.name == name.0)
    }

    pub(crate) fn last_seen(&self, slot: u8) -> u32 {
        self.slots[usize::from(slot)].last_seen
    }

    pub(crate) fn tie(&self, slot: u8) -> Option<Tie> {
        self.slots[usize::from(slot)].tie()
    }

    /// How `other` is related to the agent, if it knows them.
    pub(crate) fn tie_with(&self, other: AgentId) -> Option<Tie> {
        self.slot_of(other).and_then(|slot| self.tie(slot))
    }

    /// The agent received a favour from `slot` and owes them one more.
    pub(crate) fn owe(&mut self, slot: u8) {
        let known = &mut self.slots[usize::from(slot)];
        let owed = (known.owed() + 1).min(MAX_OWED);
        known.ties = (known.ties & !0xF0) | owed << 4;
    }

    /// The agent did `slot` a favour back: one fewer owed.
    pub(crate) fn repay(&mut self, slot: u8) {
        let known = &mut self.slots[usize::from(slot)];
        let owed = known.owed().saturating_sub(1);
        known.ties = (known.ties & !0xF0) | owed << 4;
    }

    pub(crate) fn owed(&self, slot: u8) -> u8 {
        self.slots[usize::from(slot)].owed()
    }

    /// Whether the agent holds `slot` in contempt.
    pub(crate) fn distrusts(&self, slot: u8) -> bool {
        self.slots[usize::from(slot)].trust < DISTRUST
    }

    /// Forgets `other` (after mourning them).
    pub(crate) fn forget(&mut self, other: AgentId) {
        if let Some(slot) = self.slot_of(other) {
            self.slots[usize::from(slot)] = Acquaintance::default();
        }
    }

    pub(crate) fn familiarity(&self, slot: u8) -> u8 {
        self.slots[usize::from(slot)].familiarity
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
                    && known.trust >= DISTRUST
                    && now.saturating_sub(known.last_seen) <= max_age
            })
            .max_by_key(|known| {
                (
                    // A partner first.
                    known.tie() == Some(Tie::Partner),
                    known.familiarity,
                    known.last_seen,
                    u32::MAX - known.agent,
                )
            })
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
                tie: known.tie(),
                owed: known.owed(),
                name: (known.name != NO_NAME).then_some(crate::Name(known.name)),
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
        assert_eq!(size_of::<Acquaintance>(), 20);
        assert_eq!(size_of::<SocialMemory>(), 20 * ACQUAINTANCE_SLOTS);
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
    fn family_is_never_forgotten_and_favours_are_counted() {
        let mut social = SocialMemory::default();
        let parent = social.notice(AgentId::new(50), at(0, 0), 0).unwrap().slot;
        social.set_tie(parent, Tie::Parent);
        for id in 0..ACQUAINTANCE_SLOTS as u32 * 2 {
            for _ in 0..5 {
                social.notice(AgentId::new(id), at(0, 0), 1);
            }
        }
        assert_eq!(social.tie_with(AgentId::new(50)), Some(Tie::Parent));
        social.owe(parent);
        social.owe(parent);
        social.repay(parent);
        assert_eq!(social.owed(parent), 1);
        assert_eq!(social.tie(parent), Some(Tie::Parent), "owing keeps the tie");
        for _ in 0..3 {
            social.hint_checked(parent, false);
        }
        assert!(social.distrusts(parent));
    }

    #[test]
    fn a_misheard_name_is_set_right_by_hearing_the_real_one_again() {
        let mut social = SocialMemory::default();
        let slot = social.notice(AgentId::new(3), at(0, 0), 0).unwrap().slot;
        social.learn_name(slot, crate::Name(10));
        social.learn_name(slot, crate::Name(20));
        assert_eq!(social.name(slot), Some(crate::Name(10)), "once is not enough");
        social.learn_name(slot, crate::Name(10));
        social.learn_name(slot, crate::Name(20));
        assert_eq!(social.name(slot), Some(crate::Name(10)), "the doubt was reset");
        social.learn_name(slot, crate::Name(20));
        assert_eq!(social.name(slot), Some(crate::Name(20)));
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
