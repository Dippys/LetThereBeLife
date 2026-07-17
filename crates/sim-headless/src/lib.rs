use std::{collections::BTreeSet, error::Error, fmt};

use sim_core::{
    AgentActivity, DeathCause, Engine, EngineCapacityMetrics, EngineCommand, EngineCommandOutcome,
    EngineConfig, EngineDiagnostics, MovementOutcomeKind, PHYSICAL_POLICY_RADIUS, PhysicalGoal,
    PolicyDiagnosticKind, PolicyFailureReason, PopulationInit, ResourceKind, RouteOutcomeKind,
    SleepDiagnosticKind, Standability, StructureDiagnosticKind, StructureState, TickOutcome,
    WaterSource, WorldConfig, WorldPosition, WorldRect,
};

pub const REPORT_FORMAT_VERSION: u16 = 1;
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
            initial_food_per_agent: sim_core::INVENTORY_CAPACITY_PER_KIND,
            initial_wood_per_water_agent: sim_core::SHELTER_WOOD_COST,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScenarioError(String);

impl fmt::Display for ScenarioError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for ScenarioError {}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ActionCounts {
    pub movements: u64,
    pub route_arrivals: u64,
    pub route_failures: u64,
    pub policy_selections: u64,
    pub gathers: u64,
    pub eats: u64,
    pub drinks: u64,
    pub sleep_starts: u64,
    pub planned_wakes: u64,
    pub interrupted_wakes: u64,
    pub shelter_starts: u64,
    pub shelter_completions: u64,
    pub shelter_cancellations: u64,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct FailureCounts {
    pub total: u64,
    pub blocked_progress: u64,
    pub depletion: u64,
    pub no_perceived_target: u64,
    pub invariant: u64,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct DeathCounts {
    pub dehydration: u32,
    pub exposure: u32,
    pub starvation: u32,
    pub exhaustion: u32,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct FinalAgentCounts {
    pub idle: u32,
    pub moving: u32,
    pub gathering: u32,
    pub building: u32,
    pub sleeping: u32,
    pub incapacitated: u32,
    pub dead: u32,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SoakEvidence {
    pub samples: u64,
    pub invariant_violations: u64,
    pub peak_scheduler_capacity: usize,
    pub peak_occupancy_capacity: usize,
    pub peak_resource_deltas: usize,
    pub peak_structure_slots: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScenarioReport {
    pub format_version: u16,
    pub seed: u64,
    pub world_width: u32,
    pub world_height: u32,
    pub population: u32,
    pub requested_driver_ticks: u64,
    pub final_tick: u64,
    pub access_radius: u8,
    pub initial_food_per_agent: u8,
    pub initial_wood_per_water_agent: u8,
    pub spawn_bounds: WorldRect,
    pub spawn_hash: u64,
    pub resource_access_spawns: u32,
    pub fallback_spawns: u32,
    pub living_agents: u32,
    pub active_agents: u32,
    pub final_agents: FinalAgentCounts,
    pub actions: ActionCounts,
    pub failures: FailureCounts,
    pub deaths: DeathCounts,
    pub modified_resources: u32,
    pub resource_units_removed: u64,
    pub structures: u32,
    pub complete_structures: u32,
    pub inventory_food: u64,
    pub inventory_wood: u64,
    pub inventory_stone: u64,
    pub work: sim_core::EngineWorkMetrics,
    pub capacity: EngineCapacityMetrics,
    pub soak: SoakEvidence,
    pub semantic_hash: u64,
}

impl fmt::Display for ScenarioReport {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "physical-agent-report-v{} hash={:016x} seed={} world={}x{} population={} water/fallback={}/{} supplies={}/{} driver_ticks={} final_tick={} living={} active={} deaths={}/{}/{}/{} moves={} routes={}/{} gather={} eat={} drink={} sleep={}/{}/{} shelters={}/{} failures={} blocked={} depletion={} resources={}:{} events={}/{} stale={}/{} peak_queue={} retries={}:{} soak_samples={} invariant_violations={}",
            self.format_version,
            self.semantic_hash,
            self.seed,
            self.world_width,
            self.world_height,
            self.population,
            self.resource_access_spawns,
            self.fallback_spawns,
            self.initial_food_per_agent,
            self.initial_wood_per_water_agent,
            self.requested_driver_ticks,
            self.final_tick,
            self.living_agents,
            self.active_agents,
            self.deaths.dehydration,
            self.deaths.exposure,
            self.deaths.starvation,
            self.deaths.exhaustion,
            self.actions.movements,
            self.actions.route_arrivals,
            self.actions.route_failures,
            self.actions.gathers,
            self.actions.eats,
            self.actions.drinks,
            self.actions.sleep_starts,
            self.actions.planned_wakes,
            self.actions.interrupted_wakes,
            self.actions.shelter_completions,
            self.structures,
            self.failures.total,
            self.failures.blocked_progress,
            self.failures.depletion,
            self.modified_resources,
            self.resource_units_removed,
            self.work.events_processed,
            self.work.events_scheduled,
            self.work.stale_events_processed,
            self.work.stale_events_compacted,
            self.work.peak_scheduled_events,
            self.work.policy_retries,
            self.work.peak_policy_retry_depth,
            self.soak.samples,
            self.soak.invariant_violations,
        )
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct RunCounters {
    actions: ActionCounts,
    failures: FailureCounts,
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
                sim_core::InventoryView {
                    food: initial_food_per_agent,
                    wood: if raw < water_spawn_count {
                        initial_wood_per_water_agent
                    } else {
                        0
                    },
                    ..sim_core::InventoryView::default()
                },
            )
            .map_err(|error| ScenarioError(format!("initial supplies failed: {error}")))?;
    }
    engine
        .activate_physical_policy()
        .map_err(|error| ScenarioError(format!("policy activation failed: {error}")))?;
    Ok(())
}

struct SpawnSelection {
    positions: Vec<WorldPosition>,
    resource_access_count: u32,
    fallback_count: u32,
}

fn select_spawn_locations(
    engine: &Engine,
    population: u32,
    radius: u8,
) -> Result<SpawnSelection, ScenarioError> {
    let bounds = engine.world().initial_bounds();
    let radius = u64::from(radius);
    let mut wood = Vec::new();
    for feature in engine.world().all_features() {
        match feature.base_resource().kind {
            ResourceKind::Food => {}
            ResourceKind::Wood => wood.push(feature.position),
            ResourceKind::Stone => {}
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

fn invariant_violations(engine: &Engine, diagnostics: EngineDiagnostics) -> u64 {
    let mut violations = 0;
    let agents: Vec<_> = engine.agent_views(usize::MAX).collect();
    let occupied: BTreeSet<_> = agents
        .iter()
        .filter(|agent| agent.activity != AgentActivity::Dead)
        .map(|agent| agent.position)
        .collect();
    let living = agents
        .iter()
        .filter(|agent| agent.activity != AgentActivity::Dead)
        .count();
    violations += u64::from(diagnostics.capacity.occupancy_entries != living);

    let structures: Vec<_> = engine.structure_views(usize::MAX).collect();
    let sites: BTreeSet<_> = structures
        .iter()
        .map(|structure| structure.position)
        .collect();
    violations += u64::from(sites.len() != structures.len());
    violations += u64::from(sites.iter().any(|site| occupied.contains(site)));

    for delta in engine.resource_delta_views() {
        let valid = engine
            .world()
            .base_resource_at(delta.position)
            .is_some_and(|base| base.kind == delta.kind && delta.remaining <= base.capacity);
        violations += u64::from(!valid);
    }
    violations
}

fn build_report(
    engine: &Engine,
    config: ScenarioConfig,
    spawns: &[WorldPosition],
    counters: RunCounters,
    soak: SoakEvidence,
    resource_access_spawns: u32,
    fallback_spawns: u32,
) -> ScenarioReport {
    let snapshot = engine.snapshot();
    let diagnostics = engine.diagnostics();
    let mut final_agents = FinalAgentCounts::default();
    let mut inventory_food = 0;
    let mut inventory_wood = 0;
    let mut inventory_stone = 0;
    for agent in engine.agent_views(usize::MAX) {
        match agent.activity {
            AgentActivity::Idle => final_agents.idle += 1,
            AgentActivity::Moving => final_agents.moving += 1,
            AgentActivity::Gathering => final_agents.gathering += 1,
            AgentActivity::Building => final_agents.building += 1,
            AgentActivity::Sleeping => final_agents.sleeping += 1,
            AgentActivity::Incapacitated => final_agents.incapacitated += 1,
            AgentActivity::Dead => final_agents.dead += 1,
        }
        if let Some(inventory) = engine.inventory(agent.id) {
            inventory_food += u64::from(inventory.food);
            inventory_wood += u64::from(inventory.wood);
            inventory_stone += u64::from(inventory.stone);
        }
    }
    let mut deaths = DeathCounts::default();
    for death in engine.death_records() {
        match death.cause {
            DeathCause::Dehydration => deaths.dehydration += 1,
            DeathCause::Exposure => deaths.exposure += 1,
            DeathCause::Starvation => deaths.starvation += 1,
            DeathCause::Exhaustion => deaths.exhaustion += 1,
        }
    }
    let resource_units_removed = engine
        .resource_delta_views()
        .map(|delta| {
            u64::from(
                engine
                    .world()
                    .base_resource_at(delta.position)
                    .expect("delta identity remains backed by generated resource")
                    .capacity
                    - delta.remaining,
            )
        })
        .sum();
    let complete_structures = engine
        .structure_views(usize::MAX)
        .filter(|structure| structure.state == StructureState::Complete)
        .count() as u32;
    let spawn_hash = hash_positions(spawns);
    let mut report = ScenarioReport {
        format_version: REPORT_FORMAT_VERSION,
        seed: snapshot.seed,
        world_width: engine.world().width(),
        world_height: engine.world().height(),
        population: config.population,
        requested_driver_ticks: config.driver_ticks,
        final_tick: snapshot.tick,
        access_radius: config.access_radius,
        initial_food_per_agent: config.initial_food_per_agent,
        initial_wood_per_water_agent: config.initial_wood_per_water_agent,
        spawn_bounds: engine.world().initial_bounds(),
        spawn_hash,
        resource_access_spawns,
        fallback_spawns,
        living_agents: snapshot.living_agent_count,
        active_agents: snapshot.active_agent_count,
        final_agents,
        actions: counters.actions,
        failures: counters.failures,
        deaths,
        modified_resources: diagnostics.capacity.resource_deltas as u32,
        resource_units_removed,
        structures: snapshot.structure_count,
        complete_structures,
        inventory_food,
        inventory_wood,
        inventory_stone,
        work: diagnostics.work,
        capacity: diagnostics.capacity,
        soak,
        semantic_hash: 0,
    };
    report.semantic_hash = semantic_hash(engine, &report, spawns);
    report
}

fn hash_positions(positions: &[WorldPosition]) -> u64 {
    let mut hash = SemanticHasher::new();
    hash.u64(positions.len() as u64);
    for position in positions {
        hash.position(*position);
    }
    hash.finish()
}

fn semantic_hash(engine: &Engine, report: &ScenarioReport, spawns: &[WorldPosition]) -> u64 {
    let mut hash = SemanticHasher::new();
    hash.u16(report.format_version);
    hash.u64(report.seed);
    hash.u32(report.world_width);
    hash.u32(report.world_height);
    hash.u32(report.population);
    hash.u64(report.requested_driver_ticks);
    hash.u64(report.final_tick);
    hash.u8(report.access_radius);
    hash.u8(report.initial_food_per_agent);
    hash.u8(report.initial_wood_per_water_agent);
    hash.u64(report.spawn_hash);
    hash.u32(report.resource_access_spawns);
    hash.u32(report.fallback_spawns);
    for position in spawns {
        hash.position(*position);
    }
    hash.u64(report.actions.movements);
    hash.u64(report.actions.route_arrivals);
    hash.u64(report.actions.route_failures);
    hash.u64(report.actions.policy_selections);
    hash.u64(report.actions.gathers);
    hash.u64(report.actions.eats);
    hash.u64(report.actions.drinks);
    hash.u64(report.actions.sleep_starts);
    hash.u64(report.actions.planned_wakes);
    hash.u64(report.actions.interrupted_wakes);
    hash.u64(report.actions.shelter_starts);
    hash.u64(report.actions.shelter_completions);
    hash.u64(report.actions.shelter_cancellations);
    hash.u64(report.failures.total);
    hash.u64(report.failures.blocked_progress);
    hash.u64(report.failures.depletion);
    hash.u64(report.failures.no_perceived_target);
    hash.u64(report.failures.invariant);
    hash.u64(report.work.events_scheduled);
    hash.u64(report.work.events_processed);
    hash.u64(report.work.stale_events_processed);
    hash.u64(report.work.stale_events_compacted);
    hash.u64(report.work.due_backlog_ticks);
    hash.u32(report.work.peak_scheduled_events);
    hash.u16(report.work.peak_events_processed_per_tick);
    hash.u64(report.work.policy_perception_queries);
    hash.u64(report.work.policy_perceived_cells);
    hash.u64(report.work.route_plans);
    hash.u64(report.work.route_expansions);
    hash.u64(report.work.policy_retries);
    hash.u8(report.work.peak_policy_retry_depth);
    for agent in engine.agent_views(usize::MAX) {
        hash.u32(agent.id.get());
        hash.position(agent.position);
        hash.u8(agent.activity as u8);
        if let Ok(needs) = engine.physical_needs(agent.id) {
            for level in [needs.hunger, needs.thirst, needs.rest, needs.exposure] {
                hash.u16(level.value);
                hash.i16(level.rate_per_period);
                hash.u16(level.threshold);
                hash.u8(level.threshold_reached as u8);
            }
            hash.u8(needs.next_threshold.is_some() as u8);
            if let Some(next) = needs.next_threshold {
                hash.u8(next.kind as u8);
                hash.u64(next.due.ticks());
            }
        }
        if let Some(health) = engine.health(agent.id) {
            hash.u16(health.value);
            hash.u8(health.status as u8);
            hash.u8(health.next_consequence.is_some() as u8);
            if let Some(next) = health.next_consequence {
                hash.u64(next.ticks());
            }
        }
        if let Some(inventory) = engine.inventory(agent.id) {
            hash.u8(inventory.food);
            hash.u8(inventory.wood);
            hash.u8(inventory.stone);
        }
        if let Some(policy) = engine.physical_policy(agent.id) {
            hash.u8(policy.goal as u8);
            hash.u8(policy.committed as u8);
            hash.u8(policy.retry_count);
            hash.u8(policy.exploration_heading as u8);
            hash.optional_position(policy.target);
        }
    }
    for delta in engine.resource_delta_views() {
        hash.position(delta.position);
        hash.u8(delta.kind as u8);
        hash.u16(delta.remaining);
    }
    for structure in engine.structure_views(usize::MAX) {
        hash.u32(structure.id.get());
        hash.position(structure.position);
        hash.u8(structure.kind as u8);
        hash.u8(structure.state as u8);
        hash.u8(structure.builder.is_some() as u8);
        if let Some(builder) = structure.builder {
            hash.u32(builder.get());
        }
        hash.u64(structure.started_at.ticks());
        hash.u64(structure.completes_at.ticks());
    }
    for death in engine.death_records() {
        hash.u32(death.agent.get());
        hash.u8(death.cause as u8);
        hash.u64(death.at.ticks());
        hash.position(death.position);
    }
    hash.finish()
}

struct SemanticHasher(u64);

impl SemanticHasher {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;

    const fn new() -> Self {
        Self(Self::OFFSET)
    }

    fn bytes(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 ^= u64::from(*byte);
            self.0 = self.0.wrapping_mul(Self::PRIME);
        }
    }

    fn u8(&mut self, value: u8) {
        self.bytes(&[value]);
    }

    fn u16(&mut self, value: u16) {
        self.bytes(&value.to_le_bytes());
    }

    fn i16(&mut self, value: i16) {
        self.bytes(&value.to_le_bytes());
    }

    fn u32(&mut self, value: u32) {
        self.bytes(&value.to_le_bytes());
    }

    fn u64(&mut self, value: u64) {
        self.bytes(&value.to_le_bytes());
    }

    fn i64(&mut self, value: i64) {
        self.bytes(&value.to_le_bytes());
    }

    fn position(&mut self, position: WorldPosition) {
        self.i64(position.x);
        self.i64(position.y);
    }

    fn optional_position(&mut self, position: Option<WorldPosition>) {
        self.u8(position.is_some() as u8);
        if let Some(position) = position {
            self.position(position);
        }
    }

    const fn finish(self) -> u64 {
        self.0
    }
}
