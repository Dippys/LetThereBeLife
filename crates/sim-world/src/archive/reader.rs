//! Opening and validating an archive, and loading individual chunks from it.

use std::{
    fs::File,
    io::{BufReader, Read, Seek, SeekFrom},
    path::Path,
    sync::{Arc, Mutex},
};

use super::{
    ArchiveBakeProgress, ArchiveBakeStats, ArchiveShared, CHECKSUM_OFFSET, INDEX_ENTRY_BYTES,
    WorldArchive, WorldArchiveError, WorldOverview, archive_index,
    format::{checksum, decode_index_entry, invalid, read_header, update_checksum},
    writer::bake,
};
use crate::{
    CHUNK_SIZE, ChunkCoord, ChunkLoadRequest, Feature, FeatureKind, TerrainCell, TerrainClass,
    WORLD_GENERATION_BOUNDS, WorldChunk, WorldChunkLoad, WorldPosition, WorldRect,
    chunk::chunk_origin,
    loads::{ChunkLoadKind, InitialChunk, LoadedChunk},
};

impl WorldArchive {
    pub fn open(path: impl AsRef<Path>, expected_seed: u64) -> Result<Self, WorldArchiveError> {
        Self::open_with_coverage(path.as_ref(), expected_seed, true)
    }

    pub(super) fn open_with_coverage(
        path: &Path,
        expected_seed: u64,
        require_full_world: bool,
    ) -> Result<Self, WorldArchiveError> {
        let file = File::open(path)?;
        let file_len = file.metadata()?.len();
        let mut reader = BufReader::new(file);
        let header = read_header(&mut reader)?;
        if header.seed != expected_seed {
            return Err(invalid(format!(
                "seed {}, expected {expected_seed}",
                header.seed
            )));
        }
        if require_full_world && header.bounds != WORLD_GENERATION_BOUNDS {
            return Err(invalid("archive does not cover the complete finite world"));
        }
        let expected_chunk_min = ChunkCoord::from_world_position(header.bounds.min);
        let expected_chunk_width = (header.bounds.max.x - header.bounds.min.x) / CHUNK_SIZE;
        let expected_chunk_height = (header.bounds.max.y - header.bounds.min.y) / CHUNK_SIZE;
        if header.chunk_min != expected_chunk_min
            || i64::from(header.chunk_width) != expected_chunk_width
            || i64::from(header.chunk_height) != expected_chunk_height
        {
            return Err(invalid(
                "chunk index dimensions do not match archive bounds",
            ));
        }
        if header.data_offset > file_len {
            return Err(invalid("data offset is beyond end of file"));
        }
        let mut entries = Vec::with_capacity(header.chunk_count as usize);
        let mut overviews = Vec::with_capacity(header.chunk_count as usize);
        let mut index_checksum = CHECKSUM_OFFSET;
        let mut previous_end = header.data_offset;
        for _ in 0..header.chunk_count {
            let mut bytes = [0_u8; INDEX_ENTRY_BYTES as usize];
            reader.read_exact(&mut bytes)?;
            update_checksum(&mut index_checksum, &bytes);
            let entry = decode_index_entry(&bytes)?;
            let end = entry
                .0
                .offset
                .checked_add(u64::from(entry.0.length))
                .ok_or_else(|| invalid("chunk payload offset overflow"))?;
            if entry.0.offset < previous_end || end > file_len {
                return Err(invalid(
                    "chunk payload offsets are overlapping or out of bounds",
                ));
            }
            previous_end = end;
            entries.push(entry.0);
            overviews.push(entry.1);
        }
        if previous_end != file_len {
            return Err(invalid("archive has trailing or unindexed payload bytes"));
        }
        if index_checksum != header.index_checksum {
            return Err(invalid("index checksum mismatch"));
        }
        let file = reader.into_inner();
        Ok(Self(Arc::new(ArchiveShared {
            seed: header.seed,
            bounds: header.bounds,
            chunk_min: header.chunk_min,
            chunk_width: header.chunk_width,
            chunk_height: header.chunk_height,
            entries: entries.into_boxed_slice(),
            overviews: overviews.into_boxed_slice(),
            file: Mutex::new(file),
        })))
    }

    pub fn overview(&self) -> WorldOverview {
        WorldOverview(Arc::clone(&self.0))
    }

    pub fn seed(&self) -> u64 {
        self.0.seed
    }

    pub fn load_chunk(
        &self,
        request: ChunkLoadRequest,
    ) -> Result<WorldChunkLoad, WorldArchiveError> {
        let index = archive_index(&self.0, request.coord)
            .ok_or_else(|| invalid(format!("chunk {:?} is outside the archive", request.coord)))?;
        let entry = self.0.entries[index];
        let mut bytes = vec![0_u8; entry.length as usize];
        let mut file = self
            .0
            .file
            .lock()
            .map_err(|_| WorldArchiveError::Poisoned)?;
        file.seek(SeekFrom::Start(entry.offset))?;
        file.read_exact(&mut bytes)?;
        drop(file);
        if checksum(&bytes) != entry.checksum {
            return Err(invalid(format!(
                "chunk {:?} checksum mismatch",
                request.coord
            )));
        }
        let chunk = decode_chunk(request.coord, &bytes)?;
        let loaded = match request.kind {
            ChunkLoadKind::Expansion => LoadedChunk::Expansion(chunk),
            ChunkLoadKind::Bootstrap => {
                LoadedChunk::Bootstrap(clip_initial_chunk(chunk, request.bounds)?)
            }
        };
        Ok(WorldChunkLoad {
            seed: self.0.seed,
            request,
            chunk: loaded,
        })
    }

    pub fn bake_full(
        seed: u64,
        path: impl AsRef<Path>,
        progress: impl FnMut(ArchiveBakeProgress),
    ) -> Result<ArchiveBakeStats, WorldArchiveError> {
        bake(seed, WORLD_GENERATION_BOUNDS, path.as_ref(), progress)
    }
}

fn decode_chunk(coord: ChunkCoord, bytes: &[u8]) -> Result<WorldChunk, WorldArchiveError> {
    let terrain_bytes = CHUNK_SIZE as usize * CHUNK_SIZE as usize * 4;
    if bytes.len() < terrain_bytes + 2 {
        return Err(invalid("truncated chunk payload"));
    }
    let mut terrain = Vec::with_capacity((CHUNK_SIZE * CHUNK_SIZE) as usize);
    for record in bytes[..terrain_bytes].chunks_exact(4) {
        let class = TerrainClass::from_packed(record[3])
            .ok_or_else(|| invalid(format!("invalid terrain class {:#04x}", record[3])))?;
        terrain.push(TerrainCell {
            elevation: u16::from_le_bytes([record[0], record[1]]),
            moisture: record[2],
            class,
        });
    }
    let feature_count =
        u16::from_le_bytes(bytes[terrain_bytes..terrain_bytes + 2].try_into().unwrap()) as usize;
    if bytes.len() != terrain_bytes + 2 + feature_count * 3 {
        return Err(invalid("inconsistent chunk feature count"));
    }
    let origin = chunk_origin(coord);
    let mut features = Vec::with_capacity(feature_count);
    let mut previous = None;
    for record in bytes[terrain_bytes + 2..].chunks_exact(3) {
        let kind = match record[2] {
            0 => FeatureKind::Tree,
            1 => FeatureKind::Rock,
            2 => FeatureKind::BerryBush,
            3 => FeatureKind::BitterBush,
            value => return Err(invalid(format!("invalid feature kind {value}"))),
        };
        let order = (record[1], record[0]);
        if previous.is_some_and(|last| last >= order) {
            return Err(invalid("chunk features are not strictly row-major"));
        }
        previous = Some(order);
        features.push(Feature {
            position: WorldPosition {
                x: origin.x + i64::from(record[0]),
                y: origin.y + i64::from(record[1]),
            },
            kind,
        });
    }
    Ok(WorldChunk {
        coord,
        terrain,
        features,
    })
}

fn clip_initial_chunk(
    chunk: WorldChunk,
    bounds: WorldRect,
) -> Result<InitialChunk, WorldArchiveError> {
    let full = chunk
        .coord
        .bounds()
        .map_err(|error| invalid(error.to_string()))?;
    if !full.contains_rect(bounds) {
        return Err(invalid("bootstrap request is outside archived chunk"));
    }
    let width = (bounds.max.x - bounds.min.x) as usize;
    let mut terrain = Vec::with_capacity(width * (bounds.max.y - bounds.min.y) as usize);
    for y in bounds.min.y..bounds.max.y {
        let local_y = (y - full.min.y) as usize;
        let start = local_y * CHUNK_SIZE as usize + (bounds.min.x - full.min.x) as usize;
        terrain.extend_from_slice(&chunk.terrain[start..start + width]);
    }
    let features = chunk
        .features
        .into_iter()
        .filter(|feature| bounds.contains(feature.position))
        .collect::<Vec<_>>()
        .into_boxed_slice();
    Ok(InitialChunk {
        bounds,
        terrain: terrain.into_boxed_slice(),
        features,
    })
}
