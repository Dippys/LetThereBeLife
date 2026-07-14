//! Renders a sampled overview of the generated world to a 24-bit BMP so the
//! generation pipeline can be inspected and calibrated visually.
//!
//! Usage: cargo run --release -p sim-core --example render_map -- \
//!     [--seed N] [--min-x N] [--min-y N] [--width N] [--height N] \
//!     [--step N] [--out PATH]

use std::io::Write as _;

use sim_core::{
    CHUNK_SIZE, ChunkCoord, ChunkGenerator, ChunkLocalPosition, GroundType, TerrainCell,
};

const DEFAULT_WIDTH: i64 = 4_096;
const DEFAULT_HEIGHT: i64 = 4_096;
const DEFAULT_STEP: i64 = 8;
const MAX_PIXELS: usize = 16_777_216;

#[derive(Debug)]
struct Options {
    seed: u64,
    min_x: i64,
    min_y: i64,
    width: i64,
    height: i64,
    step: i64,
    out: String,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let Some(options) = Options::parse(std::env::args().skip(1))? else {
        return Ok(());
    };
    let columns = usize::try_from(options.width / options.step)?;
    let rows = usize::try_from(options.height / options.step)?;
    let last_x = options.min_x + options.width - 1;
    let last_y = options.min_y + options.height - 1;
    let mut pixels = vec![[0_u8; 3]; columns * rows];
    let started = std::time::Instant::now();
    let chunk_min_x = options.min_x.div_euclid(CHUNK_SIZE);
    let chunk_min_y = options.min_y.div_euclid(CHUNK_SIZE);
    let chunk_max_x = last_x.div_euclid(CHUNK_SIZE);
    let chunk_max_y = last_y.div_euclid(CHUNK_SIZE);

    for chunk_y in chunk_min_y..=chunk_max_y {
        for chunk_x in chunk_min_x..=chunk_max_x {
            let coord = ChunkCoord {
                x: chunk_x,
                y: chunk_y,
            };
            let origin = coord.bounds()?.min;
            let sampler = ChunkGenerator::new(options.seed, coord)?;
            let start_x = align_up(origin.x.max(options.min_x), options.min_x, options.step)?;
            let start_y = align_up(origin.y.max(options.min_y), options.min_y, options.step)?;
            let end_x = (origin.x + CHUNK_SIZE - 1).min(last_x);
            let end_y = (origin.y + CHUNK_SIZE - 1).min(last_y);
            if start_x > end_x || start_y > end_y {
                continue;
            }
            for y in (start_y..=end_y).step_by(options.step as usize) {
                for x in (start_x..=end_x).step_by(options.step as usize) {
                    let local = ChunkLocalPosition {
                        x: (x - origin.x) as u8,
                        y: (y - origin.y) as u8,
                    };
                    let cell = sampler
                        .sample(local)
                        .ok_or("validated chunk-local coordinate became invalid")?
                        .terrain;
                    let column = usize::try_from((x - options.min_x) / options.step)?;
                    let row = usize::try_from((y - options.min_y) / options.step)?;
                    pixels[row * columns + column] = terrain_color(cell);
                }
            }
        }
    }
    println!(
        "rendered {columns}x{rows} samples (seed {}) in {:.2?}",
        options.seed,
        started.elapsed()
    );

    write_bmp(&options.out, columns, rows, &pixels)?;
    println!("wrote {}", options.out);
    Ok(())
}

impl Options {
    fn parse(args: impl Iterator<Item = String>) -> Result<Option<Self>, String> {
        let mut options = Self {
            seed: 1,
            min_x: 0,
            min_y: 0,
            width: DEFAULT_WIDTH,
            height: DEFAULT_HEIGHT,
            step: DEFAULT_STEP,
            out: "map.bmp".to_owned(),
        };
        let mut args = args;
        while let Some(argument) = args.next() {
            match argument.as_str() {
                "--seed" => options.seed = parse_value(next_value(&mut args, "--seed")?, "--seed")?,
                "--min-x" => {
                    options.min_x = parse_value(next_value(&mut args, "--min-x")?, "--min-x")?
                }
                "--min-y" => {
                    options.min_y = parse_value(next_value(&mut args, "--min-y")?, "--min-y")?
                }
                "--width" => {
                    options.width = parse_value(next_value(&mut args, "--width")?, "--width")?
                }
                "--height" => {
                    options.height = parse_value(next_value(&mut args, "--height")?, "--height")?
                }
                "--step" => options.step = parse_value(next_value(&mut args, "--step")?, "--step")?,
                "--out" => options.out = next_value(&mut args, "--out")?,
                "--help" | "-h" => {
                    print_usage();
                    return Ok(None);
                }
                _ => return Err(format!("unknown argument {argument}")),
            }
        }
        options.validate()?;
        Ok(Some(options))
    }

    fn validate(&self) -> Result<(), String> {
        if self.width <= 0 || self.height <= 0 || self.step <= 0 {
            return Err("--width, --height, and --step must be positive".to_owned());
        }
        if self.width.rem_euclid(self.step) != 0 || self.height.rem_euclid(self.step) != 0 {
            return Err("--width and --height must be exact multiples of --step".to_owned());
        }
        let last_x = self
            .min_x
            .checked_add(self.width - 1)
            .ok_or("--min-x plus --width exceeds i64 coordinates")?;
        let last_y = self
            .min_y
            .checked_add(self.height - 1)
            .ok_or("--min-y plus --height exceeds i64 coordinates")?;
        for coord in [
            ChunkCoord {
                x: self.min_x.div_euclid(CHUNK_SIZE),
                y: self.min_y.div_euclid(CHUNK_SIZE),
            },
            ChunkCoord {
                x: last_x.div_euclid(CHUNK_SIZE),
                y: last_y.div_euclid(CHUNK_SIZE),
            },
        ] {
            coord
                .bounds()
                .map_err(|error| format!("requested coordinates are not representable: {error}"))?;
        }
        let columns = usize::try_from(self.width / self.step)
            .map_err(|_| "sample width does not fit this platform".to_owned())?;
        let rows = usize::try_from(self.height / self.step)
            .map_err(|_| "sample height does not fit this platform".to_owned())?;
        let pixels = columns
            .checked_mul(rows)
            .ok_or("sampled image is too large for this platform")?;
        if pixels > MAX_PIXELS {
            return Err(format!(
                "sampled image has {pixels} pixels; maximum is {MAX_PIXELS}"
            ));
        }
        if self.out.is_empty() {
            return Err("--out must not be empty".to_owned());
        }
        Ok(())
    }
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
        "Usage: render_map [--seed N] [--min-x N] [--min-y N] [--width N] [--height N] [--step N] [--out PATH]"
    );
    println!(
        "Width and height must be positive multiples of step; the image is capped at {MAX_PIXELS} pixels."
    );
}

fn align_up(value: i64, origin: i64, step: i64) -> Result<i64, String> {
    let offset = (i128::from(value) - i128::from(origin)).rem_euclid(i128::from(step));
    let increment = (i128::from(step) - offset).rem_euclid(i128::from(step));
    value
        .checked_add(increment as i64)
        .ok_or("sample alignment exceeds i64 coordinates".to_owned())
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
    path: &str,
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
