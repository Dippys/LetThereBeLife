//! Waterholes: small fresh-water ponds at the scale agents live at.
//!
//! Continental drainage produces large lakes and rivers thousands of cells
//! apart, while an agent sees about eight cells. Ponds fill that gap the way
//! real landscapes do: frequent in wet climates, scattered across savanna, rare
//! in deserts. Each chunk holds at most one pond, kept far enough inside the
//! chunk that the pond and its shore never cross a chunk edge, so generation
//! stays independent of chunk order.

use super::noise::hash;
use super::plates::SEA_LEVEL;
use crate::CHUNK_SIZE;

const POND_SEED: u64 = 0x504f_4e44_5345_4544;
/// Largest pond radius in cells.
pub(super) const MAX_POND_RADIUS: i64 = 6;
/// Width of the vegetated shore ring around a pond.
pub(super) const SHORE_WIDTH: i64 = 4;
/// Distance from the chunk edge to the pond center: radius plus shore fits inside.
const EDGE_MARGIN: i64 = MAX_POND_RADIUS + SHORE_WIDTH + 2;
/// Ponds are not placed on land this close to sea level (coasts drain to the sea).
const COASTAL_ELEVATION_MARGIN: i64 = 400;
/// Chance (percent) of a pond per chunk at the driest and the wettest climates.
const MIN_POND_PERCENT: i64 = 3;
const MAX_POND_PERCENT: i64 = 80;
/// Moisture (same scale as terrain classification) mapped onto that range.
const DRY_MOISTURE: i64 = 6_000;
const MOISTURE_PER_PERCENT: i64 = 400;
/// Pond surface sits this far below the surrounding ground.
pub(super) const POND_DEPTH: i32 = 180;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Pond {
    x: i64,
    y: i64,
    radius: i64,
}

/// What a pond contributes to one cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PondCover {
    Deep,
    Shallow,
    Shore,
}

/// Chooses the chunk's pond from its site and the climate there. `sample`
/// returns (macro elevation, moisture) at a chunk-local position.
pub(super) fn pond_for_chunk(
    seed: u64,
    origin_x: i64,
    origin_y: i64,
    sample: impl Fn(i64, i64) -> (i64, i64),
) -> Option<Pond> {
    let key = hash(seed ^ POND_SEED, origin_x, origin_y);
    let span = (CHUNK_SIZE - 2 * EDGE_MARGIN) as u64;
    let local_x = EDGE_MARGIN + (key % span) as i64;
    let local_y = EDGE_MARGIN + ((key >> 12) % span) as i64;
    let (elevation, moisture) = sample(local_x, local_y);
    if elevation <= i64::from(SEA_LEVEL) + COASTAL_ELEVATION_MARGIN {
        return None;
    }
    let chance = ((moisture - DRY_MOISTURE) / MOISTURE_PER_PERCENT)
        .clamp(MIN_POND_PERCENT, MAX_POND_PERCENT);
    if ((key >> 24) % 100) as i64 >= chance {
        return None;
    }
    Some(Pond {
        x: origin_x + local_x,
        y: origin_y + local_y,
        radius: 2 + ((key >> 40) % (MAX_POND_RADIUS as u64 - 1)) as i64,
    })
}

impl Pond {
    /// Water, shore, or nothing at `(x, y)`. Edges are slightly ragged so ponds
    /// don't look like stamped circles.
    pub(super) fn cover(self, seed: u64, x: i64, y: i64) -> Option<PondCover> {
        let (dx, dy) = (x - self.x, y - self.y);
        let distance_sq = dx * dx + dy * dy;
        let ragged = (hash(seed ^ POND_SEED, x, y) % 3) as i64 - 1;
        let water_sq = self.radius * self.radius + ragged * self.radius;
        if distance_sq <= water_sq {
            let deep_radius = self.radius / 2;
            return Some(
                if self.radius >= 4 && distance_sq <= deep_radius * deep_radius {
                    PondCover::Deep
                } else {
                    PondCover::Shallow
                },
            );
        }
        let shore = self.radius + SHORE_WIDTH;
        (distance_sq <= shore * shore).then_some(PondCover::Shore)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ponds_and_shores_stay_inside_their_chunk() {
        for origin_x in (-640..640).step_by(64) {
            for origin_y in (-640..640).step_by(64) {
                let Some(pond) = pond_for_chunk(9, origin_x, origin_y, |_, _| (40_000, 40_000))
                else {
                    continue;
                };
                for y in origin_y - 16..origin_y + 80 {
                    for x in origin_x - 16..origin_x + 80 {
                        if pond.cover(9, x, y).is_some() {
                            assert!(
                                (origin_x..origin_x + CHUNK_SIZE).contains(&x)
                                    && (origin_y..origin_y + CHUNK_SIZE).contains(&y)
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn wet_climates_have_more_ponds_than_dry_ones() {
        let count = |moisture| {
            (0..400)
                .filter(|index| {
                    pond_for_chunk(3, index * 64, 0, |_, _| (40_000, moisture)).is_some()
                })
                .count()
        };
        let (desert, savanna, forest) = (count(4_000), count(17_000), count(40_000));
        assert!(
            desert < savanna && savanna < forest,
            "{desert} {savanna} {forest}"
        );
        assert!(desert > 0, "deserts still have rare oases");
    }

    #[test]
    fn no_ponds_at_the_coast() {
        assert!((0..200).all(|index| {
            pond_for_chunk(3, index * 64, 0, |_, _| {
                (i64::from(SEA_LEVEL) + 100, 40_000)
            })
            .is_none()
        }));
    }
}
