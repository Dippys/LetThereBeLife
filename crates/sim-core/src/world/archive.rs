use std::{
    error::Error,
    fmt,
    fs::{self, File},
    io::{self, BufReader, BufWriter, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use rayon::prelude::*;

use super::*;

const MAGIC: &[u8; 8] = b"LTBLFULL";
const FORMAT_VERSION: u16 = 1;
const HEADER_BYTES: u64 = 112;
const INDEX_ENTRY_BYTES: u64 = 36;
const BAKE_BATCH_CHUNKS: usize = 512;
const CHECKSUM_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const CHECKSUM_PRIME: u64 = 0x0000_0100_0000_01b3;
const NO_TERRAIN_CLASS: u8 = u8::MAX;
const NO_FEATURE: u8 = u8::MAX;

#[derive(Debug)]
pub enum WorldArchiveError {
    Io(io::Error),
    Invalid(String),
    Poisoned,
}

impl fmt::Display for WorldArchiveError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => error.fmt(formatter),
            Self::Invalid(message) => write!(formatter, "invalid full-world archive: {message}"),
            Self::Poisoned => formatter.write_str("full-world archive reader lock is poisoned"),
        }
    }
}

impl Error for WorldArchiveError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Invalid(_) | Self::Poisoned => None,
        }
    }
}

impl From<io::Error> for WorldArchiveError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArchiveBakeProgress {
    pub completed_chunks: u32,
    pub total_chunks: u32,
    pub bytes_written: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArchiveBakeStats {
    pub chunks: u32,
    pub bytes: u64,
    pub elapsed: Duration,
}

/// Fixed 16-byte visual proxy for one complete 64 x 64 chunk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct ChunkOverview {
    base: TerrainCell,
    detail: TerrainCell,
    detail_bounds: [u8; 4],
    feature_count: u16,
    feature_kind: u8,
    reserved: u8,
}

impl ChunkOverview {
    pub const fn base(self) -> TerrainCell {
        self.base
    }

    pub fn detail(self) -> Option<TerrainCell> {
        (self.detail.classification().packed() != NO_TERRAIN_CLASS).then_some(self.detail)
    }

    pub fn detail_bounds(self, coord: ChunkCoord) -> Option<WorldRect> {
        self.detail()?;
        let origin = chunk_origin(coord);
        Some(WorldRect {
            min: WorldPosition {
                x: origin.x + i64::from(self.detail_bounds[0]),
                y: origin.y + i64::from(self.detail_bounds[1]),
            },
            max: WorldPosition {
                x: origin.x + i64::from(self.detail_bounds[2]),
                y: origin.y + i64::from(self.detail_bounds[3]),
            },
        })
    }

    pub fn feature(self) -> Option<(FeatureKind, u16)> {
        let kind = match self.feature_kind {
            0 => FeatureKind::Tree,
            1 => FeatureKind::Rock,
            2 => FeatureKind::BerryBush,
            _ => return None,
        };
        Some((kind, self.feature_count))
    }
}

const _: () = assert!(std::mem::size_of::<ChunkOverview>() == 16);

#[derive(Debug, Clone, Copy)]
struct ArchiveEntry {
    offset: u64,
    checksum: u64,
    length: u32,
}

const _: () = assert!(std::mem::size_of::<ArchiveEntry>() == 24);

#[derive(Debug)]
struct ArchiveShared {
    seed: u64,
    bounds: WorldRect,
    chunk_min: ChunkCoord,
    chunk_width: u32,
    chunk_height: u32,
    entries: Box<[ArchiveEntry]>,
    overviews: Box<[ChunkOverview]>,
    file: Mutex<File>,
}

#[derive(Debug, Clone)]
pub struct WorldArchive(Arc<ArchiveShared>);

#[derive(Debug, Clone)]
pub struct WorldOverview(Arc<ArchiveShared>);

impl WorldOverview {
    pub fn chunk_count_in(&self, bounds: WorldRect) -> usize {
        let Some(bounds) = bounds.intersection(self.0.bounds) else {
            return 0;
        };
        let min = ChunkCoord::from_world_position(bounds.min);
        let max = ChunkCoord::from_world_position(WorldPosition {
            x: bounds.max.x - 1,
            y: bounds.max.y - 1,
        });
        ((max.x - min.x + 1) * (max.y - min.y + 1)) as usize
    }

    pub fn visit_chunks_in(
        &self,
        bounds: WorldRect,
        mut visitor: impl FnMut(ChunkCoord, ChunkOverview),
    ) {
        let Some(bounds) = bounds.intersection(self.0.bounds) else {
            return;
        };
        let min = ChunkCoord::from_world_position(bounds.min);
        let inclusive_max = WorldPosition {
            x: bounds.max.x - 1,
            y: bounds.max.y - 1,
        };
        let max = ChunkCoord::from_world_position(inclusive_max);
        for y in min.y..=max.y {
            for x in min.x..=max.x {
                let coord = ChunkCoord { x, y };
                if let Some(index) = archive_index(&self.0, coord) {
                    visitor(coord, self.0.overviews[index]);
                }
            }
        }
    }

    pub fn bounds(&self) -> WorldRect {
        self.0.bounds
    }

    pub fn logical_bytes(&self) -> usize {
        self.0.overviews.len() * std::mem::size_of::<ChunkOverview>()
    }
}

impl WorldArchive {
    pub fn open(path: impl AsRef<Path>, expected_seed: u64) -> Result<Self, WorldArchiveError> {
        Self::open_with_coverage(path.as_ref(), expected_seed, true)
    }

    fn open_with_coverage(
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

fn bake(
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

#[derive(Clone, Copy)]
struct Header {
    seed: u64,
    bounds: WorldRect,
    chunk_min: ChunkCoord,
    chunk_width: u32,
    chunk_height: u32,
    chunk_count: u32,
    data_offset: u64,
    index_checksum: u64,
}

fn encode_header(header: Header) -> [u8; HEADER_BYTES as usize] {
    let mut bytes = [0_u8; HEADER_BYTES as usize];
    let mut cursor = 0;
    put(&mut bytes, &mut cursor, MAGIC);
    put(&mut bytes, &mut cursor, &FORMAT_VERSION.to_le_bytes());
    put(
        &mut bytes,
        &mut cursor,
        &WORLD_GENERATOR_VERSION.to_le_bytes(),
    );
    put(&mut bytes, &mut cursor, &header.seed.to_le_bytes());
    for value in [
        header.bounds.min.x,
        header.bounds.min.y,
        header.bounds.max.x,
        header.bounds.max.y,
        header.chunk_min.x,
        header.chunk_min.y,
    ] {
        put(&mut bytes, &mut cursor, &value.to_le_bytes());
    }
    for value in [header.chunk_width, header.chunk_height, header.chunk_count] {
        put(&mut bytes, &mut cursor, &value.to_le_bytes());
    }
    put(&mut bytes, &mut cursor, &header.data_offset.to_le_bytes());
    put(
        &mut bytes,
        &mut cursor,
        &header.index_checksum.to_le_bytes(),
    );
    let header_checksum = checksum(&bytes[..HEADER_BYTES as usize - 8]);
    bytes[HEADER_BYTES as usize - 8..].copy_from_slice(&header_checksum.to_le_bytes());
    bytes
}

fn read_header(reader: &mut impl Read) -> Result<Header, WorldArchiveError> {
    let mut bytes = [0_u8; HEADER_BYTES as usize];
    reader.read_exact(&mut bytes)?;
    let stored = u64::from_le_bytes(bytes[HEADER_BYTES as usize - 8..].try_into().unwrap());
    if checksum(&bytes[..HEADER_BYTES as usize - 8]) != stored {
        return Err(invalid("header checksum mismatch"));
    }
    let mut cursor = 0;
    if take::<8>(&bytes, &mut cursor) != *MAGIC {
        return Err(invalid("wrong file signature"));
    }
    let format = u16::from_le_bytes(take(&bytes, &mut cursor));
    let generator = u32::from_le_bytes(take(&bytes, &mut cursor));
    if format != FORMAT_VERSION || generator != WORLD_GENERATOR_VERSION {
        return Err(invalid(format!(
            "format/generator {format}/{generator}, expected {FORMAT_VERSION}/{WORLD_GENERATOR_VERSION}"
        )));
    }
    let seed = u64::from_le_bytes(take(&bytes, &mut cursor));
    let values: [i64; 6] = std::array::from_fn(|_| i64::from_le_bytes(take(&bytes, &mut cursor)));
    let chunk_width = u32::from_le_bytes(take(&bytes, &mut cursor));
    let chunk_height = u32::from_le_bytes(take(&bytes, &mut cursor));
    let chunk_count = u32::from_le_bytes(take(&bytes, &mut cursor));
    let data_offset = u64::from_le_bytes(take(&bytes, &mut cursor));
    let index_checksum = u64::from_le_bytes(take(&bytes, &mut cursor));
    if u64::from(chunk_width) * u64::from(chunk_height) != u64::from(chunk_count)
        || data_offset != HEADER_BYTES + u64::from(chunk_count) * INDEX_ENTRY_BYTES
    {
        return Err(invalid("inconsistent archive dimensions or offsets"));
    }
    Ok(Header {
        seed,
        bounds: WorldRect {
            min: WorldPosition {
                x: values[0],
                y: values[1],
            },
            max: WorldPosition {
                x: values[2],
                y: values[3],
            },
        },
        chunk_min: ChunkCoord {
            x: values[4],
            y: values[5],
        },
        chunk_width,
        chunk_height,
        chunk_count,
        data_offset,
        index_checksum,
    })
}

fn encode_index_entry(
    entry: ArchiveEntry,
    overview: ChunkOverview,
) -> [u8; INDEX_ENTRY_BYTES as usize] {
    let mut bytes = [0_u8; INDEX_ENTRY_BYTES as usize];
    bytes[0..8].copy_from_slice(&entry.offset.to_le_bytes());
    bytes[8..12].copy_from_slice(&entry.length.to_le_bytes());
    bytes[12..20].copy_from_slice(&entry.checksum.to_le_bytes());
    encode_overview(overview, &mut bytes[20..36]);
    bytes
}

fn decode_index_entry(
    bytes: &[u8; INDEX_ENTRY_BYTES as usize],
) -> Result<(ArchiveEntry, ChunkOverview), WorldArchiveError> {
    let length = u32::from_le_bytes(bytes[8..12].try_into().unwrap());
    let minimum = (CHUNK_SIZE * CHUNK_SIZE * 4 + 2) as u32;
    let maximum = minimum + (CHUNK_SIZE * CHUNK_SIZE * 3) as u32;
    if !(minimum..=maximum).contains(&length) {
        return Err(invalid("chunk payload length is outside encoded bounds"));
    }
    Ok((
        ArchiveEntry {
            offset: u64::from_le_bytes(bytes[0..8].try_into().unwrap()),
            length,
            checksum: u64::from_le_bytes(bytes[12..20].try_into().unwrap()),
        },
        decode_overview(&bytes[20..36])?,
    ))
}

fn summarize_chunk(chunk: &WorldChunk) -> ChunkOverview {
    let mut counts = [0_u16; 256];
    let mut representatives = [None; 256];
    let mut bounds = [[u8::MAX, u8::MAX, 0, 0]; 256];
    for (index, &cell) in chunk.terrain.iter().enumerate() {
        let class = cell.classification().packed() as usize;
        counts[class] = counts[class].saturating_add(1);
        representatives[class].get_or_insert(cell);
        let x = (index % CHUNK_SIZE as usize) as u8;
        let y = (index / CHUNK_SIZE as usize) as u8;
        bounds[class][0] = bounds[class][0].min(x);
        bounds[class][1] = bounds[class][1].min(y);
        bounds[class][2] = bounds[class][2].max(x + 1);
        bounds[class][3] = bounds[class][3].max(y + 1);
    }
    let base_index = counts
        .iter()
        .enumerate()
        .max_by_key(|&(index, count)| (*count, std::cmp::Reverse(index)))
        .map(|(index, _)| index)
        .unwrap();
    let detail_index = counts
        .iter()
        .enumerate()
        .filter(|&(index, count)| index != base_index && *count > 0)
        .max_by_key(|&(index, count)| {
            (
                overview_priority(index as u8),
                *count,
                std::cmp::Reverse(index),
            )
        })
        .map(|(index, _)| index);
    let mut feature_counts = [0_u16; 3];
    for feature in &chunk.features {
        feature_counts[feature.kind as usize] =
            feature_counts[feature.kind as usize].saturating_add(1);
    }
    let feature_kind = feature_counts
        .iter()
        .enumerate()
        .filter(|(_, count)| **count > 0)
        .max_by_key(|&(index, count)| (*count, std::cmp::Reverse(index)))
        .map_or(NO_FEATURE, |(index, _)| index as u8);
    ChunkOverview {
        base: representatives[base_index].unwrap(),
        detail: detail_index
            .and_then(|index| representatives[index])
            .unwrap_or(TerrainCell {
                elevation: 0,
                moisture: 0,
                class: TerrainClass(NO_TERRAIN_CLASS),
            }),
        detail_bounds: detail_index.map_or([0; 4], |index| bounds[index]),
        feature_count: feature_counts.iter().copied().sum(),
        feature_kind,
        reserved: 0,
    }
}

fn overview_priority(packed: u8) -> u8 {
    let Some(class) = TerrainClass::from_packed(packed) else {
        return 0;
    };
    match (class.surface(), class.biome()) {
        (_, BiomeType::River) => 6,
        (_, BiomeType::Lake) => 5,
        (SurfaceType::Sand, BiomeType::Beach) => 4,
        (SurfaceType::SnowIce, _) => 3,
        (SurfaceType::Rock | SurfaceType::Hill, _) => 2,
        _ => 1,
    }
}

fn encode_chunk(chunk: &WorldChunk) -> Vec<u8> {
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

fn encode_overview(overview: ChunkOverview, bytes: &mut [u8]) {
    encode_cell(overview.base, &mut bytes[0..4]);
    encode_cell(overview.detail, &mut bytes[4..8]);
    bytes[8..12].copy_from_slice(&overview.detail_bounds);
    bytes[12..14].copy_from_slice(&overview.feature_count.to_le_bytes());
    bytes[14] = overview.feature_kind;
    bytes[15] = 0;
}

fn decode_overview(bytes: &[u8]) -> Result<ChunkOverview, WorldArchiveError> {
    if bytes[15] != 0 {
        return Err(invalid("nonzero overview reserved byte"));
    }
    let base = decode_cell(&bytes[0..4])?;
    let detail_present = bytes[7] != NO_TERRAIN_CLASS;
    let detail = if !detail_present {
        TerrainCell {
            elevation: 0,
            moisture: 0,
            class: TerrainClass(NO_TERRAIN_CLASS),
        }
    } else {
        decode_cell(&bytes[4..8])?
    };
    if bytes[14] != NO_FEATURE && bytes[14] > 2 {
        return Err(invalid("invalid overview feature kind"));
    }
    let detail_bounds: [u8; 4] = bytes[8..12].try_into().unwrap();
    if detail_present
        && (detail_bounds[0] >= detail_bounds[2]
            || detail_bounds[1] >= detail_bounds[3]
            || detail_bounds[2] > CHUNK_SIZE as u8
            || detail_bounds[3] > CHUNK_SIZE as u8)
    {
        return Err(invalid("invalid overview detail bounds"));
    }
    if !detail_present && detail_bounds != [0; 4] {
        return Err(invalid("absent overview detail has nonempty bounds"));
    }
    let feature_count = u16::from_le_bytes(bytes[12..14].try_into().unwrap());
    if (bytes[14] == NO_FEATURE) != (feature_count == 0) {
        return Err(invalid("overview feature kind/count disagree"));
    }
    Ok(ChunkOverview {
        base,
        detail,
        detail_bounds,
        feature_count,
        feature_kind: bytes[14],
        reserved: 0,
    })
}

fn encode_cell(cell: TerrainCell, bytes: &mut [u8]) {
    bytes[0..2].copy_from_slice(&cell.elevation.to_le_bytes());
    bytes[2] = cell.moisture;
    bytes[3] = cell.classification().packed();
}

fn decode_cell(bytes: &[u8]) -> Result<TerrainCell, WorldArchiveError> {
    let class = TerrainClass::from_packed(bytes[3])
        .ok_or_else(|| invalid("invalid overview terrain class"))?;
    Ok(TerrainCell {
        elevation: u16::from_le_bytes(bytes[0..2].try_into().unwrap()),
        moisture: bytes[2],
        class,
    })
}

fn archive_index(shared: &ArchiveShared, coord: ChunkCoord) -> Option<usize> {
    let x = coord.x.checked_sub(shared.chunk_min.x)?;
    let y = coord.y.checked_sub(shared.chunk_min.y)?;
    if x < 0 || y < 0 || x >= i64::from(shared.chunk_width) || y >= i64::from(shared.chunk_height) {
        return None;
    }
    Some(y as usize * shared.chunk_width as usize + x as usize)
}

fn coord_for_index(min: ChunkCoord, width: u32, index: usize) -> ChunkCoord {
    ChunkCoord {
        x: min.x + (index % width as usize) as i64,
        y: min.y + (index / width as usize) as i64,
    }
}

fn put<const N: usize>(target: &mut [u8], cursor: &mut usize, bytes: &[u8; N]) {
    target[*cursor..*cursor + N].copy_from_slice(bytes);
    *cursor += N;
}

fn take<const N: usize>(source: &[u8], cursor: &mut usize) -> [u8; N] {
    let bytes = source[*cursor..*cursor + N].try_into().unwrap();
    *cursor += N;
    bytes
}

fn update_checksum(value: &mut u64, bytes: &[u8]) {
    for byte in bytes {
        *value ^= u64::from(*byte);
        *value = value.wrapping_mul(CHECKSUM_PRIME);
    }
}

fn checksum(bytes: &[u8]) -> u64 {
    let mut value = CHECKSUM_OFFSET;
    update_checksum(&mut value, bytes);
    value
}

fn invalid(message: impl Into<String>) -> WorldArchiveError {
    WorldArchiveError::Invalid(message.into())
}

fn temporary_path(path: &Path) -> PathBuf {
    let mut extension = path
        .extension()
        .map(|value| value.to_os_string())
        .unwrap_or_default();
    extension.push(".tmp");
    path.with_extension(extension)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::OpenOptions;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn path(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("ltbl-archive-{name}-{unique}.bin"))
    }

    fn test_bounds() -> WorldRect {
        WorldRect {
            min: WorldPosition { x: -64, y: -64 },
            max: WorldPosition { x: 64, y: 64 },
        }
    }

    fn synthetic_chunk(coord: ChunkCoord) -> WorldChunk {
        let origin = chunk_origin(coord);
        WorldChunk {
            coord,
            terrain: vec![
                TerrainCell {
                    elevation: 32_000,
                    moisture: 128,
                    class: TerrainClass::new(SurfaceType::Soil, BiomeType::Grassland),
                };
                (CHUNK_SIZE * CHUNK_SIZE) as usize
            ],
            features: vec![Feature {
                position: WorldPosition {
                    x: origin.x + 1,
                    y: origin.y + 1,
                },
                kind: FeatureKind::Tree,
            }],
        }
    }

    fn write_test_archive(seed: u64, output: &Path) -> Vec<WorldChunk> {
        let bounds = test_bounds();
        let chunk_min = ChunkCoord::from_world_position(bounds.min);
        let chunk_width = 2;
        let chunk_height = 2;
        let chunks: Vec<_> = (0..4)
            .map(|index| synthetic_chunk(coord_for_index(chunk_min, chunk_width, index)))
            .collect();
        let data_offset = HEADER_BYTES + INDEX_ENTRY_BYTES * chunks.len() as u64;
        let mut offset = data_offset;
        let mut records = Vec::new();
        let mut index_checksum = CHECKSUM_OFFSET;
        for chunk in &chunks {
            let payload = encode_chunk(chunk);
            let entry = ArchiveEntry {
                offset,
                checksum: checksum(&payload),
                length: payload.len() as u32,
            };
            let index = encode_index_entry(entry, summarize_chunk(chunk));
            update_checksum(&mut index_checksum, &index);
            records.push((index, payload));
            offset += u64::from(entry.length);
        }
        let header = encode_header(Header {
            seed,
            bounds,
            chunk_min,
            chunk_width,
            chunk_height,
            chunk_count: chunks.len() as u32,
            data_offset,
            index_checksum,
        });
        let mut writer = BufWriter::new(File::create(output).unwrap());
        writer.write_all(&header).unwrap();
        for (index, _) in &records {
            writer.write_all(index).unwrap();
        }
        for (_, payload) in &records {
            writer.write_all(payload).unwrap();
        }
        writer.flush().unwrap();
        chunks
    }

    #[test]
    fn small_archive_round_trips_exact_chunks_and_overviews() {
        let output = path("round-trip");
        let chunks = write_test_archive(7, &output);
        assert!(matches!(
            WorldArchive::open(&output, 7),
            Err(WorldArchiveError::Invalid(_))
        ));
        let archive = WorldArchive::open_with_coverage(&output, 7, false).unwrap();
        let request = ChunkLoadRequest {
            coord: ChunkCoord { x: -1, y: -1 },
            bounds: ChunkCoord { x: -1, y: -1 }.bounds().unwrap(),
            kind: ChunkLoadKind::Expansion,
        };
        let load = archive.load_chunk(request).unwrap();
        let expected = WorldChunkLoad {
            seed: 7,
            request,
            chunk: LoadedChunk::Expansion(chunks[0].clone()),
        };
        assert_eq!(load, expected);
        let mut seen = 0;
        archive
            .overview()
            .visit_chunks_in(test_bounds(), |_, _| seen += 1);
        assert_eq!(seen, 4);
        fs::remove_file(output).unwrap();
    }

    #[test]
    fn archive_rejects_wrong_seed_and_detects_corrupt_chunk_payload() {
        let output = path("validation");
        write_test_archive(7, &output);
        assert!(matches!(
            WorldArchive::open_with_coverage(&output, 8, false),
            Err(WorldArchiveError::Invalid(_))
        ));

        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&output)
            .unwrap();
        file.seek(SeekFrom::Start(HEADER_BYTES)).unwrap();
        let mut index = [0_u8; INDEX_ENTRY_BYTES as usize];
        file.read_exact(&mut index).unwrap();
        let (entry, _) = decode_index_entry(&index).unwrap();
        file.seek(SeekFrom::Start(entry.offset)).unwrap();
        let mut byte = [0_u8; 1];
        file.read_exact(&mut byte).unwrap();
        file.seek(SeekFrom::Start(entry.offset)).unwrap();
        file.write_all(&[byte[0] ^ 1]).unwrap();
        file.sync_all().unwrap();
        drop(file);

        let archive = WorldArchive::open_with_coverage(&output, 7, false).unwrap();
        let request = ChunkLoadRequest {
            coord: ChunkCoord { x: -1, y: -1 },
            bounds: ChunkCoord { x: -1, y: -1 }.bounds().unwrap(),
            kind: ChunkLoadKind::Expansion,
        };
        assert!(matches!(
            archive.load_chunk(request),
            Err(WorldArchiveError::Invalid(_))
        ));
        fs::remove_file(output).unwrap();
    }

    #[test]
    fn overview_layout_and_chunk_payload_are_compact() {
        assert_eq!(std::mem::size_of::<ChunkOverview>(), 16);
        let chunk = synthetic_chunk(ChunkCoord { x: 0, y: 0 });
        let payload = encode_chunk(&chunk);
        assert!(payload.len() < 17 * 1024);
    }
}
