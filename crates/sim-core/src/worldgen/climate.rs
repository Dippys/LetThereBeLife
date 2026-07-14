//! Climate fields derived from latitude, altitude, upwind ocean distance,
//! and mountain rain shadows rather than free-standing noise.

use super::noise::{NOISE_HALF, centered_noise, value_noise};
use super::plates::{SEA_LEVEL, macro_sample};

/// World-space wavelength of one full cold-warm-cold latitude cycle.
const LATITUDE_PERIOD: i64 = 262_144;
/// Shifts the cycle so the default start area sits in a temperate-warm band
/// instead of on a polar node at `y = 0`.
const LATITUDE_OFFSET: i64 = 96_000;

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
    let phase = y
        .saturating_add(LATITUDE_OFFSET)
        .rem_euclid(LATITUDE_PERIOD);
    let half = LATITUDE_PERIOD / 2;
    let toward_warm = if phase < half {
        phase
    } else {
        LATITUDE_PERIOD - phase
    };
    let latitude = 6_000 + toward_warm * 40_000 / half;
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
fn wind_rays(seed: u64, x: i64, y: i64) -> [(i64, i64, i64); 2] {
    let wobble = centered_noise(seed ^ WIND_BAND_SEED, x, y, 8_192) * 6_000 / NOISE_HALF;
    let banded_y = y.saturating_add(LATITUDE_OFFSET).saturating_add(wobble);
    if banded_y.div_euclid(LATITUDE_PERIOD / 4).rem_euclid(2) == 0 {
        [(4, 3, 5), (6, 1, 6)]
    } else {
        [(-4, -3, 5), (-6, -1, 6)]
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
