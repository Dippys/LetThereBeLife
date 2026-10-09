//! Chunk materialization: bootstrap and expansion requests, payload
//! generation, and atomic insertion with capacity accounting.

use std::collections::BTreeSet;

use super::World;
use crate::{
    ChunkCoord, ChunkLoadRequest, GenerateAreaError, MAX_CHUNKS_PER_GENERATION,
    MAX_GENERATED_CHUNKS, WORLD_GENERATION_BOUNDS, WorldChunk, WorldChunkLoad, WorldRect,
    generator::generate_chunk,
    geometry::intersection,
    loads::{ChunkLoadKind, InitialChunk, LoadedChunk},
    validation::{
        validate_chunk_axis, validate_chunk_span, validate_full_chunk_request,
        validate_world_bounds,
    },
};

impl World {
    pub(super) fn materialize_initial_area_in_batches(
        &mut self,
        batch_size: usize,
    ) -> Result<(), GenerateAreaError> {
        assert!(
            batch_size > 0,
            "bootstrap batches must contain at least one chunk"
        );
        let bounds = self.initial_bounds();
        let span = validate_chunk_span(bounds)?;
        let mut loads = Vec::with_capacity(batch_size);
        for coord in span.coords() {
            let request = self.load_request_for(bounds, coord)?;
            if self
                .chunks
                .get(&coord)
                .is_some_and(|chunk| chunk.bounds(coord).contains_rect(request.bounds))
            {
                continue;
            }
            loads.push(Self::generate_chunk_load(self.seed, request));
            if loads.len() == batch_size {
                self.insert_chunk_loads(std::mem::take(&mut loads))?;
            }
        }
        if !loads.is_empty() {
            self.insert_chunk_loads(loads)?;
        }
        Ok(())
    }

    pub fn generate_area(&mut self, bounds: WorldRect) -> Result<(), GenerateAreaError> {
        if bounds == self.initial_bounds() {
            return self.materialize_initial_area();
        }
        let loads = self
            .missing_chunk_load_requests(bounds)?
            .into_iter()
            .map(|request| Self::generate_chunk_load(self.seed, request))
            .collect();
        self.insert_chunk_loads(loads)?;
        Ok(())
    }

    pub fn generate_chunks(
        seed: u64,
        bounds: WorldRect,
    ) -> Result<Vec<WorldChunk>, GenerateAreaError> {
        let span = validate_full_chunk_request(bounds)?;
        let mut chunks = Vec::with_capacity(span.total as usize);
        chunks.extend(span.coords().map(|coord| generate_chunk(seed, coord)));
        Ok(chunks)
    }

    pub fn generate_chunks_streaming(
        seed: u64,
        bounds: WorldRect,
    ) -> Result<impl Iterator<Item = WorldChunk>, GenerateAreaError> {
        let span = validate_full_chunk_request(bounds)?;
        Ok(span.coords().map(move |coord| generate_chunk(seed, coord)))
    }

    pub fn generate_chunk_at(
        seed: u64,
        coord: ChunkCoord,
    ) -> Result<WorldChunk, GenerateAreaError> {
        validate_chunk_axis(coord.x)?;
        validate_chunk_axis(coord.y)?;
        validate_world_bounds(coord.bounds()?)?;
        Ok(generate_chunk(seed, coord))
    }

    /// Generates one opaque materialization payload for a validated request.
    pub fn generate_chunk_load(seed: u64, request: ChunkLoadRequest) -> WorldChunkLoad {
        let chunk = match request.kind {
            ChunkLoadKind::Bootstrap => {
                LoadedChunk::Bootstrap(InitialChunk::generate(seed, request.coord, request.bounds))
            }
            ChunkLoadKind::Expansion => LoadedChunk::Expansion(generate_chunk(seed, request.coord)),
        };
        WorldChunkLoad {
            seed,
            request,
            chunk,
        }
    }

    /// Prepares shared deterministic regional inputs for a bounded request
    /// window. This is a performance hint only and does not materialize world
    /// state or change generated output.
    pub fn prepare_chunk_loads(seed: u64, requests: &[ChunkLoadRequest]) {
        crate::worldgen::prepare_chunk_regions(seed, requests);
    }

    /// Inserts full expansion chunks for callers that intentionally need whole
    /// chunk payloads. Viewer/bootstrap work should use [`Self::insert_chunk_loads`].
    pub fn insert_chunks(&mut self, chunks: Vec<WorldChunk>) -> Result<usize, GenerateAreaError> {
        let loads = chunks
            .into_iter()
            .map(|chunk| {
                let coord = chunk.coord();
                let bounds = coord
                    .bounds()
                    .expect("full chunks accepted by this API have representable bounds");
                WorldChunkLoad {
                    seed: self.seed,
                    request: ChunkLoadRequest {
                        coord,
                        bounds,
                        kind: ChunkLoadKind::Expansion,
                    },
                    chunk: LoadedChunk::Expansion(chunk),
                }
            })
            .collect();
        self.insert_chunk_loads(loads)
    }

    /// Applies worker-generated bootstrap or expansion payloads atomically with
    /// respect to retained expansion capacity.
    pub fn insert_chunk_loads(
        &mut self,
        loads: Vec<WorldChunkLoad>,
    ) -> Result<usize, GenerateAreaError> {
        let mut new_expansions = BTreeSet::new();
        for load in &loads {
            if load.seed != self.seed {
                return Err(GenerateAreaError::SeedMismatch {
                    expected: self.seed,
                    received: load.seed,
                });
            }
            self.validate_chunk_load_request(load.request)?;
            let coord = load.coord();
            let covered = self
                .chunks
                .get(&coord)
                .is_some_and(|existing| existing.bounds(coord).contains_rect(load.bounds()));
            if !covered
                && self.request_consumes_expansion_capacity(load.request)
                && !self
                    .chunks
                    .get(&coord)
                    .is_some_and(LoadedChunk::is_expansion)
            {
                new_expansions.insert(coord);
            }
        }
        self.ensure_chunk_capacity(new_expansions.len())?;

        let mut inserted = 0;
        for load in loads {
            let coord = load.coord();
            let target = load.bounds();
            let replace = match self.chunks.get(&coord) {
                None => true,
                Some(existing) => {
                    let existing_bounds = existing.bounds(coord);
                    !existing_bounds.contains_rect(target) && target.contains_rect(existing_bounds)
                }
            };
            if replace {
                self.chunks.insert(coord, load.chunk);
                inserted += 1;
            }
        }
        if inserted > 0 {
            self.revision = self.revision.saturating_add(1);
        }
        Ok(inserted)
    }

    /// Validates a world-aware request and returns its missing chunk count.
    pub fn validate_generation_request(&self, bounds: WorldRect) -> Result<u64, GenerateAreaError> {
        self.missing_chunk_load_requests(bounds)
            .map(|requests| requests.len() as u64)
    }

    pub fn missing_chunk_coords(
        &self,
        bounds: WorldRect,
    ) -> Result<Vec<ChunkCoord>, GenerateAreaError> {
        self.missing_chunk_load_requests(bounds)
            .map(|requests| requests.into_iter().map(ChunkLoadRequest::coord).collect())
    }

    /// Returns bounded, authoritative load requests for the uncovered portion
    /// of `bounds`. Requests remain region-major so repeated generation reuses
    /// the bounded regional hydrology cache.
    pub fn missing_chunk_load_requests(
        &self,
        bounds: WorldRect,
    ) -> Result<Vec<ChunkLoadRequest>, GenerateAreaError> {
        let span = validate_chunk_span(bounds)?;
        let mut requests = Vec::new();
        let mut new_expansions = BTreeSet::new();
        for coord in span.coords() {
            let request = self.load_request_for(bounds, coord)?;
            if self
                .chunks
                .get(&coord)
                .is_some_and(|chunk| chunk.bounds(coord).contains_rect(request.bounds))
            {
                continue;
            }
            if requests.len() == MAX_CHUNKS_PER_GENERATION as usize {
                return Err(GenerateAreaError::TooManyChunks {
                    requested: MAX_CHUNKS_PER_GENERATION + 1,
                    maximum: MAX_CHUNKS_PER_GENERATION,
                });
            }
            if self.request_consumes_expansion_capacity(request)
                && !self
                    .chunks
                    .get(&coord)
                    .is_some_and(LoadedChunk::is_expansion)
            {
                new_expansions.insert(coord);
            }
            requests.push(request);
        }
        self.ensure_chunk_capacity(new_expansions.len())?;
        Ok(requests)
    }

    fn load_request_for(
        &self,
        requested_bounds: WorldRect,
        coord: ChunkCoord,
    ) -> Result<ChunkLoadRequest, GenerateAreaError> {
        let chunk_bounds = coord.bounds()?;
        let selected = intersection(requested_bounds, chunk_bounds)
            .expect("chunk spans are derived from an intersecting request");
        let bootstrap = self.bootstrap_coverage(coord);
        Ok(match bootstrap {
            Some(bounds) if bounds.contains_rect(selected) => ChunkLoadRequest {
                coord,
                bounds,
                kind: ChunkLoadKind::Bootstrap,
            },
            _ => ChunkLoadRequest {
                coord,
                bounds: chunk_bounds,
                kind: ChunkLoadKind::Expansion,
            },
        })
    }

    fn validate_chunk_load_request(
        &self,
        request: ChunkLoadRequest,
    ) -> Result<(), GenerateAreaError> {
        let full_bounds = request.coord.bounds()?;
        let valid = WORLD_GENERATION_BOUNDS.contains_rect(full_bounds)
            && match request.kind {
                ChunkLoadKind::Bootstrap => {
                    self.bootstrap_coverage(request.coord) == Some(request.bounds)
                }
                ChunkLoadKind::Expansion => request.bounds == full_bounds,
            };
        valid
            .then_some(())
            .ok_or(GenerateAreaError::InvalidChunkLoad)
    }

    pub(super) fn ensure_chunk_capacity(&self, requested: usize) -> Result<(), GenerateAreaError> {
        let remaining = MAX_GENERATED_CHUNKS.saturating_sub(self.generated_chunk_count());
        if requested > remaining {
            return Err(GenerateAreaError::WorldCapacity {
                requested,
                remaining,
            });
        }
        Ok(())
    }

    pub fn generated_chunk_count(&self) -> usize {
        self.chunks
            .iter()
            .filter(|(coord, chunk)| {
                chunk.is_expansion()
                    && self.request_consumes_expansion_capacity(ChunkLoadRequest {
                        coord: **coord,
                        bounds: chunk.bounds(**coord),
                        kind: ChunkLoadKind::Expansion,
                    })
            })
            .count()
    }

    pub fn area_is_generated(&self, bounds: WorldRect) -> bool {
        self.missing_chunk_load_requests(bounds)
            .is_ok_and(|requests| requests.is_empty())
    }

    fn request_consumes_expansion_capacity(&self, request: ChunkLoadRequest) -> bool {
        request.kind == ChunkLoadKind::Expansion
            && !self
                .bootstrap_coverage(request.coord)
                .is_some_and(|coverage| coverage.contains_rect(request.bounds))
    }
}
