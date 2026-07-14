# Current Implementation

Last synchronized: 2026-07-14.

## Implemented

- Rust 2024 Cargo workspace containing `sim-core`, `sim-config`, `sim-headless`, and `sim-viewer`.
- Engine-independent `sim-core` configuration, deterministic tick counter, pause/reset/speed commands, and immutable `SimulationSnapshot` output.
- Constant-time `Engine`/`World` construction with a declared configurable bootstrap rectangle. The repository configuration targets 4,096 x 4,096 cells (with a 1,024 x 1,024 Rust fallback), but the viewer streams it after window creation instead of allocating it at startup; `sim-headless` explicitly materializes it before ticks. Bootstrap coverage is capped at 16,777,216 cells and supports non-aligned rectangular edges.
- A private, three-tier integer-only generation pipeline in `sim-core/src/worldgen/`. Global analytic fields provide domain-warped tectonic plates, continental/oceanic structure, mountain ranges, basins, latitude/altitude temperature, and upwind moisture. No presentation or operating-system dependency enters this pipeline.
- Cached regional drainage: each 4,096-cell region has a 129 x 129 lattice at 32-cell steps. Priority-flood filling derives static lakes and runoff-derived river segments. Two edge node layers are dry so region-local lakes cannot create a false cross-region seam; river segments stop before the separate 128-cell border margin. A bounded thread-local LRU cache (40 region maps) is an implementation cache only: generated output remains a function of seed and coordinates.
- Per-chunk synthesis interpolates a 3 x 3 regional lattice, adds roughness-budgeted detail, preserves masked lake-water classification and surface elevation, carves static river channels, and classifies deep water, shallow water, beach/desert sand, grass, forest floor, hill, and bare rock. Chunk river lookup uses a proved 25-slot fixed index with regression coverage rather than silently discarding segments.
- `World::generate_chunk_at` remains the normal chunk-generation entry point. `ChunkGenerator` is a validated read-only sampler for developer tooling such as the `render_map` BMP example; its internal worldgen context is not public API.
- Shared `sim-config` TOML loading for `sim-viewer` and `sim-headless`, with explicit validation of initial dimensions.
- `sim-config` build integration copies `config/simulation.toml` beside Cargo binaries under the active profile directory, including `target/debug/config/simulation.toml` and the equivalent release path.
- VS Code launch entries build and run viewer/headless binaries in debug or release mode with the repository configuration and working directory, plus a task for the complete workspace validation gate.
- Sparse deterministic tree, rock, and berry-bush feature records owned by the generated world.
- Fixed-step accumulator in `sim-viewer`, separating variable render timing from 60 Hz simulation ticks and capping large frame delays. Worker-produced terrain loads merge only after the event-loop turn's fixed-tick phase; resident coverage is a deterministic materialization cache, not tick-driven domain state.
- Native `winit` window and event lifecycle with redraws capped at 60 Hz while active and suspended when paused and visually unchanged.
- GPU presentation through `wgpu`, using size-checked camera uniforms and instanced terrain, feature, selection, hover, and status rectangles instead of CPU framebuffer rasterization.
- Camera-bounded GPU instance caches with an approximately 128-screen-pixel scale-relative margin, deterministic power-of-two zoomed-out sampling with exact loaded-tile clipping, and static buffers segmented at 1,000,000 instances. Streamed changes rebuild a visible cache at a 125 ms cadence, force a final page-completion sync, and advance cache revision without an upload when a changed tile is off-cache.
- Deterministic 64 x 64 generated chunks keyed by signed `ChunkCoord`, with bounded lookup instead of a growing linear patch search. Actual storage distinguishes clipped bootstrap tiles from full expansion tiles, so unloaded configured cells are never exposed and partial bootstrap tiles promote monotonically when expansion reaches their fringe. Initial and streaming generation visit complete drainage-region groups before moving to the next region; each tile keeps sparse features row-major for lookup, while resident iterators use deterministic chunk-coordinate then tile-local order.
- A dedicated persistent world-generation worker receives opaque core-owned load requests, keeps generation off the window/event-loop thread, and returns seed/coverage-validated loads for main-thread insertion into `sim-core`. An unsent page is retained if its queue is full or disconnected; a disconnected worker is explicitly reported in the title.
- Large-generation guardrails: 4,096 missing chunks per manual request, 16,384 retained expansion chunks, a 64-message worker channel, and 16 loads applied per frame. Bootstrap tiles do not consume expansion capacity; a partial initial tile promoted to a full expansion tile does. `C` drops remaining queued work while preserving already applied terrain.
- Automatic visible-area generation runs after startup, resize, zoom, and camera movement. A deterministic center-out 8 x 8 chunk pager produces only the next page, so large zoomed-out views stream progressively rather than forming one oversized request. Manual work preempts automatic work, automatic work preempts bootstrap work, and generation IDs discard stale results after view changes. Coordinate or capacity errors remain visible in the title.
- Viewer camera starting at full-map fit with cursor-anchored mouse-wheel zoom from 1/16x fit to 64x magnification and unbounded left-button drag panning through generated or empty space.
- Right-button drag selection with a translucent yellow valid preview and red invalid preview for missing-chunk-budget, retained-capacity, or coordinate-safety failures. A new manual preview is available while automatic/bootstrap work runs and preempts that background work on release; only active manual work or an unavailable worker prevents a new preview. Release generates deterministic terrain and sparse features only for an accepted rectangle.
- Signed world positions supporting generated patches in negative and positive coordinate space.
- Allocation-free `sim-core` chunk inspection exposing canonical signed chunk coordinates, half-open bounds, compact local coordinates, and unloaded initial, unloaded partial-initial, initial, partial-initial, retained, retained partial-initial, or missing coverage without exposing mutable storage.
- Hover inspection with a highlighted loaded cell, a color-coded outline for the inspected chunk when it is at least four screen pixels wide, and window-title output for world/chunk/local coordinates, coverage, terrain, elevation, moisture, and sparse feature kind. Unloaded cells remain inspectable.
- Keyboard controls: pause/resume, 1x-8x speed selection, cancel active generation, reset, and exit.
- Headless runner accepting `--ticks` and `--seed`.
- Unit tests covering equal-input tick determinism, cache-residency-independent tick progression, pause behavior, reset behavior, lazy bootstrap residency, clipped-edge promotion, seed/coverage-validated worker loads, bounded center-out paging, cancellation/preemption/non-lossy queue failure, progressive renderer-cache policy, chunk/world agreement across a regional boundary, cold-cache output independence, drainage descent and border constraints across positive and negative regions, rasterized lake-water/feature exclusion, and complete chunk river indexing.
- Repository-local skills for orientation, Rust implementation, living documentation, validation, architecture/code review, and workflow evolution.
- Root agent routing with a mandatory inspect, implement, document, review, and validate lifecycle.
- A checksum manifest that detects any change under immutable `InitialDocumentation/`.

## Not implemented

- Chunk unloading, generator versioning, or chunk persistence; initial and automatically or manually generated chunks currently remain in memory until exit.
- Cross-region drainage, dynamic water flow, erosion, wetlands, dams/canals, resource quantities, or terrain modification. Current lakes and channels are static deterministic terrain derivation, not a watershed simulation across the whole world.
- Persistent agents, needs, cognition, movement, or event scheduling.
- General-purpose deterministic RNG streams, save/load, snapshots on disk, or replay logs.
- Text, asset, animation, and advanced inspection systems beyond the current GPU rectangle renderer.
- Region workers, parallel simulation, networking, or long-term persistence.
