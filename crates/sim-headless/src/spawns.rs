//! Deterministic spawn-location selection near drinkable water, wood, or any standable cell.

use std::collections::BTreeSet;

use sim_core::{Engine, Material, Standability, WaterSource, WorldPosition};

use crate::scenario::ScenarioError;

pub(crate) struct SpawnSelection {
    pub(crate) positions: Vec<WorldPosition>,
    pub(crate) resource_access_count: u32,
    pub(crate) fallback_count: u32,
}

pub(crate) fn select_spawn_locations(
    engine: &Engine,
    population: u32,
    radius: u8,
) -> Result<SpawnSelection, ScenarioError> {
    let bounds = engine.world().initial_bounds();
    let radius = u64::from(radius);
    let mut wood = Vec::new();
    for feature in engine.world().all_features() {
        match feature.base_resource().kind {
            Material::Wood => wood.push(feature.position),
            Material::Berries | Material::Bitterberries | Material::Stone | Material::Meat => {}
        }
    }
    let target_count = population as usize;
    let mut resource_access_candidates = BTreeSet::new();
    for (water, _) in engine.world().cells().filter(|(position, _)| {
        engine
            .world()
            .water_at(*position)
            .is_ok_and(|source| source.is_some_and(WaterSource::is_drinkable))
    }) {
        for candidate in cardinal_neighbors(water) {
            if engine.world().standability_at(candidate) == Ok(Standability::Standable) {
                resource_access_candidates.insert(candidate);
            }
        }
    }
    let mut resource_access_candidates: Vec<_> = resource_access_candidates.into_iter().collect();
    let mut fallback_candidates = candidate_cells_near(&wood[..wood.len().min(1)], radius, engine);
    resource_access_candidates.sort_unstable_by_key(|position| (position.y, position.x));
    fallback_candidates.sort_unstable_by_key(|position| (position.y, position.x));
    let mut selected = Vec::with_capacity(target_count);
    let water_target = target_count - target_count.div_ceil(4);
    select_separated(&resource_access_candidates, water_target, &mut selected);
    let resource_access_count = selected.len() as u32;
    select_separated(&fallback_candidates, target_count, &mut selected);
    if selected.len() < target_count {
        let mut general_candidates: Vec<_> = engine
            .world()
            .cells()
            .map(|(position, _)| position)
            .filter(|&position| {
                engine.world().standability_at(position) == Ok(Standability::Standable)
            })
            .collect();
        general_candidates.sort_unstable_by_key(|position| (position.y, position.x));
        select_separated(&general_candidates, target_count, &mut selected);
    }
    if selected.len() == target_count {
        return Ok(SpawnSelection {
            positions: selected,
            resource_access_count,
            fallback_count: population - resource_access_count,
        });
    }
    Err(ScenarioError(format!(
        "bounded spawn search {:?} found {} of {} water-access or wood-access cells within radius {}",
        bounds,
        selected.len(),
        population,
        radius
    )))
}

fn select_separated(
    candidates: &[WorldPosition],
    target_count: usize,
    selected: &mut Vec<WorldPosition>,
) {
    for minimum_separation in [4_u64, 2, 0] {
        for &candidate in candidates {
            if selected.contains(&candidate)
                || selected
                    .iter()
                    .any(|&other| chebyshev(candidate, other) < minimum_separation)
            {
                continue;
            }
            selected.push(candidate);
            if selected.len() == target_count {
                return;
            }
        }
    }
}

fn candidate_cells_near(
    targets: &[WorldPosition],
    radius: u64,
    engine: &Engine,
) -> Vec<WorldPosition> {
    let mut candidates = BTreeSet::new();
    let radius = radius as i64;
    for target in targets {
        for y in target.y - radius..=target.y + radius {
            for x in target.x - radius..=target.x + radius {
                let position = WorldPosition { x, y };
                if engine.world().standability_at(position) == Ok(Standability::Standable) {
                    candidates.insert(position);
                }
            }
        }
    }
    candidates.into_iter().collect()
}

fn chebyshev(left: WorldPosition, right: WorldPosition) -> u64 {
    left.x.abs_diff(right.x).max(left.y.abs_diff(right.y))
}

fn cardinal_neighbors(position: WorldPosition) -> [WorldPosition; 4] {
    [
        WorldPosition {
            x: position.x,
            y: position.y - 1,
        },
        WorldPosition {
            x: position.x - 1,
            y: position.y,
        },
        WorldPosition {
            x: position.x + 1,
            y: position.y,
        },
        WorldPosition {
            x: position.x,
            y: position.y + 1,
        },
    ]
}
