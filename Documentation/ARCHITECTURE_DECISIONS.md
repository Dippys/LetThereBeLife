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

## D-029: Canonical whole-envelope drainage above regional refinement

Date: 2026-07-14

**Decision:** Build one immutable drainage skeleton per seed over the complete finite world boundary at a 256-cell step (257 x 257 nodes). Priority-fill and route that graph in deterministic serial order after parallel pure field sampling. Give the skeleton canonical ownership of basin sink IDs, lake IDs/outlets/spill elevations and terminal status, major-channel identities, confluences, downstream water/ocean/world-edge destinations, and river geometry subdivided at 32-cell intervals. Retain up to four completed seed skeletons through a build-once LRU; preserve in-flight builds. Keep the existing 4,096-cell `RegionMap` as the climate/macro refinement cache, but derive its water depth from the skeleton's filled surface and consume the same global segments on both sides of an edge. Remove coordinate-based lake and river dry margins. Keep an eight-entry allocation-free river index per chunk, proven over the complete envelope for the four review seeds.

**Reason:** Independent region fills could only avoid contradictions by deleting water near every edge, which made geography end at an implementation boundary. The finite 65,536-cell envelope makes a coarse canonical graph cheap enough to solve once per seed without densely generating terrain. Isolated seed-1 release measurements favored step 256 over step 128: 44.3 ms versus 175.9 ms build time, 1,109,240 versus 1,918,704 retained logical bytes, and 4,359,234 versus 17,369,154 bytes of fixed node-build scratch upper bound. Both candidates interpolate a four-node-step moisture lattice rather than repeating expensive upwind probes at every drainage node. The selected step retained 167 lakes and 3,832 major links, passed signed seam and continuation checks, and remained convincing in all canonical views. Regional 32-cell sampling and river subdivision preserve chunk-scale rasterization without making a regional cache authoritative.

**Consequences:** This supersedes D-020's temporary two-node lake and four-node river border policy while retaining its regional cache and deterministic chunk-synthesis boundaries. Generated terrain for existing seeds intentionally changes again; post-decision sampled hashes are recorded in `TESTING.md`. A cold seed pays one bounded whole-envelope solve before regional fan-out, but no chunk performs a global solve and equal inputs remain byte-identical across request order and pool size. The representative four-seed skeletons retain 4,155,352 logical bytes together; the conservative four-cache representation ceiling is 62,350,256 bytes before metadata. `RegionMap` keeps its five-array payload and 64-entry cache. Major watersheds, lakes, and rivers now cross regional boundaries, while tributary hierarchy, local streams, wetland rules, dynamic water, erosion, generator versioning, and persistence remain later work.

## D-030: Finite-envelope climate band and derived inspection

Date: 2026-07-14

**Decision:** Interpret the complete vertical envelope as one stylized cold-to-warm-to-cold band, provisionally at approximately 2 metres per cell (about 131 km square). Use a 6,000-to-46,000 lowland latitude baseline from either vertical edge to the center, then retain deterministic broad variation and altitude lapse. Divide the envelope into four 16,384-cell circulation bands with alternating northwest/southeast flow and noise-displaced boundaries. Do not wrap either axis; retain off-envelope analytic samples only as boundary inputs to ocean-fetch probes. Expose resident-cell diagnostics through an allocation-free four-byte `ClimateSample` containing exact reconstructed classification temperature, retained quantized moisture, and prevailing wind.

**Reason:** The former 262,144-cell latitude cycle covered four times the playable height, so the finite world omitted intentional polar lowlands and its circulation bands did not describe the actual envelope. Compressing the climate zones is a gameplay geography contract rather than an Earth-scale physical claim. Deriving inspection temperature from the same four 32-cell analytic nodes preserves exact classification diagnostics without increasing the four-byte cell payload or risking a synchronous regional/drainage cache build on the viewer thread.

**Consequences:** Generated terrain and feature distributions intentionally change for existing seeds; the four post-decision review hashes and visual findings are recorded in `TESTING.md`. Complete-envelope tests now cover cold, temperate, and warm lowlands across all representative seeds and wobbled wind transitions. Hover inspection displays temperature, the zero-to-255 moisture byte, and `NW`/`SE` wind. Explicit tundra, snow, biome classes, and traversal/resource meaning remain Slice 3; world topology, physical scale, and off-envelope moisture boundary behavior require new decisions before persistence or wrapping.

## D-031: Packed surface and biome semantics

Date: 2026-07-14

**Decision:** Replace the one-byte rendered `GroundType` field with a private one-byte `TerrainClass`. Pack seven-value `SurfaceType` in the low nibble and eleven-value `BiomeType` in the high nibble, and expose only typed read access through `TerrainCell`. Keep elevation and moisture unchanged so the complete cell remains four bytes. Distinguish ocean, lake, and river identity independently from deep/shallow water; distinguish beach from desert despite their shared sand surface; and represent grassland, savanna, forest, wetland, tundra, alpine, and snow/ice without stored traversal or fertility flags. Advance world-quality output to review format 2 with separate surface/biome distributions and hashes over the packed class byte.

**Reason:** Movement, gathering, settlement, inspection, and later resource rules need environmental meaning that a single rendered-ground enum conflated. Two independent public enum fields would grow the hot terrain record, while a packed byte covers the proven domains and preserves safe enum-returning accessors. Broad existing elevation, moisture, and temperature fields provide coherent regions without a new per-cell noise pass or retained cache.

**Consequences:** `TerrainCell` stays four bytes, so the 64 MiB bootstrap and 16 GiB full-envelope raw-payload calculations do not change. Existing seeds intentionally change classification, feature placement, colors, report schema, and semantic hashes. Lake identity survives overlapping river geometry while deep-water precedence remains unchanged; water, sand, and snow/ice exclude sparse features. Current wetland classification uses saturated non-cold lowland moisture plus elevation and is explicitly provisional until Slice 4 adds drainage-grounded water-table, floodplain, basin-edge, and transition logic. Traversal, fertility, resource yield, settlement suitability, generator versioning, and persistence remain future contracts.

## D-032: Restrained ordered streams and bounded terrain transitions

Date: 2026-07-14

**Supersedes:** D-029 only where it retained links at the major-channel threshold, kept an eight-entry chunk river index, and deferred all tributary/wetland logic; D-031 only where wetland classification was moisture/elevation-only.

**Decision:** Keep the 256-cell whole-envelope drainage graph as canonical ownership for local streams, but retain only links from 220,000 accumulated flow rather than the rejected 70,000 leaf-vein candidate; the existing major threshold remains 260,000. Compute Strahler order in deterministic upstream-to-downstream order and store it in existing padding so `DrainageSegment` remains 24 bytes and `ChannelLink` remains 28 bytes. Derive half-width from order and flow with a three-cell local minimum and thirteen-cell cap. Reject a diagonal flow target that crosses an already selected diagonal in the same coarse cell, bound refinement jitter to ten cells, and reject any refined segment crossing outside a shared endpoint for all representative seeds.

Classify wetland only when saturated warm/temperate lowland limits coincide with low macro slope and either an interpolated canonical basin edge or an 18-cell stream floodplain. Use a four-cell riparian band for qualifying soil-backed banks. Reuse the already-computed coherent detail value to shift beach, desert, forest, wetland, snowline, and treeline thresholds within fixed bands; never alter selected water geometry. Keep a 13-segment allocation-free chunk fast path for the representative seeds and an empty-vector overflow path for correctness on other accepted seeds. Define Slice 4 stream visibility as authoritative seven-cell corridors through globally aligned sample step 4; assign coarser minority-preserving presentation to Slice 6.

**Reason:** The major-only threshold left some believable basins empty, but exposing nearly the complete runoff graph at 70,000 produced an implausible leaf-vein pattern. A narrow 220,000-to-260,000 band adds secondary channels without turning every headwater into visible water. Reusing the canonical acyclic graph guarantees downstream destinations and exact shared joins; explicit diagonal/jitter constraints prevent visual crossings. Hydrologic evidence, riparian proximity, and bounded reuse of existing detail improve transitions without another regional lattice, universal cell flag, or per-cell noise call. The overflow path removes a normal-runtime panic that a representative-seed-only fixed bound could not justify for every `u64` seed.

**Consequences:** Existing seed output intentionally changes and the final review-format-2 hashes are recorded in `TESTING.md`. Seed 1 retains 4,479 links and 35,832 32-cell segments; its isolated release build observation used 1,251,580 retained logical bytes, 4,359,234 scratch-upper-bound bytes, and 52.7 ms. The four review seeds retain 4,648,592 logical skeleton bytes together. `TerrainCell` remains four bytes and `RegionMap` keeps five lattices. Representative chunks use 312 bytes of inline segment payload plus a 24-byte empty vector header with no heap allocation; exceptional overflow is safe. Slice 4 is complete. Sources remain limited to the 256-cell lattice, and zoom steps above four still need Slice 6 summaries. Dynamic water, erosion, persistence, and generator versioning remain deferred.

## D-033: Explicit sparse lake-fed river systems and graded water surfaces

Date: 2026-07-15

**Supersedes:** D-032's accumulated-flow visibility threshold, claim that its selected network was visually restrained, ten-cell independent point jitter, 24-byte `DrainageSegment`, and fixed below-sea-level river carving. D-032's wetland, riparian, transition, crossing, overflow, and deterministic ownership decisions remain active.

**Decision:** Keep the complete 256-cell priority-filled runoff graph as hidden drainage topology, but stop rendering every link above a threshold. Nodes at or below 12,000 moisture contribute zero perennial runoff. A visible river must begin at a canonical nonterminal lake outlet with at least 260,000 accumulated runoff, drain to ocean or world edge, remain at least 2,048 cells from another selected source, and fit within a 24-source complete-envelope cap. Retain only the complete downstream paths of those ranked sources; paths may merge or pass through downstream lakes. Spring, snowmelt, and threshold-frontier sources are not emitted.

Refine selected links at 32-cell intervals with exact shared endpoints, deterministic bounded perpendicular curvature, and at most six cells of interior detail rather than nearly straight independently jittered chords. Check each refined link against already retained geometry; retry at half curvature and then exact straight subdivision if required, so aesthetic refinement cannot introduce a crossing. Store `surface_a` and `surface_b` on every `DrainageSegment`, require the surface not to rise downstream, and interpolate that grade during chunk rasterization. `TerrainCell.elevation` represents the local river water surface on river cells; it is no longer forced to the global 24,500/27,800 constants. Retain the source node and lake identity in a compact `RiverSource` record; mouth coordinates remain derivable build scratch rather than persistent state.

**Reason:** The 220,000 threshold still exposed 4,479 seed-1 graph links. Every dry node also contributed a positive runoff floor, visible channels began at arbitrary threshold frontiers, 256-cell D8 chords remained obvious despite small jitter, and all river cells were assigned below-sea-level elevations even in uplands. These are structural failures and cannot be fixed by another density threshold. Explicit lake sources make every current river visually explainable; source spacing and a global cap remove comb/leaf patterns; effective rainfall removes perennial desert runoff; curved refinement hides the coarse routing lattice; and longitudinal surfaces restore meaningful elevation.

**Consequences:** The four review seeds retain 8, 4, 15, and 15 lake-fed sources; 87, 33, 131, and 144 coarse links; and 696, 264, 1,048, and 1,152 refined segments. Their retained logical skeleton payloads are 288,188, 273,240, 299,272, and 302,428 bytes, totaling 1,163,128 bytes. The conservative logical scratch upper bound grows to 4,425,283 bytes because source selection can retain lake-size work and one candidate tuple per node; actual candidates equal qualifying lake outlets, not all nodes. `DrainageSegment` grows from 24 to 28 bytes to own its grade, `RiverSource` is 8 bytes, `ChannelLink` remains 28 bytes, and `LakeDescriptor` remains 12 bytes. The 13-entry chunk fast path is now 364 bytes plus its empty 24-byte overflow-vector header and remains allocation-free for the review seeds. Lake-only sources are intentionally conservative; physically modeled springs, snowmelt headwaters, and selected major tributaries require a later reviewed contract rather than silently reintroducing source-less water.

## D-034: Derived base resources and spatial surface-feature ecology

Date: 2026-07-15

**Decision:** Keep `Feature` as the immutable 24-byte world-position-plus-kind record and derive a four-byte `BaseResource` from `FeatureKind`: berry bushes expose 12 abstract food units, trees 120 wood units, and rocks 80 stone units. Treat feature position as stable identity within the world seed and eventual generator version. Do not store remaining quantity, removal, regrowth, damage, fire, or ownership in generated terrain or features; a future sparse delta keyed by that identity owns mutable state and persistence.

Generate feature composition by reusing the already-computed coherent local-detail sample as deterministic bounded canopy, grove, berry-patch, and outcrop bands combined with biome, temperature, moisture, macro slope, and basin/river proximity. Keep water, sand, and snow/ice invalid. Permit rare soil outcrops, increase berries on suitable near-water ground, form forest clearings/edges through the canopy band, and retain row-major sparse emission. Do not add another noise call or stored field, and do not add species until an implemented yield, tolerance, lifecycle, fire, or presentation rule requires the distinction.

**Reason:** Independent scatter made forests visually uniform, berries fragile as a first-agent food source, and stone dependent on uncommon hill/rock classification. Storing mutable quantities in generated base would make cache residency and later persistence semantics ambiguous. Derived capacities provide a small read contract now while preserving a clean generated-base-plus-sparse-delta model.

**Consequences:** Existing seeds intentionally change feature distributions and semantic hashes. `FeatureKind` remains one byte and `Feature` remains 24 bytes; `BaseResource` is four bytes but is returned by value rather than retained per feature. Review format 3 contains 14 views, adding full-resolution forest and outcrop probes to the berry-bearing close-up. Resource capacities are provisional abstract units for later gathering balance. Depletion, removal, regrowth, generator versioning, persistence, and species remain unimplemented and cannot affect generated output.

## D-035: One-level viewer-owned chunk summaries

Date: 2026-07-15

**Decision:** Keep step-1 rendering exact. For every coarser power-of-two renderer step, retain one active viewer-owned `ChunkRenderSummary` for each resident chunk intersecting the camera cache rectangle and its approximately 128-screen-pixel margin. Scan every resident cell and sparse feature in that chunk once when its summary is missing. Each block emits one dominant ordinary terrain base; it may additionally emit one bounded minority rectangle, prioritized as river, lake, mixed ocean/land coast, snow, rock, then hill. Each block emits at most one feature marker whose kind is the stable count majority and whose area is a bounded function of total feature density. Do not retain all zoom levels simultaneously.

Invalidate summaries only when their chunk intersects authoritative world change bounds, evict entries outside the current cache margin, and clear entries when the active step changes. Build independent missing chunks through Rayon and collect them in deterministic input order before extending the `BTreeMap`. Add exact read-only `World` visitors for resident-region enumeration and direct chunk-local cell/feature access so presentation does not scan the complete resident map once per summary or expose private storage. Keep summaries and GPU instances outside `sim-core` truth.

**Reason:** Coordinate-modulus filtering deleted every unaligned feature and could delete a continuous narrow river, while expanding one sampled cell over a coarse block hid coast, lake, mountain, and biome structure. A complete retained pyramid multiplies cache state even though the renderer displays only one level. One active chunk-local level bounds residency to the existing camera policy, makes change invalidation explicit, and permits parallel deterministic construction. Base-plus-one-detail-plus-one-marker retains the important minority signal without unbounded rectangles or an asset dependency.

**Consequences:** A full block emits one to two 20-byte terrain instances and zero or one 20-byte feature instance. `VisualSample` is size-asserted at 12 bytes and the transient 15-class `SummaryAccumulator` at 186 bytes; neither survives summary construction. On the recorded release machine, rebuilding the fully resident 4,096 x 4,096 repository rectangle took 23.660, 17.494, 16.883, 15.240, and 15.020 ms at steps 4, 8, 16, 32, and 64 after the first high-volume step-2 run initialized and saturated the shared pool; each CPU-cache/GPU-instance copy had logical payloads of 20.36 MiB, 5.26 MiB, 1.37 MiB, 362.5 KiB, and 96.8 KiB. The artificial full-rectangle step-2 case used 80.40 MiB per copy and took 523.499 ms; an actual step-2 camera cache covers only the visible rectangle plus margin, not all 4,096 cells per axis. A hidden streamed step-16 sync over 64 resident chunks measured 3.305 ms summary construction, 0.040 ms CPU upload enqueue, and 3.345 ms combined synchronization. These observations are not cross-machine regression limits; timestamp-query GPU completion and long interactive frame distributions remain open measurement work.

## D-036: Derived resident physical-world query boundary

Date: 2026-07-15

**Decision:** End Phase 1 with three explicit allocation-free `World` queries. `water_at` returns resident ocean, lake, or river identity; lake and river water is drinkable while ocean water is not. `resource_at` returns an optional immutable `BaseResource` while distinguishing unloaded and outside-envelope terrain. `traversal_step` accepts exactly one cardinal-cell move and returns an eight-byte `TraversalStep` with signed elevation delta, integer cost, and explicit passable, water-blocked, slope-blocked, or feature-blocked kind. All deep and shallow water currently blocks walking. Trees and rocks block their target cell, berry bushes do not, and adjacent elevation changes through 512 generator units are accepted while 513 and above are blocked. Keep this threshold named and provisional until the physical-agent loop supplies movement-scale evidence.

Treat `WorldPosition` as terrain identity and expose `Feature::identity` as the feature position within one seed and eventual generator version. Keep generated cells/features immutable, put future depletion/removal/terrain modification in sparse simulation-owned deltas, and keep dynamic agents/entities separate. Do not add a settlement-suitability score to `sim-core`; headless agent/settlement code composes water distance, traversable connectivity, and resource access according to its own bounded policy.

**Reason:** The physical-agent loop needs unambiguous behavior for resident absence, unloaded coverage, invalid movement, water access, gathering, and slope without reading renderer state or private generator inputs. Deriving those answers preserves the four-byte terrain cell and avoids freezing a universal settlement policy before agents exist. Cardinal movement is the smallest deterministic pathfinding contract; diagonal corner rules, swimming, wading, boats, bridges, and calibrated physical slope remain later movement decisions.

**Consequences:** Slice 7 adds no retained world state or dependency. Each traversal query performs two chunk-map lookups and at most one target-feature binary search; point water/resource queries perform one resident lookup and resource lookup as applicable. `WaterSource` and `TraversalKind` are one byte and `TraversalStep` is eight bytes. Presentation-oriented `cell`, `feature_at`, and `base_resource_at` remain available, but physical simulation must use the explicit query methods where unloaded versus absent matters. The world-foundation plan is complete, and subsequent foundational changes require an observed physical-agent failure rather than speculative expansion.

## D-037: Dense physical agents and bounded totally ordered movement events

Date: 2026-07-16

**Supersedes:** D-036 only where it described three physical-world queries; Slice 0 adds `standability_at` to centralize the already-decided water/tree/rock standing rule without changing generated output.

**Decision:** Represent each physical agent as an implicit dense zero-based `AgentId(u32)`, a private checked `i16` position covering the exact `[-32,768, 32,768)` envelope, and one-byte idle/moving/dead activity. Keep movement generations in a parallel `u32` array rather than widening the hot record. Create a population only through an atomic explicit boundary after a fixed active simulation rectangle is completely resident. Preserve requested-position order for the first IDs, fill any remainder from standable cells in row-major order, and clear the complete population/ID/scheduler boundary on reset while retaining deterministic terrain residency.

Measure authoritative time as `SimTime(u64)` fixed ticks with checked exhaustion. Start Phase 2 with a private standard-library binary heap and order every event by due time, explicit event-class rank, agent ID, then checked monotonic sequence. Use `TraversalStep::cost()` directly as the positive integer movement delay. Rescheduling increments the agent generation and makes earlier heap records typed stale events; filter stale records when retained heap length reaches population plus 4,096 events, with a 64-entry minimum. Drain at most 4,096 due events per engine tick and report remaining due work as backlog. Revalidate traversal at completion and publish bounded read-only agent views plus typed command/completion outcomes.

**Reason:** Twenty to one hundred agents need understandable exact behavior now, while the ten-million-agent target forbids a per-tick population scan, heap allocation per agent/event, unstable ties, and wide speculative agent objects. The finite world proves compact coordinates without quantization loss. Dense slot identity removes a repeated ID field, parallel generations make cancellation cheap, and total heap ordering gives a small measured scheduler whose private boundary can later move to buckets without changing public event semantics. A dedicated standability query prevents spawn rules from drifting away from traversal rules.

**Consequences:** `AgentId` is four bytes, the hot `AgentRecord` is six bytes/alignment two, and `ScheduledEvent` is 32 bytes/alignment eight. Population initialization retains four contiguous buffers (records, generations, reserved events, and bounded recent outcomes) and publishes nothing on validation or allocation failure. IDs are stable only within one initialized run; reset invalidates old handles and reinitialization restarts at zero. Slice 0 deliberately has no occupancy index, so agents may share or target one cell until Slice 1 defines collision arbitration. The viewer remains population-empty because it lacks the deterministic complete-residency startup gate. The binary heap is accepted for the measured initial scale, not frozen as the ten-million-agent scheduler.

## D-038: Sparse occupancy, bounded objective perception, and recomputed local routes

Date: 2026-07-16

**Supersedes:** D-037 only where Slice 0 deliberately permitted shared cells and lacked occupancy, perception, and routes. Its dense identity, compact position, time, scheduler, movement-generation, and reset decisions remain active.

**Decision:** Permit exactly one living agent per cell. Keep moving agents authoritative at their source until scheduled completion. Group occupancy by signed 64 x 64 world chunks in a private `BTreeMap`; each bucket stores sorted eight-byte `(u16 row-major local cell, AgentId)` entries. Build the index atomically with population initialization and transfer source-to-target only after revalidating terrain and verifying both index endpoints. Reject an already occupied request target, but do not reserve an empty one. When events contend for the same empty cell at equal time, reuse the existing `(time, class, AgentId, sequence)` order: the lower `AgentId` transfers first and later contenders receive a typed occupied outcome without partial mutation.

Bound objective perception to an active-area-clipped square radius through 31 cells and return agent, drinkable-water, immutable-resource, and traversable-cell facts in global row-major order. Do not create structure identity before the Slice 6 simulation-owned structure store exists; extend this same query boundary then. Route cardinally with deterministic minimum-travel-time Dijkstra using `TraversalStep::cost`, a caller-selected positive expansion budget capped at 4,096, fixed neighbor order, and world-position tie-breaks. Store only optional compact destination plus budget per agent. Reuse one engine-owned node/hash/frontier scratch set and recompute a next step after completion or an occupancy conflict; never retain a path vector or scan routes each tick.

**Reason:** A dense active-area occupancy grid would cost 64 MiB for a 4,096 x 4,096 rectangle before population data, while one tree node per agent would add pointer-heavy overhead. Chunk buckets preserve spatial locality and signed-coordinate correctness with one compact entry per agent. Completion-time arbitration is independent of client command insertion order; target reservation would not be. Weighted routing respects actual traversal time rather than treating every terrain edge equally, while a hard budget and destination-only state bound both work and retained memory. Reusable scratch prevents steady route progress from allocating fresh search buffers.

**Consequences:** `CellOccupant` is eight bytes/alignment four, `RouteState` is six bytes and `Option<RouteState>` is eight bytes, and route scratch node/frontier records are each 12 bytes/alignment four. The 31-cell maximum perception radius inspects at most 3,969 cells before active-edge clipping. Routes may repeat bounded search work at each waypoint instead of retaining complete paths; current release evidence records a 75-expansion search at 12,798 ns with zero scratch-buffer growth after warm-up. Dynamic occupancy can change the next route step, but immutable terrain and irrelevant world revisions require no invalidation record. Structure perception remains a documented Slice 6 extension rather than speculative Slice 1 state.

## D-039: Fixed-point analytical needs and one-shot threshold wakeups

Date: 2026-07-16

**Supersedes:** D-037's event order only by inserting need-threshold class and need-kind ranks while preserving its time, agent, and checked monotonic-sequence tie-breakers. D-037/D-038 movement, occupancy, and route decisions otherwise remain active.

**Decision:** Store one parallel 32-byte `NeedState` per agent for hunger, thirst, rest, and physical exposure. Each need uses a saturated 0-10,000 `u16` visible value, a signed `i8` rate per 60 fixed `SimTime` ticks, and a retained 0-59 remainder; the four needs share one `SimTime` reference, `u32` generation, and crossed-threshold mask. Rebase from the exact value numerator only when an activity/rate changes. Use integer ceiling division to predict future crossings and schedule no event for a zero or recovering rate, a crossed threshold, or time overflow.

Use one actionable threshold per need in Slice 2: hunger 7,000, thirst 6,000, rest 8,000, and exposure 7,000. A reached threshold emits one wake/debug outcome and remains crossed until a later exact rebase places it below the boundary; health severity and death bands remain Slice 7 work. Rate profiles are idle `(2,4,1,0)`, moving `(3,6,3,1)`, gathering `(4,7,4,1)`, building `(5,8,5,1)`, and sleeping `(1,2,-8,2)` in hunger/thirst/rest/exposure order. At Slice 2 completion the gathering, building, and sleeping profiles were deliberately unreachable future-action boundaries; Slice 4 later activated gathering without changing these rates. Neutral idle exposure schedules no event; later climate, sleep-location, and shelter work may replace its provisional physical rate without introducing emotion or social safety. Need generations wrap deliberately: the maximum retained heap bound at the ten-million-agent limit is far below `u32::MAX`, and scheduling compacts stale generations before an ancient value can alias a current event.

Extend the complete scheduler key to `(due time, event class, AgentId, event-detail rank, monotonic sequence)`. Need thresholds rank before movement at the same time, and same-agent needs rank hunger, thirst, rest, then exposure. This observes the finishing activity's exact accumulated state before movement completion rebases its rates. One shared need generation invalidates every earlier threshold after a rate change. Initial population publication reserves scheduler storage and creates applicable thresholds before becoming authoritative; scheduler compaction recognizes up to four current need events plus movement per agent and remains behind the private heap boundary.

**Reason:** Exact analytical values remove idle per-agent tick work, while retained remainders prevent repeated short activities from losing fractional progress. A shared reference and generation keep state pointer-free and smaller than four independent `LinearNeed` records. One threshold provides the Slice 3 wake seam without prematurely freezing health/death semantics. Explicit detail rank prevents insertion order from choosing equal-time need priority, and threshold-before-movement avoids invalidating a crossing at the exact completion time.

**Consequences:** `NeedState` is 32 bytes/alignment eight and `ScheduledEvent` remains 32 bytes/alignment eight. Idle initialization creates three threshold events per agent; exposure adds none until its physical rate is positive. A rate change may append up to four new events and leave earlier generations harmlessly stale, so the heap remains an intentionally temporary 20-100-agent scheduler. Multi-waypoint routes remain one continuous moving activity and do not rebase/reschedule needs at each waypoint. If the checked global event sequence is exhausted while completing movement, the agent settles idle without moving and emits a typed terminal scheduling failure rather than remaining stranded as moving. `Engine::physical_needs` evaluates read-only current state without mutation, and `Engine::need_threshold_outcomes` exposes reached/stale work from the latest advancing tick. No floating point, world mutation, cognition, health consequence, or presentation-owned state enters the authoritative need loop.

## D-040: Explicit compact physical policy with single commitments and bounded retry

Date: 2026-07-16

**Extends:** D-037 through D-039 by connecting movement, occupancy/perception/routes, and analytical need wakeups. It does not supersede their state ownership, event generation, or deterministic ordering decisions.

**Decision:** Keep the physical action policy in a private headless `sim-core::policy` module and activate it only through explicit fallible `Engine::activate_physical_policy` after population initialization while every agent is idle. Reject activation with the first canonical committed `AgentId` rather than inheriting a manual movement/route commitment. Do not make initialization auto-start agents, and do not activate it in the viewer before the viewer owns an equivalent deterministic residency gate. Store one parallel 12-byte, pointer-free `PolicyState` per agent with compact target, `u32` stale-event generation, one-byte goal/phase/reason, and capped retry count. Define one-byte goals for seek water, seek food, gather material, eat, drink, sleep, seek shelter, build shelter, wait, and incapacitated. Variants that require inventory, resource deltas, structures, or health remain dormant until their owning later slice supplies those objective facts; the policy must not invent placeholder state.

Rank reached needs by normalized integer value/threshold score, then explicit thirst, exposure, hunger, rest priority. Select currently perceived water/food access by Manhattan distance, row, then column. Do not add keyed randomness while these physical keys completely settle ties; add a purpose-keyed deterministic stream only if a later candidate domain contains physically identical choices after all meaningful keys. Read only `PhysicalNeedsView` and bounded radius-eight `PhysicalPerception`, never belief, memory, relationship, personality, language, presentation, wall-clock, or unordered iteration state.

Extend the scheduler's complete class rank to need threshold, action completion, movement, decision. Keep the remaining due time, `AgentId`, class-detail, and checked sequence keys. A policy generation makes only one decision/action commitment current. Once activated, reject public manual move/route requests with typed `PolicyControlled` outcomes; internal policy routing remains the sole commitment path until reset. A newly reached need may interrupt an existing policy action or route: precheck every needed event and generation increment, invalidate movement and policy work, clear the route, rebase activity idle, then schedule reconsideration one tick later. A route arrival also schedules its next decision one tick later. A no-target or typed perception/route/action failure schedules positive exponential retry from 60 ticks, capped at 1,920; no failure may schedule a same-time reaction. Emit latest-tick typed diagnostics for selection, route/action start, deferral, retry, and stale work. Do not persist diagnostics as simulation truth.

Action completions in Slice 3 validate the scheduling seam but explicitly report `DeferredToLaterSlice`: drinking/eating/gathering/inventory/depletion are Slice 4, full sleep/wake is Slice 5, shelter/building is Slice 6, and incapacitation/death is Slice 7. `sim-headless` activates the policy and counts typed selections/failures; manual engine users remain able to initialize an inert population and issue explicit movement/route requests.

**Reason:** An explicit activation boundary preserves deterministic terrain residency and existing manual test/tool behavior. One small parallel state avoids widening the hot position record or allocating an object graph per agent. Integer scores and physical tie keys make changes to irrelevant perception facts unable to reorder a choice. Separate decision/completion events make commitment boundaries inspectable, and positive capped retry prevents unreachable or unavailable targets from becoming an unbounded same-time loop. Deferring effects to their owning stores avoids turning generated base resources or presentation observations into hidden mutable truth.

**Consequences:** `PolicyState` is 12 bytes/alignment four, latest-tick `PolicyDiagnostic` is 40 bytes/alignment eight, and `ScheduledEvent` remains 32 bytes/alignment eight. Population initialization reserves at most two diagnostics per agent up to the due-event ceiling. One initial decision plus policy state adds 44 logical bytes per active agent before allocator metadata and existing need/movement state. The release harness retained 880, 4,400, and 440,000 logical bytes for exact-capacity 20, 100, and 10,000-agent policy-plus-one-decision workloads, with zero heap growth. Radius-eight decisions inspect at most 289 cells and only occur on activation, threshold wakeup, arrival/completion, or delayed retry; idle ticks still do no population scan. At Slice 3 completion drink/eat/sleep could not satisfy needs and gathering could not mutate resources; D-041 activates gather/eat/drink effects while sleep, shelter, and health remain deferred.

## D-041: Compact fixed inventory and sparse permanent resource depletion

Date: 2026-07-16

**Extends:** D-034, D-036, and D-037 through D-040 by adding the dynamic resource state and action effects that those decisions explicitly deferred. It preserves immutable generated features, resident query errors, analytical need state, and total scheduler ordering.

**Decision:** Store one parallel three-byte `InventoryView` per agent with independent `u8` food, wood, and stone amounts capped at 32 units each. Do not allocate item records, slots, or ownership objects in Phase 2. A gather action lasts the existing 60 action ticks and transfers at most four abstract units, further clamped by the selected feature's remaining capacity and the matching inventory slot. Eating consumes one food and reduces hunger by 4,000; drinking stores no water and reduces thirst by 5,000. Rebase the affected analytical need exactly at completion, preserve its fractional remainder, advance the shared need generation, recompute crossed flags, and schedule replacement thresholds. Do not change any other need.

Keep `World::resource_at` as immutable generated capacity. `Engine` owns one sparse deterministic `BTreeMap` from checked `i16` feature position to changed `u16` remaining capacity. Create the entry only on the first successful gather, retain zero as permanent depletion for Phase 2, and do not schedule regrowth. `Engine::available_resource_at` and bounded physical perception compose base capacity with this store and omit depleted resources. Reset clears the entire dynamic store and inventories without changing world residency or generated records.

Idle agents may gather the nearest perceived resource for which their matching inventory slot has capacity. Urgent hunger consumes carried food or seeks food and gathers it first. At completion, revalidate a resource at the agent's same/cardinal cell; if hunger caused the gather, accept food only. Revalidate drinking at the committed land access against same/cardinal resident lake or river water. Reject ocean-only, unloaded, outside-world, stale/nonadjacent, depleted, full-inventory, and no-food cases with typed diagnostics. Apply equal-time completions through the existing total event order, so lower `AgentId` deterministically receives contested final units. Successful effects schedule reconsideration one tick later; failures use the existing positive bounded retry.

**Reason:** Fixed abstract amounts prove carrying and consumption without premature item identity or ownership systems. Independent caps keep the layout at three bytes and prevent collected construction material from making food impossible. A sparse changed-only layer preserves immutable/versionable procedural generation and charges memory only for interacted features. Permanent depletion is the smallest deterministic Phase 2 rule and avoids introducing an unproven regrowth scheduler. Reusing event order makes contention deterministic without locks or reservations.

**Consequences:** Inventory costs exactly three logical bytes per agent with alignment one. A compact changed-feature key plus remaining capacity is six logical bytes/alignment two before `BTreeMap` node and allocator overhead. The release harness retained 60, 300, and 30,000 inventory bytes for exact 20, 100, and 10,000-agent populations with no capacity growth. One measured 120-unit wood feature retained one delta through 120 one-unit gathers and completed the optimized local loop in 9,100 ns; this is an observation, not a regression limit. Perception now reports current availability rather than immutable capacity, while `World` remains unchanged. Regrowth, item identity, ownership, recipes/tools, water quantity, persistence, and scale beyond the 20-100-agent Phase 2 proof remain deferred.

## D-042: Analytical quality-based sleep with threshold-first interruption

Date: 2026-07-16

**Extends:** D-039 through D-041 by making the provisional sleeping need profile and `PhysicalGoal::Sleep` executable. It preserves analytical need evolution, compact parallel population storage, checked policy generations, and the existing deterministic event key.

**Decision:** Store one fixed 24-byte `SleepState` per agent outside the six-byte hot record. It contains start time, planned wake time, one-byte quality, and an active flag; position remains authoritative in `AgentRecord`, and the checked policy generation owns stale-wake invalidation. Define `OpenGround` recovery as eight rest-deficit units per 60 ticks with hunger `+1`, thirst `+2`, and exposure `+2`. Define the `Sheltered` quality discriminator and twelve-unit rest recovery now, but do not select it until Slice 6 supplies an authoritative completed structure. Predict wake directly from the exact fixed-point rest numerator with ceiling division and schedule one dedicated wake event. Do not periodically check sleepers.

Require the requested sleep cell to be inside the active area, resident, standable, owned by the requesting agent in the spatial index, equal to its current position, and below the actionable exposure threshold. Reject water, blocking features, another occupant, unloaded/outside coverage, unsafe exposure, remote targets, dead/missing agents, existing activity, and scheduling exhaustion explicitly. This below-threshold exposure check is the narrow provisional Slice 5 safety definition; terrain aesthetics or presentation state do not imply safety.

Order equal-time work as need threshold, wake, action completion, movement, then decision. Hunger, thirst, or exposure crossing interrupts sleep, rebases idle once, clears active sleep state, invalidates the wake through policy phase/generation, records the causal need, and schedules at most one reconsideration when autonomous policy is active. If event-sequence exhaustion prevents that reschedule, settle the agent idle and dormant so the urgent threshold cannot be bypassed or followed by a second wake. Rest crossings never interrupt recovery. Normal wake clears the sleep record, rebases idle, records completion, and schedules one later decision only for a policy-controlled population.

**Reason:** A generic 60-tick action restored only eight rest units, then reported sleep as deferred. Exact physical sleep requires a duration derived from accumulated fatigue and a single interruptible event boundary. A small parallel state keeps active timing inspectable without widening the hot movement record or allocating an object per sleeper. Threshold-first ordering reuses the established scheduler and guarantees urgent physical conditions win deterministically, including at the exact planned wake tick. Reserving quality in the contract lets Slice 6 add shelter benefit without changing event semantics or inventing structures early.

**Consequences:** `SleepState` is 24 bytes/alignment eight, or 480, 2,400, and 240,000 logical bytes at 20, 100, and 10,000 exact-capacity populations. The optimized harness scheduled seven retained records per fresh sleeper in its synthetic workload: three superseded idle thresholds, three current sleeping thresholds, and one wake, totaling 140, 700, and 70,000 records. Scheduling all sleepers took 4,500 ns, 11,800 ns, and 1,213,600 ns; extracting the simultaneously due wakes took 1,000 ns, 5,200 ns, and 876,600 ns on the recorded machine. These are observations, not regression limits; normal stale-event compaction and varied due times reduce retained/due concentration. Structure identity/occupancy, sheltered selection, environmental danger beyond exposure, sleep comfort, health/death consequences, persistence, and large-population scheduler replacement remain deferred.

## D-043: Sparse one-cell lean-to shelters with adjacent shared access

Date: 2026-07-16

**Extends:** D-038 through D-042 by activating structure perception, building policy goals, the building need profile, and sheltered sleep. Generated-world immutability, total event ordering, compact policy generations, and analytical needs remain unchanged.

**Decision:** Keep shelters in a private `Engine`-owned `StructureStore`, never in `World`, `TerrainCell`, or generated `Feature`. Allocate monotonic dense `u32` `StructureId` values and retain a fixed 32-byte lifecycle record containing compact footprint position, builder during construction, start/completion time, kind, and state. Index live footprints by `(i16 row, i16 column)` in a deterministic `BTreeMap` and active builds by `AgentId`. Cancelled slots remain tombstones so published identities are never reused; this is acceptable for the 20-100-agent proof and remains a scale budget for later measurement.

Define one blocking cell as the shelter footprint and any cardinal resident standable non-footprint cell as shared access. Shelters are unowned after completion; builder identity is lifecycle provenance only. Construction uses one provisional lean-to recipe of eight wood, zero stone, and 600 ticks. An earlier 8-wood/4-stone candidate failed the bounded-policy contract in canonical seed-42 trials: timber and stone could not both be found within radius eight even when the bootstrap was enlarged to 512 x 512. The wood-only recipe uses the measured 120-unit tree capacity, requires two four-unit gathers, and proves construction without whole-world search or premature recipes/tools.

Consume the complete recipe at construction start after validating the builder is idle, cardinally adjacent, funded, and inside the active area, and that the site is resident, dry, standable, agent-vacant, and structure-vacant. Reserve the footprint before another equal-time decision may observe it. A reached need cancels the active build, removes the footprint, refunds the complete recipe exactly once, rebases the builder idle, and leaves the old completion stale. Equal-time builders reuse the scheduler's `AgentId` order, so the lower ID reserves a contested site first. Recheck structures on direct movement request, route validation/search, and scheduled movement completion.

Completed shelters enter bounded row-major physical perception and are omitted from traversable cells. A sleeper on cardinal access receives `SleepQuality::Sheltered`: rest changes at `-12` and exposure at `-4` per 60 ticks, compared with open-ground `-8/+2`. Rest-driven sleep predicts full rest recovery as before. Exposure-driven shelter use predicts one wake at the maximum of full rest recovery and the exact fixed-point time required to fall below the 7,000 exposure threshold; it never creates a one-tick recovery loop. Shelter has no ownership, comfort, cognition, health, or social effects. Snapshot count, stable views, latest-tick diagnostics, headless counters, and reset provide inspection without presentation-owned truth.

**Reason:** A one-cell footprint plus adjacent access represents a real route obstacle without allowing an agent to occupy structure matter or requiring a second retained access field. Start-time consumption makes overlap and double-spend arbitration atomic; full interruption refund is simpler and more inspectable than partial progress loss. A sparse store charges only constructed shelters and preserves immutable procedural inputs. Sheltered exposure recovery supplies the smallest measurable physical safety effect while leaving injury and death to Slice 7.

**Consequences:** `StructureRecord` and `Option<StructureRecord>` are 32 bytes/alignment eight; the logical row/column-plus-ID footprint entry is eight bytes before `BTreeMap` node and allocator overhead. `ScheduledEvent` remains 32 bytes. The 2026-07-16 release harness retained 32/128/16,384 vector slots for 20/100/10,000 live shelters, or 1,024/4,096/524,288 slot bytes; combined structure insertion plus one build event took 14,500/17,900/1,962,200 ns, and equal-time due extraction took 3,200/3,600/513,800 ns. These single-run observations are not regression limits. Cancelled tombstone frequency, full allocator attribution for both maps, structure persistence, multi-cell buildings, ownership, repair, demolition, and scheduler replacement remain open beyond the Phase 2 proof.
