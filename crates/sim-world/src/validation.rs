//! Generation request validation, region-major chunk spans, and area errors.

use std::{error::Error, fmt};

use crate::{
    CHUNK_SIZE, CHUNKS_PER_REGION, ChunkCoord, MAX_CHUNKS_PER_GENERATION, WORLD_GENERATION_BOUNDS,
    WORLD_HALF_EXTENT, WorldPosition, WorldRect, chunk::chunk_coord,
};

#[derive(Clone, Copy)]
pub(crate) struct ChunkSpan {
    pub(crate) min: ChunkCoord,
    pub(crate) max: ChunkCoord,
    pub(crate) total: u128,
}

impl ChunkSpan {
    pub(crate) fn coords(self) -> impl Iterator<Item = ChunkCoord> {
        let min_region_x = self.min.x.div_euclid(CHUNKS_PER_REGION);
        let max_region_x = self.max.x.div_euclid(CHUNKS_PER_REGION);
        let min_region_y = self.min.y.div_euclid(CHUNKS_PER_REGION);
        let max_region_y = self.max.y.div_euclid(CHUNKS_PER_REGION);

        (min_region_y..=max_region_y).flat_map(move |region_y| {
            let region_min_y = region_y * CHUNKS_PER_REGION;
            let min_y = self.min.y.max(region_min_y);
            let max_y = self.max.y.min(region_min_y + CHUNKS_PER_REGION - 1);
            (min_region_x..=max_region_x).flat_map(move |region_x| {
                let region_min_x = region_x * CHUNKS_PER_REGION;
                let min_x = self.min.x.max(region_min_x);
                let max_x = self.max.x.min(region_min_x + CHUNKS_PER_REGION - 1);
                (min_y..=max_y).flat_map(move |y| (min_x..=max_x).map(move |x| ChunkCoord { x, y }))
            })
        })
    }
}

pub(crate) fn validate_chunk_span(bounds: WorldRect) -> Result<ChunkSpan, GenerateAreaError> {
    if bounds.max.x <= bounds.min.x || bounds.max.y <= bounds.min.y {
        return Err(GenerateAreaError::Empty);
    }
    validate_world_bounds(bounds)?;
    let min = chunk_coord(bounds.min);
    let max = chunk_coord(WorldPosition {
        x: bounds.max.x - 1,
        y: bounds.max.y - 1,
    });
    for coord in [min, max] {
        validate_chunk_axis(coord.x)?;
        validate_chunk_axis(coord.y)?;
    }
    let columns = (i128::from(max.x) - i128::from(min.x) + 1) as u128;
    let rows = (i128::from(max.y) - i128::from(min.y) + 1) as u128;
    let requested = columns
        .checked_mul(rows)
        .ok_or(GenerateAreaError::TooLarge)?;
    Ok(ChunkSpan {
        min,
        max,
        total: requested,
    })
}

pub(crate) fn validate_world_bounds(bounds: WorldRect) -> Result<(), GenerateAreaError> {
    WORLD_GENERATION_BOUNDS
        .contains_rect(bounds)
        .then_some(())
        .ok_or(GenerateAreaError::OutsideWorldBounds)
}

pub(crate) fn validate_full_chunk_request(
    bounds: WorldRect,
) -> Result<ChunkSpan, GenerateAreaError> {
    let span = validate_chunk_span(bounds)?;
    if span.total > u128::from(MAX_CHUNKS_PER_GENERATION) {
        return Err(GenerateAreaError::TooManyChunks {
            requested: span.total.min(u128::from(u64::MAX)) as u64,
            maximum: MAX_CHUNKS_PER_GENERATION,
        });
    }
    Ok(span)
}

pub(crate) fn validate_chunk_axis(coord: i64) -> Result<(), GenerateAreaError> {
    coord
        .checked_mul(CHUNK_SIZE)
        .and_then(|origin| origin.checked_add(CHUNK_SIZE))
        .ok_or(GenerateAreaError::TooLarge)
        .map(|_| ())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenerateAreaError {
    Empty,
    TooLarge,
    OutsideWorldBounds,
    TooManyChunks { requested: u64, maximum: u64 },
    WorldCapacity { requested: usize, remaining: usize },
    SeedMismatch { expected: u64, received: u64 },
    InvalidChunkLoad,
}

impl fmt::Display for GenerateAreaError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => formatter.write_str("generation bounds must have positive dimensions"),
            Self::TooLarge => formatter.write_str("generation coordinates exceed safe limits"),
            Self::OutsideWorldBounds => write!(
                formatter,
                "generation must stay inside [{}, {}) on both axes",
                -WORLD_HALF_EXTENT, WORLD_HALF_EXTENT
            ),
            Self::TooManyChunks { requested, maximum } => write!(
                formatter,
                "generation needs at least {requested} new chunks; maximum per request is {maximum}"
            ),
            Self::WorldCapacity {
                requested,
                remaining,
            } => write!(
                formatter,
                "generation needs {requested} new chunks but only {remaining} slots remain"
            ),
            Self::SeedMismatch { expected, received } => write!(
                formatter,
                "generated payload seed {received} does not match world seed {expected}"
            ),
            Self::InvalidChunkLoad => {
                formatter.write_str("generated payload does not match this world's chunk coverage")
            }
        }
    }
}

impl Error for GenerateAreaError {}
