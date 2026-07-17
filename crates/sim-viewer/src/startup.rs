use std::{error::Error, fmt};

use sim_core::{
    AgentId, AgentSpawnError, ChunkCoord, Engine, EngineCommand, GenerateAreaError,
    PolicyActivationError, PopulationInit, PopulationInitError, World, WorldPosition, WorldRect,
};

pub const VIEWER_AGENT_LIMIT: usize = 4_096;
pub const VIEWER_SIMULATION_SIDE: i64 = 2_048;

#[derive(Debug)]
pub enum ViewerSpawnError {
    NotReady,
    PresentationLimit,
    Population(PopulationInitError),
    Policy(PolicyActivationError),
    Agent(AgentSpawnError),
}

impl fmt::Display for ViewerSpawnError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotReady => formatter.write_str("agent spawning is waiting for world residency"),
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

pub fn simulation_bounds(world: &World) -> WorldRect {
    let half = VIEWER_SIMULATION_SIDE / 2;
    let fixed = WorldRect {
        min: WorldPosition { x: -half, y: -half },
        max: WorldPosition { x: half, y: half },
    };
    fixed
        .intersection(world.initial_bounds())
        .expect("validated initial worlds include the origin")
}

pub fn residency_ready(world: &World) -> Result<bool, GenerateAreaError> {
    Ok(world
        .missing_chunk_load_requests(simulation_bounds(world))?
        .is_empty())
}

pub fn spawn_at(engine: &mut Engine, position: WorldPosition) -> Result<AgentId, ViewerSpawnError> {
    if !residency_ready(engine.world()).map_err(|_| ViewerSpawnError::NotReady)? {
        return Err(ViewerSpawnError::NotReady);
    }
    if engine.snapshot().agent_count as usize >= VIEWER_AGENT_LIMIT {
        return Err(ViewerSpawnError::PresentationLimit);
    }
    if engine.snapshot().agent_count > 0 {
        return engine
            .spawn_agent(position)
            .map_err(ViewerSpawnError::Agent);
    }

    let active_area = initial_spawn_area(engine.world(), position)?;
    let outcome = engine
        .initialize_population(
            PopulationInit {
                active_area,
                population: 1,
            },
            &[position],
        )
        .map_err(ViewerSpawnError::Population)?;
    if let Err(error) = engine.activate_physical_policy_with_exploration() {
        engine.command(EngineCommand::Reset);
        return Err(ViewerSpawnError::Policy(error));
    }
    Ok(outcome.first_id)
}

fn initial_spawn_area(
    world: &World,
    position: WorldPosition,
) -> Result<WorldRect, ViewerSpawnError> {
    if world.standability_at(position).is_err() {
        return Err(ViewerSpawnError::NotReady);
    }
    let initial = world.initial_bounds();
    if initial.contains(position) && world.area_is_generated(initial) {
        return Ok(initial);
    }
    let chunk = ChunkCoord::from_world_position(position)
        .bounds()
        .map_err(|_| ViewerSpawnError::NotReady)?;
    if world.area_is_generated(chunk) {
        return Ok(chunk);
    }
    Ok(WorldRect {
        min: position,
        max: WorldPosition {
            x: position.x + 1,
            y: position.y + 1,
        },
    })
}

pub fn reset(engine: &mut Engine) {
    engine.command(EngineCommand::Reset);
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_core::{EngineConfig, WorldConfig};

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
        assert!(!residency_ready(engine.world()).unwrap());
        engine = resident_engine(false);
        assert!(residency_ready(engine.world()).unwrap());
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
            "viewer exploration mode must produce authoritative movement"
        );
    }
}
