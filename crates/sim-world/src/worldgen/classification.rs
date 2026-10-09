//! Per-cell terrain classification (surface and biome thresholds), local
//! detail noise, and sparse surface-feature placement.

use super::{
    noise::{NOISE_HALF, centered_noise, hash},
    plates::SEA_LEVEL,
};
use crate::{BiomeType, FeatureKind, SurfaceType, TerrainClass};

const DEEP_WATER_MAX: i32 = 25_000;
const BEACH_MAX: i32 = 33_000;
const HILL_MIN: i32 = 50_000;
const ROCK_MIN: i32 = 56_000;
const DESERT_MOISTURE_MAX: i32 = 12_000;
const DESERT_TEMPERATURE_MIN: i32 = 30_000;
const FOREST_MOISTURE_MIN: i32 = 30_000;
const FOREST_TEMPERATURE_MIN: i32 = 13_000;
const WETLAND_MOISTURE_MIN: i32 = 48_000;
const WETLAND_ELEVATION_MAX: i32 = 39_000;
const RIPARIAN_MOISTURE_MIN: i32 = 10_000;
const RIPARIAN_ELEVATION_MAX: i32 = 42_000;
const SAVANNA_MOISTURE_MAX: i32 = 24_000;
const TUNDRA_TEMPERATURE_MAX: i32 = 13_000;
const SNOW_TEMPERATURE_MAX: i32 = 6_500;
const MOUNTAIN_SNOW_TEMPERATURE_MAX: i32 = 9_000;

const DETAIL_SEED_A: u64 = 0x4445_5441_494c_4131;
const DETAIL_SEED_B: u64 = 0x4445_5441_494c_4232;
const FEATURE_SEED: u64 = 0x4654_5253;

pub(super) fn classify(
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

pub(super) fn local_detail(seed: u64, x: i64, y: i64) -> i64 {
    (centered_noise(seed ^ DETAIL_SEED_A, x, y, 160) * 5
        + centered_noise(seed ^ DETAIL_SEED_B, x, y, 40) * 2)
        / 7
}

#[derive(Clone, Copy)]
pub(super) struct FeatureEnvironment {
    pub(super) class: TerrainClass,
    pub(super) moisture: i32,
    pub(super) temperature: i32,
    pub(super) slope: i64,
    pub(super) near_water: bool,
    pub(super) ecology: i64,
}

pub(super) fn feature(
    seed: u64,
    x: i64,
    y: i64,
    environment: FeatureEnvironment,
) -> Option<FeatureKind> {
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
