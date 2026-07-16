//! Engine-independent deterministic simulation foundation.

mod agent;
mod health;
mod needs;
mod policy;
mod resources;
mod routing;
mod scheduler;
mod sleep;
mod spatial;
mod structures;
mod world;
mod worldgen;

pub use agent::{
    AgentActivity, AgentId, AgentView, EventId, MAX_PERCEPTION_CELLS, MAX_PERCEPTION_RADIUS,
    MAX_POPULATION, MoveRequestError, MovementEventOutcome, MovementOutcomeKind, MovementScheduled,
    PerceivedResource, PerceivedWater, PerceptionError, PhysicalPerception, PopulationInit,
    PopulationInitError, PopulationInitOutcome, RouteEventOutcome, RouteOutcomeKind,
    RouteScheduled, SimTime, SpawnInvalidReason,
};
pub use health::{
    DeathCause, DeathRecord, HEALTH_CONSEQUENCE_INTERVAL_TICKS, HEALTH_INCAPACITATION_THRESHOLD,
    HEALTH_MAX, HealthDiagnostic, HealthDiagnosticKind, HealthStatus, HealthView,
};
pub use needs::{
    NEED_MAX, NEED_RATE_PERIOD_TICKS, NeedKind, NeedLevelView, NeedQueryError, NeedThreshold,
    NeedThresholdEventOutcome, NeedThresholdOutcomeKind, PhysicalNeedsView,
};
pub use policy::{
    PHYSICAL_POLICY_ACTION_TICKS, PHYSICAL_POLICY_IDLE_RECHECK_TICKS,
    PHYSICAL_POLICY_MAX_BACKOFF_TICKS, PHYSICAL_POLICY_RADIUS, PHYSICAL_POLICY_ROUTE_BUDGET,
    PhysicalGoal, PhysicalPolicyView, PolicyActivationError, PolicyDiagnostic,
    PolicyDiagnosticKind, PolicyFailureReason, PolicyReason,
};
pub use resources::{
    DRINK_THIRST_RELIEF, EAT_HUNGER_RELIEF, FOOD_CONSUMPTION, GATHER_YIELD,
    INVENTORY_CAPACITY_PER_KIND, InventoryView,
};
pub use routing::{MAX_ROUTE_EXPANSIONS, RouteRequest, RouteRequestError};
pub use sleep::{
    SleepDiagnostic, SleepDiagnosticKind, SleepInterruptionReason, SleepQuality, SleepRequestError,
    SleepView,
};
pub use structures::{
    BuildShelterError, SHELTER_BUILD_TICKS, SHELTER_STONE_COST, SHELTER_WOOD_COST,
    StructureDiagnostic, StructureDiagnosticKind, StructureId, StructureKind, StructureState,
    StructureView,
};
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

use agent::{ActionEffectError, MovementEnvironment, Population};
use policy::{PolicyAction, PolicySelection, retry_delay, select};
use resources::ResourceDeltas;
use routing::{RouteEnvironment, RoutePlanner};
use scheduler::{EventClass, MAX_DUE_EVENTS_PER_TICK, Scheduler};
use sleep::interruption_for_need;
use structures::StructureStore;

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
    pub living_agent_count: u32,
    pub active_agent_count: u32,
    pub death_count: u32,
    pub scheduled_event_count: u32,
    pub structure_count: u32,
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
    policy_diagnostics: Vec<PolicyDiagnostic>,
    sleep_diagnostics: Vec<SleepDiagnostic>,
    structure_diagnostics: Vec<StructureDiagnostic>,
    health_diagnostics: Vec<HealthDiagnostic>,
    death_records: Vec<DeathRecord>,
    policy_active: bool,
    resource_deltas: ResourceDeltas,
    structures: StructureStore,
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
            policy_diagnostics: Vec::new(),
            sleep_diagnostics: Vec::new(),
            structure_diagnostics: Vec::new(),
            health_diagnostics: Vec::new(),
            death_records: Vec::new(),
            policy_active: false,
            resource_deltas: ResourceDeltas::default(),
            structures: StructureStore::default(),
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
                self.policy_diagnostics.clear();
                self.sleep_diagnostics.clear();
                self.structure_diagnostics.clear();
                self.health_diagnostics.clear();
                self.death_records.clear();
                self.policy_active = false;
                self.resource_deltas = ResourceDeltas::default();
                self.structures = StructureStore::default();
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
        self.policy_diagnostics.clear();
        self.sleep_diagnostics.clear();
        self.structure_diagnostics.clear();
        self.health_diagnostics.clear();
        let mut processed = 0_usize;
        while processed < MAX_DUE_EVENTS_PER_TICK {
            let Some(event) = self.scheduler.pop_due(self.time) else {
                break;
            };
            if event.class == EventClass::NeedThreshold {
                let outcome = self.population.apply_need_threshold(event);
                if outcome.outcome == NeedThresholdOutcomeKind::Reached {
                    let construction_cancelled = self.cancel_construction(outcome.agent);
                    let sleeping = self.population.sleep_view(outcome.agent);
                    let interruption = interruption_for_need(outcome.kind);
                    if construction_cancelled {
                        if self
                            .population
                            .interrupt_for_policy_decision(
                                &mut self.scheduler,
                                self.time,
                                outcome.agent,
                                self.policy_active,
                            )
                            .is_err()
                        {
                            self.population.force_settle_idle(self.time, outcome.agent);
                        }
                    } else if sleeping.is_some() && interruption.is_some() {
                        let result = self.population.interrupt_for_policy_decision(
                            &mut self.scheduler,
                            self.time,
                            outcome.agent,
                            self.policy_active,
                        );
                        let interrupted = match result {
                            Ok((_, interrupted)) => interrupted,
                            Err(_) => self
                                .population
                                .force_interrupt_sleep(self.time, outcome.agent),
                        };
                        if let (Some(sleep), Some(reason)) = (sleeping, interruption)
                            && interrupted.is_some()
                        {
                            self.sleep_diagnostics.push(SleepDiagnostic {
                                sleep,
                                at: self.time,
                                kind: SleepDiagnosticKind::Interrupted,
                                interruption: Some(reason),
                            });
                        }
                    } else if self.policy_active
                        && self
                            .population
                            .interrupt_for_policy_decision(
                                &mut self.scheduler,
                                self.time,
                                outcome.agent,
                                true,
                            )
                            .is_err()
                    {
                        self.population.force_settle_idle(self.time, outcome.agent);
                    }
                }
                self.need_outcomes.push(outcome);
                processed += 1;
                continue;
            }
            if event.class == EventClass::HealthConsequence {
                let outcome = self
                    .population
                    .apply_health_consequence(&mut self.scheduler, event);
                match outcome.kind {
                    HealthDiagnosticKind::Incapacitated => {
                        self.cancel_construction(outcome.agent);
                        self.population.incapacitate(outcome.at, outcome.agent);
                    }
                    HealthDiagnosticKind::Died => {
                        self.cancel_construction(outcome.agent);
                        if let Some(cause) = outcome.cause
                            && let Some(record) =
                                self.population
                                    .finalize_death(outcome.at, outcome.agent, cause)
                        {
                            self.death_records.push(record);
                        }
                    }
                    HealthDiagnosticKind::Deteriorated | HealthDiagnosticKind::StaleEvent => {}
                }
                self.health_diagnostics.push(outcome);
                processed += 1;
                continue;
            }
            if event.class == EventClass::Decision {
                self.apply_policy_decision(event);
                processed += 1;
                continue;
            }
            if matches!(event.class, EventClass::Wake | EventClass::ActionCompletion) {
                self.apply_policy_action_completion(event);
                processed += 1;
                continue;
            }
            let outcome = self.population.apply_movement(
                &mut self.scheduler,
                MovementEnvironment {
                    world: &self.world,
                    structures: &self.structures,
                },
                event,
            );
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
                | MovementOutcomeKind::Blocked(_)
                | MovementOutcomeKind::BlockedByStructure(_) => {
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
        self.route_outcomes.clear();
        self.route_planner = RoutePlanner::default();
        Ok(outcome)
    }

    pub fn request_move(
        &mut self,
        agent: AgentId,
        target: WorldPosition,
    ) -> Result<MovementScheduled, MoveRequestError> {
        if self.policy_active {
            return Err(MoveRequestError::PolicyControlled);
        }
        self.compact_scheduler_if_needed();
        self.population.schedule_movement(
            &mut self.scheduler,
            MovementEnvironment {
                world: &self.world,
                structures: &self.structures,
            },
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
        if self.policy_active {
            return Err(RouteRequestError::PolicyControlled);
        }
        self.schedule_route(agent, request)
    }

    fn schedule_route(
        &mut self,
        agent: AgentId,
        request: RouteRequest,
    ) -> Result<RouteScheduled, RouteRequestError> {
        self.compact_scheduler_if_needed();
        let (origin, active_area) = self.population.route_context(agent)?;
        let plan = self.route_planner.plan(
            RouteEnvironment {
                world: &self.world,
                occupancy: self.population.spatial(),
                structures: &self.structures,
                active_area,
            },
            agent,
            origin,
            request,
        )?;
        let scheduled = self
            .population
            .schedule_route_step(
                &mut self.scheduler,
                MovementEnvironment {
                    world: &self.world,
                    structures: &self.structures,
                },
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
        self.population.perceive(
            &self.world,
            &self.resource_deltas,
            &self.structures,
            agent,
            radius,
        )
    }

    /// Returns objective facts for a bounded half-open rectangle inside the active area.
    pub fn perceive_physical_area(
        &self,
        agent: AgentId,
        area: WorldRect,
    ) -> Result<PhysicalPerception, PerceptionError> {
        self.population.perceive_area(
            &self.world,
            &self.resource_deltas,
            &self.structures,
            agent,
            area,
        )
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

    /// Returns compact current health for a retained stable agent identity.
    pub fn health(&self, agent: AgentId) -> Option<HealthView> {
        self.population.health_view(agent)
    }

    /// Health deterioration, incapacitation, death, and stale outcomes from the latest tick.
    pub fn health_diagnostics(&self) -> &[HealthDiagnostic] {
        &self.health_diagnostics
    }

    /// Persistent terminal records in deterministic death-application order.
    pub fn death_records(&self) -> &[DeathRecord] {
        &self.death_records
    }

    /// Returns compact carried resource amounts for one living agent.
    pub fn inventory(&self, agent: AgentId) -> Option<InventoryView> {
        self.population.inventory(agent)
    }

    /// Composes immutable generated capacity with sparse simulation-owned depletion.
    pub fn available_resource_at(
        &self,
        position: WorldPosition,
    ) -> Result<Option<BaseResource>, WorldQueryError> {
        self.resource_deltas.resource_at(&self.world, position)
    }

    /// Number of generated features with simulation-owned remaining-capacity state.
    pub fn modified_resource_count(&self) -> usize {
        self.resource_deltas.len()
    }

    /// Starts one explicit sleep intent at the agent's current physical location.
    pub fn request_sleep(
        &mut self,
        agent: AgentId,
        position: WorldPosition,
    ) -> Result<SleepView, SleepRequestError> {
        if self.policy_active {
            return Err(SleepRequestError::PolicyControlled);
        }
        let quality = self.population.validate_sleep_location(
            &self.world,
            self.time,
            agent,
            position,
            self.structures.is_sheltered_access(position),
            self.structures.structure_at(position),
        )?;
        self.compact_scheduler_if_needed();
        let sleep = self.population.schedule_sleep(
            &mut self.scheduler,
            self.time,
            agent,
            position,
            quality,
            PolicyReason::RestThreshold,
        )?;
        self.sleep_diagnostics.push(SleepDiagnostic {
            sleep,
            at: self.time,
            kind: SleepDiagnosticKind::Started,
            interruption: None,
        });
        Ok(sleep)
    }

    /// Returns the active sleep interval for one agent, if any.
    pub fn sleep(&self, agent: AgentId) -> Option<SleepView> {
        self.population.sleep_view(agent)
    }

    /// Sleep starts, planned wakes, and threshold interruptions from the latest advancing tick.
    pub fn sleep_diagnostics(&self) -> &[SleepDiagnostic] {
        &self.sleep_diagnostics
    }

    /// Starts a one-cell shelter build from a cardinally adjacent access cell.
    pub fn request_build_shelter(
        &mut self,
        agent: AgentId,
        site: WorldPosition,
    ) -> Result<StructureView, BuildShelterError> {
        if self.policy_active {
            return Err(BuildShelterError::PolicyControlled);
        }
        self.start_shelter_build(agent, site, PolicyReason::NoUrgentNeed)
    }

    /// Returns at most `limit` live structures in stable identity order.
    pub fn structure_views(&self, limit: usize) -> impl Iterator<Item = StructureView> + '_ {
        self.structures.views(limit)
    }

    /// Construction starts, completions, and cancellations from the latest advancing tick.
    pub fn structure_diagnostics(&self) -> &[StructureDiagnostic] {
        &self.structure_diagnostics
    }

    /// Activates autonomous physical decisions after population initialization.
    pub fn activate_physical_policy(&mut self) -> Result<(), PolicyActivationError> {
        if !self.population.is_initialized() {
            return Err(PolicyActivationError::PopulationNotInitialized);
        }
        if self.policy_active {
            return Err(PolicyActivationError::AlreadyActive);
        }
        if let Some(agent) = self.population.first_non_idle_agent() {
            return Err(PolicyActivationError::AgentCommitted { agent });
        }
        let due = self
            .time
            .checked_add(1)
            .ok_or(PolicyActivationError::TimeOverflow)?;
        if !self.scheduler.can_schedule(self.population.len() as u64) {
            return Err(PolicyActivationError::EventSequenceExhausted);
        }
        self.population
            .activate_policy(&mut self.scheduler, due)
            .map_err(|_| PolicyActivationError::EventSequenceExhausted)?;
        self.policy_active = true;
        Ok(())
    }

    pub fn physical_policy(&self, agent: AgentId) -> Option<PhysicalPolicyView> {
        self.population.policy_view(agent)
    }

    /// Compact policy selections, commitments, and failures from the latest advancing tick.
    pub fn policy_diagnostics(&self) -> &[PolicyDiagnostic] {
        &self.policy_diagnostics
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
            living_agent_count: self.population.living_count() as u32,
            active_agent_count: self.population.active_count() as u32,
            death_count: self.death_records.len() as u32,
            scheduled_event_count: self.scheduler.len() as u32,
            structure_count: self.structures.len() as u32,
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
            RouteEnvironment {
                world: &self.world,
                occupancy: self.population.spatial(),
                structures: &self.structures,
                active_area,
            },
            agent,
            origin,
            request,
        ) {
            Ok(plan) => {
                if let Err(error) = self.population.schedule_route_step(
                    &mut self.scheduler,
                    MovementEnvironment {
                        world: &self.world,
                        structures: &self.structures,
                    },
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
        if self.policy_active
            && let Some((goal, target, reason)) = self.population.policy_commitment(agent)
        {
            if kind == RouteOutcomeKind::Arrived {
                if self
                    .population
                    .schedule_policy_decision(
                        &mut self.scheduler,
                        self.time,
                        agent,
                        1,
                        PolicyReason::RouteArrived,
                        false,
                    )
                    .is_err()
                {
                    self.policy_diagnostics.push(PolicyDiagnostic {
                        agent,
                        at: self.time,
                        goal,
                        target: Some(target),
                        reason,
                        kind: PolicyDiagnosticKind::RetryScheduled,
                        failure: Some(PolicyFailureReason::EventSequenceExhausted),
                    });
                }
            } else {
                self.schedule_policy_retry(agent, goal, Some(target), reason, route_failure(kind));
            }
        }
    }

    fn apply_policy_decision(&mut self, event: scheduler::ScheduledEvent) {
        if !self.population.policy_event_is_current(event) {
            self.policy_diagnostics.push(PolicyDiagnostic {
                agent: event.agent,
                at: self.time,
                goal: event.goal,
                target: None,
                reason: PolicyReason::Retry,
                kind: PolicyDiagnosticKind::StaleEvent,
                failure: None,
            });
            return;
        }
        let Some((view, needs, inventory)) = self.population.policy_context(event.agent, self.time)
        else {
            return;
        };
        let perception = match self.perceive_physical(event.agent, PHYSICAL_POLICY_RADIUS) {
            Ok(perception) => perception,
            Err(error) => {
                self.schedule_policy_retry(
                    event.agent,
                    event.goal,
                    None,
                    PolicyReason::Retry,
                    perception_failure(error),
                );
                return;
            }
        };
        let selection = select(view.position, needs, inventory, &perception);
        self.policy_diagnostics.push(PolicyDiagnostic {
            agent: event.agent,
            at: self.time,
            goal: selection.goal,
            target: selection.target,
            reason: selection.reason,
            kind: PolicyDiagnosticKind::Selected,
            failure: None,
        });
        self.apply_policy_selection(event.agent, view.position, selection);
    }

    fn apply_policy_selection(
        &mut self,
        agent: AgentId,
        origin: WorldPosition,
        selection: PolicySelection,
    ) {
        let Some(target) = selection.target else {
            self.schedule_policy_retry(
                agent,
                selection.goal,
                None,
                selection.reason,
                if selection.goal == PhysicalGoal::SeekShelter {
                    PolicyFailureReason::DeferredToLaterSlice
                } else {
                    PolicyFailureReason::NoPerceivedTarget
                },
            );
            return;
        };
        if selection.goal == PhysicalGoal::BuildShelter {
            match self.start_shelter_build(agent, target, selection.reason) {
                Ok(structure) => self.policy_diagnostics.push(PolicyDiagnostic {
                    agent,
                    at: self.time,
                    goal: selection.goal,
                    target: Some(structure.position),
                    reason: selection.reason,
                    kind: PolicyDiagnosticKind::ActionStarted,
                    failure: None,
                }),
                Err(error) => self.schedule_policy_retry(
                    agent,
                    selection.goal,
                    Some(target),
                    selection.reason,
                    build_failure(error),
                ),
            }
            return;
        }
        if selection.goal == PhysicalGoal::Wait {
            self.population.clear_route(agent);
            if let Err(error) = self.population.schedule_policy_decision(
                &mut self.scheduler,
                self.time,
                agent,
                PHYSICAL_POLICY_IDLE_RECHECK_TICKS,
                PolicyReason::NoUrgentNeed,
                false,
            ) {
                self.schedule_policy_retry(
                    agent,
                    selection.goal,
                    Some(target),
                    selection.reason,
                    move_failure(error),
                );
            }
            return;
        }
        if target == origin {
            self.population.clear_route(agent);
            let action_goal = match selection.goal {
                PhysicalGoal::SeekWater => PhysicalGoal::Drink,
                PhysicalGoal::SeekFood => PhysicalGoal::GatherMaterial,
                goal => goal,
            };
            if action_goal == PhysicalGoal::Sleep {
                let quality = match self.population.validate_sleep_location(
                    &self.world,
                    self.time,
                    agent,
                    target,
                    self.structures.is_sheltered_access(target),
                    self.structures.structure_at(target),
                ) {
                    Ok(quality) => quality,
                    Err(error) => {
                        self.schedule_policy_retry(
                            agent,
                            action_goal,
                            Some(target),
                            selection.reason,
                            sleep_failure(error),
                        );
                        return;
                    }
                };
                match self.population.schedule_sleep(
                    &mut self.scheduler,
                    self.time,
                    agent,
                    target,
                    quality,
                    selection.reason,
                ) {
                    Ok(sleep) => {
                        self.policy_diagnostics.push(PolicyDiagnostic {
                            agent,
                            at: self.time,
                            goal: action_goal,
                            target: Some(target),
                            reason: selection.reason,
                            kind: PolicyDiagnosticKind::ActionStarted,
                            failure: None,
                        });
                        self.sleep_diagnostics.push(SleepDiagnostic {
                            sleep,
                            at: self.time,
                            kind: SleepDiagnosticKind::Started,
                            interruption: None,
                        });
                    }
                    Err(error) => self.schedule_policy_retry(
                        agent,
                        action_goal,
                        Some(target),
                        selection.reason,
                        sleep_failure(error),
                    ),
                }
                return;
            }
            match self.population.schedule_policy_action(
                &mut self.scheduler,
                self.time,
                agent,
                PolicyAction {
                    goal: action_goal,
                    target,
                    reason: selection.reason,
                    duration: PHYSICAL_POLICY_ACTION_TICKS,
                },
            ) {
                Ok(_) => self.policy_diagnostics.push(PolicyDiagnostic {
                    agent,
                    at: self.time,
                    goal: action_goal,
                    target: Some(target),
                    reason: selection.reason,
                    kind: PolicyDiagnosticKind::ActionStarted,
                    failure: None,
                }),
                Err(error) => self.schedule_policy_retry(
                    agent,
                    action_goal,
                    Some(target),
                    selection.reason,
                    move_failure(error),
                ),
            }
            return;
        }
        match self.schedule_route(
            agent,
            RouteRequest {
                destination: target,
                max_expansions: PHYSICAL_POLICY_ROUTE_BUDGET,
            },
        ) {
            Ok(_) => {
                self.population.commit_policy_route(
                    agent,
                    selection.goal,
                    target,
                    selection.reason,
                );
                self.policy_diagnostics.push(PolicyDiagnostic {
                    agent,
                    at: self.time,
                    goal: selection.goal,
                    target: Some(target),
                    reason: selection.reason,
                    kind: PolicyDiagnosticKind::RouteScheduled,
                    failure: None,
                });
            }
            Err(error) => self.schedule_policy_retry(
                agent,
                selection.goal,
                Some(target),
                selection.reason,
                request_failure(error),
            ),
        }
    }

    fn apply_policy_action_completion(&mut self, event: scheduler::ScheduledEvent) {
        let completion = self
            .population
            .complete_policy_action(&mut self.scheduler, event);
        let Some((goal, target, reason)) = (match completion {
            Ok(completion) => completion,
            Err(error) => {
                if event.goal == PhysicalGoal::BuildShelter {
                    self.cancel_construction(event.agent);
                }
                self.policy_diagnostics.push(PolicyDiagnostic {
                    agent: event.agent,
                    at: self.time,
                    goal: event.goal,
                    target: Some(event.target.world()),
                    reason: PolicyReason::Retry,
                    kind: PolicyDiagnosticKind::ActionCompleted,
                    failure: Some(move_failure(error)),
                });
                return;
            }
        }) else {
            self.policy_diagnostics.push(PolicyDiagnostic {
                agent: event.agent,
                at: self.time,
                goal: event.goal,
                target: Some(event.target.world()),
                reason: PolicyReason::Retry,
                kind: PolicyDiagnosticKind::StaleEvent,
                failure: None,
            });
            return;
        };
        let result = match goal {
            PhysicalGoal::Drink => self.apply_drink(event.agent, target),
            PhysicalGoal::Eat => self.apply_eat(event.agent),
            PhysicalGoal::GatherMaterial => self.apply_gather(event.agent, reason),
            PhysicalGoal::Sleep => {
                let sleep = self.population.finish_sleep(event.agent);
                if let Some(sleep) = sleep {
                    self.sleep_diagnostics.push(SleepDiagnostic {
                        sleep,
                        at: self.time,
                        kind: SleepDiagnosticKind::Woke,
                        interruption: None,
                    });
                    Ok(())
                } else {
                    Err(PolicyFailureReason::InconsistentState)
                }
            }
            PhysicalGoal::BuildShelter => self.apply_build_completion(event.agent),
            PhysicalGoal::SeekShelter | PhysicalGoal::Incapacitated => {
                Err(PolicyFailureReason::DeferredToLaterSlice)
            }
            PhysicalGoal::SeekWater | PhysicalGoal::SeekFood | PhysicalGoal::Wait => {
                Err(PolicyFailureReason::InconsistentState)
            }
        };
        match result {
            Ok(()) => {
                self.policy_diagnostics.push(PolicyDiagnostic {
                    agent: event.agent,
                    at: self.time,
                    goal,
                    target: Some(target),
                    reason,
                    kind: PolicyDiagnosticKind::ActionCompleted,
                    failure: None,
                });
                if self.policy_active
                    && let Err(error) = self.population.schedule_policy_decision(
                        &mut self.scheduler,
                        self.time,
                        event.agent,
                        1,
                        PolicyReason::ActionCompleted,
                        false,
                    )
                {
                    self.schedule_policy_retry(
                        event.agent,
                        goal,
                        Some(target),
                        PolicyReason::Retry,
                        move_failure(error),
                    );
                }
            }
            Err(failure) => {
                let deferred = failure == PolicyFailureReason::DeferredToLaterSlice;
                self.policy_diagnostics.push(PolicyDiagnostic {
                    agent: event.agent,
                    at: self.time,
                    goal,
                    target: Some(target),
                    reason,
                    kind: if deferred {
                        PolicyDiagnosticKind::ActionDeferred
                    } else {
                        PolicyDiagnosticKind::ActionCompleted
                    },
                    failure: Some(failure),
                });
                if self.policy_active {
                    self.schedule_policy_retry(
                        event.agent,
                        goal,
                        Some(target),
                        PolicyReason::Retry,
                        failure,
                    );
                }
            }
        }
    }

    fn apply_drink(
        &mut self,
        agent: AgentId,
        target: WorldPosition,
    ) -> Result<(), PolicyFailureReason> {
        let position = self
            .population
            .view(agent)
            .ok_or(PolicyFailureReason::InconsistentState)?
            .position;
        if position != target || !self.has_adjacent_drinkable_water(position)? {
            return Err(PolicyFailureReason::InvalidWaterAccess);
        }
        self.population
            .apply_need_relief(
                &mut self.scheduler,
                self.time,
                agent,
                NeedKind::Thirst,
                DRINK_THIRST_RELIEF,
                false,
            )
            .map_err(action_effect_failure)
    }

    fn apply_eat(&mut self, agent: AgentId) -> Result<(), PolicyFailureReason> {
        self.population
            .apply_need_relief(
                &mut self.scheduler,
                self.time,
                agent,
                NeedKind::Hunger,
                EAT_HUNGER_RELIEF,
                true,
            )
            .map_err(action_effect_failure)
    }

    fn apply_gather(
        &mut self,
        agent: AgentId,
        reason: PolicyReason,
    ) -> Result<(), PolicyFailureReason> {
        let position = self
            .population
            .view(agent)
            .ok_or(PolicyFailureReason::InconsistentState)?
            .position;
        let inventory = self
            .population
            .inventory(agent)
            .ok_or(PolicyFailureReason::InconsistentState)?;
        let candidate = [
            WorldPosition {
                x: position.x,
                y: position.y - 1,
            },
            WorldPosition {
                x: position.x - 1,
                y: position.y,
            },
            position,
            WorldPosition {
                x: position.x + 1,
                y: position.y,
            },
            WorldPosition {
                x: position.x,
                y: position.y + 1,
            },
        ]
        .into_iter()
        .filter_map(|candidate| {
            self.resource_deltas
                .resource_at(&self.world, candidate)
                .ok()
                .flatten()
                .filter(|resource| {
                    inventory.can_add(resource.kind)
                        && (reason != PolicyReason::HungerThreshold
                            || resource.kind == ResourceKind::Food)
                        && (reason != PolicyReason::ShelterMaterials
                            || (resource.kind == ResourceKind::Wood
                                && inventory.wood < SHELTER_WOOD_COST))
                })
                .map(|resource| (candidate, resource))
        })
        .min_by_key(|(candidate, resource)| {
            (
                position.x.abs_diff(candidate.x) + position.y.abs_diff(candidate.y),
                candidate.y,
                candidate.x,
                resource.kind as u8,
            )
        });
        let Some((resource_position, resource)) = candidate else {
            return Err(
                if [ResourceKind::Food, ResourceKind::Wood, ResourceKind::Stone]
                    .into_iter()
                    .all(|kind| !inventory.can_add(kind))
                {
                    PolicyFailureReason::InventoryFull
                } else {
                    PolicyFailureReason::ResourceDepleted
                },
            );
        };
        let maximum = inventory
            .remaining_capacity(resource.kind)
            .min(GATHER_YIELD);
        let Some((kind, gathered)) = self
            .resource_deltas
            .gather(&self.world, resource_position, maximum)
            .map_err(|_| PolicyFailureReason::TargetUnavailable)?
        else {
            return Err(PolicyFailureReason::ResourceDepleted);
        };
        let accepted = self.population.add_inventory(agent, kind, gathered);
        debug_assert_eq!(accepted, gathered);
        Ok(())
    }

    fn has_adjacent_drinkable_water(
        &self,
        position: WorldPosition,
    ) -> Result<bool, PolicyFailureReason> {
        let mut failure = None;
        for candidate in [
            position,
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
        ] {
            match self.world.water_at(candidate) {
                Ok(Some(source)) if source.is_drinkable() => return Ok(true),
                Ok(_) => {}
                Err(WorldQueryError::Unloaded) => failure = Some(PolicyFailureReason::Unloaded),
                Err(WorldQueryError::OutsideWorldBounds) => {
                    failure.get_or_insert(PolicyFailureReason::OutsideWorld);
                }
                Err(WorldQueryError::NonCardinalStep) => {
                    return Err(PolicyFailureReason::InconsistentState);
                }
            }
        }
        failure.map_or(Ok(false), Err)
    }

    fn start_shelter_build(
        &mut self,
        agent: AgentId,
        site: WorldPosition,
        reason: PolicyReason,
    ) -> Result<StructureView, BuildShelterError> {
        let view = self
            .population
            .view(agent)
            .ok_or(BuildShelterError::MissingAgent)?;
        if view.activity == AgentActivity::Dead {
            return Err(BuildShelterError::DeadAgent);
        }
        if view.activity != AgentActivity::Idle {
            return Err(BuildShelterError::AgentCommitted);
        }
        if !WORLD_GENERATION_BOUNDS.contains(site) {
            return Err(BuildShelterError::OutsideWorld);
        }
        if !self
            .population
            .active_area()
            .is_some_and(|area| area.contains(site))
        {
            return Err(BuildShelterError::OutsideActiveArea);
        }
        if view.position.x.abs_diff(site.x) + view.position.y.abs_diff(site.y) != 1 {
            return Err(BuildShelterError::NotCardinallyAdjacent);
        }
        self.structures.can_start(agent, site)?;
        if let Some(occupant) = self.population.spatial().occupant(site) {
            return Err(BuildShelterError::Occupied(occupant));
        }
        match self.world.standability_at(site) {
            Ok(Standability::Standable) => {}
            Ok(Standability::BlockedByWater) => return Err(BuildShelterError::Water),
            Ok(Standability::BlockedByFeature) => {
                return Err(BuildShelterError::BlockingFeature);
            }
            Err(WorldQueryError::Unloaded) => return Err(BuildShelterError::Unloaded),
            Err(WorldQueryError::OutsideWorldBounds) => {
                return Err(BuildShelterError::OutsideWorld);
            }
            Err(WorldQueryError::NonCardinalStep) => unreachable!("standing queries have no step"),
        }
        if !self.population.can_build_shelter(agent) {
            return Err(BuildShelterError::InsufficientMaterials);
        }
        self.compact_scheduler_if_needed();
        let due = self
            .population
            .schedule_policy_action(
                &mut self.scheduler,
                self.time,
                agent,
                PolicyAction {
                    goal: PhysicalGoal::BuildShelter,
                    target: site,
                    reason,
                    duration: SHELTER_BUILD_TICKS,
                },
            )
            .map_err(map_build_schedule_error)?;
        let structure = self
            .structures
            .start(agent, site, self.time, due)
            .expect("construction capacity and conflicts were prevalidated");
        self.population
            .consume_shelter_materials(agent)
            .expect("shelter recipe was prevalidated");
        self.structure_diagnostics.push(StructureDiagnostic {
            structure,
            at: self.time,
            kind: StructureDiagnosticKind::Started,
            refunded_wood: 0,
            refunded_stone: 0,
        });
        Ok(structure)
    }

    fn apply_build_completion(&mut self, agent: AgentId) -> Result<(), PolicyFailureReason> {
        let structure = self
            .structures
            .complete_for_builder(agent)
            .ok_or(PolicyFailureReason::InconsistentState)?;
        self.structure_diagnostics.push(StructureDiagnostic {
            structure,
            at: self.time,
            kind: StructureDiagnosticKind::Completed,
            refunded_wood: 0,
            refunded_stone: 0,
        });
        Ok(())
    }

    fn cancel_construction(&mut self, agent: AgentId) -> bool {
        let Some(structure) = self.structures.cancel_for_builder(agent) else {
            return false;
        };
        self.population.refund_shelter_materials(agent);
        self.structure_diagnostics.push(StructureDiagnostic {
            structure,
            at: self.time,
            kind: StructureDiagnosticKind::Cancelled,
            refunded_wood: SHELTER_WOOD_COST,
            refunded_stone: SHELTER_STONE_COST,
        });
        true
    }

    fn schedule_policy_retry(
        &mut self,
        agent: AgentId,
        goal: PhysicalGoal,
        target: Option<WorldPosition>,
        reason: PolicyReason,
        failure: PolicyFailureReason,
    ) {
        self.population.clear_route(agent);
        let delay = retry_delay(self.population.policy_retries(agent));
        let scheduling_failure = self
            .population
            .schedule_policy_decision(
                &mut self.scheduler,
                self.time,
                agent,
                delay,
                PolicyReason::Retry,
                true,
            )
            .err()
            .map(move_failure);
        self.policy_diagnostics.push(PolicyDiagnostic {
            agent,
            at: self.time,
            goal,
            target,
            reason,
            kind: PolicyDiagnosticKind::RetryScheduled,
            failure: scheduling_failure.or(Some(failure)),
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
        MoveRequestError::PolicyControlled => RouteRequestError::PolicyControlled,
        MoveRequestError::MissingAgent => RouteRequestError::MissingAgent,
        MoveRequestError::DeadAgent => RouteRequestError::DeadAgent,
        MoveRequestError::InvalidStep => RouteRequestError::NoPath { expansions: 0 },
        MoveRequestError::Unloaded => RouteRequestError::Unloaded,
        MoveRequestError::OutsideWorld => RouteRequestError::OutsideWorld,
        MoveRequestError::OutsideActiveArea => RouteRequestError::OutsideActiveArea,
        MoveRequestError::Blocked(kind) => RouteRequestError::Blocked(kind),
        MoveRequestError::Occupied(agent) => RouteRequestError::Occupied(agent),
        MoveRequestError::BlockedByStructure(structure) => {
            RouteRequestError::BlockedByStructure(structure)
        }
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
        RouteRequestError::BlockedByStructure(structure) => {
            RouteOutcomeKind::BlockedByStructure(structure)
        }
        RouteRequestError::TimeOverflow => RouteOutcomeKind::TimeOverflow,
        RouteRequestError::RescheduleLimit => RouteOutcomeKind::RescheduleLimit,
        RouteRequestError::EventSequenceExhausted => RouteOutcomeKind::EventSequenceExhausted,
        RouteRequestError::PolicyControlled
        | RouteRequestError::MissingAgent
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
        MovementOutcomeKind::BlockedByStructure(structure) => {
            RouteOutcomeKind::BlockedByStructure(structure)
        }
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

fn perception_failure(error: PerceptionError) -> PolicyFailureReason {
    match error {
        PerceptionError::Unloaded => PolicyFailureReason::Unloaded,
        PerceptionError::OutsideWorld => PolicyFailureReason::OutsideWorld,
        PerceptionError::AreaOutsideActive => PolicyFailureReason::OutsideActiveArea,
        PerceptionError::MissingAgent
        | PerceptionError::DeadAgent
        | PerceptionError::RadiusTooLarge { .. }
        | PerceptionError::EmptyArea
        | PerceptionError::AreaTooLarge { .. }
        | PerceptionError::AllocationFailed => PolicyFailureReason::InconsistentState,
    }
}

fn request_failure(error: RouteRequestError) -> PolicyFailureReason {
    match error {
        RouteRequestError::Occupied(_) => PolicyFailureReason::Occupied,
        RouteRequestError::NoPath { .. } => PolicyFailureReason::NoPath,
        RouteRequestError::BudgetExhausted { .. } => PolicyFailureReason::RouteBudgetExhausted,
        RouteRequestError::Unloaded => PolicyFailureReason::Unloaded,
        RouteRequestError::OutsideWorld => PolicyFailureReason::OutsideWorld,
        RouteRequestError::OutsideActiveArea => PolicyFailureReason::OutsideActiveArea,
        RouteRequestError::Blocked(_)
        | RouteRequestError::BlockedByStructure(_)
        | RouteRequestError::AlreadyAtDestination => PolicyFailureReason::TargetUnavailable,
        RouteRequestError::TimeOverflow => PolicyFailureReason::TimeOverflow,
        RouteRequestError::RescheduleLimit => PolicyFailureReason::RescheduleLimit,
        RouteRequestError::EventSequenceExhausted => PolicyFailureReason::EventSequenceExhausted,
        RouteRequestError::PolicyControlled
        | RouteRequestError::MissingAgent
        | RouteRequestError::DeadAgent
        | RouteRequestError::ZeroBudget
        | RouteRequestError::BudgetTooLarge { .. } => PolicyFailureReason::InconsistentState,
    }
}

fn route_failure(kind: RouteOutcomeKind) -> PolicyFailureReason {
    match kind {
        RouteOutcomeKind::Occupied(_) => PolicyFailureReason::Occupied,
        RouteOutcomeKind::NoPath { .. } => PolicyFailureReason::NoPath,
        RouteOutcomeKind::BudgetExhausted { .. } => PolicyFailureReason::RouteBudgetExhausted,
        RouteOutcomeKind::Unloaded => PolicyFailureReason::Unloaded,
        RouteOutcomeKind::OutsideWorld => PolicyFailureReason::OutsideWorld,
        RouteOutcomeKind::OutsideActiveArea => PolicyFailureReason::OutsideActiveArea,
        RouteOutcomeKind::Blocked(_) | RouteOutcomeKind::BlockedByStructure(_) => {
            PolicyFailureReason::TargetUnavailable
        }
        RouteOutcomeKind::TimeOverflow => PolicyFailureReason::TimeOverflow,
        RouteOutcomeKind::RescheduleLimit => PolicyFailureReason::RescheduleLimit,
        RouteOutcomeKind::EventSequenceExhausted => PolicyFailureReason::EventSequenceExhausted,
        RouteOutcomeKind::InconsistentOccupancy | RouteOutcomeKind::Arrived => {
            PolicyFailureReason::InconsistentState
        }
    }
}

fn move_failure(error: MoveRequestError) -> PolicyFailureReason {
    match error {
        MoveRequestError::Occupied(_) => PolicyFailureReason::Occupied,
        MoveRequestError::Unloaded => PolicyFailureReason::Unloaded,
        MoveRequestError::OutsideWorld => PolicyFailureReason::OutsideWorld,
        MoveRequestError::OutsideActiveArea => PolicyFailureReason::OutsideActiveArea,
        MoveRequestError::Blocked(_)
        | MoveRequestError::BlockedByStructure(_)
        | MoveRequestError::InvalidStep => PolicyFailureReason::TargetUnavailable,
        MoveRequestError::TimeOverflow => PolicyFailureReason::TimeOverflow,
        MoveRequestError::RescheduleLimit => PolicyFailureReason::RescheduleLimit,
        MoveRequestError::EventSequenceExhausted => PolicyFailureReason::EventSequenceExhausted,
        MoveRequestError::PolicyControlled
        | MoveRequestError::MissingAgent
        | MoveRequestError::DeadAgent => PolicyFailureReason::InconsistentState,
    }
}

fn sleep_failure(error: SleepRequestError) -> PolicyFailureReason {
    match error {
        SleepRequestError::Water => PolicyFailureReason::SleepLocationWater,
        SleepRequestError::BlockingFeature => PolicyFailureReason::SleepLocationBlocked,
        SleepRequestError::Occupied(_) => PolicyFailureReason::SleepLocationOccupied,
        SleepRequestError::StructureOccupied(_) => PolicyFailureReason::SleepLocationOccupied,
        SleepRequestError::UnsafeExposure => PolicyFailureReason::SleepLocationUnsafe,
        SleepRequestError::OutsideActiveArea
        | SleepRequestError::OutsideWorld
        | SleepRequestError::Unloaded
        | SleepRequestError::NotAtLocation => PolicyFailureReason::SleepLocationUnavailable,
        SleepRequestError::TimeOverflow => PolicyFailureReason::TimeOverflow,
        SleepRequestError::RescheduleLimit => PolicyFailureReason::RescheduleLimit,
        SleepRequestError::EventSequenceExhausted => PolicyFailureReason::EventSequenceExhausted,
        SleepRequestError::MissingAgent
        | SleepRequestError::DeadAgent
        | SleepRequestError::PolicyControlled
        | SleepRequestError::AgentCommitted => PolicyFailureReason::InconsistentState,
    }
}

fn action_effect_failure(error: ActionEffectError) -> PolicyFailureReason {
    match error {
        ActionEffectError::NoEdibleInventory => PolicyFailureReason::NoEdibleInventory,
        ActionEffectError::EventSequenceExhausted => PolicyFailureReason::EventSequenceExhausted,
    }
}

fn map_build_schedule_error(error: MoveRequestError) -> BuildShelterError {
    match error {
        MoveRequestError::MissingAgent => BuildShelterError::MissingAgent,
        MoveRequestError::DeadAgent => BuildShelterError::DeadAgent,
        MoveRequestError::TimeOverflow => BuildShelterError::TimeOverflow,
        MoveRequestError::RescheduleLimit => BuildShelterError::RescheduleLimit,
        MoveRequestError::EventSequenceExhausted => BuildShelterError::EventSequenceExhausted,
        MoveRequestError::PolicyControlled
        | MoveRequestError::InvalidStep
        | MoveRequestError::Unloaded
        | MoveRequestError::OutsideWorld
        | MoveRequestError::OutsideActiveArea
        | MoveRequestError::Blocked(_)
        | MoveRequestError::Occupied(_)
        | MoveRequestError::BlockedByStructure(_) => BuildShelterError::AgentCommitted,
    }
}

fn build_failure(error: BuildShelterError) -> PolicyFailureReason {
    match error {
        BuildShelterError::InsufficientMaterials => PolicyFailureReason::InsufficientMaterials,
        BuildShelterError::Occupied(_) | BuildShelterError::StructureOccupied(_) => {
            PolicyFailureReason::Occupied
        }
        BuildShelterError::Unloaded => PolicyFailureReason::Unloaded,
        BuildShelterError::OutsideWorld => PolicyFailureReason::OutsideWorld,
        BuildShelterError::OutsideActiveArea => PolicyFailureReason::OutsideActiveArea,
        BuildShelterError::TimeOverflow => PolicyFailureReason::TimeOverflow,
        BuildShelterError::RescheduleLimit => PolicyFailureReason::RescheduleLimit,
        BuildShelterError::EventSequenceExhausted => PolicyFailureReason::EventSequenceExhausted,
        BuildShelterError::Water
        | BuildShelterError::BlockingFeature
        | BuildShelterError::NotCardinallyAdjacent => PolicyFailureReason::BuildSiteInvalid,
        BuildShelterError::MissingAgent
        | BuildShelterError::DeadAgent
        | BuildShelterError::PolicyControlled
        | BuildShelterError::AgentCommitted
        | BuildShelterError::StructureLimit => PolicyFailureReason::InconsistentState,
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

    #[test]
    fn policy_failure_mapping_preserves_typed_runtime_categories() {
        assert_eq!(
            perception_failure(PerceptionError::Unloaded),
            PolicyFailureReason::Unloaded
        );
        assert_eq!(
            request_failure(RouteRequestError::Occupied(AgentId::new(7))),
            PolicyFailureReason::Occupied
        );
        assert_eq!(
            request_failure(RouteRequestError::NoPath { expansions: 9 }),
            PolicyFailureReason::NoPath
        );
        assert_eq!(
            request_failure(RouteRequestError::BudgetExhausted { expansions: 9 }),
            PolicyFailureReason::RouteBudgetExhausted
        );
        assert_eq!(
            request_failure(RouteRequestError::OutsideActiveArea),
            PolicyFailureReason::OutsideActiveArea
        );
        assert_eq!(
            route_failure(RouteOutcomeKind::Blocked(TraversalKind::BlockedByWater)),
            PolicyFailureReason::TargetUnavailable
        );
        assert_eq!(
            move_failure(MoveRequestError::EventSequenceExhausted),
            PolicyFailureReason::EventSequenceExhausted
        );
        assert_eq!(
            move_failure(MoveRequestError::RescheduleLimit),
            PolicyFailureReason::RescheduleLimit
        );
        assert_eq!(
            move_failure(MoveRequestError::TimeOverflow),
            PolicyFailureReason::TimeOverflow
        );
    }

    fn resident_engine(size: u32) -> Engine {
        let mut engine = Engine::new(EngineConfig {
            seed: 42,
            world: WorldConfig::new(size, size).unwrap(),
            ..EngineConfig::default()
        });
        engine.materialize_initial_area().unwrap();
        engine
    }

    fn standable_shelter_site(engine: &Engine) -> (WorldPosition, WorldPosition) {
        standable_steps(engine, 1)[0]
    }

    #[test]
    fn shelter_build_blocks_travel_completes_and_enables_safer_sleep() {
        let mut engine = resident_engine(64);
        let (access, site) = standable_shelter_site(&engine);
        engine
            .initialize_population(
                PopulationInit {
                    active_area: engine.world().initial_bounds(),
                    population: 1,
                },
                &[access],
            )
            .unwrap();
        engine
            .population
            .add_inventory(AgentId::new(0), ResourceKind::Wood, SHELTER_WOOD_COST);
        engine
            .population
            .add_inventory(AgentId::new(0), ResourceKind::Stone, SHELTER_STONE_COST);

        let started = engine.request_build_shelter(AgentId::new(0), site).unwrap();
        assert_eq!(started.state, StructureState::UnderConstruction);
        assert_eq!(engine.snapshot().structure_count, 1);
        assert_eq!(engine.inventory(AgentId::new(0)).unwrap().wood, 0);
        assert_eq!(engine.inventory(AgentId::new(0)).unwrap().stone, 0);
        let perception = engine.perceive_physical(AgentId::new(0), 2).unwrap();
        assert_eq!(perception.structures, [started]);
        assert!(!perception.traversable_cells.contains(&site));
        assert_eq!(
            engine.request_move(AgentId::new(0), site),
            Err(MoveRequestError::BlockedByStructure(started.id))
        );
        assert_eq!(
            engine.request_route(
                AgentId::new(0),
                RouteRequest {
                    destination: site,
                    max_expansions: 16,
                },
            ),
            Err(RouteRequestError::BlockedByStructure(started.id))
        );

        while engine.snapshot().tick < started.completes_at.ticks() {
            engine.tick();
        }
        let completed = engine.structure_views(1).next().unwrap();
        assert_eq!(completed.state, StructureState::Complete);
        assert_eq!(completed.builder, None);
        assert_eq!(engine.structure_diagnostics().len(), 1);
        assert_eq!(
            engine.structure_diagnostics()[0].kind,
            StructureDiagnosticKind::Completed
        );

        let exposure_before = engine
            .physical_needs(AgentId::new(0))
            .unwrap()
            .exposure
            .value;
        let sleep = engine.request_sleep(AgentId::new(0), access).unwrap();
        assert_eq!(sleep.quality, SleepQuality::Sheltered);
        for _ in 0..60 {
            engine.tick();
        }
        assert!(
            engine
                .physical_needs(AgentId::new(0))
                .unwrap()
                .exposure
                .value
                < exposure_before
        );
    }

    #[test]
    fn construction_interruption_refunds_once_and_stale_completion_is_harmless() {
        let mut engine = resident_engine(64);
        let (access, site) = standable_shelter_site(&engine);
        engine
            .initialize_population(
                PopulationInit {
                    active_area: engine.world().initial_bounds(),
                    population: 1,
                },
                &[access],
            )
            .unwrap();
        engine
            .population
            .add_inventory(AgentId::new(0), ResourceKind::Wood, SHELTER_WOOD_COST);
        engine
            .population
            .add_inventory(AgentId::new(0), ResourceKind::Stone, SHELTER_STONE_COST);
        engine.time = SimTime::from_ticks(89_950);
        let started = engine
            .start_shelter_build(AgentId::new(0), site, PolicyReason::NoUrgentNeed)
            .unwrap();

        while engine.snapshot().structure_count != 0 {
            engine.tick();
        }
        let cancelled_at = engine.snapshot().tick;
        assert_eq!(engine.snapshot().structure_count, 0);
        assert_eq!(
            engine.inventory(AgentId::new(0)).unwrap().wood,
            SHELTER_WOOD_COST
        );
        assert_eq!(
            engine.inventory(AgentId::new(0)).unwrap().stone,
            SHELTER_STONE_COST
        );
        assert_eq!(
            engine.structure_diagnostics()[0].kind,
            StructureDiagnosticKind::Cancelled
        );
        assert_eq!(
            engine.agent_views(1).next().unwrap().activity,
            AgentActivity::Idle
        );
        while engine.snapshot().tick <= started.completes_at.ticks().max(cancelled_at) {
            engine.tick();
        }
        assert_eq!(engine.snapshot().structure_count, 0);
        assert_eq!(
            engine.inventory(AgentId::new(0)).unwrap().wood,
            SHELTER_WOOD_COST
        );
    }

    #[test]
    fn structure_reserved_after_move_request_blocks_at_movement_completion() {
        let mut engine = resident_engine(128);
        let bounds = engine.world().initial_bounds();
        let mut found = None;
        'rows: for y in bounds.min.y + 1..bounds.max.y - 1 {
            for x in bounds.min.x + 1..bounds.max.x - 1 {
                let site = WorldPosition { x, y };
                let from = WorldPosition { x: x - 1, y };
                let builder = WorldPosition { x: x + 1, y };
                if [from, site, builder].into_iter().all(|position| {
                    engine.world().standability_at(position) == Ok(Standability::Standable)
                }) {
                    found = Some((from, site, builder));
                    break 'rows;
                }
            }
        }
        let (from, site, builder) =
            found.expect("seeded world should contain three horizontal standable cells");
        engine
            .initialize_population(
                PopulationInit {
                    active_area: bounds,
                    population: 2,
                },
                &[from, builder],
            )
            .unwrap();
        engine
            .population
            .add_inventory(AgentId::new(1), ResourceKind::Wood, SHELTER_WOOD_COST);
        let movement = engine.request_move(AgentId::new(0), site).unwrap();
        let shelter = engine.request_build_shelter(AgentId::new(1), site).unwrap();
        while engine.snapshot().tick < movement.completes_at.ticks() {
            engine.tick();
        }
        assert_eq!(
            engine.movement_outcomes()[0].kind,
            MovementOutcomeKind::BlockedByStructure(shelter.id)
        );
        assert_eq!(engine.agent_views(1).next().unwrap().position, from);
    }

    #[test]
    fn equal_time_builders_resolve_overlap_by_agent_id_without_double_spending() {
        let mut engine = resident_engine(128);
        let bounds = engine.world().initial_bounds();
        let center = (bounds.min.y + 2..bounds.max.y - 2)
            .flat_map(|y| (bounds.min.x + 2..bounds.max.x - 2).map(move |x| WorldPosition { x, y }))
            .find(|center| {
                (-2..=2).all(|dx| {
                    (-1..=1).all(|dy| {
                        engine.world().standability_at(WorldPosition {
                            x: center.x + dx,
                            y: center.y + dy,
                        }) == Ok(Standability::Standable)
                    })
                })
            })
            .expect("seeded world should contain a standable 5x3 construction test patch");
        let positions = [
            WorldPosition {
                x: center.x - 1,
                y: center.y,
            },
            WorldPosition {
                x: center.x + 1,
                y: center.y,
            },
            WorldPosition {
                x: center.x - 2,
                y: center.y,
            },
            WorldPosition {
                x: center.x - 1,
                y: center.y - 1,
            },
            WorldPosition {
                x: center.x - 1,
                y: center.y + 1,
            },
            WorldPosition {
                x: center.x + 2,
                y: center.y,
            },
            WorldPosition {
                x: center.x + 1,
                y: center.y - 1,
            },
            WorldPosition {
                x: center.x + 1,
                y: center.y + 1,
            },
        ];
        engine
            .initialize_population(
                PopulationInit {
                    active_area: bounds,
                    population: positions.len() as u32,
                },
                &positions,
            )
            .unwrap();
        for agent in [AgentId::new(0), AgentId::new(1)] {
            engine
                .population
                .add_inventory(agent, ResourceKind::Wood, SHELTER_WOOD_COST);
            engine
                .population
                .add_inventory(agent, ResourceKind::Stone, SHELTER_STONE_COST);
        }
        engine.activate_physical_policy().unwrap();
        engine.tick();

        let structure = engine.structure_views(1).next().unwrap();
        assert_eq!(structure.position, center);
        assert_eq!(structure.builder, Some(AgentId::new(0)));
        assert_eq!(engine.inventory(AgentId::new(0)).unwrap().wood, 0);
        assert_eq!(
            engine.inventory(AgentId::new(1)).unwrap().wood,
            SHELTER_WOOD_COST
        );
    }

    #[test]
    #[ignore = "release-only Slice 6 structure layout and scheduler concentration measurement"]
    fn release_slice6_structure_measurement() {
        for count in [20_u32, 100, 10_000] {
            let mut store = StructureStore::default();
            let mut scheduler = Scheduler::with_capacity(count as usize);
            let build_start = Instant::now();
            for raw in 0..count {
                store
                    .start(
                        AgentId::new(raw),
                        WorldPosition {
                            x: i64::from(raw % 200),
                            y: i64::from(raw / 200),
                        },
                        SimTime::ZERO,
                        SimTime::from_ticks(SHELTER_BUILD_TICKS),
                    )
                    .unwrap();
                scheduler
                    .schedule_action_completion(
                        SimTime::from_ticks(SHELTER_BUILD_TICKS),
                        AgentId::new(raw),
                        1,
                        PhysicalGoal::BuildShelter,
                        agent::CompactPosition {
                            x: (raw % 200) as i16,
                            y: (raw / 200) as i16,
                        },
                    )
                    .unwrap();
            }
            let build_ns = build_start.elapsed().as_nanos();
            let extraction_start = Instant::now();
            while scheduler
                .pop_due(SimTime::from_ticks(SHELTER_BUILD_TICKS))
                .is_some()
            {}
            println!(
                "slice6 structures={count} record_bytes={} slot_bytes={} index_entry_bytes={} event_bytes={} retained_slots={} logical_record_bytes={} build_schedule_ns={} due_extract_ns={}",
                size_of::<structures::StructureRecord>(),
                size_of::<Option<structures::StructureRecord>>(),
                size_of::<((i16, i16), StructureId)>(),
                size_of::<scheduler::ScheduledEvent>(),
                store.retained_slots(),
                store.retained_slots() * size_of::<Option<structures::StructureRecord>>(),
                build_ns,
                extraction_start.elapsed().as_nanos(),
            );
        }
    }

    #[test]
    fn eating_consumes_one_food_and_rebases_only_hunger() {
        let mut engine = resident_engine(64);
        let position = standable_steps(&engine, 1)[0].0;
        engine
            .initialize_population(
                PopulationInit {
                    active_area: engine.world().initial_bounds(),
                    population: 1,
                },
                &[position],
            )
            .unwrap();
        engine.time = SimTime::from_ticks(210_000);
        assert_eq!(
            engine
                .population
                .add_inventory(AgentId::new(0), ResourceKind::Food, 2),
            2
        );
        let before = engine.physical_needs(AgentId::new(0)).unwrap();
        engine.apply_eat(AgentId::new(0)).unwrap();
        let after = engine.physical_needs(AgentId::new(0)).unwrap();
        assert_eq!(engine.inventory(AgentId::new(0)).unwrap().food, 1);
        assert_eq!(after.hunger.value, before.hunger.value - EAT_HUNGER_RELIEF);
        assert_eq!(after.thirst.value, before.thirst.value);
        assert_eq!(after.rest.value, before.rest.value);
        assert_eq!(after.exposure.value, before.exposure.value);
        assert_eq!(
            after.next_threshold,
            Some(NeedThreshold {
                kind: NeedKind::Hunger,
                due: SimTime::from_ticks(330_000),
            })
        );
    }

    #[test]
    fn eating_without_food_is_an_explicit_atomic_failure() {
        let mut engine = resident_engine(64);
        let position = standable_steps(&engine, 1)[0].0;
        engine
            .initialize_population(
                PopulationInit {
                    active_area: engine.world().initial_bounds(),
                    population: 1,
                },
                &[position],
            )
            .unwrap();
        engine.time = SimTime::from_ticks(210_000);
        let before = engine.physical_needs(AgentId::new(0)).unwrap();
        assert_eq!(
            engine.apply_eat(AgentId::new(0)),
            Err(PolicyFailureReason::NoEdibleInventory)
        );
        assert_eq!(engine.physical_needs(AgentId::new(0)).unwrap(), before);
        assert_eq!(engine.inventory(AgentId::new(0)).unwrap().food, 0);
    }

    #[test]
    fn inventory_addition_clamps_at_the_per_kind_capacity() {
        let mut engine = resident_engine(64);
        let position = standable_steps(&engine, 1)[0].0;
        engine
            .initialize_population(
                PopulationInit {
                    active_area: engine.world().initial_bounds(),
                    population: 1,
                },
                &[position],
            )
            .unwrap();
        assert_eq!(
            engine
                .population
                .add_inventory(AgentId::new(0), ResourceKind::Wood, u8::MAX),
            INVENTORY_CAPACITY_PER_KIND
        );
        assert_eq!(
            engine
                .population
                .add_inventory(AgentId::new(0), ResourceKind::Wood, 1),
            0
        );
        assert_eq!(
            engine.inventory(AgentId::new(0)).unwrap().wood,
            INVENTORY_CAPACITY_PER_KIND
        );
    }

    #[test]
    fn action_completion_sequence_exhaustion_settles_idle_without_applying_effects() {
        let mut engine = resident_engine(64);
        let position = standable_steps(&engine, 1)[0].0;
        engine
            .initialize_population(
                PopulationInit {
                    active_area: engine.world().initial_bounds(),
                    population: 1,
                },
                &[position],
            )
            .unwrap();
        engine
            .population
            .schedule_policy_action(
                &mut engine.scheduler,
                SimTime::ZERO,
                AgentId::new(0),
                PolicyAction {
                    goal: PhysicalGoal::GatherMaterial,
                    target: position,
                    reason: PolicyReason::NoUrgentNeed,
                    duration: 1,
                },
            )
            .unwrap();
        engine.scheduler.exhaust_sequence();
        engine.tick();
        assert_eq!(
            engine.agent_views(1).next().unwrap().activity,
            AgentActivity::Idle
        );
        assert!(!engine.physical_policy(AgentId::new(0)).unwrap().committed);
        assert_eq!(engine.modified_resource_count(), 0);
        assert!(engine.policy_diagnostics().iter().any(|diagnostic| {
            diagnostic.kind == PolicyDiagnosticKind::ActionCompleted
                && diagnostic.failure == Some(PolicyFailureReason::EventSequenceExhausted)
        }));
    }

    #[test]
    fn drinking_rejects_ocean_only_and_unloaded_access() {
        let coast = WorldRect {
            min: WorldPosition {
                x: -16_128,
                y: -16_896,
            },
            max: WorldPosition {
                x: -15_104,
                y: -15_872,
            },
        };
        let mut ocean_engine = Engine::new(EngineConfig {
            seed: 1,
            world: WorldConfig::new(64, 64).unwrap(),
            ..EngineConfig::default()
        });
        ocean_engine.command(EngineCommand::GenerateWorldArea(coast));
        let bounds = coast;
        let ocean_access = (bounds.min.y + 1..bounds.max.y - 1)
            .flat_map(|y| (bounds.min.x + 1..bounds.max.x - 1).map(move |x| WorldPosition { x, y }))
            .find(|&position| {
                ocean_engine.world().standability_at(position) == Ok(Standability::Standable)
                    && [
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
                    .into_iter()
                    .any(|candidate| {
                        ocean_engine.world().water_at(candidate) == Ok(Some(WaterSource::Ocean))
                    })
            })
            .expect("seeded resident area should contain an ocean shore");
        ocean_engine
            .initialize_population(
                PopulationInit {
                    active_area: bounds,
                    population: 1,
                },
                &[ocean_access],
            )
            .unwrap();
        assert_eq!(
            ocean_engine.apply_drink(AgentId::new(0), ocean_access),
            Err(PolicyFailureReason::InvalidWaterAccess)
        );

        let mut unloaded_engine = resident_engine(64);
        let bounds = unloaded_engine.world().initial_bounds();
        let unloaded_access = (bounds.min.y..bounds.max.y)
            .map(|y| WorldPosition {
                x: bounds.max.x - 1,
                y,
            })
            .find(|&position| {
                unloaded_engine.world().standability_at(position) == Ok(Standability::Standable)
                    && [
                        position,
                        WorldPosition {
                            x: position.x,
                            y: position.y - 1,
                        },
                        WorldPosition {
                            x: position.x - 1,
                            y: position.y,
                        },
                        WorldPosition {
                            x: position.x,
                            y: position.y + 1,
                        },
                    ]
                    .into_iter()
                    .all(|candidate| {
                        unloaded_engine
                            .world()
                            .water_at(candidate)
                            .is_ok_and(|source| !source.is_some_and(WaterSource::is_drinkable))
                    })
            })
            .expect("seeded boundary should contain a dry standable access");
        unloaded_engine
            .initialize_population(
                PopulationInit {
                    active_area: bounds,
                    population: 1,
                },
                &[unloaded_access],
            )
            .unwrap();
        assert_eq!(
            unloaded_engine.apply_drink(AgentId::new(0), unloaded_access),
            Err(PolicyFailureReason::Unloaded)
        );
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

    #[test]
    fn lethal_health_consequence_precedes_movement_and_releases_occupancy() {
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
        engine.population.set_need_value_for_test(
            AgentId::new(0),
            NeedKind::Thirst,
            8_000,
            SimTime::ZERO,
        );
        engine.request_move(AgentId::new(0), target).unwrap();
        engine.population.prepare_health_consequence_for_test(
            &mut engine.scheduler,
            AgentId::new(0),
            HEALTH_INCAPACITATION_THRESHOLD,
            SimTime::ZERO,
        );

        engine.tick();

        assert_eq!(engine.death_records().len(), 1);
        assert_eq!(engine.death_records()[0].cause, DeathCause::Dehydration);
        assert_eq!(engine.death_records()[0].at, SimTime::ZERO);
        assert_eq!(engine.population.spatial().occupant(from), None);
        assert_eq!(engine.population.spatial().occupant(target), None);
        assert_eq!(engine.snapshot().living_agent_count, 0);
        assert_eq!(
            engine.health_diagnostics().last().unwrap().kind,
            HealthDiagnosticKind::Died
        );
    }

    #[test]
    fn terminal_health_cancels_construction_and_makes_completion_stale() {
        let mut engine = resident_engine(64);
        let (access, site) = standable_shelter_site(&engine);
        engine
            .initialize_population(
                PopulationInit {
                    active_area: engine.world().initial_bounds(),
                    population: 1,
                },
                &[access],
            )
            .unwrap();
        engine
            .population
            .add_inventory(AgentId::new(0), ResourceKind::Wood, SHELTER_WOOD_COST);
        let build = engine.request_build_shelter(AgentId::new(0), site).unwrap();
        engine.population.set_need_value_for_test(
            AgentId::new(0),
            NeedKind::Thirst,
            8_000,
            SimTime::ZERO,
        );
        engine.population.prepare_health_consequence_for_test(
            &mut engine.scheduler,
            AgentId::new(0),
            HEALTH_INCAPACITATION_THRESHOLD,
            SimTime::ZERO,
        );

        engine.tick();
        assert_eq!(engine.snapshot().structure_count, 0);
        assert!(engine.structure_diagnostics().iter().any(|diagnostic| {
            diagnostic.structure.id == build.id
                && diagnostic.kind == StructureDiagnosticKind::Cancelled
        }));
        while engine.snapshot().tick <= build.completes_at.ticks() {
            engine.tick();
        }
        assert_eq!(engine.snapshot().structure_count, 0);
        assert_eq!(engine.death_records().len(), 1);
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
    #[ignore = "release-only Slice 4 inventory and resource-delta measurement"]
    fn release_physical_agent_slice_four_measurement() {
        assert!(
            !std::hint::black_box(cfg!(debug_assertions)),
            "run this measurement in release mode"
        );
        eprintln!(
            "population\tinventory_size\tinventory_align\tinventory_capacity\tinventory_logical_bytes"
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
            let inventory_capacity = engine.population.inventory_capacity();
            eprintln!(
                "{population}\t{}\t{}\t{inventory_capacity}\t{}",
                size_of::<InventoryView>(),
                std::mem::align_of::<InventoryView>(),
                inventory_capacity * size_of::<InventoryView>(),
            );
        }

        let engine = resident_engine(256);
        let bounds = engine.world().initial_bounds();
        let position = (bounds.min.y..bounds.max.y)
            .flat_map(|y| (bounds.min.x..bounds.max.x).map(move |x| WorldPosition { x, y }))
            .find(|&position| {
                engine
                    .world()
                    .resource_at(position)
                    .is_ok_and(|value| value.is_some())
            })
            .expect("measurement world should contain one generated resource");
        let base = engine.world().resource_at(position).unwrap().unwrap();
        let mut deltas = ResourceDeltas::default();
        let start = Instant::now();
        let mut gathered = 0_u16;
        while let Some((_, amount)) = deltas.gather(engine.world(), position, 1).unwrap() {
            gathered += u16::from(amount);
        }
        let gather_ns = start.elapsed().as_nanos();
        assert_eq!(gathered, base.capacity);
        assert_eq!(deltas.len(), 1);
        eprintln!(
            "resource_kind={:?}\tbase_capacity={}\tdelta_entry_size={}\tdelta_entry_align={}\tdelta_records={}\tgather_completions={}\tgather_total_ns={gather_ns}",
            base.kind,
            base.capacity,
            size_of::<resources::ResourceDelta>(),
            std::mem::align_of::<resources::ResourceDelta>(),
            deltas.len(),
            gathered,
        );
    }

    #[test]
    fn sleep_rejects_unsafe_exposure_and_sequence_exhaustion_without_partial_state() {
        let mut engine = resident_engine(128);
        let position = standable_steps(&engine, 1)[0].0;
        engine
            .initialize_population(
                PopulationInit {
                    active_area: engine.world().initial_bounds(),
                    population: 1,
                },
                &[position],
            )
            .unwrap();
        engine.population.set_need_value_for_test(
            AgentId::new(0),
            NeedKind::Exposure,
            7_000,
            engine.time,
        );
        assert_eq!(
            engine.request_sleep(AgentId::new(0), position),
            Err(SleepRequestError::UnsafeExposure)
        );
        assert_eq!(engine.sleep(AgentId::new(0)), None);
        assert_eq!(
            engine.agent_views(1).next().unwrap().activity,
            AgentActivity::Idle
        );

        engine.population.set_need_value_for_test(
            AgentId::new(0),
            NeedKind::Exposure,
            0,
            engine.time,
        );
        engine.scheduler.exhaust_sequence();
        let before = engine.physical_needs(AgentId::new(0)).unwrap();
        assert_eq!(
            engine.request_sleep(AgentId::new(0), position),
            Err(SleepRequestError::EventSequenceExhausted)
        );
        assert_eq!(engine.sleep(AgentId::new(0)), None);
        assert_eq!(engine.physical_needs(AgentId::new(0)).unwrap(), before);
        assert_eq!(
            engine.agent_views(1).next().unwrap().activity,
            AgentActivity::Idle
        );
    }

    #[test]
    #[ignore = "release-only Slice 5 sleep-state and wake-event measurement"]
    fn release_physical_agent_slice_five_measurement() {
        assert!(
            !std::hint::black_box(cfg!(debug_assertions)),
            "run this measurement in release mode"
        );
        eprintln!(
            "population\tsleep_state_size\tsleep_state_align\tsleep_capacity\tsleep_logical_bytes\tscheduled_events\tschedule_ns\twake_extract_ns"
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
            let positions: Vec<_> = engine
                .population
                .views(population as usize)
                .map(|view| view.position)
                .collect();
            let schedule_start = Instant::now();
            for (raw, position) in positions.into_iter().enumerate() {
                engine
                    .population
                    .schedule_sleep(
                        &mut engine.scheduler,
                        SimTime::ZERO,
                        AgentId::new(raw as u32),
                        position,
                        SleepQuality::OpenGround,
                        PolicyReason::RestThreshold,
                    )
                    .unwrap();
            }
            let schedule_ns = schedule_start.elapsed().as_nanos();
            let scheduled_events = engine.scheduler.len();
            let wake_start = Instant::now();
            let mut extracted = 0;
            while engine.scheduler.pop_due(SimTime::from_ticks(1)).is_some() {
                extracted += 1;
            }
            let wake_extract_ns = wake_start.elapsed().as_nanos();
            assert_eq!(extracted, population as usize);
            let sleep_capacity = engine.population.sleep_capacity();
            eprintln!(
                "{population}\t{}\t{}\t{sleep_capacity}\t{}\t{scheduled_events}\t{schedule_ns}\t{wake_extract_ns}",
                size_of::<sleep::SleepState>(),
                std::mem::align_of::<sleep::SleepState>(),
                sleep_capacity * size_of::<sleep::SleepState>(),
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
                        RouteEnvironment {
                            world: &engine.world,
                            occupancy: engine.population.spatial(),
                            structures: &engine.structures,
                            active_area,
                        },
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
                    RouteEnvironment {
                        world: &engine.world,
                        occupancy: engine.population.spatial(),
                        structures: &engine.structures,
                        active_area,
                    },
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

    #[test]
    #[ignore = "release-only Slice 3 physical-policy measurement"]
    fn release_physical_agent_slice_three_measurement() {
        assert!(
            !std::hint::black_box(cfg!(debug_assertions)),
            "run this measurement in release mode"
        );
        eprintln!(
            "population\tpolicy_state_size\tpolicy_state_align\tdecision_event_size\tpolicy_capacity\tscheduler_capacity\tschedule_ns\tdue_extract_ns\tgrowth_buffers\tretained_logical_bytes"
        );
        for population in [20_usize, 100, 10_000] {
            let mut states = Vec::with_capacity(population);
            states.resize(population, policy::PolicyState::default());
            let mut scheduler = Scheduler::with_capacity(population);
            let before_capacity = scheduler.capacity();
            let schedule_start = Instant::now();
            for (raw, state) in states.iter_mut().enumerate() {
                let generation = state.next_generation().unwrap();
                state.phase = policy::PolicyPhase::DecisionPending;
                scheduler
                    .schedule_decision(
                        SimTime::from_ticks(1),
                        AgentId::new(raw as u32),
                        generation,
                        PhysicalGoal::Wait,
                    )
                    .unwrap();
            }
            let schedule_ns = schedule_start.elapsed().as_nanos();
            let growth_buffers = usize::from(scheduler.capacity() != before_capacity);
            let due_start = Instant::now();
            let mut extracted = 0;
            while scheduler.pop_due(SimTime::from_ticks(1)).is_some() {
                extracted += 1;
            }
            let due_extract_ns = due_start.elapsed().as_nanos();
            assert_eq!(extracted, population);
            let retained_logical_bytes = states.capacity() * size_of::<policy::PolicyState>()
                + scheduler.capacity() * size_of::<scheduler::ScheduledEvent>();
            eprintln!(
                "{population}\t{}\t{}\t{}\t{}\t{}\t{schedule_ns}\t{due_extract_ns}\t{growth_buffers}\t{retained_logical_bytes}",
                size_of::<policy::PolicyState>(),
                std::mem::align_of::<policy::PolicyState>(),
                size_of::<scheduler::ScheduledEvent>(),
                states.capacity(),
                scheduler.capacity(),
            );
        }
    }
}
