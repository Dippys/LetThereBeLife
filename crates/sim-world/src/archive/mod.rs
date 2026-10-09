//! Full-world archive: a baked, checksummed file of every generated chunk plus
//! a compact per-chunk overview index used for distant rendering.
//!
//! Submodules split the on-disk format (`format`), baking (`writer`),
//! validated reading (`reader`), and overview summaries (`overview`).

use std::{
    error::Error,
    fmt,
    fs::File,
    io,
    sync::{Arc, Mutex},
    time::Duration,
};

use crate::{ChunkCoord, WorldRect};

mod format;
mod overview;
mod reader;
mod writer;

pub use overview::ChunkOverview;

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

fn archive_index(shared: &ArchiveShared, coord: ChunkCoord) -> Option<usize> {
    let x = coord.x.checked_sub(shared.chunk_min.x)?;
    let y = coord.y.checked_sub(shared.chunk_min.y)?;
    if x < 0 || y < 0 || x >= i64::from(shared.chunk_width) || y >= i64::from(shared.chunk_height) {
        return None;
    }
    Some(y as usize * shared.chunk_width as usize + x as usize)
}

#[cfg(test)]
mod tests;
