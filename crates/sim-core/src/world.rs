//! Deterministic terrain and sparse surface-feature generation.

use std::{error::Error, fmt};

/// Default side length of the initially generated area.
pub const DEFAULT_INITIAL_WORLD_SIZE: u32 = 1_024;
const MAX_INITIAL_CELLS: u64 = 16_777_216;
const CHUNK_SIZE: u32 = 64;
const NOISE_MAX: i64 = 65_535;

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

/// A generated base world. Terrain is dense; interactive objects remain sparse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct World {
    seed: u64,
    width: u32,
    height: u32,
    terrain: Vec<TerrainCell>,
    features: Vec<Feature>,
    generated_areas: Vec<GeneratedArea>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GeneratedArea {
    bounds: WorldRect,
    width: usize,
    terrain: Vec<TerrainCell>,
    features: Vec<Feature>,
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
        let chunks_x = width.div_ceil(CHUNK_SIZE);
        let chunks_y = height.div_ceil(CHUNK_SIZE);
        for chunk_y in 0..chunks_y {
            for chunk_x in 0..chunks_x {
                let end_y = ((chunk_y + 1) * CHUNK_SIZE).min(height);
                let end_x = ((chunk_x + 1) * CHUNK_SIZE).min(width);
                for y in chunk_y * CHUNK_SIZE..end_y {
                    for x in chunk_x * CHUNK_SIZE..end_x {
                        let elevation = terrain_noise(seed, x, y, 0);
                        let moisture = terrain_noise(seed, x, y, 1);
                        let ground = classify_ground(elevation, moisture);
                        terrain[(y * width + x) as usize] = TerrainCell {
                            elevation,
                            moisture: (moisture >> 8) as u8,
                            ground,
                        };

                        if let Some(kind) = generate_feature(seed, x, y, ground, moisture) {
                            features.push(Feature {
                                position: WorldPosition {
                                    x: i64::from(x),
                                    y: i64::from(y),
                                },
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
            generated_areas: Vec::new(),
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

    pub fn visible_features(&self) -> impl Iterator<Item = &Feature> {
        self.features.iter().chain(
            self.generated_areas
                .iter()
                .flat_map(|area| area.features.iter()),
        )
    }

    pub fn generate_area(&mut self, bounds: WorldRect) -> Result<(), GenerateAreaError> {
        let width = bounds.max.x.saturating_sub(bounds.min.x);
        let height = bounds.max.y.saturating_sub(bounds.min.y);
        let cells = width
            .checked_mul(height)
            .ok_or(GenerateAreaError::TooLarge)?;
        if width <= 0 || height <= 0 {
            return Err(GenerateAreaError::Empty);
        }
        if cells > 1_048_576 {
            return Err(GenerateAreaError::TooLarge);
        }
        if self.area_is_generated(bounds) {
            return Ok(());
        }

        let width_usize = width as usize;
        let mut terrain = Vec::with_capacity(cells as usize);
        let mut features = Vec::new();
        for y in bounds.min.y..bounds.max.y {
            for x in bounds.min.x..bounds.max.x {
                let elevation = terrain_noise(self.seed, x, y, 0);
                let moisture = terrain_noise(self.seed, x, y, 1);
                let ground = classify_ground(elevation, moisture);
                terrain.push(TerrainCell {
                    elevation,
                    moisture: (moisture >> 8) as u8,
                    ground,
                });
                if let Some(kind) = generate_feature(self.seed, x, y, ground, moisture) {
                    features.push(Feature {
                        position: WorldPosition { x, y },
                        kind,
                    });
                }
            }
        }
        self.generated_areas.push(GeneratedArea {
            bounds,
            width: width_usize,
            terrain,
            features,
        });
        Ok(())
    }

    fn area_is_generated(&self, bounds: WorldRect) -> bool {
        let initial = WorldRect {
            min: WorldPosition { x: 0, y: 0 },
            max: WorldPosition {
                x: i64::from(self.width),
                y: i64::from(self.height),
            },
        };
        (initial.contains(bounds.min)
            && initial.contains(WorldPosition {
                x: bounds.max.x - 1,
                y: bounds.max.y - 1,
            }))
            || self.generated_areas.iter().any(|area| {
                area.bounds.contains(bounds.min)
                    && area.bounds.contains(WorldPosition {
                        x: bounds.max.x - 1,
                        y: bounds.max.y - 1,
                    })
            })
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
        self.generated_areas
            .iter()
            .rev()
            .find(|area| area.bounds.contains(position))
            .and_then(|area| {
                area.features
                    .binary_search_by_key(&(position.y, position.x), |feature| {
                        (feature.position.y, feature.position.x)
                    })
                    .ok()
                    .and_then(|index| area.features.get(index))
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
        self.generated_areas
            .iter()
            .rev()
            .find(|area| area.bounds.contains(position))
            .and_then(|area| {
                let x = (position.x - area.bounds.min.x) as usize;
                let y = (position.y - area.bounds.min.y) as usize;
                area.terrain.get(y * area.width + x)
            })
            .copied()
    }
}

const fn position_key(position: WorldPosition, width: u32) -> u64 {
    position.y as u64 * width as u64 + position.x as u64
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenerateAreaError {
    Empty,
    TooLarge,
}

fn classify_ground(elevation: u16, moisture: u16) -> GroundType {
    match elevation {
        0..=20_500 => GroundType::DeepWater,
        20_501..=24_000 => GroundType::ShallowWater,
        24_001..=27_000 => GroundType::Sand,
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
    let roll = hash(seed ^ 0x4654_5253, x.into(), y.into()) % 10_000;
    match ground {
        GroundType::ForestFloor if roll < 700 => Some(FeatureKind::Tree),
        GroundType::Grass if moisture > 31_000 && roll < 90 => Some(FeatureKind::Tree),
        GroundType::Grass if roll < 125 => Some(FeatureKind::BerryBush),
        GroundType::Hill | GroundType::BareRock if roll < 180 => Some(FeatureKind::Rock),
        _ => None,
    }
}

fn terrain_noise(seed: u64, x: impl Into<i64>, y: impl Into<i64>, stream: u64) -> u16 {
    let x = x.into();
    let y = y.into();
    let seed = seed ^ stream.wrapping_mul(0x9e37_79b9_7f4a_7c15);
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
    fn generated_area_rejects_excessive_selection() {
        let mut world = World::generate(1, WorldConfig::new(64, 64).unwrap());
        let bounds = WorldRect::from_inclusive_points(
            WorldPosition { x: 2_000, y: 2_000 },
            WorldPosition { x: 4_000, y: 4_000 },
        );
        assert_eq!(
            world.generate_area(bounds),
            Err(GenerateAreaError::TooLarge)
        );
    }
}
