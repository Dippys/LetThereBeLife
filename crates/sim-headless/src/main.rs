use sim_config::{AppConfig, DEFAULT_CONFIG_PATH};
use sim_headless::{CANONICAL_TICKS, ScenarioConfig, ScenarioRunner, StudyConfig, run_study};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut ticks = None;
    let mut seed = None;
    let mut agent_count = None;
    let mut batch_size = 1_000_u64;
    let mut canonical = false;
    let mut study = false;
    let mut verbose = false;
    let mut near_water = false;
    let mut groups = false;
    let mut valley = false;
    let mut trace = None;
    let mut comms_lines = 0_usize;
    let mut explain = None;
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
            "--valley" => valley = true,
            "--trace" => trace = Some(parse_next(&mut args, "--trace")),
            "--comms" => comms_lines = parse_next(&mut args, "--comms"),
            "--explain" => explain = Some(parse_next::<u32>(&mut args, "--explain")),
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
            "--agents" => agent_count = Some(parse_next(&mut args, "--agents")),
            "--batch-size" => batch_size = parse_next(&mut args, "--batch-size"),
            "--config" => config_path = parse_next(&mut args, "--config"),
            "--help" | "-h" => {
                println!(
                    "Usage: sim-headless [--canonical | --study [--near-water | --groups | --valley] [--mind legacy|memory|sharing|full] [--verbose] [--trace AGENT] [--comms N] [--explain AGENT]] [--config PATH] [--ticks NUMBER] [--seed NUMBER] [--agents NUMBER] [--batch-size NUMBER]"
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
        let population = agent_count.unwrap_or(if valley {
            sim_headless::VALLEY_POPULATION
        } else {
            20
        });
        let mut config = StudyConfig::new(seed.unwrap_or(1), population, ticks.unwrap_or(600_000));
        config.mind = mind;
        config.trace = trace.or(explain);
        if near_water {
            config.spawn = sim_headless::StudySpawn::NearWater;
        }
        if groups {
            config.spawn = sim_headless::StudySpawn::Groups;
        }
        if valley {
            config.spawn = sim_headless::StudySpawn::Valley;
        }
        let report = run_study(config)?;
        println!("{report}");
        if verbose {
            for line in &report.per_agent {
                println!("{line}");
            }
        }
        if comms_lines > 0 {
            println!("exchanges that led somewhere (first {comms_lines}):");
            for exchange in report
                .comms
                .exchanges()
                .iter()
                .filter(|exchange| exchange.receptions.iter().any(|r| r.outcome.is_some()))
                .take(comms_lines)
            {
                println!("  {exchange}");
            }
        }
        if let Some(agent) = explain {
            print!("{}", sim_headless::explain(&report, agent));
        }
        for line in &report.trace {
            println!("  trace {line}");
        }
        return Ok(());
    }

    let mut scenario = if canonical {
        ScenarioConfig::canonical(agent_count.unwrap_or(20))
    } else {
        ScenarioConfig {
            engine: AppConfig::load(config_path)?.engine_config()?,
            population: agent_count.unwrap_or(20),
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
