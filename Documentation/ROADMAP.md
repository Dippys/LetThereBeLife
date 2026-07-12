# Implementation Roadmap

Last synchronized: 2026-07-12.

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

## Next

- Add chunk-boundary inspection and automatic on-demand generation without moving world ownership into the viewer.
- Add chunk unloading and persistence policies around the deduplicated chunk store.
- Add regional hydrology and stable generator versioning before persistence.

## Planned

Follow the phases described in `InitialDocumentation/16_IMPLEMENTATION_ROADMAP.md`, using that file only as immutable design input. Record implementation status here as milestones land.

## Open

- Texture/sprite asset strategy beyond the current instanced GPU terrain renderer.
- Exact Phase 0 benchmark harness and reporting format.
