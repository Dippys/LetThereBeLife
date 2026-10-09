//! Bounded deterministic exploration: heading variation and local wander
//! targets when no actionable objective is perceived.

use super::selection::candidate_available;
use crate::{AgentId, PhysicalPerception, WorldPosition, policy::ExplorationHeading};

pub(super) fn varied_exploration_heading(
    agent: AgentId,
    origin: WorldPosition,
    heading: ExplorationHeading,
) -> ExplorationHeading {
    let mut key = u64::from(agent.get()) ^ (origin.x as u64).rotate_left(17);
    key ^= (origin.y as u64).rotate_left(41);
    key = (key ^ (key >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    key = (key ^ (key >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    key ^= key >> 31;
    let turn = match key >> 61 {
        0 => -2,
        1 | 2 => -1,
        3..=5 => 0,
        6 => 1,
        _ => 2,
    };
    heading.rotated(turn)
}

pub(super) fn exploration_target(
    origin: WorldPosition,
    perception: &PhysicalPerception,
    heading: ExplorationHeading,
) -> Option<(WorldPosition, ExplorationHeading)> {
    [0_i8, 1, -1, 2, -2, 3, -3, 4].into_iter().find_map(|turn| {
        let heading = heading.rotated(turn);
        let (heading_x, heading_y) = heading.delta();
        perception
            .reachable_cells
            .iter()
            .copied()
            .filter(|candidate| *candidate != origin)
            .filter(|candidate| candidate_available(origin, perception, *candidate))
            .filter_map(|candidate| {
                let dx = candidate.x - origin.x;
                let dy = candidate.y - origin.y;
                let projection = dx * heading_x + dy * heading_y;
                (projection > 0).then(|| {
                    let lateral = (dx * heading_y - dy * heading_x).unsigned_abs();
                    let distance = dx.unsigned_abs() + dy.unsigned_abs();
                    (candidate, projection, lateral, distance)
                })
            })
            .max_by_key(|(candidate, projection, lateral, distance)| {
                (
                    *projection,
                    u64::MAX - *lateral,
                    *distance,
                    candidate.y,
                    candidate.x,
                )
            })
            .map(|(target, _, _, _)| (target, heading))
    })
}
