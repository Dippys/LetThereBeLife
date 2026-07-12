use sim_core::{Engine, EngineConfig};

fn main() {
    let mut ticks = 600_u64;
    let mut seed = 1_u64;
    let mut args = std::env::args().skip(1);

    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--ticks" => ticks = parse_next(&mut args, "--ticks"),
            "--seed" => seed = parse_next(&mut args, "--seed"),
            "--help" | "-h" => {
                println!("Usage: sim-server [--ticks NUMBER] [--seed NUMBER]");
                return;
            }
            unknown => {
                eprintln!("Unknown argument: {unknown}");
                std::process::exit(2);
            }
        }
    }

    let mut engine = Engine::new(EngineConfig {
        seed,
        ..EngineConfig::default()
    });
    for _ in 0..ticks {
        engine.tick();
    }

    let snapshot = engine.snapshot();
    println!(
        "completed tick={} simulated_seconds={:.3} seed={}",
        snapshot.tick, snapshot.simulated_seconds, snapshot.seed
    );
}

fn parse_next<T: std::str::FromStr>(args: &mut impl Iterator<Item = String>, name: &str) -> T {
    args.next()
        .and_then(|value| value.parse().ok())
        .unwrap_or_else(|| {
            eprintln!("{name} requires a valid number");
            std::process::exit(2);
        })
}
