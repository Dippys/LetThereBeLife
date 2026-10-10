//! Population initialization and additive spawning, including terrain
//! reachability checks for requested and filled positions.

use std::collections::BTreeSet;

use super::{MovementEnvironment, Population};
use crate::{
    ChunkCoord, MAX_TRAVERSABLE_ELEVATION_DELTA, NeedKind, Standability, WORLD_GENERATION_BOUNDS,
    World, WorldPosition, WorldRect,
    agent::{
        AgentActivity, AgentId, AgentRecord, AgentSpawnError, CompactPosition,
        MAX_PERCEPTION_CELLS, MAX_POPULATION, PerceptionError, PopulationInit, PopulationInitError,
        PopulationInitOutcome, SimTime, SpawnInvalidReason,
    },
    health::HealthState,
    needs::NeedState,
    placements::SpawnedObjects,
    policy::{PolicyReason, PolicyState},
    resources::InventoryView,
    scheduler::Scheduler,
    sleep::SleepState,
    spatial::SpatialIndex,
};

impl Population {
    pub(crate) fn initialize(
        &mut self,
        world: &World,
        spawned_objects: &SpawnedObjects,
        now: SimTime,
        init: PopulationInit,
        requested_positions: &[WorldPosition],
    ) -> Result<PopulationInitOutcome, PopulationInitError> {
        if self.initialized {
            return Err(PopulationInitError::AlreadyInitialized);
        }
        if init.population == 0 {
            return Err(PopulationInitError::EmptyPopulation);
        }
        if init.population > MAX_POPULATION {
            return Err(PopulationInitError::PopulationTooLarge {
                requested: init.population,
                maximum: MAX_POPULATION,
            });
        }
        if requested_positions.len() > init.population as usize {
            return Err(PopulationInitError::RequestedPositionsExceedPopulation {
                positions: requested_positions.len(),
                population: init.population,
            });
        }
        if init.active_area.max.x <= init.active_area.min.x
            || init.active_area.max.y <= init.active_area.min.y
        {
            return Err(PopulationInitError::EmptyActiveArea);
        }
        if !WORLD_GENERATION_BOUNDS.contains_rect(init.active_area) {
            return Err(PopulationInitError::ActiveAreaOutsideWorld);
        }
        if !world.area_is_generated(init.active_area) {
            return Err(PopulationInitError::IncompleteResidency);
        }

        let capacity = init.population as usize;
        let mut positions = Vec::new();
        positions
            .try_reserve_exact(capacity)
            .map_err(|_| PopulationInitError::AllocationFailed)?;
        let mut occupied = BTreeSet::new();
        for &position in requested_positions {
            if !init.active_area.contains(position) {
                return Err(PopulationInitError::RequestedPositionOutsideArea { position });
            }
            let compact = CompactPosition::checked(position)
                .ok_or(PopulationInitError::ActiveAreaOutsideWorld)?;
            if !occupied.insert(compact) {
                return Err(PopulationInitError::DuplicatePosition { position });
            }
            if let Some(reason) = invalid_spawn_reason(world, spawned_objects, position)? {
                return Err(PopulationInitError::InvalidSpawn { position, reason });
            }
            positions.push(compact);
        }

        'rows: for y in init.active_area.min.y..init.active_area.max.y {
            for x in init.active_area.min.x..init.active_area.max.x {
                if positions.len() == capacity {
                    break 'rows;
                }
                let position = WorldPosition { x, y };
                let compact = CompactPosition::checked(position)
                    .ok_or(PopulationInitError::ActiveAreaOutsideWorld)?;
                if occupied.contains(&compact)
                    || invalid_spawn_reason(world, spawned_objects, position)?.is_some()
                {
                    continue;
                }
                occupied.insert(compact);
                positions.push(compact);
            }
        }
        if positions.len() != capacity {
            return Err(PopulationInitError::InsufficientValidSpawnCells {
                requested: init.population,
                found: positions.len() as u32,
            });
        }

        let mut records = Vec::new();
        records
            .try_reserve_exact(capacity)
            .map_err(|_| PopulationInitError::AllocationFailed)?;
        records.extend(positions.into_iter().map(|position| AgentRecord {
            position,
            activity: AgentActivity::Idle,
        }));
        let mut movement_generations = Vec::new();
        movement_generations
            .try_reserve_exact(capacity)
            .map_err(|_| PopulationInitError::AllocationFailed)?;
        movement_generations.resize(capacity, 0);
        let mut routes = Vec::new();
        routes
            .try_reserve_exact(capacity)
            .map_err(|_| PopulationInitError::AllocationFailed)?;
        routes.resize(capacity, None);
        let mut needs = Vec::new();
        needs
            .try_reserve_exact(capacity)
            .map_err(|_| PopulationInitError::AllocationFailed)?;
        needs.resize(capacity, NeedState::new(now));
        let mut policies = Vec::new();
        policies
            .try_reserve_exact(capacity)
            .map_err(|_| PopulationInitError::AllocationFailed)?;
        policies.extend((0..capacity).map(|raw| PolicyState::for_agent(AgentId(raw as u32))));
        let mut inventories = Vec::new();
        inventories
            .try_reserve_exact(capacity)
            .map_err(|_| PopulationInitError::AllocationFailed)?;
        inventories.resize(capacity, InventoryView::default());
        let mut sleeps = Vec::new();
        sleeps
            .try_reserve_exact(capacity)
            .map_err(|_| PopulationInitError::AllocationFailed)?;
        sleeps.resize(capacity, SleepState::default());
        let mut health = Vec::new();
        health
            .try_reserve_exact(capacity)
            .map_err(|_| PopulationInitError::AllocationFailed)?;
        health.resize(capacity, HealthState::default());
        let spatial = SpatialIndex::from_positions(
            records
                .iter()
                .enumerate()
                .map(|(raw, record)| (AgentId(raw as u32), record.position.world())),
        );

        self.records = records;
        self.movement_generations = movement_generations;
        self.routes = routes;
        self.needs = needs;
        self.policies = policies;
        self.inventories = inventories;
        self.sleeps = sleeps;
        self.health = health;
        self.spatial = spatial;
        self.living_count = init.population;
        self.active_count = init.population;
        self.active_area = Some(init.active_area);
        self.initialized = true;
        Ok(PopulationInitOutcome {
            first_id: AgentId(0),
            count: init.population,
            active_area: init.active_area,
        })
    }

    pub(crate) fn spawn_agent(
        &mut self,
        scheduler: &mut Scheduler,
        environment: MovementEnvironment<'_>,
        now: SimTime,
        position: WorldPosition,
        policy_active: bool,
    ) -> Result<AgentId, AgentSpawnError> {
        if !self.initialized {
            return Err(AgentSpawnError::PopulationNotInitialized);
        }
        if self.records.len() >= MAX_POPULATION as usize {
            return Err(AgentSpawnError::PopulationFull);
        }
        if !WORLD_GENERATION_BOUNDS.contains(position) {
            return Err(AgentSpawnError::OutsideWorld);
        }
        let current_area = self
            .active_area
            .expect("initialized population has an active area");
        let expanded_area = if current_area.contains(position) {
            current_area
        } else {
            let spawn_chunk = ChunkCoord::from_world_position(position)
                .bounds()
                .map_err(|_| AgentSpawnError::OutsideWorld)?;
            WorldRect {
                min: WorldPosition {
                    x: current_area.min.x.min(spawn_chunk.min.x),
                    y: current_area.min.y.min(spawn_chunk.min.y),
                },
                max: WorldPosition {
                    x: current_area.max.x.max(spawn_chunk.max.x),
                    y: current_area.max.y.max(spawn_chunk.max.y),
                },
            }
        };
        if !environment.world.area_is_generated(expanded_area) {
            return Err(AgentSpawnError::Unloaded);
        }
        let compact = CompactPosition::checked(position).ok_or(AgentSpawnError::OutsideWorld)?;
        if let Some(occupant) = self.spatial.occupant(position) {
            return Err(AgentSpawnError::Occupied(occupant));
        }
        if let Some(structure) = environment.structures.structure_at(position) {
            return Err(AgentSpawnError::BlockedByStructure(structure));
        }
        match invalid_spawn_reason(environment.world, environment.spawned_objects, position) {
            Ok(Some(reason)) => return Err(AgentSpawnError::InvalidSpawn(reason)),
            Ok(None) => {}
            Err(PopulationInitError::IncompleteResidency) => {
                return Err(AgentSpawnError::Unloaded);
            }
            Err(_) => return Err(AgentSpawnError::OutsideWorld),
        }
        let needs = NeedState::new(now);
        let threshold_events = NeedKind::ALL
            .into_iter()
            .filter(|kind| needs.threshold_due(*kind, now).is_some())
            .count();
        let due = policy_active
            .then(|| now.checked_add(1).ok_or(AgentSpawnError::TimeOverflow))
            .transpose()?;
        let event_count = threshold_events + usize::from(policy_active);
        if !scheduler.can_schedule(event_count as u64) {
            return Err(AgentSpawnError::EventSequenceExhausted);
        }
        scheduler
            .try_reserve(event_count)
            .map_err(|_| AgentSpawnError::AllocationFailed)?;
        self.records
            .try_reserve(1)
            .map_err(|_| AgentSpawnError::AllocationFailed)?;
        self.movement_generations
            .try_reserve(1)
            .map_err(|_| AgentSpawnError::AllocationFailed)?;
        self.routes
            .try_reserve(1)
            .map_err(|_| AgentSpawnError::AllocationFailed)?;
        self.needs
            .try_reserve(1)
            .map_err(|_| AgentSpawnError::AllocationFailed)?;
        self.policies
            .try_reserve(1)
            .map_err(|_| AgentSpawnError::AllocationFailed)?;
        self.inventories
            .try_reserve(1)
            .map_err(|_| AgentSpawnError::AllocationFailed)?;
        self.sleeps
            .try_reserve(1)
            .map_err(|_| AgentSpawnError::AllocationFailed)?;
        self.health
            .try_reserve(1)
            .map_err(|_| AgentSpawnError::AllocationFailed)?;

        let agent = AgentId(self.records.len() as u32);
        self.records.push(AgentRecord {
            position: compact,
            activity: AgentActivity::Idle,
        });
        self.movement_generations.push(0);
        self.routes.push(None);
        self.needs.push(needs);
        self.policies.push(PolicyState::for_agent(agent));
        self.inventories.push(InventoryView::default());
        self.sleeps.push(SleepState::default());
        self.health.push(HealthState::default());
        self.active_area = Some(expanded_area);
        let inserted = self.spatial.insert(agent, position);
        debug_assert!(inserted, "spawn occupancy was validated before insertion");
        self.living_count += 1;
        self.active_count += 1;
        self.schedule_need_thresholds(scheduler, agent, needs, now)
            .expect("event sequence capacity was prechecked");
        if let Some(due) = due {
            self.schedule_policy_decision(
                scheduler,
                now,
                agent,
                due.ticks().saturating_sub(now.ticks()),
                PolicyReason::InitialDecision,
                false,
            )
            .expect("event sequence capacity was prechecked");
        }
        Ok(agent)
    }
}

/// Standable cells in `area` that `origin` can get to, walking or swimming
/// through `swimmable_cells` (lakes and rivers) on the way.
pub(super) fn reachable_cells(
    area: WorldRect,
    origin: WorldPosition,
    traversable_cells: &[WorldPosition],
    swimmable_cells: &[WorldPosition],
    elevations: &[u16],
) -> Result<Vec<WorldPosition>, PerceptionError> {
    let width =
        usize::try_from(area.max.x - area.min.x).map_err(|_| PerceptionError::AreaOutsideActive)?;
    let height =
        usize::try_from(area.max.y - area.min.y).map_err(|_| PerceptionError::AreaOutsideActive)?;
    let cell_count = width
        .checked_mul(height)
        .ok_or(PerceptionError::AreaTooLarge {
            requested: u64::MAX,
            maximum: MAX_PERCEPTION_CELLS,
        })?;
    let cell_index = |position: WorldPosition| {
        (position.y - area.min.y) as usize * width + (position.x - area.min.x) as usize
    };
    let mut reachability = Vec::new();
    reachability
        .try_reserve_exact(cell_count)
        .map_err(|_| PerceptionError::AllocationFailed)?;
    reachability.resize(cell_count, 0_u8);
    // 1: open, 2: reached; water only carries the search across.
    for &position in traversable_cells.iter().chain(swimmable_cells) {
        if area.contains(position) {
            reachability[cell_index(position)] = 1;
        }
    }
    let mut queue = Vec::new();
    queue
        .try_reserve(traversable_cells.len() + swimmable_cells.len())
        .map_err(|_| PerceptionError::AllocationFailed)?;
    if reachability[cell_index(origin)] == 1 {
        reachability[cell_index(origin)] = 2;
        queue.push(origin);
    }

    let mut head = 0;
    while let Some(&current) = queue.get(head) {
        head += 1;
        for neighbor in [
            WorldPosition {
                x: current.x,
                y: current.y - 1,
            },
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
                y: current.y + 1,
            },
        ] {
            if !area.contains(neighbor) || reachability[cell_index(neighbor)] != 1 {
                continue;
            }
            if elevations[cell_index(current)].abs_diff(elevations[cell_index(neighbor)])
                <= MAX_TRAVERSABLE_ELEVATION_DELTA
            {
                reachability[cell_index(neighbor)] = 2;
                queue.push(neighbor);
            }
        }
    }

    let mut reachable = Vec::new();
    reachable
        .try_reserve(queue.len())
        .map_err(|_| PerceptionError::AllocationFailed)?;
    reachable.extend(
        traversable_cells
            .iter()
            .copied()
            .filter(|position| reachability[cell_index(*position)] == 2),
    );
    Ok(reachable)
}

fn invalid_spawn_reason(
    world: &World,
    spawned_objects: &SpawnedObjects,
    position: WorldPosition,
) -> Result<Option<SpawnInvalidReason>, PopulationInitError> {
    match spawned_objects
        .standability_at(world, position)
        .map_err(|_| PopulationInitError::IncompleteResidency)?
    {
        Standability::Standable => Ok(None),
        Standability::BlockedByWater => Ok(Some(SpawnInvalidReason::Water)),
        Standability::BlockedByFeature => Ok(Some(SpawnInvalidReason::BlockingFeature)),
    }
}
