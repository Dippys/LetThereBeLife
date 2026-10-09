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
| `engine/` | `Engine` facade. `mod.rs` (struct, config, commands, snapshot), `tick.rs` (event dispatch), `setup.rs` (world loads, population, spawn), `views.rs` (read-only accessors), `routes.rs`, `policy.rs` (activation with `PolicyOptions`, decision handling, drink/eat/gather effects), `cognition.rs` (observe → deliberate, gesture start/completion and who sees it, `mental_map`/`signal_events` views), `actions.rs` (sleep requests, action and wake completion), `shelter.rs`, `errors.rs` (error mapping) |
| `agent/` | `AgentId(u32)`, `SimTime`, 6-byte `AgentRecord`, `AgentView`, errors, perception types, `ScheduledEvent` (32 B). `population/` splits the dense `Population` store: `init`, `movement`, `vitals` (needs + health), `policy`, `sleep`, `inventory`, `perception` |
| `policy/` | Decision-making. `selection.rs` is the original reactive policy (what's in view only). `deliberate.rs` is the memory-driven policy: travel to remembered places by waypoints, spiral search when no water is known, novelty exploration with a walk-back leash, top-ups before trips, home shelters, pointing out places. `exploration.rs`, `state.rs` (`PolicyState`, 12 B) |
| `cognition/` | Private beliefs and identity. `map.rs`: per-agent `MentalMap` (256 B): 12 remembered places in per-kind slots (12 B each, first-hand or a hint with a search radius and its teller), 24 recently explored 32×32 tiles, spiral-search and sharing state. `social.rs`: `SocialMemory` (96 B), 6 acquaintances with familiarity, trust, and last-seen place. `personality.rs`: `Personality` (curiosity, caution, sociability, diligence), a pure function of seed and agent id. `gesture.rs`: pointing gestures (direction plus order-of-magnitude distance) and how watchers infer a rough place. `Minds` stores a `Mind { map, social }` per `AgentId` |
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
| `worldgen/` | Private integer-only generator (see below): `plates`, `climate`, `noise`, `drainage/` (`lattice`, `channels`), `hydrology`, `regions`, `ponds` (agent-scale waterholes), `classification`, chunk synthesis in `mod.rs` |

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
- **Reset** clears dynamic state (agents, events, time, spawned objects, minds) but keeps loaded terrain.

## Agent minds (beliefs vs truth)

```text
perceive (radius 8, truth) ──► MentalMap::observe ──► deliberate ──► goal + target
                                   ▲                       │
       watcher infers rough place  │                       └─► Signal action (120 ticks)
       (gesture::interpret) ◄──────┴──── awake agents in view see the gesture
```

- `PolicyOptions { exploration, memory, sharing, social }` picks the policy at activation.
  `social` adds personalities (otherwise everyone is `Personality::AVERAGE`), relationships,
  visiting friends, trust-weighted hints, and "I've been there" gestures. The legacy
  (`activate_physical_policy[_with_exploration]`) path is unchanged, and canonical scenarios use it.
  The viewer and the study use `PolicyOptions::full()`.
- Beliefs never touch truth. A remembered place is checked only by looking again: if it's in view
  and nothing of that kind is there, the memory is dropped (or a hint loses confidence).
- Sharing respects the vision's core rule: the sender's exact memory is never copied. Watchers get
  a direction and an order of magnitude and store a hint with a search radius, which is provably
  large enough to contain the real place (`cognition/gesture.rs` tests).
- **Personality** tunes thresholds through `policy/deliberate.rs::Temperament`. Caution sets
  top-up points, food reserve, and roaming leash. Curiosity sets exploration targets and
  excursions. Sociability sets gesture cooldown, visiting friends, and staying with company.
  Diligence sets how often a calm agent works instead of lounging. Average traits reproduce the
  original thresholds.
- **Relationships** form by seeing each other (familiarity) and by hints that turn out right or
  wrong (trust, adjusted via the hint's teller slot). A watcher's hint confidence is
  `48 + trust × 3/4`. Gestures have a topic: `Place(kind)`, or `Explored` ("I've been there"),
  which marks that tile explored for watchers so groups spread out.
- Perception includes `reserved_cells` (trees and rocks, including depleted ones) because nobody
  can sleep or build on them. Both policies use it to pick build sites, and the memory policy uses
  it to pick sleep spots.

## World generation (three tiers, all integer, all seed-pure)

1. **Global analytic fields** (`plates`, `climate`, `noise`): plates, continents, mountains,
   latitude temperature, wind bands. These are pure functions of `(seed, x, y)`.
2. **Whole-world drainage skeleton** (`drainage`): built once per seed on a 257×257 lattice.
   It fills basins, places lakes and outlets, and routes rivers to the ocean or world edge. Cached (LRU of 4 seeds).
3. **Regional refinement** (`hydrology`, `RegionMap`): 4,096-cell regions on a 129×129 lattice,
   with exact seams. Cached (LRU of 64).
4. **Chunk synthesis** (`worldgen/mod.rs`, `classification.rs`): interpolates the region, adds
   detail, rasterizes rivers and lakes, classifies biomes, and places sparse features.
5. **Waterholes** (`ponds.rs`, generator version 2): at most one small pond (radius 2–6) per chunk,
   placed well inside the chunk so it never crosses an edge. The chance follows local moisture:
   about 3% of chunks in desert, ~27% in typical savanna, ~55% in grassland, up to 80% in wet
   forest. Pond shores in dry or open country grow trees and berry bushes (oases). This exists
   because continental drainage alone left most land thousands of cells from fresh water, far
   beyond an agent's 8-cell view.

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

`sim-headless` is split into `scenario.rs` (runner), `spawns.rs`, `report.rs`, `invariants.rs`, `hash.rs`,
and `study.rs` (behavior study: viewer-like agents, survival and roaming metrics, decision traces).

The viewer's hover card shows the hovered agent's memory (counts by kind, hints, explored tiles),
and the map draws its remembered places: solid squares for seen places, outlines sized to the
search radius for hints (`render/instances.rs`).

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
