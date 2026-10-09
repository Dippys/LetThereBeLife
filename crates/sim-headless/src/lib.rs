//! Headless scenario runner: canonical survival scenarios, reports, and semantic hashes.

mod hash;
mod invariants;
mod report;
mod scenario;
mod spawns;

pub use report::{
    ActionCounts, DeathCounts, FailureCounts, FinalAgentCounts, REPORT_FORMAT_VERSION,
    ScenarioReport, SoakEvidence,
};
pub use scenario::{
    CANONICAL_SEED, CANONICAL_TICKS, CANONICAL_WORLD_SIDE, SOAK_SAMPLE_INTERVAL, ScenarioConfig,
    ScenarioError, ScenarioRunner,
};
