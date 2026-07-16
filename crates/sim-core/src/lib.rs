//! Engine-independent deterministic simulation foundation.

mod agent;
mod needs;
mod routing;
mod scheduler;
mod spatial;
mod world;
mod worldgen;

pub use agent::{
    AgentActivity, AgentId, AgentView, EventId, MAX_PERCEPTION_CELLS, MAX_PERCEPTION_RADIUS,
    MAX_POPULATION, MoveRequestError, MovementEventOutcome, MovementOutcomeKind, MovementScheduled,
    PerceivedResource, PerceivedWater, PerceptionError, PhysicalPerception, PopulationInit,
    PopulationInitError, PopulationInitOutcome, RouteEventOutcome, RouteOutcomeKind,
    RouteScheduled, SimTime, SpawnInvalidReason,
};
pub use needs::{
    NEED_MAX, NEED_RATE_PERIOD_TICKS, NeedKind, NeedLevelView, NeedQueryError, NeedThreshold,
    NeedThresholdEventOutcome, NeedThresholdOutcomeKind, PhysicalNeedsView,
};
pub use routing::{MAX_ROUTE_EXPANSIONS, RouteRequest, RouteRequestError};
pub use world::{
    BaseResource, BiomeType, CHUNK_SIZE, ChunkCoord, ChunkGenerator, ChunkInspection,
    ChunkLoadRequest, ChunkLocalPosition, ChunkPresence, ClimateSample, DEFAULT_INITIAL_WORLD_SIZE,
    Feature, FeatureKind, GenerateAreaError, GeneratedCell, MAX_CHUNKS_PER_GENERATION,
    MAX_GENERATED_CELLS, MAX_GENERATED_CHUNKS, MAX_GENERATED_TERRAIN_BYTES, MAX_INITIAL_CHUNKS,
    MAX_TRAVERSABLE_ELEVATION_DELTA, PrevailingWind, ResourceKind, Standability, SurfaceType,
    TerrainCell, TerrainClass, TraversalKind, TraversalStep, WORLD_GENERATION_BOUNDS,
    WORLD_HALF_EXTENT, WORLD_SIDE_CELLS, WaterSource, World, WorldChunk, WorldChunkLoad,
    WorldConfig, WorldConfigError, WorldPosition, WorldQueryError, WorldRect,
};

use std::time::Duration;

use agent::Population;
use routing::RoutePlanner;
use scheduler::{EventClass, MAX_DUE_EVENTS_PER_TICK, Scheduler};

/// Immutable settings used to construct or reset a simulation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EngineConfig {
    pub seed: u64,
    pub ticks_per_second: u32,
    pub world: WorldConfig,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            seed: 1,
            ticks_per_second: 60,
            world: WorldConfig::default(),
        }
    }
}

impl EngineConfig {
    pub fn tick_duration(self) -> Duration {
        Duration::from_secs_f64(1.0 / f64::from(self.ticks_per_second.max(1)))
    }
}

/// Commands are the only public mutation route used by clients.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EngineCommand {
    TogglePause,
    SetPaused(bool),
    SetSpeed(f32),
    Reset,
    GenerateWorldArea(WorldRect),
    MoveAgent {
        agent: AgentId,
        target: WorldPosition,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineCommandOutcome {
    Applied,
    Ignored,
    Movement(Result<MovementScheduled, MoveRequestError>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TickOutcome {
    Paused,
    Advanced {
        time: SimTime,
        processed_events: u16,
        due_backlog: bool,
    },
    TimeExhausted,
}

/// Read-only data intended for renderers, tools, and remote clients.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SimulationSnapshot {
    pub tick: u64,
    pub simulated_seconds: f64,
    pub paused: bool,
    pub speed: f32,
    pub seed: u64,
    pub agent_count: u32,
    pub scheduled_event_count: u32,
}

/// Owns simulation state. Presentation code must not mutate its fields directly.
#[derive(Debug)]
pub struct Engine {
    config: EngineConfig,
    world: World,
    time: SimTime,
    paused: bool,
    speed: f32,
    population: Population,
    scheduler: Scheduler,
    movement_outcomes: Vec<MovementEventOutcome>,
    route_outcomes: Vec<RouteEventOutcome>,
    need_outcomes: Vec<NeedThresholdEventOutcome>,
    route_planner: RoutePlanner,
}

impl Engine {
    /// Creates deterministic simulation state without synchronously materializing terrain.
    ///
    /// Call [`Self::materialize_initial_area`] for eager headless workflows. The
    /// viewer instead streams explicit [`WorldChunkLoad`] payloads through the
    /// main-thread insertion boundary.
    pub fn new(config: EngineConfig) -> Self {
        Self {
            world: World::new(config.seed, config.world),
            config,
            time: SimTime::ZERO,
            paused: false,
            speed: 1.0,
            population: Population::default(),
            scheduler: Scheduler::default(),
            movement_outcomes: Vec::new(),
            route_outcomes: Vec::new(),
            need_outcomes: Vec::new(),
            route_planner: RoutePlanner::default(),
        }
    }

    pub fn command(&mut self, command: EngineCommand) -> EngineCommandOutcome {
        match command {
            EngineCommand::TogglePause => {
                self.paused = !self.paused;
                EngineCommandOutcome::Applied
            }
            EngineCommand::SetPaused(paused) => {
                self.paused = paused;
                EngineCommandOutcome::Applied
            }
            EngineCommand::SetSpeed(speed) if speed.is_finite() => {
                self.speed = speed.clamp(0.0, 64.0);
                EngineCommandOutcome::Applied
            }
            EngineCommand::SetSpeed(_) => EngineCommandOutcome::Ignored,
            EngineCommand::Reset => {
                self.time = SimTime::ZERO;
                self.paused = false;
                self.speed = 1.0;
                self.population = Population::default();
                self.scheduler = Scheduler::default();
                self.movement_outcomes.clear();
                self.route_outcomes.clear();
                self.need_outcomes.clear();
                self.route_planner = RoutePlanner::default();
                EngineCommandOutcome::Applied
            }
            EngineCommand::GenerateWorldArea(bounds) => {
                let _ = self.world.generate_area(bounds);
                EngineCommandOutcome::Applied
            }
            EngineCommand::MoveAgent { agent, target } => {
                EngineCommandOutcome::Movement(self.request_move(agent, target))
            }
        }
    }

    /// Advances exactly one deterministic simulation tick.
    pub fn tick(&mut self) -> TickOutcome {
        if self.paused {
            return TickOutcome::Paused;
        }
        let Some(next_time) = self.time.checked_add(1) else {
            return TickOutcome::TimeExhausted;
        };
        self.time = next_time;
        self.movement_outcomes.clear();
        self.route_outcomes.clear();
        self.need_outcomes.clear();
        let mut processed = 0_usize;
        while processed < MAX_DUE_EVENTS_PER_TICK {
            let Some(event) = self.scheduler.pop_due(self.time) else {
                break;
            };
            if event.class == EventClass::NeedThreshold {
                self.need_outcomes
                    .push(self.population.apply_need_threshold(event));
                processed += 1;
                continue;
            }
            let outcome = self
                .population
                .apply_movement(&mut self.scheduler, &self.world, event);
            let agent = outcome.agent;
            let kind = outcome.kind;
            self.movement_outcomes.push(outcome);
            match kind {
                MovementOutcomeKind::Moved | MovementOutcomeKind::Occupied(_) => {
                    self.continue_route(agent);
                }
                MovementOutcomeKind::StaleEvent => {}
                MovementOutcomeKind::InconsistentOccupancy => {
                    self.finish_route(agent, RouteOutcomeKind::InconsistentOccupancy);
                }
                MovementOutcomeKind::MissingAgent
                | MovementOutcomeKind::DeadAgent
                | MovementOutcomeKind::InvalidStep
                | MovementOutcomeKind::Unloaded
                | MovementOutcomeKind::OutsideWorld
                | MovementOutcomeKind::OutsideActiveArea
                | MovementOutcomeKind::Blocked(_) => {
                    if self.population.route_request(agent).is_some() {
                        self.finish_route(agent, map_movement_route_failure(kind));
                    }
                }
                MovementOutcomeKind::EventSequenceExhausted => {
                    if self.population.route_request(agent).is_some() {
                        self.finish_route(agent, RouteOutcomeKind::EventSequenceExhausted);
                    }
                }
            }
            processed += 1;
        }
        TickOutcome::Advanced {
            time: self.time,
            processed_events: processed as u16,
            due_backlog: self.scheduler.has_due(self.time),
        }
    }

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
        let outcome = population.initialize(&self.world, self.time, init, requested_positions)?;
        let scheduler_capacity = (init.population as usize)
            .checked_mul(4)
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
        self.population = population;
        self.scheduler = scheduler;
        self.movement_outcomes = movement_outcomes;
        self.need_outcomes = need_outcomes;
        self.route_outcomes.clear();
        self.route_planner = RoutePlanner::default();
        Ok(outcome)
    }

    pub fn request_move(
        &mut self,
        agent: AgentId,
        target: WorldPosition,
    ) -> Result<MovementScheduled, MoveRequestError> {
        self.compact_scheduler_if_needed();
        self.population.schedule_movement(
            &mut self.scheduler,
            &self.world,
            self.time,
            agent,
            target,
        )
    }

    /// Plans a bounded deterministic minimum-travel-time local route and schedules its first step.
    pub fn request_route(
        &mut self,
        agent: AgentId,
        request: RouteRequest,
    ) -> Result<RouteScheduled, RouteRequestError> {
        self.compact_scheduler_if_needed();
        let (origin, active_area) = self.population.route_context(agent)?;
        let plan = self.route_planner.plan(
            &self.world,
            self.population.spatial(),
            active_area,
            agent,
            origin,
            request,
        )?;
        let scheduled = self
            .population
            .schedule_route_step(
                &mut self.scheduler,
                &self.world,
                self.time,
                agent,
                request,
                plan.next,
            )
            .map_err(map_move_route_error)?;
        Ok(RouteScheduled {
            first_event: scheduled.event,
            first_completion: scheduled.completes_at,
            destination: request.destination,
            expansions: plan.expansions,
        })
    }

    /// Returns objective nearby physical facts in canonical world row order.
    pub fn perceive_physical(
        &self,
        agent: AgentId,
        radius: u8,
    ) -> Result<PhysicalPerception, PerceptionError> {
        self.population.perceive(&self.world, agent, radius)
    }

    /// Returns objective facts for a bounded half-open rectangle inside the active area.
    pub fn perceive_physical_area(
        &self,
        agent: AgentId,
        area: WorldRect,
    ) -> Result<PhysicalPerception, PerceptionError> {
        self.population.perceive_area(&self.world, agent, area)
    }

    /// Returns at most `limit` canonical read-only views in ascending ID order.
    pub fn agent_views(&self, limit: usize) -> impl Iterator<Item = AgentView> + '_ {
        self.population.views(limit)
    }

    /// Outcomes from the most recent advancing tick. Paused ticks preserve them.
    pub fn movement_outcomes(&self) -> &[MovementEventOutcome] {
        &self.movement_outcomes
    }

    /// Route arrivals or terminal route failures from the most recent advancing tick.
    pub fn route_outcomes(&self) -> &[RouteEventOutcome] {
        &self.route_outcomes
    }

    /// Need thresholds reached or discarded as stale during the most recent advancing tick.
    pub fn need_threshold_outcomes(&self) -> &[NeedThresholdEventOutcome] {
        &self.need_outcomes
    }

    /// Analytically evaluates one agent's physical needs at current simulation time.
    pub fn physical_needs(&self, agent: AgentId) -> Result<PhysicalNeedsView, NeedQueryError> {
        self.population.needs_view(agent, self.time)
    }

    pub fn snapshot(&self) -> SimulationSnapshot {
        SimulationSnapshot {
            tick: self.time.ticks(),
            simulated_seconds: self.time.ticks() as f64
                / f64::from(self.config.ticks_per_second.max(1)),
            paused: self.paused,
            speed: self.speed,
            seed: self.config.seed,
            agent_count: self.population.len() as u32,
            scheduled_event_count: self.scheduler.len() as u32,
        }
    }

    fn continue_route(&mut self, agent: AgentId) {
        self.compact_scheduler_if_needed();
        let Some(request) = self.population.route_request(agent) else {
            return;
        };
        let Ok((origin, active_area)) = self.population.route_context(agent) else {
            self.finish_route(agent, RouteOutcomeKind::InconsistentOccupancy);
            return;
        };
        if origin == request.destination {
            self.finish_route(agent, RouteOutcomeKind::Arrived);
            return;
        }
        match self.route_planner.plan(
            &self.world,
            self.population.spatial(),
            active_area,
            agent,
            origin,
            request,
        ) {
            Ok(plan) => {
                if let Err(error) = self.population.schedule_route_step(
                    &mut self.scheduler,
                    &self.world,
                    self.time,
                    agent,
                    request,
                    plan.next,
                ) {
                    self.finish_route(agent, map_move_route_failure_kind(error));
                }
            }
            Err(error) => self.finish_route(agent, map_route_failure_kind(error)),
        }
    }

    fn finish_route(&mut self, agent: AgentId, kind: RouteOutcomeKind) {
        let Some(request) = self.population.route_request(agent) else {
            return;
        };
        let kind =
            match self
                .population
                .finish_route_activity(&mut self.scheduler, self.time, agent)
            {
                Ok(()) => kind,
                Err(MoveRequestError::RescheduleLimit) => RouteOutcomeKind::RescheduleLimit,
                Err(MoveRequestError::EventSequenceExhausted) => {
                    RouteOutcomeKind::EventSequenceExhausted
                }
                Err(_) => RouteOutcomeKind::InconsistentOccupancy,
            };
        self.population.clear_route(agent);
        self.route_outcomes.push(RouteEventOutcome {
            agent,
            at: self.time,
            destination: request.destination,
            kind,
        });
    }

    fn compact_scheduler_if_needed(&mut self) {
        let retention_limit = self
            .population
            .len()
            .saturating_mul(5)
            .saturating_add(4_096)
            .max(64);
        if self.scheduler.len() >= retention_limit {
            let population = &self.population;
            self.scheduler
                .retain(|event| population.event_is_current(event));
        }
    }
}

fn map_move_route_error(error: MoveRequestError) -> RouteRequestError {
    match error {
        MoveRequestError::MissingAgent => RouteRequestError::MissingAgent,
        MoveRequestError::DeadAgent => RouteRequestError::DeadAgent,
        MoveRequestError::InvalidStep => RouteRequestError::NoPath { expansions: 0 },
        MoveRequestError::Unloaded => RouteRequestError::Unloaded,
        MoveRequestError::OutsideWorld => RouteRequestError::OutsideWorld,
        MoveRequestError::OutsideActiveArea => RouteRequestError::OutsideActiveArea,
        MoveRequestError::Blocked(kind) => RouteRequestError::Blocked(kind),
        MoveRequestError::Occupied(agent) => RouteRequestError::Occupied(agent),
        MoveRequestError::TimeOverflow => RouteRequestError::TimeOverflow,
        MoveRequestError::RescheduleLimit => RouteRequestError::RescheduleLimit,
        MoveRequestError::EventSequenceExhausted => RouteRequestError::EventSequenceExhausted,
    }
}

fn map_move_route_failure_kind(error: MoveRequestError) -> RouteOutcomeKind {
    match map_move_route_error(error) {
        RouteRequestError::Occupied(agent) => RouteOutcomeKind::Occupied(agent),
        error => map_route_failure_kind(error),
    }
}

fn map_route_failure_kind(error: RouteRequestError) -> RouteOutcomeKind {
    match error {
        RouteRequestError::NoPath { expansions } => RouteOutcomeKind::NoPath { expansions },
        RouteRequestError::BudgetExhausted { expansions } => {
            RouteOutcomeKind::BudgetExhausted { expansions }
        }
        RouteRequestError::Occupied(agent) => RouteOutcomeKind::Occupied(agent),
        RouteRequestError::Unloaded => RouteOutcomeKind::Unloaded,
        RouteRequestError::OutsideWorld => RouteOutcomeKind::OutsideWorld,
        RouteRequestError::OutsideActiveArea => RouteOutcomeKind::OutsideActiveArea,
        RouteRequestError::Blocked(kind) => RouteOutcomeKind::Blocked(kind),
        RouteRequestError::TimeOverflow => RouteOutcomeKind::TimeOverflow,
        RouteRequestError::RescheduleLimit => RouteOutcomeKind::RescheduleLimit,
        RouteRequestError::EventSequenceExhausted => RouteOutcomeKind::EventSequenceExhausted,
        RouteRequestError::MissingAgent
        | RouteRequestError::DeadAgent
        | RouteRequestError::AlreadyAtDestination
        | RouteRequestError::ZeroBudget
        | RouteRequestError::BudgetTooLarge { .. } => RouteOutcomeKind::InconsistentOccupancy,
    }
}

fn map_movement_route_failure(kind: MovementOutcomeKind) -> RouteOutcomeKind {
    match kind {
        MovementOutcomeKind::Unloaded => RouteOutcomeKind::Unloaded,
        MovementOutcomeKind::OutsideWorld => RouteOutcomeKind::OutsideWorld,
        MovementOutcomeKind::OutsideActiveArea => RouteOutcomeKind::OutsideActiveArea,
        MovementOutcomeKind::Blocked(kind) => RouteOutcomeKind::Blocked(kind),
        MovementOutcomeKind::Occupied(agent) => RouteOutcomeKind::Occupied(agent),
        MovementOutcomeKind::InconsistentOccupancy
        | MovementOutcomeKind::EventSequenceExhausted
        | MovementOutcomeKind::MissingAgent
        | MovementOutcomeKind::DeadAgent
        | MovementOutcomeKind::InvalidStep
        | MovementOutcomeKind::Moved
        | MovementOutcomeKind::StaleEvent => RouteOutcomeKind::InconsistentOccupancy,
    }
}

impl Default for Engine {
    fn default() -> Self {
        Self::new(EngineConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use std::{mem::size_of, time::Instant};

    use super::*;

    fn resident_engine(size: u32) -> Engine {
        let mut engine = Engine::new(EngineConfig {
            seed: 42,
            world: WorldConfig::new(size, size).unwrap(),
            ..EngineConfig::default()
        });
        engine.materialize_initial_area().unwrap();
        engine
    }

    fn standable_steps(engine: &Engine, count: usize) -> Vec<(WorldPosition, WorldPosition)> {
        let bounds = engine.world().initial_bounds();
        let mut found = Vec::new();
        'rows: for y in bounds.min.y..bounds.max.y {
            for x in bounds.min.x..bounds.max.x {
                let from = WorldPosition { x, y };
                if engine.world().standability_at(from) != Ok(Standability::Standable) {
                    continue;
                }
                for (dx, dy) in [(1, 0), (0, 1), (-1, 0), (0, -1)] {
                    let to = WorldPosition {
                        x: x + dx,
                        y: y + dy,
                    };
                    if bounds.contains(to)
                        && engine
                            .world()
                            .traversal_step(from, to)
                            .is_ok_and(TraversalStep::is_passable)
                    {
                        found.push((from, to));
                        if found.len() == count {
                            break 'rows;
                        }
                        break;
                    }
                }
            }
        }
        assert_eq!(found.len(), count);
        found
    }

    fn blocked_step(engine: &Engine) -> (WorldPosition, WorldPosition, TraversalKind) {
        let bounds = engine.world().initial_bounds();
        for y in bounds.min.y..bounds.max.y {
            for x in bounds.min.x..bounds.max.x {
                let from = WorldPosition { x, y };
                if engine.world().standability_at(from) != Ok(Standability::Standable) {
                    continue;
                }
                for (dx, dy) in [(1, 0), (0, 1), (-1, 0), (0, -1)] {
                    let target = WorldPosition {
                        x: x + dx,
                        y: y + dy,
                    };
                    if !bounds.contains(target) {
                        continue;
                    }
                    if let Ok(step) = engine.world().traversal_step(from, target)
                        && !step.is_passable()
                    {
                        return (from, target, step.kind());
                    }
                }
            }
        }
        panic!("seeded test area should contain a blocked cardinal step");
    }

    #[test]
    fn identical_inputs_produce_identical_snapshots() {
        let mut left = Engine::default();
        let mut right = Engine::default();
        for _ in 0..1_000 {
            left.tick();
            right.tick();
        }
        assert_eq!(left.snapshot(), right.snapshot());
    }

    #[test]
    fn pause_prevents_time_advancing() {
        let mut engine = Engine::default();
        engine.command(EngineCommand::SetPaused(true));
        engine.tick();
        assert_eq!(engine.snapshot().tick, 0);
    }

    #[test]
    fn reset_restores_runtime_state_but_preserves_config() {
        let config = EngineConfig {
            seed: 42,
            ticks_per_second: 20,
            ..EngineConfig::default()
        };
        let mut engine = Engine::new(config);
        engine.tick();
        engine.command(EngineCommand::SetSpeed(8.0));
        engine.command(EngineCommand::Reset);
        assert_eq!(engine.config(), config);
        assert_eq!(engine.snapshot().tick, 0);
        assert_eq!(engine.snapshot().speed, 1.0);
    }

    #[test]
    fn equal_seeds_generate_equal_worlds() {
        let left = Engine::new(EngineConfig {
            seed: 99,
            ..EngineConfig::default()
        });
        let right = Engine::new(EngineConfig {
            seed: 99,
            ..EngineConfig::default()
        });

        assert_eq!(left.world(), right.world());
    }

    #[test]
    fn engine_construction_is_deferred_and_headless_materialization_is_explicit() {
        let config = EngineConfig {
            seed: 99,
            world: WorldConfig::new(96, 64).unwrap(),
            ..EngineConfig::default()
        };
        let mut engine = Engine::new(config);

        assert_eq!(engine.world().loaded_chunk_count(), 0);
        assert!(
            !engine
                .world()
                .area_is_generated(engine.world().initial_bounds())
        );
        engine.materialize_initial_area().unwrap();
        let eager = World::generate(config.seed, config.world);
        assert_eq!(
            engine.world().cells().collect::<Vec<_>>(),
            eager.cells().collect::<Vec<_>>()
        );
        assert_eq!(
            engine.world().all_features().copied().collect::<Vec<_>>(),
            eager.all_features().copied().collect::<Vec<_>>()
        );
    }

    #[test]
    fn terrain_materialization_does_not_change_fixed_tick_progression() {
        let config = EngineConfig {
            seed: 99,
            world: WorldConfig::new(96, 64).unwrap(),
            ..EngineConfig::default()
        };
        let mut unloaded = Engine::new(config);
        let mut resident = Engine::new(config);
        let loads = resident
            .world()
            .missing_chunk_load_requests(resident.world().initial_bounds())
            .unwrap()
            .into_iter()
            .map(|request| World::generate_chunk_load(config.seed, request))
            .collect();

        assert_eq!(resident.apply_world_chunk_loads(loads), Ok(4));
        for _ in 0..600 {
            unloaded.tick();
            resident.tick();
        }

        assert_eq!(unloaded.snapshot(), resident.snapshot());
        assert_eq!(unloaded.snapshot().tick, 600);
    }

    #[test]
    fn generate_initial_area_command_uses_bootstrap_batching() {
        let config = EngineConfig {
            seed: 99,
            world: WorldConfig::new(128, 64).unwrap(),
            ..EngineConfig::default()
        };
        let mut engine = Engine::new(config);
        let initial = engine.world().initial_bounds();

        engine.command(EngineCommand::GenerateWorldArea(initial));

        assert!(engine.world().area_is_generated(initial));
        assert_eq!(engine.world().loaded_chunk_count(), 4);
    }

    #[test]
    fn applied_chunks_remain_engine_owned_and_deduplicated() {
        let config = EngineConfig {
            seed: 9,
            world: WorldConfig::new(64, 64).unwrap(),
            ..EngineConfig::default()
        };
        let mut engine = Engine::new(config);
        let coord = ChunkCoord { x: -1, y: 0 };
        let chunk = World::generate_chunk_at(config.seed, coord).unwrap();

        assert_eq!(engine.apply_world_chunks(vec![chunk.clone()]), Ok(1));
        let revision = engine.world().revision();
        assert_eq!(
            engine
                .world()
                .inspect_chunk_at(WorldPosition { x: -1, y: 0 })
                .unwrap()
                .presence,
            ChunkPresence::RetainedPartialInitial
        );
        assert_eq!(engine.apply_world_chunks(vec![chunk]), Ok(0));
        assert_eq!(engine.world().revision(), revision);
    }

    #[test]
    fn population_initialization_is_explicit_atomic_and_deterministic() {
        let config = EngineConfig {
            seed: 42,
            world: WorldConfig::new(64, 64).unwrap(),
            ..EngineConfig::default()
        };
        let mut engine = Engine::new(config);
        let init = PopulationInit {
            active_area: engine.world().initial_bounds(),
            population: 2,
        };
        assert_eq!(
            engine.initialize_population(init, &[]),
            Err(PopulationInitError::IncompleteResidency)
        );
        assert_eq!(engine.snapshot().agent_count, 0);

        engine.materialize_initial_area().unwrap();
        let pair = standable_steps(&engine, 1)[0];
        let blocked = engine
            .world()
            .cells()
            .map(|(position, _)| position)
            .find(|&position| {
                engine.world().standability_at(position) != Ok(Standability::Standable)
            })
            .expect("seeded test area should contain a blocked spawn");
        let blocked_reason = match engine.world().standability_at(blocked).unwrap() {
            Standability::BlockedByWater => SpawnInvalidReason::Water,
            Standability::BlockedByFeature => SpawnInvalidReason::BlockingFeature,
            Standability::Standable => unreachable!(),
        };
        assert_eq!(
            engine.initialize_population(init, &[blocked]),
            Err(PopulationInitError::InvalidSpawn {
                position: blocked,
                reason: blocked_reason,
            })
        );
        assert_eq!(engine.snapshot().agent_count, 0);
        assert_eq!(
            engine.initialize_population(init, &[pair.0, pair.0]),
            Err(PopulationInitError::DuplicatePosition { position: pair.0 })
        );
        assert_eq!(engine.snapshot().agent_count, 0);

        let one_cell = WorldRect {
            min: pair.0,
            max: WorldPosition {
                x: pair.0.x + 1,
                y: pair.0.y + 1,
            },
        };
        assert_eq!(
            engine.initialize_population(
                PopulationInit {
                    active_area: one_cell,
                    population: 2,
                },
                &[],
            ),
            Err(PopulationInitError::InsufficientValidSpawnCells {
                requested: 2,
                found: 1,
            })
        );
        assert_eq!(engine.snapshot().agent_count, 0);

        let outcome = engine
            .initialize_population(init, &[pair.0, pair.1])
            .unwrap();
        assert_eq!(outcome.first_id, AgentId::new(0));
        assert_eq!(
            engine.agent_views(10).collect::<Vec<_>>(),
            [
                AgentView {
                    id: AgentId::new(0),
                    position: pair.0,
                    activity: AgentActivity::Idle,
                },
                AgentView {
                    id: AgentId::new(1),
                    position: pair.1,
                    activity: AgentActivity::Idle,
                },
            ]
        );
    }

    #[test]
    fn movement_completes_exactly_at_integer_cost_and_pause_holds_events() {
        let mut engine = resident_engine(64);
        let (from, target) = standable_steps(&engine, 1)[0];
        let cost = engine
            .world()
            .traversal_step(from, target)
            .unwrap()
            .cost()
            .unwrap();
        engine
            .initialize_population(
                PopulationInit {
                    active_area: engine.world().initial_bounds(),
                    population: 1,
                },
                &[from],
            )
            .unwrap();
        let scheduled = engine.request_move(AgentId::new(0), target).unwrap();
        assert_eq!(scheduled.completes_at, SimTime::from_ticks(u64::from(cost)));
        assert_eq!(
            engine.agent_views(1).next().unwrap().activity,
            AgentActivity::Moving
        );

        engine.command(EngineCommand::SetPaused(true));
        assert_eq!(engine.tick(), TickOutcome::Paused);
        assert_eq!(engine.agent_views(1).next().unwrap().position, from);
        engine.command(EngineCommand::SetPaused(false));
        for _ in 1..cost {
            engine.tick();
            assert_eq!(engine.agent_views(1).next().unwrap().position, from);
        }
        engine.tick();
        assert_eq!(engine.agent_views(1).next().unwrap().position, target);
        assert_eq!(engine.movement_outcomes().len(), 1);
        assert_eq!(
            engine.movement_outcomes()[0].kind,
            MovementOutcomeKind::Moved
        );
    }

    #[test]
    fn superseded_equal_time_movement_is_stale_and_cannot_move_twice() {
        let mut engine = resident_engine(64);
        let (from, target) = standable_steps(&engine, 1)[0];
        engine
            .initialize_population(
                PopulationInit {
                    active_area: engine.world().initial_bounds(),
                    population: 1,
                },
                &[from],
            )
            .unwrap();
        let due = engine
            .request_move(AgentId::new(0), target)
            .unwrap()
            .completes_at;
        assert_eq!(
            engine
                .request_move(AgentId::new(0), target)
                .unwrap()
                .completes_at,
            due
        );
        for _ in 0..due.ticks() {
            engine.tick();
        }
        assert_eq!(
            engine
                .movement_outcomes()
                .iter()
                .map(|outcome| outcome.kind)
                .collect::<Vec<_>>(),
            [MovementOutcomeKind::StaleEvent, MovementOutcomeKind::Moved]
        );
        assert_eq!(engine.agent_views(1).next().unwrap().position, target);
    }

    #[test]
    fn movement_rejections_are_typed_and_do_not_mutate_agent_or_world() {
        let mut engine = resident_engine(64);
        let (from, target, blocked_kind) = blocked_step(&engine);
        engine
            .initialize_population(
                PopulationInit {
                    active_area: engine.world().initial_bounds(),
                    population: 1,
                },
                &[from],
            )
            .unwrap();
        let revision = engine.world().revision();
        assert_eq!(
            engine.request_move(AgentId::new(0), from),
            Err(MoveRequestError::InvalidStep)
        );
        assert_eq!(
            engine.request_move(AgentId::new(99), target),
            Err(MoveRequestError::MissingAgent)
        );
        assert_eq!(
            engine.request_move(AgentId::new(0), target),
            Err(MoveRequestError::Blocked(blocked_kind))
        );
        assert_eq!(
            engine.request_move(
                AgentId::new(0),
                WorldPosition {
                    x: WORLD_HALF_EXTENT,
                    y: from.y,
                },
            ),
            Err(MoveRequestError::OutsideWorld)
        );
        assert_eq!(engine.agent_views(1).next().unwrap().position, from);
        assert_eq!(
            engine.agent_views(1).next().unwrap().activity,
            AgentActivity::Idle
        );
        assert_eq!(engine.world().revision(), revision);

        engine.population.mark_dead(AgentId::new(0));
        assert_eq!(
            engine.request_move(AgentId::new(0), target),
            Err(MoveRequestError::DeadAgent)
        );

        let mut bounded = resident_engine(64);
        let (bounded_from, bounded_target) = standable_steps(&bounded, 1)[0];
        bounded
            .initialize_population(
                PopulationInit {
                    active_area: WorldRect {
                        min: bounded_from,
                        max: WorldPosition {
                            x: bounded_from.x + 1,
                            y: bounded_from.y + 1,
                        },
                    },
                    population: 1,
                },
                &[bounded_from],
            )
            .unwrap();
        assert_eq!(
            bounded.request_move(AgentId::new(0), bounded_target),
            Err(MoveRequestError::OutsideActiveArea)
        );
    }

    #[test]
    fn equal_time_events_apply_in_agent_id_order_not_insertion_order() {
        let base = resident_engine(64);
        let candidates = standable_steps(&base, 32);
        let (first, second) = candidates
            .iter()
            .enumerate()
            .find_map(|(index, &left)| {
                let left_cost = base.world().traversal_step(left.0, left.1).ok()?.cost()?;
                candidates[index + 1..].iter().copied().find_map(|right| {
                    (base.world().traversal_step(right.0, right.1).ok()?.cost()? == left_cost
                        && left.0 != right.0
                        && left.0 != right.1
                        && left.1 != right.0
                        && left.1 != right.1)
                        .then_some((left, right))
                })
            })
            .expect("seeded test area should contain two equal-cost steps");
        let steps = [first, second];
        let origins = [steps[0].0, steps[1].0];
        let mut forward = resident_engine(64);
        let mut reverse = resident_engine(64);
        for engine in [&mut forward, &mut reverse] {
            engine
                .initialize_population(
                    PopulationInit {
                        active_area: engine.world().initial_bounds(),
                        population: 2,
                    },
                    &origins,
                )
                .unwrap();
        }
        forward.request_move(AgentId::new(0), steps[0].1).unwrap();
        forward.request_move(AgentId::new(1), steps[1].1).unwrap();
        reverse.request_move(AgentId::new(1), steps[1].1).unwrap();
        reverse.request_move(AgentId::new(0), steps[0].1).unwrap();
        for _ in 0..64 {
            forward.tick();
            reverse.tick();
        }
        assert_eq!(
            forward.agent_views(10).collect::<Vec<_>>(),
            reverse.agent_views(10).collect::<Vec<_>>()
        );
        assert_eq!(forward.snapshot(), reverse.snapshot());
    }

    #[test]
    fn reset_clears_population_scheduler_and_id_sequence_but_keeps_residency() {
        let mut engine = resident_engine(64);
        let pair = standable_steps(&engine, 1)[0];
        let init = PopulationInit {
            active_area: engine.world().initial_bounds(),
            population: 1,
        };
        engine.initialize_population(init, &[pair.0]).unwrap();
        engine.request_move(AgentId::new(0), pair.1).unwrap();
        let chunks = engine.world().loaded_chunk_count();
        engine.command(EngineCommand::Reset);
        assert_eq!(engine.snapshot().tick, 0);
        assert_eq!(engine.snapshot().agent_count, 0);
        assert_eq!(engine.snapshot().scheduled_event_count, 0);
        assert_eq!(engine.world().loaded_chunk_count(), chunks);
        assert_eq!(
            engine
                .initialize_population(init, &[pair.0])
                .unwrap()
                .first_id,
            AgentId::new(0)
        );
    }

    #[test]
    fn sequence_exhaustion_settles_due_route_without_stranding_moving_activity() {
        let mut engine = resident_engine(64);
        let pair = standable_steps(&engine, 1)[0];
        engine
            .initialize_population(
                PopulationInit {
                    active_area: engine.world().initial_bounds(),
                    population: 1,
                },
                &[pair.0],
            )
            .unwrap();
        let route = engine
            .request_route(
                AgentId::new(0),
                RouteRequest {
                    destination: pair.1,
                    max_expansions: 8,
                },
            )
            .unwrap();
        engine.scheduler.exhaust_sequence();
        while engine.snapshot().tick < route.first_completion.ticks() {
            engine.tick();
        }
        let view = engine.agent_views(1).next().unwrap();
        assert_eq!(view.position, pair.0);
        assert_eq!(view.activity, AgentActivity::Idle);
        assert_eq!(
            engine
                .physical_needs(view.id)
                .unwrap()
                .thirst
                .rate_per_period,
            4
        );
        assert_eq!(
            engine.route_outcomes().last().unwrap().kind,
            RouteOutcomeKind::EventSequenceExhausted
        );
    }

    #[test]
    fn due_event_drain_is_bounded_and_reports_backlog() {
        let mut engine = Engine::default();
        for sequence in 0..=MAX_DUE_EVENTS_PER_TICK {
            engine
                .scheduler
                .schedule_movement(
                    SimTime::from_ticks(1),
                    AgentId::new(sequence as u32),
                    0,
                    agent::CompactPosition { x: 0, y: 0 },
                )
                .unwrap();
        }
        assert_eq!(
            engine.tick(),
            TickOutcome::Advanced {
                time: SimTime::from_ticks(1),
                processed_events: MAX_DUE_EVENTS_PER_TICK as u16,
                due_backlog: true,
            }
        );
        assert_eq!(engine.movement_outcomes().len(), MAX_DUE_EVENTS_PER_TICK);
        assert_eq!(engine.snapshot().scheduled_event_count, 1);
    }

    #[test]
    fn simulation_time_exhaustion_is_typed_and_does_not_repeat_due_work() {
        let mut engine = Engine {
            time: SimTime::from_ticks(u64::MAX),
            ..Engine::default()
        };
        assert_eq!(engine.tick(), TickOutcome::TimeExhausted);
        assert_eq!(engine.snapshot().tick, u64::MAX);
    }

    #[test]
    #[ignore = "release-only Slice 0 layout and scheduler measurement"]
    fn release_physical_agent_slice_zero_measurement() {
        assert!(
            !std::hint::black_box(cfg!(debug_assertions)),
            "run this measurement in release mode"
        );
        eprintln!(
            "population\tagent_size\tagent_align\tgeneration_size\tevent_size\tevent_align\trecord_capacity\tgeneration_capacity\tscheduler_capacity\toutcome_capacity\tretained_logical_bytes\tretained_buffers\tinsert_ns\tinsert_growth_allocations\treschedule_ns\treschedule_growth_allocations\tdue_extract_ns"
        );
        for population in [20_u32, 100, 10_000] {
            let mut engine = resident_engine(512);
            engine
                .initialize_population(
                    PopulationInit {
                        active_area: engine.world().initial_bounds(),
                        population,
                    },
                    &[],
                )
                .unwrap();
            let (record_capacity, generation_capacity, _, _, _, _) = engine.population.capacities();
            let initial_scheduler_capacity = engine.scheduler.capacity();
            let outcome_capacity = engine.movement_outcomes.capacity();

            let repetitions = match population {
                20 => 10_000_u128,
                100 => 2_000,
                _ => 50,
            };
            let mut insert_ns = 0_u128;
            let mut reschedule_ns = 0_u128;
            let mut due_ns = 0_u128;
            let mut insert_growths = 0;
            let mut reschedule_growths = 0;
            for _ in 0..repetitions {
                let mut scheduler = Scheduler::with_capacity(population as usize);
                let before_insert_capacity = scheduler.capacity();
                let insert_start = Instant::now();
                for raw in 0..population {
                    scheduler
                        .schedule_movement(
                            SimTime::from_ticks(10),
                            AgentId::new(raw),
                            1,
                            agent::CompactPosition { x: 0, y: 0 },
                        )
                        .unwrap();
                }
                insert_ns += insert_start.elapsed().as_nanos();
                insert_growths += usize::from(scheduler.capacity() != before_insert_capacity);
                std::hint::black_box(scheduler.len());

                let before_reschedule_capacity = scheduler.capacity();
                let reschedule_start = Instant::now();
                for raw in 0..population {
                    scheduler
                        .schedule_movement(
                            SimTime::from_ticks(10),
                            AgentId::new(raw),
                            2,
                            agent::CompactPosition { x: 1, y: 0 },
                        )
                        .unwrap();
                }
                reschedule_ns += reschedule_start.elapsed().as_nanos();
                reschedule_growths +=
                    usize::from(scheduler.capacity() != before_reschedule_capacity);

                let due_start = Instant::now();
                let mut extracted = 0;
                while scheduler.pop_due(SimTime::from_ticks(10)).is_some() {
                    extracted += 1;
                }
                due_ns += due_start.elapsed().as_nanos();
                assert_eq!(extracted, population as usize * 2);
            }
            insert_ns /= repetitions;
            reschedule_ns /= repetitions;
            due_ns /= repetitions;
            insert_growths /= repetitions as usize;
            reschedule_growths /= repetitions as usize;

            let retained_logical_bytes = record_capacity * size_of::<agent::AgentRecord>()
                + generation_capacity * size_of::<u32>()
                + initial_scheduler_capacity * size_of::<scheduler::ScheduledEvent>()
                + outcome_capacity * size_of::<MovementEventOutcome>();
            eprintln!(
                "{population}\t{}\t{}\t{}\t{}\t{}\t{record_capacity}\t{generation_capacity}\t{initial_scheduler_capacity}\t{outcome_capacity}\t{retained_logical_bytes}\t4\t{insert_ns}\t{insert_growths}\t{reschedule_ns}\t{reschedule_growths}\t{due_ns}",
                size_of::<agent::AgentRecord>(),
                std::mem::align_of::<agent::AgentRecord>(),
                size_of::<u32>(),
                size_of::<scheduler::ScheduledEvent>(),
                std::mem::align_of::<scheduler::ScheduledEvent>(),
            );
        }
    }

    #[test]
    #[ignore = "release-only Slice 1 spatial, perception, and route measurement"]
    fn release_physical_agent_slice_one_measurement() {
        assert!(
            !std::hint::black_box(cfg!(debug_assertions)),
            "run this measurement in release mode"
        );
        eprintln!(
            "population\tspatial_entry_size\tspatial_entry_align\troute_state_size\tspatial_entry_capacity\tspatial_buckets\tspatial_logical_bytes\tperception_cells\tperceived_agents\twater\tresources\tperception_ns"
        );
        for population in [20_u32, 100, 10_000] {
            let mut engine = resident_engine(512);
            engine
                .initialize_population(
                    PopulationInit {
                        active_area: engine.world().initial_bounds(),
                        population,
                    },
                    &[],
                )
                .unwrap();
            let (_, _, route_capacity, _, spatial_capacity, spatial_buckets) =
                engine.population.capacities();
            let start = Instant::now();
            let perception = engine.perceive_physical(AgentId::new(0), 31).unwrap();
            let perception_ns = start.elapsed().as_nanos();
            let spatial_logical_bytes = spatial_capacity * size_of::<spatial::CellOccupant>()
                + route_capacity * size_of::<Option<agent::RouteState>>();
            let perception_cells = (perception.area.max.x - perception.area.min.x)
                * (perception.area.max.y - perception.area.min.y);
            eprintln!(
                "{population}\t{}\t{}\t{}\t{spatial_capacity}\t{spatial_buckets}\t{spatial_logical_bytes}\t{perception_cells}\t{}\t{}\t{}\t{perception_ns}",
                size_of::<spatial::CellOccupant>(),
                std::mem::align_of::<spatial::CellOccupant>(),
                size_of::<Option<agent::RouteState>>(),
                perception.agents.len(),
                perception.drinkable_water.len(),
                perception.resources.len(),
            );
        }

        let mut engine = resident_engine(512);
        let origin = standable_steps(&engine, 1)[0].0;
        engine
            .initialize_population(
                PopulationInit {
                    active_area: engine.world().initial_bounds(),
                    population: 1,
                },
                &[origin],
            )
            .unwrap();
        let active_area = engine.world().initial_bounds();
        let mut selected = None;
        'search: for radius in 8_i64..=48 {
            for dy in -radius..=radius {
                for dx in -radius..=radius {
                    if dx.abs().max(dy.abs()) != radius {
                        continue;
                    }
                    let destination = WorldPosition {
                        x: origin.x + dx,
                        y: origin.y + dy,
                    };
                    if !active_area.contains(destination) {
                        continue;
                    }
                    let request = RouteRequest {
                        destination,
                        max_expansions: MAX_ROUTE_EXPANSIONS,
                    };
                    if let Ok(plan) = engine.route_planner.plan(
                        &engine.world,
                        engine.population.spatial(),
                        active_area,
                        AgentId::new(0),
                        origin,
                        request,
                    ) && plan.expansions >= 64
                    {
                        selected = Some((request, plan.expansions));
                        break 'search;
                    }
                }
            }
        }
        let (request, expansions) =
            selected.expect("seeded area should have a measured local route");
        let capacities_before = engine.route_planner.capacities();
        let repetitions = 1_000_u128;
        let start = Instant::now();
        for _ in 0..repetitions {
            let plan = engine
                .route_planner
                .plan(
                    &engine.world,
                    engine.population.spatial(),
                    active_area,
                    AgentId::new(0),
                    origin,
                    request,
                )
                .unwrap();
            assert_eq!(plan.expansions, expansions);
            std::hint::black_box(plan.next);
        }
        let average_ns = start.elapsed().as_nanos() / repetitions;
        let capacities_after = engine.route_planner.capacities();
        eprintln!(
            "route_expansions\t{expansions}\troute_average_ns\t{average_ns}\tnode_capacity\t{}\tlookup_capacity\t{}\topen_capacity\t{}\tgrowth_buffers\t{}",
            capacities_after.0,
            capacities_after.1,
            capacities_after.2,
            usize::from(capacities_before.0 != capacities_after.0)
                + usize::from(capacities_before.1 != capacities_after.1)
                + usize::from(capacities_before.2 != capacities_after.2),
        );
    }

    #[test]
    #[ignore = "release-only Slice 2 analytical-needs measurement"]
    fn release_physical_agent_slice_two_measurement() {
        assert!(
            !std::hint::black_box(cfg!(debug_assertions)),
            "run this measurement in release mode"
        );
        eprintln!(
            "population\tneed_state_size\tneed_state_align\tthreshold_event_size\tneed_capacity\tscheduler_capacity\tinitial_events\trescheduled_events\tschedule_ns\tdue_extract_ns\tgrowth_buffers\tretained_logical_bytes"
        );
        for population in [20_usize, 100, 10_000] {
            let mut states = Vec::with_capacity(population);
            states.resize(population, needs::NeedState::new(SimTime::ZERO));
            let mut scheduler = Scheduler::with_capacity(population * 4);
            for (raw, state) in states.iter().copied().enumerate() {
                for kind in NeedKind::ALL {
                    if let Some(due) = state.threshold_due(kind, SimTime::ZERO) {
                        scheduler
                            .schedule_need_threshold(due, AgentId::new(raw as u32), 0, kind)
                            .unwrap();
                    }
                }
            }
            let initial_events = scheduler.len();
            let before_capacity = scheduler.capacity();
            let schedule_start = Instant::now();
            let mut rescheduled_events = 0;
            for (raw, state) in states.iter_mut().enumerate() {
                state.transition(AgentActivity::Moving, SimTime::from_ticks(1));
                for kind in NeedKind::ALL {
                    if let Some(due) = state.threshold_due(kind, SimTime::from_ticks(1)) {
                        scheduler
                            .schedule_need_threshold(
                                due,
                                AgentId::new(raw as u32),
                                state.generation(),
                                kind,
                            )
                            .unwrap();
                        rescheduled_events += 1;
                    }
                }
            }
            let schedule_ns = schedule_start.elapsed().as_nanos();
            let growth_buffers = usize::from(scheduler.capacity() != before_capacity);
            let due_start = Instant::now();
            let mut extracted = 0;
            while scheduler.pop_due(SimTime::from_ticks(u64::MAX)).is_some() {
                extracted += 1;
            }
            let due_extract_ns = due_start.elapsed().as_nanos();
            assert_eq!(extracted, initial_events + rescheduled_events);
            let retained_logical_bytes = states.capacity() * size_of::<needs::NeedState>()
                + scheduler.capacity() * size_of::<scheduler::ScheduledEvent>();
            eprintln!(
                "{population}\t{}\t{}\t{}\t{}\t{}\t{initial_events}\t{rescheduled_events}\t{schedule_ns}\t{due_extract_ns}\t{growth_buffers}\t{retained_logical_bytes}",
                size_of::<needs::NeedState>(),
                std::mem::align_of::<needs::NeedState>(),
                size_of::<scheduler::ScheduledEvent>(),
                states.capacity(),
                scheduler.capacity(),
            );
        }
    }
}
