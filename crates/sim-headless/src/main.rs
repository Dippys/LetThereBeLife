use sim_config::{AppConfig, DEFAULT_CONFIG_PATH};
use sim_core::{
    AgentActivity, Engine, MovementOutcomeKind, PopulationInit, TraversalStep, WorldPosition,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut ticks = 600_u64;
    let mut seed = None;
    let mut agent_count = 20_u32;
    let mut config_path = DEFAULT_CONFIG_PATH.to_owned();
    let mut args = std::env::args().skip(1);

    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--ticks" => ticks = parse_next(&mut args, "--ticks"),
            "--seed" => seed = Some(parse_next(&mut args, "--seed")),
            "--agents" => agent_count = parse_next(&mut args, "--agents"),
            "--config" => config_path = parse_next(&mut args, "--config"),
            "--help" | "-h" => {
                println!(
                    "Usage: sim-headless [--config PATH] [--ticks NUMBER] [--seed NUMBER] [--agents NUMBER]"
                );
                return Ok(());
            }
            unknown => {
                eprintln!("Unknown argument: {unknown}");
                std::process::exit(2);
            }
        }
    }

    let mut engine_config = AppConfig::load(config_path)?.engine_config()?;
    if let Some(seed) = seed {
        engine_config.seed = seed;
    }
    let mut engine = Engine::new(engine_config);
    engine.materialize_initial_area()?;
    let active_area = engine.world().initial_bounds();
    engine.initialize_population(
        PopulationInit {
            active_area,
            population: agent_count,
        },
        &[],
    )?;
    let initial_agents: Vec<_> = engine.agent_views(agent_count as usize).collect();
    let mut scheduled_movements = 0_u32;
    for agent in initial_agents {
        let target = [(1, 0), (0, 1), (-1, 0), (0, -1)]
            .into_iter()
            .map(|(dx, dy)| WorldPosition {
                x: agent.position.x + dx,
                y: agent.position.y + dy,
            })
            .find(|&target| {
                active_area.contains(target)
                    && engine
                        .world()
                        .traversal_step(agent.position, target)
                        .is_ok_and(TraversalStep::is_passable)
            });
        if let Some(target) = target {
            engine.request_move(agent.id, target)?;
            scheduled_movements += 1;
        }
    }
    let mut completed_movements = 0_u32;
    for _ in 0..ticks {
        engine.tick();
        completed_movements += engine
            .movement_outcomes()
            .iter()
            .filter(|outcome| outcome.kind == MovementOutcomeKind::Moved)
            .count() as u32;
    }

    let snapshot = engine.snapshot();
    let moving_agents = engine
        .agent_views(agent_count as usize)
        .filter(|agent| agent.activity == AgentActivity::Moving)
        .count();
    println!(
        "completed tick={} simulated_seconds={:.3} seed={} initial_world={}x{} agents={} movements={}/{} moving={}",
        snapshot.tick,
        snapshot.simulated_seconds,
        snapshot.seed,
        engine.world().width(),
        engine.world().height(),
        snapshot.agent_count,
        completed_movements,
        scheduled_movements,
        moving_agents,
    );
    Ok(())
}

fn parse_next<T: std::str::FromStr>(args: &mut impl Iterator<Item = String>, name: &str) -> T {
    args.next()
        .and_then(|value| value.parse().ok())
        .unwrap_or_else(|| {
            eprintln!("{name} requires a valid number");
            std::process::exit(2);
        })
}
