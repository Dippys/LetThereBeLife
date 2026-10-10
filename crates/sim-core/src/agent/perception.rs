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
    /// Cells a tree or rock stands on, depleted or not: walkable, but nobody
    /// can sleep or build there. Row-major.
    pub reserved_cells: Vec<WorldPosition>,
    /// Trees, bushes, and rocks in view that have been picked clean (capacity 0):
    /// visible evidence that someone got there first. Row-major.
    pub spent_resources: Vec<PerceivedResource>,
    /// Living animals in view, in id order. Carcasses appear among `resources`
    /// (as meat).
    pub animals: Vec<crate::AnimalView>,
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
