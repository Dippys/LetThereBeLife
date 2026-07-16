use sim_config::{AppConfig, DEFAULT_CONFIG_PATH};
use sim_headless::{CANONICAL_TICKS, ScenarioConfig, ScenarioRunner};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut ticks = None;
    let mut seed = None;
    let mut agent_count = 20_u32;
    let mut batch_size = 1_000_u64;
    let mut canonical = false;
    let mut config_path = DEFAULT_CONFIG_PATH.to_owned();
    let mut args = std::env::args().skip(1);

    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--canonical" => canonical = true,
            "--ticks" => ticks = Some(parse_next(&mut args, "--ticks")),
            "--seed" => seed = Some(parse_next(&mut args, "--seed")),
            "--agents" => agent_count = parse_next(&mut args, "--agents"),
            "--batch-size" => batch_size = parse_next(&mut args, "--batch-size"),
            "--config" => config_path = parse_next(&mut args, "--config"),
            "--help" | "-h" => {
                println!(
                    "Usage: sim-headless [--canonical] [--config PATH] [--ticks NUMBER] [--seed NUMBER] [--agents NUMBER] [--batch-size NUMBER]"
                );
                return Ok(());
            }
            unknown => {
                eprintln!("Unknown argument: {unknown}");
                std::process::exit(2);
            }
        }
    }

    let mut scenario = if canonical {
        ScenarioConfig::canonical(agent_count)
    } else {
        ScenarioConfig {
            engine: AppConfig::load(config_path)?.engine_config()?,
            population: agent_count,
            driver_ticks: 600,
            access_radius: sim_core::PHYSICAL_POLICY_RADIUS,
            initial_food_per_agent: 0,
            initial_wood_per_water_agent: 0,
        }
    };
    scenario.driver_ticks = ticks.unwrap_or(if canonical { CANONICAL_TICKS } else { 600 });
    if let Some(seed) = seed {
        scenario.engine.seed = seed;
    }

    let report = ScenarioRunner::new(scenario)?.run(batch_size)?;
    println!("{report}");
    Ok(())
}

fn parse_next<T: std::str::FromStr>(args: &mut impl Iterator<Item = String>, name: &str) -> T {
    args.next()
        .and_then(|value| value.parse().ok())
        .unwrap_or_else(|| {
            eprintln!("{name} requires a valid value");
            std::process::exit(2);
        })
}
