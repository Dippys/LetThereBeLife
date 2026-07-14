# Architecture Decisions

Append new decisions. If a decision changes, add a superseding entry rather than rewriting history.

## D-001: Rust workspace and headless core

Date: 2026-07-12

**Decision:** Use Rust and keep deterministic simulation behavior in a standalone `sim-core` crate.

**Reason:** The simulation must run without rendering for testing, benchmarking, servers, and deterministic replay work.

## D-002: Replaceable bootstrap presentation

Date: 2026-07-12

**Decision:** Confine `winit` and temporary `softbuffer` rendering to `sim-viewer`.

**Reason:** A working native window is useful now, while the production rendering strategy remains replaceable.

## D-003: Immutable initial documentation

Date: 2026-07-12

**Decision:** Treat `InitialDocumentation/` as read-only design input and maintain implementation documentation under `Documentation/`.

**Reason:** The initial design must remain an unchanged baseline while living records evolve with the code.

## D-004: Routed repository skills and enforced quality gate

Date: 2026-07-12

**Decision:** Use focused repository-local skills for orientation, implementation, documentation, validation, review, and workflow evolution, coordinated through `AGENTS.md`. Protect immutable design input with a checksum manifest.

**Reason:** Focused triggers reduce instruction overlap, while a mandatory lifecycle ensures agents inspect context, maintain documentation, review quality, and validate before completion.

**Consequences:** Skill or agent-governance changes must pass the same repository validation gate. Intentional changes to the immutable baseline require explicit user direction and a separately reviewed checksum update.

## D-005: Bounded deterministic starter world

Date: 2026-07-12

**Decision:** Begin with a fully generated 1,024 x 1,024-cell world in `sim-core`. Generate it in 64 x 64 chunk order from integer-only multi-scale noise, store terrain densely, and store surface objects as sparse feature records.

**Reason:** This creates an immediately visible and testable world-generation foundation while preserving deterministic headless ownership and the terrain/feature split needed for future chunk streaming.

**Consequences:** The starter world is allocated at engine construction and is not yet streamed, persisted, or modified. Future large-world work must replace full allocation with on-demand chunks without changing presentation into simulation truth.

## D-007: Initial area is configurable, not the world boundary

Date: 2026-07-12

**Decision:** Supersede the bounded-world interpretation in D-005. Treat `WorldConfig::initial_width` and `initial_height` as the area generated at startup, defaulting to 1,024 x 1,024, while preserving a future path to adjacent on-demand chunks. Load runtime settings through a shared `sim-config` crate and `config/simulation.toml`.

**Reason:** Startup allocation size and total world extent are different concerns. Terrain generation must not create artificial coastlines at the current loaded boundary, and viewer/headless configuration must remain consistent without adding filesystem dependencies to `sim-core`.

**Consequences:** The current engine still holds only the initial rectangle and does not expand at runtime. World-coordinate noise remains continuous beyond its edges. `sim-config` adds `serde` and `toml` dependencies outside the deterministic core.

## D-008: Selection-driven generated-area patches

Date: 2026-07-12

**Decision:** Generate deterministic signed-coordinate terrain patches through right-dragged `WorldRect` commands, capped at 1,048,576 cells per command.

**Reason:** Free camera space becomes actionable while simulation ownership and allocation bounds remain explicit.

**Consequences:** Patches remain loaded, and partially overlapping selections can duplicate cells until chunk-keyed streaming supersedes this bootstrap representation.

## D-012: Streaming chunk generation with no fixed selection cap

Date: 2026-07-12

**Supersedes:** The per-command 1,048,576-cell cap and single-batch worker result in D-008.

**Decision:** Drop the per-selection cell cap from `World::generate_area` and `validate_bounds`. Add `World::generate_chunks_streaming(seed, bounds) -> impl Iterator<Item = WorldChunk>` so the viewer worker produces one 64 x 64 chunk at a time and sends it through a bounded mpsc channel (capacity 64). The main thread drains completed chunks each frame and applies them as a batch through `Engine::apply_world_chunks`; `generation_pending` clears when the worker emits its `Done` sentinel.

**Reason:** Large right-drag selections previously produced one large `Vec<WorldChunk>` before the main thread saw any of it, hitching the frame during application. Streaming chunks keeps in-flight memory bounded, lets chunks appear incrementally as the worker completes them, and removes the arbitrary area limit while still rejecting bounds whose width*height overflows `i64`.

**Consequences:** `GenerateAreaError::TooLarge` now means only arithmetic overflow, not an arbitrary size policy. `generate_chunks` still returns a `Vec` for batch/test consumers; the viewer uses the streaming variant. Determinism is unchanged: `generate_chunks_streaming` produces chunks in the same order as `generate_chunks`, verified by a parity test.

## D-013: Large but bounded generation and deferred GPU synchronization

Date: 2026-07-12

**Supersedes:** The unbounded request policy in D-012.

**Decision:** Allow up to 4,096 chunks per generation request (16,777,216 cells) and 16,384 retained generated chunks. Check coordinate and chunk-count arithmetic before iteration, apply at most 16 streamed chunks per frame, support cancellation with `C`, and defer GPU world-cache synchronization until a stream terminates unless the camera leaves its cache.

**Reason:** A bounded channel limits only in-flight memory. Unlimited retained chunks, unbounded main-thread draining, and a full visible GPU-buffer rebuild after every streamed batch can still exhaust memory and recreate the observed hitching.

**Consequences:** Large selections remain substantially larger than the original one-million-cell cap while resource use has explicit ceilings. Worker failures and disconnects terminate the pending state. The automated quality gate renders two hidden GPU frames to catch Rust/WGSL binding-layout regressions.

## D-006: Measured compactness over source-code golfing

Date: 2026-07-12

**Decision:** Optimize runtime work, stored state, allocations, locality, and representation width in that order. Use the smallest proven variable sizes and prefer concise code only when clarity, correctness, safety, determinism, and testability remain equal.

**Reason:** Ten million persistent individuals require compact layouts and bounded work, but fewer source lines do not inherently produce faster or smaller machine code. Unmeasured narrowing can overflow, increase conversion cost, or preserve padding without saving memory.

**Consequences:** Population-scale and hot-path changes require explicit ranges, size assertions or benchmarks, and before/after evidence. Cleanup should remove dead state and unnecessary allocations without introducing cryptic code or unjustified `unsafe`.

## D-009: Copy runtime configuration into Cargo profile outputs

Date: 2026-07-12

**Decision:** Make the shared `sim-config` build script copy `config/simulation.toml` into the active Cargo profile directory while preserving its `config/` relative path.

**Reason:** Viewer and headless binaries use the same default relative configuration path whether launched through Cargo or directly from `target/debug` or `target/release`.

**Consequences:** Configuration changes retrigger the shared crate build. The quality gate verifies that the debug copy exists and exactly matches the repository source configuration.

## D-010: Name the non-graphical runner sim-headless

Date: 2026-07-12

**Decision:** Rename `sim-server` to `sim-headless` and reserve the server name for a future executable that actually owns networking, client sessions, or an authoritative service lifecycle.

**Reason:** The current executable only loads configuration, advances an in-process engine for a fixed number of ticks, and exits. Its name should describe current behavior rather than imply a network contract that does not exist.

**Consequences:** Cargo commands, executable names, VS Code launch entries, validation, and living documentation use `sim-headless`.

## D-011: Chunk-keyed world expansion and GPU presentation

Date: 2026-07-12

**Supersedes:** The temporary `softbuffer` renderer in D-002 and retained patch storage in D-008.

**Decision:** Store selected generated areas as deterministic 64 x 64 chunks keyed by signed coordinates, generate them on a dedicated viewer worker, and replace CPU framebuffer rasterization with `wgpu` instanced rendering. Bound GPU extraction to a padded camera rectangle and cap active redraws at 60 Hz.

**Reason:** Linear generated-area lookup, global feature scans, synchronous generation, per-pixel CPU rendering, and uncapped polling caused fullscreen and world-growth hitches despite low aggregate CPU and memory usage.

**Consequences:** `sim-core` remains GPU- and thread-runtime-independent; workers produce plain `WorldChunk` values and the engine owns insertion. The viewer adds `wgpu`, `pollster`, and `bytemuck`. GPU buffers rebuild when world revision changes or the camera exits its 128-cell cached margin. Paused unchanged scenes do not redraw.

## D-014: Missing-only generation and scale-bounded GPU extraction

Date: 2026-07-13

**Extends:** D-011 and D-013.

**Decision:** Resolve a validated selection to missing chunk coordinates before starting the viewer worker. Generate those coordinates individually, report worker disconnection once, and avoid paused-scene redraws for intermediate batches. Sample zoomed-out terrain with a power-of-two step derived from camera scale, clip blocks to generated coverage, scale the cache margin to approximately 128 screen pixels, and split static instance data at 1,000,000 instances per GPU buffer.

**Reason:** Regenerating overlapping chunks wastes CPU, intermediate redraws present unchanged buffers, and a large visible rectangle can exceed a device's practical single-buffer budget even though retained world storage is bounded.

**Consequences:** Large right-drag generation remains available up to 4,096 chunks, but repeat and partial-overlap selections perform only missing work. Sampling changes presentation detail only; authoritative terrain remains at full resolution in `sim-core`. The first GPU cache is built for the actual camera view rather than eagerly uploading the entire initial area, and ordinary pans retain a useful cache margin even at minimum zoom.

## D-015: Missing-only per-request generation budget

Date: 2026-07-13

**Supersedes:** D-013's interpretation of 4,096 chunks as the complete selection footprint and extends D-014's missing-only worker queue.

**Decision:** Validate coordinate safety across the complete right-drag rectangle, but count a `ChunkCoord` toward the 4,096-chunk per-request budget only when its selected portion is not fully covered by the initial rectangle and no retained generated chunk exists. Stop validation when the 4,097th missing chunk is observed. Keep raw `generate_chunks` and `generate_chunks_streaming` calls capped by their complete footprint because they have no world state from which to exclude loaded chunks.

**Reason:** Selecting an already generated rectangle plus a small adjacent extension should pay only for the new terrain. Loaded overlap consumes neither generation CPU nor additional retained storage, so charging it against the worker payload limit unnecessarily restricts expansion.

**Consequences:** A selection footprint may exceed 16,777,216 cells when most of it is loaded, while each worker job still creates at most 16,777,216 chunk-payload cells and retained generated storage remains capped at 16,384 chunks. A boundary chunk can recompute cells overlapping a non-aligned initial edge, although initial-owned cells remain authoritative and are filtered from presentation. Preview validation is cached by selection bounds and world revision to avoid repeating the bounded missing-chunk scan on unchanged frames.

## D-016: Explicit continental oceans and bounded lake descriptors

Date: 2026-07-13

**Decision:** Make the 2,048-cell absolute-coordinate continental field solely responsible for ocean, shallow-water, and sand-coast membership. Finer broad/regional fields may shape inland relief but cannot independently cross sea level. Replace thresholded local lake noise with at most one deterministic descriptor per eligible 1,024-cell region. Each descriptor has bounded radii, a guaranteed deep-water core, shallow and sand bands, regional shoreline perturbation, and margins that keep it inside the owning region. Resolve the descriptor once per aligned 64 x 64 generation chunk without adding persistent cell state.

**Reason:** Multi-scale local thresholds produced many similarly sized water splashes, and merely tuning their frequency could not rule out tiny puddles on other seeds. Separating ocean topology from bounded lakes creates coherent oceans and a small number of substantial inland water bodies while preserving deterministic on-demand generation.

**Consequences:** Worlds generated from an existing seed intentionally change and require an explicit generator version before persistence is introduced. Full drainage, river routing, and watershed hydrology remain planned. `TerrainCell` remains four bytes, and initial-area/chunk output stays identical because all descriptors and fields depend only on the seed and signed world coordinates.

## D-017: Coast-anchored descending major rivers

Date: 2026-07-13

**Extends:** D-016's explicit continental ocean topology.

**Decision:** Derive at most one major river inside each aligned 2,048-cell region from a fixed 8 x 8 continentalness lattice. Select bounded hashed coastal candidates that neighbor pre-existing continental water, grow upstream through strictly higher lattice nodes, discard routes with fewer than four land nodes or an insufficiently high source, then reverse the route so every accepted coarse path descends into that outlet. Refine each edge with a deterministic midpoint offset and rasterize a 24-to-48-cell tapered channel with a deep core, shallow water, and sand bank. Resolve the route once per 64 x 64 chunk and retain only intersecting segments in a fixed-capacity transient array.

**Reason:** Random local-noise channels do not guarantee an outlet, while a global watershed solve conflicts with independent on-demand chunks. Coast-anchored reverse growth guarantees a continental-water mouth, strict coarse descent, bounded work, and seam-independent regeneration without pretending to implement complete hydrology. The repository seed's separate 32-cell topology sample sees one boundary-touching continental-water component.

**Consequences:** Existing seeds intentionally produce new terrain again. The viewer needs no river-specific state because existing deep-water, shallow-water, and sand rendering applies automatically. `TerrainCell` remains four bytes and rivers add no persistent allocation. Full basin accumulation, tributaries, local streams, wetlands, dynamic flow, and generator versioning remain future work.

## D-018: Relief-following non-self-intersecting major rivers

Date: 2026-07-13

**Supersedes:** D-017's 8 x 8 continentalness route, random midpoint offsets, and 24-cell-wide abrupt source.

**Decision:** Derive each eligible regional river from a 16 x 16 lattice of pre-lake continental relief rather than the continent mask. Try at most eight deterministic coastal outlets and grow upstream through strictly higher eight-neighbor nodes while preferring gentle rises and stable headings. Reject returns beside the old path and diagonal edges that cross it, require 7 to 14 land nodes and a baseline source elevation of at least 50,001, then select a non-reversing continental-water mouth. Probe five bounded midpoint offsets per coarse edge while minimizing violation of its downhill elevation envelope, apply deterministic integer corner cutting, and reject refined descriptors whose nonlocal width-expanded water-and-bank corridors touch. Rasterize a nominal 20-cell shallow headwater feeding a nominal 16-to-48-cell widening channel; widen a narrow sand bank throughout and introduce the deep core downstream. Keep the 16-cell overview sample connected from headwater to outlet.

**Reason:** The prior greedy route was topologically connected but visually failed: a coarse continent-mask walk could form hairpins, cross its own diagonal geometry, ignore displayed valleys, end abruptly in grass, and alias into disconnected-looking lines at full-map zoom. Actual-relief routing, non-crossing constraints, aligned outlets, terrain-selected refinement, and an overview-width floor directly encode the missing visual and drainage invariants without requiring a global watershed solve.

**Consequences:** The repository seed now has one coherent startup-area major river rather than two malformed lines. Every accepted coarse route descends through pre-lake continental relief, but smoothed points are bounded visual refinement rather than a full final-elevation flow solve. The system still does not model basin accumulation, tributaries, lake inflows/outflows, erosion, or dynamic discharge. Each chunk evaluates a fixed 256-sample lattice and fixed-capacity arrays with no heap allocation or persistent terrain bytes. Existing seeds intentionally change again and still require generator versioning before persistence.

## D-019: Visible-demand generation and read-only chunk inspection

Date: 2026-07-13

**Extends:** D-011, D-014, and D-015.

**Decision:** Expose canonical read-only chunk inspection from `sim-core`, including Euclidean signed coordinates, half-open bounds, compact local coordinates, and full-initial, partial-initial, retained, retained partial-initial, or missing coverage. In `sim-viewer`, include fractionally visible edge cells in the camera's half-open world bounds, outline only the chunk under the pointer, and automatically submit the current visible area's missing chunks after startup, resize, zoom, left-drag pan release, or successful generation completion. Automatic and manual requests share the existing single bounded worker and `Engine::apply_world_chunks` insertion boundary. Reject an oversized or over-capacity visible request as a whole, and do not automatically retry cancellation until another view-change trigger.

**Reason:** Camera exploration should materialize nearby visible terrain without requiring repetitive selection, while chunk seams and partial initial boundaries must remain inspectable in signed coordinate space. Reusing `World::missing_chunk_coords` keeps loaded-state truth, deduplication, capacity, and ordering in `sim-core`; event-driven requests avoid a per-frame global scan, and all-or-nothing rejection avoids silently favoring row-major chunks in enormous zoomed-out views.

**Consequences:** The viewer still owns only camera, input, worker lifecycle, and temporary presentation data; `Engine` retains authoritative world ownership. A normal visible request can create at most 4,096 chunks, applies at most 16 per frame, and retains at most 16,384 total generated chunks. Views beyond those limits remain navigable but display an automatic-generation pause reason in the title, and right-drag can request a smaller valid area. Chunk outlines use at most four extra rectangle instances and disappear below four projected pixels. No dependency or persistent terrain representation changes.

## D-020: Layered regional generation within existing memory guardrails

Date: 2026-07-14

**Supersedes:** D-016, D-017, and D-018's continental-field, lake-descriptor, and coast-anchored river algorithms.

**Decision:** Generate terrain through three deterministic integer-only tiers: analytic plate/climate fields, cached 4,096-cell regional drainage lattices, and 64 x 64 chunk synthesis. Keep the existing 16,777,216-cell initial-area limit, 4,096-chunk request limit, and 16,384 retained-chunk limit. Traverse chunks by complete drainage region so the bounded 40-entry cache remains effective; keep sparse features row-major within each generated tile. Keep regional lake water two lattice nodes away from borders and stop region-local rivers before the four-node border margin until cross-region drainage has an owned design.

**Reason:** The prior descriptor-based lakes and rivers were replaced by a more coherent regional drainage model, but temporarily raising allocation limits to render a much larger default world would have violated the engine's bounded-footprint rules. Regional caches must not become hidden simulation truth or create cache-order-dependent output.

**Consequences:** Existing seed output changes again and remains ineligible for persistence until generator versioning exists. `TerrainCell` remains four bytes; regional cache data and per-chunk river indexes are transient derivation state. Lakes and river channels are static terrain, not a complete watershed, erosion, or dynamic-flow implementation. Regression coverage now includes drainage descent across signed regions, dry regional lake margins, rasterized water/feature exclusion, and complete river indexing; cross-region drainage and canonical performance measurement remain planned.

## D-021: Deferred bootstrap coverage and progressive viewport streaming

Date: 2026-07-14

**Supersedes:** The eager-start portions of D-005 and D-007, D-013's terminal-only GPU synchronization, and D-019's all-or-nothing automatic visible-demand submission.

**Decision:** Construct `Engine` and `World` with only a deterministic declared bootstrap rectangle; do not allocate or generate its terrain synchronously. Store actual coverage as private chunk-backed tiles: exact clipped bootstrap tiles inside the configured rectangle and complete expansion tiles elsewhere. Keep eager materialization as an explicit headless/tooling operation. Carry opaque core-owned load requests and loads across the viewer worker boundary, and reject loads whose seed or bootstrap coverage does not match the receiving world.

Use one persistent viewer worker with monotonically increasing job IDs. Manual selection preempts automatic demand, automatic current-viewport pages preempt bootstrap pages, and stale automatic/bootstrap results are discarded after cancellation while already applied chunks remain. Enumerate viewport pages deterministically from the camera center in 8 x 8 chunk pages without preallocating the complete zoomed-out request. Merge no more than 16 loads per frame on the event-loop thread. Coalesce visible affected GPU cache rebuilds to a 125 ms streaming cadence, skip uploads for off-cache changes, and force a final sync when a page ends.

**Reason:** The prior dense initial allocation delayed first window creation, while the prior worker could generate individual chunks but held all terrain invisible until a job completed. A zoomed-out viewport also turned into one oversized request or stale distant work. Treating configured bootstrap bounds as loaded data would make a lazy path unsafe at non-aligned edges.

**Consequences:** `World::cell`, feature queries, visitors, area checks, and inspection now distinguish declared coverage from resident coverage; an unloaded initial tile is not simulation truth. Initial clipped tiles do not consume the 16,384 expansion-tile capacity, but a partial tile promoted to a full expansion tile does. The headless runner preserves its complete-bootstrap startup contract by explicitly materializing before ticks. The viewer applies worker loads only after each event-loop turn's fixed simulation phase, so no tick observes an arrival mid-step; current simulation state does not branch on materialization residency. Any future terrain-dependent simulation must use a residency-independent deterministic query or an explicit deterministic loading phase. Extreme zoom can progressively stream nearest full-detail tiles but cannot permanently retain an unbounded high-detail world; overview LOD, unloading, and persistence remain separate future work.

## D-022: Bounded deterministic multi-threaded terrain generation

Date: 2026-07-14

**Supersedes:** D-012, D-013, D-019, and D-021 only where they specify one sequential generation worker and a fixed 16-load per-frame insertion ceiling.

**Decision:** Keep job priority, generation IDs, opaque `WorldChunkLoad` payloads, and main-thread `Engine` insertion, but replace sequential chunk computation with one persistent coordinator plus a fixed Rayon pool. Use `available_parallelism - 1` computation workers with a one-worker minimum. Bound active plus completed reorder work to two tasks per worker and 64 tasks total, keep the existing 64-message result channel, and emit loads strictly in original request order even when computation finishes out of order. On cancellation, stop dispatching, drop completed stale payloads, allow already-running pure tasks to finish, and preserve already-applied chunks.

Replace per-thread hydrology caches with a process-shared 40-completed-entry build-once LRU. Protect only lookup/recency bookkeeping with a mutex, represent each region build with one `OnceLock`, share completed immutable maps through `Arc`, and never evict an in-flight build. Insert ordered results on the event-loop thread in 16-load batches while a 2 ms budget remains, with a hard ceiling of 64 loads per frame.

**Reason:** Chunk synthesis is a pure seed-and-coordinate computation and can safely execute concurrently, but a naive worker-per-chunk design would duplicate the expensive 4,096-cell regional hydrology solve and multiply its roughly 325 KiB lattice payload per worker. Ordered bounded streaming preserves deterministic observation, cancellation, memory guardrails, and simulation ownership while using available CPU capacity. A same-seed cold-cache release measurement on a 16-logical-CPU machine improved the 4,096-chunk generation-and-channel workload from a 895.7 ms median with one worker to 186.1 ms with 15 workers, approximately 4.8x.

**Consequences:** `sim-viewer` now depends on Rayon; `sim-core` remains standard-library-only and exposes no scheduler or mutable cache handle. Cache completion and worker scheduling cannot change generated content, and tests compare one-worker and multi-worker ordered payloads exactly. Completed cache storage remains about 12.7 MiB of logical lattice data before metadata, while in-flight entries may temporarily exceed 40 only for distinct concurrent region builds. This change accelerates on-demand derivation but does not make a 256k x 256k dense bootstrap viable: the 16,777,216-cell bootstrap limit and 16,384 retained-expansion limit remain, and massive worlds still require overview LOD, unloading, persistence, and sparse modified-chunk policies.

## D-023: Parallel regional preparation and wider bounded viewport pages

Date: 2026-07-14

**Supersedes:** D-021 and D-022 where they specify 8 x 8 automatic pages, two tasks per worker, serial cold-region construction, standard-library-only `sim-core`, and the earlier world-capacity ceilings.

**Decision:** Add Rayon to `sim-core` for pure indexed regional derivation. Before a viewer request window releases dependent chunk tasks, prepare its distinct build-once regional maps through the same fixed pool. Parallelize macro elevation, roughness, temperature, and coarse moisture lattice slots by stable index; retain serial priority-flood and river-extraction ordering. Use four tasks per worker with a hard 64-task cap, align the completed regional LRU at 64 entries, preserve the 64-result channel and authoritative request-order emission, and enlarge center-out automatic pages to 32 x 32 chunks. Keep cancellation checks before and after cold preparation so a pre-cancelled job performs no derivation.

Accept the implemented ceilings of 268,435,456 bootstrap cells, 65,536 missing chunks per request, and 4,194,304 retained expansion chunks. Treat them as hard validity ceilings rather than target resident-memory budgets.

**Reason:** Chunk fan-out alone left the first worker in each cold 4,096-cell drainage region performing the expensive regional solve while sibling workers waited on its `OnceLock`. Preparing the shared dependency first lets the independent regional fields use the pool and prevents parked followers; a larger bounded page and reorder window reduce coordinator/event-loop bubbles during large zoomed-out demand. Indexed writes and ordered output preserve seed-and-coordinate determinism.

**Consequences:** Pool size and completion order do not change terrain: a regression compares every regional lattice and river segment between one-worker and four-worker builds, while existing coordinator tests compare exact chunk payload order. On a 16-logical-CPU machine, fresh-process release medians improved from 225.8 ms with one worker to 38.3 ms with 15 workers for a cold 1,024-chunk page, and from 908.7 ms to 133.8 ms for 4,096 chunks. The 65,536-chunk request ceiling represents up to 1 GiB of logical terrain payload, and the retained expansion ceiling represents 64 GiB before sparse features and container overhead. A fully resident 256k x 256k world remains outside these budgets and still requires LOD, unloading, persistence, and sparse modification storage.

## D-024: Centered finite world envelope and origin-outward generation

Date: 2026-07-14

**Supersedes:** D-007's unbounded-extent interpretation, D-019's unbounded camera navigation, and D-023's count-only retained expansion ceiling.

**Decision:** Define the maximum generatable world as the half-open square from `-32,768` inclusive to `32,768` exclusive on each axis. With 64 x 64 chunks this is exactly 1,024 x 1,024 chunks, or 1,048,576 chunks total. With the size-asserted four-byte `TerrainCell`, its 4,294,967,296 cells represent exactly 16 GiB of raw terrain payload. Enforce this spatial envelope in `sim-core` for bootstrap configuration, direct generators, area requests, inspection, and load acceptance instead of relying only on a retained-chunk counter.

Center configured bootstrap bounds and the viewer camera on `(0, 0)`. Center automatic page zero over chunks `-16..15` on both axes, then enumerate deterministic rings outward. Clamp the camera center to the envelope, reduce maximum zoom-out to 1/8x of initial fit, clip visible generation demand to the envelope, and render a persistent red square just inside its four edges.

**Reason:** A count-only capacity can be spent primarily in one direction and does not communicate where generation must stop. A fixed centered coordinate envelope makes opposite directions equally available, gives every generation entry point the same invariant, and gives users a visible boundary. The 16 GiB figure is an area calculation for dense raw terrain, not a promise that total process memory remains at or below 16 GiB.

**Consequences:** Generation cannot move the boundary or continue past it even if count capacity remains. Full residency still costs more than 16 GiB after sparse features, tree/map nodes, metadata, caches, and allocator overhead, so unloading and persistence remain required before treating full-envelope residency as practical. Existing bootstrap coordinates and iteration order become signed and origin-centered; tests and presentation behavior explicitly follow that contract.

## D-025: Maximum zoom-out fits the complete world envelope

Date: 2026-07-14

**Supersedes:** D-024 only where it fixed maximum zoom-out at 1/8x of the configured bootstrap fit and clamped only the camera center.

**Decision:** Derive the camera's minimum zoom from the ratio of the complete-world fit scale to the configured-bootstrap fit scale for the current viewport. Clamp each visible camera axis inside the world when it fits; center an axis when the viewport is as large as or larger than the world. For the repository's square 4,096-cell bootstrap and 65,536-cell world side, minimum zoom is 1/16x of initial fit.

**Reason:** A fixed 1/8x floor displayed only half the full world's height. Cursor anchoring plus center-only clamping could also leave an edge off-screen at minimum zoom. Deriving the floor from both rectangles guarantees the complete red boundary fits regardless of window aspect ratio or valid bootstrap dimensions.

**Consequences:** Startup framing remains unchanged. Ordinary wheel zoom remains cursor-anchored, while world-edge clamping may move the camera as necessary; at maximum zoom-out the full envelope is centered and visible. Extremely wide or tall windows may show empty presentation space outside the square on the surplus axis, but automatic generation remains clipped to authoritative world bounds.

## D-026: Viewer expansion generation is explicit right-drag only

Date: 2026-07-14

**Supersedes:** D-021, D-023, D-024, and D-025 only where they schedule or describe automatic current-viewport generation and view-change cancellation.

**Decision:** Keep startup bootstrap streaming through the bounded center-out 32 x 32 `ChunkPager`, but remove automatic visible-area generation from `sim-viewer`. Camera movement, resize, zoom, hover, and redraw never create or cancel terrain requests. After bootstrap, only releasing a valid right-button drag queues missing terrain. Manual work continues to preempt active bootstrap work, and the persistent coordinator, deterministic request ordering, generation IDs, load validation, and insertion budgets remain unchanged.

**Reason:** Viewpoint-triggered generation performs CPU work and retains terrain merely because the user navigated the camera. Explicit right-drag requests make expansion intentional, predictable, and bounded by the existing selection preview and core validation rules.

**Consequences:** Panning or zooming over missing terrain leaves it unloaded until selected with the right mouse button. `GenerationKind::Automatic`, the automatic pager/pending state, viewpoint-dirty flag, and automatic error title state are removed. The remaining pager is named `ChunkPager` because it serves bootstrap rather than the viewport. `C` permanently drops remaining bootstrap work for that process, while later valid right-drag selections can still queue manual generation.

## D-027: In-game diagnostic HUD and retained time-square

Date: 2026-07-14

**Decision:** Keep the native window title static and move simulation, generation, and hover-inspection data into a DPI-scaled in-game HUD owned by `sim-viewer`. When no world position is under the pointer, use the inspection area for a compact control guide. Preserve the bottom moving square as the passage-of-time motif, add a subtle rail, and derive its traversal from a repeating 60 simulated-second interval. Render HUD text with a built-in 5 x 7 bitmap alphabet encoded as horizontal rectangle runs in the existing screen-space pipeline.

**Reason:** Dynamic title text is visually detached from the simulation and the old top-left bars expose little actionable information. A self-contained HUD makes status, time, generation, and terrain inspection legible in the same visual context while retaining the requested time-square identity. Reusing rectangle instances avoids a font/runtime dependency for the current diagnostic UI.

**Consequences:** `sim-viewer` gains presentation-only cursor position and generation-status views; no HUD value becomes simulation truth. The reusable HUD string and fixed CPU/GPU screen-overlay capacities bound allocation and upload size, with regression coverage for supported layouts. The bitmap alphabet is intentionally utilitarian; richer typography or interactive widgets would require a later UI/rendering decision.

## D-028: Canonical sampled world-quality evidence

Date: 2026-07-14

**Decision:** Define world-foundation review format 1 as four representative seeds (1, 7, 42, and 10,001), four complete-envelope overviews, and fixed regional, both-axis 4,096-cell drainage-seam, coastline, river-mouth, lake, mountain, and feature-visible close-up views. Produce all 12 views through the release `sim-core` `render_map --review-set` workflow. Record half-open bounds, sampling step, dimensions, source revision, terrain and sparse-feature counts, feature density, public type sizes/alignment, and a stable coordinate-sensitive semantic sample hash in deterministic TSV reports. Keep elapsed time out of those deterministic files and report it only to the console and measured performance documentation.

**Reason:** Generator work needs comparable evidence at the same seeds, coordinates, scales, and seams before cross-region drainage and later classification changes intentionally alter output. A read-only `ChunkGenerator` client can produce that evidence without allocating a persistent world or making presentation summaries authoritative. Multiple seeds reduce the risk of choosing thresholds solely to protect seed 1's current appearance.

**Consequences:** `cargo test --workspace` now runs the example's bounds, alignment, seam, seed-set, metadata, and hash contract tests. Derived BMP/TSV artifacts live under ignored `target/` output by default and may be regenerated rather than committed. Hash changes identify sampled output drift but are not automatically failures or quality judgments; intentional generator changes must review every seed and document the behavioral reason. This decision does not introduce persistence generator versioning, full-resolution world checksums, or quantitative terrain-quality thresholds.
