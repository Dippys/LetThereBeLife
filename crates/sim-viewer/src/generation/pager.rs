//! Center-out chunk paging over a bounded area, one page ring at a time.

use sim_core::{ChunkCoord, ChunkLoadRequest, GenerateAreaError, World, WorldPosition, WorldRect};

use super::{PAGE_CHUNKS, PAGE_HALF};

/// A bounded, deterministic center-out sequence of chunk pages. It holds only
/// the current page-ring iterator, never one entry per chunk in a large area.
pub struct ChunkPager {
    bounds: WorldRect,
    min_page_x: i64,
    max_page_x: i64,
    min_page_y: i64,
    max_page_y: i64,
    focus_page_x: i64,
    focus_page_y: i64,
    next_radius: i64,
    maximum_radius: i64,
    ring: Option<PageRing>,
}

impl ChunkPager {
    pub fn new(bounds: WorldRect, focus: WorldPosition) -> Result<Self, GenerateAreaError> {
        if bounds.max.x <= bounds.min.x || bounds.max.y <= bounds.min.y {
            return Err(GenerateAreaError::Empty);
        }
        let min_chunk = ChunkCoord::from_world_position(bounds.min);
        let max_position = WorldPosition {
            x: bounds.max.x - 1,
            y: bounds.max.y - 1,
        };
        let max_chunk = ChunkCoord::from_world_position(max_position);
        for coord in [min_chunk, max_chunk] {
            coord.bounds()?;
        }

        let min_page_x = chunk_page(min_chunk.x);
        let max_page_x = chunk_page(max_chunk.x);
        let min_page_y = chunk_page(min_chunk.y);
        let max_page_y = chunk_page(max_chunk.y);
        let focus_chunk = ChunkCoord::from_world_position(focus);
        let focus_page_x = chunk_page(focus_chunk.x).clamp(min_page_x, max_page_x);
        let focus_page_y = chunk_page(focus_chunk.y).clamp(min_page_y, max_page_y);
        let maximum_radius = [
            focus_page_x - min_page_x,
            max_page_x - focus_page_x,
            focus_page_y - min_page_y,
            max_page_y - focus_page_y,
        ]
        .into_iter()
        .max()
        .expect("fixed array is nonempty");
        Ok(Self {
            bounds,
            min_page_x,
            max_page_x,
            min_page_y,
            max_page_y,
            focus_page_x,
            focus_page_y,
            next_radius: 0,
            maximum_radius,
            ring: None,
        })
    }

    pub fn next_requests(
        &mut self,
        world: &World,
    ) -> Result<Option<Vec<ChunkLoadRequest>>, GenerateAreaError> {
        while let Some(page) = self.next_page() {
            let requests = world.missing_chunk_load_requests(page)?;
            if !requests.is_empty() {
                return Ok(Some(requests));
            }
        }
        Ok(None)
    }

    fn next_page(&mut self) -> Option<WorldRect> {
        loop {
            if let Some(ring) = &mut self.ring {
                if let Some((page_x, page_y)) = ring.next() {
                    if let Some(bounds) = page_bounds(page_x, page_y, self.bounds) {
                        return Some(bounds);
                    }
                    continue;
                }
                self.ring = None;
            }
            if self.next_radius > self.maximum_radius {
                return None;
            }
            self.ring = Some(PageRing::new(
                self.focus_page_x,
                self.focus_page_y,
                self.next_radius,
                self.min_page_x,
                self.max_page_x,
                self.min_page_y,
                self.max_page_y,
            ));
            self.next_radius += 1;
        }
    }
}

fn chunk_page(chunk: i64) -> i64 {
    chunk.saturating_add(PAGE_HALF).div_euclid(PAGE_CHUNKS)
}

struct PageRing {
    segments: [Option<PageSegment>; 4],
    segment_index: usize,
}

#[derive(Clone, Copy)]
enum PageSegment {
    Horizontal { y: i64, next: i64, end: i64 },
    Vertical { x: i64, next: i64, end: i64 },
}

impl PageRing {
    fn new(
        center_x: i64,
        center_y: i64,
        radius: i64,
        min_x: i64,
        max_x: i64,
        min_y: i64,
        max_y: i64,
    ) -> Self {
        if radius == 0 {
            return Self {
                segments: [
                    Some(PageSegment::Horizontal {
                        y: center_y,
                        next: center_x,
                        end: center_x,
                    }),
                    None,
                    None,
                    None,
                ],
                segment_index: 0,
            };
        }

        let ring_min_x = center_x - radius;
        let ring_max_x = center_x + radius;
        let ring_min_y = center_y - radius;
        let ring_max_y = center_y + radius;
        let horizontal_start = ring_min_x.max(min_x);
        let horizontal_end = ring_max_x.min(max_x);
        let vertical_start = ring_min_y.saturating_add(1).max(min_y);
        let vertical_end = ring_max_y.saturating_sub(1).min(max_y);
        Self {
            segments: [
                (ring_min_y >= min_y && ring_min_y <= max_y && horizontal_start <= horizontal_end)
                    .then_some(PageSegment::Horizontal {
                        y: ring_min_y,
                        next: horizontal_start,
                        end: horizontal_end,
                    }),
                (ring_max_y >= min_y && ring_max_y <= max_y && horizontal_start <= horizontal_end)
                    .then_some(PageSegment::Horizontal {
                        y: ring_max_y,
                        next: horizontal_start,
                        end: horizontal_end,
                    }),
                (ring_min_x >= min_x && ring_min_x <= max_x && vertical_start <= vertical_end)
                    .then_some(PageSegment::Vertical {
                        x: ring_min_x,
                        next: vertical_start,
                        end: vertical_end,
                    }),
                (ring_max_x >= min_x && ring_max_x <= max_x && vertical_start <= vertical_end)
                    .then_some(PageSegment::Vertical {
                        x: ring_max_x,
                        next: vertical_start,
                        end: vertical_end,
                    }),
            ],
            segment_index: 0,
        }
    }

    fn next(&mut self) -> Option<(i64, i64)> {
        while let Some(segment) = self.segments.get_mut(self.segment_index) {
            match segment {
                Some(PageSegment::Horizontal { y, next, end }) if *next <= *end => {
                    let position = (*next, *y);
                    *next += 1;
                    return Some(position);
                }
                Some(PageSegment::Vertical { x, next, end }) if *next <= *end => {
                    let position = (*x, *next);
                    *next += 1;
                    return Some(position);
                }
                _ => self.segment_index += 1,
            }
        }
        None
    }
}

fn page_bounds(page_x: i64, page_y: i64, bounds: WorldRect) -> Option<WorldRect> {
    let min_chunk = ChunkCoord {
        x: page_x.checked_mul(PAGE_CHUNKS)?.checked_sub(PAGE_HALF)?,
        y: page_y.checked_mul(PAGE_CHUNKS)?.checked_sub(PAGE_HALF)?,
    };
    let max_chunk = ChunkCoord {
        x: min_chunk.x.checked_add(PAGE_CHUNKS - 1)?,
        y: min_chunk.y.checked_add(PAGE_CHUNKS - 1)?,
    };
    let page = WorldRect {
        min: min_chunk.bounds().ok()?.min,
        max: max_chunk.bounds().ok()?.max,
    };
    let clipped = WorldRect {
        min: WorldPosition {
            x: page.min.x.max(bounds.min.x),
            y: page.min.y.max(bounds.min.y),
        },
        max: WorldPosition {
            x: page.max.x.min(bounds.max.x),
            y: page.max.y.min(bounds.max.y),
        },
    };
    (clipped.max.x > clipped.min.x && clipped.max.y > clipped.min.y).then_some(clipped)
}
