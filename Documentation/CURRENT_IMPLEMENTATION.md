# Current Implementation

Last synchronized: 2026-07-12.

## Implemented

- Rust 2024 Cargo workspace containing `sim-core`, `sim-server`, and `sim-viewer`.
- Engine-independent `sim-core` configuration, deterministic tick counter, pause/reset/speed commands, and immutable `SimulationSnapshot` output.
- Fixed-step accumulator in `sim-viewer`, separating variable render timing from 60 Hz simulation ticks and capping large frame delays.
- Native `winit` window and event lifecycle.
- Temporary `softbuffer` framebuffer renderer with simulation-driven visual indicators.
- Keyboard controls: pause/resume, 1x–8x speed selection, reset, and exit.
- Headless runner accepting `--ticks` and `--seed`.
- Unit tests covering equal-input tick determinism, pause behavior, and reset behavior.
- Repository-local skills for orientation, Rust implementation, living documentation, validation, architecture/code review, and workflow evolution.
- Root agent routing with a mandatory inspect, implement, document, review, and validate lifecycle.
- A checksum manifest that detects any change under immutable `InitialDocumentation/`.

## Not implemented

- World coordinates, chunks, generation, terrain, or resources.
- Persistent agents, needs, cognition, movement, or event scheduling.
- Deterministic RNG streams, save/load, snapshots on disk, or replay logs.
- Production renderer, camera, UI text, assets, or inspection tools.
- Region workers, parallel simulation, networking, or long-term persistence.
