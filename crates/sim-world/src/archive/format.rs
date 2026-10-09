//! On-disk archive layout: header and index-entry encoding, byte cursors,
//! and the FNV-style checksum.

use std::io::Read;

use super::{
    ArchiveEntry, CHECKSUM_OFFSET, CHECKSUM_PRIME, ChunkOverview, FORMAT_VERSION, HEADER_BYTES,
    INDEX_ENTRY_BYTES, MAGIC, WorldArchiveError,
    overview::{decode_overview, encode_overview},
};
use crate::{CHUNK_SIZE, ChunkCoord, WORLD_GENERATOR_VERSION, WorldPosition, WorldRect};

#[derive(Clone, Copy)]
pub(super) struct Header {
    pub(super) seed: u64,
    pub(super) bounds: WorldRect,
    pub(super) chunk_min: ChunkCoord,
    pub(super) chunk_width: u32,
    pub(super) chunk_height: u32,
    pub(super) chunk_count: u32,
    pub(super) data_offset: u64,
    pub(super) index_checksum: u64,
}

pub(super) fn encode_header(header: Header) -> [u8; HEADER_BYTES as usize] {
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

pub(super) fn read_header(reader: &mut impl Read) -> Result<Header, WorldArchiveError> {
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

pub(super) fn encode_index_entry(
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

pub(super) fn decode_index_entry(
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

fn put<const N: usize>(target: &mut [u8], cursor: &mut usize, bytes: &[u8; N]) {
    target[*cursor..*cursor + N].copy_from_slice(bytes);
    *cursor += N;
}

fn take<const N: usize>(source: &[u8], cursor: &mut usize) -> [u8; N] {
    let bytes = source[*cursor..*cursor + N].try_into().unwrap();
    *cursor += N;
    bytes
}

pub(super) fn update_checksum(value: &mut u64, bytes: &[u8]) {
    for byte in bytes {
        *value ^= u64::from(*byte);
        *value = value.wrapping_mul(CHECKSUM_PRIME);
    }
}

pub(super) fn checksum(bytes: &[u8]) -> u64 {
    let mut value = CHECKSUM_OFFSET;
    update_checksum(&mut value, bytes);
    value
}

pub(super) fn invalid(message: impl Into<String>) -> WorldArchiveError {
    WorldArchiveError::Invalid(message.into())
}
