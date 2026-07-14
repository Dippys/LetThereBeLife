//! World storage, chunk streaming, and the entry points into the layered
//! terrain generation pipeline in [`crate::worldgen`].

use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fmt,
};

use crate::worldgen::{ChunkContext, REGION_SIZE, climate_at};

mod queries;
mod visits;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
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
pub enum SurfaceType {
    DeepWater,
    ShallowWater,
    Sand,
    Soil,
    Hill,
    Rock,
    SnowIce,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum BiomeType {
    Ocean,
    Lake,
    River,
    Beach,
    Desert,
    Grassland,
    Savanna,
    Forest,
    Wetland,
    Tundra,
    Alpine,
}

/// Packed rendered surface and environmental biome classification.
///
/// The low nibble stores [`SurfaceType`] and the high nibble stores
/// [`BiomeType`]. Construction is private to this crate, so every bit pattern
/// held by a public [`TerrainCell`] resolves to valid enums through safe
/// accessors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct TerrainClass(u8);

impl TerrainClass {
    pub(crate) const fn new(surface: SurfaceType, biome: BiomeType) -> Self {
        Self((biome as u8) << 4 | surface as u8)
    }

    pub fn surface(self) -> SurfaceType {
        match self.0 & 0x0f {
            0 => SurfaceType::DeepWater,
            1 => SurfaceType::ShallowWater,
            2 => SurfaceType::Sand,
            3 => SurfaceType::Soil,
            4 => SurfaceType::Hill,
            5 => SurfaceType::Rock,
            6 => SurfaceType::SnowIce,
            _ => unreachable!("TerrainClass is constructed from SurfaceType"),
        }
    }

    pub fn biome(self) -> BiomeType {
        match self.0 >> 4 {
            0 => BiomeType::Ocean,
            1 => BiomeType::Lake,
            2 => BiomeType::River,
            3 => BiomeType::Beach,
            4 => BiomeType::Desert,
            5 => BiomeType::Grassland,
            6 => BiomeType::Savanna,
            7 => BiomeType::Forest,
            8 => BiomeType::Wetland,
            9 => BiomeType::Tundra,
            10 => BiomeType::Alpine,
            _ => unreachable!("TerrainClass is constructed from BiomeType"),
        }
    }

    pub const fn packed(self) -> u8 {
        self.0
    }
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
    class: TerrainClass,
}

impl TerrainCell {
    pub(crate) const fn new(
        elevation: u16,
        moisture: u8,
        surface: SurfaceType,
        biome: BiomeType,
    ) -> Self {
        Self {
            elevation,
            moisture,
            class: TerrainClass::new(surface, biome),
        }
    }

    pub fn surface(self) -> SurfaceType {
        self.class.surface()
    }

    pub fn biome(self) -> BiomeType {
        self.class.biome()
    }

    pub const fn classification(self) -> TerrainClass {
        self.class
    }
}

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

/// Generated water-body identity at one resident cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum WaterSource {
    Ocean,
    Lake,
    River,
}

impl WaterSource {
    /// The first physical-agent loop can drink untreated lake and river water;
    /// ocean water is deliberately not drinkable.
    pub const fn is_drinkable(self) -> bool {
        matches!(self, Self::Lake | Self::River)
    }
}

/// Outcome of one cardinal movement query.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum TraversalKind {
    Passable,
    BlockedByWater,
    BlockedBySlope,
    BlockedByFeature,
}

/// Allocation-free derived movement information for two adjacent resident cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct TraversalStep {
    elevation_delta: i32,
    cost: u16,
    kind: TraversalKind,
}

impl TraversalStep {
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

fn water_source(cell: TerrainCell) -> Option<WaterSource> {
    match cell.biome() {
        BiomeType::Ocean => Some(WaterSource::Ocean),
        BiomeType::Lake => Some(WaterSource::Lake),
        BiomeType::River => Some(WaterSource::River),
        _ => None,
    }
}

fn surface_traversal_cost(surface: SurfaceType) -> u16 {
    match surface {
        SurfaceType::Sand => 14,
        SurfaceType::Soil => 10,
        SurfaceType::Hill => 18,
        SurfaceType::Rock => 22,
        SurfaceType::SnowIce => 20,
        SurfaceType::DeepWater | SurfaceType::ShallowWater => 0,
    }
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
mod tests;
