//! World storage, chunk streaming, and the entry points into the layered
//! terrain generation pipeline in [`crate::worldgen`].

use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fmt,
};

use crate::worldgen::{ChunkContext, REGION_SIZE, climate_at};

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

/// Validated initial generation dimensions, not a maximum world extent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorldConfig {
    initial_width: u32,
    initial_height: u32,
}

impl WorldConfig {
    pub fn new(initial_width: u32, initial_height: u32) -> Result<Self, WorldConfigError> {
        let cells = u64::from(initial_width) * u64::from(initial_height);
        if initial_width == 0 || initial_height == 0 {
            return Err(WorldConfigError::Empty);
        }
        if i64::from(initial_width) > WORLD_SIDE_CELLS
            || i64::from(initial_height) > WORLD_SIDE_CELLS
        {
            return Err(WorldConfigError::OutsideWorldBounds {
                width: initial_width,
                height: initial_height,
                maximum_side: WORLD_SIDE_CELLS as u32,
            });
        }
        if cells > MAX_INITIAL_CELLS {
            return Err(WorldConfigError::TooLarge {
                cells,
                maximum: MAX_INITIAL_CELLS,
            });
        }
        Ok(Self {
            initial_width,
            initial_height,
        })
    }

    pub const fn initial_width(self) -> u32 {
        self.initial_width
    }

    pub const fn initial_height(self) -> u32 {
        self.initial_height
    }
}

impl Default for WorldConfig {
    fn default() -> Self {
        Self {
            initial_width: DEFAULT_INITIAL_WORLD_SIZE,
            initial_height: DEFAULT_INITIAL_WORLD_SIZE,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorldConfigError {
    Empty,
    TooLarge {
        cells: u64,
        maximum: u64,
    },
    OutsideWorldBounds {
        width: u32,
        height: u32,
        maximum_side: u32,
    },
}

impl fmt::Display for WorldConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => formatter.write_str("width and height must both be greater than zero"),
            Self::TooLarge { cells, maximum } => write!(
                formatter,
                "requested {cells} initial cells, but the current safety limit is {maximum}"
            ),
            Self::OutsideWorldBounds {
                width,
                height,
                maximum_side,
            } => write!(
                formatter,
                "initial area {width}x{height} exceeds the centered world's {maximum_side}-cell side"
            ),
        }
    }
}

impl Error for WorldConfigError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorldPosition {
    pub x: i64,
    pub y: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorldRect {
    pub min: WorldPosition,
    pub max: WorldPosition,
}

impl WorldRect {
    pub fn from_inclusive_points(start: WorldPosition, end: WorldPosition) -> Self {
        Self {
            min: WorldPosition {
                x: start.x.min(end.x),
                y: start.y.min(end.y),
            },
            max: WorldPosition {
                x: start.x.max(end.x).saturating_add(1),
                y: start.y.max(end.y).saturating_add(1),
            },
        }
    }

    pub fn contains(self, position: WorldPosition) -> bool {
        position.x >= self.min.x
            && position.y >= self.min.y
            && position.x < self.max.x
            && position.y < self.max.y
    }

    pub fn contains_rect(self, other: Self) -> bool {
        self.min.x <= other.min.x
            && self.min.y <= other.min.y
            && self.max.x >= other.max.x
            && self.max.y >= other.max.y
    }

    pub fn intersects(self, other: Self) -> bool {
        self.max.x > other.min.x
            && self.max.y > other.min.y
            && self.min.x < other.max.x
            && self.min.y < other.max.y
    }

    pub fn intersection(self, other: Self) -> Option<Self> {
        intersection(self, other)
    }

    pub fn expanded(self, cells: i64) -> Self {
        Self {
            min: WorldPosition {
                x: self.min.x.saturating_sub(cells),
                y: self.min.y.saturating_sub(cells),
            },
            max: WorldPosition {
                x: self.max.x.saturating_add(cells),
                y: self.max.y.saturating_add(cells),
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum GroundType {
    DeepWater,
    ShallowWater,
    Sand,
    Grass,
    ForestFloor,
    Hill,
    BareRock,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PrevailingWind {
    Southeast,
    Northwest,
}

/// Diagnostic climate inputs for a generated cell. Temperature is derived
/// from the same 32-cell lattice used during classification; moisture remains
/// the compact value retained by `TerrainCell`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct ClimateSample {
    pub temperature: u16,
    pub moisture: u8,
    pub wind: PrevailingWind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct TerrainCell {
    pub elevation: u16,
    pub moisture: u8,
    pub ground: GroundType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeatureKind {
    Tree,
    Rock,
    BerryBush,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Feature {
    pub position: WorldPosition,
    pub kind: FeatureKind,
}

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
    coord: ChunkCoord,
    terrain: Vec<TerrainCell>,
    features: Vec<Feature>,
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

/// An opaque, deterministic request to materialize one chunk-sized world area.
///
/// Requests created by [`World::missing_chunk_load_requests`] preserve whether
/// a configured bootstrap edge must be clipped rather than exposing the
/// remainder of its 64 x 64 chunk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChunkLoadRequest {
    coord: ChunkCoord,
    bounds: WorldRect,
    kind: ChunkLoadKind,
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
enum ChunkLoadKind {
    Bootstrap,
    Expansion,
}

/// An opaque worker payload generated from a [`ChunkLoadRequest`].
///
/// The worker may construct this value, but only [`World::insert_chunk_loads`]
/// changes simulation-owned coverage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorldChunkLoad {
    seed: u64,
    request: ChunkLoadRequest,
    chunk: LoadedChunk,
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
enum LoadedChunk {
    Bootstrap(InitialChunk),
    Expansion(WorldChunk),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct InitialChunk {
    bounds: WorldRect,
    terrain: Box<[TerrainCell]>,
    features: Box<[Feature]>,
}

impl InitialChunk {
    fn generate(seed: u64, coord: ChunkCoord, bounds: WorldRect) -> Self {
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
    fn bounds(&self, coord: ChunkCoord) -> WorldRect {
        match self {
            Self::Bootstrap(chunk) => chunk.bounds,
            Self::Expansion(_) => coord
                .bounds()
                .expect("stored chunks always have representable bounds"),
        }
    }

    const fn is_expansion(&self) -> bool {
        matches!(self, Self::Expansion(_))
    }

    fn cell(&self, coord: ChunkCoord, position: WorldPosition) -> Option<TerrainCell> {
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

    fn features(&self) -> &[Feature] {
        match self {
            Self::Bootstrap(chunk) => &chunk.features,
            Self::Expansion(chunk) => &chunk.features,
        }
    }

    fn cell_count(&self) -> usize {
        match self {
            Self::Bootstrap(chunk) => chunk.terrain.len(),
            Self::Expansion(chunk) => chunk.terrain.len(),
        }
    }
}

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
    chunks: BTreeMap<ChunkCoord, LoadedChunk>,
    revision: u64,
}

impl World {
    /// Creates a declared but unmaterialized bootstrap world in constant time.
    pub fn new(seed: u64, config: WorldConfig) -> Self {
        Self {
            seed,
            width: config.initial_width(),
            height: config.initial_height(),
            chunks: BTreeMap::new(),
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

    /// Iterates resident cells in deterministic `ChunkCoord` order (`x`, then
    /// `y`), then in row-major order within each tile.
    ///
    /// This intentionally does not promise global world-row order, because
    /// sparse retained tiles need not form a dense rectangle.
    pub fn cells(&self) -> impl Iterator<Item = (WorldPosition, TerrainCell)> + '_ {
        self.chunks.iter().flat_map(|(&coord, chunk)| {
            let bounds = chunk.bounds(coord);
            let width = (bounds.max.x - bounds.min.x) as usize;
            (0..chunk.cell_count()).map(move |index| {
                let position = WorldPosition {
                    x: bounds.min.x + (index % width) as i64,
                    y: bounds.min.y + (index / width) as i64,
                };
                (
                    position,
                    chunk
                        .cell(coord, position)
                        .expect("stored chunk coverage must contain its cells"),
                )
            })
        })
    }

    /// Iterates resident sparse features in deterministic tile order.
    ///
    /// Tiles follow `ChunkCoord` order (`x`, then `y`); features within each
    /// tile retain generator row-major order. This intentionally does not
    /// promise global world-row order for sparse retained coverage.
    pub fn all_features(&self) -> impl Iterator<Item = &Feature> {
        self.chunks.iter().flat_map(|(&coord, chunk)| {
            let coverage = chunk.bounds(coord);
            chunk
                .features()
                .iter()
                .filter(move |feature| coverage.contains(feature.position))
        })
    }

    pub fn features_in(&self, bounds: WorldRect) -> impl Iterator<Item = &Feature> {
        self.all_features()
            .filter(move |feature| bounds.contains(feature.position))
    }

    pub fn visit_cells_in(
        &self,
        bounds: WorldRect,
        visitor: impl FnMut(WorldPosition, TerrainCell),
    ) {
        self.visit_cells_in_step(bounds, 1, visitor);
    }

    pub fn visit_cells_in_step(
        &self,
        bounds: WorldRect,
        step: u32,
        mut visitor: impl FnMut(WorldPosition, TerrainCell),
    ) {
        if bounds.max.x <= bounds.min.x || bounds.max.y <= bounds.min.y {
            return;
        }
        let step = i64::from(step.max(1));
        for (&coord, chunk) in &self.chunks {
            visit_loaded_chunk_region(chunk, coord, bounds, step, &mut visitor);
        }
    }

    pub fn visit_features_in(&self, bounds: WorldRect, mut visitor: impl FnMut(&Feature)) {
        if bounds.max.x <= bounds.min.x || bounds.max.y <= bounds.min.y {
            return;
        }
        for (&coord, chunk) in &self.chunks {
            let coverage = chunk.bounds(coord);
            for feature in chunk.features().iter().filter(|feature| {
                coverage.contains(feature.position) && bounds.contains(feature.position)
            }) {
                visitor(feature);
            }
        }
    }

    /// Eagerly completes the configured bootstrap rectangle.
    pub fn materialize_initial_area(&mut self) -> Result<(), GenerateAreaError> {
        self.materialize_initial_area_in_batches(MAX_CHUNKS_PER_GENERATION as usize)
    }

    fn materialize_initial_area_in_batches(
        &mut self,
        batch_size: usize,
    ) -> Result<(), GenerateAreaError> {
        assert!(
            batch_size > 0,
            "bootstrap batches must contain at least one chunk"
        );
        let bounds = self.initial_bounds();
        let span = validate_chunk_span(bounds)?;
        let mut loads = Vec::with_capacity(batch_size);
        for coord in span.coords() {
            let request = self.load_request_for(bounds, coord)?;
            if self
                .chunks
                .get(&coord)
                .is_some_and(|chunk| chunk.bounds(coord).contains_rect(request.bounds))
            {
                continue;
            }
            loads.push(Self::generate_chunk_load(self.seed, request));
            if loads.len() == batch_size {
                self.insert_chunk_loads(std::mem::take(&mut loads))?;
            }
        }
        if !loads.is_empty() {
            self.insert_chunk_loads(loads)?;
        }
        Ok(())
    }

    pub fn generate_area(&mut self, bounds: WorldRect) -> Result<(), GenerateAreaError> {
        if bounds == self.initial_bounds() {
            return self.materialize_initial_area();
        }
        let loads = self
            .missing_chunk_load_requests(bounds)?
            .into_iter()
            .map(|request| Self::generate_chunk_load(self.seed, request))
            .collect();
        self.insert_chunk_loads(loads)?;
        Ok(())
    }

    pub fn generate_chunks(
        seed: u64,
        bounds: WorldRect,
    ) -> Result<Vec<WorldChunk>, GenerateAreaError> {
        let span = validate_full_chunk_request(bounds)?;
        let mut chunks = Vec::with_capacity(span.total as usize);
        chunks.extend(span.coords().map(|coord| generate_chunk(seed, coord)));
        Ok(chunks)
    }

    pub fn generate_chunks_streaming(
        seed: u64,
        bounds: WorldRect,
    ) -> Result<impl Iterator<Item = WorldChunk>, GenerateAreaError> {
        let span = validate_full_chunk_request(bounds)?;
        Ok(span.coords().map(move |coord| generate_chunk(seed, coord)))
    }

    pub fn generate_chunk_at(
        seed: u64,
        coord: ChunkCoord,
    ) -> Result<WorldChunk, GenerateAreaError> {
        validate_chunk_axis(coord.x)?;
        validate_chunk_axis(coord.y)?;
        validate_world_bounds(coord.bounds()?)?;
        Ok(generate_chunk(seed, coord))
    }

    /// Generates one opaque materialization payload for a validated request.
    pub fn generate_chunk_load(seed: u64, request: ChunkLoadRequest) -> WorldChunkLoad {
        let chunk = match request.kind {
            ChunkLoadKind::Bootstrap => {
                LoadedChunk::Bootstrap(InitialChunk::generate(seed, request.coord, request.bounds))
            }
            ChunkLoadKind::Expansion => LoadedChunk::Expansion(generate_chunk(seed, request.coord)),
        };
        WorldChunkLoad {
            seed,
            request,
            chunk,
        }
    }

    /// Prepares shared deterministic regional inputs for a bounded request
    /// window. This is a performance hint only and does not materialize world
    /// state or change generated output.
    pub fn prepare_chunk_loads(seed: u64, requests: &[ChunkLoadRequest]) {
        crate::worldgen::prepare_chunk_regions(seed, requests);
    }

    /// Inserts full expansion chunks for callers that intentionally need whole
    /// chunk payloads. Viewer/bootstrap work should use [`Self::insert_chunk_loads`].
    pub fn insert_chunks(&mut self, chunks: Vec<WorldChunk>) -> Result<usize, GenerateAreaError> {
        let loads = chunks
            .into_iter()
            .map(|chunk| {
                let coord = chunk.coord();
                let bounds = coord
                    .bounds()
                    .expect("full chunks accepted by this API have representable bounds");
                WorldChunkLoad {
                    seed: self.seed,
                    request: ChunkLoadRequest {
                        coord,
                        bounds,
                        kind: ChunkLoadKind::Expansion,
                    },
                    chunk: LoadedChunk::Expansion(chunk),
                }
            })
            .collect();
        self.insert_chunk_loads(loads)
    }

    /// Applies worker-generated bootstrap or expansion payloads atomically with
    /// respect to retained expansion capacity.
    pub fn insert_chunk_loads(
        &mut self,
        loads: Vec<WorldChunkLoad>,
    ) -> Result<usize, GenerateAreaError> {
        let mut new_expansions = BTreeSet::new();
        for load in &loads {
            if load.seed != self.seed {
                return Err(GenerateAreaError::SeedMismatch {
                    expected: self.seed,
                    received: load.seed,
                });
            }
            self.validate_chunk_load_request(load.request)?;
            let coord = load.coord();
            let covered = self
                .chunks
                .get(&coord)
                .is_some_and(|existing| existing.bounds(coord).contains_rect(load.bounds()));
            if !covered
                && self.request_consumes_expansion_capacity(load.request)
                && !self
                    .chunks
                    .get(&coord)
                    .is_some_and(LoadedChunk::is_expansion)
            {
                new_expansions.insert(coord);
            }
        }
        self.ensure_chunk_capacity(new_expansions.len())?;

        let mut inserted = 0;
        for load in loads {
            let coord = load.coord();
            let target = load.bounds();
            let replace = match self.chunks.get(&coord) {
                None => true,
                Some(existing) => {
                    let existing_bounds = existing.bounds(coord);
                    !existing_bounds.contains_rect(target) && target.contains_rect(existing_bounds)
                }
            };
            if replace {
                self.chunks.insert(coord, load.chunk);
                inserted += 1;
            }
        }
        if inserted > 0 {
            self.revision = self.revision.saturating_add(1);
        }
        Ok(inserted)
    }

    /// Validates a world-aware request and returns its missing chunk count.
    pub fn validate_generation_request(&self, bounds: WorldRect) -> Result<u64, GenerateAreaError> {
        self.missing_chunk_load_requests(bounds)
            .map(|requests| requests.len() as u64)
    }

    pub fn missing_chunk_coords(
        &self,
        bounds: WorldRect,
    ) -> Result<Vec<ChunkCoord>, GenerateAreaError> {
        self.missing_chunk_load_requests(bounds)
            .map(|requests| requests.into_iter().map(ChunkLoadRequest::coord).collect())
    }

    /// Returns bounded, authoritative load requests for the uncovered portion
    /// of `bounds`. Requests remain region-major so repeated generation reuses
    /// the bounded regional hydrology cache.
    pub fn missing_chunk_load_requests(
        &self,
        bounds: WorldRect,
    ) -> Result<Vec<ChunkLoadRequest>, GenerateAreaError> {
        let span = validate_chunk_span(bounds)?;
        let mut requests = Vec::new();
        let mut new_expansions = BTreeSet::new();
        for coord in span.coords() {
            let request = self.load_request_for(bounds, coord)?;
            if self
                .chunks
                .get(&coord)
                .is_some_and(|chunk| chunk.bounds(coord).contains_rect(request.bounds))
            {
                continue;
            }
            if requests.len() == MAX_CHUNKS_PER_GENERATION as usize {
                return Err(GenerateAreaError::TooManyChunks {
                    requested: MAX_CHUNKS_PER_GENERATION + 1,
                    maximum: MAX_CHUNKS_PER_GENERATION,
                });
            }
            if self.request_consumes_expansion_capacity(request)
                && !self
                    .chunks
                    .get(&coord)
                    .is_some_and(LoadedChunk::is_expansion)
            {
                new_expansions.insert(coord);
            }
            requests.push(request);
        }
        self.ensure_chunk_capacity(new_expansions.len())?;
        Ok(requests)
    }

    fn load_request_for(
        &self,
        requested_bounds: WorldRect,
        coord: ChunkCoord,
    ) -> Result<ChunkLoadRequest, GenerateAreaError> {
        let chunk_bounds = coord.bounds()?;
        let selected = intersection(requested_bounds, chunk_bounds)
            .expect("chunk spans are derived from an intersecting request");
        let bootstrap = self.bootstrap_coverage(coord);
        Ok(match bootstrap {
            Some(bounds) if bounds.contains_rect(selected) => ChunkLoadRequest {
                coord,
                bounds,
                kind: ChunkLoadKind::Bootstrap,
            },
            _ => ChunkLoadRequest {
                coord,
                bounds: chunk_bounds,
                kind: ChunkLoadKind::Expansion,
            },
        })
    }

    fn validate_chunk_load_request(
        &self,
        request: ChunkLoadRequest,
    ) -> Result<(), GenerateAreaError> {
        let full_bounds = request.coord.bounds()?;
        let valid = WORLD_GENERATION_BOUNDS.contains_rect(full_bounds)
            && match request.kind {
                ChunkLoadKind::Bootstrap => {
                    self.bootstrap_coverage(request.coord) == Some(request.bounds)
                }
                ChunkLoadKind::Expansion => request.bounds == full_bounds,
            };
        valid
            .then_some(())
            .ok_or(GenerateAreaError::InvalidChunkLoad)
    }

    fn ensure_chunk_capacity(&self, requested: usize) -> Result<(), GenerateAreaError> {
        let remaining = MAX_GENERATED_CHUNKS.saturating_sub(self.generated_chunk_count());
        if requested > remaining {
            return Err(GenerateAreaError::WorldCapacity {
                requested,
                remaining,
            });
        }
        Ok(())
    }

    pub fn generated_chunk_count(&self) -> usize {
        self.chunks
            .iter()
            .filter(|(coord, chunk)| {
                chunk.is_expansion()
                    && self.request_consumes_expansion_capacity(ChunkLoadRequest {
                        coord: **coord,
                        bounds: chunk.bounds(**coord),
                        kind: ChunkLoadKind::Expansion,
                    })
            })
            .count()
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

    pub fn area_is_generated(&self, bounds: WorldRect) -> bool {
        self.missing_chunk_load_requests(bounds)
            .is_ok_and(|requests| requests.is_empty())
    }

    fn bootstrap_coverage(&self, coord: ChunkCoord) -> Option<WorldRect> {
        intersection(coord.bounds().ok()?, self.initial_bounds())
    }

    fn request_consumes_expansion_capacity(&self, request: ChunkLoadRequest) -> bool {
        request.kind == ChunkLoadKind::Expansion
            && !self
                .bootstrap_coverage(request.coord)
                .is_some_and(|coverage| coverage.contains_rect(request.bounds))
    }

    /// Finds a sparse surface feature without scanning the complete feature list.
    pub fn feature_at(&self, position: WorldPosition) -> Option<Feature> {
        let coord = chunk_coord(position);
        self.chunks.get(&coord).and_then(|chunk| {
            chunk.bounds(coord).contains(position).then(|| {
                chunk
                    .features()
                    .binary_search_by_key(&(position.y, position.x), |feature| {
                        (feature.position.y, feature.position.x)
                    })
                    .ok()
                    .and_then(|index| chunk.features().get(index))
                    .copied()
            })?
        })
    }

    pub fn cell(&self, position: WorldPosition) -> Option<TerrainCell> {
        let coord = chunk_coord(position);
        self.chunks
            .get(&coord)
            .and_then(|chunk| chunk.cell(coord, position))
    }

    /// Returns classification climate for a resident cell without allocating
    /// or materializing terrain or regional caches.
    pub fn climate_at(&self, position: WorldPosition) -> Option<ClimateSample> {
        let cell = self.cell(position)?;
        Some(climate_at(self.seed, position.x, position.y, cell.moisture))
    }

    /// Returns the exact resident tile coverage containing `position`.
    ///
    /// A configured bootstrap coordinate can be declared but not yet loaded, so
    /// presentation code must use this instead of configured dimensions when it
    /// needs to clip a sampled terrain block at a streamed edge.
    pub fn loaded_bounds_at(&self, position: WorldPosition) -> Option<WorldRect> {
        let coord = chunk_coord(position);
        self.chunks.get(&coord).and_then(|chunk| {
            let bounds = chunk.bounds(coord);
            bounds.contains(position).then_some(bounds)
        })
    }
}

#[derive(Clone, Copy)]
struct ChunkSpan {
    min: ChunkCoord,
    max: ChunkCoord,
    total: u128,
}

impl ChunkSpan {
    fn coords(self) -> impl Iterator<Item = ChunkCoord> {
        let min_region_x = self.min.x.div_euclid(CHUNKS_PER_REGION);
        let max_region_x = self.max.x.div_euclid(CHUNKS_PER_REGION);
        let min_region_y = self.min.y.div_euclid(CHUNKS_PER_REGION);
        let max_region_y = self.max.y.div_euclid(CHUNKS_PER_REGION);

        (min_region_y..=max_region_y).flat_map(move |region_y| {
            let region_min_y = region_y * CHUNKS_PER_REGION;
            let min_y = self.min.y.max(region_min_y);
            let max_y = self.max.y.min(region_min_y + CHUNKS_PER_REGION - 1);
            (min_region_x..=max_region_x).flat_map(move |region_x| {
                let region_min_x = region_x * CHUNKS_PER_REGION;
                let min_x = self.min.x.max(region_min_x);
                let max_x = self.max.x.min(region_min_x + CHUNKS_PER_REGION - 1);
                (min_y..=max_y).flat_map(move |y| (min_x..=max_x).map(move |x| ChunkCoord { x, y }))
            })
        })
    }
}

fn validate_chunk_span(bounds: WorldRect) -> Result<ChunkSpan, GenerateAreaError> {
    if bounds.max.x <= bounds.min.x || bounds.max.y <= bounds.min.y {
        return Err(GenerateAreaError::Empty);
    }
    validate_world_bounds(bounds)?;
    let min = chunk_coord(bounds.min);
    let max = chunk_coord(WorldPosition {
        x: bounds.max.x - 1,
        y: bounds.max.y - 1,
    });
    for coord in [min, max] {
        validate_chunk_axis(coord.x)?;
        validate_chunk_axis(coord.y)?;
    }
    let columns = (i128::from(max.x) - i128::from(min.x) + 1) as u128;
    let rows = (i128::from(max.y) - i128::from(min.y) + 1) as u128;
    let requested = columns
        .checked_mul(rows)
        .ok_or(GenerateAreaError::TooLarge)?;
    Ok(ChunkSpan {
        min,
        max,
        total: requested,
    })
}

fn validate_world_bounds(bounds: WorldRect) -> Result<(), GenerateAreaError> {
    WORLD_GENERATION_BOUNDS
        .contains_rect(bounds)
        .then_some(())
        .ok_or(GenerateAreaError::OutsideWorldBounds)
}

fn validate_full_chunk_request(bounds: WorldRect) -> Result<ChunkSpan, GenerateAreaError> {
    let span = validate_chunk_span(bounds)?;
    if span.total > u128::from(MAX_CHUNKS_PER_GENERATION) {
        return Err(GenerateAreaError::TooManyChunks {
            requested: span.total.min(u128::from(u64::MAX)) as u64,
            maximum: MAX_CHUNKS_PER_GENERATION,
        });
    }
    Ok(span)
}

fn align_to_step(value: i64, step: i64) -> i64 {
    let remainder = value.rem_euclid(step);
    if remainder == 0 {
        value
    } else {
        value.saturating_add(step - remainder)
    }
}

fn align_to_step_from(value: i64, origin: i64, step: i64) -> i64 {
    origin + align_to_step(value - origin, step)
}

fn intersection(left: WorldRect, right: WorldRect) -> Option<WorldRect> {
    let bounds = WorldRect {
        min: WorldPosition {
            x: left.min.x.max(right.min.x),
            y: left.min.y.max(right.min.y),
        },
        max: WorldPosition {
            x: left.max.x.min(right.max.x),
            y: left.max.y.min(right.max.y),
        },
    };
    (bounds.max.x > bounds.min.x && bounds.max.y > bounds.min.y).then_some(bounds)
}

fn visit_loaded_chunk_region(
    chunk: &LoadedChunk,
    coord: ChunkCoord,
    bounds: WorldRect,
    step: i64,
    visitor: &mut impl FnMut(WorldPosition, TerrainCell),
) {
    let region = chunk.bounds(coord);
    let Some(clipped) = intersection(region, bounds) else {
        return;
    };
    let origin = chunk_origin(coord);
    let start_x = align_to_step_from(clipped.min.x, origin.x, step);
    let start_y = align_to_step_from(clipped.min.y, origin.y, step);
    for y in (start_y..clipped.max.y).step_by(step as usize) {
        for x in (start_x..clipped.max.x).step_by(step as usize) {
            let position = WorldPosition { x, y };
            visitor(
                position,
                chunk
                    .cell(coord, position)
                    .expect("loaded chunk coverage must contain visited cells"),
            );
        }
    }
}

fn validate_chunk_axis(coord: i64) -> Result<(), GenerateAreaError> {
    coord
        .checked_mul(CHUNK_SIZE)
        .and_then(|origin| origin.checked_add(CHUNK_SIZE))
        .ok_or(GenerateAreaError::TooLarge)
        .map(|_| ())
}

fn chunk_coord(position: WorldPosition) -> ChunkCoord {
    ChunkCoord::from_world_position(position)
}

fn chunk_origin(coord: ChunkCoord) -> WorldPosition {
    WorldPosition {
        x: coord.x * CHUNK_SIZE,
        y: coord.y * CHUNK_SIZE,
    }
}

fn generate_chunk(seed: u64, coord: ChunkCoord) -> WorldChunk {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenerateAreaError {
    Empty,
    TooLarge,
    OutsideWorldBounds,
    TooManyChunks { requested: u64, maximum: u64 },
    WorldCapacity { requested: usize, remaining: usize },
    SeedMismatch { expected: u64, received: u64 },
    InvalidChunkLoad,
}

impl fmt::Display for GenerateAreaError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => formatter.write_str("generation bounds must have positive dimensions"),
            Self::TooLarge => formatter.write_str("generation coordinates exceed safe limits"),
            Self::OutsideWorldBounds => write!(
                formatter,
                "generation must stay inside [{}, {}) on both axes",
                -WORLD_HALF_EXTENT, WORLD_HALF_EXTENT
            ),
            Self::TooManyChunks { requested, maximum } => write!(
                formatter,
                "generation needs at least {requested} new chunks; maximum per request is {maximum}"
            ),
            Self::WorldCapacity {
                requested,
                remaining,
            } => write!(
                formatter,
                "generation needs {requested} new chunks but only {remaining} slots remain"
            ),
            Self::SeedMismatch { expected, received } => write!(
                formatter,
                "generated payload seed {received} does not match world seed {expected}"
            ),
            Self::InvalidChunkLoad => {
                formatter.write_str("generated payload does not match this world's chunk coverage")
            }
        }
    }
}

impl Error for GenerateAreaError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generation_is_deterministic() {
        assert_eq!(
            World::generate_square(42, 128),
            World::generate_square(42, 128)
        );
    }

    #[test]
    fn seed_changes_generated_terrain() {
        let left = World::generate_square(1, 128);
        let right = World::generate_square(2, 128);
        assert_ne!(
            left.cells().collect::<Vec<_>>(),
            right.cells().collect::<Vec<_>>()
        );
    }

    #[test]
    fn terrain_has_variation_and_sparse_features() {
        let world = World::generate_square(7, 256);
        let terrain: Vec<_> = world.cells().map(|(_, cell)| cell).collect();
        let features: Vec<_> = world.all_features().copied().collect();
        let first = terrain[0].ground;
        assert!(terrain.iter().any(|cell| cell.ground != first));
        assert!(!features.is_empty());
        assert!(features.len() < terrain.len() / 4);
        assert!(features.iter().all(|feature| matches!(
            world.cell(feature.position).unwrap().ground,
            GroundType::Grass | GroundType::ForestFloor | GroundType::Hill | GroundType::BareRock
        )));
    }

    #[test]
    fn terrain_records_keep_their_compact_layout() {
        assert_eq!(std::mem::size_of::<GroundType>(), 1);
        assert_eq!(std::mem::size_of::<TerrainCell>(), 4);
        assert_eq!(std::mem::size_of::<ClimateSample>(), 4);
    }

    #[test]
    fn centered_world_envelope_matches_the_raw_terrain_budget() {
        assert_eq!(
            WORLD_GENERATION_BOUNDS.min,
            WorldPosition {
                x: -32_768,
                y: -32_768
            }
        );
        assert_eq!(
            WORLD_GENERATION_BOUNDS.max,
            WorldPosition {
                x: 32_768,
                y: 32_768
            }
        );
        assert_eq!(MAX_GENERATED_CHUNKS, 1_048_576);
        assert_eq!(MAX_GENERATED_CELLS, 4_294_967_296);
        assert_eq!(MAX_GENERATED_TERRAIN_BYTES, 16 * 1_024 * 1_024 * 1_024);

        assert!(World::generate_chunk_at(1, ChunkCoord { x: -512, y: -512 }).is_ok());
        assert!(World::generate_chunk_at(1, ChunkCoord { x: 511, y: 511 }).is_ok());
        assert_eq!(
            World::generate_chunk_at(1, ChunkCoord { x: 512, y: 0 }),
            Err(GenerateAreaError::OutsideWorldBounds)
        );
    }

    #[test]
    fn initial_area_matches_independently_generated_chunks() {
        let seed = 19;
        let world = World::generate(seed, WorldConfig::new(128, 128).unwrap());
        for coord in [
            ChunkCoord { x: -1, y: -1 },
            ChunkCoord { x: -1, y: 0 },
            ChunkCoord { x: 0, y: -1 },
            ChunkCoord { x: 0, y: 0 },
        ] {
            let chunk = World::generate_chunk_at(seed, coord).unwrap();
            let origin = chunk_origin(coord);
            for (index, cell) in chunk.terrain().iter().enumerate() {
                let position = WorldPosition {
                    x: origin.x + index as i64 % CHUNK_SIZE,
                    y: origin.y + index as i64 / CHUNK_SIZE,
                };
                assert_eq!(world.cell(position), Some(*cell));
            }
            let expected_features: Vec<_> = world
                .all_features()
                .filter(|feature| chunk_coord(feature.position) == coord)
                .copied()
                .collect();
            assert_eq!(chunk.features(), expected_features);
        }
    }

    #[test]
    fn region_major_initial_generation_matches_chunks_and_keeps_feature_order() {
        let seed = 23;
        let width = (REGION_SIZE + CHUNK_SIZE) as u32;
        let world = World::generate(seed, WorldConfig::new(width, 128).unwrap());
        for coord in [ChunkCoord { x: -1, y: -1 }, ChunkCoord { x: 0, y: 0 }] {
            let chunk = World::generate_chunk_at(seed, coord).expect("coordinate is valid");
            let origin = chunk_origin(coord);
            for (index, cell) in chunk.terrain().iter().enumerate() {
                let position = WorldPosition {
                    x: origin.x + index as i64 % CHUNK_SIZE,
                    y: origin.y + index as i64 / CHUNK_SIZE,
                };
                assert_eq!(world.cell(position), Some(*cell));
            }
        }
        assert!(
            world
                .all_features()
                .all(|feature| world.feature_at(feature.position) == Some(*feature))
        );
    }

    #[test]
    fn chunk_generator_validates_inputs_and_matches_chunk_output() {
        let seed = 19;
        let coord = ChunkCoord { x: -1, y: 64 };
        let sampler = ChunkGenerator::new(seed, coord).expect("coordinate is representable");
        let chunk = World::generate_chunk_at(seed, coord).expect("coordinate is representable");
        for local in [
            ChunkLocalPosition { x: 0, y: 0 },
            ChunkLocalPosition { x: 31, y: 48 },
            ChunkLocalPosition { x: 63, y: 63 },
        ] {
            let sampled = sampler.sample(local).expect("local coordinate is valid");
            let index = usize::from(local.y) * CHUNK_SIZE as usize + usize::from(local.x);
            assert_eq!(sampled.terrain, chunk.terrain()[index]);
            let origin = chunk_origin(coord);
            let position = WorldPosition {
                x: origin.x + i64::from(local.x),
                y: origin.y + i64::from(local.y),
            };
            assert_eq!(
                sampled.feature,
                chunk
                    .features()
                    .iter()
                    .find(|feature| feature.position == position)
                    .map(|feature| feature.kind)
            );
        }
        assert!(sampler.sample(ChunkLocalPosition { x: 64, y: 0 }).is_none());
        assert!(matches!(
            ChunkGenerator::new(seed, ChunkCoord { x: i64::MAX, y: 0 }),
            Err(GenerateAreaError::TooLarge)
        ));
    }

    #[test]
    fn cell_rejects_out_of_bounds_positions() {
        let world = World::generate_square(1, 16);
        assert!(world.cell(WorldPosition { x: 7, y: 7 }).is_some());
        assert!(world.cell(WorldPosition { x: 8, y: 0 }).is_none());
    }

    #[test]
    fn sparse_features_are_sorted_and_addressable() {
        let world = World::generate_square(7, 256);
        let feature = world
            .all_features()
            .next()
            .copied()
            .expect("generated world has a sparse feature");
        assert_eq!(world.feature_at(feature.position), Some(feature));
        assert_eq!(
            world.feature_at(WorldPosition {
                x: i64::from(world.width()),
                y: 0
            }),
            None
        );
    }

    #[test]
    fn configured_initial_area_can_be_rectangular() {
        let config = WorldConfig::new(96, 64).unwrap();
        let world = World::generate(3, config);
        assert_eq!((world.width(), world.height()), (96, 64));
        assert_eq!(world.cells().count(), 96 * 64);
    }

    #[test]
    fn resident_iterators_use_documented_tile_then_local_order() {
        let world = World::generate(3, WorldConfig::new(128, 128).unwrap());
        let positions: Vec<_> = world.cells().map(|(position, _)| position).collect();

        assert_eq!(positions[0], WorldPosition { x: -64, y: -64 });
        assert_eq!(positions[1], WorldPosition { x: -63, y: -64 });
        assert_eq!(positions[64], WorldPosition { x: -64, y: -63 });
        assert_eq!(positions[4_095], WorldPosition { x: -1, y: -1 });
        assert_eq!(positions[4_096], WorldPosition { x: -64, y: 0 });
        assert_eq!(positions[8_192], WorldPosition { x: 0, y: -64 });

        let features: Vec<_> = world.all_features().copied().collect();
        assert!(features.windows(2).all(|pair| {
            let left_chunk = ChunkCoord::from_world_position(pair[0].position);
            let right_chunk = ChunkCoord::from_world_position(pair[1].position);
            left_chunk < right_chunk
                || (left_chunk == right_chunk
                    && (pair[0].position.y < pair[1].position.y
                        || (pair[0].position.y == pair[1].position.y
                            && pair[0].position.x <= pair[1].position.x)))
        }));
    }

    #[test]
    fn deferred_world_declares_bootstrap_without_claiming_loaded_cells() {
        let world = World::new(3, WorldConfig::new(96, 100).unwrap());
        let bounds = world.initial_bounds();

        assert_eq!(world.loaded_chunk_count(), 0);
        assert_eq!(world.cells().count(), 0);
        assert_eq!(world.all_features().count(), 0);
        assert!(!world.area_is_generated(bounds));
        assert_eq!(world.cell(WorldPosition { x: 0, y: 0 }), None);
        assert_eq!(world.loaded_bounds_at(WorldPosition { x: 0, y: 0 }), None);
        assert_eq!(
            world
                .inspect_chunk_at(WorldPosition { x: 0, y: 0 })
                .unwrap()
                .presence,
            ChunkPresence::PartialInitialUnloaded
        );
        assert_eq!(
            world
                .inspect_chunk_at(WorldPosition { x: 47, y: 49 })
                .unwrap()
                .presence,
            ChunkPresence::PartialInitialUnloaded
        );
        assert_eq!(world.missing_chunk_load_requests(bounds).unwrap().len(), 4);
    }

    #[test]
    fn streamed_bootstrap_matches_eager_content_for_aligned_and_clipped_worlds() {
        for config in [
            WorldConfig::new(128, 128).unwrap(),
            WorldConfig::new(96, 100).unwrap(),
        ] {
            let eager = World::generate(41, config);
            let mut streamed = World::new(41, config);
            let loads = streamed
                .missing_chunk_load_requests(streamed.initial_bounds())
                .unwrap()
                .into_iter()
                .rev()
                .map(|request| World::generate_chunk_load(41, request))
                .collect();

            assert_eq!(
                streamed.insert_chunk_loads(loads),
                Ok(eager.loaded_chunk_count())
            );
            assert_eq!(
                streamed.cells().collect::<Vec<_>>(),
                eager.cells().collect::<Vec<_>>()
            );
            assert_eq!(
                streamed.all_features().copied().collect::<Vec<_>>(),
                eager.all_features().copied().collect::<Vec<_>>()
            );
            assert!(streamed.area_is_generated(streamed.initial_bounds()));
        }
    }

    #[test]
    fn clipped_bootstrap_generation_matches_the_corresponding_full_chunk_subset() {
        let seed = 41;
        let mut world = World::new(seed, WorldConfig::new(96, 64).unwrap());
        let request = world
            .missing_chunk_load_requests(world.initial_bounds())
            .unwrap()
            .into_iter()
            .find(|request| request.coord() == ChunkCoord { x: 0, y: 0 })
            .unwrap();
        let full_chunk = World::generate_chunk_at(seed, request.coord()).unwrap();
        let full_bounds = request.coord().bounds().unwrap();

        world
            .insert_chunk_loads(vec![World::generate_chunk_load(seed, request)])
            .unwrap();

        for y in request.bounds().min.y..request.bounds().max.y {
            for x in request.bounds().min.x..request.bounds().max.x {
                let index = ((y - full_bounds.min.y) * CHUNK_SIZE + x - full_bounds.min.x) as usize;
                assert_eq!(
                    world.cell(WorldPosition { x, y }),
                    Some(full_chunk.terrain()[index])
                );
            }
        }
        let expected_features: Vec<_> = full_chunk
            .features()
            .iter()
            .filter(|feature| request.bounds().contains(feature.position))
            .copied()
            .collect();
        assert_eq!(
            world.all_features().copied().collect::<Vec<_>>(),
            expected_features
        );
    }

    #[test]
    fn eager_bootstrap_materialization_batches_without_changing_content() {
        let config = WorldConfig::new(128, 64).unwrap();
        let expected = World::generate(41, config);
        let mut world = World::new(41, config);

        world.materialize_initial_area_in_batches(1).unwrap();

        assert_eq!(world.loaded_chunk_count(), 4);
        assert_eq!(world.revision(), 4);
        assert_eq!(
            world.cells().collect::<Vec<_>>(),
            expected.cells().collect::<Vec<_>>()
        );
        assert_eq!(
            world.all_features().copied().collect::<Vec<_>>(),
            expected.all_features().copied().collect::<Vec<_>>()
        );
    }

    #[test]
    fn clipped_bootstrap_tile_hides_its_fringe_until_promoted_to_an_expansion() {
        let mut world = World::new(7, WorldConfig::new(96, 64).unwrap());
        let bootstrap = world
            .missing_chunk_load_requests(world.initial_bounds())
            .unwrap()
            .into_iter()
            .map(|request| World::generate_chunk_load(7, request))
            .collect();
        world.insert_chunk_loads(bootstrap).unwrap();

        let bootstrap_bounds = WorldRect {
            min: WorldPosition { x: 0, y: 0 },
            max: WorldPosition { x: 48, y: 32 },
        };
        assert!(world.cell(WorldPosition { x: 47, y: 31 }).is_some());
        assert_eq!(world.cell(WorldPosition { x: 48, y: 0 }), None);
        assert_eq!(
            world.loaded_bounds_at(WorldPosition { x: 47, y: 0 }),
            Some(bootstrap_bounds)
        );
        assert_eq!(world.loaded_bounds_at(WorldPosition { x: 48, y: 0 }), None);
        assert_eq!(world.generated_chunk_count(), 0);

        let expansion_bounds = WorldRect {
            min: WorldPosition { x: 48, y: 0 },
            max: WorldPosition { x: 64, y: 32 },
        };
        let expansion = world
            .missing_chunk_load_requests(expansion_bounds)
            .unwrap()
            .into_iter()
            .map(|request| World::generate_chunk_load(7, request))
            .collect();
        assert_eq!(world.insert_chunk_loads(expansion), Ok(1));

        assert!(world.cell(WorldPosition { x: 48, y: 0 }).is_some());
        assert_eq!(
            world.loaded_bounds_at(WorldPosition { x: 48, y: 0 }),
            Some(ChunkCoord { x: 0, y: 0 }.bounds().unwrap())
        );
        assert_eq!(world.generated_chunk_count(), 1);
        assert_eq!(
            world
                .inspect_chunk_at(WorldPosition { x: 48, y: 0 })
                .unwrap()
                .presence,
            ChunkPresence::RetainedPartialInitial
        );
    }

    #[test]
    fn opaque_loads_reject_wrong_seed_or_incompatible_bootstrap_coverage() {
        let config = WorldConfig::new(96, 64).unwrap();
        let mut world = World::new(7, config);
        let request = world
            .missing_chunk_load_requests(world.initial_bounds())
            .unwrap()
            .into_iter()
            .next()
            .unwrap();
        let wrong_seed = World::generate_chunk_load(8, request);

        assert_eq!(
            world.insert_chunk_loads(vec![wrong_seed]),
            Err(GenerateAreaError::SeedMismatch {
                expected: 7,
                received: 8,
            })
        );
        assert_eq!(world.loaded_chunk_count(), 0);
        assert_eq!(world.revision(), 0);

        let source = World::new(7, WorldConfig::new(128, 64).unwrap());
        let incompatible_request = source
            .missing_chunk_load_requests(source.initial_bounds())
            .unwrap()
            .into_iter()
            .find(|request| request.coord() == ChunkCoord { x: -1, y: 0 })
            .unwrap();
        let incompatible = World::generate_chunk_load(7, incompatible_request);

        assert_eq!(
            world.insert_chunk_loads(vec![incompatible]),
            Err(GenerateAreaError::InvalidChunkLoad)
        );
        assert_eq!(world.loaded_chunk_count(), 0);
        assert_eq!(world.revision(), 0);
    }

    #[test]
    fn overlapping_initial_areas_generate_identical_cells() {
        let small = World::generate(11, WorldConfig::new(96, 64).unwrap());
        let large = World::generate(11, WorldConfig::new(128, 96).unwrap());
        for position in [
            WorldPosition { x: 0, y: 0 },
            WorldPosition { x: 31, y: 31 },
            WorldPosition { x: 47, y: 31 },
        ] {
            assert_eq!(small.cell(position), large.cell(position));
            assert_eq!(small.feature_at(position), large.feature_at(position));
        }
    }

    #[test]
    fn initial_area_rejects_unsafe_dimensions() {
        assert_eq!(WorldConfig::new(0, 10), Err(WorldConfigError::Empty));
        assert!(WorldConfig::new(4_096, 4_096).is_ok());
        assert!(matches!(
            WorldConfig::new(4_097, 4_096),
            Err(WorldConfigError::TooLarge { .. })
        ));
        assert!(matches!(
            WorldConfig::new(1, 16_777_216),
            Err(WorldConfigError::OutsideWorldBounds { .. })
        ));
        assert_eq!(MAX_INITIAL_CHUNKS, 262_144);
    }

    #[test]
    fn generated_area_adds_deterministic_negative_world_space() {
        let bounds = WorldRect::from_inclusive_points(
            WorldPosition { x: -8, y: -6 },
            WorldPosition { x: -1, y: -1 },
        );
        let mut left = World::generate(33, WorldConfig::new(64, 64).unwrap());
        let mut right = World::generate(33, WorldConfig::new(64, 64).unwrap());
        left.generate_area(bounds).unwrap();
        right.generate_area(bounds).unwrap();
        assert_eq!(left, right);
        assert!(left.cell(WorldPosition { x: -4, y: -3 }).is_some());
    }

    #[test]
    fn generated_area_accepts_large_selection_beyond_prior_cap() {
        let mut world = World::generate(1, WorldConfig::new(64, 64).unwrap());
        let bounds = WorldRect::from_inclusive_points(
            WorldPosition { x: 2_000, y: 2_000 },
            WorldPosition { x: 4_000, y: 4_000 },
        );
        world
            .generate_area(bounds)
            .expect("large selection now allowed");
        assert!(world.area_is_generated(bounds));
        assert!(world.cell(WorldPosition { x: 4_000, y: 4_000 }).is_some());
    }

    #[test]
    fn generated_area_rejects_overflowing_selection() {
        let mut world = World::generate(1, WorldConfig::new(64, 64).unwrap());
        let bounds = WorldRect {
            min: WorldPosition {
                x: i64::MIN,
                y: i64::MIN,
            },
            max: WorldPosition {
                x: i64::MAX,
                y: i64::MAX,
            },
        };
        assert_eq!(
            world.generate_area(bounds),
            Err(GenerateAreaError::OutsideWorldBounds)
        );
    }

    #[test]
    fn generated_area_rejects_more_than_large_chunk_budget() {
        let bounds = WorldRect {
            min: WorldPosition { x: 0, y: 0 },
            max: WorldPosition {
                x: 363 * CHUNK_SIZE,
                y: 363 * CHUNK_SIZE,
            },
        };
        assert!(matches!(
            World::generate_chunks_streaming(1, bounds),
            Err(GenerateAreaError::TooManyChunks { .. })
        ));
    }

    #[test]
    fn generation_budget_accepts_limit_and_rejects_one_more() {
        let maximum = WorldRect {
            min: WorldPosition {
                x: -128 * CHUNK_SIZE,
                y: -128 * CHUNK_SIZE,
            },
            max: WorldPosition {
                x: 128 * CHUNK_SIZE,
                y: 128 * CHUNK_SIZE,
            },
        };
        assert!(World::generate_chunks_streaming(1, maximum).is_ok());

        let over = WorldRect {
            min: WorldPosition {
                x: -128 * CHUNK_SIZE,
                y: -128 * CHUNK_SIZE,
            },
            max: WorldPosition {
                x: 129 * CHUNK_SIZE,
                y: 128 * CHUNK_SIZE,
            },
        };
        assert_eq!(
            World::generate_chunks_streaming(1, over).map(|_| ()),
            Err(GenerateAreaError::TooManyChunks {
                requested: 257 * 256,
                maximum: MAX_CHUNKS_PER_GENERATION,
            })
        );
    }

    #[test]
    fn generation_budget_counts_only_missing_chunks() {
        let world = World::generate(1, WorldConfig::new(64, 64).unwrap());
        let outside_initial = WorldRect {
            min: WorldPosition {
                x: CHUNK_SIZE,
                y: -128 * CHUNK_SIZE,
            },
            max: WorldPosition {
                x: 257 * CHUNK_SIZE,
                y: 128 * CHUNK_SIZE,
            },
        };
        assert_eq!(
            world.validate_generation_request(outside_initial),
            Ok(MAX_CHUNKS_PER_GENERATION)
        );
        let missing = world.missing_chunk_coords(outside_initial).unwrap();
        assert_eq!(missing.len(), MAX_CHUNKS_PER_GENERATION as usize);
        assert!(!missing.contains(&ChunkCoord { x: 0, y: 0 }));

        let requires_one_over = WorldRect {
            min: WorldPosition {
                x: CHUNK_SIZE,
                y: -128 * CHUNK_SIZE,
            },
            max: WorldPosition {
                x: 258 * CHUNK_SIZE,
                y: 128 * CHUNK_SIZE,
            },
        };
        let over_limit = GenerateAreaError::TooManyChunks {
            requested: MAX_CHUNKS_PER_GENERATION + 1,
            maximum: MAX_CHUNKS_PER_GENERATION,
        };
        assert_eq!(
            world.validate_generation_request(requires_one_over),
            Err(over_limit)
        );
        assert_eq!(
            world.missing_chunk_coords(requires_one_over),
            Err(over_limit)
        );
    }

    #[test]
    fn missing_budget_counts_partial_initial_boundary_chunk() {
        let world = World::generate(1, WorldConfig::new(96, 64).unwrap());
        let bounds = WorldRect {
            min: WorldPosition { x: 64, y: 0 },
            max: WorldPosition { x: 128, y: 64 },
        };
        assert_eq!(world.validate_generation_request(bounds), Ok(1));
        assert_eq!(
            world.missing_chunk_coords(bounds).unwrap(),
            vec![ChunkCoord { x: 1, y: 0 }]
        );
    }

    #[test]
    fn chunk_inspection_tracks_signed_boundaries_and_partial_initial_coverage() {
        let mut world = World::generate(1, WorldConfig::new(96, 64).unwrap());
        let cases = [
            (
                WorldPosition { x: -65, y: 0 },
                ChunkCoord { x: -2, y: 0 },
                ChunkLocalPosition { x: 63, y: 0 },
                ChunkPresence::Missing,
            ),
            (
                WorldPosition { x: -64, y: 0 },
                ChunkCoord { x: -1, y: 0 },
                ChunkLocalPosition { x: 0, y: 0 },
                ChunkPresence::PartialInitial,
            ),
            (
                WorldPosition { x: -1, y: 0 },
                ChunkCoord { x: -1, y: 0 },
                ChunkLocalPosition { x: 63, y: 0 },
                ChunkPresence::PartialInitial,
            ),
            (
                WorldPosition { x: 0, y: 0 },
                ChunkCoord { x: 0, y: 0 },
                ChunkLocalPosition { x: 0, y: 0 },
                ChunkPresence::PartialInitial,
            ),
            (
                WorldPosition { x: 47, y: 0 },
                ChunkCoord { x: 0, y: 0 },
                ChunkLocalPosition { x: 47, y: 0 },
                ChunkPresence::PartialInitial,
            ),
            (
                WorldPosition { x: 48, y: 0 },
                ChunkCoord { x: 0, y: 0 },
                ChunkLocalPosition { x: 48, y: 0 },
                ChunkPresence::PartialInitial,
            ),
            (
                WorldPosition { x: 64, y: 0 },
                ChunkCoord { x: 1, y: 0 },
                ChunkLocalPosition { x: 0, y: 0 },
                ChunkPresence::Missing,
            ),
        ];

        for (position, coord, local, presence) in cases {
            let inspection = world.inspect_chunk_at(position).unwrap();
            assert_eq!(inspection.coord, coord);
            assert_eq!(inspection.local, local);
            assert_eq!(inspection.presence, presence);
            assert!(inspection.bounds.contains(position));
        }

        world
            .generate_area(WorldRect {
                min: WorldPosition { x: 48, y: 0 },
                max: WorldPosition { x: 64, y: 32 },
            })
            .unwrap();
        assert_eq!(
            world
                .inspect_chunk_at(WorldPosition { x: 48, y: 0 })
                .unwrap()
                .presence,
            ChunkPresence::RetainedPartialInitial
        );
    }

    #[test]
    fn chunk_inspection_rejects_an_unrepresentable_positive_edge() {
        let world = World::generate(1, WorldConfig::new(64, 64).unwrap());
        assert_eq!(
            world.inspect_chunk_at(WorldPosition { x: i64::MAX, y: 0 }),
            Err(GenerateAreaError::OutsideWorldBounds)
        );
    }

    #[test]
    fn huge_world_aware_request_stops_at_missing_chunk_limit() {
        let world = World::generate(1, WorldConfig::new(64, 64).unwrap());
        let bounds = WorldRect {
            min: WorldPosition { x: 0, y: 0 },
            max: WorldPosition {
                x: 1_000_000_000,
                y: 1,
            },
        };
        assert_eq!(
            world.validate_generation_request(bounds),
            Err(GenerateAreaError::OutsideWorldBounds)
        );
    }

    #[test]
    fn one_dimensional_extreme_range_is_rejected() {
        let bounds = WorldRect {
            min: WorldPosition { x: i64::MIN, y: 0 },
            max: WorldPosition { x: 0, y: 1 },
        };
        assert!(matches!(
            World::generate_chunks_streaming(1, bounds),
            Err(GenerateAreaError::OutsideWorldBounds)
        ));
    }

    #[test]
    fn chunk_at_positive_coordinate_limit_is_rejected() {
        let bounds = WorldRect {
            min: WorldPosition {
                x: i64::MAX - 10,
                y: 0,
            },
            max: WorldPosition { x: i64::MAX, y: 1 },
        };
        assert_eq!(
            World::generate_chunks_streaming(1, bounds).map(|_| ()),
            Err(GenerateAreaError::OutsideWorldBounds)
        );
    }

    #[test]
    fn chunk_outside_centered_world_is_rejected() {
        let coord = ChunkCoord {
            x: i64::MIN / CHUNK_SIZE,
            y: 0,
        };
        assert_eq!(
            World::generate_chunk_at(1, coord),
            Err(GenerateAreaError::OutsideWorldBounds)
        );
    }

    #[test]
    fn generated_chunks_are_keyed_and_do_not_duplicate_initial_cells() {
        let mut world = World::generate(5, WorldConfig::new(64, 64).unwrap());
        let bounds = WorldRect::from_inclusive_points(
            WorldPosition { x: 60, y: 0 },
            WorldPosition { x: 70, y: 10 },
        );
        world.generate_area(bounds).unwrap();
        assert_eq!(world.revision(), 1);
        assert!(world.area_is_generated(bounds));
        assert_eq!(world.cells().count(), 11_264);
        assert!(world.cell(WorldPosition { x: 70, y: 10 }).is_some());
    }

    #[test]
    fn generating_existing_chunks_does_not_advance_revision() {
        let mut world = World::generate(5, WorldConfig::new(64, 64).unwrap());
        let bounds = WorldRect::from_inclusive_points(
            WorldPosition { x: -10, y: -10 },
            WorldPosition { x: -1, y: -1 },
        );
        world.generate_area(bounds).unwrap();
        let revision = world.revision();
        world.generate_area(bounds).unwrap();
        assert_eq!(world.revision(), revision);
    }

    #[test]
    fn missing_chunks_skip_initial_and_already_generated_areas() {
        let mut world = World::generate(5, WorldConfig::new(64, 64).unwrap());
        let first = WorldRect {
            min: WorldPosition { x: -64, y: 0 },
            max: WorldPosition { x: 128, y: 64 },
        };
        assert_eq!(
            world.missing_chunk_coords(first).unwrap(),
            vec![
                ChunkCoord { x: -1, y: 0 },
                ChunkCoord { x: 0, y: 0 },
                ChunkCoord { x: 1, y: 0 },
            ]
        );
        world.generate_area(first).unwrap();

        let extended = WorldRect {
            min: first.min,
            max: WorldPosition { x: 192, y: 64 },
        };
        assert_eq!(
            world.missing_chunk_coords(extended).unwrap(),
            vec![ChunkCoord { x: 2, y: 0 }]
        );
    }

    #[test]
    fn stepped_cell_visit_samples_initial_and_generated_chunks() {
        let mut world = World::generate(5, WorldConfig::new(64, 64).unwrap());
        world
            .generate_area(WorldRect {
                min: WorldPosition { x: -64, y: 0 },
                max: WorldPosition { x: 0, y: 64 },
            })
            .unwrap();
        let mut positions = Vec::new();
        world.visit_cells_in_step(
            WorldRect {
                min: WorldPosition { x: -64, y: 0 },
                max: WorldPosition { x: 64, y: 64 },
            },
            4,
            |position, _| positions.push(position),
        );

        assert_eq!(positions.len(), 320);
        assert!(
            positions
                .iter()
                .all(|position| position.x.rem_euclid(4) == 0 && position.y.rem_euclid(4) == 0)
        );
    }

    #[test]
    fn generate_chunks_streaming_matches_batch_output() {
        let bounds = WorldRect::from_inclusive_points(
            WorldPosition { x: -64, y: -128 },
            WorldPosition { x: 200, y: -1 },
        );
        let batch = World::generate_chunks(11, bounds).expect("valid bounds");
        let streamed: Vec<_> = World::generate_chunks_streaming(11, bounds)
            .expect("valid bounds")
            .collect();
        assert_eq!(batch.len(), streamed.len());
        assert!(batch.iter().zip(streamed.iter()).all(|(a, b)| a == b));
    }

    #[test]
    fn chunk_spans_visit_complete_region_groups_before_the_next_region() {
        let coords: Vec<_> = ChunkSpan {
            min: ChunkCoord { x: 0, y: 0 },
            max: ChunkCoord { x: 64, y: 1 },
            total: 130,
        }
        .coords()
        .collect();
        assert_eq!(coords[64], ChunkCoord { x: 0, y: 1 });
        assert_eq!(coords[128], ChunkCoord { x: 64, y: 0 });
    }

    #[test]
    fn generated_chunk_store_enforces_total_capacity_without_allocating_the_limit() {
        let mut world = World::generate(1, WorldConfig::new(64, 64).unwrap());
        assert_eq!(world.ensure_chunk_capacity(MAX_GENERATED_CHUNKS), Ok(()));
        assert_eq!(
            world.ensure_chunk_capacity(MAX_GENERATED_CHUNKS + 1),
            Err(GenerateAreaError::WorldCapacity {
                requested: MAX_GENERATED_CHUNKS + 1,
                remaining: MAX_GENERATED_CHUNKS,
            })
        );

        assert_eq!(
            world.insert_chunks(vec![WorldChunk {
                coord: ChunkCoord { x: -1, y: 0 },
                terrain: Vec::new(),
                features: Vec::new(),
            }]),
            Ok(1)
        );
        assert_eq!(
            world.ensure_chunk_capacity(MAX_GENERATED_CHUNKS),
            Err(GenerateAreaError::WorldCapacity {
                requested: MAX_GENERATED_CHUNKS,
                remaining: MAX_GENERATED_CHUNKS - 1,
            })
        );
    }
}
