//! Command-line launch options and full-world archive loading.

use std::{fs, path::PathBuf, time::Instant};

use sim_config::{AppConfig, DEFAULT_CONFIG_PATH};
use sim_core::WorldArchive;

pub(super) struct LaunchOptions {
    pub(super) config_path: String,
    pub(super) smoke_frames: Option<u32>,
    pub(super) pregenerate_world: bool,
    pub(super) valley: bool,
    /// Ticks to run before the window opens (with `--valley`).
    pub(super) advance: u64,
    /// Where to save the last smoke frame as a PNG.
    pub(super) screenshot: Option<PathBuf>,
    /// A person whose panel starts open.
    pub(super) select: Option<u32>,
}

pub(super) fn launch_options() -> Result<LaunchOptions, Box<dyn std::error::Error>> {
    let mut config_path = DEFAULT_CONFIG_PATH.to_owned();
    let mut smoke_frames = None;
    let mut pregenerate_world = false;
    let mut valley = false;
    let mut advance = 0;
    let mut screenshot = None;
    let mut select = None;
    let mut args = std::env::args().skip(1);
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--config" => config_path = args.next().ok_or("--config requires a path")?,
            "--smoke-frames" => {
                let frames: u32 = args
                    .next()
                    .ok_or("--smoke-frames requires a number")?
                    .parse()?;
                if frames == 0 {
                    return Err("--smoke-frames must be greater than zero".into());
                }
                smoke_frames = Some(frames);
            }
            "--pregenerate-world" => pregenerate_world = true,
            "--valley" => valley = true,
            "--advance" => {
                advance = args
                    .next()
                    .ok_or("--advance requires a number of ticks")?
                    .parse()?;
            }
            "--select" => {
                select = Some(
                    args.next()
                        .ok_or("--select requires a person number")?
                        .parse()?,
                );
            }
            "--screenshot" => {
                screenshot = Some(PathBuf::from(
                    args.next().ok_or("--screenshot requires a path")?,
                ));
            }
            "--help" | "-h" => {
                println!(
                    "Usage: sim-viewer [--config PATH] [--smoke-frames NUMBER] [--pregenerate-world] [--valley] [--advance TICKS] [--select PERSON] [--screenshot PNG]"
                );
                std::process::exit(0);
            }
            _ => return Err(format!("unknown argument: {argument}").into()),
        }
    }
    if screenshot.is_some() && smoke_frames.is_none() {
        return Err("--screenshot needs --smoke-frames (it saves the last smoke frame)".into());
    }
    Ok(LaunchOptions {
        config_path,
        smoke_frames,
        pregenerate_world,
        valley,
        advance,
        screenshot,
        select,
    })
}

pub(super) fn load_world_archive(seed: u64, app: &AppConfig) -> Option<WorldArchive> {
    if !app.world_cache.enabled {
        return None;
    }
    let started = Instant::now();
    match WorldArchive::open(&app.world_cache.path, seed) {
        Ok(archive) => {
            let bytes = fs::metadata(&app.world_cache.path)
                .map(|metadata| metadata.len())
                .unwrap_or(0);
            println!(
                "loaded full-world archive index {} ({:.2} GiB) in {:.2?}",
                app.world_cache.path,
                bytes as f64 / 1024_f64.powi(3),
                started.elapsed()
            );
            Some(archive)
        }
        Err(error) => {
            eprintln!(
                "full-world archive {} was not loaded ({error}); deterministic generation remains available",
                app.world_cache.path
            );
            None
        }
    }
}
