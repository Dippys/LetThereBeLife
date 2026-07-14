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

The current terrain layout intentionally uses `u16` elevation, `u8` moisture, and a one-byte `TerrainClass` packing a `SurfaceType` low nibble with a `BiomeType` high nibble; unit assertions fix `TerrainClass` at 1 byte and `TerrainCell` at 4 bytes. The 16,777,216-cell bootstrap ceiling therefore still permits a 64 MiB logical cell payload before tile metadata and sparse features, and the complete 4,294,967,296-cell envelope remains exactly 16 GiB of raw terrain. `Engine::new` owns zero terrain cells; the viewer retains only streamed clipped bootstrap/full expansion tiles, while headless explicitly chooses the cost of completely materializing its configured rectangle. Temperature remains derived rather than adding it to every cell: the public `ClimateSample` is four bytes and is built allocation-free from four analytic temperature nodes, the retained moisture byte, and one wind-direction byte. Slice 3 adds no retained per-cell, chunk, or regional payload.

### World-quality baseline

World-foundation Slice 0 uses this exact release workload:

```powershell
cargo build --release -p sim-core --example render_map
target/release/examples/render_map.exe --review-set --out-dir target/world-quality
```

The workload emits four 512 x 512 full-envelope views plus eight fixed focused views, 4,456,448 sampled cells total. Three fresh-process runs on 2026-07-14 used `rustc 1.96.1 (31fca3adb 2026-06-26)` on the same 16-logical-CPU machine as the generation-pool measurements:

| Run | Elapsed | Peak working set |
| ---: | ---: | ---: |
| 1 | 12,139.6 ms | 34.2 MiB |
| 2 | 12,206.6 ms | 34.7 MiB |
| 3 | 11,499.6 ms | 33.8 MiB |
| **Median** | **12,139.6 ms** | **34.2 MiB** |

The measurement includes regional derivation, sampling, pixel buffers, distribution/hash accounting, 12 BMP writes, and four TSV reports. It excludes Cargo compilation and does not construct or retain a `World`. Peak working set is an operating-system process observation polled every 25 ms, not allocator attribution. All per-view semantic hashes were identical across the three processes.

Each full-envelope view samples 262,144 fixed coordinates at a 128-cell step. This is the recorded baseline distribution, not a quality threshold:

| Seed | Deep water | Shallow water | Sand | Grass | Forest floor | Hill | Bare rock | Features / 10k samples |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 49.83% | 4.41% | 3.54% | 36.03% | 3.25% | 1.99% | 0.94% | 24.60 |
| 7 | 69.42% | 2.84% | 6.93% | 15.85% | 3.68% | 0.83% | 0.45% | 22.89 |
| 42 | 49.85% | 4.67% | 12.72% | 25.76% | 5.15% | 0.94% | 0.92% | 34.71 |
| 10,001 | 57.04% | 3.94% | 8.80% | 23.22% | 5.31% | 1.39% | 0.30% | 35.78 |

The review-format-2 `representation.tsv` currently records: `SurfaceType` 1 byte/alignment 1, `BiomeType` 1/1, `TerrainClass` 1/1, `TerrainCell` 4/2, `PrevailingWind` 1/1, `ClimateSample` 4/2, `FeatureKind` 1/1, `Feature` 24/8, `GeneratedCell` 6/2, and `ChunkCoord` 16/8. These are complete Rust record sizes, not sums of field widths. The existing regional-cache calculation below remains the relevant retained derivation-cache baseline.

### Cross-region drainage skeleton

The candidate command is the ignored release test documented in `TESTING.md`, run in a fresh process for each `SIM_DRAINAGE_STEP`. Seed 1 on 2026-07-14 produced:

| Step | Grid | Build time | Lakes | Channel links | Render segments | Retained logical bytes | Scratch logical upper bound |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 128 | 513 x 513 | 175.9 ms | 187 | 6,966 | 27,864 | 1,918,704 | 17,369,154 |
| **256** | **257 x 257** | **44.3 ms** | **167** | **3,832** | **30,656** | **1,109,240** | **4,359,234** |

Step 256 remains implemented. Both resolution candidates sample the expensive upwind moisture field every fourth drainage node and bilinearly interpolate it before flow routing. Step 256 retains the same complete-envelope basin/outlet model at roughly one quarter of the measured build time and scratch payload. The initial Slice 4 attempts exposed the runoff graph by threshold: 70,000 retained 13,653 seed-1 links and 220,000 still retained 4,479. The 2026-07-15 implementation instead retains at most 24 spatially separated exact lake-outlet paths and gives dry nodes zero runoff. Fresh release processes for seeds 1, 7, 42, and 10,001 retained 8/4/15/15 sources, 87/33/131/144 links, and 696/264/1,048/1,152 subdivided segments. Observed fresh builds took 51.3/51.0/60.2/51.0 ms. Source selection raises the conservative logical scratch upper bound to 4,425,283 bytes; this assumes the impossible worst case of one candidate tuple per node. These are observations, not latency regression thresholds.

`DrainageSegment` is size-asserted at 28 bytes after adding two `u16` longitudinal surface endpoints; `ChannelLink` remains 28 bytes, `LakeDescriptor` remains 12 bytes, and `RiverSource` is 8 bytes. The selected skeleton also retains two 66,049-entry `u16` arrays for filled surface and lake depth. Seeds 1, 7, 42, and 10,001 retain 288,188, 273,240, 299,272, and 302,428 logical bytes respectively, or 1,163,128 bytes together. This is 3,485,464 bytes less than the superseded four-seed Slice 4 selection despite the graded segment growing by four bytes. Recalculating the deliberately unrealistic all-nodes/eight-segments-per-node four-cache ceiling for the wider segment plus 24 sources gives 70,805,296 bytes. These numbers exclude `Arc`, boxed-slice/cache metadata, and allocator overhead.

The current scratch figure is an explicit upper bound for the widest source-selection overlap: canonical elevation/fill/target/accumulation/basin/lake arrays, lake-size work, the selection mask, and at most one candidate tuple per node. It excludes output-vector spare capacity during construction, allocator metadata, Rayon stacks, and thread-local plate/climate caches; the fresh-process working-set measurement below captures those costs together but does not attribute them.

The transient per-chunk fast array contains thirteen 28-byte `RiverSegment` entries (364 bytes) plus an empty 24-byte `Vec` header. Every finite-world chunk for the four representative seeds remains allocation-free. If another accepted seed exceeds the measured fast capacity, only that chunk context allocates overflow storage instead of panicking or dropping a river. No retained terrain-field growth was introduced.

Three fresh-process post-Slice-1 canonical review runs used the already-built release executable and separate output directories:

| Run | Elapsed | Peak working set |
| ---: | ---: | ---: |
| 1 | 12,163.2 ms | 36.9 MiB |
| 2 | 11,284.1 ms | 37.4 MiB |
| 3 | 12,547.9 ms | 37.0 MiB |
| **Median** | **12,163.2 ms** | **37.0 MiB** |

Compared with the Slice-0 median (12,139.6 ms and 34.2 MiB), elapsed time was effectively unchanged (+0.2%) and peak working set increased by 2.8 MiB. The memory increase is consistent with retaining four seed skeletons during the four-seed review; this is an OS process measurement, not allocator attribution.

Local release measurements on 2026-07-14, with the repository's `4096 x 4096`, seed-1 configuration, warm build artifacts, and `rustc 1.96.1 (31fca3adb 2026-06-26)`:

| Command | Runs (ms) | Median | Scope |
| --- | ---: | ---: | --- |
| `target/release/sim-viewer.exe --config config/simulation.toml --smoke-frames 2` | 572.6, 529.6, 540.3 | 540.3 ms | Hidden window/GPU creation, first pool-produced terrain load, main-thread merge, and two rendered frames after arrival; not full bootstrap completion. |
| `target/release/sim-headless.exe --config config/simulation.toml --ticks 0 --seed 1` | 946.8, 926.7, 943.1 | 943.1 ms | Explicit complete bootstrap materialization with no simulation ticks; this path remains sequential. |

These are local observations, not cross-machine targets or a controlled before/after benchmark. Compared with the immediately preceding measurements in the same checkout history (532.3 ms viewer and 963.5 ms headless medians), neither path shows a material latency regression; the focused pool benchmark below measures the changed computation path directly.

The focused throughput command is `cargo test --release -p sim-viewer generation::tests::release_generation_pool_throughput -- --ignored --nocapture --test-threads=1`. Set `SIM_GENERATION_BENCH_WORKERS` before each invocation and optionally set `SIM_GENERATION_BENCH_WORLD_SIZE`; its default remains 4,096 cells per side. Fresh-process runs used seed 10,001, so every sample started with a cold regional cache:

| Workload | Workers | Runs (ms) | Median | Throughput |
| --- | ---: | ---: | ---: | ---: |
| One 32 x 32 page footprint (1,024 chunks) | 1 | 225.8, 228.7, 215.8 | 225.8 ms | 4,535 chunks/s |
| One 32 x 32 page footprint (1,024 chunks) | 15 | 38.2, 38.3, 39.0 | 38.3 ms | 26,736 chunks/s |
| 4,096 x 4,096 cells (4,096 chunks) | 1 | 890.7, 908.7, 932.6 | 908.7 ms | 4,507 chunks/s |
| 4,096 x 4,096 cells (4,096 chunks) | 15 | 130.5, 133.8, 134.1 | 133.8 ms | 30,613 chunks/s |

On this 16-logical-CPU machine, the normal 15-worker policy was 5.9x faster for a cold 1,024-chunk page and 6.8x faster for the 4,096-chunk workload. Before cold-region preparation and parallel regional fields were added, a same-checkout 15-worker 4,096-chunk sample took 206.6 ms; the new three-run median is 133.8 ms. The test validates output count and terminal status, retains returned payloads, and excludes `Engine` insertion, GPU synchronization, and rendering. It is a focused comparison, not yet a resident-memory or frame-time benchmark.

Each cached 129 x 129 region still retains five `i32` lattices for elevation, canonical lake depth, temperature, moisture, and roughness: about 325 KiB of logical array payload before river segments, box metadata, and temporary build buffers. The process-shared 64-completed-entry cache therefore has about 20.3 MiB of lattice payload at capacity before those extras, rather than that amount per worker. The capacity matches the viewer's maximum prepared task window so a sparse window cannot evict a freshly prepared region before its dependent chunk starts. In-flight build slots are never evicted and can temporarily exceed 64 entries if more distinct regions are concurrently requested outside that viewer boundary. These are representation calculations, not a resident-memory measurement. The separate four-seed skeleton cache is measured above.

Manual requests are capped at 65,536 missing chunks (268,435,456 terrain cells, or 1 GiB logical `TerrainCell` payload). The complete centered envelope contains 1,048,576 chunks and 4,294,967,296 cells: exactly 16 GiB of logical `TerrainCell` payload if every cell is resident. That number is a raw-terrain-area definition, not a process-memory promise; sparse features, `BTreeMap` nodes, chunk metadata, regional derivation caches, and allocator overhead are additional. All generation paths reject coordinates outside `[-32,768, 32,768)` before allocating work, so generating heavily toward one side cannot move or consume a separate count-only boundary.

The viewer no longer rasterizes every framebuffer pixel on the CPU. `wgpu` draws compact 20-byte rectangle instances, with a size-asserted 32-byte camera uniform transformed in the vertex shader. Terrain and feature buffers contain only a camera-bounded rectangle plus a scale-relative reuse margin of approximately 128 screen pixels; camera motion inside that margin updates only the uniform. Zoomed-out extraction deterministically uses power-of-two steps that divide a 64-cell chunk and target roughly two screen pixels per terrain block. Edge blocks are clipped to actual initial/chunk coverage, and static uploads are segmented at 1,000,000 instances per GPU buffer instead of relying on one potentially oversized allocation.

Generated world data is stored in deterministic chunk-keyed tiles. Cell lookup performs a `BTreeMap` lookup rather than scanning every generated patch. Camera extraction visits only intersecting resident tiles, avoiding traversal across configured-but-unloaded or otherwise empty coordinate rectangles. Exact clipped bootstrap generation samples only retained edge cells rather than first materializing a full 64 x 64 tile; it prevents a boundary tile from leaking cells beyond a non-aligned configured edge, and a later full expansion safely replaces that tile.

Selection generation queues only missing authoritative load requests. One persistent coordinator owns a fixed pool of `available_parallelism - 1` Rayon workers, with a one-worker minimum. Before dependent chunk work is dispatched, `sim-core` prepares each bounded window's distinct regional prerequisites through the same pool; independent macro/climate lattice slots execute in parallel, while topology-sensitive priority fill and river extraction remain serial. Active plus completed-but-not-yet-ordered work is limited to four tasks per worker and 64 total; a separate 64-message channel bounds emitted loads, so the combined terrain payload in those two windows remains at most about 2 MiB before features, task metadata, and chunks already applied to the world. The main thread inserts 16-load batches while a 2 ms budget remains and never exceeds 64 loads per frame. Bootstrap demand is represented by a lazy 32 x 32 `ChunkPager`, so no complete bootstrap coordinate vector is retained; each page describes at most 1,024 chunks (16 MiB of eventual logical terrain payload), but only the bounded task/result windows are simultaneously queued as completed payloads. The renderer unions changed loaded bounds, rebuilds only affected visible caches at a 125 ms cadence, skips off-cache uploads, and performs a final synchronization when a page completes. `C` stops dispatch and discards stale completed loads; already-running pure tasks finish and chunks already applied remain retained. Rendering uses a persistent 60 Hz deadline while simulation, generation, or cache synchronization is active and stops when paused with no visual changes.

Camera movement, resize, zoom, and hover perform no terrain-demand allocation, paging, cancellation, or worker submission. The viewer retains no automatic pager, pending-page vector, or viewpoint-dirty flag. Startup bootstrap paging remains bounded, and terrain outside it is requested only by an accepted right drag. This cleanup removes viewpoint-triggered scheduling work without changing the measured chunk-computation path above; no new timing measurement was needed for the deleted path. Chunk inspection performs one `BTreeMap` presence lookup, stores its 0-to-63 local coordinates as two `u8` values, and adds at most four GPU rectangle instances. The ten-instance world-overlay staging vector is allocated once and reused for hover, selection, chunk outline, and four persistent red border rectangles; sub-four-pixel chunk outlines are omitted.

The in-game HUD reuses a 512-byte text string and a fixed 4,096-entry CPU screen-overlay vector; its matching GPU buffer uses the existing size-asserted 20-byte `Instance`. Their logical reserved payloads are 512 bytes, 80 KiB, and 80 KiB respectively before allocator/GPU metadata. Bitmap text is emitted as contiguous horizontal glyph runs rather than one rectangle per lit pixel, and a regression exercises idle, bootstrap, manual, cancelling, unavailable-worker, no-cursor, loaded-cell, outside-world, maximum-number, and maximum-selection layouts against the fixed instance ceiling. This is a representation calculation and capacity invariant, not a frame-time measurement.

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
