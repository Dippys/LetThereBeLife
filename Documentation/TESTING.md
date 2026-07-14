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

## Existing automated coverage

- Identical engine inputs yield identical snapshots after 1,000 ticks.
- A paused engine does not advance.
- Reset restores runtime state while preserving engine configuration.
- Equal seeds produce equal declared engine worlds; explicit bootstrap materialization reproduces eager deterministic terrain and features, and materializing terrain does not alter fixed tick progression.
- World generation is deterministic and different seeds change terrain.
- Generated samples contain terrain variation plus sparse features.
- A structural fixed-seed overview checks a dominant open ocean, varied continent sizes, elongated connected mountain ranges, and coherent desert, forest, and grass regions.
- Regional-drainage checks cover two seeds and positive/negative 4,096-cell regions. They prove coarse river segments descend through the filled surface, terminate in water/continuing drainage/border drainage, and keep the two-node lake edge margin dry.
- Rasterized samples from flooded regional nodes must remain deep or shallow water and cannot emit a sparse surface feature. An overlapping-segment regression proves a deep river core wins over a shallow bank in both segment orders. A multi-seed river-index regression compares every candidate chunk's retained fixed-array segments against the region's exact width-expanded intersection set.
- Initial generation is compared to independently generated chunks both near the origin and across the 4,096-cell regional boundary. A coordinate-order regression ensures wide chunk spans finish one drainage region before beginning the next, while a cold-cache reset cannot change chunk output. Direct one-worker and four-worker `RegionMap` builds compare every retained lattice and river segment exactly.
- Deferred bootstrap construction has zero resident cells while retaining exact configured bounds and unloaded inspection states. Streamed aligned and clipped bootstrap payloads match eager terrain/features even when inserted in a different order; eager materialization is also exercised in multiple bounded batches.
- `GroundType` remains one byte and `TerrainCell` remains four bytes; surface features remain excluded from water and sand.
- Terrain lookup exposes only resident valid coordinates and rejects configured-but-unloaded or out-of-bounds cells.
- Chunk inspection uses canonical Euclidean coordinates across -65, -64, -1, 0, 63, and 64; it distinguishes full initial coverage, a non-aligned partial-initial boundary before and after its remainder is retained, fully retained chunks, missing chunks, and an unrepresentable positive edge.
- Rectangular initial-area generation preserves configured dimensions and exact cell count, including the maximum-cell one-cell-wide configuration without silently requiring chunk alignment.
- Overlapping configured initial areas generate identical terrain and features at equal world coordinates.
- Invalid zero-sized or excessive initial allocations are rejected.
- `ChunkGenerator` validates unrepresentable chunk coordinates and invalid local cells, while valid samples match `World::generate_chunk_at` terrain and feature output.
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
- HUD regressions cover simulation/time/status text, loaded-cell inspection, unloaded partial-initial coverage, unavailable-worker reporting, and every supported status/cursor layout under the fixed 4,096-instance screen-overlay budget.
- Re-requesting existing chunks does not advance world revision.
- The viewer world-generation worker returns immutable bootstrap-aware loads independently of the event-loop thread; the hidden runtime smoke proves a worker-produced terrain arrival can merge and render in the same runtime path.

## Known gaps

- GPU adapter/surface creation, shader binding layout, drawing, and presentation are covered by the automated hidden-window smoke run. Resize recovery and interactive event dispatch remain manual runtime checks.
- No property tests, generator checksums, cross-region drainage/tributary tests, save/load tests, or long-running soak tests exist yet. A focused ignored release test measures cold generation with a fixed seed, selectable worker count, and optional square world size; current records cover one 1,024-chunk page footprint and 4,096 chunks, but this is not yet the complete world-generation benchmark suite.
- Renderer `CameraUniform` and `Instance` sizes have compile-time assertions; broader foundational layout assertions, allocation/resident-memory measurements, frame-time capture, and a canonical multi-workload reporting harness do not exist yet. `Documentation/PERFORMANCE.md` records current representation calculations and those remaining measurement gaps.
- Segmented multi-buffer drawing, interactive confirmation that pan/zoom remains generation-free, a full bootstrap completion run, and an interactive large right-drag generation path are not integration-tested. The hidden GPU smoke covers startup pipeline creation plus the first streamed terrain arrival and presentation.
