//! Engine unit tests, grouped by behaviour. Shared fixtures live here.

use std::{mem::size_of, time::Instant};

use super::errors::{move_failure, perception_failure, request_failure, route_failure};
use crate::policy::{PolicyAction, select_with_exploration};
use crate::resources::ResourceDeltas;
use crate::routing::RouteEnvironment;
use crate::scheduler::{self, MAX_DUE_EVENTS_PER_TICK, Scheduler};
use crate::structures::StructureStore;
use crate::{
    AgentActivity, AgentId, AgentView, ChunkCoord, ChunkPresence, DeathCause, EAT_HUNGER_RELIEF,
    Engine, EngineCommand, EngineCommandOutcome, EngineConfig, ExplorationHeading,
    HEALTH_INCAPACITATION_THRESHOLD, HealthDiagnosticKind, INVENTORY_CAPACITY_PER_KIND,
    InventoryView, MAX_ROUTE_EXPANSIONS, MAX_SIMULATION_SPEED, MoveRequestError,
    MovementEventOutcome, MovementOutcomeKind, NeedKind, NeedThreshold, PerceptionError,
    PhysicalGoal, PolicyDiagnosticKind, PolicyFailureReason, PolicyReason, PopulationInit,
    PopulationInitError, ResourceKind, RouteOutcomeKind, RouteRequest, RouteRequestError,
    SHELTER_BUILD_TICKS, SHELTER_STONE_COST, SHELTER_WOOD_COST, SimTime, SleepQuality,
    SleepRequestError, SpawnInvalidReason, SpawnKind, Standability, StructureDiagnosticKind,
    StructureId, StructureState, TickOutcome, TraversalKind, TraversalStep, WORLD_HALF_EXTENT,
    WaterSource, World, WorldConfig, WorldPosition, WorldRect,
};
use crate::{agent, needs, policy, resources, sleep, spatial, structures};

mod actions;
mod autonomy;
mod cognition;
mod health_sleep;
mod lifecycle;
mod measurements;
mod movement;
mod shelter;

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
