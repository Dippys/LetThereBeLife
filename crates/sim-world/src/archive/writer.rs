//! Baking a full-world archive: parallel chunk generation, payload encoding,
//! index/header finalization, and atomic replacement of the output file.

use std::{
    fs::{self, File},
    io::{BufWriter, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    time::Instant,
};

use rayon::prelude::*;

use super::{
    ArchiveBakeProgress, ArchiveBakeStats, ArchiveEntry, BAKE_BATCH_CHUNKS, CHECKSUM_OFFSET,
    HEADER_BYTES, INDEX_ENTRY_BYTES, WorldArchiveError,
    format::{Header, checksum, encode_header, encode_index_entry, invalid, update_checksum},
    overview::summarize_chunk,
};
use crate::{
    CHUNK_SIZE, ChunkCoord, WORLD_GENERATION_BOUNDS, World, WorldChunk, WorldRect,
    chunk::chunk_origin,
};

pub(super) fn bake(
    seed: u64,
    bounds: WorldRect,
    path: &Path,
    mut progress: impl FnMut(ArchiveBakeProgress),
) -> Result<ArchiveBakeStats, WorldArchiveError> {
    let started = Instant::now();
    if !WORLD_GENERATION_BOUNDS.contains_rect(bounds)
        || bounds.min.x.rem_euclid(CHUNK_SIZE) != 0
        || bounds.min.y.rem_euclid(CHUNK_SIZE) != 0
        || bounds.max.x.rem_euclid(CHUNK_SIZE) != 0
        || bounds.max.y.rem_euclid(CHUNK_SIZE) != 0
        || bounds.max.x <= bounds.min.x
        || bounds.max.y <= bounds.min.y
    {
        return Err(invalid(
            "archive bounds must be a nonempty, full-chunk-aligned world rectangle",
        ));
    }
    let chunk_min = ChunkCoord::from_world_position(bounds.min);
    let chunk_width = ((bounds.max.x - bounds.min.x) / CHUNK_SIZE) as u32;
    let chunk_height = ((bounds.max.y - bounds.min.y) / CHUNK_SIZE) as u32;
    let chunk_count = chunk_width
        .checked_mul(chunk_height)
        .ok_or_else(|| invalid("too many archive chunks"))?;
    let index_bytes = u64::from(chunk_count) * INDEX_ENTRY_BYTES;
    let data_offset = HEADER_BYTES + index_bytes;
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)?;
    }
    let temporary = temporary_path(path);
    let file = File::create(&temporary)?;
    let mut writer = BufWriter::with_capacity(1024 * 1024, file);
    writer.write_all(&[0_u8; HEADER_BYTES as usize])?;
    let zero_index = [0_u8; 4096];
    let mut remaining = index_bytes;
    while remaining > 0 {
        let count = remaining.min(zero_index.len() as u64) as usize;
        writer.write_all(&zero_index[..count])?;
        remaining -= count as u64;
    }

    let mut entries = Vec::with_capacity(chunk_count as usize);
    let mut overviews = Vec::with_capacity(chunk_count as usize);
    let mut offset = data_offset;
    let mut start = 0_usize;
    while start < chunk_count as usize {
        let end = (start + BAKE_BATCH_CHUNKS).min(chunk_count as usize);
        let generate = |index| {
            let coord = coord_for_index(chunk_min, chunk_width, index);
            World::generate_chunk_at(seed, coord)
                .expect("validated full-world archive coordinates generate")
        };
        let chunks: Vec<_> = if end - start == BAKE_BATCH_CHUNKS {
            (start..end).into_par_iter().map(generate).collect()
        } else {
            (start..end).map(generate).collect()
        };
        for chunk in chunks {
            let overview = summarize_chunk(&chunk);
            let payload = encode_chunk(&chunk);
            let length = u32::try_from(payload.len())
                .map_err(|_| invalid("chunk payload exceeds u32 length"))?;
            writer.write_all(&payload)?;
            entries.push(ArchiveEntry {
                offset,
                checksum: checksum(&payload),
                length,
            });
            overviews.push(overview);
            offset += u64::from(length);
        }
        start = end;
        progress(ArchiveBakeProgress {
            completed_chunks: start as u32,
            total_chunks: chunk_count,
            bytes_written: offset,
        });
    }
    writer.flush()?;
    let mut file = writer.into_inner().map_err(|error| error.into_error())?;
    file.seek(SeekFrom::Start(HEADER_BYTES))?;
    let mut index_checksum = CHECKSUM_OFFSET;
    for (&entry, &overview) in entries.iter().zip(&overviews) {
        let bytes = encode_index_entry(entry, overview);
        update_checksum(&mut index_checksum, &bytes);
        file.write_all(&bytes)?;
    }
    let header = Header {
        seed,
        bounds,
        chunk_min,
        chunk_width,
        chunk_height,
        chunk_count,
        data_offset,
        index_checksum,
    };
    file.seek(SeekFrom::Start(0))?;
    file.write_all(&encode_header(header))?;
    file.sync_all()?;
    let bytes = file.metadata()?.len();
    drop(file);
    if path.exists() {
        fs::remove_file(path)?;
    }
    fs::rename(&temporary, path)?;
    Ok(ArchiveBakeStats {
        chunks: chunk_count,
        bytes,
        elapsed: started.elapsed(),
    })
}

pub(super) fn encode_chunk(chunk: &WorldChunk) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(chunk.terrain.len() * 4 + 2 + chunk.features.len() * 3);
    for cell in &chunk.terrain {
        bytes.extend_from_slice(&cell.elevation.to_le_bytes());
        bytes.push(cell.moisture);
        bytes.push(cell.classification().packed());
    }
    bytes.extend_from_slice(&(chunk.features.len() as u16).to_le_bytes());
    let origin = chunk_origin(chunk.coord);
    for feature in &chunk.features {
        bytes.push((feature.position.x - origin.x) as u8);
        bytes.push((feature.position.y - origin.y) as u8);
        bytes.push(feature.kind as u8);
    }
    bytes
}

pub(super) fn coord_for_index(min: ChunkCoord, width: u32, index: usize) -> ChunkCoord {
    ChunkCoord {
        x: min.x + (index % width as usize) as i64,
        y: min.y + (index / width as usize) as i64,
    }
}

fn temporary_path(path: &Path) -> PathBuf {
    let mut extension = path
        .extension()
        .map(|value| value.to_os_string())
        .unwrap_or_default();
    extension.push(".tmp");
    path.with_extension(extension)
}
