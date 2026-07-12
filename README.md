# Let There Be Life

The first Rust bootstrap for the simulation engine. The simulation is a headless,
deterministic library; the windowed viewer displays its generated world and only
observes simulation-owned state.

Current implementation records, architecture decisions, roadmap status, and validation guidance live in [`Documentation/`](Documentation/README.md). `InitialDocumentation/` is read-only design input.

## Run

```powershell
cargo run -p sim-viewer
```

The viewer initially generates the area configured in [`config/simulation.toml`](config/simulation.toml),
which defaults to 1,024 x 1,024 cells. This is the startup area, not the intended
maximum world size. Restarting with the same configuration reproduces the same terrain.

```toml
[simulation]
seed = 1
ticks_per_second = 60

[world]
initial_width = 1024
initial_height = 1024
```

Use a different file with `cargo run -p sim-viewer -- --config path/to/file.toml`.

Cargo builds copy the default file to `target/<profile>/config/simulation.toml`, so the viewer and server can also be launched directly from `target/debug` or `target/release` without manually copying configuration.

Viewer controls:

- Move the pointer over the map to highlight a cell and show its coordinates, terrain, elevation, moisture, and feature in the window title.
- Scroll the mouse wheel to zoom in or out around the pointer, down to 1/16x of the initial full-area fit.
- Hold the left mouse button and drag to move freely, including beyond the currently generated area.
- Hold the right mouse button and drag to preview a translucent yellow selection; release to generate and render that world area.
- `Space`: pause/resume simulation
- `1`, `2`, `3`, `4`: set simulation speed to 1x, 2x, 4x, or 8x
- `R`: reset to the configured seed
- `Escape`: close

Run the headless engine smoke test with:

```powershell
cargo run -p sim-server -- --config config/simulation.toml --ticks 600 --seed 1
```

Verify the workspace with `cargo test --workspace` and `cargo clippy --workspace --all-targets -- -D warnings`.

## Workspace

- `sim-core`: deterministic time, engine lifecycle, world generation, sparse features, commands, and presentation snapshots
- `sim-config`: shared TOML configuration loading and validation
- `sim-server`: minimal headless runner
- `sim-viewer`: native window, input, fixed-step loop, and a software-rendered status view
