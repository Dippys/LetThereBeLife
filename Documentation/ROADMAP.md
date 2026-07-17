# Implementation Roadmap

Last synchronized: 2026-07-17.

## Completed

- Rust workspace bootstrap.
- Headless core and command-line runner.
- Native viewer window, input, fixed-step loop, and GPU renderer.
- Initial formatting, testing, and linting baseline.
- Repository-local agent skill and separate living-documentation system.
- Chunk-keyed generated world storage, background generation, bounded presentation queries, and `wgpu` rendering.
- Deterministic configurable initial-area generation with terrain and sparse surface features.
- Shared TOML runtime configuration for the viewer and headless runner.
- Cursor-anchored viewer zoom and terrain/feature hover inspection.
- Origin-centered camera navigation clamped to a visible red maximum-world boundary, with maximum zoom-out fitting the complete envelope.
- Selection-driven deterministic generation beyond the initial area.
- Read-only signed chunk-boundary inspection and explicit right-drag generation for missing terrain, while `sim-core` retains world ownership; viewpoint-triggered generation was removed.
- Deferred bootstrap coverage with clipped non-aligned tiles, explicit eager headless materialization, opaque seed/coverage-validated worker loads, job-ID cancellation, center-out paged bootstrap streaming, and progressive GPU cache synchronization.
- Three-tier integer world generation: analytic tectonic/climate fields, cached 4,096-cell regional drainage, and local chunk synthesis with static lakes, rivers, biomes, and sparse features.
- Region-aware chunk traversal, bounded regional caches, validated developer map sampling, and regression coverage for drainage borders, rasterized water, and complete chunk river indexing.
- Deterministic multi-threaded chunk generation with a fixed computation pool, bounded ordered result streaming, generation-ID cancellation, build-once shared regional caching, adaptive bounded main-thread insertion, and a same-seed release throughput harness.
- Parallel cold-region preparation, indexed regional macro/climate sampling, a wider bounded task window, and 32 x 32 bootstrap pages without changing generated chunk content across pool sizes.
- A centered 65,536 x 65,536-cell generation envelope (1,048,576 chunks, 16 GiB raw terrain at full residency), with origin-outward paging and core-enforced spatial bounds.
- World-foundation Slice 0: a repeatable four-seed, 12-view quality baseline with full-envelope, regional, both-axis drainage-seam, coastline, river-mouth, river-source, mountain, and close-up evidence; deterministic metadata/distribution/type reports; focused tooling tests; and recorded release timing and peak working set.
- World-foundation Slice 1: a bounded seed-keyed whole-envelope drainage skeleton with canonical basin/lake outlets, channel identities and confluences, continuous major rivers across signed 4,096-cell region seams, shared regional water sampling, deterministic pool/order regressions, compact layout bounds, and measured 128-versus-256-cell resolution evidence.
- World-foundation Slice 2: a finite cold-to-warm-to-cold latitude contract fitted to the complete envelope, four alternating wobbled prevailing-wind bands, complete-envelope multi-seed climate coverage, and allocation-free resident-cell temperature/moisture/wind inspection while retaining the four-byte terrain cell.
- World-foundation Slice 3: a one-byte packed surface/biome classification exposed through typed accessors, with distinct ocean/lake/river, beach/desert, grassland/savanna/forest/wetland, tundra/alpine, and snow/ice semantics; updated HUD/map palettes and review-format distributions; and no growth beyond the four-byte terrain cell.
- World-foundation Slice 4: sparse explicit lake-fed river sources with moisture-gated runoff, complete ocean/world-edge routes, bounded curved refinement, downstream water-surface grades, non-crossing geometry, hydrologic wetlands, riparian banks, bounded coherent coast/biome/snowline/treeline transitions, and a safe arbitrary-seed chunk-index overflow path.
- World-foundation Slice 5: deterministic canopy, grove, berry-patch, riparian, slope, and outcrop feature ecology; stone availability beyond mountain surfaces; derived compact food/wood/stone base capacities; an explicit future sparse depletion boundary; and full-resolution forest/outcrop/berry review evidence with record-footprint accounting.
- World-foundation Slice 6: viewer-owned power-of-two per-chunk summaries with dominant terrain, minority river/lake/coast/mountain preservation, density-scaled feature markers, bounded camera-margin residency, authoritative change-bound invalidation, deterministic parallel construction, and release cache/build/upload-enqueue measurements.
- World-foundation Slice 7: explicit residency-aware cardinal traversal, fresh/salt-water, and immutable resource queries; stable generated feature identity; a public-only deterministic settlement-candidate scenario; and a documented immutable-base, future sparse-delta, dynamic-entity handoff.
- Physical-agent Slice 0: atomic resident-area population initialization, opaque dense IDs, compact checked positions/activity, bounded totally ordered event scheduling, lazy stale-event cancellation, scheduled cardinal movement with typed outcomes, reset/replay semantics, a 20-agent public headless proof, and recorded 20/100/10,000-agent layout/scheduler measurements.
- Physical-agent Slice 1: sparse chunk-bucketed multi-agent spatial positions, exact source-validated overlap-tolerant transfers, bounded row-major physical perception, deterministic budgeted minimum-travel-time local routes, compact destination-only route state, scheduled route continuation, public shared-destination/no-path/budget/arrival proofs, and recorded 20/100/10,000-agent spatial/perception plus reusable-route measurements.
- Physical-agent Slice 2: 32-byte fixed-point hunger/thirst/rest/exposure state with exact remainder-preserving activity rebasing, scheduled one-shot actionable thresholds, deterministic threshold-before-movement and need-kind priority, typed reached/stale outcomes, public analytical inspection, replay/pause/reset proofs, and recorded 20/100/10,000-agent scheduling/extraction measurements.
- Physical-agent Slice 3: explicitly activated compact deterministic physical policy, single route/action commitments, bounded radius-eight decisions, typed diagnostics, need interruption, positive capped retry, public activation/drink/no-target proofs, and recorded 20/100/10,000-agent policy/event measurements.
- Physical-agent Slice 4: three-byte fixed inventories, scheduled deterministic gather/eat/drink effects, composed resource perception, permanent sparse generated-feature depletion, equal-time contention, explicit ocean/unloaded/no-food/overflow failures, public immutable-base/depletion proof, and recorded inventory/delta measurements.
- Physical-agent Slice 5: 24-byte parallel sleep state, explicit valid-location sleep intent, analytical quality-based rest recovery, dedicated threshold-ordered wake events, hunger/thirst/exposure interruption, public sleep diagnostics, headless counters, and recorded 20/100/10,000-agent state/event measurements.
- Physical-agent Slice 6: sparse simulation-owned one-cell lean-to construction, atomic eight-wood reservation/refund, structure-aware perception/traversal, completed adjacent sheltered sleep, deterministic overlap arbitration, and recorded structure/index/event measurements.
- Physical-agent Slice 7: 16-byte scheduled health state, explicit severe hunger/thirst/rest/exposure consequences, stable multi-cause precedence, incapacitation, terminal physical death with occupancy/event cleanup, retained causal records, public health/death inspection, and headless cause counts.
- Physical-agent Slice 8 and Phase 2 exit: reusable canonical seed-1 20/100-agent headless scenarios over a 2,048-cell square, explicit recorded starting supplies and freshwater/fallback spawn roles, equality-stable causal reports plus versioned semantic hashes, replay/divergence/batching/reset proofs, cumulative scheduler/route/perception/capacity diagnostics, 600,000-tick release soaks with mixed survival/dehydration outcomes, and zero sampled invariant violations.
- Physical-agent viewer presentation: tick-zero readiness gating on a fully resident centered 2,048 x 2,048 rectangle that also becomes the first population's baseline execution area, zero automatic agents, individual exact resident cursor spawning with `T`, deterministic active-area expansion across loaded terrain without storage-chunk behavioral boundaries, bounded exploration for unresolved local objectives, 1-9 power-of-two speed controls through 256x, reset to zero agents, read-only capped agent/shelter GPU instances, activity/lifecycle colors, far-zoom hiding, population HUD counts, spawn feedback, and a complete mirrored top-right agent hover card including current physical-policy reason and terminal cause/time.
- Interactive world-object spawning: a numpad-navigated bottom-left viewer menu and repeated left-click placement for walkable trees, berries, and rocks plus blocking fresh water, backed by a deterministic sparse `sim-core` overlay composed into agent perception, resources, drinking, routing, movement revalidation, spawn/sleep/build validity, read-only rendering, and reset.
- `sim-core` maintainability pass: split world queries and deterministic visitation into responsibility-focused private modules, and move the large world/worldgen unit suites out of production entry files without changing public APIs or test paths.

## Active

- Phase 2 is complete. Phase 3 is now defined by the canonical [beliefs, memory, and relationships implementation plan](PHASE3_BELIEFS_RELATIONSHIPS_PLAN.md). Slice 0, cognitive storage and event foundation, is the next implementation boundary.
- Treat generated terrain as intentionally unstable while there is no save/load or other persisted world output. Generator versioning is not current work.

## Next

The completed Phase 1 sequence remains recorded in [WORLD_FOUNDATION_PLAN.md](WORLD_FOUNDATION_PLAN.md). Phase 2 scope, dependencies, per-slice deliverables, acceptance criteria, measurements, documentation duties, and explicit deferrals are canonical in [PHYSICAL_AGENT_PLAN.md](PHYSICAL_AGENT_PLAN.md).

- Implement only Phase 3 Slice 0: benchmark and establish compact cognitive storage, validated record identity, bounded cognition triggers, reset/death semantics, copied diagnostics, and exact performance evidence before semantic belief records affect behavior.
- After Slice 0 validates the storage boundary, proceed one coherent slice at a time through direct observation, environmental beliefs, survival planning, episodic memory, sparse relationships, consequences, and the integrated Phase 3 proof.
- Keep future agent/structure presentation extensions read-only; the viewer still owns no authoritative physical state.
- Use the representative-seed review set only when an observed simulation failure requires another generator change.

## Deferred

- Generator versioning, save/load, chunk persistence, and unloading policy are deferred until persisted world output is about to be introduced and the generator is ready to stabilize. Version identifiers must still be added before the first durable world format ships.
- Society and richer sprite/animation/asset presentation remain deferred until canonical plans establish concrete requirements.

## Planned

Phase 3 implementation follows [PHASE3_BELIEFS_RELATIONSHIPS_PLAN.md](PHASE3_BELIEFS_RELATIONSHIPS_PLAN.md). Phase 2 physical-agent behavior is an implemented dependency, not active feature work. Nonverbal communication, language, families, settlements, economy, and institutions remain later phases.

## Open

- Texture/sprite asset strategy beyond the current instanced GPU terrain renderer.
- Exact Phase 0 benchmark reporting format and resident-memory/frame-time instrumentation beyond the focused generation-pool harness.
