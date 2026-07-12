# Testing and Validation

## Required Rust baseline

```powershell
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

The canonical full gate also verifies immutable documentation and all repository skill manifests:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .codex/skills/validate-rust-workspace/scripts/validate.ps1
```

The gate also verifies that `target/debug/config/simulation.toml` exists and is byte-equivalent to the repository configuration after the workspace build.

## Runtime checks

```powershell
cargo run -p sim-headless -- --ticks 600 --seed 42
cargo run -p sim-viewer
```

Expected headless result for the command above:

```text
completed tick=600 simulated_seconds=10.000 seed=42 initial_world=1024x1024
```

## Existing automated coverage

- Identical engine inputs yield identical snapshots after 1,000 ticks.
- A paused engine does not advance.
- Reset restores runtime state while preserving engine configuration.
- Equal seeds produce equal complete starter worlds.
- World generation is deterministic and different seeds change terrain.
- Generated samples contain terrain variation plus sparse features.
- Terrain lookup accepts valid edge coordinates and rejects out-of-bounds coordinates.
- Rectangular initial-area generation preserves configured dimensions and exact cell count.
- Overlapping configured initial areas generate identical terrain and features at equal world coordinates.
- Invalid zero-sized or excessive initial allocations are rejected.
- TOML configuration parsing maps simulation/world settings and rejects invalid world dimensions.
- Sparse features remain row-major sorted and support coordinate lookup.
- The viewer camera initially centers/fits the world, preserves cursor anchoring during zoom, and can pan beyond generated world edges.
- Viewer zoom clamps at 1/16x of the initial fit rather than stopping at the startup framing.
- Selected patches generate deterministically in negative coordinate space and reject excessive selections.

## Known gaps

- Viewer event dispatch, title updates, and framebuffer rendering have no automated tests yet; camera coordinate behavior is unit tested.
- No property tests, world-generation benchmarks, distribution snapshots, save/load tests, or long-running soak tests exist yet.
- No automated foundational type-size assertions, allocation measurements, or release-mode performance baselines exist yet.
