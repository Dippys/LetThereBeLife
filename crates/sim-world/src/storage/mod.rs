//! The `World` store: a declared bootstrap area plus a deterministic,
//! chunk-backed materialization cache.

use std::collections::{BTreeMap, HashMap};
use std::hash::{BuildHasherDefault, Hasher};

use crate::{
    ChunkCoord, ChunkInspection, ChunkLocalPosition, ChunkPresence, GenerateAreaError,
    MAX_CHUNKS_PER_GENERATION, WORLD_GENERATION_BOUNDS, WorldConfig, WorldPosition, WorldRect,
    geometry::intersection, loads::LoadedChunk,
};

mod loading;
mod queries;
pub use queries::{ChunkCells, traversal_between};
mod visits;

/// A declared bootstrap world with a deterministic, chunk-backed
/// materialization cache.
///
/// Construction is constant-time. The seed and configured rectangle define
/// terrain independently of cache residency; the rectangle is a bootstrap
/// target rather than a claim that every cell is already resident. Simulation
/// rules must not use resident coverage as time-varying domain state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct World {
    seed: u64,
    width: u32,
    height: u32,
    chunks: ChunkMap,
    revision: u64,
}

/// Resident chunks: found by coordinate through a hash index (lookups only,
/// many per tick), walked in coordinate order through a sorted one.
#[derive(Debug, Clone, Default)]
struct ChunkMap {
    chunks: Vec<LoadedChunk>,
    order: BTreeMap<ChunkCoord, usize>,
    lookup: HashMap<ChunkCoord, usize, BuildHasherDefault<CoordHasher>>,
}

impl PartialEq for ChunkMap {
    fn eq(&self, other: &Self) -> bool {
        self.len() == other.len() && self.iter().eq(other.iter())
    }
}

impl Eq for ChunkMap {}

impl ChunkMap {
    fn get(&self, coord: &ChunkCoord) -> Option<&LoadedChunk> {
        self.lookup.get(coord).map(|&index| &self.chunks[index])
    }

    fn insert(&mut self, coord: ChunkCoord, chunk: LoadedChunk) {
        match self.lookup.get(&coord) {
            Some(&index) => self.chunks[index] = chunk,
            None => {
                let index = self.chunks.len();
                self.chunks.push(chunk);
                self.order.insert(coord, index);
                self.lookup.insert(coord, index);
            }
        }
    }

    fn len(&self) -> usize {
        self.chunks.len()
    }

    /// In coordinate order.
    fn iter(&self) -> impl Iterator<Item = (&ChunkCoord, &LoadedChunk)> {
        self.order
            .iter()
            .map(|(coord, &index)| (coord, &self.chunks[index]))
    }
}

impl<'a> IntoIterator for &'a ChunkMap {
    type Item = (&'a ChunkCoord, &'a LoadedChunk);
    type IntoIter = Box<dyn Iterator<Item = Self::Item> + 'a>;

    fn into_iter(self) -> Self::IntoIter {
        Box::new(self.iter())
    }
}

/// A fast hash for chunk coordinates (a multiply and a fold), for the
/// lookup index only.
#[derive(Default)]
struct CoordHasher(u64);

impl Hasher for CoordHasher {
    fn write(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.write_u64(u64::from(byte));
        }
    }

    fn write_i64(&mut self, value: i64) {
        self.write_u64(value as u64);
    }

    fn write_i32(&mut self, value: i32) {
        self.write_u64(u64::from(value as u32));
    }

    fn write_u64(&mut self, value: u64) {
        self.0 = (self.0.rotate_left(5) ^ value).wrapping_mul(0x517c_c1b7_2722_0a95);
    }

    fn finish(&self) -> u64 {
        self.0 ^ (self.0 >> 29)
    }
}

impl World {
    /// Creates a declared but unmaterialized bootstrap world in constant time.
    pub fn new(seed: u64, config: WorldConfig) -> Self {
        Self {
            seed,
            width: config.initial_width(),
            height: config.initial_height(),
            chunks: ChunkMap::default(),
            revision: 0,
        }
    }

    /// Eagerly materializes the configured bootstrap area using a stable seed.
    ///
    /// This remains useful for deterministic headless work and focused tests.
    pub fn generate(seed: u64, config: WorldConfig) -> Self {
        let mut world = Self::new(seed, config);
        world
            .materialize_initial_area()
            .expect("validated bootstrap bounds must materialize");
        world.revision = 0;
        world
    }

    #[cfg(test)]
    fn generate_square(seed: u64, size: u32) -> Self {
        Self::generate(
            seed,
            WorldConfig::new(size, size).expect("test size is valid"),
        )
    }

    pub const fn width(&self) -> u32 {
        self.width
    }

    pub const fn height(&self) -> u32 {
        self.height
    }

    pub const fn revision(&self) -> u64 {
        self.revision
    }

    /// Returns the configured bootstrap rectangle, whether or not it is loaded.
    pub fn initial_bounds(&self) -> WorldRect {
        let min_x = -(i64::from(self.width) / 2);
        let min_y = -(i64::from(self.height) / 2);
        WorldRect {
            min: WorldPosition { x: min_x, y: min_y },
            max: WorldPosition {
                x: min_x + i64::from(self.width),
                y: min_y + i64::from(self.height),
            },
        }
    }

    /// Number of chunk records currently held, including clipped bootstrap tiles.
    pub fn loaded_chunk_count(&self) -> usize {
        self.chunks.len()
    }

    /// Eagerly completes the configured bootstrap rectangle.
    pub fn materialize_initial_area(&mut self) -> Result<(), GenerateAreaError> {
        self.materialize_initial_area_in_batches(MAX_CHUNKS_PER_GENERATION as usize)
    }

    /// Describes the chunk containing `position` without exposing mutable world storage.
    pub fn inspect_chunk_at(
        &self,
        position: WorldPosition,
    ) -> Result<ChunkInspection, GenerateAreaError> {
        if !WORLD_GENERATION_BOUNDS.contains(position) {
            return Err(GenerateAreaError::OutsideWorldBounds);
        }
        let coord = ChunkCoord::from_world_position(position);
        let bounds = coord.bounds()?;
        let bootstrap = self.bootstrap_coverage(coord);
        let presence = match (bootstrap, self.chunks.get(&coord)) {
            (None, None) => ChunkPresence::Missing,
            (Some(coverage), None) if coverage == bounds => ChunkPresence::InitialUnloaded,
            (Some(_), None) => ChunkPresence::PartialInitialUnloaded,
            (Some(coverage), Some(LoadedChunk::Bootstrap(_))) if coverage == bounds => {
                ChunkPresence::Initial
            }
            (Some(_), Some(LoadedChunk::Bootstrap(_))) => ChunkPresence::PartialInitial,
            (Some(coverage), Some(LoadedChunk::Expansion(_))) if coverage == bounds => {
                ChunkPresence::Initial
            }
            (Some(_), Some(LoadedChunk::Expansion(_))) => ChunkPresence::RetainedPartialInitial,
            (None, Some(_)) => ChunkPresence::Retained,
        };
        Ok(ChunkInspection {
            coord,
            bounds,
            local: ChunkLocalPosition {
                x: (position.x - bounds.min.x) as u8,
                y: (position.y - bounds.min.y) as u8,
            },
            presence,
        })
    }

    fn bootstrap_coverage(&self, coord: ChunkCoord) -> Option<WorldRect> {
        intersection(coord.bounds().ok()?, self.initial_bounds())
    }
}

#[cfg(test)]
mod tests;
