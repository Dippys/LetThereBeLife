//! Deterministic per-chunk sampling and full-chunk synthesis.

use crate::{
    CHUNK_SIZE, ChunkCoord, ChunkLocalPosition, Feature, FeatureKind, GenerateAreaError,
    TerrainCell, WorldChunk, WorldPosition, chunk::chunk_origin, validation::validate_world_bounds,
    worldgen::ChunkContext,
};

/// A read-only deterministic sampler for one validated 64 x 64 chunk.
///
/// This supports tooling that needs a sparse overview without exposing the
/// generator's internal regional inputs as a public API.
pub struct ChunkGenerator {
    origin: WorldPosition,
    context: ChunkContext,
}

/// One procedurally generated cell and its optional sparse feature.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GeneratedCell {
    pub terrain: TerrainCell,
    pub feature: Option<FeatureKind>,
}

impl ChunkGenerator {
    /// Creates a sampler for a chunk with representable world bounds.
    pub fn new(seed: u64, coord: ChunkCoord) -> Result<Self, GenerateAreaError> {
        let bounds = coord.bounds()?;
        validate_world_bounds(bounds)?;
        let origin = bounds.min;
        Ok(Self {
            origin,
            context: ChunkContext::new(seed, origin.x, origin.y),
        })
    }

    /// Samples a chunk-local cell, returning `None` for coordinates outside
    /// the chunk's 0 through 63 local range.
    pub fn sample(&self, local: ChunkLocalPosition) -> Option<GeneratedCell> {
        if i64::from(local.x) >= CHUNK_SIZE || i64::from(local.y) >= CHUNK_SIZE {
            return None;
        }
        let (terrain, feature) = self.context.generate(
            self.origin.x + i64::from(local.x),
            self.origin.y + i64::from(local.y),
        );
        Some(GeneratedCell { terrain, feature })
    }
}

pub(crate) fn generate_chunk(seed: u64, coord: ChunkCoord) -> WorldChunk {
    let origin = chunk_origin(coord);
    let context = ChunkContext::new(seed, origin.x, origin.y);
    let mut terrain = Vec::with_capacity((CHUNK_SIZE * CHUNK_SIZE) as usize);
    let mut features = Vec::new();
    for y in origin.y..origin.y + CHUNK_SIZE {
        for x in origin.x..origin.x + CHUNK_SIZE {
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
    WorldChunk {
        coord,
        terrain,
        features,
    }
}
