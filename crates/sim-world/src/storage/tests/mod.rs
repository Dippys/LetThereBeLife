//! Unit tests for world storage, chunk residency, generation, and queries.

use std::collections::BTreeSet;

use super::*;
use crate::{
    chunk::{chunk_coord, chunk_origin},
    traversal::surface_traversal_cost,
    validation::ChunkSpan,
    worldgen::REGION_SIZE,
    *,
};

mod bootstrap;
mod expansion;
mod terrain;
