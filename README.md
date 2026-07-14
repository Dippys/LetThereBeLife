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

The configured bootstrap area in [`config/simulation.toml`](config/simulation.toml) currently
requests 4,096 x 4,096 cells. The Rust fallback is 1,024 x 1,024 when dimensions are omitted.
The viewer opens first, then streams that bootstrap area in prioritized chunk pages; the headless
runner explicitly materializes it before ticking. This is a deterministic loading target, not the
intended maximum world size. Restarting with the same configuration reproduces the same continental oceans, coasts,
plate/climate fields, region-local static lakes, drainage-derived river channels, and sparse
surface features. These channels are deterministic terrain generation, not dynamic or
cross-region water simulation.

```toml
[simulation]
seed = 1
ticks_per_second = 60

[world]
initial_width = 4096
initial_height = 4096
```

Use a different file with `cargo run -p sim-viewer -- --config path/to/file.toml`.

For a sampled BMP overview without opening the viewer, use the validated developer tool:

```powershell
cargo run --release -p sim-core --example render_map -- --width 4096 --height 4096 --step 8 --out map.bmp
```

`--width` and `--height` must be positive multiples of `--step`; the output is capped at 16,777,216 sampled pixels.

Cargo builds copy the default file to `target/<profile>/config/simulation.toml`, so the viewer and headless runner can also be launched directly from `target/debug` or `target/release` without manually copying configuration.

Viewer controls:

- Move the pointer over the map to inspect world, chunk, and chunk-local coordinates plus unloaded-initial, unloaded-partial-initial, initial, partial-initial, retained, retained partial-initial, or missing coverage in the window title. Loaded cells also show terrain, elevation, moisture, and feature. The inspected chunk is outlined gray while configured but unloaded, blue for initial, yellow for partial-initial, green for retained (including retained partial-initial), or red for missing coverage when it is large enough to read on screen.
- Scroll the mouse wheel to zoom in or out around the pointer, down to 1/16x of the initial full-area fit.
- Hold the left mouse button and drag to move freely, including beyond the currently generated area. Moving, releasing, resizing, or zooming replaces stale background work with a fresh visible-demand pager.
- Hold the right mouse button and drag to preview a translucent selection. It is yellow while the request is within the missing-chunk, expansion-capacity, and coordinate-safety limits; otherwise it is red. Release queues accepted manual work ahead of automatic/bootstrap work.
- A selection may span more than 4,096 chunks when it overlaps loaded terrain, but one release can add at most 4,096 previously missing chunks (16,777,216 generated chunk-payload cells). The configured bootstrap is separate from the 16,384 retained full-expansion-chunk capacity.
- Automatic generation streams one deterministic center-out 8 x 8 chunk page at a time, so zooming out does not make the viewer wait for every visible chunk before showing terrain. The nearest current view wins over background bootstrap work; stale automatic pages are cancelled after a view change. At extreme zoom, full-detail residency is still finite—overview LOD or unloading would be a separate future feature.
- Already loaded chunks and selection portions fully covered by the initial rectangle consume none of the manual-request budget. A boundary chunk still counts when the selection extends from its initial-area portion into unloaded terrain. At distant zoom levels, rendering samples terrain near screen-pixel density and splits large instance uploads into bounded GPU buffers.
- `C`: stop remaining generation work and drop pending pagers/selections (chunks already applied stay loaded); generation resumes only after a later view change
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
