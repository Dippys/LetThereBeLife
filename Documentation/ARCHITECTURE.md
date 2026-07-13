# Current Architecture

## Runtime boundaries

```text
sim-core
  Owns deterministic simulation state, generated world terrain/features,
  commands, and snapshots.
  Has no windowing or rendering dependency.

sim-headless
  Loads shared configuration and runs sim-core headlessly for a requested number of ticks.

sim-viewer
  Loads shared configuration and owns native windowing, keyboard input, the fixed-step driver,
  camera/hover state, a background generation worker, and wgpu rendering. Reads SimulationSnapshot
  and immutable World data for presentation.
```

## Current contracts

- Interactive presentation actions mutate simulation state through `EngineCommand`; completed worker chunks enter through the explicit, capacity-checked `Engine::apply_world_chunks` boundary.
- Presentation reads engine state through `SimulationSnapshot`.
- `Engine::tick` advances one deterministic simulation step when not paused.
- Viewer wall-clock time is accumulated and converted into fixed simulation ticks.
- Rendered objects are temporary views and are not persistent simulation entities.
- The initial `World` area is generated once from `EngineConfig::seed`; identical seeds produce identical row-major terrain and deterministically ordered sparse features. Selected chunks can extend loaded coverage afterward.
- `WorldConfig` defines validated initial generation dimensions, defaulting to 1,024 x 1,024 cells. Those dimensions are the startup area, not a declared maximum world extent or the complete loaded coverage after expansion.
- Generation visits 64 x 64 chunks, while `World::cell` exposes coordinate lookup without presentation dependencies. `ChunkCoord::from_world_position`, `ChunkCoord::bounds`, and `World::inspect_chunk_at` provide the same Euclidean signed-coordinate rules to inspection clients and distinguish fully initial, partial-initial, retained, retained partial-initial, and missing chunk footprints.
- Terrain uses dense `TerrainCell` values. Trees, rocks, and berry bushes use sparse `Feature` records rather than per-cell object slots.
- Elevation uses integer-only absolute-coordinate generation. A 2,048-cell continental field exclusively decides continental ocean, shallow-water, and sand-coast membership, so finer detail cannot punch isolated water holes through land. Broad/regional relief fields shape only inland elevation. Eligible 1,024-cell regions derive at most one bounded lake descriptor with a guaranteed deep-water core, shallow band, perturbed elliptical shore, and enough margin to remain inside its region.
- Major rivers use a 16 x 16 lattice of pre-lake `continental_elevation` inside each aligned 2,048-cell region. Up to eight deterministic coastal outlets grow backward through strictly higher eight-neighbor nodes. Candidate steps prefer gentle rises and stable headings, cannot return beside old route nodes, and reject diagonal edges that cross the existing path. Accepted routes contain 7 to 14 land nodes, begin at baseline elevation 50,001 or higher, and choose a non-reversing continental-water mouth. Reversing the route therefore produces strict coarse descent through the continental-relief baseline. Five bounded midpoint probes favor the smallest violation of each coarse edge's downhill elevation envelope, integer corner cutting smooths the result without random kinks, and a final clearance check rejects refined segments whose complete water-and-bank corridors would touch nonlocally. This is bounded terrain shaping rather than a proof that every smoothed point follows final lake-composed elevation.
- Each 64 x 64 generation chunk resolves its lake and owning river route from seed plus absolute coordinates, then retains only width-expanded river segments that intersect that chunk. A nominal 20-cell shallow headwater feeds a nominal 16-to-48-cell widening channel. A narrow sand bank exists at the headwater and widens downstream; the deep core appears only after the channel grows beyond headwater width. The route descriptor, 256-sample elevation lattice, and fixed-capacity segment arrays are transient and add no persistent per-cell state or heap allocation. Moisture remains an independent multi-scale field. All terrain stages are independent of loaded-area edges, generation order, and presentation state. The outlet contract proves continental water; for the repository seed, a separate 32-cell sample sees one boundary-touching continental-water component and one accepted startup-area river.
- `sim-viewer::camera::Camera` owns presentation-only center and zoom state. `CameraView` caches the per-frame transform used for screen/world conversion. Visible bounds are half-open: the minimum floors the top-left world point and the maximum ceils the bottom-right point so fractional edge cells are included without adding a cell at exact integer edges.
- Mouse-wheel zoom preserves the world point under the cursor. Left-button dragging translates the presentation camera without clamping it to the generated initial area.
- Camera space uses floating-point presentation coordinates and may move into negative or otherwise ungenerated locations; `World` lookup still returns only simulation-owned generated cells.
- `EngineCommand::GenerateWorldArea` exposes area generation as an `Engine` command path; `WorldRect` uses signed inclusive-minimum/exclusive-maximum coordinates. The viewer's right-drag does not use this path — it streams chunks through a dedicated worker (see below).
- Generated selections are split into deterministic 64 x 64 chunks stored by signed `ChunkCoord` in a `BTreeMap`. A selection may span more than 4,096 chunks, but resolving it may yield at most 4,096 missing chunks (16,777,216 generated chunk-payload cells), and the bootstrap retains at most 16,384 generated chunks. Coordinate-to-chunk arithmetic is checked before iteration. A coordinate counts only when its selected portion is not fully covered by the initial rectangle and no retained chunk exists; retained chunks are omitted before worker generation, and duplicate insertion does not change world revision.
- `World::feature_at` uses binary search over row-major sorted sparse features for bounded hover lookup without a global scan.
- `World::visit_cells_in`, `visit_cells_in_step`, and `visit_features_in` bound presentation extraction to a camera rectangle. They filter the capped retained-chunk map instead of iterating every coordinate in a potentially enormous empty camera rectangle. Stepped visits provide deterministic zoomed-out sampling without changing simulation-owned terrain.
- Manual selected-area and automatic visible-area generation share one dedicated viewer worker. Automatic demand is recalculated after startup, resize, zoom, left-drag pan release, and successful worker completion; continuous camera motion does not scan the world on every pointer or render event. The main thread validates the current half-open view and queues only missing `ChunkCoord` values. A view needing more than 4,096 new chunks or more than remaining retained capacity is rejected as a whole rather than truncated, and right-drag remains the explicit smaller-area fallback. The worker generates chunks individually and returns `WorldChunk` values through a 64-message bounded channel. The main thread applies at most 16 chunks per frame. `C` stops remaining work without rolling back chunks already applied and does not retry the same automatic demand until another view-change trigger; terminal worker outcomes cover completion, cancellation, validation failure, and one-shot disconnection reporting.
- GPU world-cache rebuilds are deferred while chunks stream and occur once when the generation job finishes, unless camera movement leaves the current cache or zoom changes the sampling step first.
- The viewer enforces a persistent 60 Hz redraw deadline, including during input bursts and background-generation polling, and stops redrawing when paused and unchanged. Input, resize, ticks, surface recovery, and terminal generation outcomes mark presentation dirty.
- The `wgpu` renderer keeps camera transforms in 32-byte uniforms and terrain/features in 20-byte rectangle instances. A scale-relative cache margin of approximately 128 screen pixels prevents buffer uploads during ordinary pans at every zoom; terrain is sampled in power-of-two, chunk-aligned blocks of roughly two screen pixels, and static instance uploads are split at 1,000,000 instances per GPU buffer. The cursor chunk uses four bounded overlay rectangles, suppressed below four projected pixels, and the six-instance overlay staging vector is reused across frames.

## Dependencies

- `sim-core`: Rust standard library only.
- `sim-config`: `sim-core`, `serde`, and `toml`; owns filesystem and TOML concerns shared by runtime binaries.
- The `sim-config` build script tracks the repository configuration and copies it into the active Cargo profile directory so directly launched binaries retain the default `config/simulation.toml` layout.
- `sim-headless`: `sim-config` and `sim-core`.
- `sim-viewer`: `sim-config`, `sim-core`, `winit`, `wgpu`, `pollster`, and `bytemuck`.
