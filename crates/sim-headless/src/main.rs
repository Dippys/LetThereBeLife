use sim_config::{AppConfig, DEFAULT_CONFIG_PATH};
use sim_headless::{CANONICAL_TICKS, ScenarioConfig, ScenarioRunner, StudyConfig, run_study};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut ticks = None;
    let mut seed = None;
    let mut agent_count = 20_u32;
    let mut batch_size = 1_000_u64;
    let mut canonical = false;
    let mut study = false;
    let mut verbose = false;
    let mut near_water = false;
    let mut groups = false;
    let mut trace = None;
    let mut mind = sim_core::PolicyOptions::full();
    let mut config_path = DEFAULT_CONFIG_PATH.to_owned();
    let mut args = std::env::args().skip(1);

    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--canonical" => canonical = true,
            "--study" => study = true,
            "--verbose" => verbose = true,
            "--near-water" => near_water = true,
            "--groups" => groups = true,
            "--trace" => trace = Some(parse_next(&mut args, "--trace")),
            "--mind" => {
                mind = match args.next().as_deref() {
                    Some("legacy") => sim_core::PolicyOptions {
                        exploration: true,
                        ..sim_core::PolicyOptions::default()
                    },
                    Some("memory") => sim_core::PolicyOptions {
                        sharing: false,
                        social: false,
                        ..sim_core::PolicyOptions::full()
                    },
                    Some("sharing") => sim_core::PolicyOptions {
                        social: false,
                        ..sim_core::PolicyOptions::full()
                    },
                    Some("full") => sim_core::PolicyOptions::full(),
                    _ => {
                        eprintln!("--mind requires legacy, memory, sharing, or full");
                        std::process::exit(2);
                    }
                }
            }
            "--ticks" => ticks = Some(parse_next(&mut args, "--ticks")),
            "--seed" => seed = Some(parse_next(&mut args, "--seed")),
            "--agents" => agent_count = parse_next(&mut args, "--agents"),
            "--batch-size" => batch_size = parse_next(&mut args, "--batch-size"),
            "--config" => config_path = parse_next(&mut args, "--config"),
            "--help" | "-h" => {
                println!(
                    "Usage: sim-headless [--canonical | --study [--near-water | --groups] [--mind legacy|memory|sharing|full] [--verbose] [--trace AGENT]] [--config PATH] [--ticks NUMBER] [--seed NUMBER] [--agents NUMBER] [--batch-size NUMBER]"
                );
                return Ok(());
            }
            unknown => {
                eprintln!("Unknown argument: {unknown}");
                std::process::exit(2);
            }
        }
    }

    if study {
        let mut config = StudyConfig::new(seed.unwrap_or(1), agent_count, ticks.unwrap_or(600_000));
        config.mind = mind;
        config.trace = trace;
        if near_water {
            config.spawn = sim_headless::StudySpawn::NearWater;
        }
        if groups {
            config.spawn = sim_headless::StudySpawn::Groups;
        }
        let report = run_study(config)?;
        println!("{report}");
        if verbose {
            for line in &report.per_agent {
                println!("{line}");
            }
        }
        for line in &report.trace {
            println!("  trace {line}");
        }
        return Ok(());
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
