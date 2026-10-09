# Architecture

How the code fits together today. For exhaustive per-feature detail, see
[`archive/ARCHITECTURE.md`](archive/ARCHITECTURE.md) and the decision log
[`archive/ARCHITECTURE_DECISIONS.md`](archive/ARCHITECTURE_DECISIONS.md) (D-001 … D-058).

## Crates

```text
sim-world ◄── sim-core ◄── sim-headless
                 ▲   ▲
     sim-config ─┘   └── sim-viewer   (sim-config is also used by both binaries)
```

| Crate | Owns | Dependencies |
|---|---|---|
| `sim-world` | Terrain types, chunk storage (`World`), world generation, full-world archive format, `render_map` example | std, `rayon` (pure parallel derivation only) |
| `sim-core` | All dynamic simulation truth: `Engine`, time, agents, scheduler, policy. Re-exports the `sim-world` public API, so clients only use `sim_core::…` | `sim-world` |
| `sim-config` | Loading/validating `config/simulation.toml`; `build.rs` copies it next to built binaries | `sim-core`, `serde`, `toml` |
| `sim-headless` | CLI runner, `ScenarioRunner`, canonical survival scenarios, versioned reports + hashes | `sim-core`, `sim-config` |
| `sim-viewer` | Window, input, fixed-step driver, camera, HUD, spawning UI, background chunk loading, `wgpu` rendering | `sim-core`, `sim-config`, `winit`, `wgpu`, `pollster`, `bytemuck`, `rayon` |

**Hard boundaries:** `sim-world` knows nothing about agents. `sim-core` never depends on presentation.
Clients read state through views/snapshots and change it only through `Engine` methods / `EngineCommand`.

Each folder module has a `mod.rs` with the main type, sibling files for groups of `impl` blocks,
and a `tests.rs` or `tests/` folder next to it. Every file starts with a `//!` line describing it.

## `sim-core` module map

| Module | Responsibility |
|---|---|
| `lib.rs` | `mod` declarations and public re-exports only |
| `engine/` | `Engine` facade. `mod.rs` (struct, config, commands, snapshot), `tick.rs` (event dispatch), `setup.rs` (world loads, population, spawn), `views.rs` (read-only accessors), `routes.rs`, `policy.rs` (decision handling and drink/eat/gather effects), `actions.rs` (sleep requests, action and wake completion), `shelter.rs`, `errors.rs` (error mapping) |
| `agent/` | `AgentId(u32)`, `SimTime`, 6-byte `AgentRecord`, `AgentView`, errors, perception types, `ScheduledEvent` (32 B). `population/` splits the dense `Population` store: `init`, `movement`, `vitals` (needs + health), `policy`, `sleep`, `inventory`, `perception` |
| `policy/` | Physical decision-making: `selection.rs` (goal by urgency, target from perception), `exploration.rs`, `state.rs` (`PolicyState`, 12 B) |
| `scheduler` | Binary-heap event queue with a total order (see below), ≤4,096 due events per tick |
| `needs` | Analytical fixed-point hunger/thirst/rest/exposure (`NeedState`, 32 B). Values are computed from rates, not ticked. |
| `health` | Severe-need damage, incapacitation, death (`HealthState`, 16 B) |
| `routing` | Bounded A* with reusable scratch |
| `spatial` | Sparse chunk-bucketed agent positions; multiple agents may share a cell |
| `resources` | 3-byte inventories; sparse `BTreeMap` of depleted generated features |
| `sleep` | Sleep intervals, quality, wake/interrupt (`SleepState`, 24 B) |
| `structures` | Sparse one-cell lean-to shelters, construction lifecycle |
| `placements` | Sparse user-spawned trees/berries/rocks/water overlay |
| `diagnostics` | Copied work/capacity counters for reports |

## `sim-world` module map

| Module | Responsibility |
|---|---|
| `lib.rs` | World constants (envelope, chunk size, limits) and re-exports |
| `config.rs`, `geometry.rs`, `terrain.rs`, `features.rs`, `traversal.rs` | Value types: `WorldConfig`, `WorldPosition`/`WorldRect`, `TerrainCell`/surface/biome/climate, `Feature`/resources, walking rules and query errors |
| `chunk.rs`, `loads.rs`, `validation.rs` | `ChunkCoord`, chunk inspection and payloads; worker load requests and resident chunk forms; request validation and region-major spans |
| `storage/` | `World`: chunk-keyed resident storage (`BTreeMap` of 64×64 tiles), `loading.rs`, `queries.rs` (resident physical lookups), `visits.rs` (deterministic read-only iteration) |
| `generator.rs` | `ChunkGenerator` (read-only sampler for tooling) and full-chunk synthesis |
| `archive/` | Full-world archive: `format`, `writer`, `reader`, `overview` |
| `worldgen/` | Private integer-only generator (see below): `plates`, `climate`, `noise`, `drainage/` (`lattice`, `channels`), `hydrology`, `regions`, `classification`, chunk synthesis in `mod.rs` |

## Engine lifecycle

```text
Engine::new(config)                         // empty world + empty population, O(1)
  ├─ materialize_initial_area()             // headless: generate bootstrap terrain eagerly
  │  or apply_world_chunk_loads(...)        // viewer: insert chunks built on worker threads
  ├─ initialize_population(...) / spawn_agent(pos)
  ├─ activate_physical_policy[_with_exploration]()   // agents only decide after this
  └─ loop { tick() }                        // +1 SimTime, drain due events
```

- **Time:** `tick()` advances integer `SimTime` by one (unless paused) and processes due events.
  An idle tick only peeks at the heap root; nothing scans the whole population.
- **Event order** (the determinism contract): `(due time, event class, AgentId, event detail, sequence)`.
  Need thresholds come before health, wake, action completion, movement, then decisions.
  Cancellation is lazy: each agent carries a generation counter, so stale events are skipped.
- **Needs are analytical.** Each need stores a rate and a reference time. The scheduler gets one
  event at the next threshold crossing instead of updating every tick.
- **Reset** clears dynamic state (agents, events, time, spawned objects) but keeps loaded terrain.

## World generation (three tiers, all integer, all seed-pure)

1. **Global analytic fields** (`plates`, `climate`, `noise`): plates, continents, mountains,
   latitude temperature, wind bands. These are pure functions of `(seed, x, y)`.
2. **Whole-world drainage skeleton** (`drainage`): built once per seed on a 257×257 lattice.
   It fills basins, places lakes and outlets, and routes rivers to the ocean or world edge. Cached (LRU of 4 seeds).
3. **Regional refinement** (`hydrology`, `RegionMap`): 4,096-cell regions on a 129×129 lattice,
   with exact seams. Cached (LRU of 64).
4. **Chunk synthesis** (`worldgen/mod.rs`, `classification.rs`): interpolates the region, adds
   detail, rasterizes rivers and lakes, classifies biomes, and places sparse features.

Output: 4-byte `TerrainCell` (elevation, moisture, packed surface/biome) and sparse 24-byte `Feature`s.
Worker count and completion order never change the output; tests enforce this.

The world envelope is `[-32,768, 32,768)` on both axes. The config's `initial_width/height` sets
only the bootstrap load area, not the world size.

## Viewer

| Module | Responsibility |
|---|---|
| `main.rs`, `launch.rs` | Entry point, CLI flags (`--config`, `--smoke-frames`, `--pregenerate-world`), archive loading |
| `app/` | `ViewerApp`: `events.rs` (winit `ApplicationHandler`), `input.rs`, `simulation.rs` (fixed-step ticks, spawn, reset), `world_loading.rs` (generation and archive polling, dirty regions), `selection.rs`, `frame.rs` (window creation and redraw) |
| `render/` | `wgpu` `Renderer`: `gpu.rs` (uniforms, instance buffers), `instances.rs` (agents, shelters, objects, outlines), `summary.rs` (multi-level chunk summaries for zoomed-out views), `colors.rs`, `hud.rs` (HUD and agent text), `overlay.rs` (screen overlay, spawn menu, bitmap font), `shader.wgsl` |
| `generation/` | Background Rayon pool: `mod.rs` (`WorldGenerator`, jobs), `worker.rs`, `pager.rs` (center-out 32×32-chunk bootstrap pages) |
| `startup.rs` | Cursor-spawn validation (`T`), residency readiness check, 4,096-agent viewer limit, reset |
| `camera.rs` | Presentation-only camera, zoom/pan clamping |
| `spawn_menu.rs` | Numpad object-placement menu |

`sim-headless` is split into `scenario.rs` (runner), `spawns.rs`, `report.rs`, `invariants.rs`, and `hash.rs`.

Wall-clock time is converted into whole fixed ticks. Render frames never drive simulation.
Worker-built chunks are merged on the main thread after that frame's ticks.

## Key decisions worth knowing (from the decision log)

- **D-001/D-006:** Rust; headless core; compactness is measured with `size_of` asserts, not source brevity.
- **D-024:** a finite centered world envelope (not infinite).
- **D-029:** a whole-world drainage skeleton above regional detail, so rivers cross region seams.
- **D-037/D-039:** dense agents, totally ordered events, analytical needs with threshold wakeups.
- **D-045:** survival reports belong to `sim-headless`, not the engine.
- **D-055:** agents may overlap; natural features are walkable (water and shelters block).
- **D-058:** the full-world archive is presentation/cache data, never save-game state.
