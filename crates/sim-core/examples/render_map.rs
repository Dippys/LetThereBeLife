//! Renders deterministic sampled world maps for visual inspection.
//!
//! A single view remains available for ad-hoc work:
//!
//! `cargo run --release -p sim-core --example render_map -- [view options]`
//!
//! The complete repeatable review set is produced with:
//!
//! `cargo run --release -p sim-core --example render_map -- --review-set`

use std::{
    fmt::Write as _,
    io::Write as _,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use sim_core::{
    CHUNK_SIZE, ChunkCoord, ChunkGenerator, ChunkLocalPosition, ClimateSample, Feature,
    FeatureKind, GeneratedCell, GroundType, PrevailingWind, TerrainCell, WORLD_GENERATION_BOUNDS,
    WorldPosition, WorldRect,
};

const DEFAULT_WIDTH: i64 = 4_096;
const DEFAULT_HEIGHT: i64 = 4_096;
const DEFAULT_STEP: i64 = 8;
const DEFAULT_REVIEW_DIRECTORY: &str = "target/world-quality";
const MAX_PIXELS: usize = 16_777_216;
const REVIEW_FORMAT_VERSION: u32 = 1;
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

const FOCUSED_VIEWS: [ReviewView; 8] = [
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
    ReviewView::new("river-mouth", 42, (14_080, 16_640, 2_048, 2_048), 2, false),
    ReviewView::new("lake", 1, (2_048, -4_096, 2_048, 2_048), 4, false),
    ReviewView::new("mountain", 7, (-16_384, -4_096, 8_192, 8_192), 16, false),
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
    terrain: [u64; 7],
    features: [u64; 3],
    samples: u64,
    sample_hash: u64,
}

impl Default for SampleStats {
    fn default() -> Self {
        Self {
            terrain: [0; 7],
            features: [0; 3],
            samples: 0,
            sample_hash: FNV_OFFSET_BASIS,
        }
    }
}

impl SampleStats {
    fn record(&mut self, x: i64, y: i64, cell: GeneratedCell) {
        let ground = ground_index(cell.terrain.ground);
        self.terrain[ground] += 1;
        if let Some(feature) = cell.feature {
            self.features[feature_index(feature)] += 1;
        }
        self.samples += 1;

        self.hash_bytes(&x.to_le_bytes());
        self.hash_bytes(&y.to_le_bytes());
        self.hash_bytes(&cell.terrain.elevation.to_le_bytes());
        self.hash_bytes(&[cell.terrain.moisture, ground as u8]);
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

fn review_manifest(source_revision: &str, rendered: &[RenderedView]) -> String {
    let mut report = "review_format\tsource_revision\tview\tseed\tmin_x\tmin_y\tmax_x\tmax_y\tstep\tcolumns\trows\tfeatures\tsample_hash\tpath\n".to_owned();
    for output in rendered {
        let view = output.view;
        let (columns, rows) = view.dimensions();
        writeln!(
            report,
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{:016x}\t{}",
            REVIEW_FORMAT_VERSION,
            source_revision,
            view.name,
            view.seed,
            view.bounds.min.x,
            view.bounds.min.y,
            view.bounds.max.x,
            view.bounds.max.y,
            view.step,
            columns,
            rows,
            view.show_features,
            output.stats.sample_hash,
            output.relative_path.display(),
        )
        .expect("writing to a String cannot fail");
    }
    report
}

fn distribution_report(rendered: &[RenderedView]) -> String {
    let mut report = String::from(
        "view\tseed\tsamples\tdeep_water\tshallow_water\tsand\tgrass\tforest_floor\thill\tbare_rock\ttrees\trocks\tberry_bushes\tfeatures\tfeature_ppm\tsample_hash\n",
    );
    for output in rendered {
        let terrain = &output.stats.terrain;
        let features = &output.stats.features;
        writeln!(
            report,
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{:016x}",
            output.view.name,
            output.view.seed,
            output.stats.samples,
            terrain[0],
            terrain[1],
            terrain[2],
            terrain[3],
            terrain[4],
            terrain[5],
            terrain[6],
            features[0],
            features[1],
            features[2],
            output.stats.feature_count(),
            output.stats.feature_parts_per_million(),
            output.stats.sample_hash,
        )
        .expect("writing to a String cannot fail");
    }
    report
}

fn representation_report() -> String {
    let mut report = String::from("type\tsize_bytes\talign_bytes\n");
    for (name, size, align) in [
        type_layout::<GroundType>("GroundType"),
        type_layout::<TerrainCell>("TerrainCell"),
        type_layout::<PrevailingWind>("PrevailingWind"),
        type_layout::<ClimateSample>("ClimateSample"),
        type_layout::<FeatureKind>("FeatureKind"),
        type_layout::<Feature>("Feature"),
        type_layout::<GeneratedCell>("GeneratedCell"),
        type_layout::<ChunkCoord>("ChunkCoord"),
    ] {
        writeln!(report, "{name}\t{size}\t{align}").expect("writing to a String cannot fail");
    }
    report
}

fn type_layout<T>(name: &'static str) -> (&'static str, usize, usize) {
    (name, std::mem::size_of::<T>(), std::mem::align_of::<T>())
}

fn seed_roles_report() -> String {
    let mut report = String::from("seed\trole\n");
    for case in REPRESENTATIVE_SEEDS {
        writeln!(report, "{}\t{}", case.seed, case.role).expect("writing to a String cannot fail");
    }
    report
}

fn detect_source_revision() -> String {
    let revision = std::process::Command::new("git")
        .args(["rev-parse", "--verify", "HEAD"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|revision| revision.trim().to_owned())
        .filter(|revision| !revision.is_empty())
        .unwrap_or_else(|| "unavailable".to_owned());
    let dirty = std::process::Command::new("git")
        .args(["status", "--porcelain", "--untracked-files=no"])
        .output()
        .ok()
        .is_some_and(|output| output.status.success() && !output.stdout.is_empty());
    if dirty {
        format!("{revision}+dirty")
    } else {
        revision
    }
}

fn sanitize_field(value: &str) -> String {
    value
        .chars()
        .map(|character| match character {
            '\t' | '\r' | '\n' => ' ',
            other => other,
        })
        .collect()
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

fn ground_index(ground: GroundType) -> usize {
    match ground {
        GroundType::DeepWater => 0,
        GroundType::ShallowWater => 1,
        GroundType::Sand => 2,
        GroundType::Grass => 3,
        GroundType::ForestFloor => 4,
        GroundType::Hill => 5,
        GroundType::BareRock => 6,
    }
}

fn feature_index(feature: FeatureKind) -> usize {
    match feature {
        FeatureKind::Tree => 0,
        FeatureKind::Rock => 1,
        FeatureKind::BerryBush => 2,
    }
}

fn sample_color(cell: GeneratedCell, show_features: bool) -> [u8; 3] {
    if show_features && let Some(feature) = cell.feature {
        return match feature {
            FeatureKind::Tree => [17, 48, 24],
            FeatureKind::Rock => [84, 82, 78],
            FeatureKind::BerryBush => [164, 35, 77],
        };
    }
    terrain_color(cell.terrain)
}

/// Mirrors the viewer palette in crates/sim-viewer/src/renderer.rs.
fn terrain_color(cell: TerrainCell) -> [u8; 3] {
    let shade = (cell.elevation >> 12) as u8;
    let rgb = |r: u8, g: u8, b: u8| [r, g, b];
    match cell.ground {
        GroundType::DeepWater => rgb(16, 48 + shade, 94 + shade),
        GroundType::ShallowWater => rgb(28, 84 + shade, 126 + shade),
        GroundType::Sand => rgb(184 + shade, 166 + shade, 105),
        GroundType::Grass => rgb(50 + shade, 112 + shade, 51),
        GroundType::ForestFloor => rgb(37, 86 + shade, 39),
        GroundType::Hill => rgb(100 + shade, 108 + shade, 72),
        GroundType::BareRock => rgb(125 + shade, 124 + shade, 119 + shade),
    }
}

fn write_bmp(
    path: &Path,
    width: usize,
    height: usize,
    pixels: &[[u8; 3]],
) -> Result<(), std::io::Error> {
    let invalid = |message| std::io::Error::new(std::io::ErrorKind::InvalidInput, message);
    let row_bytes = width
        .checked_mul(3)
        .ok_or_else(|| invalid("BMP row too wide"))?;
    let padding = (4 - row_bytes % 4) % 4;
    let data_size = row_bytes
        .checked_add(padding)
        .and_then(|row_size| row_size.checked_mul(height))
        .ok_or_else(|| invalid("BMP data is too large"))?;
    let file_size = 54_usize
        .checked_add(data_size)
        .ok_or_else(|| invalid("BMP file is too large"))?;
    let width = i32::try_from(width).map_err(|_| invalid("BMP width exceeds i32"))?;
    let height = i32::try_from(height).map_err(|_| invalid("BMP height exceeds i32"))?;
    let file_size = u32::try_from(file_size).map_err(|_| invalid("BMP file exceeds u32"))?;
    let mut file = std::io::BufWriter::new(std::fs::File::create(path)?);
    file.write_all(b"BM")?;
    file.write_all(&file_size.to_le_bytes())?;
    file.write_all(&[0; 4])?;
    file.write_all(&54_u32.to_le_bytes())?;
    file.write_all(&40_u32.to_le_bytes())?;
    file.write_all(&width.to_le_bytes())?;
    file.write_all(&height.to_le_bytes())?;
    file.write_all(&1_u16.to_le_bytes())?;
    file.write_all(&24_u16.to_le_bytes())?;
    file.write_all(&[0; 24])?;
    for row in (0..height as usize).rev() {
        for pixel in &pixels[row * width as usize..(row + 1) * width as usize] {
            file.write_all(&[pixel[2], pixel[1], pixel[0]])?;
        }
        file.write_all(&[0, 0, 0, 0][..padding])?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn view_validation_enforces_world_bounds_and_pixel_budget() {
        assert!(validate_view(WORLD_GENERATION_BOUNDS, 128).is_ok());
        assert!(
            validate_view(
                WorldRect {
                    min: WorldPosition {
                        x: WORLD_GENERATION_BOUNDS.min.x - 1,
                        y: 0,
                    },
                    max: WorldPosition { x: 64, y: 64 },
                },
                1,
            )
            .unwrap_err()
            .contains("must remain inside")
        );
        assert!(
            validate_view(
                WorldRect {
                    min: WorldPosition { x: 0, y: 0 },
                    max: WorldPosition { x: 4_097, y: 4_097 },
                },
                1,
            )
            .unwrap_err()
            .contains("maximum")
        );
    }

    #[test]
    fn ad_hoc_view_rejects_coordinate_overflow() {
        let error = parse_single_options(
            [
                "--min-x".to_owned(),
                i64::MAX.to_string(),
                "--width".to_owned(),
                "1".to_owned(),
            ]
            .into_iter(),
        )
        .unwrap_err();
        assert_eq!(error, "--min-x plus --width exceeds i64 coordinates");
    }

    #[test]
    fn sampling_alignment_is_relative_to_the_view_origin_across_signed_chunks() {
        let origin = -1_003;
        let step = 7;
        for value in [-1_100, -1_024, -1_003, -1_000, -960, -1] {
            let aligned = align_up(value, origin, step).unwrap();
            assert!(aligned >= value);
            assert_eq!((aligned - origin).rem_euclid(step), 0);
            assert!(aligned - value < step);
        }
        assert_eq!(align_up(-1_024, origin, step), Ok(-1_024));
    }

    #[test]
    fn canonical_views_cover_both_drainage_seam_axes() {
        let seam_x = FOCUSED_VIEWS
            .iter()
            .find(|view| view.name == "drainage-seam-x")
            .unwrap();
        let seam_y = FOCUSED_VIEWS
            .iter()
            .find(|view| view.name == "drainage-seam-y")
            .unwrap();
        assert!(seam_x.bounds.min.x < 0 && seam_x.bounds.max.x > 0);
        assert!(seam_y.bounds.min.y < 0 && seam_y.bounds.max.y > 0);
        assert!(
            FOCUSED_VIEWS
                .into_iter()
                .all(|view| view.validate().is_ok())
        );
    }

    #[test]
    fn representative_seed_contract_is_multi_seed_and_keeps_repository_seed() {
        assert_eq!(REPRESENTATIVE_SEEDS[0].seed, 1);
        assert!(REPRESENTATIVE_SEEDS.len() >= 4);
        let unique: std::collections::BTreeSet<_> = REPRESENTATIVE_SEEDS
            .into_iter()
            .map(|case| case.seed)
            .collect();
        assert_eq!(unique.len(), REPRESENTATIVE_SEEDS.len());
    }

    #[test]
    fn output_metadata_is_byte_stable_for_equal_inputs() {
        let output = RenderedView {
            view: ReviewView::new("fixture", 7, (-64, -32, 128, 64), 4, true),
            relative_path: PathBuf::from("seed-7/fixture.bmp"),
            stats: SampleStats {
                terrain: [1, 2, 3, 4, 5, 6, 7],
                features: [8, 9, 10],
                samples: 28,
                sample_hash: 0x1234_5678_9abc_def0,
            },
            elapsed: Duration::from_secs(99),
        };
        let left = review_manifest("abc123+dirty", std::slice::from_ref(&output));
        let right = review_manifest("abc123+dirty", &[output]);
        assert_eq!(left, right);
        assert_eq!(
            left,
            "review_format\tsource_revision\tview\tseed\tmin_x\tmin_y\tmax_x\tmax_y\tstep\tcolumns\trows\tfeatures\tsample_hash\tpath\n1\tabc123+dirty\tfixture\t7\t-64\t-32\t64\t32\t4\t32\t16\ttrue\t123456789abcdef0\tseed-7/fixture.bmp\n"
        );
    }

    #[test]
    fn semantic_sample_hash_is_deterministic_and_coordinate_sensitive() {
        let cell = GeneratedCell {
            terrain: TerrainCell {
                elevation: 42_000,
                moisture: 127,
                ground: GroundType::Grass,
            },
            feature: Some(FeatureKind::BerryBush),
        };
        let mut left = SampleStats::default();
        let mut right = SampleStats::default();
        left.record(-1, 2, cell);
        right.record(-1, 2, cell);
        assert_eq!(left, right);
        right.record(0, 2, cell);
        assert_ne!(left.sample_hash, right.sample_hash);
    }
}
