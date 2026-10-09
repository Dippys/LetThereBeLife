//! Signed world coordinates and half-open world rectangles.

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WorldPosition {
    pub x: i64,
    pub y: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorldRect {
    pub min: WorldPosition,
    pub max: WorldPosition,
}

impl WorldRect {
    pub fn from_inclusive_points(start: WorldPosition, end: WorldPosition) -> Self {
        Self {
            min: WorldPosition {
                x: start.x.min(end.x),
                y: start.y.min(end.y),
            },
            max: WorldPosition {
                x: start.x.max(end.x).saturating_add(1),
                y: start.y.max(end.y).saturating_add(1),
            },
        }
    }

    pub fn contains(self, position: WorldPosition) -> bool {
        position.x >= self.min.x
            && position.y >= self.min.y
            && position.x < self.max.x
            && position.y < self.max.y
    }

    pub fn contains_rect(self, other: Self) -> bool {
        self.min.x <= other.min.x
            && self.min.y <= other.min.y
            && self.max.x >= other.max.x
            && self.max.y >= other.max.y
    }

    pub fn intersects(self, other: Self) -> bool {
        self.max.x > other.min.x
            && self.max.y > other.min.y
            && self.min.x < other.max.x
            && self.min.y < other.max.y
    }

    pub fn intersection(self, other: Self) -> Option<Self> {
        intersection(self, other)
    }

    pub fn expanded(self, cells: i64) -> Self {
        Self {
            min: WorldPosition {
                x: self.min.x.saturating_sub(cells),
                y: self.min.y.saturating_sub(cells),
            },
            max: WorldPosition {
                x: self.max.x.saturating_add(cells),
                y: self.max.y.saturating_add(cells),
            },
        }
    }
}

pub(crate) fn intersection(left: WorldRect, right: WorldRect) -> Option<WorldRect> {
    let bounds = WorldRect {
        min: WorldPosition {
            x: left.min.x.max(right.min.x),
            y: left.min.y.max(right.min.y),
        },
        max: WorldPosition {
            x: left.max.x.min(right.max.x),
            y: left.max.y.min(right.max.y),
        },
    };
    (bounds.max.x > bounds.min.x && bounds.max.y > bounds.min.y).then_some(bounds)
}
