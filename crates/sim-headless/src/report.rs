//! Scenario report types, their one-line `Display` form, and report assembly from engine state.

use std::fmt;

use sim_core::{
    AgentActivity, DeathCause, Engine, EngineCapacityMetrics, StructureState, WorldPosition,
    WorldRect,
};

use crate::{
    hash::{hash_positions, semantic_hash},
    scenario::{RunCounters, ScenarioConfig},
};

pub const REPORT_FORMAT_VERSION: u16 = 1;

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

pub(crate) fn build_report(
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
            inventory_food += u64::from(inventory.amount(sim_core::Material::Berries));
            inventory_wood += u64::from(inventory.amount(sim_core::Material::Wood));
            inventory_stone += u64::from(inventory.amount(sim_core::Material::Stone));
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
