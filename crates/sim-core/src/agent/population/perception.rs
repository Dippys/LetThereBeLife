//! Bounded physical perception of nearby water, resources, and occupancy.

use super::Population;
use super::init::reachable_cells;
use crate::{
    BaseResource, Standability, World, WorldPosition, WorldQueryError, WorldRect,
    agent::{
        AgentId, MAX_PERCEPTION_CELLS, MAX_PERCEPTION_RADIUS, PerceivedResource, PerceivedWater,
        PerceptionError, PhysicalPerception,
    },
    placements::SpawnedObjects,
    resources::ResourceDeltas,
    structures::StructureStore,
};

impl Population {
    pub(crate) fn perceive(
        &self,
        world: &World,
        spawned_objects: &SpawnedObjects,
        resource_deltas: &ResourceDeltas,
        structures: &StructureStore,
        agent: AgentId,
        radius: u8,
    ) -> Result<PhysicalPerception, PerceptionError> {
        if radius > MAX_PERCEPTION_RADIUS {
            return Err(PerceptionError::RadiusTooLarge {
                requested: radius,
                maximum: MAX_PERCEPTION_RADIUS,
            });
        }
        let view = self.view(agent).ok_or(PerceptionError::MissingAgent)?;
        if view.activity.is_terminal() {
            return Err(PerceptionError::DeadAgent);
        }
        let active = self.active_area.expect("initialized population");
        let radius = i64::from(radius);
        let requested = WorldRect {
            min: WorldPosition {
                x: view.position.x - radius,
                y: view.position.y - radius,
            },
            max: WorldPosition {
                x: view.position.x + radius + 1,
                y: view.position.y + radius + 1,
            },
        };
        let area = active
            .intersection(requested)
            .ok_or(PerceptionError::OutsideWorld)?;
        self.perceive_area(
            world,
            spawned_objects,
            resource_deltas,
            structures,
            agent,
            area,
        )
    }

    pub(crate) fn perceive_area(
        &self,
        world: &World,
        spawned_objects: &SpawnedObjects,
        resource_deltas: &ResourceDeltas,
        structures: &StructureStore,
        agent: AgentId,
        area: WorldRect,
    ) -> Result<PhysicalPerception, PerceptionError> {
        let view = self.view(agent).ok_or(PerceptionError::MissingAgent)?;
        if view.activity.is_terminal() {
            return Err(PerceptionError::DeadAgent);
        }
        if area.max.x <= area.min.x || area.max.y <= area.min.y {
            return Err(PerceptionError::EmptyArea);
        }
        if !self
            .active_area
            .expect("initialized population")
            .contains_rect(area)
        {
            return Err(PerceptionError::AreaOutsideActive);
        }
        let width = u64::try_from(area.max.x - area.min.x)
            .map_err(|_| PerceptionError::AreaOutsideActive)?;
        let height = u64::try_from(area.max.y - area.min.y)
            .map_err(|_| PerceptionError::AreaOutsideActive)?;
        let requested = width
            .checked_mul(height)
            .ok_or(PerceptionError::AreaTooLarge {
                requested: u64::MAX,
                maximum: MAX_PERCEPTION_CELLS,
            })?;
        if requested > u64::from(MAX_PERCEPTION_CELLS) {
            return Err(PerceptionError::AreaTooLarge {
                requested,
                maximum: MAX_PERCEPTION_CELLS,
            });
        }
        let cell_count = requested as usize;
        let mut agent_ids = Vec::new();
        agent_ids
            .try_reserve(self.len().min(cell_count))
            .map_err(|_| PerceptionError::AllocationFailed)?;
        self.spatial.agents_in(area, &mut agent_ids);
        let mut agents = Vec::new();
        agents
            .try_reserve(agent_ids.len())
            .map_err(|_| PerceptionError::AllocationFailed)?;
        let mut claimed_targets = Vec::new();
        claimed_targets
            .try_reserve(agent_ids.len())
            .map_err(|_| PerceptionError::AllocationFailed)?;
        for id in agent_ids {
            if let Some(view) = self.view(id) {
                agents.push(view);
            }
            if id != agent
                && let Some((_, target, _)) = self.policy_commitment(id)
            {
                claimed_targets.push(target);
            }
        }
        claimed_targets.sort_unstable_by_key(|position| (position.y, position.x));
        claimed_targets.dedup();
        let mut drinkable_water = Vec::new();
        let mut resources = Vec::new();
        let mut reserved_cells = Vec::new();
        let mut spent_resources = Vec::new();
        let mut perceived_structures = Vec::new();
        perceived_structures
            .try_reserve(structures.len().min(cell_count))
            .map_err(|_| PerceptionError::AllocationFailed)?;
        structures.push_views_in(area, &mut perceived_structures);
        let mut traversable_cells = Vec::new();
        traversable_cells
            .try_reserve(cell_count)
            .map_err(|_| PerceptionError::AllocationFailed)?;
        let mut elevations = Vec::new();
        elevations
            .try_reserve_exact(cell_count)
            .map_err(|_| PerceptionError::AllocationFailed)?;
        for y in area.min.y..area.max.y {
            for x in area.min.x..area.max.x {
                let position = WorldPosition { x, y };
                elevations.push(
                    world
                        .cell(position)
                        .ok_or(PerceptionError::Unloaded)?
                        .elevation,
                );
                match spawned_objects
                    .standability_at(world, position)
                    .map_err(map_perception_query_error)?
                {
                    Standability::Standable if structures.structure_at(position).is_none() => {
                        traversable_cells.push(position)
                    }
                    Standability::Standable => {}
                    Standability::BlockedByWater | Standability::BlockedByFeature => {}
                }
                if spawned_objects.reserves_exclusive_use_at(world, position) {
                    try_push(&mut reserved_cells, position)?;
                }
                if let Some(source) = spawned_objects
                    .water_at(world, position)
                    .map_err(map_perception_query_error)?
                    && source.is_drinkable()
                {
                    try_push(&mut drinkable_water, PerceivedWater { position, source })?;
                }
                let resource = if let Some(resource) = spawned_objects.resource_at(position) {
                    Some(resource)
                } else {
                    resource_deltas
                        .resource_at(world, position)
                        .map_err(map_perception_query_error)?
                };
                if let Some(resource) = resource {
                    try_push(&mut resources, PerceivedResource { position, resource })?;
                } else if let Some(base) = world.base_resource_at(position) {
                    let spent = BaseResource {
                        capacity: 0,
                        kind: base.kind,
                    };
                    try_push(
                        &mut spent_resources,
                        PerceivedResource {
                            position,
                            resource: spent,
                        },
                    )?;
                }
            }
        }
        let reachable_cells =
            reachable_cells(area, view.position, &traversable_cells, &elevations)?;
        Ok(PhysicalPerception {
            area,
            agents,
            claimed_targets,
            drinkable_water,
            resources,
            structures: perceived_structures,
            traversable_cells,
            reachable_cells,
            reserved_cells,
            spent_resources,
        })
    }
}

fn map_perception_query_error(error: WorldQueryError) -> PerceptionError {
    match error {
        WorldQueryError::OutsideWorldBounds => PerceptionError::OutsideWorld,
        WorldQueryError::Unloaded => PerceptionError::Unloaded,
        WorldQueryError::NonCardinalStep => unreachable!("point queries are not movement steps"),
    }
}

fn try_push<T>(values: &mut Vec<T>, value: T) -> Result<(), PerceptionError> {
    if values.len() == values.capacity() {
        values
            .try_reserve(1)
            .map_err(|_| PerceptionError::AllocationFailed)?;
    }
    values.push(value);
    Ok(())
}
