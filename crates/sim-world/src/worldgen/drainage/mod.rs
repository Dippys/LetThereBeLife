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

use crate::{WORLD_GENERATION_BOUNDS, WORLD_SIDE_CELLS};

use super::climate::moisture;
use super::hydrology::{LAKE_MIN_DEPTH, NODE_STEP, REGION_SIZE};
use super::plates::{SEA_LEVEL, macro_sample};
use channels::extract_channels;
use lattice::{
    identify_basins, identify_lakes, moisture_lattice, node_position, priority_fill, route_flow,
};

mod channels;
mod lattice;

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
                    lake.id == source.lake_id && lake.outlet_node == source.node && !lake.terminal
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

#[cfg(test)]
mod tests;
