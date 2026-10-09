//! Multi-resolution terrain summary cache: per-chunk visual sampling, summary instances, and cache-sync decisions.

use std::collections::BTreeMap;

use rayon::prelude::*;
use sim_core::{
    BiomeType, CHUNK_SIZE, ChunkCoord, FeatureKind, SurfaceType, TerrainCell, World, WorldOverview,
    WorldPosition, WorldRect,
};

use super::{
    CACHE_MARGIN_PIXELS, MIN_TERRAIN_SAMPLE_PIXELS,
    colors::{summary_feature_color, terrain_color},
    gpu::Instance,
    instances::{build_exact_world_instances, rect_instance},
};

#[cfg(test)]
pub(super) fn build_world_instances(
    world: &World,
    bounds: WorldRect,
    step: u32,
) -> (Vec<Instance>, Vec<Instance>) {
    let mut summaries = WorldSummaryCache::default();
    summaries.sync(world, None, bounds, step, None)
}

#[derive(Default)]
pub(super) struct WorldSummaryCache {
    pub(super) step: u32,
    pub(super) chunks: BTreeMap<ChunkCoord, ChunkRenderSummary>,
}

impl WorldSummaryCache {
    pub(super) fn sync(
        &mut self,
        world: &World,
        overview: Option<&WorldOverview>,
        bounds: WorldRect,
        step: u32,
        changed_bounds: Option<WorldRect>,
    ) -> (Vec<Instance>, Vec<Instance>) {
        if step == 1 {
            self.step = 1;
            self.chunks.clear();
            return build_exact_world_instances(world, bounds);
        }
        debug_assert!(step.is_power_of_two() && step <= CHUNK_SIZE as u32);
        if self.step != step {
            self.step = step;
            self.chunks.clear();
        }
        self.chunks.retain(|coord, _| {
            coord
                .bounds()
                .is_ok_and(|chunk_bounds| chunk_bounds.intersects(bounds))
        });
        if let Some(changed) = changed_bounds {
            self.chunks.retain(|coord, _| {
                coord
                    .bounds()
                    .is_ok_and(|chunk_bounds| !chunk_bounds.intersects(changed))
            });
        }

        let mut missing = Vec::new();
        world.visit_loaded_regions_in(bounds, |coord, coverage| {
            if !self.chunks.contains_key(&coord) {
                missing.push((coord, coverage));
            }
        });
        let built: Vec<_> = missing
            .into_par_iter()
            .map(|(coord, coverage)| (coord, build_chunk_summary(world, coord, coverage, step)))
            .collect();
        self.chunks.extend(built);

        let terrain_len = self
            .chunks
            .values()
            .map(|summary| summary.terrain.len())
            .sum();
        let feature_len = self
            .chunks
            .values()
            .map(|summary| summary.features.len())
            .sum();
        let mut terrain = Vec::with_capacity(terrain_len);
        let mut features = Vec::with_capacity(feature_len);
        for summary in self.chunks.values() {
            terrain.extend_from_slice(&summary.terrain);
            features.extend_from_slice(&summary.features);
        }
        if let Some(overview) = overview {
            let overview_chunks = overview.chunk_count_in(bounds);
            terrain.reserve(overview_chunks.saturating_mul(2));
            features.reserve(overview_chunks);
            overview.visit_chunks_in(bounds, |coord, chunk| {
                if self.chunks.contains_key(&coord) {
                    return;
                }
                let block = coord
                    .bounds()
                    .expect("archive overview coordinates are world-valid");
                terrain.push(rect_instance(block, terrain_color(chunk.base())));
                if let (Some(detail), Some(detail_bounds)) =
                    (chunk.detail(), chunk.detail_bounds(coord))
                {
                    terrain.push(rect_instance(
                        visible_detail_bounds(detail_bounds, block),
                        terrain_color(detail),
                    ));
                }
                if let Some((kind, count)) = chunk.feature() {
                    let density = f32::from(count) / (CHUNK_SIZE * CHUNK_SIZE) as f32;
                    let fraction = (0.2 + density.sqrt() * 1.6).clamp(0.25, 0.8);
                    let size = CHUNK_SIZE as f32 * fraction;
                    let inset = (CHUNK_SIZE as f32 - size) * 0.5;
                    features.push(Instance::new(
                        block.min.x as f32 + inset,
                        block.min.y as f32 + inset,
                        size,
                        size,
                        summary_feature_color(kind),
                    ));
                }
            });
        }
        (terrain, features)
    }

    pub(super) fn logical_bytes(&self) -> usize {
        self.chunks.values().fold(0, |bytes, summary| {
            bytes
                + summary.terrain.capacity() * size_of::<Instance>()
                + summary.features.capacity() * size_of::<Instance>()
        })
    }
}

pub(super) struct ChunkRenderSummary {
    terrain: Vec<Instance>,
    features: Vec<Instance>,
}

const TERRAIN_VISUAL_COUNT: usize = 15;
const OCEAN_DEEP: usize = 0;
pub(super) const OCEAN_SHALLOW: usize = 1;
pub(super) const LAKE_VISUAL: usize = 2;
pub(super) const RIVER_VISUAL: usize = 3;
const BEACH_VISUAL: usize = 4;
const DESERT_VISUAL: usize = 5;
pub(super) const GRASS_VISUAL: usize = 6;
const SAVANNA_VISUAL: usize = 7;
pub(super) const FOREST_VISUAL: usize = 8;
const WETLAND_VISUAL: usize = 9;
const TUNDRA_VISUAL: usize = 10;
const HILL_VISUAL: usize = 11;
const ROCK_VISUAL: usize = 12;
pub(super) const SNOW_VISUAL: usize = 13;
const FALLBACK_VISUAL: usize = 14;

#[derive(Clone, Copy, Default)]
pub(super) struct VisualSample {
    pub(super) count: u16,
    representative: Option<TerrainCell>,
    min_x: u8,
    min_y: u8,
    max_x: u8,
    max_y: u8,
}

impl VisualSample {
    fn observe(&mut self, position: WorldPosition, origin: WorldPosition, cell: TerrainCell) {
        let x = (position.x - origin.x) as u8;
        let y = (position.y - origin.y) as u8;
        if self.count == 0 {
            self.min_x = x;
            self.min_y = y;
            self.max_x = x + 1;
            self.max_y = y + 1;
        } else {
            self.min_x = self.min_x.min(x);
            self.min_y = self.min_y.min(y);
            self.max_x = self.max_x.max(x + 1);
            self.max_y = self.max_y.max(y + 1);
        }
        self.count = self.count.saturating_add(1);
        self.representative.get_or_insert(cell);
    }

    fn world_bounds(self, origin: WorldPosition) -> WorldRect {
        debug_assert!(self.count > 0);
        WorldRect {
            min: WorldPosition {
                x: origin.x + i64::from(self.min_x),
                y: origin.y + i64::from(self.min_y),
            },
            max: WorldPosition {
                x: origin.x + i64::from(self.max_x),
                y: origin.y + i64::from(self.max_y),
            },
        }
    }
}

#[derive(Clone)]
pub(super) struct SummaryAccumulator {
    pub(super) visuals: [VisualSample; TERRAIN_VISUAL_COUNT],
    pub(super) feature_counts: [u16; 3],
}

impl Default for SummaryAccumulator {
    fn default() -> Self {
        Self {
            visuals: [VisualSample::default(); TERRAIN_VISUAL_COUNT],
            feature_counts: [0; 3],
        }
    }
}

impl SummaryAccumulator {
    pub(super) fn observe_cell(
        &mut self,
        position: WorldPosition,
        origin: WorldPosition,
        cell: TerrainCell,
    ) {
        self.visuals[terrain_visual(cell)].observe(position, origin, cell);
    }

    fn observe_feature(&mut self, kind: FeatureKind) {
        let index = match kind {
            FeatureKind::Tree => 0,
            FeatureKind::Rock => 1,
            FeatureKind::BerryBush => 2,
        };
        self.feature_counts[index] = self.feature_counts[index].saturating_add(1);
    }

    pub(super) fn instances(
        &self,
        block: WorldRect,
        chunk_origin: WorldPosition,
        terrain: &mut Vec<Instance>,
        features: &mut Vec<Instance>,
    ) {
        let Some(base_index) = self.base_visual() else {
            return;
        };
        let base = self.visuals[base_index]
            .representative
            .expect("observed visual retains a representative cell");
        terrain.push(rect_instance(block, terrain_color(base)));
        if let Some(detail_index) = self.detail_visual(base_index) {
            let detail = self.visuals[detail_index];
            let detail_cell = detail
                .representative
                .expect("observed detail retains a representative cell");
            terrain.push(rect_instance(
                visible_detail_bounds(detail.world_bounds(chunk_origin), block),
                terrain_color(detail_cell),
            ));
        }

        let total_features: u16 = self.feature_counts.iter().copied().sum();
        if total_features == 0 {
            return;
        }
        let feature_index = self
            .feature_counts
            .iter()
            .enumerate()
            .max_by_key(|&(index, count)| (*count, std::cmp::Reverse(index)))
            .map(|(index, _)| index)
            .expect("fixed feature count array is nonempty");
        let area = ((block.max.x - block.min.x) * (block.max.y - block.min.y)).max(1) as f32;
        let density = f32::from(total_features) / area;
        let fraction = (0.2 + density.sqrt() * 1.6).clamp(0.25, 0.8);
        let width = ((block.max.x - block.min.x) as f32 * fraction).max(1.0);
        let height = ((block.max.y - block.min.y) as f32 * fraction).max(1.0);
        let x = block.min.x as f32 + ((block.max.x - block.min.x) as f32 - width) * 0.5;
        let y = block.min.y as f32 + ((block.max.y - block.min.y) as f32 - height) * 0.5;
        let kind = [FeatureKind::Tree, FeatureKind::Rock, FeatureKind::BerryBush][feature_index];
        features.push(Instance::new(
            x,
            y,
            width,
            height,
            summary_feature_color(kind),
        ));
    }

    pub(super) fn base_visual(&self) -> Option<usize> {
        let dominant = |indices: &[usize]| {
            indices
                .iter()
                .copied()
                .filter(|&index| self.visuals[index].count > 0)
                .max_by_key(|&index| (self.visuals[index].count, std::cmp::Reverse(index)))
        };
        let ordinary = [
            OCEAN_DEEP,
            OCEAN_SHALLOW,
            BEACH_VISUAL,
            DESERT_VISUAL,
            GRASS_VISUAL,
            SAVANNA_VISUAL,
            FOREST_VISUAL,
            WETLAND_VISUAL,
            TUNDRA_VISUAL,
            HILL_VISUAL,
            ROCK_VISUAL,
            SNOW_VISUAL,
            FALLBACK_VISUAL,
        ];
        dominant(&ordinary).or_else(|| dominant(&[LAKE_VISUAL, RIVER_VISUAL]))
    }

    pub(super) fn detail_visual(&self, base: usize) -> Option<usize> {
        for index in [RIVER_VISUAL, LAKE_VISUAL] {
            if index != base && self.visuals[index].count > 0 {
                return Some(index);
            }
        }
        let ocean = self.visuals[OCEAN_DEEP].count + self.visuals[OCEAN_SHALLOW].count;
        let land: u16 = self.visuals[BEACH_VISUAL..]
            .iter()
            .map(|sample| sample.count)
            .sum();
        if ocean > 0 && land > 0 {
            if base == OCEAN_DEEP || base == OCEAN_SHALLOW {
                return (BEACH_VISUAL..TERRAIN_VISUAL_COUNT)
                    .filter(|&index| self.visuals[index].count > 0)
                    .max_by_key(|&index| (self.visuals[index].count, std::cmp::Reverse(index)));
            }
            return [OCEAN_DEEP, OCEAN_SHALLOW]
                .into_iter()
                .filter(|&index| self.visuals[index].count > 0)
                .max_by_key(|&index| (self.visuals[index].count, std::cmp::Reverse(index)));
        }
        [SNOW_VISUAL, ROCK_VISUAL, HILL_VISUAL]
            .into_iter()
            .find(|&index| index != base && self.visuals[index].count > 0)
    }
}

fn build_chunk_summary(
    world: &World,
    coord: ChunkCoord,
    coverage: WorldRect,
    step: u32,
) -> ChunkRenderSummary {
    let chunk_bounds = coord
        .bounds()
        .expect("resident chunks always have representable bounds");
    let blocks_per_axis = CHUNK_SIZE as usize / step as usize;
    let mut blocks = vec![SummaryAccumulator::default(); blocks_per_axis * blocks_per_axis];
    let block_index = |position: WorldPosition| {
        let x = (position.x - chunk_bounds.min.x) as usize / step as usize;
        let y = (position.y - chunk_bounds.min.y) as usize / step as usize;
        y * blocks_per_axis + x
    };
    assert_eq!(
        world.visit_cells_in_chunk(coord, |position, cell| {
            blocks[block_index(position)].observe_cell(position, chunk_bounds.min, cell);
        }),
        Some(coverage)
    );
    assert_eq!(
        world.visit_features_in_chunk(coord, |feature| {
            blocks[block_index(feature.position)].observe_feature(feature.kind);
        }),
        Some(coverage)
    );

    let mut terrain = Vec::with_capacity(blocks.len() * 2);
    let mut features = Vec::with_capacity(blocks.len());
    for (index, block) in blocks.iter().enumerate() {
        let x = index % blocks_per_axis;
        let y = index / blocks_per_axis;
        let block_min = WorldPosition {
            x: chunk_bounds.min.x + (x * step as usize) as i64,
            y: chunk_bounds.min.y + (y * step as usize) as i64,
        };
        let block_bounds = WorldRect {
            min: block_min,
            max: WorldPosition {
                x: block_min.x + i64::from(step),
                y: block_min.y + i64::from(step),
            },
        };
        if let Some(clipped) = block_bounds.intersection(coverage) {
            block.instances(clipped, chunk_bounds.min, &mut terrain, &mut features);
        }
    }
    terrain.shrink_to_fit();
    features.shrink_to_fit();
    ChunkRenderSummary { terrain, features }
}

fn terrain_visual(cell: TerrainCell) -> usize {
    match (cell.surface(), cell.biome()) {
        (SurfaceType::DeepWater, BiomeType::Ocean) => OCEAN_DEEP,
        (SurfaceType::ShallowWater, BiomeType::Ocean) => OCEAN_SHALLOW,
        (_, BiomeType::Lake) => LAKE_VISUAL,
        (_, BiomeType::River) => RIVER_VISUAL,
        (SurfaceType::Sand, BiomeType::Beach) => BEACH_VISUAL,
        (SurfaceType::Sand, BiomeType::Desert) => DESERT_VISUAL,
        (SurfaceType::Soil, BiomeType::Grassland) => GRASS_VISUAL,
        (SurfaceType::Soil, BiomeType::Savanna) => SAVANNA_VISUAL,
        (SurfaceType::Soil, BiomeType::Forest) => FOREST_VISUAL,
        (SurfaceType::Soil, BiomeType::Wetland) => WETLAND_VISUAL,
        (_, BiomeType::Tundra) => TUNDRA_VISUAL,
        (SurfaceType::Hill, _) => HILL_VISUAL,
        (SurfaceType::Rock, _) => ROCK_VISUAL,
        (SurfaceType::SnowIce, _) => SNOW_VISUAL,
        _ => FALLBACK_VISUAL,
    }
}

fn visible_detail_bounds(detail: WorldRect, block: WorldRect) -> WorldRect {
    let minimum = ((block.max.x - block.min.x).min(block.max.y - block.min.y) / 4).max(1);
    let inflate_axis = |min: i64, max: i64, block_min: i64, block_max: i64| {
        let missing = minimum.saturating_sub(max - min);
        let before = missing / 2;
        let after = missing - before;
        ((min - before).max(block_min), (max + after).min(block_max))
    };
    let (min_x, max_x) = inflate_axis(detail.min.x, detail.max.x, block.min.x, block.max.x);
    let (min_y, max_y) = inflate_axis(detail.min.y, detail.max.y, block.min.y, block.max.y);
    WorldRect {
        min: WorldPosition { x: min_x, y: min_y },
        max: WorldPosition { x: max_x, y: max_y },
    }
}

pub(super) fn terrain_sample_step(scale: f32) -> u32 {
    let requested = (MIN_TERRAIN_SAMPLE_PIXELS / scale.max(f32::EPSILON))
        .ceil()
        .max(1.0) as u32;
    requested.clamp(1, CHUNK_SIZE as u32).next_power_of_two()
}

pub(super) fn cache_margin(scale: f32) -> i64 {
    (CACHE_MARGIN_PIXELS / scale.max(f32::EPSILON))
        .ceil()
        .max(1.0) as i64
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CacheSyncAction {
    Rebuild,
    Skip,
    AdvanceRevision,
}

pub(super) fn cache_sync_action(
    cache_contains_view: bool,
    step_matches: bool,
    revision_changed: bool,
    allow_world_sync: bool,
    changed_affects_cache: bool,
) -> CacheSyncAction {
    if !cache_contains_view || !step_matches {
        return CacheSyncAction::Rebuild;
    }
    if !revision_changed || !changed_affects_cache {
        return if revision_changed {
            CacheSyncAction::AdvanceRevision
        } else {
            CacheSyncAction::Skip
        };
    }
    if allow_world_sync {
        CacheSyncAction::Rebuild
    } else {
        CacheSyncAction::Skip
    }
}
