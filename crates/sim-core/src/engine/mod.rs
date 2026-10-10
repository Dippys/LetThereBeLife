//! The simulation engine: configuration, commands, tick outcomes, snapshots,
//! and the `Engine` state owner. Behaviour is split across submodules by
//! responsibility; each adds an `impl Engine` block.

mod actions;
mod cognition;
mod errors;
mod policy;
mod requests;
mod routes;
mod setup;
pub use setup::SimulationAdvanced;
mod shelter;
mod tick;
mod views;
mod wildlife;
pub use wildlife::{HUNT_TICKS, STRIKE_RANGE};

use std::time::Duration;

use crate::agent::Population;
use crate::cognition::Minds;
use crate::diagnostics::RuntimeCounters;
use crate::placements::SpawnedObjects;
use crate::resources::ResourceDeltas;
use crate::routing::RoutePlanner;
use crate::scheduler::Scheduler;
use crate::structures::StructureStore;
use crate::{
    AgentId, DeathRecord, HealthDiagnostic, HintOutcomeEvent, InterpretationEvent,
    MoveRequestError, MovementEventOutcome, MovementScheduled, NeedThresholdEventOutcome,
    PolicyDiagnostic, PolicyOptions, RouteEventOutcome, SignalEvent, SimTime, SleepDiagnostic,
    StructureDiagnostic, World, WorldConfig, WorldPosition, WorldRect,
};

/// Immutable settings used to construct or reset a simulation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EngineConfig {
    pub seed: u64,
    pub ticks_per_second: u32,
    pub world: WorldConfig,
}

pub const MAX_SIMULATION_SPEED: f32 = 256.0;

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
    policy_options: PolicyOptions,
    minds: Minds,
    signal_events: Vec<SignalEvent>,
    interpretation_events: Vec<InterpretationEvent>,
    hint_outcomes: Vec<HintOutcomeEvent>,
    meal_events: Vec<crate::MealEvent>,
    wildlife: crate::wildlife::Wildlife,
    wildlife_events: Vec<crate::WildlifeEvent>,
    lesson_events: Vec<crate::LessonEvent>,
    repair_events: Vec<crate::RepairEvent>,
    request_events: Vec<crate::RequestEvent>,
    next_signal_id: u64,
    resource_deltas: ResourceDeltas,
    spawned_objects: SpawnedObjects,
    structures: StructureStore,
    route_planner: RoutePlanner,
    runtime_counters: RuntimeCounters,
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
            policy_options: PolicyOptions::default(),
            minds: Minds::new(config.seed),
            signal_events: Vec::new(),
            interpretation_events: Vec::new(),
            hint_outcomes: Vec::new(),
            meal_events: Vec::new(),
            wildlife: crate::wildlife::Wildlife::default(),
            wildlife_events: Vec::new(),
            lesson_events: Vec::new(),
            repair_events: Vec::new(),
            request_events: Vec::new(),
            next_signal_id: 0,
            resource_deltas: ResourceDeltas::default(),
            spawned_objects: SpawnedObjects::default(),
            structures: StructureStore::default(),
            route_planner: RoutePlanner::default(),
            runtime_counters: RuntimeCounters::default(),
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
                self.speed = speed.clamp(0.0, MAX_SIMULATION_SPEED);
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
                self.policy_options = PolicyOptions::default();
                self.minds = Minds::new(self.config.seed);
                self.signal_events.clear();
                self.interpretation_events.clear();
                self.hint_outcomes.clear();
                self.meal_events.clear();
                self.wildlife = crate::wildlife::Wildlife::default();
                self.wildlife_events.clear();
                self.lesson_events.clear();
                self.repair_events.clear();
                self.request_events.clear();
                self.next_signal_id = 0;
                self.resource_deltas = ResourceDeltas::default();
                self.spawned_objects = SpawnedObjects::default();
                self.structures = StructureStore::default();
                self.route_planner = RoutePlanner::default();
                self.runtime_counters = RuntimeCounters::default();
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

    /// Drops lazily cancelled events once they outnumber what live agents can
    /// hold. Each agent has at most about 7 live events (4 needs, health, and one
    /// decision, action, movement, or wake), so the queue stays near `8 × population`.
    pub(super) fn compact_scheduler_if_needed(&mut self) {
        let retention_limit = self
            .population
            .len()
            .saturating_mul(8)
            .saturating_add(256)
            .max(64);
        if self.scheduler.len() >= retention_limit {
            let population = &self.population;
            self.scheduler
                .retain(|event| population.event_is_current(event));
        }
    }
}

impl Default for Engine {
    fn default() -> Self {
        Self::new(EngineConfig::default())
    }
}

#[cfg(test)]
mod tests;
