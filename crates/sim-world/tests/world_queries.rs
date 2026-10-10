//! Public-API world queries used to select a deterministic, plausible settlement candidate.

use sim_world::{
    Material, TraversalKind, WaterSource, World, WorldConfig, WorldPosition, WorldRect,
};
use std::collections::VecDeque;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CandidateInputs {
    position: WorldPosition,
    nearest_water: u16,
    reachable_cells: u32,
    food: u32,
    wood: u32,
    stone: u32,
}

fn inspect_candidate(world: &World, position: WorldPosition) -> Option<CandidateInputs> {
    const RADIUS: i64 = 96;
    const SIDE: usize = (RADIUS as usize) * 2 + 1;
    if world
        .water_at(position)
        .expect("the scenario surveys only resident cells")
        .is_some()
        || world
            .resource_at(position)
            .expect("the scenario surveys only resident cells")
            .is_some_and(|resource| matches!(resource.kind, Material::Wood | Material::Stone))
    {
        return None;
    }
    let mut inputs = CandidateInputs {
        position,
        nearest_water: u16::MAX,
        reachable_cells: 0,
        food: 0,
        wood: 0,
        stone: 0,
    };
    let min = WorldPosition {
        x: position.x - RADIUS,
        y: position.y - RADIUS,
    };
    let index =
        |sample: WorldPosition| ((sample.y - min.y) as usize) * SIDE + (sample.x - min.x) as usize;
    let mut visited = vec![false; SIDE * SIDE];
    let mut counted_resources = vec![false; SIDE * SIDE];
    let mut pending = VecDeque::from([position]);
    visited[index(position)] = true;
    while let Some(current) = pending.pop_front() {
        inputs.reachable_cells += 1;
        for neighbor in [
            WorldPosition {
                x: current.x - 1,
                y: current.y,
            },
            WorldPosition {
                x: current.x + 1,
                y: current.y,
            },
            WorldPosition {
                x: current.x,
                y: current.y - 1,
            },
            WorldPosition {
                x: current.x,
                y: current.y + 1,
            },
        ] {
            if neighbor.x < min.x
                || neighbor.y < min.y
                || neighbor.x > position.x + RADIUS
                || neighbor.y > position.y + RADIUS
            {
                continue;
            }
            let neighbor_index = index(neighbor);
            if !counted_resources[neighbor_index] {
                counted_resources[neighbor_index] = true;
                if let Some(resource) = world
                    .resource_at(neighbor)
                    .expect("the scenario surveys only resident cells")
                {
                    let total = match resource.kind {
                        Material::Berries | Material::Bitterberries | Material::Meat => {
                            &mut inputs.food
                        }
                        Material::Wood => &mut inputs.wood,
                        Material::Stone | Material::Blade => &mut inputs.stone,
                    };
                    *total += u32::from(resource.capacity);
                }
            }
            if world
                .water_at(neighbor)
                .expect("the scenario surveys only resident cells")
                .is_some_and(WaterSource::is_drinkable)
            {
                inputs.nearest_water = inputs.nearest_water.min(
                    (neighbor.x.abs_diff(position.x) + neighbor.y.abs_diff(position.y)) as u16,
                );
            }
            if !visited[neighbor_index]
                && world
                    .traversal_step(current, neighbor)
                    .expect("the scenario surveys only resident cardinal steps")
                    .kind()
                    == TraversalKind::Passable
            {
                visited[neighbor_index] = true;
                pending.push_back(neighbor);
            }
        }
    }
    Some(inputs)
}

fn select_candidate(world: &World, bounds: WorldRect) -> Option<CandidateInputs> {
    const MARGIN: i64 = 96;
    let mut best = None;
    for y in (bounds.min.y + MARGIN..bounds.max.y - MARGIN).step_by(128) {
        for x in (bounds.min.x + MARGIN..bounds.max.x - MARGIN).step_by(128) {
            let Some(candidate) = inspect_candidate(world, WorldPosition { x, y }) else {
                continue;
            };
            if candidate.nearest_water <= 96
                && candidate.reachable_cells >= 12_000
                && candidate.stone > 0
            {
                let score = |value: CandidateInputs| {
                    (
                        value.reachable_cells,
                        u16::MAX - value.nearest_water,
                        value.food + value.wood + value.stone,
                        std::cmp::Reverse(value.position),
                    )
                };
                if best.is_none_or(|current| score(candidate) > score(current)) {
                    best = Some(candidate);
                }
            }
        }
    }
    best
}

#[test]
fn public_world_queries_select_a_deterministic_plausible_settlement_candidate() {
    let bounds = WorldRect {
        min: WorldPosition {
            x: -16_128,
            y: -16_896,
        },
        max: WorldPosition {
            x: -15_104,
            y: -15_872,
        },
    };
    let generate = || {
        let mut world = World::new(1, WorldConfig::new(64, 64).unwrap());
        world.generate_area(bounds).unwrap();
        world
    };
    let left = generate();
    let right = generate();

    let selected = select_candidate(&left, bounds)
        .expect("the canonical river mouth must offer fresh water, traversal, and a resource");
    assert_eq!(Some(selected), inspect_candidate(&right, selected.position));
}
