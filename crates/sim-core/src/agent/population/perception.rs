//! Bounded physical perception of nearby water, resources, and occupancy.

use super::Population;
use super::init::reachable_cells;
use crate::{
    BaseResource, SpawnKind, World, WorldPosition, WorldRect,
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
        // Each cell is looked up once: its terrain and generated feature
        // (chunk by chunk), then anything placed or grown there, and any structure.
        let width_cells = width as usize;
        let mut terrain: Vec<Option<(crate::TerrainCell, Option<crate::Feature>)>> = Vec::new();
        terrain
            .try_reserve_exact(cell_count)
            .map_err(|_| PerceptionError::AllocationFailed)?;
        terrain.resize(cell_count, None);
        world.visit_area(area, |position, cell, feature| {
            let index = (position.y - area.min.y) as usize * width_cells
                + (position.x - area.min.x) as usize;
            terrain[index] = Some((cell, feature));
        });
        for y in area.min.y..area.max.y {
            // What's placed and built in this row, walked alongside the cells.
            let mut row_objects = spawned_objects.in_row(y, area.min.x, area.max.x).peekable();
            let mut row_structures = structures.xs_in_row(y, area.min.x, area.max.x).peekable();
            for x in area.min.x..area.max.x {
                let position = WorldPosition { x, y };
                if !crate::WORLD_GENERATION_BOUNDS.contains(position) {
                    return Err(PerceptionError::OutsideWorld);
                }
                let index = (y - area.min.y) as usize * width_cells + (x - area.min.x) as usize;
                let (cell, feature) = terrain[index].ok_or(PerceptionError::Unloaded)?;
                elevations.push(cell.elevation);
                let object = row_objects.next_if(|object| object.position.x == x);
                let built = row_structures.next_if_eq(&x).is_some();
                let placed_water = object.is_some_and(|object| object.kind == SpawnKind::Water);
                let water = cell
                    .water()
                    .or_else(|| placed_water.then_some(crate::WaterSource::Lake));
                if cell.water().is_none() && !placed_water && !built {
                    traversable_cells.push(position);
                }
                let blocks = |kind: crate::FeatureKind| {
                    matches!(kind, crate::FeatureKind::Tree | crate::FeatureKind::Rock)
                };
                if feature.is_some_and(|feature| blocks(feature.kind))
                    || object
                        .and_then(|object| object.kind.feature())
                        .is_some_and(blocks)
                {
                    try_push(&mut reserved_cells, position)?;
                }
                if let Some(source) = water
                    && source.is_drinkable()
                {
                    try_push(&mut drinkable_water, PerceivedWater { position, source })?;
                }
                let base = feature.map(crate::Feature::base_resource);
                let placed = object.and_then(|object| {
                    let mut resource = object.kind.resource()?;
                    resource.capacity = object.remaining?;
                    (resource.capacity > 0).then_some(resource)
                });
                let resource = placed
                    .or_else(|| base.and_then(|base| resource_deltas.remaining_of(position, base)));
                if let Some(resource) = resource {
                    try_push(&mut resources, PerceivedResource { position, resource })?;
                } else if let Some(base) = base {
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
        let reachable_cells = {
            let swimmable: Vec<WorldPosition> =
                drinkable_water.iter().map(|water| water.position).collect();
            reachable_cells(
                area,
                view.position,
                &traversable_cells,
                &swimmable,
                &elevations,
            )?
        };
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
            animals: Vec::new(),
            expressions: Vec::new(),
        })
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
