//! Agent cognition: private mental maps (remembered places, explored tiles) and
//! pointing gestures that let agents pass place knowledge to each other through
//! observable behavior. Beliefs live here; physical truth stays in the world,
//! population, and resource stores.

mod gesture;
mod map;

pub(crate) use gesture::{interpret, point};
pub(crate) use map::MentalMap;
pub use map::{LANDMARK_SLOTS, MERGE_RADIUS, SEARCH_SPACING, VISIT_TILE_SIZE, VISITED_TILE_SLOTS};

use crate::{AgentId, SimTime, WorldPosition};

/// Seconds-resolution timestamps keep remembered places at 12 bytes.
pub(crate) fn belief_seconds(time: SimTime) -> u32 {
    u32::try_from(time.ticks() / 60).unwrap_or(u32::MAX - 1)
}

/// Minimum simulated seconds between two gestures by the same agent.
pub const SHARE_COOLDOWN_SECONDS: u32 = 60;
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

/// A read-only copy of an agent's mental map.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MentalMapView {
    pub agent: AgentId,
    pub landmarks: Vec<LandmarkView>,
    pub explored_tiles: usize,
}

/// One pointing gesture observed during the latest tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SignalEvent {
    pub sender: AgentId,
    pub at: SimTime,
    pub kind: LandmarkKind,
    /// Where watchers concluded the place is (what they can know, not the truth).
    pub inferred_position: WorldPosition,
    /// Watchers whose mental map changed.
    pub informed: u16,
    /// Awake agents that saw the gesture.
    pub watchers: u16,
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
}

impl PolicyOptions {
    /// Memory, exploration, and sharing: the full current agent mind.
    pub const fn full() -> Self {
        Self {
            exploration: true,
            memory: true,
            sharing: true,
        }
    }
}

/// Mental maps for every agent, indexed by `AgentId`, grown on demand.
#[derive(Debug, Default)]
pub(crate) struct Minds {
    maps: Vec<MentalMap>,
}

impl Minds {
    pub(crate) fn get(&self, agent: AgentId) -> Option<&MentalMap> {
        self.maps.get(agent.get() as usize)
    }

    pub(crate) fn get_mut(&mut self, agent: AgentId) -> &mut MentalMap {
        let index = agent.get() as usize;
        if index >= self.maps.len() {
            self.maps.resize(index + 1, MentalMap::default());
        }
        &mut self.maps[index]
    }
}

#[cfg(test)]
mod tests;
