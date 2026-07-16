# Performance and Footprint

Last synchronized: 2026-07-16.

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

### Physical-agent Slice 0

The foundational layouts are unit-size-asserted: `AgentId` is 4 bytes/alignment 4, private `CompactPosition` is 4 bytes, `AgentActivity` is 1 byte, the complete hot `AgentRecord` is 6 bytes/alignment 2, and `ScheduledEvent` is 32 bytes/alignment 8. Stable ID is implicit in dense slot order and therefore consumes no bytes in `AgentRecord`; the parallel stale-event generation costs 4 bytes per agent. `MovementEventOutcome` is a cold 64-byte diagnostic record retained only in a reusable buffer capped at 4,096 entries.

The ignored release harness uses `rustc 1.96.1 (31fca3adb 2026-06-26)` and this command:

```powershell
cargo test --release -p sim-core tests::release_physical_agent_slice_zero_measurement -- --ignored --nocapture --test-threads=1
```

On 2026-07-16 it initialized seed-42 agents inside a resident 512 x 512 rectangle, then averaged fixed scheduler batches over 10,000 repetitions for 20 agents, 2,000 for 100, and 50 for 10,000. Timed insertion fills one pre-reserved event per agent; rescheduling pushes a second generation per agent; due extraction removes both generations. Nanosecond observations are local optimized CPU timings, not cross-machine regression limits:

| Population | Record/gen/event/outcome capacities | Retained logical bytes | Retained buffers | Insert batch | Insert growth allocations | Reschedule batch | Reschedule growth allocations | Extract 2x events |
| ---: | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 20 | 20 / 20 / 20 / 20 | 2,120 | 4 | 71 ns | 0 | 161 ns | 1 | 564 ns |
| 100 | 100 / 100 / 100 / 100 | 10,600 | 4 | 275 ns | 0 | 528 ns | 1 | 4,391 ns |
| 10,000 | 10,000 / 10,000 / 10,000 / 4,096 | 682,144 | 4 | 28,466 ns | 0 | 46,418 ns | 1 | 1,083,352 ns |

Logical bytes include reserved `AgentRecord`, `u32` generation, `ScheduledEvent`, and cold outcome payload capacities. They exclude four `Vec`/heap headers, allocator metadata, the population's active rectangle, engine/world fields, initialization scratch, and any spare capacity beyond the reported exact reservations. "Retained buffers" is the structural allocation count after initialization. Growth allocations count observed capacity changes during each timed scheduler phase; initial insertion performs none because this Slice 0 harness reserves event storage to population size, while the artificial all-agent reschedule batch grows the heap once. Slice 2 now recognizes up to four current need events plus movement per agent and compacts stale work at five times population plus 4,096 entries (minimum 64).

### Physical-agent Slice 1

The spatial/index layouts are unit-size-asserted: private `CellOccupant` is 8 bytes/alignment 4, private `RouteState` is 6 bytes, `Option<RouteState>` is 8 bytes, and reusable Dijkstra node/frontier records are each 12 bytes/alignment 4. Occupancy uses one compact entry per agent inside a chunk bucket rather than a dense active-area grid; the optional route array is parallel to the population. Perception is capped at radius 31, or 3,969 cells away from active-area edges, and allocates only bounded returned fact vectors. The route planner retains and clears three shared scratch collections rather than allocating a path per agent.

The ignored release harness uses this command:

```powershell
cargo test --release -p sim-core tests::release_physical_agent_slice_one_measurement -- --ignored --nocapture --test-threads=1
```

On 2026-07-16, seed 42 used a resident 512 x 512 rectangle. Agent 0 began near an active-area corner, so its radius-31 perception clipped to 1,024 cells. Nanosecond observations are local optimized CPU timings, not regression limits:

| Population | Spatial entry capacity | Nonempty buckets | Spatial + route logical bytes | Returned agents | Perception time |
| ---: | ---: | ---: | ---: | ---: | ---: |
| 20 | 32 | 1 | 416 | 20 | 42,700 ns |
| 100 | 128 | 2 | 1,824 | 32 | 27,300 ns |
| 10,000 | 16,384 | 8 | 211,072 | 672 | 35,100 ns |

Logical bytes include retained `CellOccupant` capacity plus one `Option<RouteState>` slot per agent. They exclude `BTreeMap` nodes, per-bucket `Vec` headers, allocator metadata, agent/scheduler storage already reported for Slice 0, and the bounded caller-owned perception result vectors. Population placement is canonical row-major, so bucket count and local density reflect this synthetic initialization rather than an expected settled distribution.

The same harness warmed one sparse-agent route, then repeated its deterministic 75-expansion minimum-travel-time search 1,000 times. It averaged 12,798 ns per search, retained capacities of 128 route nodes, 112 hash slots, and 16 frontier entries, and observed zero buffer-capacity growth after warm-up. The lookup hash table is never iterated and therefore cannot influence route order; explicit heap keys and fixed neighbor order own determinism. Recomputing per waypoint trades bounded repeated work for zero per-agent path allocations. Larger route distributions and allocator-level measurements remain open until the action policy supplies representative destinations.

### Physical-agent Slice 2

The analytical need layout is unit-size-asserted: private `NeedState` is 32 bytes/alignment 8 and remains parallel to the unchanged six-byte hot `AgentRecord`. It contains four `u16` visible values, four signed one-byte rates, four one-byte exact remainders, one shared `SimTime`, one shared `u32` stale-event generation, and a crossed mask plus alignment. The existing typed `ScheduledEvent` remains 32 bytes/alignment 8 after adding need kind/class payload. Idle initialization schedules three positive-rate thresholds per agent; one activity change may reschedule four, while exact analytical evaluation performs no per-tick population scan or allocation.

The ignored release harness uses `rustc 1.96.1 (31fca3adb 2026-06-26)` and this command:

```powershell
cargo test --release -p sim-core tests::release_physical_agent_slice_two_measurement -- --ignored --nocapture --test-threads=1
```

On 2026-07-16 it created exact-capacity need arrays, reserved the temporary heap to four events per agent, inserted three idle thresholds per agent, changed every state to moving at tick 1, inserted four replacement thresholds per agent, then extracted all seven threshold records per agent at `u64::MAX`. Times are one local optimized observation, not cross-machine regression limits:

| Population | Need/event capacity | Initial events | Rescheduled events | Schedule batch | Extract 7x events | Heap growth buffers | Retained logical bytes |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 20 | 20 / 160 | 60 | 80 | 10,800 ns | 6,700 ns | 1 | 5,760 |
| 100 | 100 / 800 | 300 | 400 | 25,200 ns | 40,100 ns | 1 | 28,800 |
| 10,000 | 10,000 / 80,000 | 30,000 | 40,000 | 1,543,200 ns | 8,498,900 ns | 1 | 2,880,000 |

Logical bytes include retained `NeedState` and final heap capacities only. They exclude agent/spatial/route storage, need-outcome buffers, `Vec`/heap headers, and allocator metadata. The synthetic all-agent activity change proves the current heap doubles once from four to eight events per agent; normal 20-100-agent behavior remains bounded by stale compaction, while the private heap and event density are explicitly temporary before population scaling. Four new threshold records per changed agent is the current worst-case reschedule rate; representative action-policy transition distributions remain a Slice 3 measurement.

### Physical-agent Slice 3

The policy layout is unit-size-asserted: private pointer-free `PolicyState` is 12 bytes/alignment 4 and remains parallel to the unchanged six-byte hot `AgentRecord`. The ten public goal discriminants fit in one byte. Latest-tick `PolicyDiagnostic` is 40 bytes/alignment 8 and population initialization reserves at most two records per agent up to the due-event ceiling, covering the current selection-plus-result maximum without growth for the intended 20-100 agents. Adding distinct action-completion and decision payloads does not grow the 32-byte/alignment-8 `ScheduledEvent`. Normal decisions inspect at most the radius-eight 289-cell objective perception boundary, allocate only its bounded returned fact vectors, and schedule one commitment or one delayed reconsideration; no idle tick scans the population. Retry delay doubles from 60 ticks and caps at 1,920 ticks.

The ignored release harness uses this command:

```powershell
cargo test --release -p sim-core release_physical_agent_slice_three_measurement -- --ignored --nocapture --test-threads=1
```

On 2026-07-16 it created exact-capacity policy arrays and scheduler heaps, inserted one decision per agent at tick 1, then extracted all decisions. Times are one local optimized observation, not cross-machine regression limits:

| Population | Policy/event capacity | Schedule batch | Due extraction | Heap growth buffers | Retained logical bytes |
| ---: | ---: | ---: | ---: | ---: | ---: |
| 20 | 20 / 20 | 200 ns | 1,700 ns | 0 | 880 |
| 100 | 100 / 100 | 700 ns | 2,500 ns | 0 | 4,400 |
| 10,000 | 10,000 / 10,000 | 149,300 ns | 650,100 ns | 0 | 440,000 |

Logical bytes include retained `PolicyState` and decision-event capacities only. They exclude agent/need/spatial/route storage, heap and vector headers, allocator metadata, bounded perception results, and latest-tick diagnostics. This isolates the 44-byte per-agent policy-plus-one-event startup footprint. Real 20-100-agent mixes will also retain need thresholds and transient stale decisions/actions; scheduler compaction remains the guardrail. Slice 4 now supplies representative resource effects separately below.

### Physical-agent Slice 4

`InventoryView` is unit-size-asserted at three bytes/alignment one: one `u8` each for food, wood, and stone with an explicit 32-unit cap per kind. Population initialization reserves the parallel inventory vector exactly, so the measured 20/100/10,000-agent workloads retain 60/300/30,000 logical bytes with capacity equal to population and no vector growth. The logical changed-resource record is six bytes/alignment two: a checked compact `i16` position pair plus `u16` remaining capacity. Actual sparse storage uses `BTreeMap`, so node pointers, balancing metadata, allocator headers, and fragmentation are additional and deliberately not hidden in the six-byte payload figure. Unmodified features retain no map entry; repeated updates to one feature retain one entry, including at zero.

The ignored release harness uses this command:

```powershell
cargo test -p sim-core --release release_physical_agent_slice_four_measurement -- --ignored --nocapture
```

On 2026-07-16, the optimized local harness selected one 120-unit wood feature and performed 120 one-unit gather mutations in 9,100 ns total while retaining exactly one delta record. This is one machine-local observation, not a throughput regression threshold; it isolates composed capacity lookup plus sparse update and excludes perception, routing, action scheduling, inventory transfer, and allocator-level instrumentation. The public contention scenario separately proves that two equal-time gatherers cannot exceed one 12-unit berry capacity and that only one sparse delta is retained. Broader 20-100-agent action distributions and allocator profiling remain part of the Phase 2 integrated survival/soak slice.

### Physical-agent Slice 5

`SleepState` is unit-size-asserted at 24 bytes/alignment eight. It is a fixed pointer-free parallel record containing two `SimTime` values, one-byte quality, and an active flag; location and wake generation are not duplicated from the existing agent/policy state. Exact population reservation therefore retains 480, 2,400, and 240,000 logical bytes for 20, 100, and 10,000 agents. The synthetic worst-concentration workload starts every agent sleeping at the same tick. It retains seven scheduler records per agent: three superseded idle thresholds, three current sleeping thresholds, and one wake. Ordinary runs distribute due times, and stale compaction remains bounded by the existing engine policy.

The ignored optimized harness uses this command:

```powershell
cargo test --release -p sim-core release_physical_agent_slice_five_measurement -- --ignored --nocapture --test-threads=1
```

One run on 2026-07-16 recorded:

| Agents | Sleep capacity | Logical sleep bytes | Scheduled records | Schedule all | Extract equal-time wakes |
| ---: | ---: | ---: | ---: | ---: | ---: |
| 20 | 20 | 480 | 140 | 4,500 ns | 1,000 ns |
| 100 | 100 | 2,400 | 700 | 11,800 ns | 5,200 ns |
| 10,000 | 10,000 | 240,000 | 70,000 | 1,213,600 ns | 876,600 ns |

The timings are one machine-local optimized observation, not regression thresholds. They isolate state transition, analytical threshold/wake insertion, and due extraction; they exclude world validation, policy selection, diagnostics retention, allocator attribution, and the complete tick dispatcher. Sleep performs no per-tick population scan. The integrated Slice 8 soak must measure mixed sleep/wake/interrupt distributions and determine whether the private binary heap should move to buckets before population scale increases.

The current terrain layout intentionally uses `u16` elevation, `u8` moisture, and a one-byte `TerrainClass` packing a `SurfaceType` low nibble with a `BiomeType` high nibble; unit assertions fix `TerrainClass` at 1 byte and `TerrainCell` at 4 bytes. The 16,777,216-cell bootstrap ceiling therefore still permits a 64 MiB logical cell payload before tile metadata and sparse features, and the complete 4,294,967,296-cell envelope remains exactly 16 GiB of raw terrain. `Engine::new` owns zero terrain cells; the viewer retains only streamed clipped bootstrap/full expansion tiles, while headless explicitly chooses the cost of completely materializing its configured rectangle. Temperature remains derived rather than adding it to every cell: the public `ClimateSample` is four bytes and is built allocation-free from four analytic temperature nodes, the retained moisture byte, and one wind-direction byte. Slice 7 likewise adds no retained world state: `WaterSource` and `TraversalKind` are one byte, `TraversalStep` is eight bytes, and point/step queries allocate nothing. A traversal query performs two chunk-map lookups plus at most one target-feature binary search; settlement searches remain explicitly bounded caller work until the physical-agent loop provides a measured batching need.

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

The review-format-3 `representation.tsv` currently records: `SurfaceType` 1 byte/alignment 1, `BiomeType` 1/1, `TerrainClass` 1/1, `TerrainCell` 4/2, `PrevailingWind` 1/1, `ClimateSample` 4/2, `FeatureKind` 1/1, `Feature` 24/8, `ResourceKind` 1/1, `BaseResource` 4/2, `GeneratedCell` 6/2, and `ChunkCoord` 16/8. `BaseResource` is derived and returned by value, so Stage 5 adds no retained bytes per feature. These are complete Rust record sizes, not sums of field widths. The existing regional-cache calculation below remains the relevant retained derivation-cache baseline.

### Surface-feature ecology

The Stage 5 review extends the canonical workload from 12 to 14 views by adding two full-resolution 512 x 512 probes. One already-built release run on 2026-07-15 rendered the 14 views in 16.39 seconds; this is a visual-review observation, not a benchmark distribution or regression limit. The four 128-cell full-envelope samples retained 812, 723, 1,355, and 1,356 features for seeds 1, 7, 42, and 10,001, or 30.97, 27.58, 51.68, and 51.72 features per 10,000 sampled coordinates. Coarse aligned sampling is not an estimate of full-resolution density.

The three full-resolution 512 x 512 feature probes each cover 64 chunks and provide the record-footprint measurement required by Slice 5:

| Probe | Trees | Rocks | Berry bushes | Features | Average records/chunk | Average logical feature bytes/chunk |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Forest, seed 42 | 15,877 | 69 | 2,948 | 18,894 | 295.22 | 7,085.25 |
| Outcrop, seed 7 | 0 | 10,164 | 0 | 10,164 | 158.81 | 3,811.50 |
| Berry coast, seed 1 | 0 | 46 | 2,366 | 2,412 | 37.69 | 904.50 |

Logical bytes multiply the asserted 24-byte `Feature` size by exact retained records and divide by 64. They exclude `Vec` capacity, allocator metadata, and tile/map metadata; actual chunks vary because patches are intentionally spatially uneven. Canopy, grove, berry-patch, and outcrop bands reuse the local-detail value already computed for terrain synthesis, so Stage 5 adds no noise call or retained patch field. Future sparse depletion records remain unimplemented and therefore have no measured payload yet.

The focused release pool workload compared the final Stage 5 tree with an isolated archive of the immediately preceding revision. That revision's mismatched D-033 refinement helper signature was adapted to its callers in the archive only so it would compile; no feature code changed there. The final tree removes the same unused helper context and dead `primary_upstream` scratch while preserving its curve strengths and output. Both sides used seed 10,001, a cold 2,048 x 2,048 footprint (1,024 chunks), the same release profile, and three fresh test processes per worker count:

| Revision | Workers | Runs (ms) | Median |
| --- | ---: | ---: | ---: |
| Pre-Stage-5 feature placement plus compile repair | 1 | 329.1, 323.4, 327.3 | 327.3 ms |
| Stage 5 | 1 | 318.7, 315.0, 319.9 | 318.7 ms |
| Pre-Stage-5 feature placement plus compile repair | 15 | 114.4, 110.7, 130.8 | 114.4 ms |
| Stage 5 | 15 | 117.0, 119.3, 122.3 | 119.3 ms |

The one-worker median improved 2.6% and the 15-worker median regressed 4.3%, both within the observed local run spread rather than evidence of a material Stage 5 cost. The important implementation constraint is structural: feature patching reuses the two local-detail samples already required for terrain and collapses the three kind rolls into different bit ranges of one coordinate hash. The workload includes canonical drainage/regional preparation and chunk payload construction, so it is not feature-only attribution.

### Multi-scale renderer summaries

Slice 6 replaces coarse coordinate sampling with one active viewer-owned per-chunk summary level. Each retained block owns one 20-byte base `Instance`, optionally one 20-byte minority-terrain detail, and optionally one 20-byte density-scaled feature marker. Retained CPU summary vectors request `shrink_to_fit`; on the recorded allocator their measured capacities equal their instance lengths and the same logical byte count is uploaded to GPU buffers. The table reports that per-copy payload, not the combined CPU-plus-GPU total. It excludes each chunk's two `Vec` headers, `BTreeMap` nodes, allocator metadata, wgpu buffer metadata/alignment, and driver allocations. Summary construction uses a transient 12-byte `VisualSample` for each of 15 terrain classes; the complete per-block `SummaryAccumulator` is size-asserted at 186 bytes and released after the chunk's instances are built. Only one step is retained, and only chunks intersecting the camera rectangle plus its approximately 128-screen-pixel margin remain cached.

The focused release command is documented in `TESTING.md`. On 2026-07-15, a fully resident 4,096 x 4,096 seed-1 rectangle on the recorded 16-logical-CPU machine produced:

| Step | Chunks | Terrain instances | Feature instances | Logical payload per CPU/GPU copy | Summary build |
| ---: | ---: | ---: | ---: | ---: | ---: |
| 2 | 4,096 | 4,194,663 | 20,482 | 84,302,900 bytes | 523.499 ms |
| 4 | 4,096 | 1,048,840 | 18,652 | 21,349,840 bytes | 23.660 ms |
| 8 | 4,096 | 262,289 | 13,517 | 5,516,120 bytes | 17.494 ms |
| 16 | 4,096 | 65,615 | 6,011 | 1,432,520 bytes | 16.883 ms |
| 32 | 4,096 | 16,423 | 2,137 | 371,200 bytes | 15.240 ms |
| 64 | 4,096 | 4,117 | 839 | 99,120 bytes | 15.020 ms |

The test deliberately rebuilds the complete resident rectangle at every step. Step 2 is not a realistic complete-rectangle camera workload: at that scale, the viewport and margin cover only a fraction of 4,096 cells per axis. It is retained as a high-volume allocation/upload ceiling and also pays the first large parallel-pool use in this ordered run. Steps 8-16 approximate initial-fit levels for common windows; independent chunk construction reduces the previous sequential step-16 observation from 139.257 ms to 16.883 ms while preserving deterministic map insertion and exact output counts.

The real hidden-window release command `SIM_VIEWER_SUMMARY_METRICS=1 target/release/sim-viewer.exe --config config/simulation.toml --smoke-frames 8` observed one streamed step-16 synchronization after 64 chunks arrived: 1,024 terrain instances, 114 feature instances, 22,760 bytes in the CPU cache and the same logical GPU instance payload, 3.305 ms summary construction, 0.040 ms CPU upload enqueue, and 3.345 ms combined synchronization. Buffer creation/upload enqueue timing is a CPU observation, not GPU completion. Timestamp-query completion, allocator attribution, full bootstrap completion, and long interactive frame-hitch distributions remain open; the instrumentation makes repeated representative collection possible without changing authoritative state.

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

The viewer no longer rasterizes every framebuffer pixel on the CPU. `wgpu` draws compact 20-byte rectangle instances, with a size-asserted 32-byte camera uniform transformed in the vertex shader. Terrain and feature buffers contain only a camera-bounded rectangle plus a scale-relative reuse margin of approximately 128 screen pixels; camera motion inside that margin updates only the uniform. Close rendering remains exact. Zoomed-out extraction uses the active power-of-two chunk-summary level targeting roughly two screen pixels per block, preserving bounded minority terrain and feature density rather than sampling one coordinate. Edge blocks are clipped to actual initial/chunk coverage, and static uploads are segmented at 1,000,000 instances per GPU buffer instead of relying on one potentially oversized allocation.

Generated world data is stored in deterministic chunk-keyed tiles. Cell lookup performs a `BTreeMap` lookup rather than scanning every generated patch. Camera extraction visits only intersecting resident tiles, avoiding traversal across configured-but-unloaded or otherwise empty coordinate rectangles. Exact clipped bootstrap generation samples only retained edge cells rather than first materializing a full 64 x 64 tile; it prevents a boundary tile from leaking cells beyond a non-aligned configured edge, and a later full expansion safely replaces that tile.

Selection generation queues only missing authoritative load requests. One persistent coordinator owns a fixed pool of `available_parallelism - 1` Rayon workers, with a one-worker minimum. Before dependent chunk work is dispatched, `sim-core` prepares each bounded window's distinct regional prerequisites through the same pool; independent macro/climate lattice slots execute in parallel, while topology-sensitive priority fill and river extraction remain serial. Active plus completed-but-not-yet-ordered work is limited to four tasks per worker and 64 total; a separate 64-message channel bounds emitted loads, so the combined terrain payload in those two windows remains at most about 2 MiB before features, task metadata, and chunks already applied to the world. The main thread inserts 16-load batches while a 2 ms budget remains and never exceeds 64 loads per frame. Bootstrap demand is represented by a lazy 32 x 32 `ChunkPager`, so no complete bootstrap coordinate vector is retained; each page describes at most 1,024 chunks (16 MiB of eventual logical terrain payload), but only the bounded task/result windows are simultaneously queued as completed payloads. The renderer unions changed loaded bounds, rebuilds only affected visible caches at a 125 ms cadence, skips off-cache uploads, and performs a final synchronization when a page completes. `C` stops dispatch and discards stale completed loads; already-running pure tasks finish and chunks already applied remain retained. Rendering uses a persistent 60 Hz deadline while simulation, generation, or cache synchronization is active and stops when paused with no visual changes.

Camera movement, resize, zoom, and hover perform no terrain-demand allocation, paging, cancellation, or worker submission. The viewer retains no automatic pager, pending-page vector, or viewpoint-dirty flag. Startup bootstrap paging remains bounded, and terrain outside it is requested only by an accepted right drag. This cleanup removes viewpoint-triggered scheduling work without changing the measured chunk-computation path above; no new timing measurement was needed for the deleted path. Chunk inspection performs one `BTreeMap` presence lookup, stores its 0-to-63 local coordinates as two `u8` values, and adds at most four GPU rectangle instances. The ten-instance world-overlay staging vector is allocated once and reused for hover, selection, chunk outline, and four persistent red border rectangles; sub-four-pixel chunk outlines are omitted.

The in-game HUD reuses a 512-byte text string and a fixed 4,096-entry CPU screen-overlay vector; its matching GPU buffer uses the existing size-asserted 20-byte `Instance`. Their logical reserved payloads are 512 bytes, 80 KiB, and 80 KiB respectively before allocator/GPU metadata. Bitmap text is emitted as contiguous horizontal glyph runs rather than one rectangle per lit pixel, and a regression exercises idle, bootstrap, manual, cancelling, unavailable-worker, no-cursor, loaded-cell, outside-world, maximum-number, and maximum-selection layouts against the fixed instance ceiling. This is a representation calculation and capacity invariant, not a frame-time measurement.

### Phase 2 Slice 6 minimal shelters

`sim-core::structures::StructureRecord` and its optional retained slot are size-asserted at 32 bytes/alignment eight. The logical row/column-plus-`StructureId` footprint entry is eight bytes before `BTreeMap` node pointers, allocator metadata, and the separate active-builder index. `ScheduledEvent` remains 32 bytes, so construction reuses the existing action-completion payload rather than adding an event allocation or per-tick progress record. The store is sparse: only started structures allocate records/index entries. Cancelled builds leave compact tombstones to preserve monotonic non-reused identities; cancellation frequency and allocator-attributed map bytes remain later scale budgets.

The focused command was:

```powershell
cargo test --release -p sim-core release_slice6_structure_measurement -- --ignored --nocapture --test-threads=1
```

One optimized run on 2026-07-16 recorded:

| Structures | Retained slots | Slot bytes | Logical live footprint-index bytes | Insert structures + schedule events | Extract equal-time completions |
| ---: | ---: | ---: | ---: | ---: | ---: |
| 20 | 32 | 1,024 | 160 | 14,500 ns | 3,200 ns |
| 100 | 128 | 4,096 | 800 | 17,900 ns | 3,600 ns |
| 10,000 | 16,384 | 524,288 | 80,000 | 1,962,200 ns | 513,800 ns |

The timings include deterministic `BTreeMap` footprint/builder insertion plus binary-heap scheduling, or due extraction respectively. They are single-run observations, not regression limits. Slot bytes include geometric spare `Vec` capacity; index bytes are only the eight-byte logical key/value payload and exclude tree nodes and allocation overhead. The public seed-42 Slice 6 scenario also established the algorithmic recipe boundary: an 8-wood/4-stone candidate could not satisfy bounded radius-eight gathering even with a 512 x 512 bootstrap because tree and outcrop regions were separated, while the implemented eight-wood lean-to completes through two local four-unit gathers without a global search.

## Required measurement conditions

- Use release builds and a recorded compiler version.
- Use fixed seeds and identical workloads for comparisons.
- Report elapsed time, throughput, resident memory, logical payload bytes, retained capacity, and relevant type sizes.
- Include before/after results and the command used.
- Reject optimizations that alter deterministic output unless the change is intentional and documented.

## Open budgets

- Maximum later-slice hot agent-core size beyond the current six-byte Slice 0 record plus four-byte parallel generation.
- Terrain bytes per loaded cell and per chunk.
- Sparse feature bytes per record.
- Scheduler replacement threshold and bucket/timing-wheel budgets beyond the measured 32-byte initial event.
- Allocations and generation time per world chunk.
- Release binary size and startup-time targets.
- Maximum cached visible GPU instance count and upload-time budget.
