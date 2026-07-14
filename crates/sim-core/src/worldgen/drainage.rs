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
pub(crate) const FLOODPLAIN_RADIUS: i64 = 18;
const RIVER_SOURCE_FLOW_THRESHOLD: u64 = 260_000;
const RUNOFF_MOISTURE_FLOOR: i32 = 12_000;
const MAX_RIVER_SOURCES: usize = 24;
const RIVER_SOURCE_SPACING: i64 = 2_048;
const MAJOR_RIVER_FLOW_THRESHOLD: u64 = 520_000;
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
    pub(crate) surface_a: u16,
    pub(crate) surface_b: u16,
    pub(crate) half_width: u8,
    pub(crate) stream_order: u8,
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
    stream_order: u8,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LakeDescriptor {
    id: u32,
    outlet_node: u32,
    spill_elevation: u16,
    terminal: bool,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RiverSource {
    node: u32,
    lake_id: u32,
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
    sources: Box<[RiverSource]>,
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
        Self::build_with_threshold(seed, step, RIVER_SOURCE_FLOW_THRESHOLD)
    }

    fn build_with_threshold(seed: u64, step: i64, stream_flow_threshold: u64) -> Self {
        assert!(step > 0 && WORLD_SIDE_CELLS % step == 0);
        assert!(step % NODE_STEP == 0);
        assert!(stream_flow_threshold > 0);
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
        let (channels, rivers, sources) = extract_channels(
            seed,
            &elevation,
            &fill,
            &target,
            &accumulation,
            &basin_ids,
            &lake_ids,
            &lakes,
            grid,
            step,
            stream_flow_threshold,
        );

        Self {
            step,
            grid,
            lake_depth: lake_depth.into_boxed_slice(),
            fill_surface: fill_surface.into_boxed_slice(),
            rivers: rivers.into_boxed_slice(),
            channels: channels.into_boxed_slice(),
            lakes: lakes.into_boxed_slice(),
            sources: sources.into_boxed_slice(),
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
                let width = i64::from(segment.half_width) + FLOODPLAIN_RADIUS;
                i64::from(segment.ax.max(segment.bx)) + width >= origin_x
                    && i64::from(segment.ax.min(segment.bx)) - width < max_x
                    && i64::from(segment.ay.max(segment.by)) + width >= origin_y
                    && i64::from(segment.ay.min(segment.by)) - width < max_y
            })
            .collect()
    }

    fn validate_metadata(&self) {
        debug_assert!(self.channels.len() <= self.rivers.len());
        debug_assert!(self.sources.len() <= MAX_RIVER_SOURCES);
        debug_assert!(
            self.sources
                .iter()
                .all(|source| self.lakes.iter().any(|lake| {
                    lake.id == source.lake_id
                        && lake.outlet_node == source.node
                        && !lake.terminal
                }))
        );
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
            + self.sources.len() * size_of::<RiverSource>()
    }

    #[cfg(test)]
    fn scratch_upper_bound_bytes(&self) -> usize {
        // The source-selection phase is the widest logical overlap: canonical
        // fields, lake size/outlet work arrays, the selected mask, and at most
        // one candidate tuple per node. Allocator metadata, output-vector spare
        // capacity, Rayon stacks, and plate/climate caches are excluded.
        let node_count = self.grid * self.grid;
        node_count
            * (size_of::<i32>() * 2
                + size_of::<u32>() * 3
                + size_of::<u64>()
                + size_of::<u16>() * 3
                + size_of::<bool>()
                + size_of::<(usize, u32, u16, u64, i32, u32)>())
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
        let effective_runoff =
            (i64::from(moisture[index]) - i64::from(RUNOFF_MOISTURE_FLOOR)).max(0) as u64 / 96;
        accumulation[index] = accumulation[index].saturating_add(effective_runoff * area_scale);
        let downstream = neighbors8(index, grid)
            .filter(|&neighbor| {
                fill[neighbor] < fill[index]
                    && !crosses_selected_diagonal(index, neighbor, &target, grid)
            })
            .min_by_key(|&neighbor| (fill[neighbor], neighbor));
        if let Some(downstream) = downstream {
            target[index] = downstream as u32;
            accumulation[downstream] = accumulation[downstream].saturating_add(accumulation[index]);
        }
    }
    (target, accumulation)
}

fn crosses_selected_diagonal(index: usize, neighbor: usize, target: &[u32], grid: usize) -> bool {
    let x = index % grid;
    let y = index / grid;
    let neighbor_x = neighbor % grid;
    let neighbor_y = neighbor / grid;
    if x == neighbor_x || y == neighbor_y {
        return false;
    }
    let other_a = y * grid + neighbor_x;
    let other_b = neighbor_y * grid + x;
    target[other_a] == other_b as u32 || target[other_b] == other_a as u32
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
            |(_, from, downstream)| (from, false, fill[downstream]),
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
    lake_ids: &[u32],
    lakes: &[LakeDescriptor],
    grid: usize,
    step: i64,
    source_flow_threshold: u64,
) -> (Vec<ChannelLink>, Vec<DrainageSegment>, Vec<RiverSource>) {
    let mut order: Vec<u32> = (0..elevation.len() as u32).collect();
    order.sort_unstable_by_key(|&index| (Reverse(fill[index as usize]), index));
    let (selected, sources) = select_lake_fed_channels(
        elevation,
        target,
        accumulation,
        basin_ids,
        lake_ids,
        lakes,
        grid,
        step,
        source_flow_threshold,
    );
    let mut channel_ids = vec![0_u32; elevation.len()];
    let mut strongest_flow = vec![0_u64; elevation.len()];
    let mut primary_upstream = vec![NO_TARGET; elevation.len()];
    let stream_orders = derive_stream_orders(target, &selected, &order);
    for &index in &order {
        let index = index as usize;
        if !selected[index] || target[index] == NO_TARGET {
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
            primary_upstream[downstream] = index as u32;
        }
    }

    let subdivisions = (step / NODE_STEP) as usize;
    let mut channels = Vec::new();
    let mut rivers = Vec::new();
    let mut rebuild_straight = false;
    for from in 0..elevation.len() {
        if !selected[from] || elevation[from] <= SEA_LEVEL || target[from] == NO_TARGET {
            continue;
        }
        let stream_order = stream_orders[from];
        let to = target[from] as usize;
        if lake_ids[from] != 0 && lake_ids[from] == lake_ids[to] {
            continue;
        }
        let outlet = if elevation[to] <= SEA_LEVEL {
            ChannelOutlet::Ocean
        } else if lake_ids[to] != 0 {
            ChannelOutlet::Lake
        } else if is_border(to, grid) {
            ChannelOutlet::WorldEdge
        } else if selected[to] && target[to] != NO_TARGET {
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
        debug_assert_ne!(stream_order, 0);
        channels.push(ChannelLink {
            from: from as u32,
            to: to as u32,
            flow,
            channel_id,
            basin_id: basin_ids[from],
            water_body_id: lake_ids[to],
            outlet,
            stream_order,
        });
        let half_width = channel_half_width(accumulation[from], stream_order);
        let start = node_position(from, grid, step);
        let end = node_position(to, grid, step);
        let previous = (primary_upstream[from] != NO_TARGET).then(|| {
            node_position(primary_upstream[from] as usize, grid, step)
        });
        let next = (selected[to] && target[to] != NO_TARGET)
            .then(|| node_position(target[to] as usize, grid, step));
        let refined = [(true, 100), (true, 50), (false, 0)]
            .into_iter()
            .map(|(smooth, strength)| {
                refine_channel_link(
                    seed,
                    previous,
                    start,
                    end,
                    next,
                    fill[from],
                    fill[to],
                    subdivisions,
                    smooth,
                    strength,
                    channel_id,
                    half_width,
                    stream_order,
                )
            })
            .find(|candidate| !refinement_collides(candidate, &rivers, start, end))
            .unwrap_or_else(|| {
                rebuild_straight = true;
                refine_channel_link(
                    seed,
                    None,
                    start,
                    end,
                    None,
                    fill[from],
                    fill[to],
                    subdivisions,
                    false,
                    0,
                    channel_id,
                    half_width,
                    stream_order,
                )
            });
        rivers.extend(refined);
    }
    if rebuild_straight {
        rivers.clear();
        for link in &channels {
            let from = link.from as usize;
            let to = link.to as usize;
            rivers.extend(refine_channel_link(
                seed,
                None,
                node_position(from, grid, step),
                node_position(to, grid, step),
                None,
                fill[from],
                fill[to],
                subdivisions,
                false,
                0,
                link.channel_id,
                channel_half_width(u64::from(link.flow), link.stream_order),
                link.stream_order,
            ));
        }
    }
    debug_assert!(channels.iter().all(|link| {
        sources
            .iter()
            .any(|source| source.node.saturating_add(1) == link.channel_id)
    }));
    (channels, rivers, sources)
}

fn channel_half_width(flow: u64, stream_order: u8) -> u8 {
    let local_width = 2 + u64::from(stream_order) + (flow / 260_000).isqrt();
    let major_width = 3 + (flow / 130_000).isqrt();
    local_width
        .max(if flow >= MAJOR_RIVER_FLOW_THRESHOLD {
            major_width
        } else {
            0
        })
        .min(RIVER_MAX_HALF_WIDTH as u64) as u8
}

#[allow(clippy::too_many_arguments)]
fn refine_channel_link(
    seed: u64,
    previous_node: Option<(i64, i64)>,
    start: (i64, i64),
    end: (i64, i64),
    next_node: Option<(i64, i64)>,
    surface_start: i32,
    surface_end: i32,
    subdivisions: usize,
    smooth: bool,
    strength: i64,
    channel_id: u32,
    half_width: u8,
    stream_order: u8,
) -> Vec<DrainageSegment> {
    let mut segments = Vec::with_capacity(subdivisions);
    let mut previous = channel_point_with_strength(
        seed,
        previous_node,
        start,
        end,
        next_node,
        0,
        subdivisions,
        smooth,
        strength,
    );
    let mut previous_surface = channel_surface(surface_start, surface_end, 0, subdivisions);
    for part in 1..=subdivisions {
        let next = channel_point_with_strength(
            seed,
            previous_node,
            start,
            end,
            next_node,
            part,
            subdivisions,
            smooth,
            strength,
        );
        let next_surface = channel_surface(surface_start, surface_end, part, subdivisions);
        if next != previous {
            segments.push(DrainageSegment {
                ax: previous.0 as i32,
                ay: previous.1 as i32,
                bx: next.0 as i32,
                by: next.1 as i32,
                channel_id,
                surface_a: previous_surface,
                surface_b: next_surface,
                half_width,
                stream_order,
            });
        }
        previous = next;
        previous_surface = next_surface;
    }
    segments
}

fn refinement_collides(
    candidate: &[DrainageSegment],
    retained: &[DrainageSegment],
    start: (i64, i64),
    end: (i64, i64),
) -> bool {
    for offset in 0..candidate.len().saturating_sub(2) {
        let left = candidate[offset];
        if candidate[offset + 2..]
            .iter()
            .any(|&right| segments_intersect(left, right))
        {
            return true;
        }
    }
    let allowed_endpoint = |point: (i32, i32)| {
        point == (start.0 as i32, start.1 as i32) || point == (end.0 as i32, end.1 as i32)
    };
    candidate.iter().any(|&left| {
        retained.iter().any(|&right| {
            if !segments_intersect(left, right) {
                return false;
            }
            let (count, point) = shared_endpoint(left, right);
            count != 1 || !allowed_endpoint(point)
        })
    })
}

#[allow(clippy::too_many_arguments)]
fn select_lake_fed_channels(
    elevation: &[i32],
    target: &[u32],
    accumulation: &[u64],
    basin_ids: &[u32],
    lake_ids: &[u32],
    lakes: &[LakeDescriptor],
    grid: usize,
    step: i64,
    source_flow_threshold: u64,
) -> (Vec<bool>, Vec<RiverSource>) {
    let mut lake_sizes = vec![0_u16; lake_ids.len()];
    for &lake_id in lake_ids {
        if lake_id != 0 {
            lake_sizes[lake_id as usize - 1] = lake_sizes[lake_id as usize - 1].saturating_add(1);
        }
    }

    let mut candidates = lakes
        .iter()
        .filter_map(|lake| {
            if lake.terminal {
                return None;
            }
            let from = lake.outlet_node as usize;
            if accumulation[from] < source_flow_threshold {
                return None;
            }
            let to = target[from];
            debug_assert_ne!(to, NO_TARGET);
            debug_assert_eq!(lake_ids[from], lake.id);
            debug_assert_ne!(lake_ids[to as usize], lake.id);
            drains_to_open_water(to, target, elevation, grid).then_some((
                from,
                lake.id,
                lake_sizes[lake.id as usize - 1],
                accumulation[from],
                elevation[from],
                basin_ids[from],
            ))
        })
        .collect::<Vec<_>>();
    candidates.sort_unstable_by_key(|&(from, _, lake_size, flow, height, basin)| {
        (
            Reverse(lake_size),
            Reverse(flow),
            Reverse(height),
            basin,
            from,
        )
    });

    let mut selected = vec![false; target.len()];
    let mut sources = Vec::with_capacity(MAX_RIVER_SOURCES);
    for (from, lake_id, _, _, _, _) in candidates {
        if selected[from] {
            continue;
        }
        let position = node_position(from, grid, step);
        if sources.iter().any(|source: &RiverSource| {
            let other = node_position(source.node as usize, grid, step);
            let dx = position.0 - other.0;
            let dy = position.1 - other.1;
            dx * dx + dy * dy < RIVER_SOURCE_SPACING * RIVER_SOURCE_SPACING
        }) {
            continue;
        }
        let mut current = from;
        while target[current] != NO_TARGET && elevation[current] > SEA_LEVEL {
            selected[current] = true;
            current = target[current] as usize;
        }
        sources.push(RiverSource {
            node: from as u32,
            lake_id,
        });
        if sources.len() == MAX_RIVER_SOURCES {
            break;
        }
    }
    (selected, sources)
}

fn drains_to_open_water(start: u32, target: &[u32], elevation: &[i32], grid: usize) -> bool {
    let mut current = start;
    for _ in 0..target.len() {
        if current == NO_TARGET {
            return false;
        }
        let index = current as usize;
        if elevation[index] <= SEA_LEVEL || is_border(index, grid) {
            return true;
        }
        current = target[index];
    }
    false
}

fn derive_stream_orders(target: &[u32], selected: &[bool], order: &[u32]) -> Vec<u8> {
    let mut incoming_order = vec![0_u8; target.len()];
    let mut equal_order_inputs = vec![0_u8; target.len()];
    let mut stream_orders = vec![0_u8; target.len()];
    for &index in order {
        let index = index as usize;
        if !selected[index] || target[index] == NO_TARGET {
            continue;
        }
        let stream_order = if incoming_order[index] == 0 {
            1
        } else if equal_order_inputs[index] >= 2 {
            incoming_order[index].saturating_add(1)
        } else {
            incoming_order[index]
        };
        stream_orders[index] = stream_order;
        let downstream = target[index] as usize;
        match stream_order.cmp(&incoming_order[downstream]) {
            std::cmp::Ordering::Greater => {
                incoming_order[downstream] = stream_order;
                equal_order_inputs[downstream] = 1;
            }
            std::cmp::Ordering::Equal => {
                equal_order_inputs[downstream] = equal_order_inputs[downstream].saturating_add(1);
            }
            std::cmp::Ordering::Less => {}
        }
    }
    stream_orders
}

fn channel_surface(from: i32, to: i32, part: usize, subdivisions: usize) -> u16 {
    let part = part as i64;
    let subdivisions = subdivisions as i64;
    ((i64::from(from) * (subdivisions - part) + i64::from(to) * part) / subdivisions
        - i64::from(LAKE_MIN_DEPTH))
    .clamp(0, i64::from(u16::MAX)) as u16
}

fn segments_intersect(left: DrainageSegment, right: DrainageSegment) -> bool {
    let orient = |ax: i32, ay: i32, bx: i32, by: i32, cx: i32, cy: i32| {
        let abx = i128::from(bx) - i128::from(ax);
        let aby = i128::from(by) - i128::from(ay);
        let acx = i128::from(cx) - i128::from(ax);
        let acy = i128::from(cy) - i128::from(ay);
        abx * acy - aby * acx
    };
    let a = orient(left.ax, left.ay, left.bx, left.by, right.ax, right.ay);
    let b = orient(left.ax, left.ay, left.bx, left.by, right.bx, right.by);
    let c = orient(right.ax, right.ay, right.bx, right.by, left.ax, left.ay);
    let d = orient(right.ax, right.ay, right.bx, right.by, left.bx, left.by);
    let on_segment = |ax: i32, ay: i32, bx: i32, by: i32, px: i32, py: i32| {
        px >= ax.min(bx) && px <= ax.max(bx) && py >= ay.min(by) && py <= ay.max(by)
    };
    (a.signum() != b.signum() && c.signum() != d.signum())
        || (a == 0 && on_segment(left.ax, left.ay, left.bx, left.by, right.ax, right.ay))
        || (b == 0 && on_segment(left.ax, left.ay, left.bx, left.by, right.bx, right.by))
        || (c == 0 && on_segment(right.ax, right.ay, right.bx, right.by, left.ax, left.ay))
        || (d == 0 && on_segment(right.ax, right.ay, right.bx, right.by, left.bx, left.by))
}

fn shared_endpoint(left: DrainageSegment, right: DrainageSegment) -> (usize, (i32, i32)) {
    let mut count = 0;
    let mut shared = (0, 0);
    for point in [(left.ax, left.ay), (left.bx, left.by)] {
        if point == (right.ax, right.ay) || point == (right.bx, right.by) {
            count += 1;
            shared = point;
        }
    }
    (count, shared)
}

#[cfg(test)]
fn channel_point(
    seed: u64,
    start: (i64, i64),
    end: (i64, i64),
    part: usize,
    subdivisions: usize,
) -> (i64, i64) {
    channel_point_with_strength(seed, start, end, part, subdivisions, 100)
}

fn channel_point_with_strength(
    seed: u64,
    start: (i64, i64),
    end: (i64, i64),
    part: usize,
    subdivisions: usize,
    strength: i64,
) -> (i64, i64) {
    if part == 0 {
        return start;
    }
    if part == subdivisions {
        return end;
    }
    let remaining = (subdivisions - part) as i64;
    let part = part as i64;
    let divisor = subdivisions as i64;
    let base_x = (start.0 * remaining + end.0 * part).div_euclid(divisor);
    let base_y = (start.1 * remaining + end.1 * part).div_euclid(divisor);
    let curve_bits = super::noise::hash(
        seed ^ RIVER_JITTER_SEED,
        start.0.saturating_add(end.0),
        start.1.saturating_add(end.1),
    );
    let primary = (curve_bits % 97) as i64 - 48;
    let inflection = ((curve_bits >> 8) % 65) as i64 - 32;
    let shape = primary + inflection * (part * 2 - divisor) / divisor;
    let envelope = 4 * part * (divisor - part);
    let lateral = shape * envelope * strength / (divisor * divisor * 100);
    let dx = end.0 - start.0;
    let dy = end.1 - start.1;
    let scale = dx.abs().max(dy.abs()).max(1);
    let detail_bits = super::noise::hash(seed ^ RIVER_JITTER_SEED.rotate_left(17), base_x, base_y);
    let jitter_x = ((detail_bits % 13) as i64 - 6) * strength / 100;
    let jitter_y = (((detail_bits >> 8) % 13) as i64 - 6) * strength / 100;
    let x = base_x - dy * lateral / scale + jitter_x;
    let y = base_y + dx * lateral / scale + jitter_y;
    (
        x.clamp(WORLD_GENERATION_BOUNDS.min.x, WORLD_GENERATION_BOUNDS.max.x),
        y.clamp(WORLD_GENERATION_BOUNDS.min.y, WORLD_GENERATION_BOUNDS.max.y),
    )
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};
    use std::time::Instant;

    use rayon::ThreadPoolBuilder;

    use super::*;

    #[test]
    fn drainage_records_have_bounded_layouts() {
        assert_eq!(size_of::<DrainageSegment>(), 28);
        assert_eq!(size_of::<ChannelLink>(), 28);
        assert_eq!(size_of::<LakeDescriptor>(), 12);
        assert_eq!(size_of::<RiverSource>(), 8);
    }

    #[test]
    fn strahler_order_increases_only_for_equal_order_confluences() {
        // 0 and 1 join at 2, then the order-2 branch meets one order-1 branch
        // at 4. The unequal confluence must remain order 2.
        let target = [2, 2, 4, 4, 5, NO_TARGET];
        let selected = [true, true, true, true, true, false];
        let order = [0, 1, 2, 3, 4, 5];
        let stream_orders = derive_stream_orders(&target, &selected, &order);
        assert_eq!(stream_orders, [1, 1, 2, 1, 2, 0]);
    }

    #[test]
    fn arid_cells_do_not_create_perennial_runoff() {
        let elevation = [
            40_009, 40_008, 40_007, 40_006, 40_005, 40_004, 40_003, 40_002, 40_001,
        ];
        let moisture = [RUNOFF_MOISTURE_FLOOR; 9];
        let (_, accumulation) = route_flow(&elevation, &elevation, &moisture, 3, SKELETON_STEP);
        assert_eq!(accumulation, [0; 9]);

        let wet = [RUNOFF_MOISTURE_FLOOR + 9_600; 9];
        let (_, accumulation) = route_flow(&elevation, &elevation, &wet, 3, SKELETON_STEP);
        assert!(accumulation.into_iter().any(|flow| flow > 0));
    }

    #[test]
    fn segment_intersection_includes_overlap_and_endpoint_touches() {
        let segment = |ax, ay, bx, by| DrainageSegment {
            ax,
            ay,
            bx,
            by,
            channel_id: 1,
            surface_a: 40_000,
            surface_b: 39_000,
            half_width: 1,
            stream_order: 1,
        };
        let horizontal = segment(0, 0, 10, 0);
        assert!(segments_intersect(horizontal, segment(5, 0, 15, 0)));
        assert!(segments_intersect(horizontal, segment(10, 0, 10, 5)));
        assert!(segments_intersect(horizontal, segment(5, -5, 5, 5)));
        assert!(!segments_intersect(horizontal, segment(11, 0, 15, 0)));
    }

    #[test]
    fn retained_channels_do_not_cross_outside_shared_endpoints() {
        for seed in [1, 7, 42, 10_001] {
            let drainage = DrainageSkeleton::build(seed, SKELETON_STEP);
            let subdivisions = (drainage.step / NODE_STEP) as usize;
            let mut node_channels = BTreeMap::<u32, Vec<(u32, (i64, i64))>>::new();
            for link in &drainage.channels {
                let start = node_position(link.from as usize, drainage.grid, drainage.step);
                let end = node_position(link.to as usize, drainage.grid, drainage.step);
                node_channels.entry(link.from).or_default().push((
                    link.channel_id,
                    channel_point(seed, start, end, 0, subdivisions),
                ));
                node_channels.entry(link.to).or_default().push((
                    link.channel_id,
                    channel_point(seed, start, end, subdivisions, subdivisions),
                ));
            }
            let mut canonical_joins = BTreeSet::new();
            for node_channels in node_channels.into_values() {
                for (offset, &(left, point)) in node_channels.iter().enumerate() {
                    for &(right, other_point) in &node_channels[offset + 1..] {
                        assert_eq!(point, other_point, "shared nodes must refine identically");
                        canonical_joins.insert((
                            left.min(right),
                            left.max(right),
                            point.0 as i32,
                            point.1 as i32,
                        ));
                    }
                }
            }
            let mut bins = BTreeMap::<(i64, i64), Vec<usize>>::new();
            for (index, segment) in drainage.rivers.iter().enumerate() {
                let min_x = i64::from(segment.ax.min(segment.bx)).div_euclid(64);
                let max_x = i64::from(segment.ax.max(segment.bx)).div_euclid(64);
                let min_y = i64::from(segment.ay.min(segment.by)).div_euclid(64);
                let max_y = i64::from(segment.ay.max(segment.by)).div_euclid(64);
                for y in min_y..=max_y {
                    for x in min_x..=max_x {
                        bins.entry((x, y)).or_default().push(index);
                    }
                }
            }
            let mut compared = BTreeSet::new();
            for indices in bins.values() {
                for (offset, &left) in indices.iter().enumerate() {
                    for &right in &indices[offset + 1..] {
                        let pair = (left.min(right), left.max(right));
                        if !compared.insert(pair) {
                            continue;
                        }
                        let left = drainage.rivers[pair.0];
                        let right = drainage.rivers[pair.1];
                        if !segments_intersect(left, right) {
                            continue;
                        }
                        let (shared_count, shared) = shared_endpoint(left, right);
                        let channel_pair = (
                            left.channel_id.min(right.channel_id),
                            left.channel_id.max(right.channel_id),
                            shared.0,
                            shared.1,
                        );
                        let allowed_join = shared_count == 1
                            && ((left.channel_id == right.channel_id && pair.1 == pair.0 + 1)
                                || canonical_joins.contains(&channel_pair));
                        if !allowed_join {
                            panic!(
                                "seed {seed} segments intersect outside their routed polyline: {left:?} and {right:?}"
                            );
                        }
                    }
                }
            }
        }
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
        let mut observed_max = 0;
        let mut source_count = 0;
        for seed in [1, 7, 42, 10_001] {
            let drainage = DrainageSkeleton::build(seed, SKELETON_STEP);
            source_count += drainage.sources.len();
            assert!(!drainage.sources.is_empty());
            assert!(drainage.sources.len() <= MAX_RIVER_SOURCES);
            assert!(drainage.channels.len() < 600);
            for (offset, source) in drainage.sources.iter().enumerate() {
                assert!(drainage.channels.iter().any(|link| {
                    link.from == source.node && u64::from(link.flow) >= RIVER_SOURCE_FLOW_THRESHOLD
                }));
                assert!(
                    drainage
                        .lakes
                        .iter()
                        .any(|lake| {
                            lake.id == source.lake_id
                                && lake.outlet_node == source.node
                                && !lake.terminal
                        })
                );
                let point = node_position(source.node as usize, drainage.grid, drainage.step);
                for other in &drainage.sources[offset + 1..] {
                    let other = node_position(other.node as usize, drainage.grid, drainage.step);
                    let dx = point.0 - other.0;
                    let dy = point.1 - other.1;
                    assert!(
                        dx * dx + dy * dy >= RIVER_SOURCE_SPACING * RIVER_SOURCE_SPACING,
                        "river sources must remain spatially distinct"
                    );
                }
            }
            assert!(
                drainage
                    .rivers
                    .iter()
                    .all(|segment| segment.surface_b <= segment.surface_a)
            );
            let mut segments_per_chunk = BTreeMap::<(i64, i64), usize>::new();
            for segment in &drainage.rivers {
                let width = i64::from(segment.half_width) + FLOODPLAIN_RADIUS;
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
            observed_max = observed_max.max(maximum);
            assert!(
                maximum <= crate::worldgen::MAX_CHUNK_RIVERS,
                "seed {seed} needs {maximum} chunk river slots"
            );
            for link in &drainage.channels {
                checked += 1;
                assert_ne!(link.stream_order, 0);
                match link.outlet {
                    ChannelOutlet::Continues => {
                        let downstream = drainage
                            .channels
                            .iter()
                            .find(|candidate| candidate.from == link.to)
                            .expect("declared continuation must exist");
                        assert_eq!(downstream.channel_id, link.channel_id);
                        assert_eq!(downstream.basin_id, link.basin_id);
                        assert!(downstream.stream_order >= link.stream_order);
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
        assert!(source_count >= 16, "too few canonical lake-fed sources");
        assert!(
            observed_max <= crate::worldgen::MAX_CHUNK_RIVERS,
            "representative streams must fit the allocation-free chunk fast path"
        );
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
        let stream_flow_threshold = std::env::var("SIM_RIVER_SOURCE_FLOW_THRESHOLD")
            .ok()
            .map_or(RIVER_SOURCE_FLOW_THRESHOLD, |value| {
                value
                    .parse::<u64>()
                    .expect("SIM_RIVER_SOURCE_FLOW_THRESHOLD must be a positive integer")
            });
        let steps: &[i64] = requested.as_ref().map_or(&[128, 256], std::slice::from_ref);
        for &step in steps {
            let started = Instant::now();
            let drainage =
                DrainageSkeleton::build_with_threshold(seed, step, stream_flow_threshold);
            println!(
                "seed={seed} step={step} source_flow_threshold={stream_flow_threshold} grid={} sources={} channels={} segments={} lakes={} retained_bytes={} scratch_upper_bound_bytes={} elapsed_ms={:.1}",
                drainage.grid,
                drainage.sources.len(),
                drainage.channels.len(),
                drainage.rivers.len(),
                drainage.lakes.len(),
                drainage.retained_bytes(),
                drainage.scratch_upper_bound_bytes(),
                started.elapsed().as_secs_f64() * 1_000.0
            );
            for source in &drainage.sources {
                let (x, y) = node_position(source.node as usize, drainage.grid, drainage.step);
                println!(
                    "source lake={} node={} x={x} y={y}",
                    source.lake_id, source.node
                );
            }
        }
    }
}
