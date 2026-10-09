# Project Status

_Last updated: 2026-10-10._

## North star

> An agent misunderstands a signal for a believable reason, acts on that misunderstanding, and
> both participants update future behavior using only observable evidence.

That's the spec's definition of success, and the active plan works toward it in a small valley:
[`plans/VERTICAL_SLICE.md`](plans/VERTICAL_SLICE.md).

## Where the project is

The design spec ([`InitialDocumentation/16_IMPLEMENTATION_ROADMAP.md`](../InitialDocumentation/16_IMPLEMENTATION_ROADMAP.md))
lays out phases 0–10, from world → physical agents → cognition → communication → language → society.

| Phase | State |
|---|---|
| 0. Technical spikes: workspace, headless core, viewer, quality gate | Done |
| 1. World foundation: terrain, climate, rivers/lakes, waterholes, biomes, resources | Done |
| 2. Physical agents: movement, perception, needs, gathering, sleep, shelter, health, death | Done |
| 3. Beliefs, memory, relationships | **In progress.** Mental maps, personalities, and sparse relationships with trust are done; episodic memory isn't. Plan: [`plans/MINDS.md`](plans/MINDS.md) |
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
  shelter. It points out places, or ground it has already explored, to agents nearby, who get only
  a rough hint to search.
- **People are different (viewer and study):** four personality traits (curiosity, caution,
  sociability, diligence) and up to six acquaintances with familiarity, trust, and last-seen
  place. Sociable agents seek out friends and stay with company. Hints count for more from people
  whose past hints were right.
- **Viewer:** pan and zoom, HUD, `T` to spawn, the object-placement menu, speed 1×–256×. Hovering
  an agent shows its memory and draws its remembered places on the map.
- **Headless:** canonical scenarios with fingerprint hashes, and the **behavior study**
  (`--study`), which measures survival, roaming, idleness, and gestures, with per-agent traces.

## Did agents stop camping and dying? (behavior study, 2026-10-10)

20 agents, 600k ticks (~2.8 simulated hours), no supplies. Survivors per scenario. Mind columns:
legacy = old reactive policy, memory = mental map, sharing = plus gestures, full = plus
personality and relationships (what the viewer runs).

| Seed (terrain) | Spawn | legacy | memory | sharing | full |
|---|---|---|---|---|---|
| 1 (savanna/desert) | random land | 9 | 20 | 20 | 20 |
| 1 | groups of 5 | 11 | 18 | 20 | 20 |
| 1 | near water | 16 | 19 | 19 | 19 |
| 4 | random land | 11 | 20 | 19 | 20 |
| 4 | groups | 14 | 20 | 20 | 20 |
| 4 | near water | 13 | 20 | 20 | 20 |
| 7 (desert/alpine) | random land | 1 | 2 | 2 | 4 |
| 7 | groups | 1 | 1 | 3 | 2 |
| 7 | near water | 7 | 14 | 16 | 12 |
| 9 (small island) | random land | 5 | 12 | 13 | 13 |
| 9 | groups | 5 | 16 | 17 | 19 |
| 9 | near water | 11 | 20 | 20 | 20 |
| 42 (forest) | random land | 18 | 20 | 20 | 20 |
| 42 | groups | 16 | 20 | 20 | 20 |
| 42 | near water | 16 | 20 | 20 | 20 |
| **Total of 300** | | **154** | **242** | **249** | **249** |

The social layer doesn't add survival, since most scenarios were at their ceiling. It makes agents
different from each other and makes sharing work: in group spawns, time in company and the share of
gestures that tell someone something new rose severalfold on seeds 1, 9, and 42 (D-069). The seed 7
near-water drop (16 → 12) is luck, not personality: the dead span every trait profile, and all
starved without ever finding food in a desert with ~190 bushes.

Do personalities behave differently? 100 agents in groups, seeds 1, 4, and 42 (agents below vs at
or above each trait's midpoint):

| Trait | Behavior | Low | High |
|---|---|---|---|
| Curiosity | ground explored (tiles) | 172–227 | 218–317 |
| Caution | peak thirst | 3,978–4,786 | 3,449–4,370 |
| Sociability | % time in company | 12–18% | 28–47% |
| Diligence | % time idle | 64–70% | 48–66% |

Over 2.4M ticks (~11 hours), seeds 1, 4, and 42 keep all 20 agents alive with the full mind, and
idle time is around 55% instead of 80–90%. The "legacy" column already includes the waterholes.
Before them, legacy agents near seed 1's only lake all died by ~250k ticks.

## Known problems and limitations

- **Hints never drive decisions (found by the M1 log).** Agents always prefer places they saw
  themselves, so in practice hearsay changes beliefs but never actions. Many hints are also stale
  (the place pointed at has since been eaten). M4 addresses this.
- **Sharing improves survival only where knowledge is scarce.** In most scenarios agents already
  survive on their own knowledge. Sharing should matter more with bigger populations, scarcer
  resources, or children who start out knowing nothing.
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

- 2026-10-10 (latest): M2, private intent separated from the public signal (D-072).
- 2026-10-10: M1, the valley scenario, communication log, and explain output (D-071).
- 2026-10-10 (later): personalities (D-067), relationships and trust (D-068), visiting friends
  and "I've been there" gestures (D-069).
- 2026-10-10: behavior study (D-061), waterholes and generator v2 (D-062), mental maps and the
  memory-driven policy (D-063), gestures (D-064), perception `reserved_cells` (D-065), and stale
  events pruned every tick (D-066).
- 2026-10-09: workflow and docs cleanup (D-059), and the `sim-world` crate with the module split (D-060).

## What's next

Follow [`plans/VERTICAL_SLICE.md`](plans/VERTICAL_SLICE.md). **M1 (valley and communication log) and
M2 (private intent vs public signals) are done.** Next is **M3: concepts and personal lexicons**, then personal lexicons, interpretation with
competing meanings, learning and repair, and finally the automated success test. Run the study
before and after each milestone.
