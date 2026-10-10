//! Read-only engine accessors: perception, per-agent views, latest-tick
//! outcomes and diagnostics, composed world queries, and snapshots.

use crate::{
    AgentId, AgentView, BaseResource, DeathRecord, Engine, EngineCapacityMetrics,
    EngineDiagnostics, EngineWorkMetrics, HealthDiagnostic, HealthView, InventoryView,
    MovementEventOutcome, NeedQueryError, NeedThresholdEventOutcome, PerceptionError,
    PhysicalNeedsView, PhysicalPerception, PhysicalPolicyView, PolicyDiagnostic, ResourceDeltaView,
    RouteEventOutcome, SimulationSnapshot, SleepDiagnostic, SleepView, SpawnedObjectView,
    Standability, StructureDiagnostic, StructureView, WORLD_GENERATION_BOUNDS, WaterSource,
    WorldPosition, WorldQueryError, WorldRect,
};

/// How far (cells) agents spot animals.
pub const ANIMAL_SIGHT: i64 = 16;

impl Engine {
    /// Returns objective nearby physical facts in canonical world row order.
    pub fn perceive_physical(
        &self,
        agent: AgentId,
        radius: u8,
    ) -> Result<PhysicalPerception, PerceptionError> {
        self.population
            .perceive(
                &self.world,
                &self.spawned_objects,
                &self.resource_deltas,
                &self.structures,
                agent,
                radius,
            )
            .map(|perception| self.with_wildlife(perception))
    }

    /// Adds the carcasses inside the perceived area, and the animals within
    /// `ANIMAL_SIGHT` of its center (big, moving animals are spotted from farther
    /// away than a bush).
    fn with_wildlife(&self, mut perception: PhysicalPerception) -> PhysicalPerception {
        let area = perception.area;
        let center = WorldPosition {
            x: (area.min.x + area.max.x - 1) / 2,
            y: (area.min.y + area.max.y - 1) / 2,
        };
        let sight = ANIMAL_SIGHT.max((area.max.x - area.min.x) / 2);
        perception.animals = self
            .wildlife
            .views()
            .filter(|animal| {
                animal
                    .position
                    .x
                    .abs_diff(center.x)
                    .max(animal.position.y.abs_diff(center.y))
                    <= sight as u64
            })
            .collect();
        let carcasses = self
            .wildlife
            .carcasses
            .iter()
            .filter(|carcass| carcass.meat > 0 && area.contains(carcass.position()));
        let mut added = false;
        for carcass in carcasses {
            perception.resources.push(crate::PerceivedResource {
                position: carcass.position(),
                resource: BaseResource {
                    capacity: u16::from(carcass.meat),
                    kind: crate::Material::Meat,
                },
            });
            added = true;
        }
        if added {
            perception
                .resources
                .sort_by_key(|resource| (resource.position.y, resource.position.x));
        }
        perception
    }

    /// Returns objective facts for a bounded half-open rectangle inside the active area.
    pub fn perceive_physical_area(
        &self,
        agent: AgentId,
        area: WorldRect,
    ) -> Result<PhysicalPerception, PerceptionError> {
        self.population
            .perceive_area(
                &self.world,
                &self.spawned_objects,
                &self.resource_deltas,
                &self.structures,
                agent,
                area,
            )
            .map(|perception| self.with_wildlife(perception))
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
        if let Some(meat) = self.wildlife.carcass_meat(position) {
            return Ok(Some(BaseResource {
                capacity: u16::from(meat),
                kind: crate::Material::Meat,
            }));
        }
        if let Some(resource) = self.spawned_objects.resource_at(position) {
            // Preserve the same typed residency contract as generated resources.
            self.world.cell(position).ok_or_else(|| {
                if WORLD_GENERATION_BOUNDS.contains(position) {
                    WorldQueryError::Unloaded
                } else {
                    WorldQueryError::OutsideWorldBounds
                }
            })?;
            return Ok(Some(resource));
        }
        self.resource_deltas.resource_at(&self.world, position)
    }

    pub fn spawned_object_at(&self, position: WorldPosition) -> Option<SpawnedObjectView> {
        self.spawned_objects.at(position)
    }

    pub fn spawned_object_views(&self) -> impl Iterator<Item = SpawnedObjectView> + '_ {
        self.spawned_objects.views()
    }

    pub fn spawned_object_count(&self) -> usize {
        self.spawned_objects.len()
    }

    pub fn spawned_object_revision(&self) -> u64 {
        self.spawned_objects.revision()
    }

    /// Composes generated terrain with sparse user-spawned blockers.
    pub fn physical_standability_at(
        &self,
        position: WorldPosition,
    ) -> Result<Standability, WorldQueryError> {
        self.spawned_objects.standability_at(&self.world, position)
    }

    /// Composes generated hydrology with explicitly placed fresh-water cells.
    pub fn available_water_at(
        &self,
        position: WorldPosition,
    ) -> Result<Option<WaterSource>, WorldQueryError> {
        self.spawned_objects.water_at(&self.world, position)
    }

    /// Number of generated features with simulation-owned remaining-capacity state.
    pub fn modified_resource_count(&self) -> usize {
        self.resource_deltas.len()
    }

    /// Sparse modified generated resources in deterministic position order.
    pub fn resource_delta_views(&self) -> impl Iterator<Item = ResourceDeltaView> + '_ {
        self.resource_deltas.views(&self.world)
    }

    /// Returns the active sleep interval for one agent, if any.
    pub fn sleep(&self, agent: AgentId) -> Option<SleepView> {
        self.population.sleep_view(agent)
    }

    /// Sleep starts, planned wakes, and threshold interruptions from the latest advancing tick.
    pub fn sleep_diagnostics(&self) -> &[SleepDiagnostic] {
        &self.sleep_diagnostics
    }

    /// Returns at most `limit` live structures in stable identity order.
    pub fn structure_views(&self, limit: usize) -> impl Iterator<Item = StructureView> + '_ {
        self.structures.views(limit)
    }

    /// Construction starts, completions, and cancellations from the latest advancing tick.
    pub fn structure_diagnostics(&self) -> &[StructureDiagnostic] {
        &self.structure_diagnostics
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

    /// Cumulative deterministic work and retained-capacity diagnostics for this run.
    pub fn diagnostics(&self) -> EngineDiagnostics {
        let population = self.population.capacities();
        EngineDiagnostics {
            work: EngineWorkMetrics {
                events_scheduled: self.scheduler.total_scheduled(),
                events_processed: self.runtime_counters.events_processed,
                stale_events_processed: self.runtime_counters.stale_events_processed,
                stale_events_compacted: self.scheduler.total_compacted(),
                due_backlog_ticks: self.runtime_counters.due_backlog_ticks,
                peak_scheduled_events: self.scheduler.peak_len() as u32,
                peak_events_processed_per_tick: self
                    .runtime_counters
                    .peak_events_processed_per_tick,
                policy_perception_queries: self.runtime_counters.policy_perception_queries,
                policy_perceived_cells: self.runtime_counters.policy_perceived_cells,
                route_plans: self.runtime_counters.route_plans,
                route_expansions: self.runtime_counters.route_expansions,
                policy_retries: self.runtime_counters.policy_retries,
                peak_policy_retry_depth: self.runtime_counters.peak_policy_retry_depth,
            },
            capacity: EngineCapacityMetrics {
                agent_records: population.records,
                movement_generations: population.movement_generations,
                routes: population.routes,
                needs: population.needs,
                policies: population.policies,
                inventories: population.inventories,
                sleeps: population.sleeps,
                health: population.health,
                occupancy_entries: population.occupancy_entries,
                occupancy_entry_capacity: population.occupancy_entry_capacity,
                occupancy_buckets: population.occupancy_buckets,
                scheduled_events: self.scheduler.len(),
                scheduler_capacity: self.scheduler.capacity(),
                resource_deltas: self.resource_deltas.len(),
                structure_slots: self.structures.retained_slots(),
            },
        }
    }
}
