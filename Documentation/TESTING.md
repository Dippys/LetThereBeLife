# Testing and Validation

Last synchronized: 2026-07-14.

## Required Rust baseline

```powershell
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

The canonical full gate also verifies immutable documentation and all repository skill manifests:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .codex/skills/validate-rust-workspace/scripts/validate.ps1
```

The gate also verifies that `target/debug/config/simulation.toml` exists and is byte-equivalent to the repository configuration after the workspace build.
It launches the hidden viewer, waits for at least one asynchronously streamed terrain tile, then renders two frames, validating GPU adapter/surface creation, WGSL pipeline layout, worker-to-main-thread loading, command submission, and presentation.

## Runtime checks

```powershell
cargo run -p sim-headless -- --ticks 600 --seed 42
cargo run -p sim-viewer
```

Expected headless result for the command above:

```text
completed tick=600 simulated_seconds=10.000 seed=42 initial_world=4096x4096
```

## Repeatable world-quality review

Build and emit the complete canonical set with one release-mode command from the repository root:

```powershell
cargo run --release -p sim-core --example render_map -- --review-set
```

The command writes ignored derived artifacts under `target/world-quality/`:

- `seed-<seed>/<view>.bmp`: 12 canonical sampled images across seeds 1, 7, 42, and 10,001;
- `review_manifest.tsv`: review format, source revision (with `+dirty` when tracked files differ), view name, seed, half-open bounds, sample step, dimensions, feature-overlay state, semantic sample hash, and relative image path;
- `distribution.tsv`: exact sampled surface, biome, and feature-kind counts, aggregate feature density in parts per million, and the semantic hash;
- `representation.tsv`: `size_of` and alignment for `SurfaceType`, `BiomeType`, `TerrainClass`, `TerrainCell`, `ClimateSample`, `FeatureKind`, `Feature`, `GeneratedCell`, and `ChunkCoord`;
- `seed_roles.tsv`: the fixed reason each seed belongs to the set.

The four `full-envelope` views sample `[-32,768, 32,768)` at a 128-cell step. Focused views cover the origin region, an x-axis drainage boundary, a y-axis drainage boundary, coastline, a verified river mouth, lake, mountain belt, and a cell-scale forest/coast close-up with sparse feature colors. The reports are deterministic for equal generator inputs and source-revision metadata; per-view and total elapsed times are intentionally console-only. `--source-revision TEXT` can replace Git discovery for packaged or controlled comparisons, and `--out-dir PATH` can isolate repeated runs.

These views are review evidence, not pass/fail appearance thresholds. When generator behavior changes intentionally, compare all representative seeds and update hashes only with the implementation and documented decision that caused the change.

World-foundation Slice 3 intentionally advanced the report to review format 2 and changed semantic hashes. The 2026-07-14 post-slice complete-envelope hashes are `a2373a6b276fcb06` (seed 1), `a3bef667d00b0691` (seed 7), `0c174931efc451af` (seed 42), and `7ce7b66e324e6cb6` (seed 10,001). Visual inspection confirmed broad tundra/snow shoulders, separate beach and hot-arid colors, regional grassland/savanna/forest structure, visible but sparse saturated-lowland wetlands, and preserved connected drainage. The provisional wetland placement is specifically a Slice 4 refinement target rather than a final hydrologic claim.

Compare drainage skeleton candidates in isolated release test processes by setting `SIM_DRAINAGE_STEP` to `128` and `256` in turn. `SIM_DRAINAGE_SEED` optionally selects a seed and defaults to 1:

```powershell
$env:SIM_DRAINAGE_STEP='256'
cargo test --release -p sim-core worldgen::drainage::tests::compare_candidate_skeleton_steps -- --ignored --nocapture --test-threads=1
```

## Existing automated coverage

- Identical engine inputs yield identical snapshots after 1,000 ticks.
- A paused engine does not advance.
- Reset restores runtime state while preserving engine configuration.
- Equal seeds produce equal declared engine worlds; explicit bootstrap materialization reproduces eager deterministic terrain and features, and materializing terrain does not alter fixed tick progression.
- World generation is deterministic and different seeds change terrain.
- Generated samples contain terrain variation plus sparse features.
- A structural fixed-seed overview checks a dominant open ocean, varied continent sizes, elongated connected mountain ranges, and coherent desert, forest, and grass regions.
- Packed terrain-class regressions round-trip every public surface/biome enum combination without collision, keep `TerrainClass` at one byte and `TerrainCell` at four bytes, and prove that equal rendered surfaces can retain distinct beach/desert and grassland/wetland semantics while cold lowlands and cold mountains remain separately typed tundra and alpine snow/ice.
- Climate structure is sampled only inside `WORLD_GENERATION_BOUNDS`. Seeds 1, 7, 42, and 10,001 must each contain at least 3% cold, temperate, and warm qualifying lowland samples on the canonical 256-cell lattice, with cold lowlands represented on both vertical sides across the review set. Separate regressions prove the center is materially warmer than both finite edges and each nominal circulation boundary contains both directions rather than forming a straight seam.
- Signed edge, center, and wind-band probes require `ClimateSample.temperature` to equal the exact 32-cell value used by chunk classification, retain the terrain cell's moisture byte, and remain deterministic on repeat. Existing one/four-worker regional equality and cache-clear chunk equality cover climate output independence from worker count and cache state.
- Whole-envelope drainage checks build seeds 1, 7, 42, and 10,001 and prove every major link continues with the same channel identity, joins a declared confluence, enters an identified lake, reaches ocean/world-edge drainage, or uses explicit terminal-basin status. They cover river crossings on positive and negative x/y 4,096-cell seams, basin identity through downstream links, canonical lake outlets, and the eight-segment complete-envelope chunk-index maximum.
- Adjacent `RegionMap` checks compare every shared x/y seam sample exactly and prove crossed river segments are present on both sides without the former fixed dry margins. A focused chunk regression generates signed seam-adjacent chunks in forward/single-worker and reversed/four-worker order and requires byte-identical results.
- Rasterized samples from flooded regional nodes must remain deep or shallow lake water and cannot emit a sparse surface feature. An overlapping-segment regression proves a deep river core wins over a shallow bank in both segment orders and remains typed as river water. A multi-seed river-index regression compares every candidate chunk's retained fixed-array segments against the region's exact width-expanded intersection set.
- Initial generation is compared to independently generated chunks both near the origin and across the 4,096-cell regional boundary. A coordinate-order regression ensures wide chunk spans finish one drainage region before beginning the next, while a cold-cache reset cannot change chunk output. Direct one-worker and four-worker builds compare every retained `DrainageSkeleton` field and every `RegionMap` lattice/river segment exactly.
- Deferred bootstrap construction has zero resident cells while retaining exact configured bounds and unloaded inspection states. Streamed aligned and clipped bootstrap payloads match eager terrain/features even when inserted in a different order; eager materialization is also exercised in multiple bounded batches.
- `SurfaceType`, `BiomeType`, and packed `TerrainClass` are each one byte; `TerrainCell` and the derived `ClimateSample` remain four bytes. Surface features remain excluded from water, sand, and snow/ice.
- Terrain lookup exposes only resident valid coordinates and rejects configured-but-unloaded or out-of-bounds cells.
- Chunk inspection uses canonical Euclidean coordinates across -65, -64, -1, 0, 63, and 64; it distinguishes full initial coverage, a non-aligned partial-initial boundary before and after its remainder is retained, fully retained chunks, missing chunks, and an unrepresentable positive edge.
- Rectangular initial-area generation preserves configured dimensions and exact cell count, including the maximum-cell one-cell-wide configuration without silently requiring chunk alignment.
- Overlapping configured initial areas generate identical terrain and features at equal world coordinates.
- Invalid zero-sized or excessive initial allocations are rejected.
- `ChunkGenerator` validates unrepresentable chunk coordinates and invalid local cells, while valid samples match `World::generate_chunk_at` terrain and feature output.
- The `render_map` example test harness rejects views outside the finite envelope or pixel budget, proves sampling alignment across signed chunk boundaries, asserts that canonical seam views cross both axes of a 4,096-cell boundary, keeps repository seed 1 in a unique multi-seed set, and verifies byte-stable manifest metadata plus coordinate-sensitive deterministic semantic hashes.
- TOML configuration parsing maps simulation/world settings and rejects invalid world dimensions.
- Resident cell and feature iteration is locked to deterministic chunk-coordinate then tile-local row order; sparse features remain row-major within each loaded tile and support coordinate lookup.
- The viewer camera starts at world origin, preserves cursor anchoring during ordinary zoom, and clamps visible edges to the maximum world boundary where the viewport fits.
- Maximum zoom-out derives its floor from viewport, bootstrap, and full-envelope dimensions. Tests prove the full red world square is visible and that the repository's 4,096 x 4,096 bootstrap reaches it at 1/16x of initial fit.
- Camera view rectangles floor fractional minimum edges and ceil fractional maximum edges while preserving exact integer maxima as exclusive bounds.
- Selected patches generate deterministically in negative coordinate space. Generation budgeting counts only missing chunks and accepts exactly 65,536 new chunks while rejecting the next rectangular footprint over that limit. Coordinates outside the centered maximum-world envelope are rejected consistently.
- A non-aligned initial boundary counts as missing when a selected 64 x 64 chunk extends from its initial-area portion into unloaded terrain. The clipped tile hides its fringe until a full expansion payload promotes it, and bootstrap tiles do not consume expansion capacity.
- Opaque streamed loads reject a mismatched generator seed or bootstrap geometry before changing world revision or storage.
- Maximum-world assertions fix the envelope at `[-32,768, 32,768)`, 1,048,576 chunks, 4,294,967,296 cells, and 16 GiB of raw four-byte terrain; edge chunks are accepted and the first chunk beyond the edge is rejected.
- The persistent generation coordinator carries job identity, uses one-worker and four-worker test pools, preserves exact request order and byte-equivalent loads across pool sizes, shares one regional materialization across concurrent callers, supports cancellation without starting queued chunk work, discards stale queued loads, obeys the configured per-frame insertion budget, returns an unsent job intact when its queue is full or disconnected, and marks a disconnected worker unavailable in the HUD. Viewer scheduling has no work after bootstrap unless a manual request is queued, guarding the right-drag-only expansion contract.
- Right-drag preview uses the same missing-chunk budget: a footprint larger than 65,536 chunks remains yellow when at most 65,536 are missing, while a 65,537-missing request is red. Focused viewer regressions prove a drag started in bounds caps at the maximum-world boundary and a drag cannot start outside it. Accepted manual work preempts bootstrap paging.
- The center-out `ChunkPager` yields bounded 32 x 32 chunk pages without preallocating the complete bootstrap request; the origin page is proven to span chunks `-16..15` on both axes before paging outward. It skips already resident coverage. Manual work wins over bootstrap work, background cancellation never discards manual work, and scheduling has no work once bootstrap is absent unless a manual request is queued.
- Chunk-keyed generation filters initial-area and existing-chunk overlap before worker generation and does not duplicate rendered cells.
- Stepped cell visits sample resident bootstrap and generated chunks deterministically; renderer tests verify coarse instance reduction, power-of-two scale selection, exact loaded-tile clipping, empty deferred coverage, progressive visible-cache synchronization, off-cache upload avoidance, and the static-buffer partition boundary.
- Chunk-inspection rendering aligns its four-rectangle outline to signed chunk bounds and suppresses the overlay when a chunk projects below four screen pixels.
- HUD regressions cover simulation/time/status text, loaded-cell elevation/temperature/moisture/wind inspection, unloaded partial-initial coverage, unavailable-worker reporting, and every supported status/cursor layout under the fixed 4,096-instance screen-overlay budget.
- Re-requesting existing chunks does not advance world revision.
- The viewer world-generation worker returns immutable bootstrap-aware loads independently of the event-loop thread; the hidden runtime smoke proves a worker-produced terrain arrival can merge and render in the same runtime path.

## Known gaps

- GPU adapter/surface creation, shader binding layout, drawing, and presentation are covered by the automated hidden-window smoke run. Resize recovery and interactive event dispatch remain manual runtime checks.
- No property tests, authoritative full-resolution generator checksums, tributary/local-stream hierarchy tests, save/load tests, or long-running soak tests exist yet. Cross-region major drainage has deterministic topology, seam, pool-size, request-order, and complete-envelope index coverage, while the world-quality set records hashes only for its fixed sampled views. Focused ignored release tests measure candidate skeleton steps and cold chunk generation, but this is not yet the complete world-generation benchmark suite.
- Renderer `CameraUniform` and `Instance` sizes have compile-time assertions, `TerrainCell` has a unit size assertion, and the world-quality report records relevant public generation-tool layouts. Broader foundational layout assertions, allocation instrumentation, frame-time capture, and a canonical multi-workload runtime benchmark harness do not exist yet. `Documentation/PERFORMANCE.md` records current representation calculations and those remaining measurement gaps.
- Segmented multi-buffer drawing, interactive confirmation that pan/zoom remains generation-free, a full bootstrap completion run, and an interactive large right-drag generation path are not integration-tested. The hidden GPU smoke covers startup pipeline creation plus the first streamed terrain arrival and presentation.
