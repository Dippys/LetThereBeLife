use std::{error::Error, fmt};

use sim_core::{
    AgentId, AgentSpawnError, Engine, EngineCommand, PolicyActivationError, PolicyOptions,
    PopulationInit, PopulationInitError, World, WorldPosition, WorldRect,
};

pub const VIEWER_AGENT_LIMIT: usize = 4_096;

#[derive(Debug)]
pub enum ViewerSpawnError {
    Unloaded,
    PresentationLimit,
    Population(PopulationInitError),
    Policy(PolicyActivationError),
    Agent(AgentSpawnError),
}

impl fmt::Display for ViewerSpawnError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unloaded => formatter.write_str("the cursor is not over loaded terrain"),
            Self::PresentationLimit => write!(
                formatter,
                "viewer agent limit of {VIEWER_AGENT_LIMIT} has been reached"
            ),
            Self::Population(error) => write!(formatter, "first agent spawn failed: {error}"),
            Self::Policy(error) => write!(formatter, "agent policy activation failed: {error}"),
            Self::Agent(error) => error.fmt(formatter),
        }
    }
}

impl Error for ViewerSpawnError {}

pub fn residency_ready(world: &World) -> bool {
    world.loaded_chunk_count() > 0
}

pub fn spawn_at(engine: &mut Engine, position: WorldPosition) -> Result<AgentId, ViewerSpawnError> {
    if engine.snapshot().agent_count as usize >= VIEWER_AGENT_LIMIT {
        return Err(ViewerSpawnError::PresentationLimit);
    }
    if engine.snapshot().agent_count > 0 {
        return engine
            .spawn_agent(position)
            .map_err(ViewerSpawnError::Agent);
    }

    let active_area = initial_active_area(engine.world(), position)?;
    let outcome = engine
        .initialize_population(
            PopulationInit {
                active_area,
                population: 1,
            },
            &[position],
        )
        .map_err(ViewerSpawnError::Population)?;
    if let Err(error) = engine.activate_physical_policy_with_options(PolicyOptions::full()) {
        engine.command(EngineCommand::Reset);
        return Err(ViewerSpawnError::Policy(error));
    }
    Ok(outcome.first_id)
}

fn initial_active_area(
    world: &World,
    position: WorldPosition,
) -> Result<WorldRect, ViewerSpawnError> {
    world
        .loaded_bounds_at(position)
        .ok_or(ViewerSpawnError::Unloaded)
}

pub fn reset(engine: &mut Engine) {
    engine.command(EngineCommand::Reset);
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_core::{EngineConfig, WorldConfig};

    const TEST_SIMULATION_SIDE: i64 = 2_048;

    fn simulation_bounds(world: &World) -> WorldRect {
        let half = TEST_SIMULATION_SIDE / 2;
        let fixed = WorldRect {
            min: WorldPosition { x: -half, y: -half },
            max: WorldPosition { x: half, y: half },
        };
        fixed
            .intersection(world.initial_bounds())
            .expect("validated initial worlds include the origin")
    }

    fn engine() -> Engine {
        Engine::new(EngineConfig {
            seed: 7,
            ticks_per_second: 60,
            world: WorldConfig::new(64, 64).unwrap(),
        })
    }

    fn resident_engine(reverse: bool) -> Engine {
        let mut engine = engine();
        let mut loads: Vec<_> = engine
            .world()
            .missing_chunk_load_requests(simulation_bounds(engine.world()))
            .unwrap()
            .into_iter()
            .map(|request| World::generate_chunk_load(7, request))
            .collect();
        if reverse {
            loads.reverse();
        }
        for load in loads {
            engine.apply_world_chunk_loads(vec![load]).unwrap();
        }
        engine
    }

    fn first_standable(engine: &Engine) -> WorldPosition {
        let bounds = simulation_bounds(engine.world());
        (bounds.min.y..bounds.max.y)
            .flat_map(|y| (bounds.min.x..bounds.max.x).map(move |x| WorldPosition { x, y }))
            .find(|position| {
                engine.world().standability_at(*position) == Ok(sim_core::Standability::Standable)
            })
            .unwrap()
    }

    #[test]
    fn residency_becomes_ready_without_automatic_agents() {
        let mut engine = engine();
        assert!(!residency_ready(engine.world()));
        engine = resident_engine(false);
        assert!(residency_ready(engine.world()));
        assert_eq!(engine.snapshot().agent_count, 0);
        assert_eq!(engine.snapshot().tick, 0);
    }

    #[test]
    fn cursor_spawns_are_individual_dense_and_resettable() {
        let mut engine = resident_engine(false);
        let first = first_standable(&engine);
        let second = WorldPosition {
            x: first.x + 1,
            y: first.y,
        };
        assert_eq!(spawn_at(&mut engine, first).unwrap(), AgentId::new(0));
        let second_id =
            if engine.world().standability_at(second) == Ok(sim_core::Standability::Standable) {
                spawn_at(&mut engine, second).unwrap()
            } else {
                let alternative = (first.y..simulation_bounds(engine.world()).max.y)
                    .flat_map(|y| {
                        (simulation_bounds(engine.world()).min.x
                            ..simulation_bounds(engine.world()).max.x)
                            .map(move |x| WorldPosition { x, y })
                    })
                    .find(|position| {
                        *position != first
                            && engine.world().standability_at(*position)
                                == Ok(sim_core::Standability::Standable)
                    })
                    .unwrap();
                spawn_at(&mut engine, alternative).unwrap()
            };
        assert_eq!(second_id, AgentId::new(1));
        assert_eq!(engine.snapshot().agent_count, 2);

        reset(&mut engine);
        assert_eq!(engine.snapshot().agent_count, 0);
        assert_eq!(spawn_at(&mut engine, first).unwrap(), AgentId::new(0));
    }

    #[test]
    fn worker_completion_order_does_not_change_cursor_spawn() {
        let mut forward = resident_engine(false);
        let mut reverse = resident_engine(true);
        let position = first_standable(&forward);
        spawn_at(&mut forward, position).unwrap();
        spawn_at(&mut reverse, position).unwrap();
        assert_eq!(forward.policy_options(), PolicyOptions::full());
        for _ in 0..600 {
            forward.tick();
            reverse.tick();
        }
        assert_eq!(forward.snapshot(), reverse.snapshot());
        assert_eq!(
            forward.agent_views(usize::MAX).collect::<Vec<_>>(),
            reverse.agent_views(usize::MAX).collect::<Vec<_>>()
        );
        assert_ne!(
            forward.agent_views(1).next().unwrap().position,
            position,
            "viewer full-mind mode must produce authoritative movement"
        );
    }

    #[test]
    fn first_spawn_depends_only_on_cursor_residency() {
        let mut engine = Engine::new(EngineConfig {
            seed: 7,
            ticks_per_second: 60,
            world: WorldConfig::new(TEST_SIMULATION_SIDE as u32 + 64, 64).unwrap(),
        });
        let cursor_area = WorldRect {
            min: WorldPosition { x: 0, y: 0 },
            max: WorldPosition { x: 1, y: 1 },
        };
        let requests = engine
            .world()
            .missing_chunk_load_requests(cursor_area)
            .unwrap();
        assert_eq!(requests.len(), 1);
        engine
            .apply_world_chunk_loads(vec![World::generate_chunk_load(7, requests[0])])
            .unwrap();

        assert!(
            !engine
                .world()
                .area_is_generated(simulation_bounds(engine.world()))
        );
        let loaded = engine
            .world()
            .loaded_bounds_at(WorldPosition { x: 0, y: 0 })
            .unwrap();
        let position = (loaded.min.y..loaded.max.y)
            .flat_map(|y| (loaded.min.x..loaded.max.x).map(move |x| WorldPosition { x, y }))
            .find(|position| {
                engine.world().standability_at(*position) == Ok(sim_core::Standability::Standable)
            })
            .expect("seeded loaded terrain exposes a standable position");

        let agent = spawn_at(&mut engine, position).unwrap();
        let perception = engine.perceive_physical(agent, 8).unwrap();

        assert_eq!(agent, AgentId::new(0));
        assert!(loaded.contains_rect(perception.area));
    }

    #[test]
    fn first_spawn_rejects_only_an_unloaded_cursor() {
        let mut engine = engine();

        assert!(matches!(
            spawn_at(&mut engine, WorldPosition { x: 0, y: 0 }),
            Err(ViewerSpawnError::Unloaded)
        ));
    }
}
