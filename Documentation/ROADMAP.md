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

## Next

- Extend the focused generation-pool throughput harness into controlled first-window, first-terrain, full-bootstrap-completion, frame-hitch, resident-memory, and allocation measurements; use them to set explicit runtime budgets.
- Add stable generator versioning before any persisted world output, then define chunk unloading and persistence policies around the deduplicated chunk store.
- Extend the current region-local static drainage into cross-region basins, tributaries, local streams, wetlands, erosion, and dynamic water only when their ownership, persistence, and performance budgets are specified.

## Planned

Follow the phases described in `InitialDocumentation/16_IMPLEMENTATION_ROADMAP.md`, using that file only as immutable design input. Record implementation status here as milestones land.

## Open

- Texture/sprite asset strategy beyond the current instanced GPU terrain renderer.
- Exact Phase 0 benchmark reporting format and resident-memory/frame-time instrumentation beyond the focused generation-pool harness.
