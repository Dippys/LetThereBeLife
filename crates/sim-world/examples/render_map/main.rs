//! Renders deterministic sampled world maps for visual inspection.
//!
//! A single view remains available for ad-hoc work:
//!
//! `cargo run --release -p sim-world --example render_map -- [view options]`
//!
//! The complete repeatable review set is produced with:
//!
//! `cargo run --release -p sim-world --example render_map -- --review-set`

use std::{
    fmt::Write as _,
    io::Write as _,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use sim_world::{
    BaseResource, BiomeType, CHUNK_SIZE, ChunkCoord, ChunkGenerator, ChunkLocalPosition,
    ClimateSample, Feature, FeatureKind, GeneratedCell, Material, PrevailingWind, SurfaceType,
    TerrainCell, TerrainClass, WORLD_GENERATION_BOUNDS, WorldPosition, WorldRect,
};

mod image;
mod reports;

use image::{biome_index, feature_index, sample_color, surface_index, write_bmp};
use reports::{
    detect_source_revision, distribution_report, representation_report, review_manifest,
    sanitize_field, seed_roles_report,
};

const DEFAULT_WIDTH: i64 = 4_096;

const DEFAULT_HEIGHT: i64 = 4_096;

const DEFAULT_STEP: i64 = 8;

const DEFAULT_REVIEW_DIRECTORY: &str = "target/world-quality";

const MAX_PIXELS: usize = 16_777_216;

const REVIEW_FORMAT_VERSION: u32 = 3;

const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;

const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

const REPRESENTATIVE_SEEDS: [SeedCase; 4] = [
    SeedCase {
        seed: 1,
        role: "repository baseline; continents, lakes, and rivers",
    },
    SeedCase {
        seed: 7,
        role: "mountain, desert, and forest contrast",
    },
    SeedCase {
        seed: 42,
        role: "archipelago and coastline contrast",
    },
    SeedCase {
        seed: 10_001,
        role: "separated continents, inland lakes, and relief",
    },
];

const FOCUSED_VIEWS: [ReviewView; 10] = [
    ReviewView::new("regional", 1, (-4_096, -4_096, 8_192, 8_192), 16, false),
    ReviewView::new(
        "drainage-seam-x",
        7,
        (-1_024, -2_048, 2_048, 4_096),
        4,
        false,
    ),
    ReviewView::new(
        "drainage-seam-y",
        42,
        (-2_048, -1_024, 4_096, 2_048),
        4,
        false,
    ),
    ReviewView::new("coastline", 1, (-2_048, 2_048, 4_096, 4_096), 8, false),
    ReviewView::new("river-mouth", 1, (-16_896, -17_152, 2_048, 2_048), 2, false),
    ReviewView::new(
        "river-source",
        1,
        (-14_592, -16_384, 2_048, 2_048),
        2,
        false,
    ),
    ReviewView::new("mountain", 7, (-16_384, -4_096, 8_192, 8_192), 16, false),
    ReviewView::new("forest-features", 42, (1_024, -512, 512, 512), 1, true),
    ReviewView::new("outcrop-features", 7, (-10_240, 2_048, 512, 512), 1, true),
    ReviewView::new("close-up", 1, (-25_088, 14_848, 512, 512), 1, true),
];

#[derive(Debug, Clone, Copy)]
struct SeedCase {
    seed: u64,
    role: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ReviewView {
    name: &'static str,
    seed: u64,
    bounds: WorldRect,
    step: i64,
    show_features: bool,
}

impl ReviewView {
    const fn new(
        name: &'static str,
        seed: u64,
        rectangle: (i64, i64, i64, i64),
        step: i64,
        show_features: bool,
    ) -> Self {
        let (min_x, min_y, width, height) = rectangle;
        Self {
            name,
            seed,
            bounds: WorldRect {
                min: WorldPosition { x: min_x, y: min_y },
                max: WorldPosition {
                    x: min_x + width,
                    y: min_y + height,
                },
            },
            step,
            show_features,
        }
    }

    const fn full_envelope(seed: u64) -> Self {
        Self {
            name: "full-envelope",
            seed,
            bounds: WORLD_GENERATION_BOUNDS,
            step: 128,
            show_features: false,
        }
    }

    fn validate(self) -> Result<(), String> {
        validate_view(self.bounds, self.step).map(|_| ())
    }

    fn dimensions(self) -> (usize, usize) {
        (
            ((self.bounds.max.x - self.bounds.min.x) / self.step) as usize,
            ((self.bounds.max.y - self.bounds.min.y) / self.step) as usize,
        )
    }
}

#[derive(Debug)]
struct SingleOptions {
    view: ReviewView,
    out: PathBuf,
}

#[derive(Debug)]
struct ReviewOptions {
    out_dir: PathBuf,
    source_revision: Option<String>,
}

#[derive(Debug)]
enum Mode {
    Single(SingleOptions),
    ReviewSet(ReviewOptions),
    Help,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SampleStats {
    surfaces: [u64; 7],
    biomes: [u64; 11],
    features: [u64; 4],
    samples: u64,
    sample_hash: u64,
}

impl Default for SampleStats {
    fn default() -> Self {
        Self {
            surfaces: [0; 7],
            biomes: [0; 11],
            features: [0; 4],
            samples: 0,
            sample_hash: FNV_OFFSET_BASIS,
        }
    }
}

impl SampleStats {
    fn record(&mut self, x: i64, y: i64, cell: GeneratedCell) {
        let surface = surface_index(cell.terrain.surface());
        let biome = biome_index(cell.terrain.biome());
        self.surfaces[surface] += 1;
        self.biomes[biome] += 1;
        if let Some(feature) = cell.feature {
            self.features[feature_index(feature)] += 1;
        }
        self.samples += 1;

        self.hash_bytes(&x.to_le_bytes());
        self.hash_bytes(&y.to_le_bytes());
        self.hash_bytes(&cell.terrain.elevation.to_le_bytes());
        self.hash_bytes(&[
            cell.terrain.moisture,
            cell.terrain.classification().packed(),
        ]);
        self.hash_bytes(&[cell
            .feature
            .map_or(0, |feature| feature_index(feature) as u8 + 1)]);
    }

    fn feature_count(&self) -> u64 {
        self.features.iter().sum()
    }

    fn feature_parts_per_million(&self) -> u64 {
        self.feature_count()
            .saturating_mul(1_000_000)
            .checked_div(self.samples)
            .unwrap_or(0)
    }

    fn hash_bytes(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.sample_hash ^= u64::from(byte);
            self.sample_hash = self.sample_hash.wrapping_mul(FNV_PRIME);
        }
    }
}

#[derive(Debug)]
struct RenderedView {
    view: ReviewView,
    relative_path: PathBuf,
    stats: SampleStats,
    elapsed: Duration,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    match parse_mode(std::env::args().skip(1))? {
        Mode::Single(options) => render_single(options)?,
        Mode::ReviewSet(options) => render_review_set(options)?,
        Mode::Help => print_usage(),
    }
    Ok(())
}

fn parse_mode(args: impl Iterator<Item = String>) -> Result<Mode, String> {
    let args: Vec<_> = args.collect();
    if args
        .iter()
        .any(|argument| argument == "--help" || argument == "-h")
    {
        return Ok(Mode::Help);
    }
    if let Some(position) = args.iter().position(|argument| argument == "--review-set") {
        let mut remaining = args;
        remaining.remove(position);
        return parse_review_options(remaining.into_iter()).map(Mode::ReviewSet);
    }
    parse_single_options(args.into_iter()).map(Mode::Single)
}

fn parse_single_options(mut args: impl Iterator<Item = String>) -> Result<SingleOptions, String> {
    let mut seed = 1;
    let mut min_x: i64 = 0;
    let mut min_y: i64 = 0;
    let mut width: i64 = DEFAULT_WIDTH;
    let mut height: i64 = DEFAULT_HEIGHT;
    let mut step: i64 = DEFAULT_STEP;
    let mut out = PathBuf::from("map.bmp");
    let mut show_features = false;
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--seed" => seed = parse_value(next_value(&mut args, "--seed")?, "--seed")?,
            "--min-x" => min_x = parse_value(next_value(&mut args, "--min-x")?, "--min-x")?,
            "--min-y" => min_y = parse_value(next_value(&mut args, "--min-y")?, "--min-y")?,
            "--width" => width = parse_value(next_value(&mut args, "--width")?, "--width")?,
            "--height" => height = parse_value(next_value(&mut args, "--height")?, "--height")?,
            "--step" => step = parse_value(next_value(&mut args, "--step")?, "--step")?,
            "--out" => out = PathBuf::from(next_value(&mut args, "--out")?),
            "--features" => show_features = true,
            _ => return Err(format!("unknown argument {argument}")),
        }
    }
    let max_x = min_x
        .checked_add(width)
        .ok_or("--min-x plus --width exceeds i64 coordinates")?;
    let max_y = min_y
        .checked_add(height)
        .ok_or("--min-y plus --height exceeds i64 coordinates")?;
    let view = ReviewView {
        name: "ad-hoc",
        seed,
        bounds: WorldRect {
            min: WorldPosition { x: min_x, y: min_y },
            max: WorldPosition { x: max_x, y: max_y },
        },
        step,
        show_features,
    };
    view.validate()?;
    if out.as_os_str().is_empty() {
        return Err("--out must not be empty".to_owned());
    }
    Ok(SingleOptions { view, out })
}

fn parse_review_options(mut args: impl Iterator<Item = String>) -> Result<ReviewOptions, String> {
    let mut out_dir = PathBuf::from(DEFAULT_REVIEW_DIRECTORY);
    let mut source_revision = None;
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--out-dir" => out_dir = PathBuf::from(next_value(&mut args, "--out-dir")?),
            "--source-revision" => {
                source_revision = Some(next_value(&mut args, "--source-revision")?)
            }
            _ => {
                return Err(format!(
                    "{argument} is not valid with --review-set; use --out-dir or --source-revision"
                ));
            }
        }
    }
    if out_dir.as_os_str().is_empty() {
        return Err("--out-dir must not be empty".to_owned());
    }
    if source_revision.as_deref().is_some_and(str::is_empty) {
        return Err("--source-revision must not be empty".to_owned());
    }
    Ok(ReviewOptions {
        out_dir,
        source_revision,
    })
}

fn render_single(options: SingleOptions) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(parent) = options
        .out
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)?;
    }
    let rendered = render_view(options.view, &options.out, PathBuf::from(&options.out))?;
    let (columns, rows) = options.view.dimensions();
    println!(
        "rendered {columns}x{rows} samples (seed {}) in {:.2?}; hash {:016x}",
        options.view.seed, rendered.elapsed, rendered.stats.sample_hash
    );
    println!("wrote {}", options.out.display());
    Ok(())
}

fn render_review_set(options: ReviewOptions) -> Result<(), Box<dyn std::error::Error>> {
    for view in review_views() {
        view.validate()?;
    }
    let detected_revision;
    let revision = if let Some(revision) = options.source_revision.as_deref() {
        revision
    } else {
        detected_revision = detect_source_revision();
        &detected_revision
    };
    let source_revision = sanitize_field(revision);
    std::fs::create_dir_all(&options.out_dir)?;

    let started = Instant::now();
    let mut rendered = Vec::with_capacity(REPRESENTATIVE_SEEDS.len() + FOCUSED_VIEWS.len());
    for view in review_views() {
        let relative_path = PathBuf::from(format!("seed-{}/{}.bmp", view.seed, view.name));
        let path = options.out_dir.join(&relative_path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let result = render_view(view, &path, relative_path)?;
        let (columns, rows) = view.dimensions();
        println!(
            "{:<17} seed {:>5}: {columns}x{rows} in {:>8.2?}, hash {:016x}",
            view.name, view.seed, result.elapsed, result.stats.sample_hash
        );
        rendered.push(result);
    }

    std::fs::write(
        options.out_dir.join("review_manifest.tsv"),
        review_manifest(&source_revision, &rendered),
    )?;
    std::fs::write(
        options.out_dir.join("distribution.tsv"),
        distribution_report(&rendered),
    )?;
    std::fs::write(
        options.out_dir.join("representation.tsv"),
        representation_report(),
    )?;
    std::fs::write(options.out_dir.join("seed_roles.tsv"), seed_roles_report())?;
    println!(
        "wrote {} review views and four reports under {} in {:.2?}",
        rendered.len(),
        options.out_dir.display(),
        started.elapsed()
    );
    Ok(())
}

fn review_views() -> impl Iterator<Item = ReviewView> {
    REPRESENTATIVE_SEEDS
        .into_iter()
        .map(|case| ReviewView::full_envelope(case.seed))
        .chain(FOCUSED_VIEWS)
}

fn render_view(
    view: ReviewView,
    path: &Path,
    relative_path: PathBuf,
) -> Result<RenderedView, Box<dyn std::error::Error>> {
    let (columns, rows) = view.dimensions();
    let mut pixels = vec![[0_u8; 3]; columns * rows];
    let mut stats = SampleStats::default();
    let started = Instant::now();
    let last_x = view.bounds.max.x - 1;
    let last_y = view.bounds.max.y - 1;
    let chunk_min_x = view.bounds.min.x.div_euclid(CHUNK_SIZE);
    let chunk_min_y = view.bounds.min.y.div_euclid(CHUNK_SIZE);
    let chunk_max_x = last_x.div_euclid(CHUNK_SIZE);
    let chunk_max_y = last_y.div_euclid(CHUNK_SIZE);

    for chunk_y in chunk_min_y..=chunk_max_y {
        for chunk_x in chunk_min_x..=chunk_max_x {
            let coord = ChunkCoord {
                x: chunk_x,
                y: chunk_y,
            };
            let origin = coord.bounds()?.min;
            let start_x = align_up(
                origin.x.max(view.bounds.min.x),
                view.bounds.min.x,
                view.step,
            )?;
            let start_y = align_up(
                origin.y.max(view.bounds.min.y),
                view.bounds.min.y,
                view.step,
            )?;
            let end_x = (origin.x + CHUNK_SIZE - 1).min(last_x);
            let end_y = (origin.y + CHUNK_SIZE - 1).min(last_y);
            if start_x > end_x || start_y > end_y {
                continue;
            }
            let sampler = ChunkGenerator::new(view.seed, coord)?;
            for y in (start_y..=end_y).step_by(view.step as usize) {
                for x in (start_x..=end_x).step_by(view.step as usize) {
                    let local = ChunkLocalPosition {
                        x: (x - origin.x) as u8,
                        y: (y - origin.y) as u8,
                    };
                    let cell = sampler
                        .sample(local)
                        .ok_or("validated chunk-local coordinate became invalid")?;
                    let column = usize::try_from((x - view.bounds.min.x) / view.step)?;
                    let row = usize::try_from((y - view.bounds.min.y) / view.step)?;
                    pixels[row * columns + column] = sample_color(cell, view.show_features);
                    stats.record(x, y, cell);
                }
            }
        }
    }
    write_bmp(path, columns, rows, &pixels)?;
    Ok(RenderedView {
        view,
        relative_path,
        stats,
        elapsed: started.elapsed(),
    })
}

fn validate_view(bounds: WorldRect, step: i64) -> Result<(usize, usize), String> {
    let width = bounds
        .max
        .x
        .checked_sub(bounds.min.x)
        .ok_or("view width exceeds i64 coordinates")?;
    let height = bounds
        .max
        .y
        .checked_sub(bounds.min.y)
        .ok_or("view height exceeds i64 coordinates")?;
    if width <= 0 || height <= 0 || step <= 0 {
        return Err("--width, --height, and --step must be positive".to_owned());
    }
    if width.rem_euclid(step) != 0 || height.rem_euclid(step) != 0 {
        return Err("--width and --height must be exact multiples of --step".to_owned());
    }
    if !WORLD_GENERATION_BOUNDS.contains_rect(bounds) {
        return Err(format!(
            "view must remain inside [{}, {}) on both axes",
            WORLD_GENERATION_BOUNDS.min.x, WORLD_GENERATION_BOUNDS.max.x
        ));
    }
    let columns = usize::try_from(width / step)
        .map_err(|_| "sample width does not fit this platform".to_owned())?;
    let rows = usize::try_from(height / step)
        .map_err(|_| "sample height does not fit this platform".to_owned())?;
    let pixels = columns
        .checked_mul(rows)
        .ok_or("sampled image is too large for this platform")?;
    if pixels > MAX_PIXELS {
        return Err(format!(
            "sampled image has {pixels} pixels; maximum is {MAX_PIXELS}"
        ));
    }
    Ok((columns, rows))
}

fn parse_value<T: std::str::FromStr>(value: String, argument: &str) -> Result<T, String> {
    value
        .parse()
        .map_err(|_| format!("{argument} requires a valid number"))
}

fn next_value(args: &mut impl Iterator<Item = String>, argument: &str) -> Result<String, String> {
    args.next()
        .ok_or_else(|| format!("{argument} requires a value"))
}

fn print_usage() {
    println!(
        "Usage:\n  render_map [--seed N] [--min-x N] [--min-y N] [--width N] [--height N] [--step N] [--features] [--out PATH]\n  render_map --review-set [--out-dir PATH] [--source-revision TEXT]"
    );
    println!(
        "Views must stay inside the complete world envelope; sampled images are capped at {MAX_PIXELS} pixels."
    );
}

fn align_up(value: i64, origin: i64, step: i64) -> Result<i64, String> {
    let offset = (i128::from(value) - i128::from(origin)).rem_euclid(i128::from(step));
    let increment = (i128::from(step) - offset).rem_euclid(i128::from(step));
    value
        .checked_add(increment as i64)
        .ok_or("sample alignment exceeds i64 coordinates".to_owned())
}

#[cfg(test)]
mod tests;
