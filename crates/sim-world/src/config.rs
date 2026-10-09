//! Validated bootstrap-area configuration and its errors.

use std::{error::Error, fmt};

use crate::{DEFAULT_INITIAL_WORLD_SIZE, MAX_INITIAL_CELLS, WORLD_SIDE_CELLS};

/// Validated initial generation dimensions, not a maximum world extent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorldConfig {
    initial_width: u32,
    initial_height: u32,
}

impl WorldConfig {
    pub fn new(initial_width: u32, initial_height: u32) -> Result<Self, WorldConfigError> {
        let cells = u64::from(initial_width) * u64::from(initial_height);
        if initial_width == 0 || initial_height == 0 {
            return Err(WorldConfigError::Empty);
        }
        if i64::from(initial_width) > WORLD_SIDE_CELLS
            || i64::from(initial_height) > WORLD_SIDE_CELLS
        {
            return Err(WorldConfigError::OutsideWorldBounds {
                width: initial_width,
                height: initial_height,
                maximum_side: WORLD_SIDE_CELLS as u32,
            });
        }
        if cells > MAX_INITIAL_CELLS {
            return Err(WorldConfigError::TooLarge {
                cells,
                maximum: MAX_INITIAL_CELLS,
            });
        }
        Ok(Self {
            initial_width,
            initial_height,
        })
    }

    pub const fn initial_width(self) -> u32 {
        self.initial_width
    }

    pub const fn initial_height(self) -> u32 {
        self.initial_height
    }
}

impl Default for WorldConfig {
    fn default() -> Self {
        Self {
            initial_width: DEFAULT_INITIAL_WORLD_SIZE,
            initial_height: DEFAULT_INITIAL_WORLD_SIZE,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorldConfigError {
    Empty,
    TooLarge {
        cells: u64,
        maximum: u64,
    },
    OutsideWorldBounds {
        width: u32,
        height: u32,
        maximum_side: u32,
    },
}

impl fmt::Display for WorldConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => formatter.write_str("width and height must both be greater than zero"),
            Self::TooLarge { cells, maximum } => write!(
                formatter,
                "requested {cells} initial cells, but the current safety limit is {maximum}"
            ),
            Self::OutsideWorldBounds {
                width,
                height,
                maximum_side,
            } => write!(
                formatter,
                "initial area {width}x{height} exceeds the centered world's {maximum_side}-cell side"
            ),
        }
    }
}

impl Error for WorldConfigError {}
