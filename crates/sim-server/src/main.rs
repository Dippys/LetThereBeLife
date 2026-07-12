use sim_config::{AppConfig, DEFAULT_CONFIG_PATH};
use sim_core::Engine;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut ticks = 600_u64;
    let mut seed = None;
    let mut config_path = DEFAULT_CONFIG_PATH.to_owned();
    let mut args = std::env::args().skip(1);

    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--ticks" => ticks = parse_next(&mut args, "--ticks"),
            "--seed" => seed = Some(parse_next(&mut args, "--seed")),
            "--config" => config_path = parse_next(&mut args, "--config"),
            "--help" | "-h" => {
                println!("Usage: sim-server [--config PATH] [--ticks NUMBER] [--seed NUMBER]");
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
    for _ in 0..ticks {
        engine.tick();
    }

    let snapshot = engine.snapshot();
    println!(
        "completed tick={} simulated_seconds={:.3} seed={} initial_world={}x{}",
        snapshot.tick,
        snapshot.simulated_seconds,
        snapshot.seed,
        engine.world().width(),
        engine.world().height()
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
