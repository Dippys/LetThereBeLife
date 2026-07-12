# Let There Be Life

The first Rust bootstrap for the simulation engine. The simulation is a headless,
deterministic library; the windowed viewer only consumes snapshots from it.

Current implementation records, architecture decisions, roadmap status, and validation guidance live in [`Documentation/`](Documentation/README.md). `InitialDocumentation/` is read-only design input.

## Run

```powershell
cargo run -p sim-viewer
```

Viewer controls:

- `Space`: pause/resume simulation
- `1`, `2`, `3`, `4`: set simulation speed to 1x, 2x, 4x, or 8x
- `R`: reset to the configured seed
- `Escape`: close

Run the headless engine smoke test with:

```powershell
cargo run -p sim-server -- --ticks 600 --seed 1
```

Verify the workspace with `cargo test --workspace` and `cargo clippy --workspace --all-targets -- -D warnings`.

## Workspace

- `sim-core`: deterministic time, engine lifecycle, commands, and presentation snapshots
- `sim-server`: minimal headless runner
- `sim-viewer`: native window, input, fixed-step loop, and a software-rendered status view
