//! Canonical whole-envelope drainage topology.
//!
//! A compact coarse lattice is built once per seed. It owns depression fill,
//! lake outlets, major-channel continuation, and the geometry that crosses
//! regional boundaries. Regional maps sample this immutable result; they do
//! not independently decide what happens at a shared edge.

use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

use rayon::prelude::*;

use crate::world::{WORLD_GENERATION_BOUNDS, WORLD_SIDE_CELLS};

use super::climate::moisture;
use super::hydrology::{LAKE_MIN_DEPTH, NODE_STEP, REGION_SIZE};
use super::plates::{SEA_LEVEL, macro_sample};

pub(crate) const SKELETON_STEP: i64 = 256;
pub(crate) const RIVER_MAX_HALF_WIDTH: i64 = 13;
const RIVER_FLOW_THRESHOLD: u64 = 260_000;
const RIVER_JITTER_SEED: u64 = 0x5249_564a_4954_5445;
const SKELETON_CACHE_CAPACITY: usize = 4;
const NO_TARGET: u32 = u32::MAX;

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DrainageSegment {
    pub(crate) ax: i32,
    pub(crate) ay: i32,
    pub(crate) bx: i32,
    pub(crate) by: i32,
    pub(crate) channel_id: u32,
    pub(crate) half_width: u8,
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChannelOutlet {
    Continues,
    Confluence,
    Lake,
    Ocean,
    WorldEdge,
    TerminalBasin,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ChannelLink {
    from: u32,
    to: u32,
    flow: u32,
    channel_id: u32,
    basin_id: u32,
    water_body_id: u32,
    outlet: ChannelOutlet,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LakeDescriptor {
    id: u32,
    outlet_node: u32,
    spill_elevation: u16,
    terminal: bool,
}

/// Immutable seed-owned drainage data retained independently of regional
/// materialization. Only sampled fill/lake surfaces and compact lake/channel
/// records survive the build; elevation, flow, and graph scratch are released.
pub(crate) struct DrainageSkeleton {
    step: i64,
    grid: usize,
    lake_depth: Box<[u16]>,
    fill_surface: Box<[u16]>,
    rivers: Box<[DrainageSegment]>,
    channels: Box<[ChannelLink]>,
    lakes: Box<[LakeDescriptor]>,
}

type DrainageSlot = Arc<OnceLock<Arc<DrainageSkeleton>>>;
static DRAINAGE_CACHE: OnceLock<Mutex<Vec<(u64, DrainageSlot)>>> = OnceLock::new();

fn drainage_cache() -> &'static Mutex<Vec<(u64, DrainageSlot)>> {
    DRAINAGE_CACHE.get_or_init(|| Mutex::new(Vec::new()))
}

fn lock_drainage_cache() -> MutexGuard<'static, Vec<(u64, DrainageSlot)>> {
    drainage_cache()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn trim_drainage_cache(cache: &mut Vec<(u64, DrainageSlot)>) {
    while cache.len() > SKELETON_CACHE_CAPACITY {
        let Some(position) = cache.iter().rposition(|(_, slot)| slot.get().is_some()) else {
            // Do not evict in-flight builds; a bounded temporary excess is
            // allowed when distinct seeds are first requested concurrently.
            break;
        };
        cache.remove(position);
    }
}

pub(crate) fn world_drainage(seed: u64) -> Arc<DrainageSkeleton> {
    let slot = {
        let mut cache = lock_drainage_cache();
        if let Some(position) = cache.iter().position(|(entry_seed, _)| *entry_seed == seed) {
            let entry = cache.remove(position);
            let slot = Arc::clone(&entry.1);
            cache.insert(0, entry);
            slot
        } else {
            let slot = Arc::new(OnceLock::new());
            cache.insert(0, (seed, Arc::clone(&slot)));
            trim_drainage_cache(&mut cache);
            slot
        }
    };
    let drainage =
        Arc::clone(slot.get_or_init(|| Arc::new(DrainageSkeleton::build(seed, SKELETON_STEP))));
    drainage.validate_metadata();
    trim_drainage_cache(&mut lock_drainage_cache());
    drainage
}

impl DrainageSkeleton {
    fn build(seed: u64, step: i64) -> Self {
        assert!(step > 0 && WORLD_SIDE_CELLS % step == 0);
        assert!(step % NODE_STEP == 0);
        let grid = (WORLD_SIDE_CELLS / step) as usize + 1;
        let node_count = grid * grid;
        let mut elevation = vec![0_i32; node_count];
        elevation
            .par_iter_mut()
            .enumerate()
            .for_each(|(index, elevation_value)| {
                let (x, y) = node_position(index, grid, step);
                *elevation_value = macro_sample(seed, x, y).elevation;
            });
        let moisture_map = moisture_lattice(seed, grid, step);

        let fill = priority_fill(&elevation, grid);
        let (target, accumulation) = route_flow(&elevation, &fill, &moisture_map, grid, step);
        drop(moisture_map);
        let basin_ids = identify_basins(&target, &fill);

        let mut lake_depth = vec![0_u16; node_count];
        let mut fill_surface = vec![0_u16; node_count];
        for index in 0..node_count {
            lake_depth[index] = (fill[index] - elevation[index]).clamp(0, u16::MAX as i32) as u16;
            fill_surface[index] = fill[index].clamp(0, u16::MAX as i32) as u16;
        }
        let (lake_ids, lakes) = identify_lakes(&elevation, &fill, &target, &lake_depth, grid);
        let (channels, rivers) = extract_channels(
            seed,
            &elevation,
            &fill,
            &target,
            &accumulation,
            &basin_ids,
            &lake_depth,
            &lake_ids,
            grid,
            step,
        );

        Self {
            step,
            grid,
            lake_depth: lake_depth.into_boxed_slice(),
            fill_surface: fill_surface.into_boxed_slice(),
            rivers: rivers.into_boxed_slice(),
            channels: channels.into_boxed_slice(),
            lakes: lakes.into_boxed_slice(),
        }
    }

    pub(crate) fn water_depth_at(&self, x: i64, y: i64, elevation: i32) -> i32 {
        let offset_x = (x - WORLD_GENERATION_BOUNDS.min.x).clamp(0, WORLD_SIDE_CELLS);
        let offset_y = (y - WORLD_GENERATION_BOUNDS.min.y).clamp(0, WORLD_SIDE_CELLS);
        let cell_x = ((offset_x / self.step) as usize).min(self.grid - 2);
        let cell_y = ((offset_y / self.step) as usize).min(self.grid - 2);
        let fx = offset_x - cell_x as i64 * self.step;
        let fy = offset_y - cell_y as i64 * self.step;
        let base = cell_y * self.grid + cell_x;
        let depth_top = i64::from(self.lake_depth[base]) * (self.step - fx)
            + i64::from(self.lake_depth[base + 1]) * fx;
        let depth_bottom = i64::from(self.lake_depth[base + self.grid]) * (self.step - fx)
            + i64::from(self.lake_depth[base + self.grid + 1]) * fx;
        let interpolated_depth =
            (depth_top * (self.step - fy) + depth_bottom * fy) / (self.step * self.step);
        if interpolated_depth < i64::from(LAKE_MIN_DEPTH) {
            return 0;
        }
        let surface_top = i64::from(self.fill_surface[base]) * (self.step - fx)
            + i64::from(self.fill_surface[base + 1]) * fx;
        let surface_bottom = i64::from(self.fill_surface[base + self.grid]) * (self.step - fx)
            + i64::from(self.fill_surface[base + self.grid + 1]) * fx;
        let surface =
            (surface_top * (self.step - fy) + surface_bottom * fy) / (self.step * self.step);
        (surface - i64::from(elevation)).clamp(0, i64::from(u16::MAX)) as i32
    }

    pub(crate) fn rivers_in_region(&self, origin_x: i64, origin_y: i64) -> Vec<DrainageSegment> {
        let max_x = origin_x + REGION_SIZE;
        let max_y = origin_y + REGION_SIZE;
        self.rivers
            .iter()
            .copied()
            .filter(|segment| {
                let width = i64::from(segment.half_width);
                i64::from(segment.ax.max(segment.bx)) + width >= origin_x
                    && i64::from(segment.ax.min(segment.bx)) - width < max_x
                    && i64::from(segment.ay.max(segment.by)) + width >= origin_y
                    && i64::from(segment.ay.min(segment.by)) - width < max_y
            })
            .collect()
    }

    fn validate_metadata(&self) {
        debug_assert!(self.channels.len() <= self.rivers.len());
        debug_assert_eq!(self.fill_surface.len(), self.lake_depth.len());
        debug_assert!(
            self.lakes
                .iter()
                .all(|lake| { lake.terminal || lake.outlet_node < (self.grid * self.grid) as u32 })
        );
    }

    #[cfg(test)]
    fn retained_bytes(&self) -> usize {
        self.lake_depth.len() * size_of::<u16>()
            + self.fill_surface.len() * size_of::<u16>()
            + self.rivers.len() * size_of::<DrainageSegment>()
            + self.channels.len() * size_of::<ChannelLink>()
            + self.lakes.len() * size_of::<LakeDescriptor>()
    }

    #[cfg(test)]
    fn scratch_upper_bound_bytes(&self) -> usize {
        // Upper bound for simultaneously useful vector capacities during the
        // build. Allocator metadata, Rayon stacks, and plate/climate caches are
        // deliberately excluded and remain measurement gaps.
        let node_count = self.grid * self.grid;
        node_count
            * (size_of::<i32>() * 3
                + size_of::<u8>() * 2
                + size_of::<(i32, u32)>()
                + size_of::<u32>() * 6
                + size_of::<u64>() * 2
                + size_of::<u16>() * 2)
    }
}

const MOISTURE_COARSE_STEP: usize = 4;

fn moisture_lattice(seed: u64, grid: usize, node_step: i64) -> Vec<i32> {
    let coarse_grid = (grid - 1) / MOISTURE_COARSE_STEP + 1;
    let mut coarse = vec![0_i32; coarse_grid * coarse_grid];
    coarse
        .par_iter_mut()
        .enumerate()
        .for_each(|(index, moisture_value)| {
            let coarse_x = index % coarse_grid;
            let coarse_y = index / coarse_grid;
            let x = WORLD_GENERATION_BOUNDS.min.x
                + (coarse_x * MOISTURE_COARSE_STEP) as i64 * node_step;
            let y = WORLD_GENERATION_BOUNDS.min.y
                + (coarse_y * MOISTURE_COARSE_STEP) as i64 * node_step;
            let elevation = macro_sample(seed, x, y).elevation;
            *moisture_value = moisture(seed, x, y, elevation);
        });

    let mut fine = vec![0_i32; grid * grid];
    let interpolation_step = MOISTURE_COARSE_STEP as i64;
    for y in 0..grid {
        for x in 0..grid {
            let coarse_x = (x / MOISTURE_COARSE_STEP).min(coarse_grid - 2);
            let coarse_y = (y / MOISTURE_COARSE_STEP).min(coarse_grid - 2);
            let fraction_x = x as i64 - (coarse_x * MOISTURE_COARSE_STEP) as i64;
            let fraction_y = y as i64 - (coarse_y * MOISTURE_COARSE_STEP) as i64;
            let base = coarse_y * coarse_grid + coarse_x;
            let top = i64::from(coarse[base]) * (interpolation_step - fraction_x)
                + i64::from(coarse[base + 1]) * fraction_x;
            let bottom = i64::from(coarse[base + coarse_grid]) * (interpolation_step - fraction_x)
                + i64::from(coarse[base + coarse_grid + 1]) * fraction_x;
            fine[y * grid + x] = ((top * (interpolation_step - fraction_y) + bottom * fraction_y)
                / (interpolation_step * interpolation_step))
                as i32;
        }
    }
    fine
}

fn node_position(index: usize, grid: usize, step: i64) -> (i64, i64) {
    (
        WORLD_GENERATION_BOUNDS.min.x + (index % grid) as i64 * step,
        WORLD_GENERATION_BOUNDS.min.y + (index / grid) as i64 * step,
    )
}

fn neighbors4(index: usize, grid: usize) -> impl Iterator<Item = usize> {
    let x = index % grid;
    let y = index / grid;
    [
        (x > 0).then(|| index - 1),
        (x + 1 < grid).then(|| index + 1),
        (y > 0).then(|| index - grid),
        (y + 1 < grid).then(|| index + grid),
    ]
    .into_iter()
    .flatten()
}

fn neighbors8(index: usize, grid: usize) -> impl Iterator<Item = usize> {
    let x = (index % grid) as isize;
    let y = (index / grid) as isize;
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
        (nx >= 0 && ny >= 0 && nx < grid as isize && ny < grid as isize)
            .then(|| ny as usize * grid + nx as usize)
    })
}

fn is_border(index: usize, grid: usize) -> bool {
    let x = index % grid;
    let y = index / grid;
    x == 0 || y == 0 || x == grid - 1 || y == grid - 1
}

fn priority_fill(elevation: &[i32], grid: usize) -> Vec<i32> {
    let mut fill = vec![i32::MAX; elevation.len()];
    let mut done = vec![false; elevation.len()];
    let mut heap = BinaryHeap::with_capacity(elevation.len());
    for index in 0..elevation.len() {
        if is_border(index, grid) || elevation[index] <= SEA_LEVEL {
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
        for neighbor in neighbors4(index, grid) {
            if done[neighbor] {
                continue;
            }
            let candidate = elevation[neighbor].max(level.saturating_add(1));
            if candidate < fill[neighbor] {
                fill[neighbor] = candidate;
                heap.push(Reverse((candidate, neighbor as u32)));
            }
        }
    }
    fill
}

fn route_flow(
    elevation: &[i32],
    fill: &[i32],
    moisture: &[i32],
    grid: usize,
    step: i64,
) -> (Vec<u32>, Vec<u64>) {
    let mut order: Vec<u32> = (0..elevation.len() as u32).collect();
    order.sort_unstable_by_key(|&index| (Reverse(fill[index as usize]), index));
    let mut target = vec![NO_TARGET; elevation.len()];
    let mut accumulation = vec![0_u64; elevation.len()];
    let area_scale = ((step / NODE_STEP) * (step / NODE_STEP)) as u64;
    for &index in &order {
        let index = index as usize;
        if elevation[index] <= SEA_LEVEL {
            continue;
        }
        accumulation[index] = accumulation[index]
            .saturating_add((200 + i64::from(moisture[index]) / 96).max(0) as u64 * area_scale);
        let downstream = neighbors8(index, grid)
            .filter(|&neighbor| fill[neighbor] < fill[index])
            .min_by_key(|&neighbor| (fill[neighbor], neighbor));
        if let Some(downstream) = downstream {
            target[index] = downstream as u32;
            accumulation[downstream] = accumulation[downstream].saturating_add(accumulation[index]);
        }
    }
    (target, accumulation)
}

fn identify_basins(target: &[u32], fill: &[i32]) -> Vec<u32> {
    let mut order: Vec<u32> = (0..target.len() as u32).collect();
    order.sort_unstable_by_key(|&index| (fill[index as usize], index));
    let mut basin_ids = vec![0_u32; target.len()];
    for index in order {
        let index = index as usize;
        basin_ids[index] = if target[index] == NO_TARGET {
            index as u32 + 1
        } else {
            let downstream_id = basin_ids[target[index] as usize];
            debug_assert_ne!(downstream_id, 0);
            downstream_id
        };
    }
    basin_ids
}

fn identify_lakes(
    elevation: &[i32],
    fill: &[i32],
    target: &[u32],
    lake_depth: &[u16],
    grid: usize,
) -> (Vec<u32>, Vec<LakeDescriptor>) {
    let mut ids = vec![0_u32; elevation.len()];
    let mut lakes = Vec::new();
    let mut stack = Vec::new();
    let mut component = Vec::new();
    for start in 0..elevation.len() {
        if ids[start] != 0
            || elevation[start] <= SEA_LEVEL
            || i32::from(lake_depth[start]) < LAKE_MIN_DEPTH
        {
            continue;
        }
        let id = start as u32 + 1;
        ids[start] = id;
        stack.push(start);
        component.clear();
        while let Some(index) = stack.pop() {
            component.push(index);
            for neighbor in neighbors4(index, grid) {
                if ids[neighbor] == 0
                    && elevation[neighbor] > SEA_LEVEL
                    && i32::from(lake_depth[neighbor]) >= LAKE_MIN_DEPTH
                {
                    ids[neighbor] = id;
                    stack.push(neighbor);
                }
            }
        }
        let outlet = component
            .iter()
            .filter_map(|&index| {
                let downstream = target[index];
                (downstream != NO_TARGET && ids[downstream as usize] != id).then_some((
                    fill[downstream as usize],
                    index,
                    downstream as usize,
                ))
            })
            .min();
        let (outlet_node, terminal, spill_elevation) = outlet.map_or_else(
            || (start, true, fill[start]),
            |(_, _, downstream)| (downstream, false, fill[downstream]),
        );
        lakes.push(LakeDescriptor {
            id,
            outlet_node: outlet_node as u32,
            spill_elevation: spill_elevation.clamp(0, u16::MAX as i32) as u16,
            terminal,
        });
    }
    (ids, lakes)
}

#[allow(clippy::too_many_arguments)]
fn extract_channels(
    seed: u64,
    elevation: &[i32],
    fill: &[i32],
    target: &[u32],
    accumulation: &[u64],
    basin_ids: &[u32],
    lake_depth: &[u16],
    lake_ids: &[u32],
    grid: usize,
    step: i64,
) -> (Vec<ChannelLink>, Vec<DrainageSegment>) {
    let mut order: Vec<u32> = (0..elevation.len() as u32).collect();
    order.sort_unstable_by_key(|&index| (Reverse(fill[index as usize]), index));
    let mut channel_ids = vec![0_u32; elevation.len()];
    let mut strongest_flow = vec![0_u64; elevation.len()];
    for &index in &order {
        let index = index as usize;
        if accumulation[index] < RIVER_FLOW_THRESHOLD || target[index] == NO_TARGET {
            continue;
        }
        if channel_ids[index] == 0 {
            channel_ids[index] = index as u32 + 1;
        }
        let downstream = target[index] as usize;
        let candidate = (accumulation[index], Reverse(channel_ids[index]));
        let current = (strongest_flow[downstream], Reverse(channel_ids[downstream]));
        if candidate > current {
            strongest_flow[downstream] = accumulation[index];
            channel_ids[downstream] = channel_ids[index];
        }
    }

    let subdivisions = (step / NODE_STEP) as usize;
    let mut channels = Vec::new();
    let mut rivers = Vec::new();
    for from in 0..elevation.len() {
        if accumulation[from] < RIVER_FLOW_THRESHOLD
            || elevation[from] <= SEA_LEVEL
            || target[from] == NO_TARGET
        {
            continue;
        }
        let to = target[from] as usize;
        let from_lake = i32::from(lake_depth[from]) >= LAKE_MIN_DEPTH;
        let to_lake = i32::from(lake_depth[to]) >= LAKE_MIN_DEPTH;
        if from_lake && to_lake {
            continue;
        }
        let outlet = if elevation[to] <= SEA_LEVEL {
            ChannelOutlet::Ocean
        } else if lake_ids[to] != 0 {
            ChannelOutlet::Lake
        } else if is_border(to, grid) {
            ChannelOutlet::WorldEdge
        } else if accumulation[to] >= RIVER_FLOW_THRESHOLD && target[to] != NO_TARGET {
            if channel_ids[to] == channel_ids[from] {
                ChannelOutlet::Continues
            } else {
                ChannelOutlet::Confluence
            }
        } else {
            ChannelOutlet::TerminalBasin
        };
        let flow = accumulation[from].min(u64::from(u32::MAX)) as u32;
        let channel_id = channel_ids[from];
        debug_assert_ne!(channel_id, 0);
        channels.push(ChannelLink {
            from: from as u32,
            to: to as u32,
            flow,
            channel_id,
            basin_id: basin_ids[from],
            water_body_id: lake_ids[to],
            outlet,
        });

        let half_width =
            (2 + (accumulation[from] / 65_000).isqrt()).min(RIVER_MAX_HALF_WIDTH as u64) as u8;
        let start = node_position(from, grid, step);
        let end = node_position(to, grid, step);
        let mut previous = channel_point(seed, start, end, 0, subdivisions);
        for part in 1..=subdivisions {
            let next = channel_point(seed, start, end, part, subdivisions);
            if next != previous {
                rivers.push(DrainageSegment {
                    ax: previous.0 as i32,
                    ay: previous.1 as i32,
                    bx: next.0 as i32,
                    by: next.1 as i32,
                    channel_id,
                    half_width,
                });
            }
            previous = next;
        }
    }
    (channels, rivers)
}

fn channel_point(
    seed: u64,
    start: (i64, i64),
    end: (i64, i64),
    part: usize,
    subdivisions: usize,
) -> (i64, i64) {
    let remaining = (subdivisions - part) as i64;
    let part = part as i64;
    let divisor = subdivisions as i64;
    let base_x = (start.0 * remaining + end.0 * part).div_euclid(divisor);
    let base_y = (start.1 * remaining + end.1 * part).div_euclid(divisor);
    let bits = super::noise::hash(seed ^ RIVER_JITTER_SEED, base_x, base_y);
    let x = base_x + (bits & 0x1f) as i64 - 16;
    let y = base_y + ((bits >> 5) & 0x1f) as i64 - 16;
    (
        x.clamp(WORLD_GENERATION_BOUNDS.min.x, WORLD_GENERATION_BOUNDS.max.x),
        y.clamp(WORLD_GENERATION_BOUNDS.min.y, WORLD_GENERATION_BOUNDS.max.y),
    )
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::time::Instant;

    use rayon::ThreadPoolBuilder;

    use super::*;

    #[test]
    fn drainage_records_have_bounded_layouts() {
        assert_eq!(size_of::<DrainageSegment>(), 24);
        assert_eq!(size_of::<ChannelLink>(), 28);
        assert_eq!(size_of::<LakeDescriptor>(), 12);
    }

    #[test]
    fn skeleton_parallelism_preserves_exact_output() {
        let build = |workers| {
            ThreadPoolBuilder::new()
                .num_threads(workers)
                .build()
                .unwrap()
                .install(|| DrainageSkeleton::build(42, SKELETON_STEP))
        };
        let single = build(1);
        let parallel = build(4);
        assert_eq!(parallel.lake_depth, single.lake_depth);
        assert_eq!(parallel.fill_surface, single.fill_surface);
        assert_eq!(parallel.rivers, single.rivers);
        assert_eq!(parallel.channels, single.channels);
        assert_eq!(parallel.lakes, single.lakes);
    }

    #[test]
    fn channels_continue_to_canonical_outlets_across_signed_region_seams() {
        let mut seam_kinds = [false; 4];
        let mut checked = 0;
        for seed in [1, 7, 42, 10_001] {
            let drainage = DrainageSkeleton::build(seed, SKELETON_STEP);
            let mut segments_per_chunk = BTreeMap::<(i64, i64), usize>::new();
            for segment in &drainage.rivers {
                let width = i64::from(segment.half_width);
                let min_x = (i64::from(segment.ax.min(segment.bx)) - width)
                    .div_euclid(crate::world::CHUNK_SIZE);
                let max_x = (i64::from(segment.ax.max(segment.bx)) + width)
                    .div_euclid(crate::world::CHUNK_SIZE);
                let min_y = (i64::from(segment.ay.min(segment.by)) - width)
                    .div_euclid(crate::world::CHUNK_SIZE);
                let max_y = (i64::from(segment.ay.max(segment.by)) + width)
                    .div_euclid(crate::world::CHUNK_SIZE);
                for chunk_y in min_y..=max_y {
                    for chunk_x in min_x..=max_x {
                        if (-512..512).contains(&chunk_x) && (-512..512).contains(&chunk_y) {
                            *segments_per_chunk.entry((chunk_x, chunk_y)).or_default() += 1;
                        }
                    }
                }
            }
            let maximum = segments_per_chunk.values().copied().max().unwrap_or(0);
            assert!(
                maximum <= crate::worldgen::MAX_CHUNK_RIVERS,
                "seed {seed} needs {maximum} chunk river slots"
            );
            for link in &drainage.channels {
                checked += 1;
                match link.outlet {
                    ChannelOutlet::Continues => {
                        let downstream = drainage
                            .channels
                            .iter()
                            .find(|candidate| candidate.from == link.to)
                            .expect("declared continuation must exist");
                        assert_eq!(downstream.channel_id, link.channel_id);
                        assert_eq!(downstream.basin_id, link.basin_id);
                    }
                    ChannelOutlet::Confluence => assert!(
                        drainage.channels.iter().any(|candidate| {
                            candidate.from == link.to && candidate.basin_id == link.basin_id
                        }),
                        "tributary {} does not reach its canonical basin",
                        link.channel_id
                    ),
                    ChannelOutlet::Lake => {
                        assert_ne!(link.water_body_id, 0);
                        assert!(
                            drainage
                                .lakes
                                .iter()
                                .any(|lake| lake.id == link.water_body_id)
                        );
                    }
                    ChannelOutlet::Ocean | ChannelOutlet::WorldEdge => {}
                    ChannelOutlet::TerminalBasin => {
                        assert!(is_border(link.to as usize, drainage.grid));
                    }
                }
                let (from_x, from_y) =
                    node_position(link.from as usize, drainage.grid, drainage.step);
                let (to_x, to_y) = node_position(link.to as usize, drainage.grid, drainage.step);
                if from_x.div_euclid(REGION_SIZE) != to_x.div_euclid(REGION_SIZE) {
                    seam_kinds[usize::from(from_x.max(to_x) > 0)] = true;
                }
                if from_y.div_euclid(REGION_SIZE) != to_y.div_euclid(REGION_SIZE) {
                    seam_kinds[2 + usize::from(from_y.max(to_y) > 0)] = true;
                }
            }
            assert!(drainage.lakes.iter().all(|lake| {
                lake.terminal || lake.outlet_node < (drainage.grid * drainage.grid) as u32
            }));
        }
        assert!(checked > 100, "too few canonical channel links: {checked}");
        assert!(
            seam_kinds.into_iter().all(|covered| covered),
            "major channels must cross positive and negative x/y region seams"
        );
    }

    #[test]
    #[ignore = "release-only topology and footprint comparison"]
    fn compare_candidate_skeleton_steps() {
        let seed = std::env::var("SIM_DRAINAGE_SEED").ok().map_or(1, |value| {
            value
                .parse::<u64>()
                .expect("SIM_DRAINAGE_SEED must be an integer")
        });
        let requested = std::env::var("SIM_DRAINAGE_STEP").ok().map(|value| {
            value
                .parse::<i64>()
                .expect("SIM_DRAINAGE_STEP must be an integer")
        });
        let steps: &[i64] = requested.as_ref().map_or(&[128, 256], std::slice::from_ref);
        for &step in steps {
            let started = Instant::now();
            let drainage = DrainageSkeleton::build(seed, step);
            println!(
                "seed={seed} step={step} grid={} channels={} segments={} lakes={} retained_bytes={} scratch_upper_bound_bytes={} elapsed_ms={:.1}",
                drainage.grid,
                drainage.channels.len(),
                drainage.rivers.len(),
                drainage.lakes.len(),
                drainage.retained_bytes(),
                drainage.scratch_upper_bound_bytes(),
                started.elapsed().as_secs_f64() * 1_000.0
            );
        }
    }
}
