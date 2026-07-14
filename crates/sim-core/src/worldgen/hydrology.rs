//! Regional refinement of the canonical whole-world drainage skeleton.
//!
//! Each `REGION_SIZE` square retains a `GRID`x`GRID` node lattice sampled every
//! `NODE_STEP` cells. Macro terrain and climate remain regional, while lake
//! depth and major-river geometry come from one immutable seed-keyed skeleton.

use rayon::prelude::*;

use crate::world::WORLD_HALF_EXTENT;

use super::climate::{moisture, temperature};
pub(crate) use super::drainage::DrainageSegment as RiverSegment;
use super::drainage::world_drainage;
use super::plates::macro_sample;

pub(crate) const REGION_SIZE: i64 = 4_096;
pub(crate) const NODE_STEP: i64 = 32;
pub(crate) const GRID: usize = (REGION_SIZE / NODE_STEP) as usize + 1;
const NODE_COUNT: usize = GRID * GRID;

/// Water shallower than this over a node is treated as dry land.
pub(crate) const LAKE_MIN_DEPTH: i32 = 250;

/// Cached per-region lattice of macro terrain, climate, and canonical drainage.
pub(crate) struct RegionMap {
    pub(crate) elevation: Box<[i32]>,
    /// Standing water above each node, sampled from the world skeleton.
    pub(crate) water_depth: Box<[i32]>,
    pub(crate) temperature: Box<[i32]>,
    pub(crate) moisture: Box<[i32]>,
    pub(crate) roughness: Box<[i32]>,
    pub(crate) rivers: Vec<RiverSegment>,
}

impl RegionMap {
    pub(crate) fn build(seed: u64, region_x: i64, region_y: i64) -> Self {
        let half_regions = WORLD_HALF_EXTENT / REGION_SIZE;
        assert!(
            (-half_regions..half_regions).contains(&region_x)
                && (-half_regions..half_regions).contains(&region_y),
            "regional drainage coordinates must remain inside the finite world"
        );
        let origin_x = region_x * REGION_SIZE;
        let origin_y = region_y * REGION_SIZE;
        let drainage = world_drainage(seed);
        let mut elevation = vec![0_i32; NODE_COUNT];
        let mut water_depth = vec![0_i32; NODE_COUNT];
        let mut temperature_map = vec![0_i32; NODE_COUNT];
        let mut roughness = vec![0_i32; NODE_COUNT];
        elevation
            .par_iter_mut()
            .zip(water_depth.par_iter_mut())
            .zip(roughness.par_iter_mut())
            .zip(temperature_map.par_iter_mut())
            .enumerate()
            .for_each(
                |(index, (((elevation, water_depth), roughness), temperature_value))| {
                    let i = index % GRID;
                    let j = index / GRID;
                    let x = origin_x.saturating_add(i as i64 * NODE_STEP);
                    let y = origin_y.saturating_add(j as i64 * NODE_STEP);
                    let sample = macro_sample(seed, x, y);
                    *elevation = sample.elevation;
                    *water_depth = drainage.water_depth_at(x, y, sample.elevation);
                    *roughness = sample.roughness;
                    *temperature_value = temperature(seed, x, y, sample.elevation);
                },
            );
        let moisture_map = moisture_lattice(seed, origin_x, origin_y);
        let rivers = drainage.rivers_in_region(origin_x, origin_y);

        Self {
            elevation: elevation.into_boxed_slice(),
            water_depth: water_depth.into_boxed_slice(),
            temperature: temperature_map.into_boxed_slice(),
            moisture: moisture_map.into_boxed_slice(),
            roughness: roughness.into_boxed_slice(),
            rivers,
        }
    }
}

/// Moisture varies over kilometres, so it is sampled on a coarse sub-lattice
/// and interpolated to the full grid. The upwind ocean probes make this the
/// most expensive retained regional climate field.
const MOISTURE_COARSE_STEP: usize = 4;
const MOISTURE_COARSE_GRID: usize = (GRID - 1) / MOISTURE_COARSE_STEP + 1;

fn moisture_lattice(seed: u64, origin_x: i64, origin_y: i64) -> Vec<i32> {
    let mut coarse = vec![0_i32; MOISTURE_COARSE_GRID * MOISTURE_COARSE_GRID];
    coarse
        .par_iter_mut()
        .enumerate()
        .for_each(|(index, moisture_value)| {
            let ci = index % MOISTURE_COARSE_GRID;
            let cj = index / MOISTURE_COARSE_GRID;
            let x = origin_x.saturating_add((ci * MOISTURE_COARSE_STEP) as i64 * NODE_STEP);
            let y = origin_y.saturating_add((cj * MOISTURE_COARSE_STEP) as i64 * NODE_STEP);
            let elevation = macro_sample(seed, x, y).elevation;
            *moisture_value = moisture(seed, x, y, elevation);
        });
    let mut fine = vec![0_i32; NODE_COUNT];
    let step = MOISTURE_COARSE_STEP as i64;
    for j in 0..GRID {
        for i in 0..GRID {
            let ci = (i / MOISTURE_COARSE_STEP).min(MOISTURE_COARSE_GRID - 2);
            let cj = (j / MOISTURE_COARSE_STEP).min(MOISTURE_COARSE_GRID - 2);
            let fx = i as i64 - (ci * MOISTURE_COARSE_STEP) as i64;
            let fy = j as i64 - (cj * MOISTURE_COARSE_STEP) as i64;
            let base = cj * MOISTURE_COARSE_GRID + ci;
            let top = i64::from(coarse[base]) * (step - fx) + i64::from(coarse[base + 1]) * fx;
            let bottom = i64::from(coarse[base + MOISTURE_COARSE_GRID]) * (step - fx)
                + i64::from(coarse[base + MOISTURE_COARSE_GRID + 1]) * fx;
            fine[j * GRID + i] = ((top * (step - fy) + bottom * fy) / (step * step)) as i32;
        }
    }
    fine
}

/// Squared distance from `point` to segment `start..end`, as a ratio pair so
/// callers compare against squared widths without rooting.
pub(crate) fn point_segment_distance_ratio(
    px: i64,
    py: i64,
    ax: i64,
    ay: i64,
    bx: i64,
    by: i64,
) -> (i128, i128) {
    let dx = i128::from(bx) - i128::from(ax);
    let dy = i128::from(by) - i128::from(ay);
    let fx = i128::from(px) - i128::from(ax);
    let fy = i128::from(py) - i128::from(ay);
    let length_squared = dx * dx + dy * dy;
    let projection = fx * dx + fy * dy;
    if projection <= 0 {
        (fx * fx + fy * fy, 1)
    } else if projection >= length_squared {
        let ex = i128::from(px) - i128::from(bx);
        let ey = i128::from(py) - i128::from(by);
        (ex * ex + ey * ey, 1)
    } else {
        let cross = fx * dy - fy * dx;
        (cross * cross, length_squared)
    }
}
