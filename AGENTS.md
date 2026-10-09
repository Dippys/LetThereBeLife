# Let There Be Life — Agent Guide

A deterministic, headless-first Rust simulation where society should emerge from individual
agents. Long-term goal: agents with needs, beliefs, memory, relationships, and languages that
emerge from signals (never directly transmitted meaning). Today: a procedurally generated world
plus survival agents (needs, gathering, sleep, shelter, health, death) with private mental maps
(remembered places, explored areas) and pointing gestures that share rough knowledge.

**Start here:** read [`docs/STATUS.md`](docs/STATUS.md) for where things stand and what's next.

## Repository map

| Path | What it is |
|---|---|
| `crates/sim-core` | The simulation: `Engine`, agents, scheduler, policy. Re-exports `sim-world`. No window/GPU/OS deps. |
| `crates/sim-world` | Terrain types, chunk storage, world generation, world archive. Knows nothing about agents. |
| `crates/sim-config` | Loads `config/simulation.toml` for the binaries. |
| `crates/sim-headless` | CLI runner + canonical survival scenarios and reports. |
| `crates/sim-viewer` | `winit` + `wgpu` window: camera, HUD, spawning, background chunk loading. Read-only view of the sim. |
| `config/simulation.toml` | Seed, tick rate, bootstrap area, world-archive path. |
| `docs/` | Living docs: status, architecture, development guide, active plans. |
| `docs/archive/` | Historical, very detailed docs from the first build-out. Useful for deep dives; may be stale. |
| `InitialDocumentation/` | Original design spec (vision → 10M agents). **Read-only.** |
| `scripts/validate.{sh,ps1}` | Full quality gate. |

Docs: [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) (how the code fits together),
[`docs/DEVELOPMENT.md`](docs/DEVELOPMENT.md) (build, run, test, controls, tooling),
[`docs/DECISIONS.md`](docs/DECISIONS.md) (decision log).

## Commands

```sh
cargo test --workspace                                   # all tests
cargo clippy --workspace --all-targets -- -D warnings    # lint, warnings are errors
cargo fmt --all
cargo run -p sim-viewer                                  # interactive viewer
cargo run -p sim-headless -- --ticks 600 --seed 42       # headless smoke run
cargo run --release -p sim-headless -- --study           # behavior study: survival, roaming, deaths
scripts/validate.sh            # or: powershell -File scripts/validate.ps1
```

The toolchain is installed on Windows. From WSL, call `cargo.exe` (e.g.
`/mnt/c/Users/ahmed/.cargo/bin/cargo.exe`); `scripts/validate.sh` finds it automatically.

## Rules

1. **Never modify `InitialDocumentation/`.** It is checksummed (`scripts/initial-documentation.sha256`);
   the validate scripts fail if it changes. Put new knowledge in `docs/`.
2. **`sim-core` stays headless.** No windowing, rendering, UI, or OS-lifecycle dependencies.
   The viewer only reads engine state and mutates it through `EngineCommand` / explicit `Engine` methods.
3. **Determinism is the foundation.** Same seed + same inputs ⇒ identical results.
   - Simulation advances only by fixed ticks / scheduled events, never by frame time.
   - Iteration that affects outcomes uses ordered collections (`BTreeMap`, sorted `Vec`).
     `HashMap` is fine for keyed lookup only, never for iteration order.
   - Simulation state and rules use integer / fixed-point math. Floats only appear in
     presentation-facing values (speed multiplier, displayed seconds).
   - Thread count and completion order must not change results.
4. **Measure behavior changes.** Run the behavior study (`--study`, several seeds and spawn modes,
   `--mind legacy|memory|full`) before and after changing agent decisions or world generation, and
   record the numbers. Use `--trace AGENT` to see why an agent died before guessing.
5. **Beliefs are not truth.** Agent knowledge lives in `sim-core/src/cognition/` and changes only
   through perception or observed gestures. Never let policy read world state directly for things
   the agent hasn't perceived.
6. **Compact data, proven.** Hot per-agent records are small and pointer-free; add or keep
   `size_of` assertions when touching them. Prefer scheduled events over per-tick scans of all agents.
7. **Tests for new behavior and regressions.** Public-API scenario tests live in `crates/*/tests/`.
8. **Keep it simple.** No speculative abstractions or unrelated cleanup. Remove dead code in the area you touch.
9. **Don't weaken a failing check to make it pass.**

## Definition of done

- `cargo fmt`, `cargo test --workspace`, and `cargo clippy ... -D warnings` all pass
  (or run `scripts/validate.sh`).
- `docs/STATUS.md` is updated if what works, what's broken, or what's next changed.
- `docs/ARCHITECTURE.md` is updated if a crate/module boundary, key invariant, or dependency changed.
- Durable technical choices get a short entry in `docs/DECISIONS.md`.
- Keep docs short: describe the current state in a few lines; don't append run logs or long histories.
