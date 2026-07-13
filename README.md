# Let There Be Life

The first Rust bootstrap for the simulation engine. The simulation is a headless,
deterministic library; the windowed viewer displays its generated world and only
observes simulation-owned state.

Current implementation records, architecture decisions, roadmap status, and validation guidance live in [`Documentation/`](Documentation/README.md). `InitialDocumentation/` is read-only design input.

## Run

```powershell
cargo run -p sim-viewer
```

### VS Code

Install the recommended Rust Analyzer and CodeLLDB extensions, open **Run and Debug**, and choose one of:

- `Viewer (debug)` for breakpoints and development.
- `Viewer (release - smooth)` for optimized rendering and fullscreen testing.
- `Headless simulation (debug)` or `Headless simulation (release)` for a 600-tick run without presentation.

All launch entries build the selected binary first, use the repository root as the working directory, and pass `config/simulation.toml`. The **Run Task** menu also includes individual build tasks and `validate: workspace`.

The viewer initially generates the area configured in [`config/simulation.toml`](config/simulation.toml),
which currently requests 4,096 x 4,096 cells. The Rust fallback is 1,024 x 1,024 when
dimensions are omitted. This is the startup area, not the intended maximum world size.
Restarting with the same configuration reproduces the same continental oceans, coasts,
sparse inland lakes, and the highland-fed major river flowing into the continental-water coast.

```toml
[simulation]
seed = 1
ticks_per_second = 60

[world]
initial_width = 4096
initial_height = 4096
```

Use a different file with `cargo run -p sim-viewer -- --config path/to/file.toml`.

Cargo builds copy the default file to `target/<profile>/config/simulation.toml`, so the viewer and headless runner can also be launched directly from `target/debug` or `target/release` without manually copying configuration.

Viewer controls:

- Move the pointer over the map to highlight a cell and show its coordinates, terrain, elevation, moisture, and feature in the window title.
- Scroll the mouse wheel to zoom in or out around the pointer, down to 1/16x of the initial full-area fit.
- Hold the left mouse button and drag to move freely, including beyond the currently generated area.
- Hold the right mouse button and drag to preview a translucent selection. It is yellow while the request is within the missing-chunk, retained-capacity, and coordinate-safety limits; otherwise it is red. A new preview starts only when the background generator is available and idle, and release generates an accepted world area.
- A selection may span more than 4,096 chunks when it overlaps loaded terrain, but one release can add at most 4,096 previously missing chunks (16,777,216 generated chunk-payload cells). The bootstrap retains at most 16,384 generated chunks.
- Already loaded chunks and selection portions fully covered by the initial rectangle consume none of the per-request budget. A boundary chunk still counts when the selection extends from its initial-area portion into unloaded terrain. At distant zoom levels, rendering samples terrain near screen-pixel density and splits large instance uploads into bounded GPU buffers.
- `C`: stop the remaining generation work (chunks already applied stay loaded)
- `Space`: pause/resume simulation
- `1`, `2`, `3`, `4`: set simulation speed to 1x, 2x, 4x, or 8x
- `R`: reset to the configured seed
- `Escape`: close

Run the headless engine smoke test with:

```powershell
cargo run -p sim-headless -- --config config/simulation.toml --ticks 600 --seed 1
```

Verify the workspace with `cargo test --workspace` and `cargo clippy --workspace --all-targets -- -D warnings`.

## Workspace

- `sim-core`: deterministic time, engine lifecycle, world generation, sparse features, commands, and presentation snapshots
- `sim-config`: shared TOML configuration loading and validation
- `sim-headless`: minimal non-graphical simulation runner
- `sim-viewer`: native window, input, fixed-step loop, background chunk generation, and GPU presentation through `wgpu`
