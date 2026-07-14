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
- Rasterized samples from flooded regional nodes must remain deep or shallow water and cannot emit a sparse surface feature. A multi-seed river-index regression compares every candidate chunk's retained fixed-array segments against the region's exact width-expanded intersection set.
- Initial generation is compared to independently generated chunks both near the origin and across the 4,096-cell regional boundary. A coordinate-order regression ensures wide chunk spans finish one drainage region before beginning the next, while a cold-cache reset cannot change chunk output.
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
- The viewer camera initially centers/fits the world, preserves cursor anchoring during zoom, and can pan beyond generated world edges.
- Viewer zoom clamps at 1/16x of the initial fit rather than stopping at the startup framing.
- Camera view rectangles floor fractional minimum edges and ceil fractional maximum edges while preserving exact integer maxima as exclusive bounds.
- Selected patches generate deterministically in negative coordinate space. Generation budgeting counts only missing chunks: a selection spanning 4,097 chunks is accepted when one chunk is already covered (4,096 missing), while a selection requiring 4,097 new chunks is rejected. Unsafe-coordinate selections remain rejected while the representable negative coordinate edge is accepted.
- A non-aligned initial boundary counts as missing when a selected 64 x 64 chunk extends from its initial-area portion into unloaded terrain. The clipped tile hides its fringe until a full expansion payload promotes it, and bootstrap tiles do not consume expansion capacity.
- Opaque streamed loads reject a mismatched generator seed or bootstrap geometry before changing world revision or storage.
- Generated-world storage rejects inserts beyond its 16,384-chunk retained capacity.
- The persistent generation coordinator carries job identity, uses one-worker and four-worker test pools, preserves exact request order and byte-equivalent loads across pool sizes, shares one regional materialization across concurrent callers, supports cancellation without starting queued chunk work, discards stale queued loads, obeys the configured per-frame insertion budget, returns an unsent job intact when its queue is full or disconnected, and marks a disconnected worker unavailable in the title.
- Right-drag preview uses the same missing-chunk budget: a footprint larger than 4,096 chunks remains yellow when at most 4,096 are missing, while a 4,097-missing request is red. Accepted manual work preempts background paging.
- The center-out viewport pager yields bounded 8 x 8 chunk pages without preallocating the full zoomed-out request, starts at the focus page, skips already resident coverage, and keeps newest automatic demand separate from cancellation drain. Manual work wins over bootstrap/automatic work, while background cancellation never discards manual work.
- Chunk-keyed generation filters initial-area and existing-chunk overlap before worker generation and does not duplicate rendered cells.
- Stepped cell visits sample resident bootstrap and generated chunks deterministically; renderer tests verify coarse instance reduction, power-of-two scale selection, exact loaded-tile clipping, empty deferred coverage, progressive visible-cache synchronization, off-cache upload avoidance, and the static-buffer partition boundary.
- Chunk-inspection rendering aligns its four-rectangle outline to signed chunk bounds and suppresses the overlay when a chunk projects below four screen pixels.
- Re-requesting existing chunks does not advance world revision.
- The viewer world-generation worker returns immutable bootstrap-aware loads independently of the event-loop thread; the hidden runtime smoke proves a worker-produced terrain arrival can merge and render in the same runtime path.

## Known gaps

- GPU adapter/surface creation, shader binding layout, drawing, and presentation are covered by the automated hidden-window smoke run. Resize recovery and interactive event dispatch remain manual runtime checks.
- No property tests, generator checksums, cross-region drainage/tributary tests, save/load tests, or long-running soak tests exist yet. A focused ignored release test measures cold 4,096-chunk worker throughput with a fixed seed and selectable worker count, but it is not yet the complete world-generation benchmark suite.
- Renderer `CameraUniform` and `Instance` sizes have compile-time assertions; broader foundational layout assertions, allocation/resident-memory measurements, frame-time capture, and a canonical multi-workload reporting harness do not exist yet. `Documentation/PERFORMANCE.md` records current representation calculations and those remaining measurement gaps.
- Segmented multi-buffer drawing, interactive automatic pan/zoom generation, a full bootstrap completion run, and an interactive large right-drag generation path are not integration-tested. The hidden GPU smoke covers startup pipeline creation plus the first streamed terrain arrival and presentation.
