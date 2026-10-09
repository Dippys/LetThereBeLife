//! Surface-feature density and ecology tests.

use super::*;

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
                let origin_x = base_x + chunk_x * crate::CHUNK_SIZE;
                let origin_y = base_y + chunk_y * crate::CHUNK_SIZE;
                let context = ChunkContext::new(PROBE_SEED, origin_x, origin_y);
                for y in origin_y..origin_y + crate::CHUNK_SIZE {
                    for x in origin_x..origin_x + crate::CHUNK_SIZE {
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

fn synthetic_feature_counts(
    class: TerrainClass,
    moisture: i32,
    temperature: i32,
    slope: i64,
    near_water: bool,
) -> [u32; 3] {
    let mut counts = [0; 3];
    for y in -256..256 {
        for x in -256..256 {
            if let Some(kind) = feature(
                PROBE_SEED,
                x,
                y,
                FeatureEnvironment {
                    class,
                    moisture,
                    temperature,
                    slope,
                    near_water,
                    ecology: local_detail(PROBE_SEED, x, y),
                },
            ) {
                counts[match kind {
                    FeatureKind::Tree => 0,
                    FeatureKind::Rock => 1,
                    FeatureKind::BerryBush => 2,
                }] += 1;
            }
        }
    }
    counts
}

#[test]
fn surface_feature_ecology_varies_by_environment_and_water_proximity() {
    let forest = synthetic_feature_counts(
        TerrainClass::new(SurfaceType::Soil, BiomeType::Forest),
        42_000,
        28_000,
        20,
        false,
    );
    let grass = synthetic_feature_counts(
        TerrainClass::new(SurfaceType::Soil, BiomeType::Grassland),
        32_000,
        28_000,
        20,
        false,
    );
    let riparian_grass = synthetic_feature_counts(
        TerrainClass::new(SurfaceType::Soil, BiomeType::Grassland),
        32_000,
        28_000,
        20,
        true,
    );
    let hill = synthetic_feature_counts(
        TerrainClass::new(SurfaceType::Hill, BiomeType::Alpine),
        20_000,
        18_000,
        140,
        false,
    );

    assert!(
        forest[0] > grass[0] * 2,
        "forest {forest:?} grass {grass:?}"
    );
    assert!(
        riparian_grass[2] > grass[2],
        "dry {grass:?} riparian {riparian_grass:?}"
    );
    assert!(grass[1] > 0, "ordinary soil never exposes stone: {grass:?}");
    assert!(hill[1] > grass[1] * 2, "hill {hill:?} grass {grass:?}");
    assert!(
        riparian_grass[2] >= 128,
        "insufficient berry access: {riparian_grass:?}"
    );
}

#[test]
fn forest_canopy_contains_deterministic_clearings_and_dense_patches() {
    let class = TerrainClass::new(SurfaceType::Soil, BiomeType::Forest);
    let mut blocks = [0_u16; 256];
    for y in 0..512 {
        for x in 0..512 {
            if feature(
                PROBE_SEED,
                x,
                y,
                FeatureEnvironment {
                    class,
                    moisture: 42_000,
                    temperature: 28_000,
                    slope: 20,
                    near_water: false,
                    ecology: local_detail(PROBE_SEED, x, y),
                },
            ) == Some(FeatureKind::Tree)
            {
                blocks[(y / 32 * 16 + x / 32) as usize] += 1;
            }
        }
    }
    assert!(
        blocks.iter().any(|&count| count <= 4),
        "no clearing: {blocks:?}"
    );
    assert!(
        blocks.iter().any(|&count| count >= 40),
        "no dense canopy: {blocks:?}"
    );

    let repeated = synthetic_feature_counts(class, 42_000, 28_000, 20, false);
    assert_eq!(
        repeated,
        synthetic_feature_counts(class, 42_000, 28_000, 20, false)
    );
}

#[test]
fn incompatible_surfaces_never_emit_features() {
    for surface in [
        SurfaceType::DeepWater,
        SurfaceType::ShallowWater,
        SurfaceType::Sand,
        SurfaceType::SnowIce,
    ] {
        let class = TerrainClass::new(surface, BiomeType::Tundra);
        for y in -32..32 {
            for x in -32..32 {
                assert_eq!(
                    feature(
                        PROBE_SEED,
                        x,
                        y,
                        FeatureEnvironment {
                            class,
                            moisture: 65_535,
                            temperature: 32_000,
                            slope: 0,
                            near_water: true,
                            ecology: local_detail(PROBE_SEED, x, y),
                        },
                    ),
                    None
                );
            }
        }
    }
}
