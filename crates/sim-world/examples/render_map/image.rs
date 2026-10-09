//! Terrain and feature colouring plus uncompressed 24-bit BMP output.

use super::*;

pub(super) fn surface_index(surface: SurfaceType) -> usize {
    match surface {
        SurfaceType::DeepWater => 0,
        SurfaceType::ShallowWater => 1,
        SurfaceType::Sand => 2,
        SurfaceType::Soil => 3,
        SurfaceType::Hill => 4,
        SurfaceType::Rock => 5,
        SurfaceType::SnowIce => 6,
    }
}

pub(super) fn biome_index(biome: BiomeType) -> usize {
    match biome {
        BiomeType::Ocean => 0,
        BiomeType::Lake => 1,
        BiomeType::River => 2,
        BiomeType::Beach => 3,
        BiomeType::Desert => 4,
        BiomeType::Grassland => 5,
        BiomeType::Savanna => 6,
        BiomeType::Forest => 7,
        BiomeType::Wetland => 8,
        BiomeType::Tundra => 9,
        BiomeType::Alpine => 10,
    }
}

pub(super) fn feature_index(feature: FeatureKind) -> usize {
    match feature {
        FeatureKind::Tree => 0,
        FeatureKind::Rock => 1,
        FeatureKind::BerryBush => 2,
    }
}

pub(super) fn sample_color(cell: GeneratedCell, show_features: bool) -> [u8; 3] {
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
    match (cell.surface(), cell.biome()) {
        (SurfaceType::DeepWater, BiomeType::Ocean) => rgb(16, 48 + shade, 94 + shade),
        (SurfaceType::ShallowWater, BiomeType::Ocean) => rgb(28, 84 + shade, 126 + shade),
        (SurfaceType::DeepWater, BiomeType::Lake) => rgb(24, 66 + shade, 112 + shade),
        (SurfaceType::ShallowWater, BiomeType::Lake) => rgb(40, 102 + shade, 142 + shade),
        (SurfaceType::DeepWater, BiomeType::River) => rgb(20, 74 + shade, 128 + shade),
        (SurfaceType::ShallowWater, BiomeType::River) => rgb(38, 116 + shade, 154 + shade),
        (SurfaceType::Sand, BiomeType::Beach) => rgb(210 + shade, 190 + shade, 126),
        (SurfaceType::Sand, BiomeType::Desert) => rgb(184 + shade, 150 + shade, 75),
        (SurfaceType::Soil, BiomeType::Grassland) => rgb(50 + shade, 112 + shade, 51),
        (SurfaceType::Soil, BiomeType::Savanna) => rgb(118 + shade, 126 + shade, 55),
        (SurfaceType::Soil, BiomeType::Forest) => rgb(37, 86 + shade, 39),
        (SurfaceType::Soil, BiomeType::Wetland) => rgb(48, 94 + shade, 74 + shade),
        (SurfaceType::Soil, BiomeType::Tundra) => rgb(105 + shade, 119 + shade, 105 + shade),
        (SurfaceType::Hill, _) => rgb(100 + shade, 108 + shade, 72),
        (SurfaceType::Rock, _) => rgb(125 + shade, 124 + shade, 119 + shade),
        (SurfaceType::SnowIce, _) => rgb(220 + shade, 229 + shade, 234 + shade),
        _ => rgb(255, 0, 255),
    }
}

pub(super) fn write_bmp(
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
