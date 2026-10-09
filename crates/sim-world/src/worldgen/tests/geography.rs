//! Continental, mountain, climate, and biome-classification tests.

use super::*;

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
            let x = crate::WORLD_GENERATION_BOUNDS.min.x + i as i64 * STEP;
            let y = crate::WORLD_GENERATION_BOUNDS.min.y + j as i64 * STEP;
            let elevation = macro_sample(PROBE_SEED, x, y).elevation;
            if elevation <= SEA_LEVEL {
                continue;
            }
            land += 1;
            let temperature = temperature_field(PROBE_SEED, x, y, elevation);
            let moisture = moisture_field(PROBE_SEED, x, y, elevation);
            match classify(elevation, moisture, temperature, false, false, 0).biome() {
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
    let beach = classify(32_000, 8_000, 38_000, false, false, 0);
    let desert = classify(38_000, 8_000, 38_000, false, false, 0);
    assert_eq!(beach.surface(), SurfaceType::Sand);
    assert_eq!(desert.surface(), SurfaceType::Sand);
    assert_eq!(beach.biome(), BiomeType::Beach);
    assert_eq!(desert.biome(), BiomeType::Desert);

    let grassland = classify(38_000, 30_000, 22_000, false, false, 0);
    let wetland = classify(38_000, 52_000, 22_000, true, false, 0);
    assert_eq!(grassland.surface(), SurfaceType::Soil);
    assert_eq!(wetland.surface(), SurfaceType::Soil);
    assert_eq!(grassland.biome(), BiomeType::Grassland);
    assert_eq!(wetland.biome(), BiomeType::Wetland);

    let cold_lowland = classify(38_000, 24_000, 5_000, false, false, 0);
    let cold_mountain = classify(58_000, 24_000, 7_500, false, false, 0);
    assert_eq!(cold_lowland.surface(), SurfaceType::SnowIce);
    assert_eq!(cold_lowland.biome(), BiomeType::Tundra);
    assert_eq!(cold_mountain.surface(), SurfaceType::SnowIce);
    assert_eq!(cold_mountain.biome(), BiomeType::Alpine);
}

#[test]
fn bounded_transition_offsets_refine_beaches_biomes_and_riparian_banks() {
    let expanded_beach = classify(33_400, 20_000, 22_000, false, false, 32_000);
    let contracted_beach = classify(33_400, 20_000, 22_000, false, false, -32_000);
    assert_eq!(expanded_beach.biome(), BiomeType::Beach);
    assert_ne!(contracted_beach.biome(), BiomeType::Beach);

    let forest_side = classify(38_000, 30_000, 22_000, false, false, -16_000);
    let grass_side = classify(38_000, 30_000, 22_000, false, false, 16_000);
    assert_eq!(forest_side.biome(), BiomeType::Forest);
    assert_eq!(grass_side.biome(), BiomeType::Grassland);

    let dry_ground = classify(38_000, 11_000, 38_000, false, false, 0);
    let riverbank = classify(38_000, 11_000, 38_000, false, true, 0);
    assert_eq!(dry_ground.biome(), BiomeType::Desert);
    assert_eq!(riverbank.surface(), SurfaceType::Soil);
    assert_eq!(riverbank.biome(), BiomeType::Grassland);
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
                let x = crate::WORLD_GENERATION_BOUNDS.min.x + i as i64 * STEP;
                let y = crate::WORLD_GENERATION_BOUNDS.min.y + j as i64 * STEP;
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
