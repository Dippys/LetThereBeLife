# Implementation Roadmap

Last synchronized: 2026-07-14.

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
- World-foundation Slice 0: a repeatable four-seed, 12-view quality baseline with full-envelope, regional, both-axis drainage-seam, coastline, river-mouth, lake, mountain, and close-up evidence; deterministic metadata/distribution/type reports; focused tooling tests; and recorded release timing and peak working set.
- World-foundation Slice 1: a bounded seed-keyed whole-envelope drainage skeleton with canonical basin/lake outlets, channel identities and confluences, continuous major rivers across signed 4,096-cell region seams, shared regional water sampling, deterministic pool/order regressions, compact layout bounds, and measured 128-versus-256-cell resolution evidence.

## Active

- Keep Phase 1 world foundation as the project focus until the generated world is coherent and convincing at full-map, regional, chunk-boundary, and close inspection scales. This intentionally extends world-generation work beyond the initial baseline described in `InitialDocumentation/16_IMPLEMENTATION_ROADMAP.md` before moving into agents or another simulation domain.
- Treat generated terrain as intentionally unstable while there is no save/load or other persisted world output. Seed output may change as world generation improves; generator versioning is not current work.

## Next

Detailed sequencing, implementation areas, acceptance criteria, performance checkpoints, and explicit deferrals for the remaining world work are maintained in [WORLD_FOUNDATION_PLAN.md](WORLD_FOUNDATION_PLAN.md).

- Implement world-foundation Slice 2 by choosing and recording the finite-world climate interpretation, then fitting temperature and prevailing-wind behavior to the actual envelope with complete-envelope coverage tests and inspection support.
- After the climate contract, refine terrain/biome semantics, coastlines, tributaries, local streams, wetlands, and sparse feature placement within explicit deterministic and performance bounds. Keep erosion and dynamic water for the point where their ownership and runtime budgets are defined.
- Use the implemented representative-seed review set when changing world generation, and turn durable multi-seed findings into deterministic quality thresholds only when they express intended world behavior rather than preserve the current repository seed.
- Extend the focused generation measurements where needed to protect world-generation iteration, including first terrain, bootstrap completion, frame hitches, resident memory, and allocations; use measured results to set explicit runtime budgets.

## Deferred

- Generator versioning, save/load, chunk persistence, and unloading policy are deferred until persisted world output is about to be introduced and the generator is ready to stabilize. Version identifiers must still be added before the first durable world format ships.
- Physical agents and later simulation phases remain deferred until the active world-foundation quality bar is met.

## Planned

After the active world-foundation focus is complete, resume the phases described in `InitialDocumentation/16_IMPLEMENTATION_ROADMAP.md`, using that file only as immutable design input. Record implementation status here as milestones land.

## Open

- Texture/sprite asset strategy beyond the current instanced GPU terrain renderer.
- Exact Phase 0 benchmark reporting format and resident-memory/frame-time instrumentation beyond the focused generation-pool harness.
