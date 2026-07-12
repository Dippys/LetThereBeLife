# Current Architecture

## Runtime boundaries

```text
sim-core
  Owns deterministic simulation state, generated world terrain/features,
  commands, and snapshots.
  Has no windowing or rendering dependency.

sim-headless
  Loads shared configuration and runs sim-core headlessly for a requested number of ticks.

sim-viewer
  Loads shared configuration and owns native windowing, keyboard input, the fixed-step driver,
  camera/hover state, and temporary rendering. Reads SimulationSnapshot
  and immutable World data for presentation.
```

## Current contracts

- Presentation code mutates the engine only through `EngineCommand`.
- Presentation reads engine state through `SimulationSnapshot`.
- `Engine::tick` advances one deterministic simulation step when not paused.
- Viewer wall-clock time is accumulated and converted into fixed simulation ticks.
- Rendered objects are temporary views and are not persistent simulation entities.
- `World` is generated once from `EngineConfig::seed`; identical seeds produce identical row-major terrain and deterministically ordered sparse features.
- `WorldConfig` defines validated initial generation dimensions, defaulting to 1,024 x 1,024 cells. Those dimensions are the currently loaded/generated area, not a declared maximum world extent.
- Generation visits 64 x 64 chunks, while `World::cell` exposes coordinate lookup without presentation dependencies.
- Terrain uses dense `TerrainCell` values. Trees, rocks, and berry bushes use sparse `Feature` records rather than per-cell object slots.
- Elevation and moisture use integer-only, multi-scale world-coordinate noise. Generation does not shape terrain against the initial-area edges, preserving continuity for future adjacent chunks.
- `sim-viewer::camera::Camera` owns presentation-only center and zoom state. `CameraView` caches the per-frame transform used for screen/world conversion.
- Mouse-wheel zoom preserves the world point under the cursor. Left-button dragging translates the presentation camera without clamping it to the generated initial area.
- Camera space uses floating-point presentation coordinates and may move into negative or otherwise ungenerated locations; `World` lookup still returns only simulation-owned generated cells.
- `EngineCommand::GenerateWorldArea` routes right-drag generation into `sim-core`; `WorldRect` uses signed inclusive-minimum/exclusive-maximum coordinates.
- Generated selections are retained as dense patches with sparse features. Each command is capped at 1,048,576 cells, and fully generated selections are ignored.
- `World::feature_at` uses binary search over row-major sorted sparse features for bounded hover lookup without a global scan.

## Dependencies

- `sim-core`: Rust standard library only.
- `sim-config`: `sim-core`, `serde`, and `toml`; owns filesystem and TOML concerns shared by runtime binaries.
- The `sim-config` build script tracks the repository configuration and copies it into the active Cargo profile directory so directly launched binaries retain the default `config/simulation.toml` layout.
- `sim-headless`: `sim-config` and `sim-core`.
- `sim-viewer`: `sim-config`, `sim-core`, `winit`, and `softbuffer`.
