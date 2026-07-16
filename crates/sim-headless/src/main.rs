use sim_config::{AppConfig, DEFAULT_CONFIG_PATH};
use sim_core::{
    AgentActivity, Engine, MovementOutcomeKind, PolicyDiagnosticKind, PopulationInit,
    RouteOutcomeKind,
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
    engine.activate_physical_policy()?;
    let mut completed_movements = 0_u32;
    let mut completed_routes = 0_u32;
    let mut failed_routes = 0_u32;
    let mut policy_selections = 0_u32;
    let mut policy_failures = 0_u32;
    for _ in 0..ticks {
        engine.tick();
        completed_movements += engine
            .movement_outcomes()
            .iter()
            .filter(|outcome| outcome.kind == MovementOutcomeKind::Moved)
            .count() as u32;
        for outcome in engine.route_outcomes() {
            if outcome.kind == RouteOutcomeKind::Arrived {
                completed_routes += 1;
            } else {
                failed_routes += 1;
            }
        }
        policy_selections += engine
            .policy_diagnostics()
            .iter()
            .filter(|diagnostic| diagnostic.kind == PolicyDiagnosticKind::Selected)
            .count() as u32;
        policy_failures += engine
            .policy_diagnostics()
            .iter()
            .filter(|diagnostic| diagnostic.failure.is_some())
            .count() as u32;
    }

    let snapshot = engine.snapshot();
    let moving_agents = engine
        .agent_views(agent_count as usize)
        .filter(|agent| agent.activity == AgentActivity::Moving)
        .count();
    println!(
        "completed tick={} simulated_seconds={:.3} seed={} initial_world={}x{} agents={} routes={} route_failures={} movements={} moving={} policy_selections={} policy_failures={}",
        snapshot.tick,
        snapshot.simulated_seconds,
        snapshot.seed,
        engine.world().width(),
        engine.world().height(),
        snapshot.agent_count,
        completed_routes,
        failed_routes,
        completed_movements,
        moving_agents,
        policy_selections,
        policy_failures,
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
