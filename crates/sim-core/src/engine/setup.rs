//! World residency, population initialization, agent and object spawning, and
//! explicit starting supplies.

use crate::agent::{MovementEnvironment, Population};
use crate::diagnostics::RuntimeCounters;
use crate::routing::RoutePlanner;
use crate::scheduler::{MAX_DUE_EVENTS_PER_TICK, Scheduler};
use crate::{
    AgentId, AgentSpawnError, Engine, EngineConfig, GenerateAreaError, InitialInventoryError,
    InventoryView, PopulationInit, PopulationInitError, PopulationInitOutcome, SimTime, SpawnKind,
    SpawnObjectError, Standability, World, WorldChunk, WorldChunkLoad, WorldPosition,
    WorldQueryError,
};

impl Engine {
    pub fn config(&self) -> EngineConfig {
        self.config
    }

    /// Returns immutable world state for headless tools and presentation clients.
    pub fn world(&self) -> &World {
        &self.world
    }

    pub fn apply_world_chunks(
        &mut self,
        chunks: Vec<WorldChunk>,
    ) -> Result<usize, GenerateAreaError> {
        self.world.insert_chunks(chunks)
    }

    /// Applies bootstrap-aware worker payloads while retaining authoritative
    /// world ownership in `sim-core`.
    ///
    /// This changes only the deterministic terrain materialization cache. It
    /// does not advance, rewind, or otherwise alter fixed simulation time.
    pub fn apply_world_chunk_loads(
        &mut self,
        loads: Vec<WorldChunkLoad>,
    ) -> Result<usize, GenerateAreaError> {
        self.world.insert_chunk_loads(loads)
    }

    /// Eagerly materializes the configured bootstrap rectangle for headless
    /// callers that need complete startup coverage before advancing.
    pub fn materialize_initial_area(&mut self) -> Result<(), GenerateAreaError> {
        self.world.materialize_initial_area()
    }

    /// Atomically creates a dense population after the declared active area is resident.
    /// Requested positions receive the first IDs in slice order; any remainder is
    /// filled from passable cells in canonical row-major order.
    pub fn initialize_population(
        &mut self,
        init: PopulationInit,
        requested_positions: &[WorldPosition],
    ) -> Result<PopulationInitOutcome, PopulationInitError> {
        if self.population.is_initialized() {
            return Err(PopulationInitError::AlreadyInitialized);
        }
        let mut population = Population::default();
        let outcome = population.initialize(
            &self.world,
            &self.spawned_objects,
            self.time,
            init,
            requested_positions,
        )?;
        let scheduler_capacity = (init.population as usize)
            .checked_mul(5)
            .ok_or(PopulationInitError::AllocationFailed)?;
        let mut scheduler = Scheduler::try_with_capacity(scheduler_capacity)
            .map_err(|_| PopulationInitError::AllocationFailed)?;
        let mut movement_outcomes = Vec::new();
        movement_outcomes
            .try_reserve_exact((init.population as usize).min(MAX_DUE_EVENTS_PER_TICK))
            .map_err(|_| PopulationInitError::AllocationFailed)?;
        population
            .initialize_need_events(&mut scheduler, self.time)
            .map_err(|_| PopulationInitError::EventSequenceExhausted)?;
        let mut need_outcomes = Vec::new();
        need_outcomes
            .try_reserve_exact((init.population as usize).min(MAX_DUE_EVENTS_PER_TICK))
            .map_err(|_| PopulationInitError::AllocationFailed)?;
        let mut policy_diagnostics = Vec::new();
        policy_diagnostics
            .try_reserve_exact(
                (init.population as usize)
                    .min(MAX_DUE_EVENTS_PER_TICK)
                    .saturating_mul(2),
            )
            .map_err(|_| PopulationInitError::AllocationFailed)?;
        self.population = population;
        self.scheduler = scheduler;
        self.movement_outcomes = movement_outcomes;
        self.need_outcomes = need_outcomes;
        self.policy_diagnostics = policy_diagnostics;
        self.health_diagnostics.clear();
        self.death_records.clear();
        self.policy_active = false;
        self.policy_exploration = false;
        self.route_outcomes.clear();
        self.route_planner = RoutePlanner::default();
        self.runtime_counters = RuntimeCounters::default();
        Ok(outcome)
    }

    /// Adds one dense authoritative agent at an exact standable, unoccupied position.
    /// When autonomy is active, the new agent receives its first decision on the next tick.
    pub fn spawn_agent(&mut self, position: WorldPosition) -> Result<AgentId, AgentSpawnError> {
        self.compact_scheduler_if_needed();
        self.population.spawn_agent(
            &mut self.scheduler,
            MovementEnvironment {
                world: &self.world,
                spawned_objects: &self.spawned_objects,
                structures: &self.structures,
            },
            self.time,
            position,
            self.policy_active,
        )
    }

    /// Records explicit starting supplies before autonomous simulation begins.
    pub fn set_initial_inventory(
        &mut self,
        agent: AgentId,
        inventory: InventoryView,
    ) -> Result<(), InitialInventoryError> {
        if self.policy_active {
            return Err(InitialInventoryError::PolicyActive);
        }
        if self.time != SimTime::ZERO {
            return Err(InitialInventoryError::SimulationAdvanced);
        }
        self.population.set_initial_inventory(agent, inventory)
    }

    /// Places one sparse simulation-owned object on a resident, empty cell.
    pub fn spawn_object(
        &mut self,
        kind: SpawnKind,
        position: WorldPosition,
    ) -> Result<(), SpawnObjectError> {
        let standability = self
            .world
            .standability_at(position)
            .map_err(|error| match error {
                WorldQueryError::OutsideWorldBounds => SpawnObjectError::OutsideWorld,
                WorldQueryError::Unloaded => SpawnObjectError::Unloaded,
                WorldQueryError::NonCardinalStep => unreachable!("standing queries are not steps"),
            })?;
        match standability {
            Standability::Standable => {}
            Standability::BlockedByWater => return Err(SpawnObjectError::BlockedByWater),
            Standability::BlockedByFeature => return Err(SpawnObjectError::BlockedByFeature),
        }
        if self.spawned_objects.at(position).is_some() {
            return Err(SpawnObjectError::ExistingObject);
        }
        if self
            .spawned_objects
            .reserves_exclusive_use_at(&self.world, position)
        {
            return Err(SpawnObjectError::BlockedByFeature);
        }
        if self.population.spatial().occupant(position).is_some() {
            return Err(SpawnObjectError::Occupied);
        }
        if self.structures.structure_at(position).is_some() {
            return Err(SpawnObjectError::BlockedByStructure);
        }
        self.spawned_objects.insert(position, kind);
        Ok(())
    }
}
