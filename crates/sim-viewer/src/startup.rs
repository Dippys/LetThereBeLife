use std::{error::Error, fmt};

use rayon::prelude::*;
use sim_core::{
    AgentId, AgentSpawnError, Engine, EngineCommand, GenerateAreaError, PolicyActivationError,
    PolicyOptions, PopulationInit, PopulationInitError, VALLEY_BAND, VALLEY_CHILDREN_PER_FAMILY,
    VALLEY_FAMILIES, VALLEY_SIDE, World, WorldPosition, WorldRect, band_layout, find_valley,
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
    spawn_band(engine, active_area, &[position])
}

/// Starts the population in one step: agents at `sites` (in order) inside an
/// `active_area` they may roam, then full minds. A policy failure resets the
/// engine so no half-initialized population remains.
pub fn spawn_band(
    engine: &mut Engine,
    active_area: WorldRect,
    sites: &[WorldPosition],
) -> Result<AgentId, ViewerSpawnError> {
    if sites.len() > VIEWER_AGENT_LIMIT {
        return Err(ViewerSpawnError::PresentationLimit);
    }
    let outcome = engine
        .initialize_population(
            PopulationInit {
                active_area,
                population: sites.len() as u32,
            },
            sites,
        )
        .map_err(ViewerSpawnError::Population)?;
    if let Err(error) = engine.activate_physical_policy_with_options(PolicyOptions::full()) {
        engine.command(EngineCommand::Reset);
        return Err(ViewerSpawnError::Policy(error));
    }
    Ok(outcome.first_id)
}

/// The `--valley` preset as started: the valley and where the band camps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ValleyStart {
    pub bounds: WorldRect,
    pub camp: WorldPosition,
}

#[derive(Debug)]
pub enum ValleyStartError {
    NotFound(u64),
    Generation(GenerateAreaError),
    NoCamp,
    Spawn(ViewerSpawnError),
}

impl fmt::Display for ValleyStartError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound(seed) => {
                write!(
                    formatter,
                    "no livable valley near the origin for seed {seed}"
                )
            }
            Self::Generation(error) => write!(formatter, "valley terrain failed: {error}"),
            Self::NoCamp => formatter.write_str("the valley has no room for the band beside water"),
            Self::Spawn(error) => write!(formatter, "band spawn failed: {error}"),
        }
    }
}

impl Error for ValleyStartError {}

/// Finds the seed's valley, generates it synchronously (chunks in parallel,
/// inserted in request order, so the result is deterministic), and spawns the
/// band (two families of founders, each with children) at its camps with the whole valley as the active area. Requires
/// an engine with no population yet.
pub fn start_valley(engine: &mut Engine) -> Result<ValleyStart, ValleyStartError> {
    let seed = engine.config().seed;
    let valley = find_valley(seed, VALLEY_SIDE).ok_or(ValleyStartError::NotFound(seed))?;
    let loads = engine
        .world()
        .missing_chunk_load_requests(valley.bounds)
        .map_err(ValleyStartError::Generation)?
        .into_par_iter()
        .map(|request| World::generate_chunk_load(seed, request))
        .collect();
    engine
        .apply_world_chunk_loads(loads)
        .map_err(ValleyStartError::Generation)?;
    let layout = band_layout(
        engine.world(),
        valley.bounds,
        VALLEY_FAMILIES,
        VALLEY_BAND / VALLEY_FAMILIES,
        VALLEY_CHILDREN_PER_FAMILY,
        seed,
    )
    .ok_or(ValleyStartError::NoCamp)?;
    spawn_band(engine, valley.bounds, &layout.sites).map_err(ValleyStartError::Spawn)?;
    engine.set_founders(layout.founders as u32);
    for &(child, parent) in &layout.parents {
        engine.bond(AgentId::new(child as u32), AgentId::new(parent as u32));
    }
    Ok(ValleyStart {
        bounds: valley.bounds,
        camp: layout.sites[0],
    })
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
    fn valley_preset_starts_the_band_inside_the_valley_with_full_minds() {
        let mut engine = Engine::new(EngineConfig {
            seed: 1,
            ticks_per_second: 60,
            world: WorldConfig::new(64, 64).unwrap(),
        });
        let start = start_valley(&mut engine).unwrap();
        let valley = find_valley(1, VALLEY_SIDE).unwrap();

        assert_eq!(start.bounds, valley.bounds);
        assert!(engine.world().area_is_generated(valley.bounds));
        assert_eq!(
            engine.snapshot().agent_count as usize,
            VALLEY_BAND + VALLEY_FAMILIES * VALLEY_CHILDREN_PER_FAMILY
        );
        assert_eq!(engine.policy_options(), PolicyOptions::full());
        let agents: Vec<_> = engine.agent_views(usize::MAX).collect();
        assert_eq!(agents[0].position, start.camp);
        assert!(
            agents
                .iter()
                .all(|agent| valley.bounds.contains(agent.position))
        );
        // A second start finds the population already there.
        assert!(matches!(
            start_valley(&mut engine),
            Err(ValleyStartError::Spawn(ViewerSpawnError::Population(
                PopulationInitError::AlreadyInitialized
            )))
        ));
    }

    #[test]
    fn band_spawns_explicit_sites_in_order_and_resets_cleanly() {
        let mut engine = resident_engine(false);
        let bounds = simulation_bounds(engine.world());
        let sites: Vec<_> = (bounds.min.y..bounds.max.y)
            .flat_map(|y| (bounds.min.x..bounds.max.x).map(move |x| WorldPosition { x, y }))
            .filter(|position| {
                engine.world().standability_at(*position) == Ok(sim_core::Standability::Standable)
            })
            .step_by(97)
            .take(5)
            .collect();
        assert_eq!(
            spawn_band(&mut engine, bounds, &sites).unwrap(),
            AgentId::new(0)
        );
        assert_eq!(
            engine
                .agent_views(usize::MAX)
                .map(|agent| agent.position)
                .collect::<Vec<_>>(),
            sites
        );
        assert_eq!(engine.policy_options(), PolicyOptions::full());
        reset(&mut engine);
        assert_eq!(engine.snapshot().agent_count, 0);
        assert!(matches!(
            spawn_band(&mut engine, bounds, &vec![sites[0]; VIEWER_AGENT_LIMIT + 1]),
            Err(ViewerSpawnError::PresentationLimit)
        ));
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
