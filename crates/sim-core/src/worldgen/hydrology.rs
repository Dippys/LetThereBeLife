//! Regional drainage derived from the macro elevation field: depressions are
//! flooded into lakes and accumulated runoff is routed downhill into rivers
//! that terminate in an ocean, a lake, or the edge of their drainage region.
//!
//! Each `REGION_SIZE` square owns a `GRID`x`GRID` node lattice sampled every
//! `NODE_STEP` cells. The lattice is a pure function of the world seed and the
//! region coordinates, so chunk generation stays deterministic and seamless.
//! Region borders act as drainage sinks; rivers within a border margin are not
//! emitted, which keeps every emitted segment consistent no matter which chunk
//! asks for it.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use super::climate::{moisture, temperature};
use super::plates::{SEA_LEVEL, macro_sample};

pub(crate) const REGION_SIZE: i64 = 4_096;
pub(crate) const NODE_STEP: i64 = 32;
pub(crate) const GRID: usize = (REGION_SIZE / NODE_STEP) as usize + 1;
const NODE_COUNT: usize = GRID * GRID;

/// Nodes closer than this to a region border never emit river segments.
const RIVER_MARGIN_NODES: usize = 4;
/// Two dry node layers keep region-local lakes from reaching a neighboring
/// region that has no shared depression-fill state.
pub(super) const LAKE_BORDER_MARGIN_NODES: usize = 2;
/// Accumulated runoff required before a channel becomes a visible river.
const RIVER_FLOW_THRESHOLD: i64 = 260_000;
/// Water shallower than this over a node is treated as dry land.
pub(crate) const LAKE_MIN_DEPTH: i32 = 250;
pub(crate) const RIVER_MAX_HALF_WIDTH: i64 = 13;
const RIVER_JITTER_SEED: u64 = 0x5249_564a_4954_5445;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RiverSegment {
    pub(crate) ax: i64,
    pub(crate) ay: i64,
    pub(crate) bx: i64,
    pub(crate) by: i64,
    pub(crate) half_width: i64,
}

/// Cached per-region lattice of macro terrain, climate, and drainage.
pub(crate) struct RegionMap {
    pub(crate) elevation: Box<[i32]>,
    /// Standing water above each node (fill level minus ground), 0 when dry.
    pub(crate) water_depth: Box<[i32]>,
    pub(crate) temperature: Box<[i32]>,
    pub(crate) moisture: Box<[i32]>,
    pub(crate) roughness: Box<[i32]>,
    pub(crate) rivers: Vec<RiverSegment>,
}

impl RegionMap {
    pub(crate) fn build(seed: u64, region_x: i64, region_y: i64) -> Self {
        let origin_x = region_x * REGION_SIZE;
        let origin_y = region_y * REGION_SIZE;
        let mut elevation = vec![0_i32; NODE_COUNT];
        let mut temperature_map = vec![0_i32; NODE_COUNT];
        let mut roughness = vec![0_i32; NODE_COUNT];
        for j in 0..GRID {
            for i in 0..GRID {
                let x = origin_x.saturating_add(i as i64 * NODE_STEP);
                let y = origin_y.saturating_add(j as i64 * NODE_STEP);
                let index = j * GRID + i;
                let sample = macro_sample(seed, x, y);
                elevation[index] = sample.elevation;
                roughness[index] = sample.roughness;
                temperature_map[index] = temperature(seed, x, y, sample.elevation);
            }
        }
        let moisture_map = moisture_lattice(seed, origin_x, origin_y);

        let fill = priority_fill(&elevation);
        let rivers = extract_rivers(seed, origin_x, origin_y, &elevation, &fill, &moisture_map);
        let mut water_depth: Vec<_> = fill
            .iter()
            .zip(elevation.iter())
            .map(|(fill, ground)| fill - ground)
            .collect();
        suppress_border_lakes(&mut water_depth);

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
/// (every `MOISTURE_COARSE_STEP` nodes) and interpolated to the full grid;
/// the upwind ocean probes make it by far the most expensive climate field.
const MOISTURE_COARSE_STEP: usize = 4;
const MOISTURE_COARSE_GRID: usize = (GRID - 1) / MOISTURE_COARSE_STEP + 1;

fn moisture_lattice(seed: u64, origin_x: i64, origin_y: i64) -> Vec<i32> {
    let mut coarse = vec![0_i32; MOISTURE_COARSE_GRID * MOISTURE_COARSE_GRID];
    for cj in 0..MOISTURE_COARSE_GRID {
        for ci in 0..MOISTURE_COARSE_GRID {
            let x = origin_x.saturating_add((ci * MOISTURE_COARSE_STEP) as i64 * NODE_STEP);
            let y = origin_y.saturating_add((cj * MOISTURE_COARSE_STEP) as i64 * NODE_STEP);
            let elevation = macro_sample(seed, x, y).elevation;
            coarse[cj * MOISTURE_COARSE_GRID + ci] = moisture(seed, x, y, elevation);
        }
    }
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

fn neighbors4(index: usize) -> impl Iterator<Item = usize> {
    let x = index % GRID;
    let y = index / GRID;
    [
        (x > 0).then(|| index - 1),
        (x + 1 < GRID).then(|| index + 1),
        (y > 0).then(|| index - GRID),
        (y + 1 < GRID).then(|| index + GRID),
    ]
    .into_iter()
    .flatten()
}

fn neighbors8(index: usize) -> impl Iterator<Item = usize> {
    let x = (index % GRID) as isize;
    let y = (index / GRID) as isize;
    [
        (-1, -1),
        (0, -1),
        (1, -1),
        (-1, 0),
        (1, 0),
        (-1, 1),
        (0, 1),
        (1, 1),
    ]
    .into_iter()
    .filter_map(move |(dx, dy)| {
        let nx = x + dx;
        let ny = y + dy;
        (nx >= 0 && ny >= 0 && nx < GRID as isize && ny < GRID as isize)
            .then(|| ny as usize * GRID + nx as usize)
    })
}

fn is_border(index: usize) -> bool {
    let x = index % GRID;
    let y = index / GRID;
    x == 0 || y == 0 || x == GRID - 1 || y == GRID - 1
}

fn in_river_margin(index: usize) -> bool {
    let x = index % GRID;
    let y = index / GRID;
    x < RIVER_MARGIN_NODES
        || y < RIVER_MARGIN_NODES
        || x >= GRID - RIVER_MARGIN_NODES
        || y >= GRID - RIVER_MARGIN_NODES
}

fn in_lake_border_margin(index: usize) -> bool {
    let x = index % GRID;
    let y = index / GRID;
    x < LAKE_BORDER_MARGIN_NODES
        || y < LAKE_BORDER_MARGIN_NODES
        || x >= GRID - LAKE_BORDER_MARGIN_NODES
        || y >= GRID - LAKE_BORDER_MARGIN_NODES
}

fn suppress_border_lakes(water_depth: &mut [i32]) {
    for (index, depth) in water_depth.iter_mut().enumerate() {
        if in_lake_border_margin(index) {
            *depth = 0;
        }
    }
}

/// Priority-flood depression filling with a one-unit epsilon gradient, so
/// every node keeps a strictly descending path to an ocean or border sink.
fn priority_fill(elevation: &[i32]) -> Vec<i32> {
    let mut fill = vec![i32::MAX; NODE_COUNT];
    let mut done = vec![false; NODE_COUNT];
    let mut heap = BinaryHeap::with_capacity(NODE_COUNT);
    for index in 0..NODE_COUNT {
        if is_border(index) || elevation[index] <= SEA_LEVEL {
            fill[index] = elevation[index];
            heap.push(Reverse((elevation[index], index as u32)));
        }
    }
    while let Some(Reverse((level, index))) = heap.pop() {
        let index = index as usize;
        if done[index] || level > fill[index] {
            continue;
        }
        done[index] = true;
        for neighbor in neighbors4(index) {
            if done[neighbor] {
                continue;
            }
            let candidate = if elevation[neighbor] > level {
                elevation[neighbor]
            } else {
                level + 1
            };
            if candidate < fill[neighbor] {
                fill[neighbor] = candidate;
                heap.push(Reverse((candidate, neighbor as u32)));
            }
        }
    }
    fill
}

/// Routes runoff down the filled surface and emits channel segments wherever
/// the accumulated flow crosses the river threshold.
fn extract_rivers(
    seed: u64,
    origin_x: i64,
    origin_y: i64,
    elevation: &[i32],
    fill: &[i32],
    moisture: &[i32],
) -> Vec<RiverSegment> {
    let mut order: Vec<u32> = (0..NODE_COUNT as u32).collect();
    order.sort_unstable_by_key(|&index| (Reverse(fill[index as usize]), index));
    let mut accumulation = vec![0_i64; NODE_COUNT];
    let mut target = vec![u32::MAX; NODE_COUNT];
    for &index in &order {
        let index = index as usize;
        if fill[index] <= SEA_LEVEL {
            continue;
        }
        let mut best: Option<(i32, usize)> = None;
        for neighbor in neighbors8(index) {
            if fill[neighbor] < fill[index] && best.is_none_or(|(level, _)| fill[neighbor] < level)
            {
                best = Some((fill[neighbor], neighbor));
            }
        }
        let Some((_, downstream)) = best else {
            continue;
        };
        target[index] = downstream as u32;
        let rain = 200 + i64::from(moisture[index]) / 96;
        accumulation[index] += rain;
        accumulation[downstream] += accumulation[index];
    }

    // Deterministic per-node jitter keeps channels off the lattice diagonals;
    // both endpoints of adjacent segments jitter identically, so polylines
    // stay connected across segments and chunk borders.
    let node_position = |index: usize| {
        let x = origin_x.saturating_add((index % GRID) as i64 * NODE_STEP);
        let y = origin_y.saturating_add((index / GRID) as i64 * NODE_STEP);
        let bits = super::noise::hash(seed ^ RIVER_JITTER_SEED, x, y);
        (
            x.saturating_add((bits & 0x1F) as i64 - 16),
            y.saturating_add(((bits >> 5) & 0x1F) as i64 - 16),
        )
    };
    let mut rivers = Vec::new();
    for index in 0..NODE_COUNT {
        if accumulation[index] < RIVER_FLOW_THRESHOLD
            || elevation[index] <= SEA_LEVEL
            || in_river_margin(index)
            || target[index] == u32::MAX
        {
            continue;
        }
        let downstream = target[index] as usize;
        if in_river_margin(downstream) {
            continue;
        }
        let submerged = |node: usize| fill[node] - elevation[node] >= LAKE_MIN_DEPTH;
        if submerged(index) && submerged(downstream) {
            // Both ends are under a lake surface; the lake already renders it.
            continue;
        }
        let (ax, ay) = node_position(index);
        let (bx, by) = node_position(downstream);
        let half_width = (2 + (accumulation[index] / 65_000).isqrt()).min(RIVER_MAX_HALF_WIDTH);
        rivers.push(RiverSegment {
            ax,
            ay,
            bx,
            by,
            half_width,
        });
    }
    rivers
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
