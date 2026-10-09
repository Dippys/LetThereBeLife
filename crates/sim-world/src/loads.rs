//! Opaque chunk load requests and the resident chunk representations they produce.

use crate::{
    CHUNK_SIZE, ChunkCoord, Feature, TerrainCell, WorldChunk, WorldPosition, WorldRect,
    chunk::chunk_origin, worldgen::ChunkContext,
};

/// An opaque, deterministic request to materialize one chunk-sized world area.
///
/// Requests created by [`crate::World::missing_chunk_load_requests`] preserve whether
/// a configured bootstrap edge must be clipped rather than exposing the
/// remainder of its 64 x 64 chunk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChunkLoadRequest {
    pub(crate) coord: ChunkCoord,
    pub(crate) bounds: WorldRect,
    pub(crate) kind: ChunkLoadKind,
}

impl ChunkLoadRequest {
    pub const fn coord(self) -> ChunkCoord {
        self.coord
    }

    /// Returns the exact terrain coverage that this request will materialize.
    pub const fn bounds(self) -> WorldRect {
        self.bounds
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChunkLoadKind {
    Bootstrap,
    Expansion,
}

/// An opaque worker payload generated from a [`ChunkLoadRequest`].
///
/// The worker may construct this value, but only [`crate::World::insert_chunk_loads`]
/// changes simulation-owned coverage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorldChunkLoad {
    pub(crate) seed: u64,
    pub(crate) request: ChunkLoadRequest,
    pub(crate) chunk: LoadedChunk,
}

impl WorldChunkLoad {
    pub const fn coord(&self) -> ChunkCoord {
        self.request.coord
    }

    pub const fn bounds(&self) -> WorldRect {
        self.request.bounds
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LoadedChunk {
    Bootstrap(InitialChunk),
    Expansion(WorldChunk),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InitialChunk {
    pub(crate) bounds: WorldRect,
    pub(crate) terrain: Box<[TerrainCell]>,
    pub(crate) features: Box<[Feature]>,
}

impl InitialChunk {
    pub(crate) fn generate(seed: u64, coord: ChunkCoord, bounds: WorldRect) -> Self {
        debug_assert!(
            coord
                .bounds()
                .expect("generated chunks always have representable bounds")
                .contains_rect(bounds)
        );
        let width = (bounds.max.x - bounds.min.x) as usize;
        let height = (bounds.max.y - bounds.min.y) as usize;
        let origin = chunk_origin(coord);
        let context = ChunkContext::new(seed, origin.x, origin.y);
        let mut terrain = Vec::with_capacity(width * height);
        let mut features = Vec::new();
        for y in bounds.min.y..bounds.max.y {
            for x in bounds.min.x..bounds.max.x {
                let (cell, feature) = context.generate(x, y);
                terrain.push(cell);
                if let Some(kind) = feature {
                    features.push(Feature {
                        position: WorldPosition { x, y },
                        kind,
                    });
                }
            }
        }
        Self {
            bounds,
            terrain: terrain.into_boxed_slice(),
            features: features.into_boxed_slice(),
        }
    }

    fn cell(&self, position: WorldPosition) -> Option<TerrainCell> {
        self.bounds.contains(position).then(|| {
            let width = (self.bounds.max.x - self.bounds.min.x) as usize;
            let index = (position.y - self.bounds.min.y) as usize * width
                + (position.x - self.bounds.min.x) as usize;
            self.terrain[index]
        })
    }
}

impl LoadedChunk {
    pub(crate) fn bounds(&self, coord: ChunkCoord) -> WorldRect {
        match self {
            Self::Bootstrap(chunk) => chunk.bounds,
            Self::Expansion(_) => coord
                .bounds()
                .expect("stored chunks always have representable bounds"),
        }
    }

    pub(crate) const fn is_expansion(&self) -> bool {
        matches!(self, Self::Expansion(_))
    }

    pub(crate) fn cell(&self, coord: ChunkCoord, position: WorldPosition) -> Option<TerrainCell> {
        match self {
            Self::Bootstrap(chunk) => chunk.cell(position),
            Self::Expansion(chunk) => {
                let origin = chunk_origin(coord);
                let x = (position.x - origin.x) as usize;
                let y = (position.y - origin.y) as usize;
                chunk.terrain.get(y * CHUNK_SIZE as usize + x).copied()
            }
        }
    }

    pub(crate) fn features(&self) -> &[Feature] {
        match self {
            Self::Bootstrap(chunk) => &chunk.features,
            Self::Expansion(chunk) => &chunk.features,
        }
    }

    pub(crate) fn cell_count(&self) -> usize {
        match self {
            Self::Bootstrap(chunk) => chunk.terrain.len(),
            Self::Expansion(chunk) => chunk.terrain.len(),
        }
    }
}
