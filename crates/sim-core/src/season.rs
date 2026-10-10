//! Seasons. Each lasts one simulated hour, so a year takes four. A winter that
//! long outlasts a hunger cycle, so cold and bare bushes actually matter.

use crate::SimTime;

/// Simulated seconds in one season.
pub const SEASON_SECONDS: u64 = 3_600;
/// Ticks in one season.
pub const SEASON_TICKS: u64 = SEASON_SECONDS * 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum Season {
    Spring,
    Summer,
    Autumn,
    Winter,
}

impl Season {
    pub const ALL: [Self; 4] = [Self::Spring, Self::Summer, Self::Autumn, Self::Winter];

    /// The season at `time` (runs start in spring).
    pub const fn at(time: SimTime) -> Self {
        Self::ALL[((time.ticks() / SEASON_TICKS) % 4) as usize]
    }

    /// Extra cold (exposure per rate period) the season brings; summer warms.
    pub const fn chill(self) -> i8 {
        match self {
            Self::Spring => 0,
            Self::Summer => -1,
            Self::Autumn => 1,
            Self::Winter => 2,
        }
    }
}

/// Simulated seconds of winter in `[0, seconds)`.
const fn winter_before(seconds: u64) -> u64 {
    let year = 4 * SEASON_SECONDS;
    (seconds / year) * SEASON_SECONDS + (seconds % year).saturating_sub(3 * SEASON_SECONDS)
}

/// Seconds between `since` and `now` that weren't winter (when fruit grows).
pub(crate) const fn growing_seconds(since: u32, now: u32) -> u32 {
    let (since, now) = (since as u64, now as u64);
    if now <= since {
        return 0;
    }
    (now - since - (winter_before(now) - winter_before(since))) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seasons_turn_every_hour_and_fruit_waits_out_winter() {
        assert_eq!(Season::at(SimTime::ZERO), Season::Spring);
        assert_eq!(
            Season::at(SimTime::from_ticks(3 * SEASON_TICKS)),
            Season::Winter
        );
        assert_eq!(
            Season::at(SimTime::from_ticks(4 * SEASON_TICKS + 1)),
            Season::Spring
        );
        let s = SEASON_SECONDS as u32;
        assert_eq!(growing_seconds(0, 2 * s), 2 * s);
        assert_eq!(growing_seconds(3 * s, 4 * s), 0, "nothing grows in winter");
        assert_eq!(growing_seconds(2 * s, 5 * s), 2 * s, "autumn and spring");
    }
}
