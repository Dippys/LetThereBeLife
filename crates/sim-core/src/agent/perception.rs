//! Public physical perception results, limits, and errors.

use std::{error::Error, fmt};

use crate::{
    BaseResource, WaterSource, WorldPosition, WorldRect, agent::AgentView,
    structures::StructureView,
};

pub const MAX_PERCEPTION_RADIUS: u8 = 31;

pub const MAX_PERCEPTION_CELLS: u16 = 3_969;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PerceivedWater {
    pub position: WorldPosition,
    pub source: WaterSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PerceivedResource {
    pub position: WorldPosition,
    pub resource: BaseResource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhysicalPerception {
    pub area: WorldRect,
    pub agents: Vec<AgentView>,
    /// Active autonomous destinations already claimed by perceived agents.
    /// Kept in row-major order so policy candidate checks stay deterministic
    /// and logarithmic without introducing a second persistent spatial index.
    pub claimed_targets: Vec<WorldPosition>,
    pub drinkable_water: Vec<PerceivedWater>,
    pub resources: Vec<PerceivedResource>,
    pub structures: Vec<StructureView>,
    pub traversable_cells: Vec<WorldPosition>,
    /// Terrain-connected standable cells reachable from this agent inside the
    /// perception area. Kept row-major for deterministic policy selection.
    pub reachable_cells: Vec<WorldPosition>,
    /// For each cell of `area`, row by row, whether it's among
    /// `reachable_cells` (empty if not worked out).
    pub reachable_mask: Vec<bool>,
    /// Cells a tree or rock stands on, depleted or not: walkable, but nobody
    /// can sleep or build there. Row-major.
    pub reserved_cells: Vec<WorldPosition>,
    /// Trees, bushes, and rocks in view that have been picked clean (capacity 0):
    /// visible evidence that someone got there first. Row-major.
    pub spent_resources: Vec<PerceivedResource>,
    /// Living animals in view, in id order. Carcasses appear among `resources`
    /// (as meat).
    pub animals: Vec<crate::AnimalView>,
    /// The feeling showing on the face of each agent in view that shows one
    /// (what the face shows, not what's felt; watchers may misread it).
    pub expressions: Vec<(crate::AgentId, crate::Feeling)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PerceptionError {
    MissingAgent,
    DeadAgent,
    RadiusTooLarge { requested: u8, maximum: u8 },
    EmptyArea,
    AreaOutsideActive,
    AreaTooLarge { requested: u64, maximum: u16 },
    Unloaded,
    OutsideWorld,
    AllocationFailed,
}

impl fmt::Display for PerceptionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "physical perception failed: {self:?}")
    }
}

impl Error for PerceptionError {}

impl PhysicalPerception {
    /// Whether `position` is among `reachable_cells`.
    pub fn reachable(&self, position: WorldPosition) -> bool {
        let width = self.area.max.x - self.area.min.x;
        let height = self.area.max.y - self.area.min.y;
        if self.reachable_mask.len() as i64 == width * height {
            return self.area.contains(position)
                && self.reachable_mask[((position.y - self.area.min.y) * width
                    + (position.x - self.area.min.x))
                    as usize];
        }
        self.reachable_cells
            .binary_search_by_key(&(position.y, position.x), |candidate| {
                (candidate.y, candidate.x)
            })
            .is_ok()
    }
}
