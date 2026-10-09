# Let There Be Life

A deterministic Rust simulation where society is meant to emerge from individual people rather
than being scripted. Agents should eventually communicate only through observable signals and
develop their own languages. Today it has a 65,536² procedurally generated world (plates,
climate, rivers, biomes) and physical agents that seek water and food, sleep, build shelters,
and can die.

## Quick start

```sh
cargo run --release -p sim-viewer          # open the world; press T over land to spawn agents
cargo run -p sim-headless -- --ticks 600   # headless run, prints a deterministic report hash
cargo test --workspace
```

Viewer basics: scroll to zoom, left-drag to pan, `T` to spawn an agent, `Space` to pause,
`1`–`9` to set speed, `R` to reset. See the full controls in
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
