//! Pointing gestures: the only way one agent's place knowledge reaches another.
//!
//! A sender never transmits a position. It performs a visible gesture — a pointing
//! direction (quantized to 64 directions on the square around it) and an emphasis
//! that grows with distance (one step per doubling). Watchers who see it infer a
//! rough estimate and a search radius large enough to contain the real place.

use crate::WorldPosition;

/// Places closer than this are within the watcher's own view; nobody points at them.
pub(crate) const MIN_POINTING_DISTANCE: u64 = 9;
/// Pointing direction resolution: the vector is scaled so its larger axis is this.
const DIRECTION_SCALE: i64 = 8;

/// What watchers can observe of a pointing gesture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Gesture {
    /// Pointing direction, larger axis normalized to `±DIRECTION_SCALE`.
    direction: (i8, i8),
    /// Emphasis: `floor(log2(distance))`, the distance's order of magnitude.
    emphasis: u8,
}

impl Gesture {
    /// Pointing direction, larger axis normalized to ±8.
    pub const fn direction(self) -> (i8, i8) {
        self.direction
    }

    /// Order of magnitude of the pointed distance (`floor(log2(cells))`).
    pub const fn emphasis(self) -> u8 {
        self.emphasis
    }
}

/// Rounds `numerator / denominator` to the nearest integer, halves away from zero.
fn rounded_div(numerator: i64, denominator: i64) -> i64 {
    let twice = 2 * numerator;
    if twice >= 0 {
        (twice + denominator) / (2 * denominator)
    } else {
        (twice - denominator) / (2 * denominator)
    }
}

/// The gesture an agent at `from` makes to indicate `to`.
pub(crate) fn point(from: WorldPosition, to: WorldPosition) -> Option<Gesture> {
    point_within(from, to, MIN_POINTING_DISTANCE)
}

/// Pointing at something in plain sight (an animal) works at closer range
/// than pointing out a place.
pub(crate) const MIN_ANIMAL_POINTING_DISTANCE: u64 = 2;

/// Like `point`, refusing only targets closer than `minimum` cells.
pub(crate) fn point_within(
    from: WorldPosition,
    to: WorldPosition,
    minimum: u64,
) -> Option<Gesture> {
    let (dx, dy) = (to.x - from.x, to.y - from.y);
    let distance = dx.unsigned_abs().max(dy.unsigned_abs());
    if distance < minimum {
        return None;
    }
    let scale = distance as i64;
    Some(Gesture {
        direction: (
            rounded_div(dx * DIRECTION_SCALE, scale) as i8,
            rounded_div(dy * DIRECTION_SCALE, scale) as i8,
        ),
        emphasis: distance.ilog2() as u8,
    })
}

/// A hand held out toward someone (for requests): the direction only, at any
/// distance. Returns `None` only when `to` is where the agent stands.
pub(crate) fn reach_toward(from: WorldPosition, to: WorldPosition) -> Option<Gesture> {
    let (dx, dy) = (to.x - from.x, to.y - from.y);
    let distance = dx.unsigned_abs().max(dy.unsigned_abs());
    if distance == 0 {
        return None;
    }
    let scale = distance as i64;
    Some(Gesture {
        direction: (
            rounded_div(dx * DIRECTION_SCALE, scale) as i8,
            rounded_div(dy * DIRECTION_SCALE, scale) as i8,
        ),
        emphasis: 0,
    })
}

/// What a watcher standing near `sender` concludes: an estimated place and a
/// search radius in 4-cell units (as stored by the mental map).
pub(crate) fn interpret(sender: WorldPosition, gesture: Gesture) -> (WorldPosition, u8) {
    // The real distance lies in [2^e, 2^(e+1)); guess the middle.
    let low = 1_i64 << gesture.emphasis.min(15);
    let estimate = low + low / 2;
    let position = WorldPosition {
        x: sender.x + rounded_div(i64::from(gesture.direction.0) * estimate, DIRECTION_SCALE),
        y: sender.y + rounded_div(i64::from(gesture.direction.1) * estimate, DIRECTION_SCALE),
    };
    // Distance error is at most half the bucket; direction error is about
    // estimate / 16 per axis. Round the combined radius up to 4-cell units.
    let radius = low / 2 + estimate / 8 + 4;
    (position, ((radius + 3) / 4).clamp(1, 255) as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(x: i64, y: i64) -> WorldPosition {
        WorldPosition { x, y }
    }

    fn chebyshev(left: WorldPosition, right: WorldPosition) -> u64 {
        left.x.abs_diff(right.x).max(left.y.abs_diff(right.y))
    }

    #[test]
    fn nearby_places_are_not_pointed_at() {
        assert_eq!(point(at(0, 0), at(8, -8)), None);
        assert!(point(at(0, 0), at(9, 0)).is_some());
    }

    #[test]
    fn inferred_search_area_contains_the_real_place() {
        let sender = at(-37, 512);
        for (dx, dy) in [
            (9, 0),
            (0, -15),
            (40, 3),
            (-63, 64),
            (127, -200),
            (-700, 31),
            (1_023, 1_023),
            (-2_000, -1_500),
        ] {
            let target = at(sender.x + dx, sender.y + dy);
            let gesture = point(sender, target).expect("far enough to point");
            let (estimate, radius_units) = interpret(sender, gesture);
            let radius = u64::from(radius_units) * 4;
            assert!(
                chebyshev(estimate, target) <= radius,
                "target {target:?} estimate {estimate:?} radius {radius}"
            );
        }
    }

    #[test]
    fn gestures_are_coarse() {
        let sender = at(0, 0);
        assert_eq!(
            point(sender, at(100, 3)),
            point(sender, at(110, 5)),
            "nearby targets in one magnitude band look identical"
        );
    }
}
