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
