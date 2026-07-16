# Testing and Validation

Last synchronized: 2026-07-16.

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

The final 2026-07-16 Phase 2 Slice 2 run used the canonical command with `-Runtime`. Immutable-document and eight skill checks passed; workspace tests passed with 109 `sim-core` unit tests (105 passed, four ignored release harnesses), the public one-case `physical_agent_slice0`, three-case `physical_agent_slice1`, three-case `physical_agent_slice2`, and one-case `world_foundation_exit` integrations, eight `render_map` tests, 46 viewer tests (44 passed, two ignored release harnesses), and all remaining crate/doc tests. Formatting, Clippy with warnings denied, the 600-tick/20-agent headless route smoke, and the two-frame hidden GPU viewer smoke all passed.

## Runtime checks

```powershell
cargo run -p sim-headless -- --ticks 600 --seed 42 --agents 20
cargo run -p sim-viewer
```

Expected headless result for the command above:

```text
completed tick=600 simulated_seconds=10.000 seed=42 initial_world=4096x4096 agents=20 routes=20/20 route_failures=0 movements=20 moving=0
```

## Repeatable world-quality review

Build and emit the complete canonical set with one release-mode command from the repository root:

```powershell
cargo run --release -p sim-core --example render_map -- --review-set
```

The command writes ignored derived artifacts under `target/world-quality/`:

- `seed-<seed>/<view>.bmp`: 14 canonical sampled images across seeds 1, 7, 42, and 10,001;
- `review_manifest.tsv`: review format, source revision (with `+dirty` when tracked files differ), view name, seed, half-open bounds, sample step, dimensions, feature-overlay state, semantic sample hash, and relative image path;
- `distribution.tsv`: exact sampled surface, biome, and feature-kind counts, aggregate feature density in parts per million, and the semantic hash;
- `representation.tsv`: `size_of` and alignment for `SurfaceType`, `BiomeType`, `TerrainClass`, `TerrainCell`, `ClimateSample`, `FeatureKind`, `Feature`, `ResourceKind`, `BaseResource`, `GeneratedCell`, and `ChunkCoord`;
- `seed_roles.tsv`: the fixed reason each seed belongs to the set.

The four `full-envelope` views sample `[-32,768, 32,768)` at a 128-cell step. Focused views cover the origin region, an x-axis drainage boundary, a y-axis drainage boundary, coastline, a verified lake-fed river source, its verified mouth class, a mountain belt, and full-resolution forest, outcrop, and berry-bearing coast probes with sparse feature colors. The reports are deterministic for equal generator inputs and source-revision metadata; per-view and total elapsed times are intentionally console-only. `--source-revision TEXT` can replace Git discovery for packaged or controlled comparisons, and `--out-dir PATH` can isolate repeated runs.

These views are review evidence, not pass/fail appearance thresholds. When generator behavior changes intentionally, compare all representative seeds and update hashes only with the implementation and documented decision that caused the change.

The 2026-07-15 Slice 4 river redesign retains review format 2 and intentionally changes semantic hashes to `f95c7be0eb48381c` (seed 1), `5db53acad12b153a` (seed 7), `cd0ab6e48e2ca92b` (seed 42), and `ff4a7c35517124f0` (seed 10,001). Visual inspection rejected both threshold-exposed revisions: 70,000 produced 13,653 seed-1 links and 220,000 still produced 4,479 source-less lattice fragments. The implemented exact canonical-outlet selection retains 87 seed-1 links. Dedicated step-2 source (`a34b230102ffe52e`) and mouth (`2f617c5cfdffb1b8`) views show one continuous curved route leaving a canonical lake and reaching coastal water; a separate full-route step-4 inspection verified that the river no longer forms a cardinal/diagonal comb. Seam views remain continuous, and coast, riparian bank, wetland, biome edge, snowline, and treeline transitions remain coherent. Slice 6 now preserves those minority river/lake/coast signals at coarser viewer steps.

The 2026-07-15 Slice 5 ecology change advances the set to review format 3 and intentionally changes the full-envelope hashes to `5a4bad62b2547347`, `b3af38dc76b677f8`, `e8ca5d39a4619356`, and `8c035e131d5b3ea2` for seeds 1, 7, 42, and 10,001. The full-resolution forest (`62fad9190d558c9d`), outcrop (`bd92176457e7981d`), and berry-bearing coast (`ad24b314ae2e0cba`) views show coherent canopy clearings/edges, dense stone outcrops, and berry patches rather than uniform scatter. The same run records 18,894, 10,164, and 2,412 features across those three 64-chunk probes.

Compare drainage skeleton candidates in isolated release test processes by setting `SIM_DRAINAGE_STEP` to `128` and `256` in turn. `SIM_DRAINAGE_SEED` optionally selects a seed and defaults to 1; `SIM_RIVER_SOURCE_FLOW_THRESHOLD` overrides the lake-source flow requirement for controlled measurements:

```powershell
$env:SIM_DRAINAGE_STEP='256'
cargo test --release -p sim-core worldgen::drainage::tests::compare_candidate_skeleton_steps -- --ignored --nocapture --test-threads=1
```

## Existing automated coverage

- Identical engine inputs yield identical snapshots after 1,000 ticks.
- A paused engine does not advance.
- Reset restores runtime state while preserving engine configuration.
- Physical-agent initialization rejects incomplete residency, blocked/duplicate/out-of-area positions, insufficient standable cells, and repeated initialization without publishing partial population state. Successful initialization preserves requested order, fills canonically, restarts IDs at zero after reset, and keeps terrain residency across reset.
- `AgentId`, compact position, activity, hot agent record, and scheduled event layouts are fixed by size/alignment assertions. Scheduler tests cover the exact time/class/agent/event-detail/sequence order, future-event exclusion, sequence exhaustion without insertion, and a 4,096-event per-tick drain with deterministic backlog.
- Movement tests cover exact integer-cost completion time, pause retention, blocked/non-cardinal/missing/dead/outside-active/outside-world request outcomes, no position/world mutation on rejection, lazy stale duplicate suppression, equal-time ID ordering independent of insertion order, typed simulation-time exhaustion, and bounded read-only views. The public-only `physical_agent_slice0` integration scenario initializes 20 agents, schedules commands in forward and reverse order, and requires identical final views/snapshots and movement counts.
- Slice 1 spatial tests size-check compact bucket entries and route scratch, cover signed `-65/-64/-1/0/63/64` chunk edges, and prove occupied-target or source-mismatch transfers preserve both index entries. Public `physical_agent_slice1` scenarios require request-order-independent equal-time route contention with one `AgentId`-ordered winner, bounded row-major physical perception, distinct budget-exhausted and occupied-corridor no-path outcomes, scheduled arrival, and identical final views/snapshots. The ignored release harness records spatial/route capacities, bounded perception work, route expansions, and warm scratch growth.
- Slice 2 need tests size-check the 32-byte/alignment-eight fixed-point state and unchanged 32-byte threshold event, then cover exact interpolation, positive/negative saturation, ceiling prediction, neutral/opposite rates, time overflow, remainder-preserving activity rebasing, bounded-safe generation wrap, and explicit idle/moving/gathering/building/sleeping profiles. Scheduler tests require threshold-before-movement ordering and same-agent hunger/thirst/rest/exposure priority independent of insertion order. Public `physical_agent_slice2` scenarios prove delayed initialization, 90,000-tick analytical replay with or without intermediate reads, pause/reset determinism, exact movement-to-idle rate transitions, one reached threshold, harmless stale generations, and multi-waypoint routes without idle/restart event churn. A focused engine boundary regression proves global sequence exhaustion leaves a due route agent idle at its unchanged source with a typed route failure. The ignored release harness records 20/100/10,000-agent need/event capacities, worst-case four-event reschedules, heap growth, and due extraction throughput.
- Equal seeds produce equal declared engine worlds; explicit bootstrap materialization reproduces eager deterministic terrain and features, and materializing terrain does not alter fixed tick progression.
- World generation is deterministic and different seeds change terrain.
- Generated samples contain terrain variation plus sparse features.
- A structural fixed-seed overview checks a dominant open ocean, varied continent sizes, elongated connected mountain ranges, and coherent desert, forest, and grass regions.
- Packed terrain-class regressions round-trip every public surface/biome enum combination without collision, keep `TerrainClass` at one byte and `TerrainCell` at four bytes, and prove that equal rendered surfaces can retain distinct beach/desert and grassland/wetland semantics while cold lowlands and cold mountains remain separately typed tundra and alpine snow/ice.
- Climate structure is sampled only inside `WORLD_GENERATION_BOUNDS`. Seeds 1, 7, 42, and 10,001 must each contain at least 3% cold, temperate, and warm qualifying lowland samples on the canonical 256-cell lattice, with cold lowlands represented on both vertical sides across the review set. Separate regressions prove the center is materially warmer than both finite edges and each nominal circulation boundary contains both directions rather than forming a straight seam.
- Signed edge, center, and wind-band probes require `ClimateSample.temperature` to equal the exact 32-cell value used by chunk classification, retain the terrain cell's moisture byte, and remain deterministic on repeat. Existing one/four-worker regional equality and cache-clear chunk equality cover climate output independence from worker count and cache state.
- Whole-envelope drainage checks build seeds 1, 7, 42, and 10,001 and require 1-24 spatially separated explicit sources per seed, a qualifying nonterminal source lake, minimum source flow, fewer than 600 retained links, and a source path to open water. Every retained link has nonzero Strahler order and continues with the same channel identity, joins a declared confluence, enters an identified lake, or reaches ocean/world-edge drainage. All segment water surfaces are non-rising downstream. A synthetic dry/wet graph proves arid cells contribute zero perennial runoff; another fixes equal-order and unequal-order Strahler confluences. A spatially binned complete four-seed regression rejects refined crossings, collinear overlap, and unrelated endpoint contact.
- Adjacent `RegionMap` checks compare every shared x/y seam sample exactly and prove crossed river segments are present on both sides without the former fixed dry margins. A focused chunk regression generates signed seam-adjacent chunks in forward/single-worker and reversed/four-worker order and requires byte-identical results.
- Rasterized samples from flooded regional nodes must remain deep or shallow lake water and cannot emit a sparse surface feature. An overlapping-segment regression proves a deep graded mountain-river core remains above sea level, wins over a shallow bank in both segment orders, and remains typed as river water. Hydrologic wetland tests require a low-slope floodplain or canonical basin edge and reject steep, arid, and drainage-unrelated saturated lowlands. Transition tests cover both sides of bounded beach/forest thresholds and riparian conversion of qualifying dry bank sand to soil. Multi-seed river indexing compares every representative chunk's fast-path segments against exact water-plus-floodplain influence, while a synthetic over-capacity context proves the overflow path preserves every segment.
- Initial generation is compared to independently generated chunks both near the origin and across the 4,096-cell regional boundary. A coordinate-order regression ensures wide chunk spans finish one drainage region before beginning the next, while a cold-cache reset cannot change chunk output. Direct one-worker and four-worker builds compare every retained `DrainageSkeleton` field and every `RegionMap` lattice/river segment exactly.
- Deferred bootstrap construction has zero resident cells while retaining exact configured bounds and unloaded inspection states. Streamed aligned and clipped bootstrap payloads match eager terrain/features even when inserted in a different order; eager materialization is also exercised in multiple bounded batches.
- `SurfaceType`, `BiomeType`, packed `TerrainClass`, `FeatureKind`, and `ResourceKind` are each one byte; `TerrainCell`, the derived `ClimateSample`, and derived `BaseResource` remain four bytes, while `Feature` remains 24 bytes. Surface features remain excluded from water, sand, and snow/ice. Synthetic fixed-seed ecology regressions require forest canopy to contain both clear and dense 32-cell patches, suitable riparian ground to contain more berries than matching dry ground, ordinary soil to expose stone, hills to remain materially richer in rock, deterministic repeated placement, and at least 128 berry bushes in a suitable 512 x 512 probe. Resource mapping tests fix berries to food/12, trees to wood/120, and rocks to stone/80 without mutable state.
- Phase 1 exit-query regressions keep `WaterSource` and `TraversalKind` at one byte and `TraversalStep` at eight bytes. Synthetic resident terrain proves lake/river drinkability versus ocean, slope acceptance at 512 and rejection at 513 elevation units, all-water blocking, tree blocking, berry traversal, resource/no-resource distinction, stable feature identity, and explicit unloaded/outside/non-cardinal errors. The public-only `world_foundation_exit` integration scenario materializes the canonical seed-1 river-mouth region, rejects water/tree/rock starting cells, flood-fills cardinally reachable terrain in a bounded 193 x 193 survey, requires adjacent fresh water plus accessible generated material, and reproduces the selected inputs after regeneration.
- Terrain lookup exposes only resident valid coordinates and rejects configured-but-unloaded or out-of-bounds cells.
- Chunk inspection uses canonical Euclidean coordinates across -65, -64, -1, 0, 63, and 64; it distinguishes full initial coverage, a non-aligned partial-initial boundary before and after its remainder is retained, fully retained chunks, missing chunks, and an unrepresentable positive edge.
- Rectangular initial-area generation preserves configured dimensions and exact cell count, including the maximum-cell one-cell-wide configuration without silently requiring chunk alignment.
- Overlapping configured initial areas generate identical terrain and features at equal world coordinates.
- Invalid zero-sized or excessive initial allocations are rejected.
- `ChunkGenerator` validates unrepresentable chunk coordinates and invalid local cells, while valid samples match `World::generate_chunk_at` terrain and feature output.
- The `render_map` example test harness rejects views outside the finite envelope or pixel budget, proves sampling alignment across signed chunk boundaries, asserts that canonical seam views cross both axes of a 4,096-cell boundary, keeps repository seed 1 in a unique multi-seed set, and verifies byte-stable manifest metadata plus coordinate-sensitive deterministic semantic hashes.
- TOML configuration parsing maps simulation/world settings and rejects invalid world dimensions.
- Resident cell and feature iteration is locked to deterministic chunk-coordinate then tile-local row order; sparse features remain row-major within each loaded tile and support coordinate lookup.
- Exact loaded-region enumeration reports clipped bootstrap/full expansion coverage in stable chunk order, and direct chunk visitors read one known tile without a complete resident-map scan. Viewer summary tests size-check the 12-byte transient `VisualSample` and 186-byte `SummaryAccumulator`, preserve a canonical major river whose cells miss 64-cell block origins, preserve unaligned sparse features as density markers, clip partial coverage exactly, retain only the active power-of-two level and camera-resident chunks, and keep ordinary motion inside the existing cache margin on the no-rebuild path.
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

Run the ignored release summary measurement at the 1,024-cell default or the repository's 4,096-cell configured side:

```powershell
$env:SIM_SUMMARY_BENCH_SIZE='4096'
cargo test --release -p sim-viewer renderer::tests::release_summary_cache_measurement -- --ignored --nocapture --test-threads=1
```

Set `SIM_VIEWER_SUMMARY_METRICS=1` when running `sim-viewer` to emit per-rebuild active step, resident summary chunks, retained logical instance bytes, terrain/feature instance counts, build time, CPU upload-enqueue time, and their combined synchronization time. This instrumentation does not claim GPU completion time.

## Known gaps

- GPU adapter/surface creation, shader binding layout, drawing, and presentation are covered by the automated hidden-window smoke run. Resize recovery and interactive event dispatch remain manual runtime checks.
- No property tests, authoritative full-resolution generator checksums, exhaustive all-`u64` seed topology checks, save/load tests, or long-running soak tests exist yet. Cross-region major/local drainage has deterministic topology, Strahler, non-crossing representative geometry, seam, pool-size, request-order, and complete-envelope index coverage, while the world-quality set records hashes only for its fixed sampled views. Focused ignored release tests measure threshold/skeleton construction and cold chunk generation, but this is not yet the complete world-generation benchmark suite.
- Renderer `CameraUniform` and `Instance` sizes have compile-time assertions, summary scratch records have unit size assertions, `TerrainCell` has a unit size assertion, and the world-quality report records relevant public generation-tool layouts. Broader foundational layout assertions, allocator-level instrumentation, GPU timestamp-query completion, full frame-time distributions, and a canonical multi-workload runtime benchmark harness do not exist yet. `Documentation/PERFORMANCE.md` records current representation calculations and those remaining measurement gaps.
- Segmented multi-buffer drawing, interactive confirmation that pan/zoom remains generation-free, a full bootstrap completion run, and an interactive large right-drag generation path are not integration-tested. The hidden GPU smoke covers startup pipeline creation plus the first streamed terrain arrival and presentation.
