//! Layered deterministic world generation.
//!
//! Structure comes from three tiers, largest first:
//! 1. Global analytic fields (`plates`, `climate`): continents, oceans,
//!    mountain arcs, plateaus, temperature, and moisture as pure functions of
//!    the seed and world coordinates.
//! 2. Canonical drainage (`drainage`, refined by `hydrology`): one bounded
//!    whole-envelope basin/channel graph plus cached per-region sampling.
//! 3. Local detail: small roughness-budgeted noise applied per cell, which may
//!    erode coastlines and vary forests but never decides where geography is.

mod climate;
mod drainage;
mod hydrology;
mod noise;
mod plates;

use std::{
    collections::BTreeSet as RegionSet,
    sync::{Arc, Mutex, MutexGuard, OnceLock},
};

use rayon::prelude::*;

use crate::world::{
    BiomeType, CHUNK_SIZE, ChunkLoadRequest, ClimateSample, FeatureKind, SurfaceType, TerrainCell,
    TerrainClass,
};
use drainage::FLOODPLAIN_RADIUS;
use hydrology::{
    GRID, LAKE_MIN_DEPTH, NODE_STEP, RegionMap, RiverSegment, point_segment_distance_ratio,
};
use noise::{NOISE_HALF, centered_noise, hash};
use plates::{SEA_LEVEL, macro_sample};

pub(crate) use hydrology::REGION_SIZE;

const DEEP_WATER_MAX: i32 = 25_000;
const BEACH_MAX: i32 = 33_000;
const HILL_MIN: i32 = 50_000;
const ROCK_MIN: i32 = 56_000;
const LAKE_DEEP_DEPTH: i32 = 1_600;
const DESERT_MOISTURE_MAX: i32 = 12_000;
const DESERT_TEMPERATURE_MIN: i32 = 30_000;
const FOREST_MOISTURE_MIN: i32 = 30_000;
const FOREST_TEMPERATURE_MIN: i32 = 13_000;
const WETLAND_MOISTURE_MIN: i32 = 48_000;
const WETLAND_ELEVATION_MAX: i32 = 39_000;
const WETLAND_SLOPE_MAX: i64 = 90;
const RIPARIAN_MOISTURE_MIN: i32 = 10_000;
const RIPARIAN_ELEVATION_MAX: i32 = 42_000;
const RIPARIAN_SLOPE_MAX: i64 = 180;
const RIVERBANK_RADIUS: i64 = 4;
const SAVANNA_MOISTURE_MAX: i32 = 24_000;
const TUNDRA_TEMPERATURE_MAX: i32 = 13_000;
const SNOW_TEMPERATURE_MAX: i32 = 6_500;
const MOUNTAIN_SNOW_TEMPERATURE_MAX: i32 = 9_000;

const DETAIL_SEED_A: u64 = 0x4445_5441_494c_4131;
const DETAIL_SEED_B: u64 = 0x4445_5441_494c_4232;
const FEATURE_SEED: u64 = 0x4654_5253;

const REGION_CACHE_CAPACITY: usize = 64;

type RegionKey = (u64, i64, i64);
type RegionSlot = Arc<OnceLock<Arc<RegionMap>>>;

static REGION_CACHE: OnceLock<Mutex<Vec<(RegionKey, RegionSlot)>>> = OnceLock::new();

fn region_cache() -> &'static Mutex<Vec<(RegionKey, RegionSlot)>> {
    REGION_CACHE.get_or_init(|| Mutex::new(Vec::new()))
}

fn lock_region_cache() -> MutexGuard<'static, Vec<(RegionKey, RegionSlot)>> {
    region_cache()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn trim_region_cache(cache: &mut Vec<(RegionKey, RegionSlot)>) {
    while cache.len() > REGION_CACHE_CAPACITY {
        let Some(position) = cache.iter().rposition(|(_, slot)| slot.get().is_some()) else {
            // More than 64 distinct regions may briefly be building at once on
            // a large machine. Never evict an in-flight build: doing so could
            // let another worker duplicate the same expensive regional solve.
            break;
        };
        cache.remove(position);
    }
}

fn region(seed: u64, region_x: i64, region_y: i64) -> Arc<RegionMap> {
    let key = (seed, region_x, region_y);
    let slot = {
        let mut cache = lock_region_cache();
        if let Some(position) = cache.iter().position(|(entry, _)| *entry == key) {
            let entry = cache.remove(position);
            let slot = Arc::clone(&entry.1);
            cache.insert(0, entry);
            slot
        } else {
            let slot = Arc::new(OnceLock::new());
            cache.insert(0, (key, Arc::clone(&slot)));
            trim_region_cache(&mut cache);
            slot
        }
    };
    let map = Arc::clone(slot.get_or_init(|| Arc::new(RegionMap::build(seed, region_x, region_y))));
    trim_region_cache(&mut lock_region_cache());
    map
}

pub(crate) fn climate_at(seed: u64, x: i64, y: i64, moisture: u8) -> ClimateSample {
    let base_x = x.div_euclid(NODE_STEP) * NODE_STEP;
    let base_y = y.div_euclid(NODE_STEP) * NODE_STEP;
    let mut temperatures = [0_i32; 4];
    for (index, (node_x, node_y)) in [
        (base_x, base_y),
        (base_x + NODE_STEP, base_y),
        (base_x, base_y + NODE_STEP),
        (base_x + NODE_STEP, base_y + NODE_STEP),
    ]
    .into_iter()
    .enumerate()
    {
        let elevation = macro_sample(seed, node_x, node_y).elevation;
        temperatures[index] = climate::temperature(seed, node_x, node_y, elevation);
    }
    let fx = x.rem_euclid(NODE_STEP);
    let fy = y.rem_euclid(NODE_STEP);
    let top = i64::from(temperatures[0]) * (NODE_STEP - fx) + i64::from(temperatures[1]) * fx;
    let bottom = i64::from(temperatures[2]) * (NODE_STEP - fx) + i64::from(temperatures[3]) * fx;
    ClimateSample {
        temperature: ((top * (NODE_STEP - fy) + bottom * fy) / (NODE_STEP * NODE_STEP)) as u16,
        moisture,
        wind: climate::prevailing_wind(seed, x, y),
    }
}

/// Materializes the regional prerequisites for an ordered, bounded chunk
/// request window before dependent chunk workers enter the build-once cache.
pub(crate) fn prepare_chunk_regions(seed: u64, requests: &[ChunkLoadRequest]) {
    if requests.is_empty() {
        return;
    }
    drainage::world_drainage(seed);
    let chunks_per_region = REGION_SIZE / CHUNK_SIZE;
    let mut prepared = RegionSet::new();
    for request in requests {
        let coord = request.coord();
        let key = (
            coord.x.div_euclid(chunks_per_region),
            coord.y.div_euclid(chunks_per_region),
        );
        prepared.insert(key);
    }
    prepared
        .into_iter()
        .collect::<Vec<_>>()
        .into_par_iter()
        .for_each(|(region_x, region_y)| {
            region(seed, region_x, region_y);
        });
}

// Whole-world channel links are subdivided at the 32-cell refinement step.
// Thirteen is the measured water-plus-floodplain maximum across the four
// representative seeds and complete envelope; the fixed array avoids one
// allocation per chunk.
const MAX_CHUNK_RIVERS: usize = 13;
const EMPTY_SEGMENT: RiverSegment = RiverSegment {
    ax: 0,
    ay: 0,
    bx: 0,
    by: 0,
    channel_id: 0,
    surface_a: 0,
    surface_b: 0,
    half_width: 0,
    stream_order: 0,
};

/// Everything one chunk needs from the global and regional tiers: a 3x3 node
/// lattice to interpolate plus the river segments that touch the chunk.
pub(crate) struct ChunkContext {
    seed: u64,
    origin_x: i64,
    origin_y: i64,
    elevation: [i32; 9],
    water_depth: [i32; 9],
    temperature: [i32; 9],
    moisture: [i32; 9],
    roughness: [i32; 9],
    rivers: [RiverSegment; MAX_CHUNK_RIVERS],
    river_len: usize,
    overflow_rivers: Vec<RiverSegment>,
}

impl ChunkContext {
    pub(crate) fn new(seed: u64, origin_x: i64, origin_y: i64) -> Self {
        let region_x = origin_x.div_euclid(REGION_SIZE);
        let region_y = origin_y.div_euclid(REGION_SIZE);
        let map = region(seed, region_x, region_y);
        let node_x = ((origin_x - region_x * REGION_SIZE) / NODE_STEP) as usize;
        let node_y = ((origin_y - region_y * REGION_SIZE) / NODE_STEP) as usize;

        let mut context = Self {
            seed,
            origin_x,
            origin_y,
            elevation: [0; 9],
            water_depth: [0; 9],
            temperature: [0; 9],
            moisture: [0; 9],
            roughness: [0; 9],
            rivers: [EMPTY_SEGMENT; MAX_CHUNK_RIVERS],
            river_len: 0,
            overflow_rivers: Vec::new(),
        };
        for offset_y in 0..3 {
            for offset_x in 0..3 {
                let node = (node_y + offset_y) * GRID + node_x + offset_x;
                let local = offset_y * 3 + offset_x;
                context.elevation[local] = map.elevation[node];
                context.water_depth[local] = map.water_depth[node];
                context.temperature[local] = map.temperature[node];
                context.moisture[local] = map.moisture[node];
                context.roughness[local] = map.roughness[node];
            }
        }

        for segment in &map.rivers {
            if river_intersects_chunk(*segment, origin_x, origin_y) {
                context.push_river(*segment);
            }
        }
        context
    }

    fn push_river(&mut self, segment: RiverSegment) {
        if self.river_len < MAX_CHUNK_RIVERS {
            self.rivers[self.river_len] = segment;
            self.river_len += 1;
        } else {
            self.overflow_rivers.push(segment);
        }
    }

    fn river_segments(&self) -> impl Iterator<Item = &RiverSegment> {
        self.rivers[..self.river_len]
            .iter()
            .chain(self.overflow_rivers.iter())
    }

    fn interpolate(&self, values: &[i32; 9], local_x: i64, local_y: i64) -> i64 {
        debug_assert!((0..=CHUNK_SIZE).contains(&local_x));
        debug_assert!((0..=CHUNK_SIZE).contains(&local_y));
        let cell_x = (local_x / NODE_STEP).min(1) as usize;
        let cell_y = (local_y / NODE_STEP).min(1) as usize;
        let fx = local_x - cell_x as i64 * NODE_STEP;
        let fy = local_y - cell_y as i64 * NODE_STEP;
        let base = cell_y * 3 + cell_x;
        let top = i64::from(values[base]) * (NODE_STEP - fx) + i64::from(values[base + 1]) * fx;
        let bottom =
            i64::from(values[base + 3]) * (NODE_STEP - fx) + i64::from(values[base + 4]) * fx;
        (top * (NODE_STEP - fy) + bottom * fy) / (NODE_STEP * NODE_STEP)
    }

    pub(crate) fn generate(&self, x: i64, y: i64) -> (TerrainCell, Option<FeatureKind>) {
        let local_x = x - self.origin_x;
        let local_y = y - self.origin_y;
        let macro_elevation = self.interpolate(&self.elevation, local_x, local_y);
        let roughness = self.interpolate(&self.roughness, local_x, local_y);
        let temperature = self.interpolate(&self.temperature, local_x, local_y) as i32;
        let moisture = self.interpolate(&self.moisture, local_x, local_y) as i32;
        let water_depth = self.interpolate(&self.water_depth, local_x, local_y) as i32;
        let slope = self.local_slope(local_x, local_y);

        // Local detail is the last tier: its amplitude comes from the regional
        // roughness budget and is damped near sea level so coasts stay ragged
        // without dissolving into speckle.
        let detail = local_detail(self.seed, x, y);
        let coast_damp = 300 + (macro_elevation - i64::from(SEA_LEVEL)).abs().min(2_300);
        let amplitude = roughness * coast_damp / 2_600;
        let mut elevation = (macro_elevation + detail * amplitude / NOISE_HALF) as i32;

        let mut water_surface = None;
        let mut water_biome = None;
        let mut river_grade = None;
        let mut floodplain = false;
        let mut riverbank = false;
        if water_depth >= LAKE_MIN_DEPTH {
            let surface = macro_elevation + i64::from(water_depth - LAKE_MIN_DEPTH);
            elevation = surface.clamp(0, 65_535) as i32;
            water_surface = Some(if water_depth >= LAKE_MIN_DEPTH + LAKE_DEEP_DEPTH {
                SurfaceType::DeepWater
            } else {
                SurfaceType::ShallowWater
            });
            water_biome = Some(BiomeType::Lake);
        }
        for segment in self.river_segments() {
            let (distance_sq, denominator) = point_segment_distance_ratio(
                x,
                y,
                i64::from(segment.ax),
                i64::from(segment.ay),
                i64::from(segment.bx),
                i64::from(segment.by),
            );
            let half_width = i64::from(segment.half_width);
            let floodplain_width = half_width + FLOODPLAIN_RADIUS;
            floodplain |=
                distance_sq <= i128::from(floodplain_width * floodplain_width) * denominator;
            let bank_width = half_width + RIVERBANK_RADIUS;
            riverbank |= distance_sq <= i128::from(bank_width * bank_width) * denominator;
            let core = half_width * 5 / 8;
            if segment.half_width >= 8 && distance_sq <= i128::from(core * core) * denominator {
                let grade = river_surface_at(x, y, *segment);
                river_grade = Some(river_grade.map_or(grade, |current: i32| current.min(grade)));
                water_surface = Some(SurfaceType::DeepWater);
                water_biome.get_or_insert(BiomeType::River);
            } else if distance_sq <= i128::from(half_width * half_width) * denominator {
                let grade = river_surface_at(x, y, *segment);
                river_grade = Some(river_grade.map_or(grade, |current: i32| current.min(grade)));
                if water_surface != Some(SurfaceType::DeepWater) {
                    water_surface = Some(SurfaceType::ShallowWater);
                }
                water_biome.get_or_insert(BiomeType::River);
            }
        }
        if water_biome == Some(BiomeType::River) {
            elevation = river_grade.expect("river water has a longitudinal grade");
        }

        let basin_edge = water_depth > 0 && water_depth < LAKE_MIN_DEPTH;
        let hydrologic_wetland = (basin_edge || floodplain) && slope <= WETLAND_SLOPE_MAX;
        let riparian_bank = (basin_edge || riverbank) && slope <= RIPARIAN_SLOPE_MAX;
        let class = water_surface.map_or_else(
            || {
                classify(
                    elevation,
                    moisture,
                    temperature,
                    hydrologic_wetland,
                    riparian_bank,
                    detail as i32,
                )
            },
            |surface| {
                TerrainClass::new(
                    surface,
                    water_biome.expect("generated water has an owned water-body class"),
                )
            },
        );
        let cell = TerrainCell::new(
            elevation.clamp(0, 65_535) as u16,
            (moisture.clamp(0, 65_535) >> 8) as u8,
            class.surface(),
            class.biome(),
        );
        (
            cell,
            feature(
                self.seed,
                x,
                y,
                FeatureEnvironment {
                    class,
                    moisture,
                    temperature,
                    slope,
                    near_water: basin_edge || riverbank,
                    ecology: detail,
                },
            ),
        )
    }

    fn local_slope(&self, local_x: i64, local_y: i64) -> i64 {
        let next_x = (local_x + 1).min(CHUNK_SIZE);
        let next_y = (local_y + 1).min(CHUNK_SIZE);
        let elevation = self.interpolate(&self.elevation, local_x, local_y);
        let dx = self.interpolate(&self.elevation, next_x, local_y) - elevation;
        let dy = self.interpolate(&self.elevation, local_x, next_y) - elevation;
        dx.abs().max(dy.abs())
    }
}

fn river_intersects_chunk(segment: RiverSegment, origin_x: i64, origin_y: i64) -> bool {
    let max_x = origin_x + CHUNK_SIZE;
    let max_y = origin_y + CHUNK_SIZE;
    let influence = i64::from(segment.half_width) + FLOODPLAIN_RADIUS;
    i64::from(segment.ax.max(segment.bx)).saturating_add(influence) >= origin_x
        && i64::from(segment.ax.min(segment.bx)).saturating_sub(influence) < max_x
        && i64::from(segment.ay.max(segment.by)).saturating_add(influence) >= origin_y
        && i64::from(segment.ay.min(segment.by)).saturating_sub(influence) < max_y
}

fn river_surface_at(x: i64, y: i64, segment: RiverSegment) -> i32 {
    let dx = i128::from(segment.bx) - i128::from(segment.ax);
    let dy = i128::from(segment.by) - i128::from(segment.ay);
    let length_squared = dx * dx + dy * dy;
    if length_squared == 0 {
        return i32::from(segment.surface_a);
    }
    let projection = ((i128::from(x) - i128::from(segment.ax)) * dx
        + (i128::from(y) - i128::from(segment.ay)) * dy)
        .clamp(0, length_squared);
    let surface = (i128::from(segment.surface_a) * (length_squared - projection)
        + i128::from(segment.surface_b) * projection)
        / length_squared;
    surface as i32
}

fn classify(
    elevation: i32,
    moisture: i32,
    temperature: i32,
    hydrologic_wetland: bool,
    riparian_bank: bool,
    transition: i32,
) -> TerrainClass {
    let transition = transition.clamp(-(NOISE_HALF as i32), NOISE_HALF as i32);
    let beach_max = BEACH_MAX + transition / 40;
    let desert_moisture_max = DESERT_MOISTURE_MAX + transition / 24;
    let forest_moisture_min = FOREST_MOISTURE_MIN + transition / 16;
    let wetland_moisture_min = WETLAND_MOISTURE_MIN + transition / 32;
    let forest_temperature_min = FOREST_TEMPERATURE_MIN + transition / 32;
    let tundra_temperature_max = TUNDRA_TEMPERATURE_MAX + transition / 32;
    let snow_temperature_max = SNOW_TEMPERATURE_MAX + transition / 40;
    let mountain_snow_temperature_max = MOUNTAIN_SNOW_TEMPERATURE_MAX + transition / 40;
    if elevation <= DEEP_WATER_MAX {
        TerrainClass::new(SurfaceType::DeepWater, BiomeType::Ocean)
    } else if elevation <= SEA_LEVEL {
        TerrainClass::new(SurfaceType::ShallowWater, BiomeType::Ocean)
    } else if elevation <= beach_max {
        TerrainClass::new(SurfaceType::Sand, BiomeType::Beach)
    } else if temperature < snow_temperature_max {
        TerrainClass::new(SurfaceType::SnowIce, BiomeType::Tundra)
    } else if elevation > ROCK_MIN {
        let surface = if temperature < mountain_snow_temperature_max {
            SurfaceType::SnowIce
        } else {
            SurfaceType::Rock
        };
        TerrainClass::new(surface, BiomeType::Alpine)
    } else if elevation > HILL_MIN {
        let surface = if temperature < mountain_snow_temperature_max {
            SurfaceType::SnowIce
        } else {
            SurfaceType::Hill
        };
        TerrainClass::new(surface, BiomeType::Alpine)
    } else if temperature < tundra_temperature_max {
        TerrainClass::new(SurfaceType::Soil, BiomeType::Tundra)
    } else if hydrologic_wetland
        && moisture > wetland_moisture_min
        && elevation <= WETLAND_ELEVATION_MAX
    {
        TerrainClass::new(SurfaceType::Soil, BiomeType::Wetland)
    } else if riparian_bank
        && moisture >= RIPARIAN_MOISTURE_MIN
        && elevation <= RIPARIAN_ELEVATION_MAX
    {
        TerrainClass::new(SurfaceType::Soil, BiomeType::Grassland)
    } else if moisture < desert_moisture_max && temperature > DESERT_TEMPERATURE_MIN {
        TerrainClass::new(SurfaceType::Sand, BiomeType::Desert)
    } else if moisture > forest_moisture_min && temperature > forest_temperature_min {
        TerrainClass::new(SurfaceType::Soil, BiomeType::Forest)
    } else if moisture < SAVANNA_MOISTURE_MAX && temperature > DESERT_TEMPERATURE_MIN {
        TerrainClass::new(SurfaceType::Soil, BiomeType::Savanna)
    } else {
        TerrainClass::new(SurfaceType::Soil, BiomeType::Grassland)
    }
}

fn local_detail(seed: u64, x: i64, y: i64) -> i64 {
    (centered_noise(seed ^ DETAIL_SEED_A, x, y, 160) * 5
        + centered_noise(seed ^ DETAIL_SEED_B, x, y, 40) * 2)
        / 7
}

#[derive(Clone, Copy)]
struct FeatureEnvironment {
    class: TerrainClass,
    moisture: i32,
    temperature: i32,
    slope: i64,
    near_water: bool,
    ecology: i64,
}

fn feature(seed: u64, x: i64, y: i64, environment: FeatureEnvironment) -> Option<FeatureKind> {
    let FeatureEnvironment {
        class,
        moisture,
        temperature,
        slope,
        near_water,
        ecology,
    } = environment;
    if matches!(
        class.surface(),
        SurfaceType::DeepWater
            | SurfaceType::ShallowWater
            | SurfaceType::Sand
            | SurfaceType::SnowIce
    ) {
        return None;
    }
    let rolls = hash(seed ^ FEATURE_SEED, x, y);
    let tree_roll = (rolls % 10_000) as i64;
    let berry_roll = ((rolls >> 21) % 10_000) as i64;
    let rock_roll = ((rolls >> 42) % 10_000) as i64;
    match (class.surface(), class.biome()) {
        (SurfaceType::Soil, BiomeType::Forest) => {
            if ecology < -18_000 && rock_roll < 180 {
                Some(FeatureKind::Rock)
            } else if ecology > -7_000 && tree_roll < 820 {
                Some(FeatureKind::Tree)
            } else {
                (ecology > -20_000 && berry_roll < 120).then_some(FeatureKind::BerryBush)
            }
        }
        (SurfaceType::Soil, biome) => {
            let tree_threshold = match biome {
                BiomeType::Grassland if moisture > 28_000 => 9_000,
                BiomeType::Savanna if moisture > 17_000 => 15_000,
                _ => i64::MAX,
            };
            if ecology > tree_threshold && temperature > FOREST_TEMPERATURE_MIN && tree_roll < 360 {
                return Some(FeatureKind::Tree);
            }

            let berry_patch_min = if near_water { -11_000 } else { -3_000 };
            if matches!(
                biome,
                BiomeType::Grassland | BiomeType::Savanna | BiomeType::Wetland
            ) && moisture > 17_000
                && temperature > 9_000
                && ecology > berry_patch_min
                && ecology <= tree_threshold
                && berry_roll < 160
            {
                return Some(FeatureKind::BerryBush);
            }

            ((slope >= 70 || ecology < -14_000) && rock_roll < 150).then_some(FeatureKind::Rock)
        }
        (SurfaceType::Hill | SurfaceType::Rock, _) => {
            (ecology > -14_000 && rock_roll < 430).then_some(FeatureKind::Rock)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests;
