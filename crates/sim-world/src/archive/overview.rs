//! Fixed-size chunk overviews: summarizing, encoding, decoding, and the
//! read-only `WorldOverview` view over an archive index.

use super::{
    NO_FEATURE, NO_TERRAIN_CLASS, WorldArchiveError, WorldOverview, archive_index, format::invalid,
};
use crate::{
    BiomeType, CHUNK_SIZE, ChunkCoord, FeatureKind, SurfaceType, TerrainCell, TerrainClass,
    WorldChunk, WorldPosition, WorldRect, chunk::chunk_origin,
};

/// Fixed 16-byte visual proxy for one complete 64 x 64 chunk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct ChunkOverview {
    base: TerrainCell,
    detail: TerrainCell,
    detail_bounds: [u8; 4],
    feature_count: u16,
    feature_kind: u8,
    reserved: u8,
}

impl ChunkOverview {
    pub const fn base(self) -> TerrainCell {
        self.base
    }

    pub fn detail(self) -> Option<TerrainCell> {
        (self.detail.classification().packed() != NO_TERRAIN_CLASS).then_some(self.detail)
    }

    pub fn detail_bounds(self, coord: ChunkCoord) -> Option<WorldRect> {
        self.detail()?;
        let origin = chunk_origin(coord);
        Some(WorldRect {
            min: WorldPosition {
                x: origin.x + i64::from(self.detail_bounds[0]),
                y: origin.y + i64::from(self.detail_bounds[1]),
            },
            max: WorldPosition {
                x: origin.x + i64::from(self.detail_bounds[2]),
                y: origin.y + i64::from(self.detail_bounds[3]),
            },
        })
    }

    pub fn feature(self) -> Option<(FeatureKind, u16)> {
        let kind = match self.feature_kind {
            0 => FeatureKind::Tree,
            1 => FeatureKind::Rock,
            2 => FeatureKind::BerryBush,
            3 => FeatureKind::BitterBush,
            _ => return None,
        };
        Some((kind, self.feature_count))
    }
}

const _: () = assert!(std::mem::size_of::<ChunkOverview>() == 16);

impl WorldOverview {
    pub fn chunk_count_in(&self, bounds: WorldRect) -> usize {
        let Some(bounds) = bounds.intersection(self.0.bounds) else {
            return 0;
        };
        let min = ChunkCoord::from_world_position(bounds.min);
        let max = ChunkCoord::from_world_position(WorldPosition {
            x: bounds.max.x - 1,
            y: bounds.max.y - 1,
        });
        ((max.x - min.x + 1) * (max.y - min.y + 1)) as usize
    }

    pub fn visit_chunks_in(
        &self,
        bounds: WorldRect,
        mut visitor: impl FnMut(ChunkCoord, ChunkOverview),
    ) {
        let Some(bounds) = bounds.intersection(self.0.bounds) else {
            return;
        };
        let min = ChunkCoord::from_world_position(bounds.min);
        let inclusive_max = WorldPosition {
            x: bounds.max.x - 1,
            y: bounds.max.y - 1,
        };
        let max = ChunkCoord::from_world_position(inclusive_max);
        for y in min.y..=max.y {
            for x in min.x..=max.x {
                let coord = ChunkCoord { x, y };
                if let Some(index) = archive_index(&self.0, coord) {
                    visitor(coord, self.0.overviews[index]);
                }
            }
        }
    }

    pub fn bounds(&self) -> WorldRect {
        self.0.bounds
    }

    pub fn logical_bytes(&self) -> usize {
        self.0.overviews.len() * std::mem::size_of::<ChunkOverview>()
    }
}

pub(super) fn summarize_chunk(chunk: &WorldChunk) -> ChunkOverview {
    let mut counts = [0_u16; 256];
    let mut representatives = [None; 256];
    let mut bounds = [[u8::MAX, u8::MAX, 0, 0]; 256];
    for (index, &cell) in chunk.terrain.iter().enumerate() {
        let class = cell.classification().packed() as usize;
        counts[class] = counts[class].saturating_add(1);
        representatives[class].get_or_insert(cell);
        let x = (index % CHUNK_SIZE as usize) as u8;
        let y = (index / CHUNK_SIZE as usize) as u8;
        bounds[class][0] = bounds[class][0].min(x);
        bounds[class][1] = bounds[class][1].min(y);
        bounds[class][2] = bounds[class][2].max(x + 1);
        bounds[class][3] = bounds[class][3].max(y + 1);
    }
    let base_index = counts
        .iter()
        .enumerate()
        .max_by_key(|&(index, count)| (*count, std::cmp::Reverse(index)))
        .map(|(index, _)| index)
        .unwrap();
    let detail_index = counts
        .iter()
        .enumerate()
        .filter(|&(index, count)| index != base_index && *count > 0)
        .max_by_key(|&(index, count)| {
            (
                overview_priority(index as u8),
                *count,
                std::cmp::Reverse(index),
            )
        })
        .map(|(index, _)| index);
    let mut feature_counts = [0_u16; 3];
    for feature in &chunk.features {
        feature_counts[feature.kind as usize] =
            feature_counts[feature.kind as usize].saturating_add(1);
    }
    let feature_kind = feature_counts
        .iter()
        .enumerate()
        .filter(|(_, count)| **count > 0)
        .max_by_key(|&(index, count)| (*count, std::cmp::Reverse(index)))
        .map_or(NO_FEATURE, |(index, _)| index as u8);
    ChunkOverview {
        base: representatives[base_index].unwrap(),
        detail: detail_index
            .and_then(|index| representatives[index])
            .unwrap_or(TerrainCell {
                elevation: 0,
                moisture: 0,
                class: TerrainClass(NO_TERRAIN_CLASS),
            }),
        detail_bounds: detail_index.map_or([0; 4], |index| bounds[index]),
        feature_count: feature_counts.iter().copied().sum(),
        feature_kind,
        reserved: 0,
    }
}

fn overview_priority(packed: u8) -> u8 {
    let Some(class) = TerrainClass::from_packed(packed) else {
        return 0;
    };
    match (class.surface(), class.biome()) {
        (_, BiomeType::River) => 6,
        (_, BiomeType::Lake) => 5,
        (SurfaceType::Sand, BiomeType::Beach) => 4,
        (SurfaceType::SnowIce, _) => 3,
        (SurfaceType::Rock | SurfaceType::Hill, _) => 2,
        _ => 1,
    }
}

pub(super) fn encode_overview(overview: ChunkOverview, bytes: &mut [u8]) {
    encode_cell(overview.base, &mut bytes[0..4]);
    encode_cell(overview.detail, &mut bytes[4..8]);
    bytes[8..12].copy_from_slice(&overview.detail_bounds);
    bytes[12..14].copy_from_slice(&overview.feature_count.to_le_bytes());
    bytes[14] = overview.feature_kind;
    bytes[15] = 0;
}

pub(super) fn decode_overview(bytes: &[u8]) -> Result<ChunkOverview, WorldArchiveError> {
    if bytes[15] != 0 {
        return Err(invalid("nonzero overview reserved byte"));
    }
    let base = decode_cell(&bytes[0..4])?;
    let detail_present = bytes[7] != NO_TERRAIN_CLASS;
    let detail = if !detail_present {
        TerrainCell {
            elevation: 0,
            moisture: 0,
            class: TerrainClass(NO_TERRAIN_CLASS),
        }
    } else {
        decode_cell(&bytes[4..8])?
    };
    if bytes[14] != NO_FEATURE && bytes[14] > 2 {
        return Err(invalid("invalid overview feature kind"));
    }
    let detail_bounds: [u8; 4] = bytes[8..12].try_into().unwrap();
    if detail_present
        && (detail_bounds[0] >= detail_bounds[2]
            || detail_bounds[1] >= detail_bounds[3]
            || detail_bounds[2] > CHUNK_SIZE as u8
            || detail_bounds[3] > CHUNK_SIZE as u8)
    {
        return Err(invalid("invalid overview detail bounds"));
    }
    if !detail_present && detail_bounds != [0; 4] {
        return Err(invalid("absent overview detail has nonempty bounds"));
    }
    let feature_count = u16::from_le_bytes(bytes[12..14].try_into().unwrap());
    if (bytes[14] == NO_FEATURE) != (feature_count == 0) {
        return Err(invalid("overview feature kind/count disagree"));
    }
    Ok(ChunkOverview {
        base,
        detail,
        detail_bounds,
        feature_count,
        feature_kind: bytes[14],
        reserved: 0,
    })
}

fn encode_cell(cell: TerrainCell, bytes: &mut [u8]) {
    bytes[0..2].copy_from_slice(&cell.elevation.to_le_bytes());
    bytes[2] = cell.moisture;
    bytes[3] = cell.classification().packed();
}

fn decode_cell(bytes: &[u8]) -> Result<TerrainCell, WorldArchiveError> {
    let class = TerrainClass::from_packed(bytes[3])
        .ok_or_else(|| invalid("invalid overview terrain class"))?;
    Ok(TerrainCell {
        elevation: u16::from_le_bytes(bytes[0..2].try_into().unwrap()),
        moisture: bytes[2],
        class,
    })
}
