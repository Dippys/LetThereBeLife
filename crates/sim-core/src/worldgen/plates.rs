//! Tectonic-plate structure: the global source of continents, oceans,
//! mountain ranges, plateaus, rifts, and per-region terrain character.
//!
//! Plates are jittered Voronoi cells (`PLATE_SIZE` spacing). A very-low
//! frequency supercontinent field decides which plates are continental, so
//! land clusters into a few large masses with genuinely open oceans between
//! them. Coastlines are the domain-warped bisectors between continental and
//! oceanic plates; mountain ranges follow convergent plate boundaries, which
//! makes them long connected arcs instead of isolated noise peaks.

use std::cell::RefCell;
use std::collections::HashMap;

use super::noise::{NOISE_HALF, NOISE_MAX, centered_noise, hash, value_noise, warp};

const PLATE_SIZE: i64 = 8_192;
pub(crate) const SEA_LEVEL: i32 = 30_000;

const SUPERCONTINENT_SCALE: i64 = 26_624;
/// `(supercontinent * 5 + per-plate bias * 3) / 8` must exceed this for land.
const CONTINENTAL_THRESHOLD: i64 = 36_500;
const BEACH_RAMP: i64 = 140;
const COASTAL_PLAIN_END: i64 = 2_600;
const INTERIOR_BASE: i64 = 38_000;
const SHELF_RAMP: i64 = 260;
const RIDGE_CORE_FALLOFF: i64 = 2_800;
const PLATEAU_FALLOFF: i64 = 6_500;
const RIFT_FALLOFF: i64 = 1_200;
const DISTANCE_CLAMP: i64 = 64_000;

const PLATE_SITE_SEED: u64 = 0x504c_4154_4553_4954;
const PLATE_ATTR_SEED: u64 = 0x504c_4154_4541_5454;
const SUPERCONTINENT_SEED: u64 = 0x5355_5045_5243_4f4e;
const RIDGE_SEED: u64 = 0x5249_4447_454e_4f49;
const RELIEF_SEED: u64 = 0x5245_4c49_4546_4d41;
const BASIN_SEED: u64 = 0x4241_5349_4e44_4550;
const ROUGH_SEED: u64 = 0x524f_5547_484e_4f49;

#[derive(Debug, Clone, Copy)]
struct PlateSite {
    x: i64,
    y: i64,
    continental: bool,
    /// Interior uplift a continental plate carries above the common base.
    plateau: i64,
    /// Extra abyssal depth of an oceanic plate.
    depth: i64,
    /// Plate motion, each component in `-64..=63`.
    vx: i64,
    vy: i64,
    /// Baseline amplitude for local detail noise on this plate.
    rough: i64,
}

fn plate_site(seed: u64, cell_x: i64, cell_y: i64) -> PlateSite {
    let bits = hash(seed ^ PLATE_SITE_SEED, cell_x, cell_y);
    let span = PLATE_SIZE * 3 / 4;
    let x = cell_x
        .saturating_mul(PLATE_SIZE)
        .saturating_add(PLATE_SIZE / 8 + (bits & 0xFFFF) as i64 * span / 65_536);
    let y = cell_y
        .saturating_mul(PLATE_SIZE)
        .saturating_add(PLATE_SIZE / 8 + ((bits >> 16) & 0xFFFF) as i64 * span / 65_536);
    let attrs = hash(seed ^ PLATE_ATTR_SEED, cell_x, cell_y);
    let supercontinent = value_noise(seed ^ SUPERCONTINENT_SEED, x, y, SUPERCONTINENT_SCALE);
    let bias = (attrs & 0xFFFF) as i64;
    PlateSite {
        x,
        y,
        continental: (supercontinent * 5 + bias * 3) / 8 > CONTINENTAL_THRESHOLD,
        plateau: 2_000 + ((attrs >> 16) & 0x3FFF) as i64 % 7_000,
        depth: ((attrs >> 30) & 0x1FFF) as i64 % 6_000,
        vx: ((attrs >> 43) & 0x7F) as i64 - 64,
        vy: ((attrs >> 50) & 0x7F) as i64 - 64,
        rough: 250 + ((bits >> 32) & 0xFFFF) as i64 % 2_200,
    }
}

thread_local! {
    static SITE_CACHE: RefCell<HashMap<(u64, i64, i64), PlateSite>> =
        RefCell::new(HashMap::new());
}
const SITE_CACHE_CAPACITY: usize = 8_192;

fn plate_site_cached(seed: u64, cell_x: i64, cell_y: i64) -> PlateSite {
    SITE_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some(site) = cache.get(&(seed, cell_x, cell_y)) {
            return *site;
        }
        let site = plate_site(seed, cell_x, cell_y);
        if cache.len() >= SITE_CACHE_CAPACITY {
            cache.clear();
        }
        cache.insert((seed, cell_x, cell_y), site);
        site
    })
}

fn distance_sq(site: PlateSite, x: i64, y: i64) -> i64 {
    let dx = (site.x - x).clamp(-DISTANCE_CLAMP, DISTANCE_CLAMP);
    let dy = (site.y - y).clamp(-DISTANCE_CLAMP, DISTANCE_CLAMP);
    dx * dx + dy * dy
}

/// Distance from the sample to the bisector between the owning site and
/// `other`; this is the exact distance to that plate boundary.
fn boundary_distance(own_d_sq: i64, other_d_sq: i64, own: PlateSite, other: PlateSite) -> i64 {
    let dx = (other.x - own.x).clamp(-DISTANCE_CLAMP, DISTANCE_CLAMP);
    let dy = (other.y - own.y).clamp(-DISTANCE_CLAMP, DISTANCE_CLAMP);
    let separation = (dx * dx + dy * dy).isqrt().max(1);
    ((other_d_sq - own_d_sq) / (2 * separation)).max(0)
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct MacroSample {
    /// Regional-scale elevation before local detail noise.
    pub(crate) elevation: i32,
    /// Amplitude budget for local detail noise at this location.
    pub(crate) roughness: i32,
}

pub(crate) fn macro_sample(seed: u64, x: i64, y: i64) -> MacroSample {
    let (wx, wy) = warp(seed, x, y);
    let cell_x = wx.div_euclid(PLATE_SIZE);
    let cell_y = wy.div_euclid(PLATE_SIZE);

    let mut sites = [(0_i64, plate_site_cached(seed, cell_x, cell_y)); 25];
    let mut index = 0;
    for offset_y in -2..=2 {
        for offset_x in -2..=2 {
            let site = plate_site_cached(seed, cell_x + offset_x, cell_y + offset_y);
            sites[index] = (distance_sq(site, wx, wy), site);
            index += 1;
        }
    }
    let (own_index, &(own_d_sq, own)) = sites
        .iter()
        .enumerate()
        .min_by_key(|(index, (distance, _))| (*distance, *index))
        .expect("site neighborhood is non-empty");
    let &(edge_d_sq, edge_site) = sites
        .iter()
        .enumerate()
        .filter(|(index, _)| *index != own_index)
        .map(|(_, entry)| entry)
        .min_by_key(|(distance, _)| *distance)
        .expect("site neighborhood has more than one site");
    let opposite = sites
        .iter()
        .filter(|(_, site)| site.continental != own.continental)
        .min_by_key(|(distance, _)| *distance)
        .copied();

    // Signed distance into the owning plate from the nearest coastline.
    let coast_d = match opposite {
        Some((opposite_d_sq, opposite_site)) => {
            boundary_distance(own_d_sq, opposite_d_sq, own, opposite_site)
        }
        None => 2 * PLATE_SIZE,
    };
    let edge_d = boundary_distance(own_d_sq, edge_d_sq, own, edge_site);

    let base = if own.continental {
        if coast_d < BEACH_RAMP {
            i64::from(SEA_LEVEL) + coast_d * 3_000 / BEACH_RAMP
        } else if coast_d < COASTAL_PLAIN_END {
            33_000
                + (coast_d - BEACH_RAMP) * (INTERIOR_BASE - 33_000)
                    / (COASTAL_PLAIN_END - BEACH_RAMP)
        } else {
            INTERIOR_BASE + (coast_d - COASTAL_PLAIN_END).min(4_000) * own.plateau / 4_000
        }
    } else if coast_d < SHELF_RAMP {
        i64::from(SEA_LEVEL) - coast_d * 5_000 / SHELF_RAMP
    } else {
        (25_000 - (coast_d - SHELF_RAMP) * 9 - own.depth).max(3_500)
    };

    // Convergent boundaries raise ranges; divergent oceanic boundaries rift.
    let boundary_dx = (edge_site.x - own.x).clamp(-DISTANCE_CLAMP, DISTANCE_CLAMP);
    let boundary_dy = (edge_site.y - own.y).clamp(-DISTANCE_CLAMP, DISTANCE_CLAMP);
    let separation = (boundary_dx * boundary_dx + boundary_dy * boundary_dy)
        .isqrt()
        .max(1);
    let convergence = ((own.vx - edge_site.vx) * boundary_dx
        + (own.vy - edge_site.vy) * boundary_dy)
        / separation;
    let mut uplift = 0;
    if convergence > 4 {
        let strength = (convergence - 4).min(92) * 26_000 / 92;
        let ridge = value_noise(seed ^ RIDGE_SEED, wx, wy, 1_500);
        let modulated = strength * (24_576 + ridge * 5 / 8) / (NOISE_MAX + 1);
        let t = (edge_d * NOISE_MAX / RIDGE_CORE_FALLOFF).min(NOISE_MAX);
        let falloff = (NOISE_MAX - t) * (NOISE_MAX - t) / NOISE_MAX;
        uplift = modulated * falloff / NOISE_MAX;
        if !own.continental {
            // Offshore side of a subduction zone: island arcs, not full ranges.
            uplift = uplift * 2 / 5;
        } else if edge_site.continental {
            // Continental collision: a broad high plateau under the ridge.
            let tp = (edge_d * NOISE_MAX / PLATEAU_FALLOFF).min(NOISE_MAX);
            uplift += strength / 4 * (NOISE_MAX - tp) / NOISE_MAX;
        }
    } else if convergence < -48 && !own.continental && !edge_site.continental {
        let t = (edge_d * NOISE_MAX / RIFT_FALLOFF).min(NOISE_MAX);
        uplift = -((-convergence - 48).min(64) * 3_000 / 64) * (NOISE_MAX - t) / NOISE_MAX;
    }

    // Gentle regional undulation so interiors are not perfectly flat ramps.
    let regional = centered_noise(seed ^ RELIEF_SEED, x, y, 4_096) * 900 / NOISE_HALF;
    // Occasional broad depressions become closed basins: the hydrology tier
    // floods them into lakes or, near coasts, brackish inland seas.
    let basin = value_noise(seed ^ BASIN_SEED, x, y, 3_072);
    let depression = (basin - 50_000).max(0) * 4_500 / 15_536;

    let rough_noise = value_noise(seed ^ ROUGH_SEED, wx, wy, 6_144);
    let mut roughness = (own.rough * 3 + rough_noise * 2_200 / (NOISE_MAX + 1)) / 4;
    roughness += (uplift.max(0) / 12).min(1_400);

    MacroSample {
        elevation: (base + uplift + regional - depression).clamp(500, 65_000) as i32,
        roughness: roughness as i32,
    }
}
