//! Deterministic terrain and sparse surface-feature generation.

use std::{collections::BTreeMap, error::Error, fmt};

/// Default side length of the initially generated area.
pub const DEFAULT_INITIAL_WORLD_SIZE: u32 = 1_024;
const MAX_INITIAL_CELLS: u64 = 16_777_216;
pub const CHUNK_SIZE: i64 = 64;
/// Maximum number of previously missing chunks materialized by one request.
//pub const MAX_CHUNKS_PER_GENERATION: u64 = 4_096; ORIGINAL VALUE
//pub const MAX_GENERATED_CHUNKS: usize = 16_384; ORIGINAL VALUE
pub const MAX_CHUNKS_PER_GENERATION: u64 = 131_072; // TEMP: Increased to allow for larger world generation requests
pub const MAX_GENERATED_CHUNKS: usize = 262_144; // TEMP: Increased to allow for larger world generation requests
const NOISE_MAX: i64 = 65_535;
const DEEP_WATER_MAX: u16 = 25_000;
const SHALLOW_WATER_MAX: u16 = 30_000;
const SAND_MAX: u16 = 33_000;
const INLAND_LAKE_MIN_CONTINENTALNESS: i64 = 38_000;
const INLAND_LAKE_REGION_SIZE: i64 = 1_024;
const INLAND_LAKE_CENTER_MARGIN: i64 = 240;
const INLAND_LAKE_CENTER_SPAN: i64 = 544;
const INLAND_LAKE_MIN_RADIUS_X: i64 = 112;
const INLAND_LAKE_MIN_RADIUS_Y: i64 = 80;
const INLAND_LAKE_RADIUS_SPAN: i64 = 112;
const INLAND_LAKE_DEEP_INSET: i64 = 40;
const INLAND_LAKE_SHORE_INSET: i64 = 16;
const INLAND_LAKE_SHORE_VARIATION_DIVISOR: i64 = 2_048;
const INLAND_LAKE_MAX_SHORE_VARIATION: i64 = 16;
const INLAND_LAKE_FLOOR: i64 = 22_000;
const INLAND_LAKE_SHALLOW: i64 = 28_000;
const INLAND_LAKE_SHORE: i64 = 32_000;
const RIVER_REGION_SIZE: i64 = 2_048;
const RIVER_NODE_SIDE: usize = 16;
const RIVER_NODE_STEP: i64 = RIVER_REGION_SIZE / RIVER_NODE_SIDE as i64;
const RIVER_NODE_OFFSET: i64 = RIVER_NODE_STEP / 2;
const RIVER_OUTLET_ATTEMPTS: usize = 8;
const RIVER_MIN_LAND_NODES: usize = 7;
const RIVER_MAX_LAND_NODES: usize = 14;
const RIVER_MAX_COARSE_POINTS: usize = RIVER_MAX_LAND_NODES + 1;
const RIVER_REFINED_MAX_POINTS: usize = RIVER_MAX_COARSE_POINTS * 2 - 1;
const RIVER_SMOOTHING_PASSES: usize = 1;
const RIVER_MAX_POINTS: usize = RIVER_REFINED_MAX_POINTS << RIVER_SMOOTHING_PASSES;
const RIVER_REFINEMENT_OFFSETS: [i64; 5] = [-32, -16, 0, 16, 32];
const RIVER_SOURCE_MIN_ELEVATION: u16 = 50_001;
const RIVER_TURN_PENALTY: u32 = 2_048;
const RIVER_SOURCE_HALF_WIDTH: i64 = 8;
const RIVER_HEADWATER_HALF_WIDTH: i64 = 10;
const RIVER_MOUTH_HALF_WIDTH: i64 = 24;
const RIVER_DEEP_ELEVATION: i64 = 24_000;
const RIVER_SHALLOW_ELEVATION: i64 = 28_000;
const RIVER_BANK_ELEVATION: i64 = 32_000;
const CONTINENT_SEED: u64 = 0x434f_4e54_494e_454e;
const BROAD_RELIEF_SEED: u64 = 0x434f_4153_544c_494e;
const REGIONAL_SEED: u64 = 0x5245_4749_4f4e_414c;
const INLAND_LAKE_SEED: u64 = 0x4c41_4b45_4241_5349;
const RIVER_OUTLET_SEED: u64 = 0x5249_5645_524f_5554;
const RIVER_ROUTE_SEED: u64 = 0x5249_5645_5250_4154;

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
    TooLarge { cells: u64, maximum: u64 },
}

impl fmt::Display for WorldConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => formatter.write_str("width and height must both be greater than zero"),
            Self::TooLarge { cells, maximum } => write!(
                formatter,
                "requested {cells} initial cells, but the current safety limit is {maximum}"
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

/// A generated base world. Terrain is dense; interactive objects remain sparse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct World {
    seed: u64,
    width: u32,
    height: u32,
    terrain: Vec<TerrainCell>,
    features: Vec<Feature>,
    chunks: BTreeMap<ChunkCoord, WorldChunk>,
    revision: u64,
}

impl World {
    /// Generates the configured initial area using a stable seed.
    pub fn generate(seed: u64, config: WorldConfig) -> Self {
        Self::generate_initial_area(seed, config.initial_width(), config.initial_height())
    }

    #[cfg(test)]
    fn generate_square(seed: u64, size: u32) -> Self {
        Self::generate_initial_area(seed, size, size)
    }

    fn generate_initial_area(seed: u64, width: u32, height: u32) -> Self {
        let capacity = width as usize * height as usize;
        let mut terrain = vec![
            TerrainCell {
                elevation: 0,
                moisture: 0,
                ground: GroundType::DeepWater,
            };
            capacity
        ];
        let mut features = Vec::new();

        // Chunk-major generation establishes stable ordering for later streaming.
        let chunks_x = width.div_ceil(CHUNK_SIZE as u32);
        let chunks_y = height.div_ceil(CHUNK_SIZE as u32);
        for chunk_y in 0..chunks_y {
            for chunk_x in 0..chunks_x {
                let origin_x = i64::from(chunk_x) * CHUNK_SIZE;
                let origin_y = i64::from(chunk_y) * CHUNK_SIZE;
                let lake = lake_descriptor(seed, origin_x, origin_y);
                let river = river_for_chunk(
                    seed,
                    WorldPosition {
                        x: origin_x,
                        y: origin_y,
                    },
                );
                let end_y = ((chunk_y + 1) * CHUNK_SIZE as u32).min(height);
                let end_x = ((chunk_x + 1) * CHUNK_SIZE as u32).min(width);
                for y in chunk_y * CHUNK_SIZE as u32..end_y {
                    for x in chunk_x * CHUNK_SIZE as u32..end_x {
                        let x = i64::from(x);
                        let y = i64::from(y);
                        let (cell, feature) = generate_cell(seed, x, y, lake, &river);
                        terrain[(y as u32 * width + x as u32) as usize] = cell;

                        if let Some(kind) = feature {
                            features.push(Feature {
                                position: WorldPosition { x, y },
                                kind,
                            });
                        }
                    }
                }
            }
        }
        features.sort_unstable_by_key(|feature| position_key(feature.position, width));

        Self {
            seed,
            width,
            height,
            terrain,
            features,
            chunks: BTreeMap::new(),
            revision: 0,
        }
    }

    pub const fn width(&self) -> u32 {
        self.width
    }

    pub const fn height(&self) -> u32 {
        self.height
    }

    pub fn terrain(&self) -> &[TerrainCell] {
        &self.terrain
    }

    pub fn features(&self) -> &[Feature] {
        &self.features
    }

    pub const fn revision(&self) -> u64 {
        self.revision
    }

    pub fn cells(&self) -> impl Iterator<Item = (WorldPosition, TerrainCell)> + '_ {
        let width = self.width as usize;
        let initial = self
            .terrain
            .iter()
            .copied()
            .enumerate()
            .map(move |(index, cell)| {
                (
                    WorldPosition {
                        x: (index % width) as i64,
                        y: (index / width) as i64,
                    },
                    cell,
                )
            });
        let initial_width = i64::from(self.width);
        let initial_height = i64::from(self.height);
        let generated = self
            .chunks
            .values()
            .flat_map(|chunk| {
                let origin = chunk_origin(chunk.coord);
                chunk
                    .terrain
                    .iter()
                    .copied()
                    .enumerate()
                    .map(move |(index, cell)| {
                        (
                            WorldPosition {
                                x: origin.x + index as i64 % CHUNK_SIZE,
                                y: origin.y + index as i64 / CHUNK_SIZE,
                            },
                            cell,
                        )
                    })
            })
            .filter(move |(position, _)| {
                position.x < 0
                    || position.y < 0
                    || position.x >= initial_width
                    || position.y >= initial_height
            });
        initial.chain(generated)
    }

    pub fn all_features(&self) -> impl Iterator<Item = &Feature> {
        let initial_width = i64::from(self.width);
        let initial_height = i64::from(self.height);
        self.features.iter().chain(
            self.chunks
                .values()
                .flat_map(|chunk| chunk.features.iter())
                .filter(move |feature| {
                    feature.position.x < 0
                        || feature.position.y < 0
                        || feature.position.x >= initial_width
                        || feature.position.y >= initial_height
                }),
        )
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
        let step = i64::from(step.max(1));
        let min_x = bounds.min.x.max(0).min(i64::from(self.width));
        let min_y = bounds.min.y.max(0).min(i64::from(self.height));
        let max_x = bounds.max.x.max(0).min(i64::from(self.width));
        let max_y = bounds.max.y.max(0).min(i64::from(self.height));
        let min_x = align_to_step(min_x, step);
        let min_y = align_to_step(min_y, step);
        for y in (min_y..max_y).step_by(step as usize) {
            let row = y as usize * self.width as usize;
            for x in (min_x..max_x).step_by(step as usize) {
                visitor(WorldPosition { x, y }, self.terrain[row + x as usize]);
            }
        }

        if bounds.max.x <= bounds.min.x || bounds.max.y <= bounds.min.y {
            return;
        }
        for chunk in self
            .chunks
            .values()
            .filter(|chunk| chunk_intersects(bounds, chunk.coord))
        {
            let origin = chunk_origin(chunk.coord);
            let chunk_max = WorldPosition {
                x: origin.x + CHUNK_SIZE,
                y: origin.y + CHUNK_SIZE,
            };
            let initial_max = WorldPosition {
                x: i64::from(self.width),
                y: i64::from(self.height),
            };
            let middle_min_y = origin.y.max(0);
            let middle_max_y = chunk_max.y.min(initial_max.y);
            let outside_initial = [
                WorldRect {
                    min: origin,
                    max: WorldPosition {
                        x: chunk_max.x,
                        y: chunk_max.y.min(0),
                    },
                },
                WorldRect {
                    min: WorldPosition {
                        x: origin.x,
                        y: origin.y.max(initial_max.y),
                    },
                    max: chunk_max,
                },
                WorldRect {
                    min: WorldPosition {
                        x: origin.x,
                        y: middle_min_y,
                    },
                    max: WorldPosition {
                        x: chunk_max.x.min(0),
                        y: middle_max_y,
                    },
                },
                WorldRect {
                    min: WorldPosition {
                        x: origin.x.max(initial_max.x),
                        y: middle_min_y,
                    },
                    max: WorldPosition {
                        x: chunk_max.x,
                        y: middle_max_y,
                    },
                },
            ];
            for region in outside_initial {
                visit_chunk_region(chunk, origin, region, bounds, step, &mut visitor);
            }
        }
    }

    pub fn visit_features_in(&self, bounds: WorldRect, mut visitor: impl FnMut(&Feature)) {
        for feature in self
            .features
            .iter()
            .filter(|feature| bounds.contains(feature.position))
        {
            visitor(feature);
        }
        if bounds.max.x <= bounds.min.x || bounds.max.y <= bounds.min.y {
            return;
        }
        for chunk in self
            .chunks
            .values()
            .filter(|chunk| chunk_intersects(bounds, chunk.coord))
        {
            for feature in chunk.features.iter().filter(|feature| {
                bounds.contains(feature.position)
                    && (feature.position.x < 0
                        || feature.position.y < 0
                        || feature.position.x >= i64::from(self.width)
                        || feature.position.y >= i64::from(self.height))
            }) {
                visitor(feature);
            }
        }
    }

    pub fn generate_area(&mut self, bounds: WorldRect) -> Result<(), GenerateAreaError> {
        let coords = self.missing_chunk_coords(bounds)?;
        if coords.is_empty() {
            return Ok(());
        }
        let chunks = coords
            .into_iter()
            .map(|coord| generate_chunk(self.seed, coord))
            .collect();
        self.insert_chunks(chunks)?;
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
        Ok(generate_chunk(seed, coord))
    }

    pub fn insert_chunks(&mut self, chunks: Vec<WorldChunk>) -> Result<usize, GenerateAreaError> {
        let missing: std::collections::BTreeSet<_> = chunks
            .iter()
            .map(WorldChunk::coord)
            .filter(|coord| !self.chunks.contains_key(coord))
            .collect();
        let remaining = MAX_GENERATED_CHUNKS.saturating_sub(self.chunks.len());
        if missing.len() > remaining {
            return Err(GenerateAreaError::WorldCapacity {
                requested: missing.len(),
                remaining,
            });
        }
        let mut inserted = 0;
        for chunk in chunks {
            if let std::collections::btree_map::Entry::Vacant(entry) =
                self.chunks.entry(chunk.coord)
            {
                entry.insert(chunk);
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
        self.visit_missing_chunks_bounded(bounds, |_| {})
            .map(|missing| missing as u64)
    }

    pub fn missing_chunk_coords(
        &self,
        bounds: WorldRect,
    ) -> Result<Vec<ChunkCoord>, GenerateAreaError> {
        let mut missing = Vec::new();
        self.visit_missing_chunks_bounded(bounds, |coord| missing.push(coord))?;
        Ok(missing)
    }

    fn visit_missing_chunks_bounded(
        &self,
        bounds: WorldRect,
        mut visitor: impl FnMut(ChunkCoord),
    ) -> Result<usize, GenerateAreaError> {
        let span = validate_chunk_span(bounds)?;
        if self.initial_bounds().contains_rect(bounds) {
            return Ok(0);
        }
        let mut missing = 0;
        for coord in span
            .coords()
            .filter(|coord| self.chunk_is_missing(bounds, *coord))
        {
            missing += 1;
            if missing > MAX_CHUNKS_PER_GENERATION as usize {
                return Err(GenerateAreaError::TooManyChunks {
                    requested: missing as u64,
                    maximum: MAX_CHUNKS_PER_GENERATION,
                });
            }
            visitor(coord);
        }
        self.ensure_chunk_capacity(missing)?;
        Ok(missing)
    }

    fn chunk_is_missing(&self, bounds: WorldRect, coord: ChunkCoord) -> bool {
        self.selection_needs_chunk(bounds, coord) && !self.chunks.contains_key(&coord)
    }

    fn ensure_chunk_capacity(&self, requested: usize) -> Result<(), GenerateAreaError> {
        let remaining = MAX_GENERATED_CHUNKS.saturating_sub(self.chunks.len());
        if requested > remaining {
            return Err(GenerateAreaError::WorldCapacity {
                requested,
                remaining,
            });
        }
        Ok(())
    }

    pub fn generated_chunk_count(&self) -> usize {
        self.chunks.len()
    }

    /// Describes the chunk containing `position` without exposing mutable world storage.
    pub fn inspect_chunk_at(
        &self,
        position: WorldPosition,
    ) -> Result<ChunkInspection, GenerateAreaError> {
        let coord = ChunkCoord::from_world_position(position);
        let bounds = coord.bounds()?;
        let initial = self.initial_bounds();
        let retained = self.chunks.contains_key(&coord);
        let presence = if initial.contains_rect(bounds) {
            ChunkPresence::Initial
        } else if rects_intersect(initial, bounds) {
            if retained {
                ChunkPresence::RetainedPartialInitial
            } else {
                ChunkPresence::PartialInitial
            }
        } else if retained {
            ChunkPresence::Retained
        } else {
            ChunkPresence::Missing
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
        let Ok(span) = validate_chunk_span(bounds) else {
            return false;
        };
        if self.initial_bounds().contains_rect(bounds) {
            return true;
        }
        span.coords().all(|coord| {
            !self.selection_needs_chunk(bounds, coord) || self.chunks.contains_key(&coord)
        })
    }

    fn selection_needs_chunk(&self, bounds: WorldRect, coord: ChunkCoord) -> bool {
        let origin = chunk_origin(coord);
        let selected_part = WorldRect {
            min: WorldPosition {
                x: bounds.min.x.max(origin.x),
                y: bounds.min.y.max(origin.y),
            },
            max: WorldPosition {
                x: bounds.max.x.min(origin.x + CHUNK_SIZE),
                y: bounds.max.y.min(origin.y + CHUNK_SIZE),
            },
        };
        !self.initial_bounds().contains_rect(selected_part)
    }

    fn initial_bounds(&self) -> WorldRect {
        WorldRect {
            min: WorldPosition { x: 0, y: 0 },
            max: WorldPosition {
                x: i64::from(self.width),
                y: i64::from(self.height),
            },
        }
    }

    /// Finds a sparse surface feature without scanning the complete feature list.
    pub fn feature_at(&self, position: WorldPosition) -> Option<Feature> {
        if position.x >= 0
            && position.y >= 0
            && position.x < i64::from(self.width)
            && position.y < i64::from(self.height)
        {
            let key = position_key(position, self.width);
            return self
                .features
                .binary_search_by_key(&key, |feature| position_key(feature.position, self.width))
                .ok()
                .and_then(|index| self.features.get(index))
                .copied();
        }
        self.chunks
            .get(&chunk_coord(position))
            .and_then(|chunk| {
                chunk
                    .features
                    .binary_search_by_key(&(position.y, position.x), |feature| {
                        (feature.position.y, feature.position.x)
                    })
                    .ok()
                    .and_then(|index| chunk.features.get(index))
            })
            .copied()
    }

    pub fn cell(&self, position: WorldPosition) -> Option<TerrainCell> {
        if position.x >= 0
            && position.y >= 0
            && position.x < i64::from(self.width)
            && position.y < i64::from(self.height)
        {
            return self
                .terrain
                .get((position.y as u32 * self.width + position.x as u32) as usize)
                .copied();
        }
        let coord = chunk_coord(position);
        let origin = chunk_origin(coord);
        self.chunks
            .get(&coord)
            .and_then(|chunk| {
                let x = (position.x - origin.x) as usize;
                let y = (position.y - origin.y) as usize;
                chunk.terrain.get(y * CHUNK_SIZE as usize + x)
            })
            .copied()
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
        (self.min.y..=self.max.y)
            .flat_map(move |y| (self.min.x..=self.max.x).map(move |x| ChunkCoord { x, y }))
    }
}

fn validate_chunk_span(bounds: WorldRect) -> Result<ChunkSpan, GenerateAreaError> {
    if bounds.max.x <= bounds.min.x || bounds.max.y <= bounds.min.y {
        return Err(GenerateAreaError::Empty);
    }
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

fn chunk_intersects(bounds: WorldRect, coord: ChunkCoord) -> bool {
    let origin = chunk_origin(coord);
    bounds.max.x > origin.x
        && bounds.max.y > origin.y
        && bounds.min.x < origin.x + CHUNK_SIZE
        && bounds.min.y < origin.y + CHUNK_SIZE
}

fn rects_intersect(left: WorldRect, right: WorldRect) -> bool {
    left.max.x > right.min.x
        && left.max.y > right.min.y
        && left.min.x < right.max.x
        && left.min.y < right.max.y
}

fn visit_chunk_region(
    chunk: &WorldChunk,
    origin: WorldPosition,
    region: WorldRect,
    bounds: WorldRect,
    step: i64,
    visitor: &mut impl FnMut(WorldPosition, TerrainCell),
) {
    let clipped = WorldRect {
        min: WorldPosition {
            x: region.min.x.max(bounds.min.x),
            y: region.min.y.max(bounds.min.y),
        },
        max: WorldPosition {
            x: region.max.x.min(bounds.max.x),
            y: region.max.y.min(bounds.max.y),
        },
    };
    if clipped.max.x <= clipped.min.x || clipped.max.y <= clipped.min.y {
        return;
    }
    let start_x = align_to_step_from(clipped.min.x, region.min.x, step);
    let start_y = align_to_step_from(clipped.min.y, region.min.y, step);
    for y in (start_y..clipped.max.y).step_by(step as usize) {
        for x in (start_x..clipped.max.x).step_by(step as usize) {
            let index = ((y - origin.y) * CHUNK_SIZE + x - origin.x) as usize;
            visitor(WorldPosition { x, y }, chunk.terrain[index]);
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
    let lake = lake_descriptor(seed, origin.x, origin.y);
    let river = river_for_chunk(seed, origin);
    let mut terrain = Vec::with_capacity((CHUNK_SIZE * CHUNK_SIZE) as usize);
    let mut features = Vec::new();
    for y in origin.y..origin.y + CHUNK_SIZE {
        for x in origin.x..origin.x + CHUNK_SIZE {
            let (cell, feature) = generate_cell(seed, x, y, lake, &river);
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

const fn position_key(position: WorldPosition, width: u32) -> u64 {
    position.y as u64 * width as u64 + position.x as u64
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenerateAreaError {
    Empty,
    TooLarge,
    TooManyChunks { requested: u64, maximum: u64 },
    WorldCapacity { requested: usize, remaining: usize },
}

impl fmt::Display for GenerateAreaError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => formatter.write_str("generation bounds must have positive dimensions"),
            Self::TooLarge => formatter.write_str("generation coordinates exceed safe limits"),
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
        }
    }
}

impl Error for GenerateAreaError {}

fn generate_cell(
    seed: u64,
    x: i64,
    y: i64,
    lake: Option<LakeDescriptor>,
    rivers: &ChunkRivers,
) -> (TerrainCell, Option<FeatureKind>) {
    let elevation = terrain_elevation(seed, x, y, lake, rivers);
    let moisture = moisture_noise(seed, x, y);
    let ground = classify_ground(elevation, moisture);
    let cell = TerrainCell {
        elevation,
        moisture: (moisture >> 8) as u8,
        ground,
    };
    (cell, generate_feature(seed, x, y, ground, moisture))
}

fn classify_ground(elevation: u16, moisture: u16) -> GroundType {
    match elevation {
        0..=DEEP_WATER_MAX => GroundType::DeepWater,
        _ if elevation <= SHALLOW_WATER_MAX => GroundType::ShallowWater,
        _ if elevation <= SAND_MAX => GroundType::Sand,
        50_001..=56_000 => GroundType::Hill,
        56_001..=u16::MAX => GroundType::BareRock,
        _ if moisture > 37_000 => GroundType::ForestFloor,
        _ => GroundType::Grass,
    }
}

fn generate_feature(
    seed: u64,
    x: impl Into<i64>,
    y: impl Into<i64>,
    ground: GroundType,
    moisture: u16,
) -> Option<FeatureKind> {
    if !matches!(
        ground,
        GroundType::Grass | GroundType::ForestFloor | GroundType::Hill | GroundType::BareRock
    ) {
        return None;
    }
    let roll = hash(seed ^ 0x4654_5253, x.into(), y.into()) % 10_000;
    match ground {
        GroundType::ForestFloor if roll < 700 => Some(FeatureKind::Tree),
        GroundType::Grass if moisture > 31_000 && roll < 90 => Some(FeatureKind::Tree),
        GroundType::Grass if roll < 125 => Some(FeatureKind::BerryBush),
        GroundType::Hill | GroundType::BareRock if roll < 180 => Some(FeatureKind::Rock),
        _ => None,
    }
}

fn terrain_elevation(
    seed: u64,
    x: i64,
    y: i64,
    lake: Option<LakeDescriptor>,
    rivers: &ChunkRivers,
) -> u16 {
    let sample = continental_elevation(seed, x, y);
    let elevation = inland_lake_elevation(x, y, sample, lake).unwrap_or(sample.elevation);
    river_elevation(x, y, rivers).map_or(elevation, |river| elevation.min(river)) as u16
}

#[derive(Clone, Copy)]
struct ContinentalSample {
    elevation: i64,
    regional: i64,
}

fn continental_elevation(seed: u64, x: i64, y: i64) -> ContinentalSample {
    let continentalness = continentalness(seed, x, y);
    let broad_relief = value_noise(seed ^ BROAD_RELIEF_SEED, x, y, 768);
    let regional = value_noise(seed ^ REGIONAL_SEED, x, y, 192);
    let relief = (continentalness * 15 + broad_relief * 4 + regional) / 20;
    let elevation = if continentalness <= i64::from(SAND_MAX) {
        continentalness
    } else {
        relief.max(i64::from(SAND_MAX) + 1)
    };
    ContinentalSample {
        elevation,
        regional,
    }
}

fn continentalness(seed: u64, x: i64, y: i64) -> i64 {
    value_noise(seed ^ CONTINENT_SEED, x, y, 2_048)
}

fn inland_lake_elevation(
    x: i64,
    y: i64,
    sample: ContinentalSample,
    lake: Option<LakeDescriptor>,
) -> Option<i64> {
    let descriptor = lake?;
    let dx = x - descriptor.center_x;
    let dy = y - descriptor.center_y;
    let sheared_x = dx + dy * descriptor.shear / 64;
    let shore_variation = ((sample.regional - NOISE_MAX / 2) / INLAND_LAKE_SHORE_VARIATION_DIVISOR)
        .clamp(
            -INLAND_LAKE_MAX_SHORE_VARIATION,
            INLAND_LAKE_MAX_SHORE_VARIATION,
        );
    let radius_x = descriptor.radius_x + shore_variation;
    let radius_y = descriptor.radius_y + shore_variation;

    let lake_elevation = if inside_ellipse(
        sheared_x,
        dy,
        radius_x - INLAND_LAKE_DEEP_INSET,
        radius_y - INLAND_LAKE_DEEP_INSET,
    ) {
        INLAND_LAKE_FLOOR
    } else if inside_ellipse(
        sheared_x,
        dy,
        radius_x - INLAND_LAKE_SHORE_INSET,
        radius_y - INLAND_LAKE_SHORE_INSET,
    ) {
        INLAND_LAKE_SHALLOW
    } else if inside_ellipse(sheared_x, dy, radius_x, radius_y) {
        INLAND_LAKE_SHORE
    } else {
        return None;
    };
    Some(sample.elevation.min(lake_elevation))
}

#[derive(Clone, Copy)]
struct LakeDescriptor {
    center_x: i64,
    center_y: i64,
    radius_x: i64,
    radius_y: i64,
    shear: i64,
}

fn lake_descriptor(seed: u64, x: i64, y: i64) -> Option<LakeDescriptor> {
    let region_x = x.div_euclid(INLAND_LAKE_REGION_SIZE);
    let region_y = y.div_euclid(INLAND_LAKE_REGION_SIZE);
    let bits = hash(seed ^ INLAND_LAKE_SEED, region_x, region_y);
    if bits & 3 != 1 {
        return None;
    }

    let origin_x = region_x * INLAND_LAKE_REGION_SIZE;
    let origin_y = region_y * INLAND_LAKE_REGION_SIZE;
    let center_x = origin_x
        + INLAND_LAKE_CENTER_MARGIN
        + ((bits >> 3) & 1_023) as i64 % INLAND_LAKE_CENTER_SPAN;
    let center_y = origin_y
        + INLAND_LAKE_CENTER_MARGIN
        + ((bits >> 13) & 1_023) as i64 % INLAND_LAKE_CENTER_SPAN;
    let center_continentalness = value_noise(seed ^ CONTINENT_SEED, center_x, center_y, 2_048);
    if center_continentalness < INLAND_LAKE_MIN_CONTINENTALNESS {
        return None;
    }

    Some(LakeDescriptor {
        center_x,
        center_y,
        radius_x: INLAND_LAKE_MIN_RADIUS_X + ((bits >> 23) & 127) as i64 % INLAND_LAKE_RADIUS_SPAN,
        radius_y: INLAND_LAKE_MIN_RADIUS_Y + ((bits >> 30) & 127) as i64 % INLAND_LAKE_RADIUS_SPAN,
        shear: ((bits >> 37) & 63) as i64 - 32,
    })
}

fn inside_ellipse(x: i64, y: i64, radius_x: i64, radius_y: i64) -> bool {
    let x = i128::from(x);
    let y = i128::from(y);
    let radius_x = i128::from(radius_x);
    let radius_y = i128::from(radius_y);
    x * x * radius_y * radius_y + y * y * radius_x * radius_x
        <= radius_x * radius_x * radius_y * radius_y
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RiverDescriptor {
    points: [WorldPosition; RIVER_MAX_POINTS],
    len: u8,
}

impl RiverDescriptor {
    fn points(&self) -> &[WorldPosition] {
        &self.points[..usize::from(self.len)]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RiverPath {
    points: [WorldPosition; RIVER_MAX_COARSE_POINTS],
    len: u8,
    turns: u8,
    source_elevation: u16,
}

impl RiverPath {
    fn points(&self) -> &[WorldPosition] {
        &self.points[..usize::from(self.len)]
    }
}

const RIVER_DIRECTIONS: [(isize, isize); 8] = [
    (-1, -1),
    (0, -1),
    (1, -1),
    (-1, 0),
    (1, 0),
    (-1, 1),
    (0, 1),
    (1, 1),
];

fn river_descriptor(seed: u64, x: i64, y: i64) -> Option<RiverDescriptor> {
    river_path(seed, x, y).map(|path| smooth_river_path(seed, path))
}

fn river_path(seed: u64, x: i64, y: i64) -> Option<RiverPath> {
    let region_x = x.div_euclid(RIVER_REGION_SIZE);
    let region_y = y.div_euclid(RIVER_REGION_SIZE);
    let origin = WorldPosition {
        x: region_x * RIVER_REGION_SIZE,
        y: region_y * RIVER_REGION_SIZE,
    };
    let mut elevations = [0_u16; RIVER_NODE_SIDE * RIVER_NODE_SIDE];
    for (index, elevation) in elevations.iter_mut().enumerate() {
        let position = river_node_position(origin, index);
        *elevation = continental_elevation(seed, position.x, position.y).elevation as u16;
    }

    let mut attempted = [false; RIVER_NODE_SIDE * RIVER_NODE_SIDE];
    let mut candidates = [None; RIVER_OUTLET_ATTEMPTS];
    let mut candidate_len = 0;
    for _ in 0..RIVER_OUTLET_ATTEMPTS {
        let Some(outlet) = (0..elevations.len())
            .filter(|&index| !attempted[index] && elevations[index] > SAND_MAX)
            .filter(|&index| river_water_neighbor(index, &elevations).is_some())
            .min_by_key(|&index| {
                let position = river_node_position(origin, index);
                hash(seed ^ RIVER_OUTLET_SEED, position.x, position.y)
            })
        else {
            break;
        };
        attempted[outlet] = true;
        let Some(path) = grow_river_path(seed, origin, outlet, &elevations) else {
            continue;
        };
        let insertion = (0..candidate_len)
            .find(|&index| river_path_is_better(path, candidates[index].unwrap(), seed))
            .unwrap_or(candidate_len);
        for index in (insertion..candidate_len).rev() {
            candidates[index + 1] = candidates[index];
        }
        candidates[insertion] = Some(path);
        candidate_len += 1;
    }
    candidates[..candidate_len]
        .iter()
        .flatten()
        .copied()
        .find(|&path| river_corridor_is_clear(smooth_river_path(seed, path)))
}

fn grow_river_path(
    seed: u64,
    origin: WorldPosition,
    outlet: usize,
    elevations: &[u16; RIVER_NODE_SIDE * RIVER_NODE_SIDE],
) -> Option<RiverPath> {
    river_water_neighbor(outlet, elevations)?;
    let mut land_nodes = [outlet; RIVER_MAX_LAND_NODES];
    let mut land_len = 1;
    while land_len < RIVER_MAX_LAND_NODES {
        let current = land_nodes[land_len - 1];
        let current_elevation = elevations[current];
        let next = river_neighbors(current)
            .filter(|&index| elevations[index] > current_elevation)
            .filter(|&index| {
                land_len < 2
                    || river_direction_dot(
                        river_direction(land_nodes[land_len - 2], current),
                        river_direction(current, index),
                    ) >= 0
            })
            .filter(|&index| !river_node_returns_near_path(index, &land_nodes[..land_len - 1]))
            .filter(|&index| !river_edge_crosses_path(current, index, &land_nodes[..land_len]))
            .min_by_key(|&index| {
                let position = river_node_position(origin, index);
                let rise = u32::from(elevations[index] - current_elevation);
                let turn_penalty = (land_len > 1
                    && river_direction(land_nodes[land_len - 2], current)
                        != river_direction(current, index))
                    as u32
                    * RIVER_TURN_PENALTY;
                let dead_end = !river_neighbors(index).any(|next| {
                    elevations[next] > elevations[index]
                        && !river_node_returns_near_path(next, &land_nodes[..land_len])
                });
                (
                    dead_end,
                    rise + turn_penalty,
                    hash(seed ^ RIVER_ROUTE_SEED, position.x, position.y),
                )
            });
        let Some(next) = next else {
            break;
        };
        land_nodes[land_len] = next;
        land_len += 1;
    }
    if land_len < RIVER_MIN_LAND_NODES
        || elevations[land_nodes[land_len - 1]] < RIVER_SOURCE_MIN_ELEVATION
    {
        return None;
    }

    let mouth = river_node_position(
        origin,
        river_aligned_water_neighbor(outlet, land_nodes[1], elevations)?,
    );
    let source = river_node_position(origin, land_nodes[land_len - 1]);
    let mut points = [source; RIVER_MAX_COARSE_POINTS];
    for (point, node) in points
        .iter_mut()
        .zip(land_nodes[..land_len].iter().rev().copied())
    {
        *point = river_node_position(origin, node);
    }
    points[land_len] = mouth;
    let point_len = land_len + 1;
    let turns = points[..point_len]
        .windows(3)
        .filter(|points| river_heading(points[0], points[1]) != river_heading(points[1], points[2]))
        .count() as u8;
    Some(RiverPath {
        points,
        len: point_len as u8,
        turns,
        source_elevation: elevations[land_nodes[land_len - 1]],
    })
}

fn river_path_is_better(candidate: RiverPath, current: RiverPath, seed: u64) -> bool {
    let candidate_source = candidate.points()[0];
    let current_source = current.points()[0];
    (
        candidate.len,
        std::cmp::Reverse(candidate.turns),
        candidate.source_elevation,
        std::cmp::Reverse(hash(
            seed ^ RIVER_OUTLET_SEED,
            candidate_source.x,
            candidate_source.y,
        )),
    ) > (
        current.len,
        std::cmp::Reverse(current.turns),
        current.source_elevation,
        std::cmp::Reverse(hash(
            seed ^ RIVER_OUTLET_SEED,
            current_source.x,
            current_source.y,
        )),
    )
}

fn river_node_returns_near_path(index: usize, prior: &[usize]) -> bool {
    let x = (index % RIVER_NODE_SIDE) as isize;
    let y = (index / RIVER_NODE_SIDE) as isize;
    prior.iter().any(|&node| {
        let prior_x = (node % RIVER_NODE_SIDE) as isize;
        let prior_y = (node / RIVER_NODE_SIDE) as isize;
        (x - prior_x).abs() + (y - prior_y).abs() <= 1
    })
}

fn river_edge_crosses_path(start: usize, end: usize, path: &[usize]) -> bool {
    fn cross(start: (isize, isize), end: (isize, isize), point: (isize, isize)) -> isize {
        (end.0 - start.0) * (point.1 - start.1) - (end.1 - start.1) * (point.0 - start.0)
    }

    let position = |index: usize| {
        (
            (index % RIVER_NODE_SIDE) as isize,
            (index / RIVER_NODE_SIDE) as isize,
        )
    };
    let start = position(start);
    let end = position(end);
    path.windows(2).any(|edge| {
        let prior_start = position(edge[0]);
        let prior_end = position(edge[1]);
        let left_a = cross(start, end, prior_start);
        let left_b = cross(start, end, prior_end);
        let right_a = cross(prior_start, prior_end, start);
        let right_b = cross(prior_start, prior_end, end);
        left_a.signum() != left_b.signum()
            && right_a.signum() != right_b.signum()
            && left_a != 0
            && left_b != 0
            && right_a != 0
            && right_b != 0
    })
}

fn river_direction(start: usize, end: usize) -> (isize, isize) {
    let start_x = (start % RIVER_NODE_SIDE) as isize;
    let start_y = (start / RIVER_NODE_SIDE) as isize;
    let end_x = (end % RIVER_NODE_SIDE) as isize;
    let end_y = (end / RIVER_NODE_SIDE) as isize;
    (end_x - start_x, end_y - start_y)
}

fn river_direction_dot(left: (isize, isize), right: (isize, isize)) -> isize {
    left.0 * right.0 + left.1 * right.1
}

fn river_heading(start: WorldPosition, end: WorldPosition) -> (i64, i64) {
    ((end.x - start.x).signum(), (end.y - start.y).signum())
}

fn smooth_river_path(seed: u64, path: RiverPath) -> RiverDescriptor {
    let source = path.points()[0];
    let mut current = [source; RIVER_MAX_POINTS];
    let mut current_len = 1;
    for points in path.points().windows(2) {
        current[current_len] = river_valley_midpoint(seed, points[0], points[1]);
        current[current_len + 1] = points[1];
        current_len += 2;
    }
    for _ in 0..RIVER_SMOOTHING_PASSES {
        let mut smoothed = [source; RIVER_MAX_POINTS];
        let mut smoothed_len = 1;
        for points in current[..current_len].windows(2) {
            smoothed[smoothed_len] = weighted_river_point(points[0], points[1], 3, 1);
            smoothed[smoothed_len + 1] = weighted_river_point(points[0], points[1], 1, 3);
            smoothed_len += 2;
        }
        smoothed[smoothed_len] = current[current_len - 1];
        smoothed_len += 1;
        current = smoothed;
        current_len = smoothed_len;
    }
    RiverDescriptor {
        points: current,
        len: current_len as u8,
    }
}

fn river_valley_midpoint(seed: u64, start: WorldPosition, end: WorldPosition) -> WorldPosition {
    let midpoint = weighted_river_point(start, end, 1, 1);
    let dx = end.x - start.x;
    let dy = end.y - start.y;
    let scale = dx.abs().max(dy.abs()).max(1);
    let normal_x = -dy / scale;
    let normal_y = dx / scale;
    let start_elevation = continental_elevation(seed, start.x, start.y).elevation;
    let end_elevation = continental_elevation(seed, end.x, end.y).elevation;
    RIVER_REFINEMENT_OFFSETS
        .into_iter()
        .map(|offset| WorldPosition {
            x: midpoint.x + normal_x * offset,
            y: midpoint.y + normal_y * offset,
        })
        .min_by_key(|point| {
            let elevation = continental_elevation(seed, point.x, point.y).elevation;
            let descent_violation =
                (elevation - start_elevation).max(0) + (end_elevation - elevation).max(0);
            (
                descent_violation,
                elevation,
                hash(seed ^ RIVER_ROUTE_SEED, point.x, point.y),
            )
        })
        .unwrap_or(midpoint)
}

fn river_corridor_is_clear(river: RiverDescriptor) -> bool {
    let points = river.points();
    let segment_count = points.len() - 1;
    let mut segments = [EMPTY_RIVER_SEGMENT; RIVER_MAX_POINTS];
    segments[0] = RiverSegment {
        start: points[0],
        end: points[0],
        half_width: RIVER_HEADWATER_HALF_WIDTH,
    };
    for (index, points) in points.windows(2).enumerate() {
        segments[index + 1] = RiverSegment {
            start: points[0],
            end: points[1],
            half_width: river_half_width(index, segment_count),
        };
    }
    let segment_len = segment_count + 1;
    for left in 0..segment_len {
        for right in left + 4..segment_len {
            let clearance = segments[left].half_width
                + river_bank_width(segments[left].half_width)
                + segments[right].half_width
                + river_bank_width(segments[right].half_width);
            if river_segments_within_clearance(segments[left], segments[right], clearance) {
                return false;
            }
        }
    }
    true
}

fn river_segments_within_clearance(
    left: RiverSegment,
    right: RiverSegment,
    clearance: i64,
) -> bool {
    if river_segments_intersect(left.start, left.end, right.start, right.end) {
        return true;
    }
    [
        point_segment_distance_ratio(left.start, right.start, right.end),
        point_segment_distance_ratio(left.end, right.start, right.end),
        point_segment_distance_ratio(right.start, left.start, left.end),
        point_segment_distance_ratio(right.end, left.start, left.end),
    ]
    .into_iter()
    .any(|(distance, denominator)| distance <= i128::from(clearance).pow(2) * denominator)
}

fn river_segments_intersect(
    left_start: WorldPosition,
    left_end: WorldPosition,
    right_start: WorldPosition,
    right_end: WorldPosition,
) -> bool {
    fn cross(start: WorldPosition, end: WorldPosition, point: WorldPosition) -> i128 {
        (i128::from(end.x) - i128::from(start.x)) * (i128::from(point.y) - i128::from(start.y))
            - (i128::from(end.y) - i128::from(start.y))
                * (i128::from(point.x) - i128::from(start.x))
    }

    let left_a = cross(left_start, left_end, right_start);
    let left_b = cross(left_start, left_end, right_end);
    let right_a = cross(right_start, right_end, left_start);
    let right_b = cross(right_start, right_end, left_end);
    if left_a == 0 || left_b == 0 || right_a == 0 || right_b == 0 {
        let overlaps =
            |start: i64, end: i64, point: i64| point >= start.min(end) && point <= start.max(end);
        return (left_a == 0
            && overlaps(left_start.x, left_end.x, right_start.x)
            && overlaps(left_start.y, left_end.y, right_start.y))
            || (left_b == 0
                && overlaps(left_start.x, left_end.x, right_end.x)
                && overlaps(left_start.y, left_end.y, right_end.y))
            || (right_a == 0
                && overlaps(right_start.x, right_end.x, left_start.x)
                && overlaps(right_start.y, right_end.y, left_start.y))
            || (right_b == 0
                && overlaps(right_start.x, right_end.x, left_end.x)
                && overlaps(right_start.y, right_end.y, left_end.y));
    }
    (left_a.is_negative() != left_b.is_negative())
        && (right_a.is_negative() != right_b.is_negative())
}

fn weighted_river_point(
    start: WorldPosition,
    end: WorldPosition,
    start_weight: i128,
    end_weight: i128,
) -> WorldPosition {
    let interpolate = |start: i64, end: i64| {
        ((i128::from(start) * start_weight + i128::from(end) * end_weight)
            .div_euclid(start_weight + end_weight)) as i64
    };
    WorldPosition {
        x: interpolate(start.x, end.x),
        y: interpolate(start.y, end.y),
    }
}

fn river_node_position(origin: WorldPosition, index: usize) -> WorldPosition {
    WorldPosition {
        x: origin.x + RIVER_NODE_OFFSET + (index % RIVER_NODE_SIDE) as i64 * RIVER_NODE_STEP,
        y: origin.y + RIVER_NODE_OFFSET + (index / RIVER_NODE_SIDE) as i64 * RIVER_NODE_STEP,
    }
}

fn river_node_index(x: isize, y: isize) -> Option<usize> {
    (x >= 0 && y >= 0 && x < RIVER_NODE_SIDE as isize && y < RIVER_NODE_SIDE as isize)
        .then(|| y as usize * RIVER_NODE_SIDE + x as usize)
}

fn river_neighbors(index: usize) -> impl Iterator<Item = usize> {
    let x = (index % RIVER_NODE_SIDE) as isize;
    let y = (index / RIVER_NODE_SIDE) as isize;
    RIVER_DIRECTIONS
        .into_iter()
        .filter_map(move |(dx, dy)| river_node_index(x + dx, y + dy))
}

fn river_water_neighbor(
    index: usize,
    elevations: &[u16; RIVER_NODE_SIDE * RIVER_NODE_SIDE],
) -> Option<usize> {
    river_neighbors(index)
        .filter(|&neighbor| elevations[neighbor] <= SHALLOW_WATER_MAX)
        .max_by_key(|&neighbor| elevations[neighbor])
}

fn river_aligned_water_neighbor(
    outlet: usize,
    upstream: usize,
    elevations: &[u16; RIVER_NODE_SIDE * RIVER_NODE_SIDE],
) -> Option<usize> {
    let downstream = river_direction(upstream, outlet);
    river_neighbors(outlet)
        .filter(|&neighbor| elevations[neighbor] <= SHALLOW_WATER_MAX)
        .filter(|&neighbor| river_direction_dot(downstream, river_direction(outlet, neighbor)) >= 0)
        .max_by_key(|&neighbor| {
            (
                river_direction_dot(downstream, river_direction(outlet, neighbor)),
                elevations[neighbor],
            )
        })
}

#[derive(Clone, Copy)]
struct RiverSegment {
    start: WorldPosition,
    end: WorldPosition,
    half_width: i64,
}

const EMPTY_RIVER_SEGMENT: RiverSegment = RiverSegment {
    start: WorldPosition { x: 0, y: 0 },
    end: WorldPosition { x: 0, y: 0 },
    half_width: 0,
};

#[derive(Clone, Copy)]
struct ChunkRivers {
    segments: [RiverSegment; RIVER_MAX_POINTS],
    len: u8,
}

impl ChunkRivers {
    fn empty() -> Self {
        Self {
            segments: [EMPTY_RIVER_SEGMENT; RIVER_MAX_POINTS],
            len: 0,
        }
    }

    fn segments(&self) -> &[RiverSegment] {
        &self.segments[..usize::from(self.len)]
    }
}

fn river_for_chunk(seed: u64, origin: WorldPosition) -> ChunkRivers {
    let Some(river) = river_descriptor(seed, origin.x, origin.y) else {
        return ChunkRivers::empty();
    };
    let points = river.points();
    let segment_count = points.len() - 1;
    let mut chunk_rivers = ChunkRivers::empty();
    let headwater = RiverSegment {
        start: points[0],
        end: points[0],
        half_width: RIVER_HEADWATER_HALF_WIDTH,
    };
    if river_segment_intersects_chunk(headwater, origin) {
        chunk_rivers.segments[usize::from(chunk_rivers.len)] = headwater;
        chunk_rivers.len += 1;
    }
    for (index, points) in points.windows(2).enumerate() {
        let half_width = river_half_width(index, segment_count);
        let segment = RiverSegment {
            start: points[0],
            end: points[1],
            half_width,
        };
        if river_segment_intersects_chunk(segment, origin) {
            chunk_rivers.segments[usize::from(chunk_rivers.len)] = segment;
            chunk_rivers.len += 1;
        }
    }
    chunk_rivers
}

fn river_half_width(index: usize, segment_count: usize) -> i64 {
    RIVER_SOURCE_HALF_WIDTH
        + (RIVER_MOUTH_HALF_WIDTH - RIVER_SOURCE_HALF_WIDTH) * index as i64
            / (segment_count - 1).max(1) as i64
}

fn river_bank_width(half_width: i64) -> i64 {
    half_width / 8
}

fn river_segment_intersects_chunk(segment: RiverSegment, origin: WorldPosition) -> bool {
    let margin = segment.half_width + river_bank_width(segment.half_width);
    let min_x = segment.start.x.min(segment.end.x) - margin;
    let min_y = segment.start.y.min(segment.end.y) - margin;
    let max_x = segment.start.x.max(segment.end.x) + margin;
    let max_y = segment.start.y.max(segment.end.y) + margin;
    max_x >= origin.x
        && max_y >= origin.y
        && min_x < origin.x + CHUNK_SIZE
        && min_y < origin.y + CHUNK_SIZE
}

fn river_elevation(x: i64, y: i64, rivers: &ChunkRivers) -> Option<i64> {
    rivers
        .segments()
        .iter()
        .filter_map(|segment| river_segment_elevation(x, y, *segment))
        .min()
}

fn river_segment_elevation(x: i64, y: i64, segment: RiverSegment) -> Option<i64> {
    let (distance, denominator) =
        point_segment_distance_ratio(WorldPosition { x, y }, segment.start, segment.end);
    let deep_half_width = (segment.half_width - RIVER_HEADWATER_HALF_WIDTH).max(0) / 2;
    let bank_width = segment.half_width + river_bank_width(segment.half_width);
    if deep_half_width > 0 && distance <= i128::from(deep_half_width).pow(2) * denominator {
        Some(RIVER_DEEP_ELEVATION)
    } else if distance <= i128::from(segment.half_width).pow(2) * denominator {
        Some(RIVER_SHALLOW_ELEVATION)
    } else if distance <= i128::from(bank_width).pow(2) * denominator {
        Some(RIVER_BANK_ELEVATION)
    } else {
        None
    }
}

#[cfg(test)]
fn river_route_elevation(x: i64, y: i64, river: RiverDescriptor) -> Option<i64> {
    let points = river.points();
    let segment_count = points.len() - 1;
    std::iter::once(RiverSegment {
        start: points[0],
        end: points[0],
        half_width: RIVER_HEADWATER_HALF_WIDTH,
    })
    .chain(
        points
            .windows(2)
            .enumerate()
            .map(|(index, points)| RiverSegment {
                start: points[0],
                end: points[1],
                half_width: river_half_width(index, segment_count),
            }),
    )
    .filter_map(|segment| river_segment_elevation(x, y, segment))
    .min()
}

fn point_segment_distance_ratio(
    point: WorldPosition,
    start: WorldPosition,
    end: WorldPosition,
) -> (i128, i128) {
    let dx = i128::from(end.x) - i128::from(start.x);
    let dy = i128::from(end.y) - i128::from(start.y);
    let px = i128::from(point.x) - i128::from(start.x);
    let py = i128::from(point.y) - i128::from(start.y);
    let length_squared = dx * dx + dy * dy;
    let projection = px * dx + py * dy;
    if projection <= 0 {
        (px * px + py * py, 1)
    } else if projection >= length_squared {
        let end_x = i128::from(point.x) - i128::from(end.x);
        let end_y = i128::from(point.y) - i128::from(end.y);
        (end_x * end_x + end_y * end_y, 1)
    } else {
        let cross = px * dy - py * dx;
        (cross * cross, length_squared)
    }
}

fn moisture_noise(seed: u64, x: i64, y: i64) -> u16 {
    let seed = seed ^ 0x9e37_79b9_7f4a_7c15;
    let broad = value_noise(seed, x, y, 384);
    let regional = value_noise(seed ^ 0xa24b_aed4, x, y, 128);
    let local = value_noise(seed ^ 0x9fb2_1c65, x, y, 40);
    ((broad * 6 + regional * 3 + local) / 10) as u16
}

fn value_noise(seed: u64, x: i64, y: i64, scale: i64) -> i64 {
    let x0 = x.div_euclid(scale);
    let y0 = y.div_euclid(scale);
    let tx = x.rem_euclid(scale) * NOISE_MAX / scale;
    let ty = y.rem_euclid(scale) * NOISE_MAX / scale;
    let sx = smooth(tx);
    let sy = smooth(ty);
    let top = lerp(lattice(seed, x0, y0), lattice(seed, x0 + 1, y0), sx);
    let bottom = lerp(lattice(seed, x0, y0 + 1), lattice(seed, x0 + 1, y0 + 1), sx);
    lerp(top, bottom, sy)
}

fn smooth(value: i64) -> i64 {
    let squared = value * value / NOISE_MAX;
    squared * (3 * NOISE_MAX - 2 * value) / NOISE_MAX
}

fn lerp(start: i64, end: i64, amount: i64) -> i64 {
    start + (end - start) * amount / NOISE_MAX
}

fn lattice(seed: u64, x: i64, y: i64) -> i64 {
    (hash(seed, x, y) & NOISE_MAX as u64) as i64
}

fn hash(seed: u64, x: i64, y: i64) -> u64 {
    let mut value = seed
        ^ (x as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15)
        ^ (y as u64).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn component_sizes(cells: &[bool], side: usize) -> Vec<usize> {
        let mut visited = vec![false; cells.len()];
        let mut components = Vec::new();
        for start in 0..cells.len() {
            if !cells[start] || visited[start] {
                continue;
            }
            visited[start] = true;
            let mut stack = vec![start];
            let mut component_size = 0;
            while let Some(index) = stack.pop() {
                component_size += 1;
                let x = index % side;
                let y = index / side;
                let neighbors = [
                    x.checked_sub(1).map(|next| y * side + next),
                    (x + 1 < side).then_some(y * side + x + 1),
                    y.checked_sub(1).map(|next| next * side + x),
                    (y + 1 < side).then_some((y + 1) * side + x),
                ];
                for neighbor in neighbors.into_iter().flatten() {
                    if cells[neighbor] && !visited[neighbor] {
                        visited[neighbor] = true;
                        stack.push(neighbor);
                    }
                }
            }
            components.push(component_size);
        }
        components.sort_unstable_by(|left, right| right.cmp(left));
        components
    }

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
        assert_ne!(left.terrain(), right.terrain());
    }

    #[test]
    fn terrain_has_variation_and_sparse_features() {
        let world = World::generate_square(7, 256);
        let first = world.terrain()[0].ground;
        assert!(world.terrain().iter().any(|cell| cell.ground != first));
        assert!(!world.features().is_empty());
        assert!(world.features().len() < world.terrain().len() / 4);
        assert!(world.features().iter().all(|feature| matches!(
            world.cell(feature.position).unwrap().ground,
            GroundType::Grass | GroundType::ForestFloor | GroundType::Hill | GroundType::BareRock
        )));
    }

    #[test]
    fn terrain_records_keep_their_compact_layout() {
        assert_eq!(std::mem::size_of::<GroundType>(), 1);
        assert_eq!(std::mem::size_of::<TerrainCell>(), 4);
    }

    #[test]
    fn initial_area_matches_independently_generated_chunks() {
        let seed = 19;
        let world = World::generate(seed, WorldConfig::new(128, 128).unwrap());
        for coord in [
            ChunkCoord { x: 0, y: 0 },
            ChunkCoord { x: 1, y: 0 },
            ChunkCoord { x: 0, y: 1 },
            ChunkCoord { x: 1, y: 1 },
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
                .features()
                .iter()
                .filter(|feature| chunk_coord(feature.position) == coord)
                .copied()
                .collect();
            assert_eq!(chunk.features(), expected_features);
        }
    }

    #[test]
    fn continental_ocean_and_inland_lakes_have_distinct_scales() {
        const SAMPLE_EDGE: i64 = 4_096;
        const SAMPLE_STEP: i64 = 32;
        const SAMPLE_SIDE: usize = (SAMPLE_EDGE / SAMPLE_STEP) as usize;

        let mut continental_water_cells = vec![false; SAMPLE_SIDE * SAMPLE_SIDE];
        let mut water_cells = vec![false; SAMPLE_SIDE * SAMPLE_SIDE];
        let mut lake_water_cells = vec![false; SAMPLE_SIDE * SAMPLE_SIDE];
        let mut continental_water = 0;
        let mut lake_water = 0;
        for sample_y in 0..SAMPLE_SIDE {
            for sample_x in 0..SAMPLE_SIDE {
                let x = sample_x as i64 * SAMPLE_STEP;
                let y = sample_y as i64 * SAMPLE_STEP;
                let sample = continental_elevation(1, x, y);
                let lake_elevation = inland_lake_elevation(x, y, sample, lake_descriptor(1, x, y));
                let final_elevation = lake_elevation.unwrap_or(sample.elevation) as u16;
                let is_water = final_elevation <= SHALLOW_WATER_MAX;
                let index = sample_y * SAMPLE_SIDE + sample_x;
                continental_water_cells[index] = sample.elevation as u16 <= SHALLOW_WATER_MAX;
                water_cells[index] = is_water;
                lake_water_cells[index] = is_water && lake_elevation.is_some();
                if is_water {
                    if lake_elevation.is_some() {
                        lake_water += 1;
                    } else {
                        continental_water += 1;
                    }
                }
            }
        }

        let water_count = continental_water + lake_water;
        let sample_count = water_cells.len();
        assert!(continental_water > sample_count / 5);
        assert!(lake_water > sample_count / 500 && lake_water < sample_count / 100);
        assert!(sample_count - water_count > sample_count / 3);

        let continental_water_components = component_sizes(&continental_water_cells, SAMPLE_SIDE);
        let water_components = component_sizes(&water_cells, SAMPLE_SIDE);
        let lake_components = component_sizes(&lake_water_cells, SAMPLE_SIDE);
        assert_eq!(continental_water_components.len(), 1);
        assert!((0..SAMPLE_SIDE).any(|offset| {
            continental_water_cells[offset]
                || continental_water_cells[(SAMPLE_SIDE - 1) * SAMPLE_SIDE + offset]
                || continental_water_cells[offset * SAMPLE_SIDE]
                || continental_water_cells[offset * SAMPLE_SIDE + SAMPLE_SIDE - 1]
        }));
        assert!((2..=5).contains(&water_components.len()));
        assert!(water_components[0] * 3 > water_count);
        assert!((1..=4).contains(&lake_components.len()));
        assert!(*lake_components.last().unwrap() >= 8);
    }

    #[test]
    fn lake_descriptors_guarantee_a_bounded_water_core() {
        let mut descriptor_count = 0;
        for seed in 0..32 {
            for region_y in -4..=4 {
                for region_x in -4..=4 {
                    let probe_x = region_x * INLAND_LAKE_REGION_SIZE;
                    let probe_y = region_y * INLAND_LAKE_REGION_SIZE;
                    let Some(descriptor) = lake_descriptor(seed, probe_x, probe_y) else {
                        continue;
                    };
                    descriptor_count += 1;
                    let origin_x = region_x * INLAND_LAKE_REGION_SIZE;
                    let origin_y = region_y * INLAND_LAKE_REGION_SIZE;
                    assert!(
                        descriptor.center_x - descriptor.radius_x - INLAND_LAKE_MAX_SHORE_VARIATION
                            >= origin_x
                            && descriptor.center_y
                                - descriptor.radius_y
                                - INLAND_LAKE_MAX_SHORE_VARIATION
                                >= origin_y
                            && descriptor.center_x
                                + descriptor.radius_x
                                + INLAND_LAKE_MAX_SHORE_VARIATION
                                < origin_x + INLAND_LAKE_REGION_SIZE
                            && descriptor.center_y
                                + descriptor.radius_y
                                + INLAND_LAKE_MAX_SHORE_VARIATION
                                < origin_y + INLAND_LAKE_REGION_SIZE
                    );

                    for offset_y in -16..=16 {
                        for offset_x in -16..=16 {
                            let x = descriptor.center_x + offset_x;
                            let y = descriptor.center_y + offset_y;
                            let sample = continental_elevation(seed, x, y);
                            assert_eq!(
                                inland_lake_elevation(x, y, sample, Some(descriptor)),
                                Some(INLAND_LAKE_FLOOR)
                            );
                        }
                    }
                }
            }
        }
        assert!(descriptor_count > 100);
    }

    #[test]
    fn river_routes_follow_relief_without_self_intersections() {
        let mut river_count = 0;
        let mut configured_seed_rivers = 0;
        for seed in 0..32 {
            for region_y in -4..=4 {
                for region_x in -4..=4 {
                    let origin = WorldPosition {
                        x: region_x * RIVER_REGION_SIZE,
                        y: region_y * RIVER_REGION_SIZE,
                    };
                    let Some(path) = river_path(seed, origin.x, origin.y) else {
                        continue;
                    };
                    let river = smooth_river_path(seed, path);
                    assert!(river_corridor_is_clear(river));
                    river_count += 1;
                    if seed == 1 && (0..2).contains(&region_x) && (0..2).contains(&region_y) {
                        configured_seed_rivers += 1;
                    }
                    assert_eq!(
                        river_descriptor(
                            seed,
                            origin.x + RIVER_REGION_SIZE - 1,
                            origin.y + RIVER_REGION_SIZE - 1,
                        ),
                        Some(river)
                    );
                    assert_eq!(
                        river.points().len(),
                        (path.points().len() * 2 - 1) << RIVER_SMOOTHING_PASSES
                    );
                    assert!(river.points().len() <= RIVER_MAX_POINTS);
                    let route_margin =
                        RIVER_MOUTH_HALF_WIDTH + river_bank_width(RIVER_MOUTH_HALF_WIDTH);
                    assert!(river.points().iter().all(|point| {
                        point.x - route_margin >= origin.x
                            && point.y - route_margin >= origin.y
                            && point.x + route_margin < origin.x + RIVER_REGION_SIZE
                            && point.y + route_margin < origin.y + RIVER_REGION_SIZE
                    }));

                    let coarse = path.points();
                    let mouth = *coarse.last().unwrap();
                    assert!(
                        continental_elevation(seed, mouth.x, mouth.y).elevation
                            <= i64::from(SHALLOW_WATER_MAX)
                    );
                    assert!(
                        continental_elevation(seed, coarse[0].x, coarse[0].y).elevation
                            >= i64::from(RIVER_SOURCE_MIN_ELEVATION)
                    );
                    for pair in coarse.windows(2) {
                        let start_elevation =
                            continental_elevation(seed, pair[0].x, pair[0].y).elevation;
                        let end_elevation =
                            continental_elevation(seed, pair[1].x, pair[1].y).elevation;
                        assert!(start_elevation > end_elevation);
                    }
                    assert!(coarse[..coarse.len() - 1].iter().all(|point| {
                        continental_elevation(seed, point.x, point.y).elevation
                            > i64::from(SAND_MAX)
                    }));
                    for left in 0..river.points().len() - 1 {
                        for right in left + 2..river.points().len() - 1 {
                            assert!(!river_segments_intersect(
                                river.points()[left],
                                river.points()[left + 1],
                                river.points()[right],
                                river.points()[right + 1],
                            ));
                        }
                    }
                    assert_eq!(
                        river_route_elevation(coarse[0].x, coarse[0].y, river),
                        Some(RIVER_SHALLOW_ELEVATION)
                    );
                    assert_eq!(
                        river_route_elevation(mouth.x, mouth.y, river),
                        Some(RIVER_DEEP_ELEVATION)
                    );
                    let incoming =
                        river_heading(coarse[coarse.len() - 3], coarse[coarse.len() - 2]);
                    let outgoing = river_heading(coarse[coarse.len() - 2], mouth);
                    assert!(incoming.0 * outgoing.0 + incoming.1 * outgoing.1 >= 0);
                }
            }
        }
        assert!(river_count > 20);
        assert_eq!(configured_seed_rivers, 1);
    }

    #[test]
    fn rivers_cross_negative_chunk_seams_without_breaks() {
        let mut crossing = None;
        'search: for seed in 0..32 {
            for region_y in -4..=4 {
                for region_x in -4..=-1 {
                    let origin = WorldPosition {
                        x: region_x * RIVER_REGION_SIZE,
                        y: region_y * RIVER_REGION_SIZE,
                    };
                    let Some(river) = river_descriptor(seed, origin.x, origin.y) else {
                        continue;
                    };
                    for boundary_offset in
                        (CHUNK_SIZE..RIVER_REGION_SIZE).step_by(CHUNK_SIZE as usize)
                    {
                        let boundary_x = origin.x + boundary_offset;
                        for y in origin.y..origin.y + RIVER_REGION_SIZE {
                            let left = WorldPosition {
                                x: boundary_x - 1,
                                y,
                            };
                            let right = WorldPosition { x: boundary_x, y };
                            let both_land = [left, right].into_iter().all(|point| {
                                continental_elevation(seed, point.x, point.y).elevation
                                    > i64::from(SAND_MAX)
                            });
                            let both_river = [left, right].into_iter().all(|point| {
                                river_route_elevation(point.x, point.y, river)
                                    .is_some_and(|elevation| elevation <= RIVER_SHALLOW_ELEVATION)
                            });
                            if both_land && both_river {
                                crossing = Some((seed, left, right));
                                break 'search;
                            }
                        }
                    }
                }
            }
        }

        let (seed, left, right) = crossing.expect("expected a negative-coordinate river seam");
        assert!(left.x < 0 && right.x < 0);
        for position in [left, right] {
            let coord = chunk_coord(position);
            let chunk = World::generate_chunk_at(seed, coord).unwrap();
            let origin = chunk_origin(coord);
            let index = ((position.y - origin.y) * CHUNK_SIZE + position.x - origin.x) as usize;
            assert!(matches!(
                chunk.terrain()[index].ground,
                GroundType::DeepWater | GroundType::ShallowWater
            ));
            assert!(
                chunk
                    .features()
                    .iter()
                    .all(|feature| feature.position != position)
            );
        }
    }

    #[test]
    fn river_water_is_four_connected_to_its_continental_outlet() {
        let seed = 1;
        let river = (0..2)
            .flat_map(|region_y| (0..2).map(move |region_x| (region_x, region_y)))
            .find_map(|(region_x, region_y)| {
                river_descriptor(
                    seed,
                    region_x * RIVER_REGION_SIZE,
                    region_y * RIVER_REGION_SIZE,
                )
            })
            .expect("configured seed should contain a river");
        let margin = RIVER_MOUTH_HALF_WIDTH;
        let min_x = river.points().iter().map(|point| point.x).min().unwrap() - margin;
        let min_y = river.points().iter().map(|point| point.y).min().unwrap() - margin;
        let max_x = river.points().iter().map(|point| point.x).max().unwrap() + margin;
        let max_y = river.points().iter().map(|point| point.y).max().unwrap() + margin;
        let width = (max_x - min_x + 1) as usize;
        let height = (max_y - min_y + 1) as usize;
        let mut water = vec![false; width * height];
        let points = river.points();
        let segment_count = points.len() - 1;
        let headwater = RiverSegment {
            start: points[0],
            end: points[0],
            half_width: RIVER_HEADWATER_HALF_WIDTH,
        };
        let channel = points
            .windows(2)
            .enumerate()
            .map(|(segment_index, points)| RiverSegment {
                start: points[0],
                end: points[1],
                half_width: river_half_width(segment_index, segment_count),
            });
        for segment in std::iter::once(headwater).chain(channel) {
            let segment_min_x = segment.start.x.min(segment.end.x) - segment.half_width;
            let segment_min_y = segment.start.y.min(segment.end.y) - segment.half_width;
            let segment_max_x = segment.start.x.max(segment.end.x) + segment.half_width;
            let segment_max_y = segment.start.y.max(segment.end.y) + segment.half_width;
            for y in segment_min_y..=segment_max_y {
                for x in segment_min_x..=segment_max_x {
                    if river_segment_elevation(x, y, segment)
                        .is_some_and(|elevation| elevation <= RIVER_SHALLOW_ELEVATION)
                    {
                        water[(y - min_y) as usize * width + (x - min_x) as usize] = true;
                    }
                }
            }
        }

        let source = river.points()[0];
        assert!(continental_elevation(seed, source.x, source.y).elevation > i64::from(SAND_MAX));
        let source_index = (source.y - min_y) as usize * width + (source.x - min_x) as usize;
        assert!(water[source_index]);
        let mut visited = vec![false; water.len()];
        visited[source_index] = true;
        let mut stack = vec![source_index];
        let mut visited_count = 0;
        let mut touches_ocean = false;
        while let Some(index) = stack.pop() {
            visited_count += 1;
            let local_x = index % width;
            let local_y = index / width;
            let x = min_x + local_x as i64;
            let y = min_y + local_y as i64;
            touches_ocean |=
                continental_elevation(seed, x, y).elevation <= i64::from(SHALLOW_WATER_MAX);
            let neighbors = [
                local_x.checked_sub(1).map(|next| local_y * width + next),
                (local_x + 1 < width).then_some(local_y * width + local_x + 1),
                local_y.checked_sub(1).map(|next| next * width + local_x),
                (local_y + 1 < height).then_some((local_y + 1) * width + local_x),
            ];
            for neighbor in neighbors.into_iter().flatten() {
                if water[neighbor] && !visited[neighbor] {
                    visited[neighbor] = true;
                    stack.push(neighbor);
                }
            }
        }
        assert_eq!(visited_count, water.iter().filter(|&&cell| cell).count());
        assert!(touches_ocean);
    }

    #[test]
    fn major_river_survives_overview_sampling_as_one_route() {
        const STEP: i64 = 16;
        let seed = 1;
        let river = (0..2)
            .flat_map(|region_y| (0..2).map(move |region_x| (region_x, region_y)))
            .find_map(|(region_x, region_y)| {
                river_descriptor(
                    seed,
                    region_x * RIVER_REGION_SIZE,
                    region_y * RIVER_REGION_SIZE,
                )
            })
            .expect("configured seed should contain a river");
        let min_x = river
            .points()
            .iter()
            .map(|point| point.x)
            .min()
            .unwrap()
            .div_euclid(STEP)
            * STEP;
        let min_y = river
            .points()
            .iter()
            .map(|point| point.y)
            .min()
            .unwrap()
            .div_euclid(STEP)
            * STEP;
        let max_x =
            river.points().iter().map(|point| point.x).max().unwrap() + RIVER_MOUTH_HALF_WIDTH;
        let max_y =
            river.points().iter().map(|point| point.y).max().unwrap() + RIVER_MOUTH_HALF_WIDTH;
        let width = ((max_x - min_x).div_euclid(STEP) + 1) as usize;
        let height = ((max_y - min_y).div_euclid(STEP) + 1) as usize;
        let mut water = vec![false; width * height];
        for sample_y in 0..height {
            for sample_x in 0..width {
                let x = min_x + sample_x as i64 * STEP;
                let y = min_y + sample_y as i64 * STEP;
                water[sample_y * width + sample_x] = river_route_elevation(x, y, river)
                    .is_some_and(|elevation| elevation <= RIVER_SHALLOW_ELEVATION);
            }
        }

        let source = river.points()[0];
        let source_x = ((source.x - min_x) / STEP) as usize;
        let source_y = ((source.y - min_y) / STEP) as usize;
        let source_index = source_y * width + source_x;
        assert!(water[source_index]);
        let mut visited = vec![false; water.len()];
        visited[source_index] = true;
        let mut stack = vec![source_index];
        let mut touches_ocean = false;
        while let Some(index) = stack.pop() {
            let sample_x = index % width;
            let sample_y = index / width;
            let x = min_x + sample_x as i64 * STEP;
            let y = min_y + sample_y as i64 * STEP;
            touches_ocean |=
                continental_elevation(seed, x, y).elevation <= i64::from(SHALLOW_WATER_MAX);
            for offset_y in -1_isize..=1 {
                for offset_x in -1_isize..=1 {
                    if offset_x == 0 && offset_y == 0 {
                        continue;
                    }
                    let next_x = sample_x.checked_add_signed(offset_x);
                    let next_y = sample_y.checked_add_signed(offset_y);
                    let Some(next) = next_x
                        .filter(|&next| next < width)
                        .zip(next_y.filter(|&next| next < height))
                        .map(|(next_x, next_y)| next_y * width + next_x)
                    else {
                        continue;
                    };
                    if water[next] && !visited[next] {
                        visited[next] = true;
                        stack.push(next);
                    }
                }
            }
        }
        assert_eq!(
            visited.iter().filter(|&&cell| cell).count(),
            water.iter().filter(|&&cell| cell).count()
        );
        assert!(touches_ocean);
    }

    #[test]
    fn cell_rejects_out_of_bounds_positions() {
        let world = World::generate_square(1, 16);
        assert!(world.cell(WorldPosition { x: 15, y: 15 }).is_some());
        assert!(world.cell(WorldPosition { x: 16, y: 0 }).is_none());
    }

    #[test]
    fn sparse_features_are_sorted_and_addressable() {
        let world = World::generate_square(7, 256);
        for pair in world.features().windows(2) {
            assert!(
                position_key(pair[0].position, world.width())
                    < position_key(pair[1].position, world.width())
            );
        }
        let feature = world.features()[0];
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
        assert_eq!(world.terrain().len(), 96 * 64);
    }

    #[test]
    fn overlapping_initial_areas_generate_identical_cells() {
        let small = World::generate(11, WorldConfig::new(96, 64).unwrap());
        let large = World::generate(11, WorldConfig::new(128, 96).unwrap());
        for position in [
            WorldPosition { x: 0, y: 0 },
            WorldPosition { x: 63, y: 31 },
            WorldPosition { x: 95, y: 63 },
        ] {
            assert_eq!(small.cell(position), large.cell(position));
            assert_eq!(small.feature_at(position), large.feature_at(position));
        }
    }

    #[test]
    fn initial_area_rejects_unsafe_dimensions() {
        assert_eq!(WorldConfig::new(0, 10), Err(WorldConfigError::Empty));
        assert!(matches!(
            WorldConfig::new(8_192, 8_192),
            Err(WorldConfigError::TooLarge { .. })
        ));
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
            Err(GenerateAreaError::TooLarge)
        );
    }

    #[test]
    fn generated_area_rejects_more_than_large_chunk_budget() {
        let bounds = WorldRect::from_inclusive_points(
            WorldPosition {
                x: 10_000,
                y: 10_000,
            },
            WorldPosition {
                x: 15_000,
                y: 15_000,
            },
        );
        assert!(matches!(
            World::generate_chunks_streaming(1, bounds),
            Err(GenerateAreaError::TooManyChunks { .. })
        ));
    }

    #[test]
    fn generation_budget_accepts_4096_chunks_and_rejects_4097() {
        let maximum = WorldRect {
            min: WorldPosition { x: 0, y: 0 },
            max: WorldPosition {
                x: 64 * CHUNK_SIZE,
                y: 64 * CHUNK_SIZE,
            },
        };
        assert!(World::generate_chunks_streaming(1, maximum).is_ok());

        let over = WorldRect {
            min: WorldPosition { x: 0, y: 0 },
            max: WorldPosition {
                x: 17 * CHUNK_SIZE,
                y: 241 * CHUNK_SIZE,
            },
        };
        assert!(matches!(
            World::generate_chunks_streaming(1, over),
            Err(GenerateAreaError::TooManyChunks {
                requested: 4_097,
                maximum: MAX_CHUNKS_PER_GENERATION,
            })
        ));
    }

    #[test]
    fn generation_budget_counts_only_missing_chunks() {
        let world = World::generate(1, WorldConfig::new(64, 64).unwrap());
        let overlaps_initial = WorldRect {
            min: WorldPosition { x: 0, y: 0 },
            max: WorldPosition {
                x: 17 * CHUNK_SIZE,
                y: 241 * CHUNK_SIZE,
            },
        };
        assert_eq!(
            world.validate_generation_request(overlaps_initial),
            Ok(MAX_CHUNKS_PER_GENERATION)
        );
        let missing = world.missing_chunk_coords(overlaps_initial).unwrap();
        assert_eq!(missing.len(), MAX_CHUNKS_PER_GENERATION as usize);
        assert!(!missing.contains(&ChunkCoord { x: 0, y: 0 }));

        let outside_origin = WorldPosition {
            x: 100 * CHUNK_SIZE,
            y: 100 * CHUNK_SIZE,
        };
        let requires_4097 = WorldRect {
            min: outside_origin,
            max: WorldPosition {
                x: outside_origin.x + 17 * CHUNK_SIZE,
                y: outside_origin.y + 241 * CHUNK_SIZE,
            },
        };
        assert!(matches!(
            world.validate_generation_request(requires_4097),
            Err(GenerateAreaError::TooManyChunks {
                requested: 4_097,
                maximum: MAX_CHUNKS_PER_GENERATION,
            })
        ));
        assert!(matches!(
            world.missing_chunk_coords(requires_4097),
            Err(GenerateAreaError::TooManyChunks {
                requested: 4_097,
                maximum: MAX_CHUNKS_PER_GENERATION,
            })
        ));
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
                ChunkPresence::Missing,
            ),
            (
                WorldPosition { x: -1, y: 0 },
                ChunkCoord { x: -1, y: 0 },
                ChunkLocalPosition { x: 63, y: 0 },
                ChunkPresence::Missing,
            ),
            (
                WorldPosition { x: 0, y: 0 },
                ChunkCoord { x: 0, y: 0 },
                ChunkLocalPosition { x: 0, y: 0 },
                ChunkPresence::Initial,
            ),
            (
                WorldPosition { x: 63, y: 0 },
                ChunkCoord { x: 0, y: 0 },
                ChunkLocalPosition { x: 63, y: 0 },
                ChunkPresence::Initial,
            ),
            (
                WorldPosition { x: 64, y: 0 },
                ChunkCoord { x: 1, y: 0 },
                ChunkLocalPosition { x: 0, y: 0 },
                ChunkPresence::PartialInitial,
            ),
            (
                WorldPosition { x: 96, y: 0 },
                ChunkCoord { x: 1, y: 0 },
                ChunkLocalPosition { x: 32, y: 0 },
                ChunkPresence::PartialInitial,
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
                min: WorldPosition { x: 96, y: 0 },
                max: WorldPosition { x: 128, y: 64 },
            })
            .unwrap();
        assert_eq!(
            world
                .inspect_chunk_at(WorldPosition { x: 96, y: 0 })
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
            Err(GenerateAreaError::TooLarge)
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
        assert!(matches!(
            world.validate_generation_request(bounds),
            Err(GenerateAreaError::TooManyChunks {
                requested: 4_097,
                maximum: MAX_CHUNKS_PER_GENERATION,
            })
        ));
    }

    #[test]
    fn one_dimensional_extreme_range_is_rejected() {
        let bounds = WorldRect {
            min: WorldPosition { x: i64::MIN, y: 0 },
            max: WorldPosition { x: 0, y: 1 },
        };
        assert!(matches!(
            World::generate_chunks_streaming(1, bounds),
            Err(GenerateAreaError::TooManyChunks { .. })
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
            Err(GenerateAreaError::TooLarge)
        );
    }

    #[test]
    fn chunk_at_negative_coordinate_limit_is_generated() {
        let coord = ChunkCoord {
            x: i64::MIN / CHUNK_SIZE,
            y: 0,
        };
        let chunk = World::generate_chunk_at(1, coord).expect("minimum origin is representable");
        assert_eq!(chunk.coord(), coord);
        assert_eq!(chunk.terrain().len(), (CHUNK_SIZE * CHUNK_SIZE) as usize);
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
        assert_eq!(world.cells().count(), 64 * 64 + 64 * 64);
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
            vec![ChunkCoord { x: -1, y: 0 }, ChunkCoord { x: 1, y: 0 }]
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

        assert_eq!(positions.len(), 512);
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
    fn generated_chunk_store_enforces_total_capacity() {
        let mut world = World::generate(1, WorldConfig::new(64, 64).unwrap());
        let chunks = (0..MAX_GENERATED_CHUNKS)
            .map(|index| WorldChunk {
                coord: ChunkCoord {
                    x: index as i64 + 1,
                    y: 0,
                },
                terrain: Vec::new(),
                features: Vec::new(),
            })
            .collect();
        assert_eq!(
            world.insert_chunks(chunks).expect("capacity is accepted"),
            MAX_GENERATED_CHUNKS
        );
        let missing_bounds = WorldRect {
            min: WorldPosition { x: -64, y: 0 },
            max: WorldPosition { x: 0, y: 64 },
        };
        let capacity_error = GenerateAreaError::WorldCapacity {
            requested: 1,
            remaining: 0,
        };
        assert_eq!(
            world.validate_generation_request(missing_bounds),
            Err(capacity_error)
        );
        assert_eq!(
            world.missing_chunk_coords(missing_bounds),
            Err(capacity_error)
        );
        let error = world
            .insert_chunks(vec![WorldChunk {
                coord: ChunkCoord { x: -1, y: 0 },
                terrain: Vec::new(),
                features: Vec::new(),
            }])
            .expect_err("capacity must be enforced");
        assert_eq!(
            error,
            GenerateAreaError::WorldCapacity {
                requested: 1,
                remaining: 0,
            }
        );
    }
}
