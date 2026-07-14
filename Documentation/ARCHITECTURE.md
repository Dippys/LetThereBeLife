# Current Architecture

## Runtime boundaries

```text
sim-core
  Owns deterministic simulation state, the declared terrain source, resident
  materialization cache, commands, and snapshots.
  Has no windowing or rendering dependency.

sim-headless
  Loads shared configuration and runs sim-core headlessly for a requested number of ticks.

sim-viewer
  Loads shared configuration and owns native windowing, keyboard input, the fixed-step driver,
  camera/hover state, a background generation worker, and wgpu rendering. Reads SimulationSnapshot
  and immutable World data for presentation.
```

## Current contracts

- Interactive presentation actions mutate simulation state through `EngineCommand`; viewer worker payloads enter through the explicit, capacity-checked `Engine::apply_world_chunk_loads` boundary.
- Presentation reads engine state through `SimulationSnapshot`.
- `Engine::tick` advances one deterministic simulation step when not paused.
- Viewer wall-clock time is accumulated and converted into fixed simulation ticks. Worker terrain arrivals merge only after that event-loop turn's fixed-tick phase.
- Rendered objects are temporary views and are not persistent simulation entities.
- `Engine::new` constructs a declared, empty bootstrap world in constant time. `World::generate` and `Engine::materialize_initial_area` remain explicit eager paths; `sim-headless` uses the latter before ticks, while the viewer streams bootstrap tiles after its window and GPU are ready.
- `WorldConfig` defines validated bootstrap dimensions, defaulting to 1,024 x 1,024 cells. The rectangle is a deterministic loading target, not a claim that every cell is resident or a maximum world extent. The existing 16,777,216-cell limit also permits narrow, non-64-aligned worlds.
- World storage is a deterministic `BTreeMap` materialization cache of private loaded tiles. A bootstrap tile holds exactly its intersection with the configured rectangle; an expansion tile holds a complete 64 x 64 chunk. `World::cell`, visitors, feature queries, and `loaded_bounds_at` expose only resident coverage. The seed/configuration define terrain independently of that coverage, and current simulation state must not branch on cache residency. A partial bootstrap tile promotes monotonically to a full expansion tile if a later request reaches its fringe.
- Generation visits 64 x 64 chunks, while `World::cell` exposes coordinate lookup without presentation dependencies. `ChunkCoord::from_world_position`, `ChunkCoord::bounds`, and `World::inspect_chunk_at` provide the same Euclidean signed-coordinate rules to inspection clients and distinguish unloaded initial, unloaded partial-initial, initial, partial-initial, retained, retained partial-initial, and missing chunk footprints.
- Terrain uses dense `TerrainCell` values. Trees, rocks, and berry bushes use sparse `Feature` records rather than per-cell object slots. Resident cell/feature iterators are deterministic by `ChunkCoord` (`x`, then `y`) and tile-local row order; they do not promise global world-row order for sparse coverage.
- Terrain generation is private to `sim-core` and uses integer-only absolute coordinates in three tiers. Analytic plate, coast, relief, temperature, and moisture fields are pure functions of the seed and coordinates. A 4,096-cell drainage region then caches a 129 x 129 lattice, priority-fills depressions, derives static lake masks, and routes accumulated runoff into static river segments. Region maps live behind a process-shared `Mutex`/`OnceLock` LRU: lookup and recency changes are locked briefly, each key has one build slot, and workers clone an immutable `Arc<RegionMap>` after initialization. Completed entries are capped at 40; in-flight entries are never evicted and may temporarily exceed that cap by the number of distinct regions simultaneously building. Cache presence and completion order affect only cost, never output.
- Region-local lakes clear two node layers at each regional edge so they cannot imply false cross-region continuity. River segments end before a separate four-node (128-cell) regional margin; this is a deliberate hard cutoff until cross-region drainage exists. Final chunk synthesis interpolates a 3 x 3 node window, applies bounded local detail, preserves lake-water ground classification and surface elevation, carves river cores/banks, then emits compact terrain and sparse features. Water cells cannot emit sparse surface features.
- Each 64 x 64 generation chunk uses a fixed 25-slot river index. The bound follows the 32-cell hydrology lattice and one outgoing segment per node; filtering uses width-expanded segment bounds and an internal assertion plus regression coverage guards the invariant. The internal `ChunkContext` is not public. Tooling that needs sparse sampling uses the validated `ChunkGenerator` contract instead of raw origin/span inputs.
- `sim-viewer::camera::Camera` owns presentation-only center and zoom state. `CameraView` caches the per-frame transform used for screen/world conversion. Visible bounds are half-open: the minimum floors the top-left world point and the maximum ceils the bottom-right point so fractional edge cells are included without adding a cell at exact integer edges.
- Mouse-wheel zoom preserves the world point under the cursor. Left-button dragging translates the presentation camera without clamping it to the generated initial area.
- Camera space uses floating-point presentation coordinates and may move into negative or otherwise ungenerated locations; `World` lookup still returns only resident core-owned terrain cache entries.
- `EngineCommand::GenerateWorldArea` exposes area generation as an `Engine` command path; `WorldRect` uses signed inclusive-minimum/exclusive-maximum coordinates. The viewer's right-drag does not use this path — it streams chunks through a dedicated worker (see below).
- Generated selections use deterministic signed `ChunkCoord` tiles. Chunk traversal groups complete 4,096-cell drainage regions before moving to the next region, preventing a wide request from repeatedly evicting the 40-entry regional cache. Manual selection remains all-or-nothing at 4,096 missing chunks; only full expansion tiles outside fully covered bootstrap space consume the independent 16,384-tile expansion capacity. Coordinate arithmetic is checked before iteration, duplicate/stale payloads are idempotent, and a payload must match both the receiving world seed and its declared bootstrap coverage.
- `World::feature_at` uses binary search over each tile's row-major sparse features for bounded hover lookup without a global scan.
- `World::visit_cells_in`, `visit_cells_in_step`, and `visit_features_in` bound presentation extraction to a camera rectangle. They visit only intersecting resident tiles rather than iterating configured-but-unloaded or empty world space. Stepped visits provide deterministic zoomed-out sampling without changing simulation-owned terrain.
- Manual selection, current-viewport demand, and background bootstrap share one persistent viewer coordinator. Priority is manual, then the newest automatic viewport pager, then bootstrap. Pagers yield one deterministic center-out 8 x 8 chunk page at a time and retain only their current ring state, so a zoomed-out view never allocates or waits on an all-visible-chunk request. The coordinator owns a fixed Rayon pool of `available_parallelism - 1` workers, with a one-worker minimum. Its active-plus-completed reorder window is two tasks per worker capped at 64; immutable loads may finish out of order, but the coordinator releases only the next original request index through the separate 64-message result channel. A camera change cancels stale automatic/bootstrap work by generation ID: no new tasks are dispatched, completed stale results are dropped, already-running pure tasks finish, and already-applied chunks remain valid. The main thread inserts 16-load batches while a 2 ms budget remains, with a hard 64-load per-frame ceiling. A full or disconnected job queue returns the unsent page for retry/retention; a stopped coordinator becomes title-visible and prevents further requests.
- `C` cancels remaining work, drops pending pagers/manual work, preserves already applied chunks, and does not retry until a later view change. Manual/automatic requests that exceed coordinate, per-request, or expansion-capacity guardrails are reported rather than weakened.
- Every applied streamed batch unions its exact loaded bounds into presentation dirtiness. The renderer rebuilds a visible affected cache at most once per 125 ms during streaming, rebuilds immediately when camera coverage or sampling changes, and advances its revision without a GPU upload for off-cache changes. Page completion requests a final immediate synchronization.
- The viewer enforces a persistent 60 Hz redraw deadline while simulation, generation, or coalesced GPU synchronization is active and stops redrawing when paused and visually unchanged. Input, resize, ticks, surface recovery, and streamed arrivals mark presentation dirty.
- The `wgpu` renderer keeps camera transforms in 32-byte uniforms and terrain/features in 20-byte rectangle instances. A scale-relative cache margin of approximately 128 screen pixels prevents buffer uploads during ordinary pans at every zoom; terrain is sampled in power-of-two, chunk-aligned blocks of roughly two screen pixels, and static instance uploads are split at 1,000,000 instances per GPU buffer. The cursor chunk uses four bounded overlay rectangles, suppressed below four projected pixels, and the six-instance overlay staging vector is reused across frames.

## Dependencies

- `sim-core`: Rust standard library only.
- `sim-config`: `sim-core`, `serde`, and `toml`; owns filesystem and TOML concerns shared by runtime binaries.
- The `sim-config` build script tracks the repository configuration and copies it into the active Cargo profile directory so directly launched binaries retain the default `config/simulation.toml` layout.
- `sim-headless`: `sim-config` and `sim-core`.
- `sim-viewer`: `sim-config`, `sim-core`, `winit`, `wgpu`, `pollster`, `bytemuck`, and Rayon. Rayon is confined to immutable terrain payload computation; it never mutates `Engine` or presentation state.
