# Performance and Footprint

Last synchronized: 2026-07-14.

## Principle

Minimize runtime work, memory, allocations, cache misses, and stored data while preserving correctness, determinism, safety, and understandable invariants. Fewer source lines are desirable only when competing implementations are otherwise equal; source-code golfing is not an optimization.

## Optimization order

1. Avoid unnecessary work through event scheduling, locality, bounded searches, and suitable algorithms.
2. Avoid unnecessary state through sparse records, consolidation, interning, and derived values.
3. Reduce allocations and retained capacity; reuse buffers and keep hot data contiguous.
4. Improve cache locality through hot/cold separation and batch processing.
5. Use the smallest proven representation for IDs, counters, flags, weights, and coordinates.
6. Tune individual operations only after profiling identifies a meaningful hot path.

## Representation rules

- Document the valid range and overflow behavior before narrowing a type.
- Measure `size_of`, alignment, container capacity, and allocator overhead together.
- Prefer compact integer handles over pointers and object graphs for persistent simulation data.
- Quantize continuous values only with accuracy and determinism tests.
- Keep universal hot records fixed and compact; move optional variable state into sparse pools.
- Add size assertions for foundational records and benchmark representative distributions before committing budgets.

## Current measurements

The current terrain layout intentionally uses `u16` elevation, `u8` moisture, and a byte-represented `GroundType`; a unit assertion fixes `TerrainCell` at 4 bytes. A fully materialized 16,777,216-cell bootstrap therefore has a 64 MiB logical cell payload before tile metadata and sparse features. `Engine::new` now owns zero terrain cells; the viewer retains only streamed clipped bootstrap/full expansion tiles, while headless explicitly chooses the full bootstrap cost.

Local release measurements on 2026-07-14, with the repository's `4096 x 4096`, seed-1 configuration, warm build artifacts, and `rustc 1.96.1 (31fca3adb 2026-06-26)`:

| Command | Runs (ms) | Median | Scope |
| --- | ---: | ---: | --- |
| `target/release/sim-viewer.exe --config config/simulation.toml --smoke-frames 2` | 572.6, 529.6, 540.3 | 540.3 ms | Hidden window/GPU creation, first pool-produced terrain load, main-thread merge, and two rendered frames after arrival; not full bootstrap completion. |
| `target/release/sim-headless.exe --config config/simulation.toml --ticks 0 --seed 1` | 946.8, 926.7, 943.1 | 943.1 ms | Explicit complete bootstrap materialization with no simulation ticks; this path remains sequential. |

These are local observations, not cross-machine targets or a controlled before/after benchmark. Compared with the immediately preceding measurements in the same checkout history (532.3 ms viewer and 963.5 ms headless medians), neither path shows a material latency regression; the focused pool benchmark below measures the changed computation path directly.

The focused throughput command is `cargo test --release -p sim-viewer generation::tests::release_generation_pool_throughput -- --ignored --nocapture --test-threads=1`. Set `SIM_GENERATION_BENCH_WORKERS` before each invocation. Six fresh-process runs used seed 10,001 and the same 4,096 opaque chunk-load requests, so every sample started with a cold regional cache:

| Workers | Runs (ms) | Median | Throughput |
| ---: | ---: | ---: | ---: |
| 1 | 895.7, 893.3, 900.9 | 895.7 ms | 4,573 chunks/s |
| 15 | 187.5, 184.4, 186.1 | 186.1 ms | 22,010 chunks/s |

On this 16-logical-CPU machine, the normal 15-worker policy was 4.8x faster for the bounded 4,096-chunk generation-and-channel workload. The test validates output count and terminal status, retains returned payloads, and excludes `Engine` insertion, GPU synchronization, and rendering. It is a focused comparison, not yet a resident-memory or frame-time benchmark.

Each cached 129 x 129 region currently retains five `i32` lattices for elevation, lake depth, temperature, moisture, and roughness: about 325 KiB of logical array payload before river segments, box metadata, and temporary build buffers. The process-shared 40-completed-entry cache therefore has about 12.7 MiB of lattice payload at capacity before those extras, rather than that amount per worker. In-flight build slots are never evicted and can temporarily exceed 40 entries if more than 40 distinct regions are concurrently building; the viewer's task window caps concurrent chunk work at 64, and region-major request order normally keeps adjacent tasks in one region. These are representation calculations, not a resident-memory measurement. A fixed 25-slot per-chunk river index avoids per-cell scans of all regional channels.

Manual requests are capped at 4,096 missing chunks (16,777,216 terrain cells, or 64 MiB logical `TerrainCell` payload). Full expansion tiles are independently capped at 16,384 (256 MiB logical terrain payload), both before sparse features and map/container overhead. Bootstrap tiles are clipped to the configured rectangle and do not use expansion capacity; promoting a partial bootstrap tile to a full expansion tile does. These are safety guardrails, not suggested memory budgets.

The viewer no longer rasterizes every framebuffer pixel on the CPU. `wgpu` draws compact 20-byte rectangle instances, with a size-asserted 32-byte camera uniform transformed in the vertex shader. Terrain and feature buffers contain only a camera-bounded rectangle plus a scale-relative reuse margin of approximately 128 screen pixels; camera motion inside that margin updates only the uniform. Zoomed-out extraction deterministically uses power-of-two steps that divide a 64-cell chunk and target roughly two screen pixels per terrain block. Edge blocks are clipped to actual initial/chunk coverage, and static uploads are segmented at 1,000,000 instances per GPU buffer instead of relying on one potentially oversized allocation.

Generated world data is stored in deterministic chunk-keyed tiles. Cell lookup performs a `BTreeMap` lookup rather than scanning every generated patch. Camera extraction visits only intersecting resident tiles, avoiding traversal across configured-but-unloaded or otherwise empty coordinate rectangles. Exact clipped bootstrap generation samples only retained edge cells rather than first materializing a full 64 x 64 tile; it prevents a boundary tile from leaking cells beyond a non-aligned configured edge, and a later full expansion safely replaces that tile.

Selection generation queues only missing authoritative load requests. One persistent coordinator owns a fixed pool of `available_parallelism - 1` Rayon workers, with a one-worker minimum. Active plus completed-but-not-yet-ordered work is limited to two tasks per worker and 64 total; a separate 64-message channel bounds emitted loads, so the combined terrain payload in those two windows is at most about 2 MiB before features, task metadata, and chunks already applied to the world. The main thread inserts 16-load batches while a 2 ms budget remains and never exceeds 64 loads per frame. Automatic/bootstrap demand is represented by a lazy 8 x 8 chunk pager, so no full zoomed-out coordinate vector is retained. The renderer unions changed loaded bounds, rebuilds only affected visible caches at a 125 ms cadence, skips off-cache uploads, and performs a final synchronization when a page completes. `C` stops dispatch and discards stale completed loads; already-running pure tasks finish and chunks already applied remain retained. Rendering uses a persistent 60 Hz deadline while simulation, generation, or cache synchronization is active and stops when paused with no visual changes.

Automatic visible-area generation re-paginates after startup, resize, zoom, and camera movement; a new view cancels stale automatic/bootstrap work before it can fill a distant cache. Each page is small enough to remain under core's request guardrails, while capacity errors remain explicit. Chunk inspection performs one `BTreeMap` presence lookup, stores its 0-to-63 local coordinates as two `u8` values, and adds at most four GPU rectangle instances. The six-instance world-overlay staging vector is allocated once and reused; sub-four-pixel chunk outlines are omitted.

## Required measurement conditions

- Use release builds and a recorded compiler version.
- Use fixed seeds and identical workloads for comparisons.
- Report elapsed time, throughput, resident memory, logical payload bytes, retained capacity, and relevant type sizes.
- Include before/after results and the command used.
- Reject optimizations that alter deterministic output unless the change is intentional and documented.

## Open budgets

- Maximum hot agent-core size.
- Terrain bytes per loaded cell and per chunk.
- Sparse feature bytes per record.
- Event scheduler bytes per scheduled event.
- Allocations and generation time per world chunk.
- Release binary size and startup-time targets.
- Maximum cached visible GPU instance count and upload-time budget.
