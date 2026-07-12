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

No canonical release-mode benchmark report exists yet. The current terrain layout intentionally uses `u16` elevation, `u8` moisture, and a byte-represented `GroundType`, but complete record and collection costs must be measured rather than inferred from field widths.

The viewer no longer rasterizes every framebuffer pixel on the CPU. `wgpu` draws compact 20-byte rectangle instances, with a size-asserted 32-byte camera uniform transformed in the vertex shader. Terrain and feature buffers contain only a camera-bounded rectangle plus a scale-relative reuse margin of approximately 128 screen pixels; camera motion inside that margin updates only the uniform. Zoomed-out extraction deterministically uses power-of-two steps that divide a 64-cell chunk and target roughly two screen pixels per terrain block. Edge blocks are clipped to actual initial/chunk coverage, and static uploads are segmented at 1,000,000 instances per GPU buffer instead of relying on one potentially oversized allocation.

Generated world data is stored in deterministic 64 x 64 chunks keyed by `ChunkCoord`. Cell lookup performs a `BTreeMap` lookup rather than scanning every generated patch. Camera extraction filters the capped retained-chunk map and visits cell/feature data only for intersecting chunks, avoiding traversal across enormous empty coordinate rectangles. Initial-area overlap is split out so boundary chunks neither duplicate nor omit rendered cells.

Selection generation queues only coordinates whose selected portion is not fully covered by the initial rectangle and that have no retained chunk. A dedicated worker thread returns 64 x 64 chunks through a 64-message bounded channel. The main thread applies at most 16 chunks per frame without redrawing an unchanged paused scene, while GPU world-buffer synchronization is deferred until completion to avoid repeated full visible-cache uploads. A worker job may materialize at most 4,096 missing chunks (16,777,216 chunk-payload cells), even when its selection footprint is larger because it overlaps loaded terrain; total retained generated data is capped at 16,384 chunks. Missing-chunk validation stops on the 4,097th new chunk, and unchanged preview results are cached by selection bounds plus world revision. `C` stops remaining generation work; chunks already applied remain retained. Rendering uses a persistent 60 Hz deadline even during input bursts and worker polling, and stops when paused with no visual changes.

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
