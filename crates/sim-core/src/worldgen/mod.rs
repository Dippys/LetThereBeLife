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
use hydrology::{
    GRID, LAKE_MIN_DEPTH, NODE_STEP, RegionMap, RiverSegment, point_segment_distance_ratio,
};
use noise::{NOISE_HALF, centered_noise, hash, value_noise};
use plates::{SEA_LEVEL, macro_sample};

pub(crate) use hydrology::REGION_SIZE;

const DEEP_WATER_MAX: i32 = 25_000;
const BEACH_MAX: i32 = 33_000;
const HILL_MIN: i32 = 50_000;
const ROCK_MIN: i32 = 56_000;
const LAKE_DEEP_DEPTH: i32 = 1_600;
const DEEP_WATER_FILL: i32 = 24_500;
const SHALLOW_WATER_FILL: i32 = 27_800;
const DESERT_MOISTURE_MAX: i32 = 12_000;
const DESERT_TEMPERATURE_MIN: i32 = 30_000;
const FOREST_MOISTURE_MIN: i32 = 30_000;
const FOREST_TEMPERATURE_MIN: i32 = 13_000;
const WETLAND_MOISTURE_MIN: i32 = 48_000;
const WETLAND_ELEVATION_MAX: i32 = 39_000;
const SAVANNA_MOISTURE_MAX: i32 = 24_000;
const TUNDRA_TEMPERATURE_MAX: i32 = 13_000;
const SNOW_TEMPERATURE_MAX: i32 = 6_500;
const MOUNTAIN_SNOW_TEMPERATURE_MAX: i32 = 9_000;

const DETAIL_SEED_A: u64 = 0x4445_5441_494c_4131;
const DETAIL_SEED_B: u64 = 0x4445_5441_494c_4232;
const FEATURE_SEED: u64 = 0x4654_5253;
const CANOPY_SEED: u64 = 0x4341_4e4f_5059_4e4f;
const BUSH_SEED: u64 = 0x4255_5348_434c_5553;

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
// Eight is the measured maximum across the four representative seeds and the
// complete finite envelope; the fixed array avoids one allocation per chunk.
const MAX_CHUNK_RIVERS: usize = 8;
const EMPTY_SEGMENT: RiverSegment = RiverSegment {
    ax: 0,
    ay: 0,
    bx: 0,
    by: 0,
    channel_id: 0,
    half_width: 0,
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
                assert!(
                    context.river_len < MAX_CHUNK_RIVERS,
                    "a chunk exceeded the proved regional river-segment bound"
                );
                context.rivers[context.river_len] = *segment;
                context.river_len += 1;
            }
        }
        context
    }

    fn interpolate(&self, values: &[i32; 9], local_x: i64, local_y: i64) -> i64 {
        let cell_x = (local_x / NODE_STEP) as usize;
        let cell_y = (local_y / NODE_STEP) as usize;
        let fx = local_x % NODE_STEP;
        let fy = local_y % NODE_STEP;
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

        // Local detail is the last tier: its amplitude comes from the regional
        // roughness budget and is damped near sea level so coasts stay ragged
        // without dissolving into speckle.
        let detail = (centered_noise(self.seed ^ DETAIL_SEED_A, x, y, 160) * 5
            + centered_noise(self.seed ^ DETAIL_SEED_B, x, y, 40) * 2)
            / 7;
        let coast_damp = 300 + (macro_elevation - i64::from(SEA_LEVEL)).abs().min(2_300);
        let amplitude = roughness * coast_damp / 2_600;
        let mut elevation = (macro_elevation + detail * amplitude / NOISE_HALF) as i32;

        let mut water_surface = None;
        let mut water_biome = None;
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
        for segment in &self.rivers[..self.river_len] {
            let (distance_sq, denominator) = point_segment_distance_ratio(
                x,
                y,
                i64::from(segment.ax),
                i64::from(segment.ay),
                i64::from(segment.bx),
                i64::from(segment.by),
            );
            let half_width = i64::from(segment.half_width);
            let core = half_width * 5 / 8;
            if segment.half_width >= 8 && distance_sq <= i128::from(core * core) * denominator {
                elevation = elevation.min(DEEP_WATER_FILL);
                water_surface = Some(SurfaceType::DeepWater);
                water_biome.get_or_insert(BiomeType::River);
            } else if distance_sq <= i128::from(half_width * half_width) * denominator {
                elevation = elevation.min(SHALLOW_WATER_FILL);
                if water_surface != Some(SurfaceType::DeepWater) {
                    water_surface = Some(SurfaceType::ShallowWater);
                }
                water_biome.get_or_insert(BiomeType::River);
            }
        }

        let class = water_surface.map_or_else(
            || classify(elevation, moisture, temperature),
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
        (cell, feature(self.seed, x, y, class, moisture, temperature))
    }
}

fn river_intersects_chunk(segment: RiverSegment, origin_x: i64, origin_y: i64) -> bool {
    let max_x = origin_x + CHUNK_SIZE;
    let max_y = origin_y + CHUNK_SIZE;
    i64::from(segment.ax.max(segment.bx)).saturating_add(i64::from(segment.half_width)) >= origin_x
        && i64::from(segment.ax.min(segment.bx)).saturating_sub(i64::from(segment.half_width))
            < max_x
        && i64::from(segment.ay.max(segment.by)).saturating_add(i64::from(segment.half_width))
            >= origin_y
        && i64::from(segment.ay.min(segment.by)).saturating_sub(i64::from(segment.half_width))
            < max_y
}

fn classify(elevation: i32, moisture: i32, temperature: i32) -> TerrainClass {
    if elevation <= DEEP_WATER_MAX {
        TerrainClass::new(SurfaceType::DeepWater, BiomeType::Ocean)
    } else if elevation <= SEA_LEVEL {
        TerrainClass::new(SurfaceType::ShallowWater, BiomeType::Ocean)
    } else if elevation <= BEACH_MAX {
        TerrainClass::new(SurfaceType::Sand, BiomeType::Beach)
    } else if temperature < SNOW_TEMPERATURE_MAX {
        TerrainClass::new(SurfaceType::SnowIce, BiomeType::Tundra)
    } else if elevation > ROCK_MIN {
        let surface = if temperature < MOUNTAIN_SNOW_TEMPERATURE_MAX {
            SurfaceType::SnowIce
        } else {
            SurfaceType::Rock
        };
        TerrainClass::new(surface, BiomeType::Alpine)
    } else if elevation > HILL_MIN {
        let surface = if temperature < MOUNTAIN_SNOW_TEMPERATURE_MAX {
            SurfaceType::SnowIce
        } else {
            SurfaceType::Hill
        };
        TerrainClass::new(surface, BiomeType::Alpine)
    } else if temperature < TUNDRA_TEMPERATURE_MAX {
        TerrainClass::new(SurfaceType::Soil, BiomeType::Tundra)
    } else if moisture < DESERT_MOISTURE_MAX && temperature > DESERT_TEMPERATURE_MIN {
        TerrainClass::new(SurfaceType::Sand, BiomeType::Desert)
    } else if moisture > WETLAND_MOISTURE_MIN && elevation <= WETLAND_ELEVATION_MAX {
        TerrainClass::new(SurfaceType::Soil, BiomeType::Wetland)
    } else if moisture > FOREST_MOISTURE_MIN && temperature > FOREST_TEMPERATURE_MIN {
        TerrainClass::new(SurfaceType::Soil, BiomeType::Forest)
    } else if moisture < SAVANNA_MOISTURE_MAX && temperature > DESERT_TEMPERATURE_MIN {
        TerrainClass::new(SurfaceType::Soil, BiomeType::Savanna)
    } else {
        TerrainClass::new(SurfaceType::Soil, BiomeType::Grassland)
    }
}

fn feature(
    seed: u64,
    x: i64,
    y: i64,
    class: TerrainClass,
    moisture: i32,
    temperature: i32,
) -> Option<FeatureKind> {
    if matches!(
        class.surface(),
        SurfaceType::DeepWater
            | SurfaceType::ShallowWater
            | SurfaceType::Sand
            | SurfaceType::SnowIce
    ) {
        return None;
    }
    let roll = (hash(seed ^ FEATURE_SEED, x, y) % 10_000) as i64;
    match (class.surface(), class.biome()) {
        (SurfaceType::Soil, BiomeType::Forest) => {
            // Canopy noise opens clearings instead of uniform tree spam.
            let canopy = value_noise(seed ^ CANOPY_SEED, x, y, 176);
            (canopy > 16_000 && roll < 640).then_some(FeatureKind::Tree)
        }
        (SurfaceType::Soil, BiomeType::Grassland)
            if moisture > 30_000 && temperature > FOREST_TEMPERATURE_MIN =>
        {
            (roll < 55).then_some(FeatureKind::Tree)
        }
        (SurfaceType::Soil, BiomeType::Grassland | BiomeType::Savanna) if moisture > 18_000 => {
            (roll < 70 && value_noise(seed ^ BUSH_SEED, x, y, 96) > 39_000)
                .then_some(FeatureKind::BerryBush)
        }
        (SurfaceType::Hill | SurfaceType::Rock, _) => (roll < 170).then_some(FeatureKind::Rock),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::climate::{moisture as moisture_field, temperature as temperature_field};
    use super::plates::macro_sample;
    use super::*;
    use rayon::ThreadPoolBuilder;
    use std::cmp::Reverse;
    use std::collections::BTreeSet;
    use std::sync::{Arc as SyncArc, Barrier};

    /// Overview lattice used by the structural checks: `SIDE` x `SIDE` samples
    /// spaced `STEP` cells apart (a 65,536-cell-wide window).
    const STEP: i64 = 256;
    const SIDE: usize = 256;
    const PROBE_SEED: u64 = 1;

    fn sample_grid(predicate: impl Fn(i64, i64) -> bool) -> Vec<bool> {
        let mut cells = vec![false; SIDE * SIDE];
        for j in 0..SIDE {
            for i in 0..SIDE {
                cells[j * SIDE + i] = predicate(
                    crate::world::WORLD_GENERATION_BOUNDS.min.x + i as i64 * STEP,
                    crate::world::WORLD_GENERATION_BOUNDS.min.y + j as i64 * STEP,
                );
            }
        }
        cells
    }

    /// Sizes of 4-connected true components, largest first.
    fn component_sizes(cells: &[bool], side: usize) -> Vec<usize> {
        component_bounds(cells, side)
            .into_iter()
            .map(|(size, _, _)| size)
            .collect()
    }

    /// (size, bbox width, bbox height) of 4-connected components, largest first.
    fn component_bounds(cells: &[bool], side: usize) -> Vec<(usize, usize, usize)> {
        let mut visited = vec![false; cells.len()];
        let mut results = Vec::new();
        for start in 0..cells.len() {
            if !cells[start] || visited[start] {
                continue;
            }
            visited[start] = true;
            let mut stack = vec![start];
            let (mut min_x, mut max_x) = (start % side, start % side);
            let (mut min_y, mut max_y) = (start / side, start / side);
            let mut size = 0;
            while let Some(index) = stack.pop() {
                size += 1;
                let x = index % side;
                let y = index / side;
                min_x = min_x.min(x);
                max_x = max_x.max(x);
                min_y = min_y.min(y);
                max_y = max_y.max(y);
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
            results.push((size, max_x - min_x + 1, max_y - min_y + 1));
        }
        results.sort_unstable_by_key(|entry| Reverse(entry.0));
        results
    }

    #[test]
    fn continents_have_multiscale_size_distribution() {
        let land = sample_grid(|x, y| macro_sample(PROBE_SEED, x, y).elevation > SEA_LEVEL);
        let total = land.len();
        let land_count = land.iter().filter(|&&cell| cell).count();
        let land_fraction = land_count as f64 / total as f64;
        assert!(
            (0.2..=0.65).contains(&land_fraction),
            "land fraction {land_fraction} out of range"
        );

        let water: Vec<bool> = land.iter().map(|&cell| !cell).collect();
        let ocean_components = component_sizes(&water, SIDE);
        assert!(
            ocean_components[0] * 2 >= total - land_count,
            "no dominant open ocean: largest {} of {}",
            ocean_components[0],
            total - land_count
        );

        let land_components = component_sizes(&land, SIDE);
        assert!(
            land_components.len() >= 3,
            "expected several landmasses, found {}",
            land_components.len()
        );
        assert!(
            land_components[0] * 5 >= land_count,
            "largest landmass too small: {} of {land_count}",
            land_components[0]
        );
        let median = land_components[land_components.len() / 2];
        assert!(
            land_components[0] >= median * 5,
            "landmass sizes too uniform: largest {} median {median}",
            land_components[0]
        );
    }

    #[test]
    fn mountains_form_elongated_connected_ranges() {
        let high = sample_grid(|x, y| macro_sample(PROBE_SEED, x, y).elevation > 50_000);
        let land_count = sample_grid(|x, y| macro_sample(PROBE_SEED, x, y).elevation > SEA_LEVEL)
            .iter()
            .filter(|&&cell| cell)
            .count();
        let high_count = high.iter().filter(|&&cell| cell).count();
        let high_fraction = high_count as f64 / land_count as f64;
        assert!(
            (0.002..=0.15).contains(&high_fraction),
            "mountain fraction {high_fraction} out of range"
        );

        let ranges = component_bounds(&high, SIDE);
        let (size, width, height) = ranges[0];
        let span = width.max(height);
        assert!(
            span as i64 * STEP >= 2_500,
            "largest range spans only {} cells",
            span as i64 * STEP
        );
        // Ranges are arcs and ridge lines, not filled discs: a disc of area
        // `size` has span^2 about 1.3x its area, an arc far more.
        assert!(
            span * span >= size * 3,
            "largest range is a blob: {size} cells in {width}x{height}"
        );
    }

    #[test]
    fn climate_produces_coherent_biome_regions() {
        let mut desert = vec![false; SIDE * SIDE];
        let mut forest = vec![false; SIDE * SIDE];
        let mut grass = 0_usize;
        let mut land = 0_usize;
        for j in 0..SIDE {
            for i in 0..SIDE {
                let x = crate::world::WORLD_GENERATION_BOUNDS.min.x + i as i64 * STEP;
                let y = crate::world::WORLD_GENERATION_BOUNDS.min.y + j as i64 * STEP;
                let elevation = macro_sample(PROBE_SEED, x, y).elevation;
                if elevation <= SEA_LEVEL {
                    continue;
                }
                land += 1;
                let temperature = temperature_field(PROBE_SEED, x, y, elevation);
                let moisture = moisture_field(PROBE_SEED, x, y, elevation);
                match classify(elevation, moisture, temperature).biome() {
                    BiomeType::Desert => desert[j * SIDE + i] = true,
                    BiomeType::Forest => forest[j * SIDE + i] = true,
                    BiomeType::Grassland | BiomeType::Savanna => grass += 1,
                    _ => {}
                }
            }
        }
        let desert_count = desert.iter().filter(|&&cell| cell).count();
        let forest_count = forest.iter().filter(|&&cell| cell).count();
        assert!(
            grass * 50 >= land,
            "grasslands nearly absent: {grass} of {land}"
        );
        assert!(
            desert_count * 100 >= land,
            "deserts nearly absent: {desert_count} of {land}"
        );
        assert!(
            forest_count * 100 >= land * 3,
            "forests nearly absent: {forest_count} of {land}"
        );
        // Biomes form contiguous regions, not speckle: the largest patches
        // must be much larger than single samples.
        assert!(component_sizes(&desert, SIDE)[0] >= 30);
        assert!(component_sizes(&forest, SIDE)[0] >= 30);
    }

    #[test]
    fn terrain_semantics_separate_equal_surfaces_by_environment() {
        let beach = classify(32_000, 8_000, 38_000);
        let desert = classify(38_000, 8_000, 38_000);
        assert_eq!(beach.surface(), SurfaceType::Sand);
        assert_eq!(desert.surface(), SurfaceType::Sand);
        assert_eq!(beach.biome(), BiomeType::Beach);
        assert_eq!(desert.biome(), BiomeType::Desert);

        let grassland = classify(38_000, 30_000, 22_000);
        let wetland = classify(38_000, 52_000, 22_000);
        assert_eq!(grassland.surface(), SurfaceType::Soil);
        assert_eq!(wetland.surface(), SurfaceType::Soil);
        assert_eq!(grassland.biome(), BiomeType::Grassland);
        assert_eq!(wetland.biome(), BiomeType::Wetland);

        let cold_lowland = classify(38_000, 24_000, 5_000);
        let cold_mountain = classify(58_000, 24_000, 7_500);
        assert_eq!(cold_lowland.surface(), SurfaceType::SnowIce);
        assert_eq!(cold_lowland.biome(), BiomeType::Tundra);
        assert_eq!(cold_mountain.surface(), SurfaceType::SnowIce);
        assert_eq!(cold_mountain.biome(), BiomeType::Alpine);
    }

    #[test]
    fn complete_envelope_contains_cold_temperate_and_warm_lowlands() {
        const COLD_MAX: i32 = 15_000;
        const WARM_MIN: i32 = 30_000;

        let mut representative_cold_hemispheres = [0_usize; 2];
        for seed in [1, 7, 42, 10_001] {
            let mut zones = [0_usize; 3];
            for j in 0..SIDE {
                for i in 0..SIDE {
                    let x = crate::world::WORLD_GENERATION_BOUNDS.min.x + i as i64 * STEP;
                    let y = crate::world::WORLD_GENERATION_BOUNDS.min.y + j as i64 * STEP;
                    let elevation = macro_sample(seed, x, y).elevation;
                    if !(SEA_LEVEL + 500..=42_000).contains(&elevation) {
                        continue;
                    }
                    let temperature = temperature_field(seed, x, y, elevation);
                    let zone = if temperature < COLD_MAX {
                        representative_cold_hemispheres[usize::from(y >= 0)] += 1;
                        0
                    } else if temperature >= WARM_MIN {
                        2
                    } else {
                        1
                    };
                    zones[zone] += 1;
                }
            }
            let total: usize = zones.iter().sum();
            assert!(total > 1_000, "seed {seed} has too few sampled lowlands");
            for (name, count) in ["cold", "temperate", "warm"].into_iter().zip(zones) {
                assert!(
                    count * 100 >= total * 3,
                    "seed {seed} {name} lowlands cover only {count} of {total} samples"
                );
            }
        }
        assert!(
            representative_cold_hemispheres
                .into_iter()
                .all(|count| count > 0),
            "representative seeds lack cold lowlands in one hemisphere"
        );
    }

    #[test]
    fn climate_inspection_matches_chunk_classification_inputs() {
        let seed = 42;
        for (x, y) in [
            (-32_768_i64, -32_768_i64),
            (-16_385, 7_999),
            (-1, -1),
            (0, 0),
            (16_384, -8_001),
            (32_767, 32_767),
        ] {
            let origin_x = x.div_euclid(CHUNK_SIZE) * CHUNK_SIZE;
            let origin_y = y.div_euclid(CHUNK_SIZE) * CHUNK_SIZE;
            let context = ChunkContext::new(seed, origin_x, origin_y);
            let (cell, _) = context.generate(x, y);
            let expected_temperature = context.interpolate(
                &context.temperature,
                x - context.origin_x,
                y - context.origin_y,
            ) as u16;
            let inspected = climate_at(seed, x, y, cell.moisture);

            assert_eq!(inspected.temperature, expected_temperature);
            assert_eq!(inspected.moisture, cell.moisture);
            assert_eq!(inspected, climate_at(seed, x, y, cell.moisture));
        }
    }

    #[test]
    fn adjacent_regions_share_canonical_water_and_crossing_channels() {
        let mut crossing_channels = 0;
        let mut wet_seam_nodes = 0;
        for seed in [PROBE_SEED, 42] {
            for seam_region in [-1_i64, 0, 1] {
                let left = RegionMap::build(seed, seam_region - 1, 0);
                let right = RegionMap::build(seed, seam_region, 0);
                let seam_x = seam_region * REGION_SIZE;
                for j in 0..GRID {
                    let left_depth = left.water_depth[j * GRID + GRID - 1];
                    let right_depth = right.water_depth[j * GRID];
                    assert_eq!(left_depth, right_depth);
                    wet_seam_nodes += usize::from(left_depth >= LAKE_MIN_DEPTH);
                }
                for segment in left.rivers.iter().filter(|segment| {
                    i64::from(segment.ax.min(segment.bx)) < seam_x
                        && i64::from(segment.ax.max(segment.bx)) >= seam_x
                }) {
                    assert!(right.rivers.contains(segment));
                    crossing_channels += 1;
                }

                let top = RegionMap::build(seed, 0, seam_region - 1);
                let bottom = RegionMap::build(seed, 0, seam_region);
                let seam_y = seam_region * REGION_SIZE;
                for i in 0..GRID {
                    let top_depth = top.water_depth[(GRID - 1) * GRID + i];
                    let bottom_depth = bottom.water_depth[i];
                    assert_eq!(top_depth, bottom_depth);
                    wet_seam_nodes += usize::from(top_depth >= LAKE_MIN_DEPTH);
                }
                for segment in top.rivers.iter().filter(|segment| {
                    i64::from(segment.ay.min(segment.by)) < seam_y
                        && i64::from(segment.ay.max(segment.by)) >= seam_y
                }) {
                    assert!(bottom.rivers.contains(segment));
                    crossing_channels += 1;
                }
            }
        }
        assert!(
            crossing_channels > 0,
            "canonical channels never crossed the tested region seams"
        );
        assert!(
            wet_seam_nodes > 0,
            "canonical lakes never reached the tested region seams"
        );
    }

    #[test]
    fn flooded_lattice_nodes_render_as_water_without_surface_features() {
        let mut checked = 0;
        for seed in [PROBE_SEED, 42] {
            for region_y in -1..=1_i64 {
                for region_x in -1..=1_i64 {
                    let map = RegionMap::build(seed, region_x, region_y);
                    for (node, &depth) in map.water_depth.iter().enumerate() {
                        if depth < LAKE_MIN_DEPTH || checked >= 48 {
                            continue;
                        }
                        let x = region_x * REGION_SIZE + (node % GRID) as i64 * NODE_STEP;
                        let y = region_y * REGION_SIZE + (node / GRID) as i64 * NODE_STEP;
                        let origin_x = x.div_euclid(CHUNK_SIZE) * CHUNK_SIZE;
                        let origin_y = y.div_euclid(CHUNK_SIZE) * CHUNK_SIZE;
                        let context = ChunkContext::new(seed, origin_x, origin_y);
                        let (cell, feature) = context.generate(x, y);
                        assert!(matches!(
                            cell.surface(),
                            SurfaceType::DeepWater | SurfaceType::ShallowWater
                        ));
                        assert_eq!(cell.biome(), BiomeType::Lake);
                        assert!(feature.is_none(), "water node {node} emitted a feature");
                        checked += 1;
                    }
                }
            }
        }
        assert!(
            checked >= 16,
            "expected representative flooded lattice nodes"
        );
    }

    #[test]
    fn river_core_wins_over_overlapping_bank_in_any_segment_order() {
        let core = RiverSegment {
            ax: 0,
            ay: 16,
            bx: 32,
            by: 16,
            channel_id: 1,
            half_width: 8,
        };
        let bank = RiverSegment {
            ax: 23,
            ay: 0,
            bx: 23,
            by: 32,
            channel_id: 2,
            half_width: 8,
        };

        for ordered in [[core, bank], [bank, core]] {
            let mut rivers = [EMPTY_SEGMENT; MAX_CHUNK_RIVERS];
            rivers[..ordered.len()].copy_from_slice(&ordered);
            let context = ChunkContext {
                seed: PROBE_SEED,
                origin_x: 0,
                origin_y: 0,
                elevation: [40_000; 9],
                water_depth: [0; 9],
                temperature: [20_000; 9],
                moisture: [20_000; 9],
                roughness: [0; 9],
                rivers,
                river_len: ordered.len(),
            };

            let (cell, feature) = context.generate(16, 16);
            assert_eq!(cell.surface(), SurfaceType::DeepWater);
            assert_eq!(cell.biome(), BiomeType::River);
            assert_eq!(cell.elevation, DEEP_WATER_FILL as u16);
            assert!(feature.is_none());
        }
    }

    #[test]
    fn chunk_river_index_retains_every_intersecting_segment() {
        for (seed, region_x, region_y) in [(1, -1, -1), (7, 0, 0), (42, 1, -1)] {
            let map = RegionMap::build(seed, region_x, region_y);
            let mut coords = BTreeSet::new();
            for segment in &map.rivers {
                let min_x = i64::from(segment.ax.min(segment.bx))
                    .saturating_sub(i64::from(segment.half_width))
                    .div_euclid(CHUNK_SIZE);
                let max_x = i64::from(segment.ax.max(segment.bx))
                    .saturating_add(i64::from(segment.half_width))
                    .div_euclid(CHUNK_SIZE);
                let min_y = i64::from(segment.ay.min(segment.by))
                    .saturating_sub(i64::from(segment.half_width))
                    .div_euclid(CHUNK_SIZE);
                let max_y = i64::from(segment.ay.max(segment.by))
                    .saturating_add(i64::from(segment.half_width))
                    .div_euclid(CHUNK_SIZE);
                for chunk_y in min_y..=max_y {
                    for chunk_x in min_x..=max_x {
                        coords.insert((chunk_x, chunk_y));
                    }
                }
            }

            for (chunk_x, chunk_y) in coords {
                let origin_x = chunk_x * CHUNK_SIZE;
                let origin_y = chunk_y * CHUNK_SIZE;
                if origin_x.div_euclid(REGION_SIZE) != region_x
                    || origin_y.div_euclid(REGION_SIZE) != region_y
                {
                    continue;
                }
                let expected: Vec<_> = map
                    .rivers
                    .iter()
                    .copied()
                    .filter(|segment| river_intersects_chunk(*segment, origin_x, origin_y))
                    .collect();
                assert!(expected.len() <= MAX_CHUNK_RIVERS);
                let context = ChunkContext::new(seed, origin_x, origin_y);
                assert_eq!(
                    &context.rivers[..context.river_len],
                    expected.as_slice(),
                    "river index lost a segment in region {region_x},{region_y} chunk {chunk_x},{chunk_y}"
                );
            }
        }
    }

    #[test]
    fn clearing_region_cache_does_not_change_chunk_output() {
        use crate::{ChunkCoord, World};

        let seed = 73;
        let target = ChunkCoord { x: -1, y: -1 };
        lock_region_cache().clear();
        let expected = World::generate_chunk_at(seed, target).expect("target is representable");
        lock_region_cache().clear();
        let regenerated = World::generate_chunk_at(seed, target).expect("target is representable");
        assert_eq!(regenerated, expected);
    }

    #[test]
    fn cross_region_chunks_are_order_and_worker_count_independent() {
        use crate::{ChunkCoord, World};

        let seed = 0x4352_4f53_5352_4547;
        let coords = [
            ChunkCoord { x: -65, y: -1 },
            ChunkCoord { x: -64, y: -1 },
            ChunkCoord { x: -1, y: -65 },
            ChunkCoord { x: -1, y: -64 },
            ChunkCoord { x: 63, y: 0 },
            ChunkCoord { x: 64, y: 0 },
            ChunkCoord { x: 0, y: 63 },
            ChunkCoord { x: 0, y: 64 },
        ];
        let generate = |workers, ordered: Vec<ChunkCoord>| {
            ThreadPoolBuilder::new()
                .num_threads(workers)
                .build()
                .unwrap()
                .install(|| {
                    let mut chunks: Vec<_> = ordered
                        .into_par_iter()
                        .map(|coord| {
                            (
                                coord,
                                World::generate_chunk_at(seed, coord)
                                    .expect("test coordinate is inside the finite world"),
                            )
                        })
                        .collect();
                    chunks.sort_unstable_by_key(|(coord, _)| (coord.x, coord.y));
                    chunks
                })
        };

        let single = generate(1, coords.to_vec());
        let mut reversed = coords.to_vec();
        reversed.reverse();
        let parallel = generate(4, reversed);
        assert_eq!(parallel, single);
    }

    #[test]
    fn concurrent_region_requests_share_one_build() {
        const WORKERS: usize = 8;
        let seed = 0x5348_4152_4544_4341;
        lock_region_cache().clear();
        let barrier = SyncArc::new(Barrier::new(WORKERS));
        let handles: Vec<_> = (0..WORKERS)
            .map(|_| {
                let barrier = SyncArc::clone(&barrier);
                std::thread::spawn(move || {
                    barrier.wait();
                    region(seed, 3, -4)
                })
            })
            .collect();
        let maps: Vec<_> = handles
            .into_iter()
            .map(|handle| handle.join().expect("region requester must not panic"))
            .collect();

        assert!(
            maps.iter().all(|map| Arc::ptr_eq(map, &maps[0])),
            "concurrent requests for one region must share its materialization"
        );
    }

    #[test]
    fn regional_parallelism_preserves_exact_output_across_pool_sizes() {
        let build = |workers| {
            ThreadPoolBuilder::new()
                .num_threads(workers)
                .build()
                .unwrap()
                .install(|| RegionMap::build(0x5041_5241_4c4c_454c, -7, 7))
        };
        let single = build(1);
        let parallel = build(4);

        assert_eq!(parallel.elevation, single.elevation);
        assert_eq!(parallel.water_depth, single.water_depth);
        assert_eq!(parallel.temperature, single.temperature);
        assert_eq!(parallel.moisture, single.moisture);
        assert_eq!(parallel.roughness, single.roughness);
        assert_eq!(parallel.rivers, single.rivers);
    }

    #[test]
    fn feature_density_varies_regionally() {
        // Sixteen 256x256 blocks spread across a 32k window: biome-driven
        // vegetation must cluster instead of scattering uniformly.
        let mut counts = [0_u32; 16];
        for (block, count) in counts.iter_mut().enumerate() {
            let base_x = (block % 4) as i64 * 16_384 - 30_720;
            let base_y = (block / 4) as i64 * 16_384 - 30_720;
            for chunk_y in 0..4 {
                for chunk_x in 0..4 {
                    let origin_x = base_x + chunk_x * crate::world::CHUNK_SIZE;
                    let origin_y = base_y + chunk_y * crate::world::CHUNK_SIZE;
                    let context = ChunkContext::new(PROBE_SEED, origin_x, origin_y);
                    for y in origin_y..origin_y + crate::world::CHUNK_SIZE {
                        for x in origin_x..origin_x + crate::world::CHUNK_SIZE {
                            if context.generate(x, y).1.is_some() {
                                *count += 1;
                            }
                        }
                    }
                }
            }
        }
        let max = *counts.iter().max().unwrap();
        let min = *counts.iter().min().unwrap();
        assert!(max > 0, "no features generated in any probe block");
        assert!(
            max >= min * 2 + 16,
            "feature density is uniform: min {min} max {max}"
        );
    }
}
