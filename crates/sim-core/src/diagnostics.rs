/// Cumulative deterministic work counters for one engine run.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct EngineWorkMetrics {
    pub events_scheduled: u64,
    pub events_processed: u64,
    pub stale_events_processed: u64,
    pub stale_events_compacted: u64,
    pub due_backlog_ticks: u64,
    pub peak_scheduled_events: u32,
    pub peak_events_processed_per_tick: u16,
    pub policy_perception_queries: u64,
    pub policy_perceived_cells: u64,
    pub route_plans: u64,
    pub route_expansions: u64,
    pub policy_retries: u64,
    pub peak_policy_retry_depth: u8,
}

/// Retained collection capacities and sparse-entry counts at a snapshot boundary.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct EngineCapacityMetrics {
    pub agent_records: usize,
    pub movement_generations: usize,
    pub routes: usize,
    pub needs: usize,
    pub policies: usize,
    pub inventories: usize,
    pub sleeps: usize,
    pub health: usize,
    pub occupancy_entries: usize,
    pub occupancy_entry_capacity: usize,
    pub occupancy_buckets: usize,
    pub scheduled_events: usize,
    pub scheduler_capacity: usize,
    pub resource_deltas: usize,
    pub structure_slots: usize,
}

/// Read-only integrated diagnostics used by headless reports and soak checks.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct EngineDiagnostics {
    pub work: EngineWorkMetrics,
    pub capacity: EngineCapacityMetrics,
}

#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct RuntimeCounters {
    pub(crate) events_processed: u64,
    pub(crate) stale_events_processed: u64,
    pub(crate) due_backlog_ticks: u64,
    pub(crate) peak_events_processed_per_tick: u16,
    pub(crate) policy_perception_queries: u64,
    pub(crate) policy_perceived_cells: u64,
    pub(crate) route_plans: u64,
    pub(crate) route_expansions: u64,
    pub(crate) policy_retries: u64,
    pub(crate) peak_policy_retry_depth: u8,
}
