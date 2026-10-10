# Let There Be Life

A deterministic Rust simulation where society is meant to emerge from individual people rather
than being scripted. Agents should eventually communicate only through observable signals and
develop their own languages. Today it has a 65,536² procedurally generated world (plates,
climate, rivers, waterholes, biomes) and agents that seek water and food, sleep, build shelters,
and can die. Each agent remembers places it has seen, explores, and points out places to others,
who infer only a rough idea of where they are.

**Where it's heading:** agents who communicate only through observable signals, each with a
personal lexicon, so that they can misunderstand each other believably and learn from it. See
[docs/plans/VERTICAL_SLICE.md](docs/plans/VERTICAL_SLICE.md).

## Quick start

```sh
cargo run --release -p sim-viewer          # open the world; press T over land to spawn agents
cargo run -p sim-headless -- --ticks 600   # headless run, prints a deterministic report hash
cargo run --release -p sim-headless -- --study   # how well agents survive (see docs/DEVELOPMENT.md)
cargo test --workspace
```

Viewer basics: press `H` for help, scroll to zoom, drag to pan, click a person to see what they
think, `Space` to pause, `1`–`9` to set speed. See the full controls in
[docs/DEVELOPMENT.md](docs/DEVELOPMENT.md#viewer-controls).

## Layout

| Path | |
|---|---|
| `crates/sim-core` | The simulation (headless, deterministic) |
| `crates/sim-world` | World storage and procedural generation |
| `crates/sim-viewer` | `winit` + `wgpu` viewer |
| `crates/sim-headless` | CLI runner and canonical scenarios |
| `crates/sim-config` | Shared TOML config loading |
| `config/simulation.toml` | Seed, tick rate, bootstrap area, world archive |
| `docs/` | [Status](docs/STATUS.md), [architecture](docs/ARCHITECTURE.md), [development](docs/DEVELOPMENT.md), plans |
| `InitialDocumentation/` | Original design spec (read-only) |

AI agents: see [AGENTS.md](AGENTS.md).
