//! Scenario configuration, errors, and the fixed-tick runner that drives an engine and collects counters.

use std::{error::Error, fmt};

use sim_core::{
    Engine, EngineCommand, EngineCommandOutcome, EngineConfig, MovementOutcomeKind,
    PHYSICAL_POLICY_RADIUS, PhysicalGoal, PolicyDiagnosticKind, PolicyFailureReason,
    PopulationInit, RouteOutcomeKind, SleepDiagnosticKind, StructureDiagnosticKind, TickOutcome,
    WorldConfig, WorldPosition,
};

use crate::{
    invariants::invariant_violations,
    report::{ActionCounts, FailureCounts, ScenarioReport, SoakEvidence, build_report},
    spawns::select_spawn_locations,
};

pub const CANONICAL_SEED: u64 = 1;
pub const CANONICAL_WORLD_SIDE: u32 = 2_048;
pub const CANONICAL_TICKS: u64 = 600_000;
pub const SOAK_SAMPLE_INTERVAL: u64 = 600;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScenarioConfig {
    pub engine: EngineConfig,
    pub population: u32,
    pub driver_ticks: u64,
    pub access_radius: u8,
    pub initial_food_per_agent: u8,
    pub initial_wood_per_water_agent: u8,
}

impl ScenarioConfig {
    pub fn canonical(population: u32) -> Self {
        Self {
            engine: EngineConfig {
                seed: CANONICAL_SEED,
                ticks_per_second: 60,
                world: WorldConfig::new(CANONICAL_WORLD_SIDE, CANONICAL_WORLD_SIDE)
                    .expect("canonical world dimensions are valid"),
            },
            population,
            driver_ticks: CANONICAL_TICKS,
            access_radius: PHYSICAL_POLICY_RADIUS,
            initial_food_per_agent: sim_core::CARRY_CAPACITY - sim_core::SHELTER_WOOD_COST,
            initial_wood_per_water_agent: sim_core::SHELTER_WOOD_COST,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScenarioError(pub(crate) String);

impl fmt::Display for ScenarioError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for ScenarioError {}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RunCounters {
    pub(crate) actions: ActionCounts,
    pub(crate) failures: FailureCounts,
}

#[derive(Debug)]
pub struct ScenarioRunner {
    config: ScenarioConfig,
    engine: Engine,
    spawns: Vec<WorldPosition>,
    resource_access_spawns: u32,
    fallback_spawns: u32,
    counters: RunCounters,
    soak: SoakEvidence,
    driver_steps: u64,
}

impl ScenarioRunner {
    pub fn new(config: ScenarioConfig) -> Result<Self, ScenarioError> {
        if config.population == 0 {
            return Err(ScenarioError("scenario population must be positive".into()));
        }
        if config.access_radius > PHYSICAL_POLICY_RADIUS {
            return Err(ScenarioError(format!(
                "scenario access radius {} exceeds policy radius {}",
                config.access_radius, PHYSICAL_POLICY_RADIUS
            )));
        }
        let mut engine = Engine::new(config.engine);
        engine
            .materialize_initial_area()
            .map_err(|error| ScenarioError(format!("world materialization failed: {error}")))?;
        let selection = select_spawn_locations(&engine, config.population, config.access_radius)?;
        initialize(
            &mut engine,
            config.population,
            &selection.positions,
            config.initial_food_per_agent,
            config.initial_wood_per_water_agent,
            selection.resource_access_count,
        )?;
        let mut runner = Self {
            config,
            engine,
            spawns: selection.positions,
            resource_access_spawns: selection.resource_access_count,
            fallback_spawns: selection.fallback_count,
            counters: RunCounters::default(),
            soak: SoakEvidence::default(),
            driver_steps: 0,
        };
        runner.sample_soak();
        Ok(runner)
    }

    pub fn run(mut self, batch_size: u64) -> Result<ScenarioReport, ScenarioError> {
        self.advance_driver_steps(self.config.driver_ticks, batch_size)?;
        Ok(self.report())
    }

    pub fn advance_driver_steps(
        &mut self,
        steps: u64,
        batch_size: u64,
    ) -> Result<(), ScenarioError> {
        if batch_size == 0 {
            return Err(ScenarioError("batch size must be positive".into()));
        }
        let mut remaining = steps;
        while remaining > 0 {
            let batch = remaining.min(batch_size);
            for _ in 0..batch {
                self.driver_steps = self.driver_steps.saturating_add(1);
                match self.engine.tick() {
                    TickOutcome::Advanced { .. } => self.collect_latest_tick(),
                    TickOutcome::Paused => {}
                    TickOutcome::TimeExhausted => {
                        return Err(ScenarioError("simulation time exhausted".into()));
                    }
                }
                if self.driver_steps % SOAK_SAMPLE_INTERVAL == 0 {
                    self.sample_soak();
                }
            }
            remaining -= batch;
        }
        Ok(())
    }

    pub fn command(&mut self, command: EngineCommand) -> EngineCommandOutcome {
        self.engine.command(command)
    }

    pub fn reset_for_replay(&mut self) -> Result<(), ScenarioError> {
        self.engine.command(EngineCommand::Reset);
        initialize(
            &mut self.engine,
            self.config.population,
            &self.spawns,
            self.config.initial_food_per_agent,
            self.config.initial_wood_per_water_agent,
            self.resource_access_spawns,
        )?;
        self.counters = RunCounters::default();
        self.soak = SoakEvidence::default();
        self.driver_steps = 0;
        self.sample_soak();
        Ok(())
    }

    pub fn report(&self) -> ScenarioReport {
        build_report(
            &self.engine,
            self.config,
            &self.spawns,
            self.counters,
            self.soak,
            self.resource_access_spawns,
            self.fallback_spawns,
        )
    }

    pub fn engine(&self) -> &Engine {
        &self.engine
    }

    pub fn spawns(&self) -> &[WorldPosition] {
        &self.spawns
    }

    fn collect_latest_tick(&mut self) {
        self.counters.actions.movements += self
            .engine
            .movement_outcomes()
            .iter()
            .filter(|outcome| outcome.kind == MovementOutcomeKind::Moved)
            .count() as u64;
        for route in self.engine.route_outcomes() {
            if route.kind == RouteOutcomeKind::Arrived {
                self.counters.actions.route_arrivals += 1;
            } else {
                self.counters.actions.route_failures += 1;
            }
        }
        for diagnostic in self.engine.policy_diagnostics() {
            if diagnostic.kind == PolicyDiagnosticKind::Selected {
                self.counters.actions.policy_selections += 1;
            }
            if diagnostic.kind == PolicyDiagnosticKind::ActionCompleted
                && diagnostic.failure.is_none()
            {
                match diagnostic.goal {
                    PhysicalGoal::GatherMaterial => self.counters.actions.gathers += 1,
                    PhysicalGoal::Eat => self.counters.actions.eats += 1,
                    PhysicalGoal::Drink => self.counters.actions.drinks += 1,
                    _ => {}
                }
            }
            if let Some(failure) = diagnostic.failure {
                self.counters.failures.total += 1;
                match failure {
                    PolicyFailureReason::Occupied
                    | PolicyFailureReason::NoPath
                    | PolicyFailureReason::RouteBudgetExhausted
                    | PolicyFailureReason::TargetUnavailable => {
                        self.counters.failures.blocked_progress += 1;
                    }
                    PolicyFailureReason::ResourceDepleted => {
                        self.counters.failures.depletion += 1;
                    }
                    PolicyFailureReason::NoPerceivedTarget => {
                        self.counters.failures.no_perceived_target += 1;
                    }
                    PolicyFailureReason::InconsistentState => {
                        self.counters.failures.invariant += 1;
                    }
                    _ => {}
                }
            }
        }
        for diagnostic in self.engine.sleep_diagnostics() {
            match diagnostic.kind {
                SleepDiagnosticKind::Started => self.counters.actions.sleep_starts += 1,
                SleepDiagnosticKind::Woke => self.counters.actions.planned_wakes += 1,
                SleepDiagnosticKind::Interrupted => self.counters.actions.interrupted_wakes += 1,
            }
        }
        for diagnostic in self.engine.structure_diagnostics() {
            match diagnostic.kind {
                StructureDiagnosticKind::Started => self.counters.actions.shelter_starts += 1,
                StructureDiagnosticKind::Completed => {
                    self.counters.actions.shelter_completions += 1;
                }
                StructureDiagnosticKind::Cancelled => {
                    self.counters.actions.shelter_cancellations += 1;
                }
                StructureDiagnosticKind::Collapsed => {}
            }
        }
    }

    fn sample_soak(&mut self) {
        let diagnostics = self.engine.diagnostics();
        self.soak.samples += 1;
        self.soak.peak_scheduler_capacity = self
            .soak
            .peak_scheduler_capacity
            .max(diagnostics.capacity.scheduler_capacity);
        self.soak.peak_occupancy_capacity = self
            .soak
            .peak_occupancy_capacity
            .max(diagnostics.capacity.occupancy_entry_capacity);
        self.soak.peak_resource_deltas = self
            .soak
            .peak_resource_deltas
            .max(diagnostics.capacity.resource_deltas);
        self.soak.peak_structure_slots = self
            .soak
            .peak_structure_slots
            .max(diagnostics.capacity.structure_slots);
        self.soak.invariant_violations += invariant_violations(&self.engine, diagnostics);
    }
}

fn initialize(
    engine: &mut Engine,
    population: u32,
    spawns: &[WorldPosition],
    initial_food_per_agent: u8,
    initial_wood_per_water_agent: u8,
    water_spawn_count: u32,
) -> Result<(), ScenarioError> {
    engine
        .initialize_population(
            PopulationInit {
                active_area: engine.world().initial_bounds(),
                population,
            },
            spawns,
        )
        .map_err(|error| ScenarioError(format!("population initialization failed: {error}")))?;
    for raw in 0..population {
        engine
            .set_initial_inventory(
                sim_core::AgentId::new(raw),
                sim_core::InventoryView::of(&[
                    (sim_core::Material::Berries, initial_food_per_agent),
                    (
                        sim_core::Material::Wood,
                        if raw < water_spawn_count {
                            initial_wood_per_water_agent
                        } else {
                            0
                        },
                    ),
                ]),
            )
            .map_err(|error| ScenarioError(format!("initial supplies failed: {error}")))?;
    }
    engine
        .activate_physical_policy()
        .map_err(|error| ScenarioError(format!("policy activation failed: {error}")))?;
    Ok(())
}
