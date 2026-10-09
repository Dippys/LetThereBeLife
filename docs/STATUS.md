# Project Status

_Last updated: 2026-10-10._

## Where the project is

The design spec ([`InitialDocumentation/16_IMPLEMENTATION_ROADMAP.md`](../InitialDocumentation/16_IMPLEMENTATION_ROADMAP.md))
lays out phases 0–10, from world → physical agents → cognition → communication → language → society.

| Phase | State |
|---|---|
| 0. Technical spikes: workspace, headless core, viewer, quality gate | Done |
| 1. World foundation: terrain, climate, rivers/lakes, waterholes, biomes, resources | Done |
| 2. Physical agents: movement, perception, needs, gathering, sleep, shelter, health, death | Done |
| 3. Beliefs, memory, relationships | **In progress.** Mental maps done; relationships next. Plan: [`plans/MINDS.md`](plans/MINDS.md) |
| 4. Nonverbal communication | **Started.** Pointing gestures share rough place knowledge |
| 5–7. Proto-language, children and transmission, invention and diffusion | Not started |
| 8–10. Settlements and economy, migration and language divergence, conflict and institutions | Not started |

## What works today

- **World:** a finite 65,536² world generated from a seed: plates, continents, mountains, latitude
  climate, winds, whole-world drainage with lakes and rivers, biomes, trees, berry bushes, and rocks.
  Generator v2 adds agent-scale **waterholes** with oasis vegetation, so most non-desert land has
  fresh water within ~64 cells.
- **Agents:** hunger, thirst, rest, and exposure needs; routing; gathering, eating, and drinking;
  sleep; lean-to shelters; health, incapacitation, and death. Fully deterministic.
- **Agent minds (viewer and study):** each agent remembers up to 12 places it has seen (water,
  food, wood, stone, shelters) and forgets ones it finds empty. It explores new ground and
  spiral-searches when it knows no water. It walks back to remembered places, tops up before
  heading out, never strays farther from known water than it can walk back, and keeps a home
  shelter. It points out places to agents nearby, who get only a rough hint to search.
- **Viewer:** pan and zoom, HUD, `T` to spawn, the object-placement menu, speed 1×–256×. Hovering
  an agent shows its memory and draws its remembered places on the map.
- **Headless:** canonical scenarios with fingerprint hashes, and the **behavior study**
  (`--study`), which measures survival, roaming, idleness, and gestures, with per-agent traces.

## Did agents stop camping and dying? (behavior study, 2026-10-10)

20 agents, 600k ticks (~2.8 simulated hours), no supplies. Survivors per scenario:

| Seed (terrain) | Spawn | legacy | memory | memory + gestures |
|---|---|---|---|---|
| 1 (savanna/desert) | random land | 9 | 20 | 20 |
| 1 | groups of 5 | 11 | 18 | 20 |
| 1 | near water | 16 | 19 | 19 |
| 4 | random land | 11 | 20 | 19 |
| 4 | groups | 14 | 20 | 20 |
| 4 | near water | 13 | 20 | 20 |
| 7 (desert/alpine) | random land | 1 | 2 | 2 |
| 7 | groups | 1 | 1 | 2 |
| 7 | near water | 7 | 14 | 16 |
| 9 (small island) | random land | 5 | 14 | 16 |
| 9 | groups | 5 | 16 | 16 |
| 9 | near water | 11 | 20 | 18 |
| 42 (forest) | random land | 18 | 20 | 20 |
| 42 | groups | 16 | 20 | 20 |
| 42 | near water | 16 | 20 | 20 |
| **Total of 300** | | **154** | **244** | **248** |

Over 2.4M ticks (~11 hours), seeds 1, 4, and 42 keep all 20 agents alive with the full mind, and
idle time is around 55% instead of 80–90%. The "legacy" column already includes the waterholes.
Before them, legacy agents near seed 1's only lake all died by ~250k ticks.

## Known problems and limitations

- **Gestures help less than hoped.** Agents who meet usually know the same places, so few gestures
  tell anyone something new. Next step: give agents reasons to regroup (see the plan).
- **Deserts and tiny islands are deadly** for agents spawned 100+ cells from water. That's
  arguably correct, but terrain cues (downhill, vegetation) could help.
- **The canonical 100-agent run is ~35% slower** than before (≈4.2 s vs 3.1 s, ±15% noise).
  It comes mostly from more activity (18% more events, much more gathering near pond shores).
  No single hotspot was found.
- **Intermittent viewer smoke crash.** `0xc0000409` with no panic message: 1 in 19 runs on
  2026-10-09, 2 in 14 on 2026-10-10, 0 in 8 since. The smoke spawns no agents. Cause unknown,
  possibly GPU teardown.
- **No save/load.** Simulation state can't be persisted. The world archive needs regenerating for
  generator v2 (`--pregenerate-world`).
- **Mental maps are 256 B per agent.** That's fine at viewer scale; the spec's 10M agents would
  need 2.5 GB, so compaction comes later.

## Recent changes

- 2026-10-10: behavior study (D-061), waterholes and generator v2 (D-062), mental maps and the
  memory-driven policy (D-063), gestures (D-064), perception `reserved_cells` (D-065), and stale
  events pruned every tick (D-066).
- 2026-10-09: workflow and docs cleanup (D-059), and the `sim-world` crate with the module split (D-060).

## What's next

Follow [`plans/MINDS.md`](plans/MINDS.md): reasons to regroup, then "nothing that way" gestures,
then trust in whoever pointed. Run the study before and after each step.
