//! Agent cognition: private mental maps (remembered places, explored tiles),
//! sparse relationships, personality, and pointing gestures that let agents pass
//! knowledge to each other through observable behavior. Beliefs live here;
//! physical truth stays in the world, population, and resource stores.

mod gesture;
mod map;
mod personality;
mod social;

pub use gesture::Gesture;
pub(crate) use gesture::{interpret, point};
pub(crate) use map::MentalMap;
pub use map::{LANDMARK_SLOTS, MERGE_RADIUS, SEARCH_SPACING, VISIT_TILE_SIZE, VISITED_TILE_SLOTS};
pub use personality::Personality;
pub(crate) use social::SocialMemory;
pub use social::{ACQUAINTANCE_SLOTS, AcquaintanceView, DEFAULT_TRUST, FRIEND_FAMILIARITY};

use crate::{AgentId, SimTime, WorldPosition};

/// Seconds-resolution timestamps keep remembered places at 12 bytes.
pub(crate) fn belief_seconds(time: SimTime) -> u32 {
    u32::try_from(time.ticks() / 60).unwrap_or(u32::MAX - 1)
}

/// Simulated seconds between two gestures by an agent of average sociability.
/// The actual cooldown runs from 110 s (reserved) down to 10 s (chatty).
pub const SHARE_COOLDOWN_SECONDS: u32 = 60;

/// How sure a watcher is of a hint, given how much it trusts the teller.
/// Strangers start at `DEFAULT_TRUST`, which gives 144 (a first-hand sighting is 255).
pub(crate) const fn told_confidence(trust: u8) -> u8 {
    48 + (trust as u16 * 3 / 4) as u8
}
/// How long a pointing gesture takes, in ticks.
pub const SIGNAL_TICKS: u64 = 120;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum LandmarkKind {
    Water = 0,
    Food = 1,
    Wood = 2,
    Stone = 3,
    Shelter = 4,
}

impl LandmarkKind {
    pub const ALL: [Self; 5] = [
        Self::Water,
        Self::Food,
        Self::Wood,
        Self::Stone,
        Self::Shelter,
    ];
}

/// How an agent came to believe in a place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LandmarkSource {
    /// It saw the place itself.
    Seen,
    /// It inferred the place from someone's pointing gesture.
    Told,
}

/// A read-only copy of one remembered place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LandmarkView {
    pub kind: LandmarkKind,
    pub position: WorldPosition,
    pub source: LandmarkSource,
    /// 255 for a fresh first-hand sighting; lower for hearsay and failed searches.
    pub confidence: u8,
    /// Cells around `position` the agent expects to search (0 when seen first-hand).
    pub search_radius: u16,
    /// Simulated second when last confirmed or heard.
    pub seen_second: u32,
}

/// A read-only copy of what an agent knows and who it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MentalMapView {
    pub agent: AgentId,
    pub personality: Personality,
    pub landmarks: Vec<LandmarkView>,
    pub explored_tiles: usize,
    pub acquaintances: Vec<AcquaintanceView>,
}

/// What a gesture was about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GestureTopic {
    /// "There is water / food / ... over there."
    Place(LandmarkKind),
    /// "I've already been over there" (watchers treat that ground as explored).
    Explored,
}

/// One gesture completed during the latest tick, for logs and tools only.
/// Agents never see this record: receivers get just the public `gesture`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SignalEvent {
    /// Unique, increasing per engine run (reset clears it).
    pub id: u64,
    pub sender: AgentId,
    pub at: SimTime,
    pub origin: WorldPosition,
    /// **Private** to the sender: what it meant.
    pub topic: GestureTopic,
    /// **Private** to the sender: the exact place it had in mind.
    pub intended_place: WorldPosition,
    /// **Public:** what anyone watching could see.
    pub gesture: Gesture,
    /// Where watchers concluded the place is (what they can know, not the truth).
    pub inferred_position: WorldPosition,
    /// Search radius watchers attach to that conclusion.
    pub search_radius: u16,
    /// Watchers whose mental map changed.
    pub informed: u16,
    /// Awake agents that saw the gesture.
    pub watchers: u16,
}

/// How one watcher read one gesture (latest tick, for logs and tools only).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InterpretationEvent {
    pub signal: u64,
    pub receiver: AgentId,
    pub at: SimTime,
    /// What the receiver took the gesture to be about.
    pub understood: GestureTopic,
    pub estimate: WorldPosition,
    pub search_radius: u16,
    /// Confidence given to the hint (0 for "explored" gestures).
    pub confidence: u8,
    /// Whether the receiver's beliefs changed.
    pub changed: bool,
}

/// A hint that was checked by looking (latest tick, for logs and tools only).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HintOutcomeEvent {
    pub agent: AgentId,
    /// Who pointed it out, if still remembered.
    pub teller: Option<AgentId>,
    pub kind: LandmarkKind,
    /// `true`: found what the hint promised; `false`: searched and gave up.
    pub confirmed: bool,
    pub at: SimTime,
}

/// Which cognitive features the autonomous policy uses.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct PolicyOptions {
    /// Wander when nothing useful is in view (legacy viewer behavior).
    pub exploration: bool,
    /// Remember places, explore unvisited areas, travel to remembered places,
    /// and plan trips around known water.
    pub memory: bool,
    /// Point out remembered places to nearby agents (requires `memory`).
    pub sharing: bool,
    /// Individual personalities, relationships, visiting friends, and trust-
    /// weighted hints (requires `memory`). Without it everyone is average.
    pub social: bool,
}

impl PolicyOptions {
    /// Every cognitive feature: the full current agent mind.
    pub const fn full() -> Self {
        Self {
            exploration: true,
            memory: true,
            sharing: true,
            social: true,
        }
    }
}

/// Everything one agent privately knows: places and people.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Mind {
    pub(crate) map: MentalMap,
    pub(crate) social: SocialMemory,
}

impl Mind {
    /// Registers `other` as seen; keeps hint sources consistent if someone was forgotten.
    pub(crate) fn notice(
        &mut self,
        other: AgentId,
        position: WorldPosition,
        now: u32,
    ) -> Option<u8> {
        let noticed = self.social.notice(other, position, now)?;
        if noticed.evicted {
            self.map.forget_teller(noticed.slot);
        }
        Some(noticed.slot)
    }
}

/// Minds for every agent, indexed by `AgentId`, grown on demand.
#[derive(Debug, Default)]
pub(crate) struct Minds {
    minds: Vec<Mind>,
}

impl Minds {
    pub(crate) fn get(&self, agent: AgentId) -> Option<&Mind> {
        self.minds.get(agent.get() as usize)
    }

    pub(crate) fn get_mut(&mut self, agent: AgentId) -> &mut Mind {
        let index = agent.get() as usize;
        if index >= self.minds.len() {
            self.minds.resize(index + 1, Mind::default());
        }
        &mut self.minds[index]
    }
}

#[cfg(test)]
mod tests;
