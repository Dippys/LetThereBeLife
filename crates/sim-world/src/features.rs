//! Sparse generated surface features and their base resource yields.

use crate::WorldPosition;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum FeatureKind {
    Tree,
    Rock,
    BerryBush,
}

/// Gatherable material exposed by an immutable generated surface feature.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ResourceKind {
    Food,
    Wood,
    Stone,
}

/// Generated maximum yield before any future sparse depletion state is applied.
///
/// Capacities are abstract gathering units. They belong to the versioned base
/// generator; remaining quantity, removal, and regrowth must live in a sparse
/// mutable layer keyed by feature position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct BaseResource {
    pub capacity: u16,
    pub kind: ResourceKind,
}

impl FeatureKind {
    pub const fn base_resource(self) -> BaseResource {
        match self {
            Self::Tree => BaseResource {
                capacity: 120,
                kind: ResourceKind::Wood,
            },
            Self::Rock => BaseResource {
                capacity: 80,
                kind: ResourceKind::Stone,
            },
            Self::BerryBush => BaseResource {
                capacity: 12,
                kind: ResourceKind::Food,
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Feature {
    pub position: WorldPosition,
    pub kind: FeatureKind,
}

impl Feature {
    /// Stable generated identity within one seed and generator revision.
    ///
    /// Future persisted sparse deltas must additionally record the generator
    /// version; the world position itself is the current feature key.
    pub const fn identity(self) -> WorldPosition {
        self.position
    }

    pub const fn base_resource(self) -> BaseResource {
        self.kind.base_resource()
    }
}
