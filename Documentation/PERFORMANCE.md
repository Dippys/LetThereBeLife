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
| `target/release/sim-viewer.exe --smoke-frames 2` | 523.4, 532.3, 539.5 | 532.3 ms | Hidden window/GPU creation, first worker-produced terrain load, main-thread merge, and two rendered frames after arrival; not full bootstrap completion. |
| `target/release/sim-headless.exe --ticks 0 --seed 1` | 917.0, 963.5, 971.5 | 963.5 ms | Explicit complete bootstrap materialization with no simulation ticks. |

These are local observations, not cross-machine targets or a controlled before/after benchmark. They do verify the intended startup boundary: the viewer can present first streamed terrain without waiting for the complete configured bootstrap.

Each cached 129 x 129 region currently retains five `i32` lattices for elevation, lake depth, temperature, moisture, and roughness: about 325 KiB of logical array payload before river segments, box metadata, and temporary build buffers. The 40-entry thread-local cache therefore has about 12.7 MiB of lattice payload at capacity before those extras. These are representation calculations, not a resident-memory measurement. Region-major chunk traversal keeps a wide generation request from rebuilding regions after LRU eviction; a fixed 25-slot per-chunk river index avoids per-cell scans of all regional channels.

Manual requests are capped at 4,096 missing chunks (16,777,216 terrain cells, or 64 MiB logical `TerrainCell` payload). Full expansion tiles are independently capped at 16,384 (256 MiB logical terrain payload), both before sparse features and map/container overhead. Bootstrap tiles are clipped to the configured rectangle and do not use expansion capacity; promoting a partial bootstrap tile to a full expansion tile does. These are safety guardrails, not suggested memory budgets.

The viewer no longer rasterizes every framebuffer pixel on the CPU. `wgpu` draws compact 20-byte rectangle instances, with a size-asserted 32-byte camera uniform transformed in the vertex shader. Terrain and feature buffers contain only a camera-bounded rectangle plus a scale-relative reuse margin of approximately 128 screen pixels; camera motion inside that margin updates only the uniform. Zoomed-out extraction deterministically uses power-of-two steps that divide a 64-cell chunk and target roughly two screen pixels per terrain block. Edge blocks are clipped to actual initial/chunk coverage, and static uploads are segmented at 1,000,000 instances per GPU buffer instead of relying on one potentially oversized allocation.

Generated world data is stored in deterministic chunk-keyed tiles. Cell lookup performs a `BTreeMap` lookup rather than scanning every generated patch. Camera extraction visits only intersecting resident tiles, avoiding traversal across configured-but-unloaded or otherwise empty coordinate rectangles. Exact clipped bootstrap generation samples only retained edge cells rather than first materializing a full 64 x 64 tile; it prevents a boundary tile from leaking cells beyond a non-aligned configured edge, and a later full expansion safely replaces that tile.

Selection generation queues only missing authoritative load requests. One persistent worker returns payloads through a 64-message bounded channel; the main thread applies at most 16 loads per frame. Automatic/bootstrap demand is represented by a lazy 8 x 8 chunk pager, so no full zoomed-out coordinate vector is retained. The renderer unions changed loaded bounds, rebuilds only affected visible caches at a 125 ms cadence, skips off-cache uploads, and performs a final synchronization when a page completes. `C` stops remaining work; chunks already applied remain retained. Rendering uses a persistent 60 Hz deadline while simulation, generation, or cache synchronization is active and stops when paused with no visual changes.

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
