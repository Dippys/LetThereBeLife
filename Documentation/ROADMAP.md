# Implementation Roadmap

Last synchronized: 2026-07-12.

## Completed

- Rust workspace bootstrap.
- Headless core and command-line runner.
- Native viewer window, input, fixed-step loop, and temporary framebuffer.
- Initial formatting, testing, and linting baseline.
- Repository-local agent skill and separate living-documentation system.
- Deterministic configurable initial-area generation with terrain and sparse surface features.
- Shared TOML runtime configuration for the viewer and headless runner.
- Cursor-anchored viewer zoom and terrain/feature hover inspection.
- Unbounded left-drag camera panning across generated and unrendered space.
- Selection-driven deterministic generation beyond the initial area.

## Next

- Add chunk-boundary inspection and on-demand generation without moving world ownership into the viewer.
- Replace retained generated-area patches with independently loadable, deduplicated chunks.
- Add regional hydrology and stable generator versioning before persistence.

## Planned

Follow the phases described in `InitialDocumentation/16_IMPLEMENTATION_ROADMAP.md`, using that file only as immutable design input. Record implementation status here as milestones land.

## Open

- Production rendering stack after the technical bootstrap.
- Exact Phase 0 benchmark harness and reporting format.
