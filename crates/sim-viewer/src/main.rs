//! Viewer entry point: parses launch options, optionally pre-bakes the world archive, and runs the event loop.

mod app;
mod camera;
mod feed;
mod generation;
mod gestures;
mod labels;
mod launch;
mod progress;
mod render;
mod screenshot;
mod startup;

use std::time::Instant;

use sim_config::AppConfig;
use sim_core::{Engine, WorldArchive};
use winit::event_loop::{ControlFlow, EventLoop};

use app::ViewerApp;
use launch::{launch_options, load_world_archive};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let options = launch_options()?;
    let config = AppConfig::load(&options.config_path)?;
    let mut engine_config = config.engine_config()?;
    engine_config.life_seed = options.life_seed.unwrap_or_else(launch::fresh_life_seed);
    if options.pregenerate_world {
        let mut last_reported = 0;
        let stats = WorldArchive::bake_full(
            engine_config.world_seed,
            &config.world_cache.path,
            |progress| {
                if progress.completed_chunks == progress.total_chunks
                    || progress.completed_chunks.saturating_sub(last_reported) >= 16_384
                {
                    println!(
                        "baked {}/{} chunks ({:.1} GiB)",
                        progress.completed_chunks,
                        progress.total_chunks,
                        progress.bytes_written as f64 / 1024_f64.powi(3)
                    );
                    last_reported = progress.completed_chunks;
                }
            },
        )?;
        println!(
            "pre-generated complete seed {} world to {}: {} chunks, {:.2} GiB in {:.2?}",
            engine_config.world_seed,
            config.world_cache.path,
            stats.chunks,
            stats.bytes as f64 / 1024_f64.powi(3),
            stats.elapsed
        );
        return Ok(());
    }
    let archive = load_world_archive(engine_config.world_seed, &config);
    let loaded = match &options.load {
        Some(path) => {
            let started = Instant::now();
            let engine = Engine::load(std::fs::File::open(path)?)?;
            println!(
                "opened {} (year {}, {} people) in {:.2?}",
                path.display(),
                engine.snapshot().simulated_seconds as i64 / sim_core::SECONDS_PER_YEAR + 1,
                engine.snapshot().living_agent_count,
                started.elapsed()
            );
            Some(engine)
        }
        None => None,
    };
    let opened_save = loaded.is_some();
    let mut engine = loaded.unwrap_or_else(|| Engine::new(engine_config));
    println!(
        "world seed {}, life seed {} (replay with --life-seed {})",
        engine.config().world_seed,
        engine.config().life_seed,
        engine.config().life_seed
    );
    let valley = (options.valley && !opened_save).then(|| {
        let started = Instant::now();
        match startup::start_valley(&mut engine) {
            Ok(start) => {
                println!(
                    "valley preset: {} agents in {},{}..{},{} (camp {},{}) ready in {:.2?}",
                    engine.snapshot().agent_count,
                    start.bounds.min.x,
                    start.bounds.min.y,
                    start.bounds.max.x,
                    start.bounds.max.y,
                    start.camp.x,
                    start.camp.y,
                    started.elapsed()
                );
                Some(start)
            }
            Err(error) => {
                eprintln!("valley preset unavailable ({error}); continuing with normal startup");
                None
            }
        }
    });
    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Wait);
    let mut app = ViewerApp::new(engine, archive, options.smoke_frames, options.screenshot);
    if let Some(cells) = options.zoom {
        app = app.with_view_span(cells);
    }
    if let Some(start) = valley.flatten() {
        app = app.with_valley(start);
    }
    if opened_save {
        app = app.with_opened_world();
    }
    if options.advance > 0 {
        let started = Instant::now();
        app.advance(options.advance);
        println!(
            "advanced {} ticks in {:.2?}",
            options.advance,
            started.elapsed()
        );
    }
    if let Some(path) = &options.save {
        app.save_world(path)?;
        println!("saved the world to {}", path.display());
    }
    if let Some(person) = options.select {
        app = app.with_selected(sim_core::AgentId::new(person));
    }
    event_loop.run_app(&mut app)?;
    Ok(())
}
