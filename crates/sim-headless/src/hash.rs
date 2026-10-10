//! FNV-1a semantic hashing of scenario reports and final engine state for determinism checks.

use sim_core::{Engine, WorldPosition};

use crate::report::ScenarioReport;

pub(crate) fn hash_positions(positions: &[WorldPosition]) -> u64 {
    let mut hash = SemanticHasher::new();
    hash.u64(positions.len() as u64);
    for position in positions {
        hash.position(*position);
    }
    hash.finish()
}

pub(crate) fn semantic_hash(
    engine: &Engine,
    report: &ScenarioReport,
    spawns: &[WorldPosition],
) -> u64 {
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
            for amount in inventory.items {
                hash.u8(amount);
            }
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
