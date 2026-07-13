# Performance and Footprint

Last synchronized: 2026-07-13.

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

No canonical release-mode benchmark report exists yet. The current terrain layout intentionally uses `u16` elevation, `u8` moisture, and a byte-represented `GroundType`; a unit assertion fixes `TerrainCell` at 4 bytes. The configured 16,777,216-cell initial terrain therefore has a 64 MiB logical cell payload before vector metadata and sparse features.

A local non-canonical Windows measurement used `rustc 1.96.1`, the repository's seed-1 4,096 x 4,096 configuration, `--ticks 0`, a prebuilt release binary, and a fresh process per sample. The prior 384/128/40 local elevation generator had a 0.915-second median over three warm runs (18.34 million cells/second). Continental oceans plus chunk-cached bounded lake descriptors had a 1.081-second median over five warm runs (15.52 million cells/second), an 18.1% startup-generation increase. Lake descriptors add no persistent bytes and are resolved once per 64 x 64 chunk rather than per cell. These figures are directional machine-local evidence, not a committed performance budget.

The superseded 8 x 8 coast-anchored river implementation measured a 1.102-second median over five warm runs (15.22 million cells/second): 1.9% above the continental-ocean/lake baseline and 20.4% above the original local elevation generator. A rejected prototype that applied every route segment to every cell in the polyline's large bounding rectangle measured 1.894 seconds; segment-level chunk filtering removed that unnecessary work before completion.

The relief-following river redesign was measured with the same seed-1 4,096 x 4,096 release workload. A same-turn five-run snapshot of the superseded implementation had a 1.048-second median, while the final corridor-clearance build had a 1.001-second median over seven warm runs (16.77 million cells/second). Concurrent desktop load changed during the session, so these values establish that no obvious startup regression appeared but are not a controlled optimization claim. Each chunk now derives a fixed 16 x 16 pre-lake continental-elevation lattice, ranks at most eight bounded outlet paths in a fixed array, checks candidates in rank order for nonlocal width-expanded clearance, refines accepted edges through five fixed terrain probes, smooths into at most 58 points, and keeps only width-expanded segments intersecting the chunk. The configured seed accepts one startup-area route instead of the superseded two. The redesign remains allocation-free and adds no persistent bytes; `TerrainCell` remains four bytes. These figures remain directional machine-local evidence rather than a committed budget.

The viewer no longer rasterizes every framebuffer pixel on the CPU. `wgpu` draws compact 20-byte rectangle instances, with a size-asserted 32-byte camera uniform transformed in the vertex shader. Terrain and feature buffers contain only a camera-bounded rectangle plus a scale-relative reuse margin of approximately 128 screen pixels; camera motion inside that margin updates only the uniform. Zoomed-out extraction deterministically uses power-of-two steps that divide a 64-cell chunk and target roughly two screen pixels per terrain block. Edge blocks are clipped to actual initial/chunk coverage, and static uploads are segmented at 1,000,000 instances per GPU buffer instead of relying on one potentially oversized allocation.

Generated world data is stored in deterministic 64 x 64 chunks keyed by `ChunkCoord`. Cell lookup performs a `BTreeMap` lookup rather than scanning every generated patch. Camera extraction filters the capped retained-chunk map and visits cell/feature data only for intersecting chunks, avoiding traversal across enormous empty coordinate rectangles. Initial-area overlap is split out so boundary chunks neither duplicate nor omit rendered cells.

Selection generation queues only coordinates whose selected portion is not fully covered by the initial rectangle and that have no retained chunk. A dedicated worker thread returns 64 x 64 chunks through a 64-message bounded channel. The main thread applies at most 16 chunks per frame without redrawing an unchanged paused scene, while GPU world-buffer synchronization is deferred until completion to avoid repeated full visible-cache uploads. A worker job may materialize at most 4,096 missing chunks (16,777,216 chunk-payload cells), even when its selection footprint is larger because it overlaps loaded terrain; total retained generated data is capped at 16,384 chunks. Missing-chunk validation stops on the 4,097th new chunk, and unchanged preview results are cached by selection bounds plus world revision. `C` stops remaining generation work; chunks already applied remain retained. Rendering uses a persistent 60 Hz deadline even during input bursts and worker polling, and stops when paused with no visual changes.

Automatic visible-area generation reuses those same limits and performs its missing-chunk scan only on startup, resize, zoom, left-drag pan release, or successful generation completion rather than every render frame or pointer event. Oversized and over-capacity views are rejected without partial row-major filling. Chunk inspection performs one `BTreeMap` presence lookup, stores its 0-to-63 local coordinates as two `u8` values, and adds at most four GPU rectangle instances. The six-instance world-overlay staging vector is allocated once and reused; sub-four-pixel chunk outlines are omitted.

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
