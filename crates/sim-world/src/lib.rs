//! World storage, chunk streaming, and the entry points into the layered
//! terrain generation pipeline in [`crate::worldgen`].

mod archive;
mod chunk;
mod config;
mod features;
mod generator;
mod geometry;
mod loads;
mod storage;
mod terrain;
mod traversal;
mod validation;
mod worldgen;

pub use archive::{
    ArchiveBakeProgress, ArchiveBakeStats, ChunkOverview, WorldArchive, WorldArchiveError,
    WorldOverview,
};
pub use chunk::{ChunkCoord, ChunkInspection, ChunkLocalPosition, ChunkPresence, WorldChunk};
pub use config::{WorldConfig, WorldConfigError};
pub use features::{BaseResource, Feature, FeatureKind, ResourceKind};
pub use generator::{ChunkGenerator, GeneratedCell};
pub use geometry::{WorldPosition, WorldRect};
pub use loads::{ChunkLoadRequest, WorldChunkLoad};
pub use storage::World;
pub use terrain::{
    BiomeType, ClimateSample, PrevailingWind, SurfaceType, TerrainCell, TerrainClass, WaterSource,
};
pub use traversal::{
    MAX_TRAVERSABLE_ELEVATION_DELTA, MIN_TRAVERSAL_COST, Standability, TraversalKind,
    TraversalStep, WorldQueryError,
};
pub use validation::GenerateAreaError;

use worldgen::REGION_SIZE;

pub const WORLD_GENERATOR_VERSION: u32 = 2;

/// Default side length of the initially generated area.
pub const DEFAULT_INITIAL_WORLD_SIZE: u32 = 1_024;
const MAX_INITIAL_CELLS: u64 = 16_777_216; // 4,096 x 4,096
pub const WORLD_HALF_EXTENT: i64 = 32_768;
pub const WORLD_SIDE_CELLS: i64 = WORLD_HALF_EXTENT * 2;
pub const WORLD_GENERATION_BOUNDS: WorldRect = WorldRect {
    min: WorldPosition {
        x: -WORLD_HALF_EXTENT,
        y: -WORLD_HALF_EXTENT,
    },
    max: WorldPosition {
        x: WORLD_HALF_EXTENT,
        y: WORLD_HALF_EXTENT,
    },
};
pub const MAX_GENERATED_CELLS: u64 = (WORLD_SIDE_CELLS as u64) * (WORLD_SIDE_CELLS as u64);
pub const MAX_GENERATED_TERRAIN_BYTES: u64 = MAX_GENERATED_CELLS * 4;
/// Conservative clipped-tile ceiling derived from the initial-cell safety budget.
///
/// The centered spatial envelope makes the reachable count lower; this remains
/// a stable allocation/validation ceiling for callers that accept arbitrary
/// rectangular bootstrap dimensions.
pub const MAX_INITIAL_CHUNKS: usize = (MAX_INITIAL_CELLS / CHUNK_SIZE as u64) as usize;
pub const CHUNK_SIZE: i64 = 64;
/// Maximum number of previously missing chunks materialized by one request.
pub const MAX_CHUNKS_PER_GENERATION: u64 = 65_536;
pub const MAX_GENERATED_CHUNKS: usize =
    ((WORLD_SIDE_CELLS / CHUNK_SIZE) * (WORLD_SIDE_CELLS / CHUNK_SIZE)) as usize;
const CHUNKS_PER_REGION: i64 = REGION_SIZE / CHUNK_SIZE;
