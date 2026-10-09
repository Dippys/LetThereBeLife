//! Archive round-trip, corruption detection, and layout tests.

use std::{
    fs::{self, File, OpenOptions},
    io::{BufWriter, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use super::{
    format::{
        Header, checksum, decode_index_entry, encode_header, encode_index_entry, update_checksum,
    },
    overview::summarize_chunk,
    writer::{coord_for_index, encode_chunk},
    *,
};
use crate::{
    BiomeType, CHUNK_SIZE, ChunkCoord, ChunkLoadRequest, Feature, FeatureKind, SurfaceType,
    TerrainCell, TerrainClass, WorldChunk, WorldChunkLoad, WorldPosition, WorldRect,
    chunk::chunk_origin,
    loads::{ChunkLoadKind, LoadedChunk},
};

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
