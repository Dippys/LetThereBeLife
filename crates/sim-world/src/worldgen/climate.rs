//! Climate fields derived from latitude, altitude, upwind ocean distance,
//! and mountain rain shadows rather than free-standing noise.

use super::noise::{NOISE_HALF, centered_noise, value_noise};
use super::plates::{SEA_LEVEL, macro_sample};
use crate::{PrevailingWind, WORLD_HALF_EXTENT};

/// One circulation band spans one quarter of the finite north-south envelope.
const WIND_BAND_WIDTH: i64 = WORLD_HALF_EXTENT / 2;

const TEMP_VARIATION_SEED: u64 = 0x5445_4d50_5641_5249;
const MOIST_VARIATION_SEED: u64 = 0x4d4f_4953_5456_4152;

/// Upwind probes as (distance, humidity gain). Gains are the differences of
/// the target humidity-by-ocean-distance curve, so the summed contributions of
/// all ocean probes past distance `d` reproduce that curve smoothly instead of
/// stepping on the first hit; each step stays below the dither noise.
const UPWIND_PROBES: [(i64, i64); 16] = [
    (300, 1_000),
    (700, 1_230),
    (1_200, 1_470),
    (1_800, 1_720),
    (2_500, 1_960),
    (3_300, 2_210),
    (4_200, 2_450),
    (5_200, 2_700),
    (6_300, 2_950),
    (7_500, 3_190),
    (8_800, 3_440),
    (10_200, 3_680),
    (11_700, 3_930),
    (13_300, 4_170),
    (15_000, 4_910),
    (17_000, 1_490),
];
const PROBE_JITTER_SEEDS: [u64; 2] = [0x4d4f_4953_544a_4954, 0x4d4f_4953_544a_4232];

pub(crate) fn temperature(seed: u64, x: i64, y: i64, elevation: i32) -> i32 {
    // The finite envelope is one complete cold-to-warm-to-cold band. This is
    // deliberately not periodic: neither world axis currently wraps.
    let toward_warm = WORLD_HALF_EXTENT.saturating_sub(y.abs()).max(0);
    let latitude = 6_000 + toward_warm * 40_000 / WORLD_HALF_EXTENT;
    let variation = centered_noise(seed ^ TEMP_VARIATION_SEED, x, y, 16_384) * 7_000 / NOISE_HALF;
    let lapse = i64::from((elevation - SEA_LEVEL).max(0)) * 3 / 4;
    (latitude + variation - lapse).clamp(0, 65_535) as i32
}

const WIND_BAND_SEED: u64 = 0x5749_4e44_4241_4e44;

/// Prevailing wind alternates by latitude band, mimicking global circulation.
/// Each band probes two slightly diverging upwind rays (as (dx, dy, divisor)
/// steps) so humidity terraces from one ray's coastline never align. Band
/// edges are wobbled by low-frequency noise so the direction change never
/// draws a straight east-west seam.
pub(crate) fn prevailing_wind(seed: u64, x: i64, y: i64) -> PrevailingWind {
    let wobble = centered_noise(seed ^ WIND_BAND_SEED, x, y, 8_192) * 6_000 / NOISE_HALF;
    let banded_y = y.saturating_add(WORLD_HALF_EXTENT).saturating_add(wobble);
    if banded_y.div_euclid(WIND_BAND_WIDTH).rem_euclid(2) == 0 {
        PrevailingWind::Southeast
    } else {
        PrevailingWind::Northwest
    }
}

fn wind_rays(seed: u64, x: i64, y: i64) -> [(i64, i64, i64); 2] {
    match prevailing_wind(seed, x, y) {
        PrevailingWind::Southeast => [(4, 3, 5), (6, 1, 6)],
        PrevailingWind::Northwest => [(-4, -3, 5), (-6, -1, 6)],
    }
}

pub(crate) fn moisture(seed: u64, x: i64, y: i64, elevation: i32) -> i32 {
    let mut moist = 4_500_i64;
    let mut open_fetch = false;
    for ((ray_x, ray_y, divisor), jitter_seed) in
        wind_rays(seed, x, y).into_iter().zip(PROBE_JITTER_SEEDS)
    {
        // Smoothly varying, per-ray probe-distance jitter turns the residual
        // terraces into broad wavy transitions that never align between rays.
        let stretch = 870 + value_noise(seed ^ jitter_seed, x, y, 1_536) * 300 / 65_536;
        let mut barrier = 0_i64;
        for (distance, gain) in UPWIND_PROBES {
            let reach = distance * stretch / 1_000;
            let sample_x = x.saturating_sub(ray_x * reach / divisor);
            let sample_y = y.saturating_sub(ray_y * reach / divisor);
            let upwind = i64::from(macro_sample(seed, sample_x, sample_y).elevation);
            if upwind <= i64::from(SEA_LEVEL) {
                // Ocean moisture arrives attenuated by any mountain barrier
                // standing between it and this location (rain shadow).
                moist += (gain - (barrier / 6).min(gain * 3 / 4)) / 2;
            } else {
                barrier = barrier.max(upwind - 38_000);
            }
        }
        open_fetch |= barrier < 2_000;
    }
    if elevation > 38_000 && open_fetch {
        // Windward slopes with an open fetch to the ocean get orographic rain.
        moist += 3_000;
    }
    moist += centered_noise(seed ^ MOIST_VARIATION_SEED, x, y, 5_120) * 5_000 / NOISE_HALF;
    moist.clamp(500, 62_000) as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finite_latitude_band_is_cold_at_both_edges_and_warmest_at_center() {
        let elevation = SEA_LEVEL;
        let south = temperature(1, 0, -WORLD_HALF_EXTENT, elevation);
        let center = temperature(1, 0, 0, elevation);
        let north = temperature(1, 0, WORLD_HALF_EXTENT, elevation);

        assert!(center > south + 25_000);
        assert!(center > north + 25_000);
    }

    #[test]
    fn circulation_boundaries_are_not_straight_world_space_seams() {
        for boundary in [-WIND_BAND_WIDTH, 0, WIND_BAND_WIDTH] {
            let directions: Vec<_> = (-WORLD_HALF_EXTENT..WORLD_HALF_EXTENT)
                .step_by(512)
                .map(|x| prevailing_wind(7, x, boundary))
                .collect();
            assert!(directions.contains(&PrevailingWind::Southeast));
            assert!(directions.contains(&PrevailingWind::Northwest));
        }
    }
}
