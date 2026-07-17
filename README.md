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
runner explicitly materializes it before ticking. The viewer holds simulation time at tick zero until
its fixed centered 2,048 x 2,048 simulation rectangle is completely resident, then waits for
individual `T` cursor spawns rather than creating agents automatically. The viewer presents up to
4,096 deterministic physical agents and activates the core-owned policy. This is a deterministic loading target, not the
intended maximum world size. Restarting with the same configuration reproduces the same continental oceans, coasts,
plate/climate fields, whole-envelope static lakes, cross-region major river channels, and sparse
surface features. These channels are static deterministic terrain generation, not a
time-varying water simulation.

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

- The in-game HUD shows run/pause state, speed, simulated time, tick, seed, loaded chunks, world revision, generation status, and total/living/active/dead agent counts. Move the pointer over the map to add world, chunk, and chunk-local coordinates plus unloaded-initial, unloaded-partial-initial, initial, partial-initial, retained, retained partial-initial, or missing coverage. A hovered agent also reports its stable ID and activity. Loaded cells show terrain, elevation, temperature, zero-to-255 moisture, prevailing wind (`NW` or `SE`), and feature; without a hovered cell, the HUD shows a compact control guide. The inspected chunk is outlined gray while configured but unloaded, blue for initial, yellow for partial-initial, green for retained (including retained partial-initial), or red for missing coverage when it is large enough to read on screen.
- Scroll the mouse wheel to zoom in or out around the pointer. Maximum zoom-out fits the complete red world square; with the repository's 4,096 x 4,096 bootstrap this is 1/16x of the initial fit.
- Hold the left mouse button and drag to move the camera within the centered world envelope. Camera movement, resizing, and zooming do not generate terrain.
- Hold the right mouse button and drag to preview a translucent selection. A drag started inside the red maximum-world boundary caps at that boundary if the pointer moves beyond it. The preview is yellow while the bounded request is within the missing-chunk and expansion-capacity limits; otherwise it is red. Release generates the accepted in-bounds missing terrain and queues it ahead of bootstrap work. Outside the configured bootstrap area, this is the only viewer action that requests terrain.
- A selection may span more than 65,536 chunks when it overlaps loaded terrain, but one release can add at most 65,536 previously missing chunks. The complete world envelope is exactly 1,024 x 1,024 chunks (1,048,576 chunks) from `-32,768` inclusive to `32,768` exclusive on both axes. Its 4,294,967,296 cells equal 16 GiB of raw four-byte terrain only; sparse features, maps, and allocator overhead are additional.
- Startup bootstrap generation streams deterministic origin-centered 32 x 32 chunk pages through the bounded generation pool and applies completed chunks in request order. Right-drag work preempts this background bootstrap; camera/view changes never queue terrain. A persistent red square marks the maximum generatable boundary.
- Physical agents and one-cell shelter footprints are read-only colored GPU rectangles at close and moderate zoom. After the fixed simulation rectangle finishes loading, move the cursor over a standable cell and press `T` for each agent you want to add. Viewer agents use deterministic, directionally persistent bounded exploration when no local objective is visible; they do not abandon valid fresh-water or completed-shelter access merely to wander. Hover an agent to see its current search heading with the rest of its physical state.
- Press numpad `5` to open the bottom-left spawn menu. Use numpad `2`/`8` (or `4`/`6`) to select tree, berries, rock, or fresh water, then press numpad `5` again to enter placement mode. Left-click repeatedly on resident empty cells to place the selected kind; press numpad `5` to return to the menu or numpad `0` to close it and restore left-drag panning. Placed trees, berry bushes, and rocks are walkable and provide wood, food, or stone; placed water is drinkable but blocks walking. `R` clears these spawned objects with the rest of dynamic simulation state.
- The bottom time-square remains visible on a subtle rail and traverses it once per 60 simulated seconds; pausing also stops this motion.
- Already loaded chunks and selection portions fully covered by the initial rectangle consume none of the manual-request budget. A boundary chunk still counts when the selection extends from its initial-area portion into unloaded terrain. At distant zoom levels, rendering samples terrain near screen-pixel density and splits large instance uploads into bounded GPU buffers.
- `C`: stop remaining generation work and drop pending bootstrap pages/selections (chunks already applied stay loaded); later accepted right-drag selections can queue new manual work
- `Space`: pause/resume simulation
- `1` through `9`: set simulation speed to 1x, 2x, 4x, 8x, 16x, 32x, 64x, 128x, or 256x
- `T`: spawn one physical agent on any standable resident cursor cell (up to the 4,096-agent viewer presentation limit); the first population can explore the complete ready simulation rectangle, and spawns beyond it expand that rectangle when the intervening terrain is loaded
- Numpad `5`: open the spawn menu, or confirm/return from placement; numpad `2`/`8` and `4`/`6`: change selection; numpad `0`: end spawning
- Hover an agent to open its top-right inspection card with identity, position, activity, current goal and reason, target/retry state, needs/rates/next threshold, inventory, health/next consequence, sleep/wake state, and death cause/time when terminal
- `R`: reset simulation time and dynamic state to zero agents while retaining terrain
- `Escape`: close

Run the headless engine smoke test with:

```powershell
cargo run -p sim-headless -- --config config/simulation.toml --ticks 600 --seed 1
```

The headless runner also accepts `--agents NUMBER` (default 20) and `--batch-size NUMBER`. It selects deterministic bounded spawn roles, explicitly activates physical policy, and emits one compact versioned causal report/hash. Configured runs use zero starting supplies.

Run the complete Phase 2 canonical survival scenario with:

```powershell
cargo run --release -p sim-headless -- --canonical --agents 20 --batch-size 10000
cargo run --release -p sim-headless -- --canonical --agents 100 --batch-size 10000
```

Canonical mode fixes seed 1, a 2,048 x 2,048 resident world, 600,000 driver ticks, fresh-water and concentrated wood cohorts, 32 starting food units per agent, and eight starting shelter-wood units for the fresh-water cohort. The report includes final populations, causal death counts, actions/failures, resource/structure changes, scheduler/stale/retry work, and soak invariant results. These starting supplies are explicit scenario inputs, not generated-world mutations. The viewer starts with zero agents; each `T` press adds one exact cursor spawn and the first spawn activates the core-owned policy with bounded exploration.

Verify the workspace with `cargo test --workspace` and `cargo clippy --workspace --all-targets -- -D warnings`.

## Workspace

- `sim-core`: deterministic time, dense physical agents, event scheduling, engine lifecycle, world generation, sparse features, commands, and presentation snapshots
- `sim-config`: shared TOML configuration loading and validation
- `sim-headless`: minimal non-graphical simulation runner
- `sim-viewer`: native window, input, fixed-step loop, background chunk generation, and GPU presentation through `wgpu`
