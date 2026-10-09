//! Viewer entry point: parses launch options, optionally pre-bakes the world archive, and runs the event loop.

mod app;
mod camera;
mod generation;
mod launch;
mod render;
mod spawn_menu;
mod startup;

use sim_config::AppConfig;
use sim_core::{Engine, WorldArchive};
use winit::event_loop::{ControlFlow, EventLoop};

use app::ViewerApp;
use launch::{launch_options, load_world_archive};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let options = launch_options()?;
    let config = AppConfig::load(&options.config_path)?;
    let engine_config = config.engine_config()?;
    if options.pregenerate_world {
        let mut last_reported = 0;
        let stats =
            WorldArchive::bake_full(engine_config.seed, &config.world_cache.path, |progress| {
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
            })?;
        println!(
            "pre-generated complete seed {} world to {}: {} chunks, {:.2} GiB in {:.2?}",
            engine_config.seed,
            config.world_cache.path,
            stats.chunks,
            stats.bytes as f64 / 1024_f64.powi(3),
            stats.elapsed
        );
        return Ok(());
    }
    let archive = load_world_archive(engine_config.seed, &config);
    let engine = Engine::new(engine_config);
    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Wait);
    let mut app = ViewerApp::new(engine, archive, options.smoke_frames);
    event_loop.run_app(&mut app)?;
    Ok(())
}
