//! Soak-time invariant checks over occupancy, structure sites, and resource deltas.

use std::collections::BTreeSet;

use sim_core::{AgentActivity, Engine, EngineDiagnostics};

pub(crate) fn invariant_violations(engine: &Engine, diagnostics: EngineDiagnostics) -> u64 {
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
