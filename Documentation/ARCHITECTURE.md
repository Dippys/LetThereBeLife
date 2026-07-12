# Current Architecture

## Runtime boundaries

```text
sim-core
  Owns deterministic simulation state, commands, and snapshots.
  Has no windowing or rendering dependency.

sim-server
  Runs sim-core headlessly for a requested number of ticks.

sim-viewer
  Owns native windowing, keyboard input, the fixed-step driver,
  and temporary rendering. Reads SimulationSnapshot for presentation.
```

## Current contracts

- Presentation code mutates the engine only through `EngineCommand`.
- Presentation reads engine state through `SimulationSnapshot`.
- `Engine::tick` advances one deterministic simulation step when not paused.
- Viewer wall-clock time is accumulated and converted into fixed simulation ticks.
- Rendered objects are temporary views and are not persistent simulation entities.

## Dependencies

- `sim-core`: Rust standard library only.
- `sim-server`: `sim-core`.
- `sim-viewer`: `sim-core`, `winit`, and `softbuffer`.

