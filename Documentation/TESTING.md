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
It launches the hidden viewer for two rendered frames, validating GPU adapter/surface creation, WGSL pipeline layout, command submission, and presentation.

## Runtime checks

```powershell
cargo run -p sim-headless -- --ticks 600 --seed 42
cargo run -p sim-viewer
```

Expected headless result for the command above:

```text
completed tick=600 simulated_seconds=10.000 seed=42 initial_world=4096x4096
```

## Existing automated coverage

- Identical engine inputs yield identical snapshots after 1,000 ticks.
- A paused engine does not advance.
- Reset restores runtime state while preserving engine configuration.
- Equal seeds produce equal complete starter worlds.
- World generation is deterministic and different seeds change terrain.
- Generated samples contain terrain variation plus sparse features.
- A fixed seed-1 sample across 4,096 x 4,096 world cells verifies one boundary-connected continental ocean, bounded lake coverage, one to four coherent inland-lake components, and no single-sample lake puddles.
- Lake-descriptor invariants are exercised across 32 seeds and 81 positive/negative regions per seed: every accepted descriptor remains within its 1,024-cell region and contains a guaranteed 33 x 33 tested deep-water core.
- River-route invariants are exercised across 32 seeds and 81 positive/negative regions per seed: every accepted route stays inside its 2,048-cell region, contains 7 to 14 pre-lake continental-relief land nodes, begins at baseline elevation 50,001 or higher, strictly descends at each coarse node, keeps nonlocal width-expanded water-and-bank corridors separated, uses a non-reversing mouth heading, and terminates in pre-existing continental water. The repository seed produces one accepted major river in its 4,096 x 4,096 startup area.
- Full-resolution headwater and tapered-channel rasterization for the repository-seed river is verified as one four-neighbor-connected water component spanning baseline highland terrain to pre-existing continental water. A separate 16-cell-step overview regression verifies the same route remains one eight-neighbor-connected sampled component from its headwater to that water, matching the viewer's full-map terrain sampling scale. The repository-seed topology regression separately sees one boundary-touching continental-water component at a 32-cell sample step; it is not a full-resolution arbitrary-seed ocean-connectivity proof.
- A river-bearing negative-coordinate pair of independently generated chunks verifies deep/shallow water continuity and feature exclusion across a 64-cell seam while the underlying continental terrain is land.
- Initial-area generation matches independently generated 64 x 64 chunks for every terrain cell and sparse feature in a 128 x 128 overlap.
- `GroundType` remains one byte and `TerrainCell` remains four bytes; surface features remain excluded from water and sand.
- Terrain lookup accepts valid edge coordinates and rejects out-of-bounds coordinates.
- Rectangular initial-area generation preserves configured dimensions and exact cell count.
- Overlapping configured initial areas generate identical terrain and features at equal world coordinates.
- Invalid zero-sized or excessive initial allocations are rejected.
- TOML configuration parsing maps simulation/world settings and rejects invalid world dimensions.
- Sparse features remain row-major sorted and support coordinate lookup.
- The viewer camera initially centers/fits the world, preserves cursor anchoring during zoom, and can pan beyond generated world edges.
- Viewer zoom clamps at 1/16x of the initial fit rather than stopping at the startup framing.
- Selected patches generate deterministically in negative coordinate space. Generation budgeting counts only missing chunks: a selection spanning 4,097 chunks is accepted when one chunk is already covered (4,096 missing), while a selection requiring 4,097 new chunks is rejected. Unsafe-coordinate selections remain rejected while the representable negative coordinate edge is accepted.
- A non-aligned initial boundary counts as missing when a selected 64 x 64 chunk extends from its initial-area portion into unloaded terrain.
- Generated-world storage rejects inserts beyond its 16,384-chunk retained capacity.
- The worker reports invalid chunk coordinates, supports cancellation, applies no more than the configured per-frame chunk budget, and reports disconnection only once.
- Right-drag preview uses the same missing-chunk budget: a footprint larger than 4,096 chunks remains yellow when at most 4,096 are missing, a 4,097-missing request is red, and no preview starts while generation is active or its worker is unavailable.
- Chunk-keyed generation filters initial-area and existing-chunk overlap before worker generation and does not duplicate rendered cells.
- Stepped cell visits sample initial and generated chunks deterministically; renderer tests verify coarse instance reduction, power-of-two scale selection, initial/generated edge clipping, and the static-buffer partition boundary.
- Re-requesting existing chunks does not advance world revision.
- The viewer world-generation worker returns completed chunks independently of the event-loop thread.

## Known gaps

- GPU adapter/surface creation, shader binding layout, drawing, and presentation are covered by the automated hidden-window smoke run. Resize recovery and interactive event dispatch remain manual runtime checks.
- No property tests, canonical world-generation benchmark harness, generator checksums, full watershed/tributary tests, save/load tests, or long-running soak tests exist yet.
- Renderer `CameraUniform` and `Instance` sizes have compile-time assertions; broader foundational layout assertions, allocation measurements, and a canonical controlled release-benchmark harness do not exist yet. `Documentation/PERFORMANCE.md` records non-canonical local release measurements.
- Segmented multi-buffer drawing, deferred-sync completion/cancellation, and an interactive large right-drag generation path are not integration-tested; the hidden GPU smoke covers startup pipeline creation and presentation.
