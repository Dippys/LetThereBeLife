# Development Guide

## Setup

- Rust stable ≥ 1.85 (edition 2024). The toolchain lives on Windows; from WSL use `cargo.exe`.
- VS Code: install the recommended Rust Analyzer + CodeLLDB extensions. **Run and Debug** has
  `Viewer (debug)`, `Viewer (release - smooth)`, and headless debug/release entries;
  **Run Task** has builds and `validate: workspace`.
- `cargo build` copies `config/simulation.toml` into `target/<profile>/config/`, so binaries
  can be launched directly from `target/`.

## Run

```sh
cargo run -p sim-viewer                                  # interactive (use --release for smooth)
cargo run -p sim-viewer -- --config path/to/file.toml
cargo run -p sim-headless -- --ticks 600 --seed 42 [--agents 20] [--batch-size N]
cargo run --release -p sim-headless -- --canonical --agents 20 --batch-size 10000
```

`--canonical` runs the Phase 2 survival scenario: seed 1, a 2,048² world, 600,000 ticks, and
fixed water/wood spawn cohorts with starting supplies. It prints a versioned report and a hash.

### Configuration (`config/simulation.toml`)

| Key | Meaning |
|---|---|
| `simulation.seed` | World + simulation seed |
| `simulation.ticks_per_second` | Fixed tick rate (60) |
| `world.initial_width/height` | Bootstrap area loaded at start (4,096²). Not the world size (65,536²). |
| `world_cache.enabled/path` | Optional full-world archive (see below) |

### Full-world archive (optional, ~16 GiB, ~2 min)

```sh
cargo run --release -p sim-viewer -- --pregenerate-world
```

The archive lets the viewer show the whole world instantly and stream exact chunks while you
zoom. If it is missing, stale, or the seed doesn't match, the viewer warns and generates
procedurally instead. Re-run the command after changing the seed or the generator.

## Behavior study

Measures how viewer-like agents actually fare. They spawn without supplies and run headless:

```sh
cargo run --release -p sim-headless -- --study [--near-water | --groups] [--seed N] [--agents N]
    [--ticks N] [--mind legacy|memory|sharing|full] [--verbose] [--trace AGENT]
```

- `--near-water` spawns within 6 cells of fresh water. `--groups` drops agents in groups of 5.
  The default spawns on random land.
- `--mind legacy` is the old reactive policy (what the viewer used before). `memory` adds the
  mental map. `sharing` adds gestures. `full` (the default) adds personalities and relationships.
- The `social:` line shows the % of time agents spend near another agent, their acquaintances,
  mean trust, and "I've been there" gestures. The `trait` lines split agents at each trait's
  midpoint and compare the behavior that trait should change. That's how you check that different
  personalities really behave differently.
- The output starts with a world summary: fresh water, food, how much land is near water, and the
  biome mix. Then come survivors, death causes, roaming, idle %, meals, and gestures.
- `--verbose` prints one line per agent, including what it remembers and its last need values.
  `--trace N` prints agent N's last 60 decisions (with failures) — the fastest way to see why an
  agent died.

Compare minds before and after any behavior change. Current numbers are in [STATUS.md](STATUS.md).

## Viewer controls

| Input | Action |
|---|---|
| Mouse wheel | Zoom around cursor (max zoom-out fits the whole world) |
| Left-drag | Pan |
| Right-drag | Select an area to generate/load (yellow = OK, red = over limit of 65,536 new chunks) |
| `T` | Spawn an agent at the cursor (needs loaded, standable terrain; max 4,096). Agents use the full mind |
| Numpad `5` | Open the object menu / confirm. `2`/`8`/`4`/`6` select tree, berries, rock, water; left-click places; `0` closes |
| `Space` | Pause / resume |
| `1`–`9` | Speed 1×, 2×, 4× … 256× |
| `R` | Reset to zero agents (keeps terrain) |
| `C` | Cancel pending generation |
| Hover | Cell/chunk info in HUD. Hovering an agent shows its needs, goal, inventory, health, sleep, memory, personality (with a one-word summary such as EXPLORER or LONER), and friends with trust. The map shows its remembered places (solid = seen, outline = hint with search area) and dotted lines to where its acquaintances were last seen (violet = friend) |
| `Esc` | Quit |

## Tests

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
scripts/validate.sh [--quick] [--gpu]       # all of the above + checksum + headless smoke
```

Where tests live:

- Unit tests live in a `tests.rs` or `tests/` folder next to the module they cover
  (e.g. `sim-core/src/engine/tests/`, `sim-world/src/worldgen/tests/`, `sim-viewer/src/render/tests/`).
- Public-API tests, one file per topic: `crates/sim-core/tests/` (`movement`, `routing_and_perception`,
  `needs`, `policy`, `resources`, `sleep`, `shelter`, `health`, `initial_supplies`, `spawned_objects`)
  and `crates/sim-world/tests/world_queries.rs`.
- `crates/sim-headless/tests/canonical_scenarios.rs`: canonical scenario determinism and soaks.
- `cargo test -p sim-world` runs only the world tests, which is handy when you're not touching the world.

Release harnesses and soaks are `#[ignore]`d. Run them explicitly:

```sh
cargo test --release -p sim-headless --test canonical_scenarios -- --ignored --nocapture --test-threads=1
```

Determinism fingerprints (last verified 2026-10-10, world generator v2). If one changes,
simulation behavior changed. Canonical runs use the legacy policy; the study covers the mind.

| Run | Hash |
|---|---|
| `sim-headless --ticks 600 --seed 42` (20 agents) | `561a5ca41553db95` |
| `sim-headless --canonical --agents 20` (600,000 ticks) | `358092051ec6b444` |
| `sim-headless --canonical --agents 100` (600,000 ticks) | `e4842c31783f1ea1` |

## Tooling

- **Map render** (BMP, no window):
  `cargo run --release -p sim-world --example render_map -- --width 4096 --height 4096 --step 8 --out map.bmp`
- **World-quality review set**: `cargo run --release -p sim-world --example render_map -- --review-set`
  writes 14 views × 4 seeds plus distribution/hash reports to `target/world-quality/`.
  Compare before and after any generator change.
- **Viewer metrics**: `SIM_VIEWER_SUMMARY_METRICS=1` logs summary-cache rebuild cost.
- **Drainage experiments**: `SIM_DRAINAGE_STEP`, `SIM_DRAINAGE_SEED`, and
  `SIM_RIVER_SOURCE_FLOW_THRESHOLD`. See [`archive/TESTING.md`](archive/TESTING.md).

## Deeper detail

`docs/archive/` holds the original long-form docs: per-slice plans, per-test coverage notes,
performance measurements, and all 58 architecture decisions. They are accurate as of 2026-07-17
but very dense. Use them as reference, and don't extend them.
