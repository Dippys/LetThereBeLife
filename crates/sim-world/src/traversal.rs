//! Derived walking rules: traversal steps, standability, and query errors.

use std::{error::Error, fmt};

use crate::SurfaceType;

/// Outcome of one cardinal movement query.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum TraversalKind {
    Passable,
    BlockedByWater,
    BlockedBySlope,
    BlockedByFeature,
}

/// Whether one resident cell can hold a standing physical agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Standability {
    Standable,
    BlockedByWater,
    BlockedByFeature,
}

/// Allocation-free derived movement information for two adjacent resident cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct TraversalStep {
    pub(crate) elevation_delta: i32,
    pub(crate) cost: u16,
    pub(crate) kind: TraversalKind,
}

impl TraversalStep {
    pub const fn blocked(elevation_delta: i32, kind: TraversalKind) -> Self {
        Self {
            elevation_delta,
            cost: 0,
            kind,
        }
    }

    pub const fn kind(self) -> TraversalKind {
        self.kind
    }

    pub const fn is_passable(self) -> bool {
        matches!(self.kind, TraversalKind::Passable)
    }

    /// Relative movement cost in stable integer units, or `None` when blocked.
    pub const fn cost(self) -> Option<u16> {
        if self.is_passable() {
            Some(self.cost)
        } else {
            None
        }
    }

    /// Signed target elevation minus source elevation.
    pub const fn elevation_delta(self) -> i32 {
        self.elevation_delta
    }
}

/// Explicit failure modes for terrain-dependent simulation queries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorldQueryError {
    OutsideWorldBounds,
    Unloaded,
    NonCardinalStep,
}

impl fmt::Display for WorldQueryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OutsideWorldBounds => formatter.write_str("query is outside the world envelope"),
            Self::Unloaded => formatter.write_str("query requires terrain that is not resident"),
            Self::NonCardinalStep => {
                formatter.write_str("movement query requires one cardinal-cell step")
            }
        }
    }
}

impl Error for WorldQueryError {}

/// Largest adjacent elevation difference the first walking contract can cross.
pub const MAX_TRAVERSABLE_ELEVATION_DELTA: u16 = 512;

/// Lower bound used by route heuristics; it must not exceed any passable
/// surface's base traversal cost.
pub const MIN_TRAVERSAL_COST: u16 = 10;

pub(crate) fn surface_traversal_cost(surface: SurfaceType) -> u16 {
    match surface {
        SurfaceType::Sand => 14,
        SurfaceType::Soil => MIN_TRAVERSAL_COST,
        SurfaceType::Hill => 18,
        SurfaceType::Rock => 22,
        SurfaceType::SnowIce => 20,
        SurfaceType::DeepWater | SurfaceType::ShallowWater => 0,
    }
}
