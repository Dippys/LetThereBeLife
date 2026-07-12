# Current Implementation

Last synchronized: 2026-07-12.

## Implemented

- Rust 2024 Cargo workspace containing `sim-core`, `sim-config`, `sim-headless`, and `sim-viewer`.
- Engine-independent `sim-core` configuration, deterministic tick counter, pause/reset/speed commands, and immutable `SimulationSnapshot` output.
- Configurable initial world-area generation in `sim-core`, defaulting to 1,024 x 1,024 cells, divided into 64 x 64 generation chunks and stored as compact row-major terrain.
- Deterministic world-space integer noise for elevation and moisture, with deep water, shallow water, sand, grass, forest floor, hill, and bare-rock terrain that is independent of the initial-area boundary.
- Shared `sim-config` TOML loading for `sim-viewer` and `sim-headless`, with explicit validation of initial dimensions.
- `sim-config` build integration copies `config/simulation.toml` beside Cargo binaries under the active profile directory, including `target/debug/config/simulation.toml` and the equivalent release path.
- VS Code launch entries build and run viewer/headless binaries in debug or release mode with the repository configuration and working directory, plus a task for the complete workspace validation gate.
- Sparse deterministic tree, rock, and berry-bush feature records owned by the generated world.
- Fixed-step accumulator in `sim-viewer`, separating variable render timing from 60 Hz simulation ticks and capping large frame delays.
- Native `winit` window and event lifecycle with redraws capped at 60 Hz while active and suspended when paused and visually unchanged.
- GPU presentation through `wgpu`, using instanced terrain, feature, selection, hover, and status rectangles instead of CPU framebuffer rasterization.
- Camera-bounded GPU instance caches with a 128-cell margin, rebuilt only when the camera leaves the cached area or world revision changes.
- Deterministic 64 x 64 generated chunks keyed by signed `ChunkCoord`, with bounded chunk lookup instead of a growing linear patch search.
- A dedicated world-generation worker keeps selected-area generation off the window/event-loop thread and merges completed chunks into `sim-core` on the main thread.
- Viewer camera starting at full-map fit with cursor-anchored mouse-wheel zoom from 1/16x fit to 64x magnification and unbounded left-button drag panning through generated or empty space.
- Right-button drag selection with translucent yellow fill and border; release generates deterministic terrain and sparse features for the selected rectangle.
- Signed world positions supporting generated patches in negative and positive coordinate space.
- Hover inspection with a highlighted cell and window-title output for coordinates, terrain, elevation, moisture, and sparse feature kind.
- Keyboard controls: pause/resume, 1x–8x speed selection, reset, and exit.
- Headless runner accepting `--ticks` and `--seed`.
- Unit tests covering equal-input tick determinism, pause behavior, and reset behavior.
- Repository-local skills for orientation, Rust implementation, living documentation, validation, architecture/code review, and workflow evolution.
- Root agent routing with a mandatory inspect, implement, document, review, and validate lifecycle.
- A checksum manifest that detects any change under immutable `InitialDocumentation/`.

## Not implemented

- Streamed or unloadable chunks, regional hydrology, resource quantities, or terrain modification.
- Chunk-keyed expansion and persistence; selected expansion currently uses retained generated-area patches.
- Persistent agents, needs, cognition, movement, or event scheduling.
- General-purpose deterministic RNG streams, save/load, snapshots on disk, or replay logs.
- Text, asset, animation, and advanced inspection systems beyond the current GPU rectangle renderer.
- Region workers, parallel simulation, networking, or long-term persistence.
