//! Deterministic integer noise primitives shared by every generation field.

pub(crate) const NOISE_MAX: i64 = 65_535;
pub(crate) const NOISE_HALF: i64 = 32_768;

/// Stateless 2D coordinate hash; the only source of randomness in generation.
pub(crate) fn hash(seed: u64, x: i64, y: i64) -> u64 {
    let mut value = seed
        ^ (x as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15)
        ^ (y as u64).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

fn lattice(seed: u64, x: i64, y: i64) -> i64 {
    (hash(seed, x, y) & NOISE_MAX as u64) as i64
}

fn smooth(value: i64) -> i64 {
    let squared = value * value / NOISE_MAX;
    squared * (3 * NOISE_MAX - 2 * value) / NOISE_MAX
}

fn lerp(start: i64, end: i64, amount: i64) -> i64 {
    start + (end - start) * amount / NOISE_MAX
}

/// Smoothly interpolated lattice noise in `0..=65_535`.
pub(crate) fn value_noise(seed: u64, x: i64, y: i64, scale: i64) -> i64 {
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

/// Value noise recentered to `-32_768..=32_767`.
pub(crate) fn centered_noise(seed: u64, x: i64, y: i64, scale: i64) -> i64 {
    value_noise(seed, x, y, scale) - NOISE_HALF
}

const WARP_X1: u64 = 0x5741_5250_5831;
const WARP_X2: u64 = 0x5741_5250_5832;
const WARP_X3: u64 = 0x5741_5250_5833;
const WARP_Y1: u64 = 0x5741_5250_5931;
const WARP_Y2: u64 = 0x5741_5250_5932;
const WARP_Y3: u64 = 0x5741_5250_5933;

/// Three-octave domain warp used to bend plate boundaries into coastlines
/// with gulf-, bay-, and cove-scale irregularity.
pub(crate) fn warp(seed: u64, x: i64, y: i64) -> (i64, i64) {
    let dx = centered_noise(seed ^ WARP_X1, x, y, 8_192) * 2_600 / NOISE_HALF
        + centered_noise(seed ^ WARP_X2, x, y, 2_048) * 650 / NOISE_HALF
        + centered_noise(seed ^ WARP_X3, x, y, 512) * 150 / NOISE_HALF;
    let dy = centered_noise(seed ^ WARP_Y1, x, y, 8_192) * 2_600 / NOISE_HALF
        + centered_noise(seed ^ WARP_Y2, x, y, 2_048) * 650 / NOISE_HALF
        + centered_noise(seed ^ WARP_Y3, x, y, 512) * 150 / NOISE_HALF;
    (x.saturating_add(dx), y.saturating_add(dy))
}
