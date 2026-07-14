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
- Unbounded left-drag camera panning across generated and unrendered space.
- Selection-driven deterministic generation beyond the initial area.
- Read-only signed chunk-boundary inspection and event-driven automatic generation for visible missing terrain, while `sim-core` retains world ownership.
- Deferred bootstrap coverage with clipped non-aligned tiles, explicit eager headless materialization, opaque seed/coverage-validated worker loads, job-ID cancellation, center-out paged viewport streaming, and progressive GPU cache synchronization.
- Three-tier integer world generation: analytic tectonic/climate fields, cached 4,096-cell regional drainage, and local chunk synthesis with static lakes, rivers, biomes, and sparse features.
- Region-aware chunk traversal, bounded regional caches, validated developer map sampling, and regression coverage for drainage borders, rasterized water, and complete chunk river indexing.

## Next

- Record controlled release measurements for first window, first streamed terrain, full-bootstrap throughput, frame hitches, and resident memory; use them to set explicit runtime budgets.
- Add stable generator versioning before any persisted world output, then define chunk unloading and persistence policies around the deduplicated chunk store.
- Extend the current region-local static drainage into cross-region basins, tributaries, local streams, wetlands, erosion, and dynamic water only when their ownership, persistence, and performance budgets are specified.

## Planned

Follow the phases described in `InitialDocumentation/16_IMPLEMENTATION_ROADMAP.md`, using that file only as immutable design input. Record implementation status here as milestones land.

## Open

- Texture/sprite asset strategy beyond the current instanced GPU terrain renderer.
- Exact Phase 0 benchmark harness and reporting format.
