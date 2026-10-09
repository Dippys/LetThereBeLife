//! Chunk addressing, inspection metadata, and full generated chunk payloads.

use crate::{CHUNK_SIZE, Feature, GenerateAreaError, TerrainCell, WorldPosition, WorldRect};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ChunkCoord {
    pub x: i64,
    pub y: i64,
}

impl ChunkCoord {
    /// Resolves the canonical chunk address for a signed world position.
    pub fn from_world_position(position: WorldPosition) -> Self {
        Self {
            x: position.x.div_euclid(CHUNK_SIZE),
            y: position.y.div_euclid(CHUNK_SIZE),
        }
    }

    /// Returns this chunk's half-open world bounds when both axes are representable.
    pub fn bounds(self) -> Result<WorldRect, GenerateAreaError> {
        let min = WorldPosition {
            x: self
                .x
                .checked_mul(CHUNK_SIZE)
                .ok_or(GenerateAreaError::TooLarge)?,
            y: self
                .y
                .checked_mul(CHUNK_SIZE)
                .ok_or(GenerateAreaError::TooLarge)?,
        };
        let max = WorldPosition {
            x: min
                .x
                .checked_add(CHUNK_SIZE)
                .ok_or(GenerateAreaError::TooLarge)?,
            y: min
                .y
                .checked_add(CHUNK_SIZE)
                .ok_or(GenerateAreaError::TooLarge)?,
        };
        Ok(WorldRect { min, max })
    }
}

/// How a complete chunk footprint is currently represented by the world.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChunkPresence {
    Missing,
    InitialUnloaded,
    PartialInitialUnloaded,
    PartialInitial,
    Initial,
    Retained,
    RetainedPartialInitial,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChunkLocalPosition {
    pub x: u8,
    pub y: u8,
}

/// Read-only chunk metadata for debugging and presentation clients.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChunkInspection {
    pub coord: ChunkCoord,
    pub bounds: WorldRect,
    pub local: ChunkLocalPosition,
    pub presence: ChunkPresence,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorldChunk {
    pub(crate) coord: ChunkCoord,
    pub(crate) terrain: Vec<TerrainCell>,
    pub(crate) features: Vec<Feature>,
}

impl WorldChunk {
    pub const fn coord(&self) -> ChunkCoord {
        self.coord
    }

    pub fn terrain(&self) -> &[TerrainCell] {
        &self.terrain
    }

    pub fn features(&self) -> &[Feature] {
        &self.features
    }
}

pub(crate) fn chunk_coord(position: WorldPosition) -> ChunkCoord {
    ChunkCoord::from_world_position(position)
}

pub(crate) fn chunk_origin(coord: ChunkCoord) -> WorldPosition {
    WorldPosition {
        x: coord.x * CHUNK_SIZE,
        y: coord.y * CHUNK_SIZE,
    }
}
