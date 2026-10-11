# Project Status

_Last updated: 2026-10-10._

## North star

> An agent misunderstands a signal for a believable reason, acts on that misunderstanding, and
> both participants update future behavior using only observable evidence.

That's the spec's definition of success. The vertical slice that first met it is done
([`plans/VERTICAL_SLICE.md`](plans/VERTICAL_SLICE.md)); the active plan is
[`plans/GENERATIONS.md`](plans/GENERATIONS.md): age, sex, relationships, births, and names, so the
band renews itself and language can pass between generations. Before it,
[`plans/LIVING_WORLD.md`](plans/LIVING_WORLD.md) added things described by properties, learned
knowledge, wildlife, and hearths.

## Where the project is

The design spec ([`InitialDocumentation/16_IMPLEMENTATION_ROADMAP.md`](../InitialDocumentation/16_IMPLEMENTATION_ROADMAP.md))
lays out phases 0–10, from world → physical agents → cognition → communication → language → society.

| Phase | State |
|---|---|
| 0. Technical spikes: workspace, headless core, viewer, quality gate | Done |
| 1. World foundation: terrain, climate, rivers/lakes, waterholes, biomes, resources | Done |
| 2. Physical agents: movement, perception, needs, gathering, sleep, shelter, health, death | Done |
| 3. Beliefs, memory, relationships | **In progress.** Mental maps, personalities, and relationships (trust, kin, partners, grudges, favours, grief) are done; episodic memory isn't |
| 4. Nonverbal communication | **Mostly done.** Pointing, mimes, tone, questions, repairs, corrections, requests for food, and shouted warnings and calls to hunt |
| 5. Proto-language | **Started (vertical slice).** Personal lexicons, two founding dialects, competing interpretations, learning from consequences |
| 6. Children and transmission | **Underway.** Couples, births, aging, and death of old age make a band that renews itself over generations; children start with no words or knowledge and learn by watching; names are given and spread. Plan: [`plans/GENERATIONS.md`](plans/GENERATIONS.md) |
| 7. Invention and diffusion | **Started.** Food lore, animal lore, and the hearth spread between families by observation |
| 8–10. Settlements and economy, migration and language divergence, conflict and institutions | Not started |

## What works today

- **World:** a finite 65,536² world generated from a seed: plates, continents, mountains, latitude
  climate, winds, whole-world drainage with lakes and rivers, biomes, waterholes, trees, rocks, and
  berry bushes, 30% of them bitter (generator v3). Things yield **materials with properties**
  (nutrition, toxicity, building use, regrowth); picked bushes and trees grow back.
- **Wildlife (valley):** deer and wolves as species traits with one rule set. Deer graze, herd, and
  flee people; wolves hunt deer, avoid crowds, and bite lone people; populations breed and wander
  back in. People hunt (better together), butcher carcasses for meat, and can be wounded; sleep heals.
- **Agents:** hunger, thirst, rest, and exposure needs; routing; gathering, eating, and drinking;
  sleep; lean-to shelters; health, incapacitation, and death. Fully deterministic.
- **Agent minds (viewer and study):** each agent remembers up to 16 places it has seen (water,
  berries, bitter berries, wood, stone, shelters, hearths) and forgets ones it finds empty. It explores new ground and
  spiral-searches when it knows no water. It walks back to remembered places, tops up before
  heading out, never strays farther from known water than it can walk back, and keeps a home
  shelter. It points out places, or ground it has already explored, to agents nearby, who get only
  a rough hint to search.
- **Learned knowledge:** what each material is good for, which animals are prey or dangerous, and
  whether a hearth warms you are beliefs, changed only by evidence (eating, retching, bites,
  watching others, family lore). The two founding families start with different lore.
- **Seasons:** four seasons of an hour each. Winter is cold and bare (fruit doesn't regrow,
  animals don't breed), summer is mild; the viewer shows the season and tints the land.
- **Complete misunderstanding episodes are common** (D-097): listeners report back holding up
  what they found, so speakers see when their word was taken otherwise (79 episodes in 24 of the
  success test's 40 valleys).
- **Things to talk about come from the world's tables:** every material, species, and kind of
  structure is something people can point at, remember, and name, with gestures from properties.
- **Fires need fuel:** a hearth burns out after an hour unless someone feeds it wood.
- **Stone blades:** one family knaps them, others learn by watching; an edge doubles chopping
  and carving and helps hunting, and wears out.
- **Hearths:** agents who know fire build one near home from stones and wood and warm up by it;
  others learn fire by watching.
- **Communication (vertical slice, `--valley`):** private intent is separate from the public
  signal (pointing, mime, a word from the speaker's own lexicon, tone). Listeners weigh competing
  readings, can misunderstand for recorded reasons, ask "this?", get repaired, learn from what
  they find, and correct the speaker later. Hungry agents ask others for food, who give or refuse.
  Children start with no words. Every exchange is in the communication log (`--comms`,
  `--misreads`, `--successes`, `--explain`).
- **Generations (viewer and study):** everyone has a sex, an age (a year is the four-hour turn of the seasons),
  and a name. Couples form from closeness (not between people raised together, unless long alone)
  and fade when apart; well-fed couples have babies, who are carried and nursed until 3 and then
  walk, knowing their family, with personalities blended from both parents. Elders grow frail and
  die of old age; a dwindling band is joined by newcomers with their own words. People know their
  kin, hold grudges, owe favours, and mourn those close to them. Names spread when called out and
  can be misheard or drift into nicknames. People without a word for something sometimes coin one,
  and children sometimes pick words up with a vowel changed, so vocabularies drift.
- **People are different (viewer and study):** four personality traits (curiosity, caution,
  sociability, diligence) and up to six acquaintances with familiarity, trust, and last-seen
  place. Sociable agents seek out friends and stay with company. Hints count for more from people
  whose past hints were right.
- **Viewer:** a top bar (play, speed, time, head counts), hover tooltips, a person panel opened by
  clicking (doing and why, need bars, beliefs, words, friends), a recent-events feed
  (misunderstandings, bites, kills, gifts, deaths), speech bubbles, a build palette (`B`), help
  with a legend (`H`), and an F3 technical readout. Animals, carcasses, and hearths are drawn.
- **Headless:** canonical scenarios with fingerprint hashes, and the **behavior study**
  (`--study`), which measures survival, roaming, idleness, and gestures, with per-agent traces.

## Survival (behavior study, refreshed 2026-10-10)

20 agents, 600k ticks (~2.8 simulated hours), no supplies. Survivors per scenario. Mind columns:
legacy = old reactive policy, memory = mental map, sharing = plus gestures, full = plus
personality, relationships, and learned knowledge (what the viewer runs).

| Seed (terrain) | Spawn | legacy | memory | sharing | full |
|---|---|---|---|---|---|
| 1 (savanna/desert) | random land | 9 | 20 | 19 | 20 |
| 1 | groups of 5 | 10 | 20 | 20 | 20 |
| 1 | near water | 10 | 19 | 19 | 19 |
| 4 | random land | 14 | 20 | 20 | 20 |
| 4 | groups | 10 | 20 | 20 | 20 |
| 4 | near water | 13 | 20 | 20 | 20 |
| 7 (desert/alpine) | random land | 1 | 2 | 2 | 4 |
| 7 | groups | 0 | 1 | 3 | 0 |
| 7 | near water | 5 | 14 | 15 | 17 |
| 9 (small island) | random land | 5 | 16 | 17 | 16 |
| 9 | groups | 6 | 18 | 18 | 20 |
| 9 | near water | 4 | 18 | 19 | 19 |
| 42 (forest) | random land | 16 | 19 | 20 | 20 |
| 42 | groups | 14 | 20 | 20 | 20 |
| 42 | near water | 15 | 20 | 20 | 20 |
| **Total of 300** | | **132** | **247** | **252** | **255** |

Legacy fell from 154 when a third of the bushes became bitter (less food for agents that don't
remember places). In the valley (20 people with wildlife, 1.2M ticks, the 66 livable valleys among
seeds 1-120), 1,238 of 1,320 are alive and on their feet at the end. Earlier figures (such as
"119 of 120") counted people lying collapsed after wolf bites as survivors: before wounded people
could come round (D-082), 168 of 1,303 "survivors" were collapsed for good.

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

- **Winter cold is the main killer** (D-102): fires mostly go out and people freeze near huts.
- **Huts never decay**, so a burst of building in the first year stays forever.
- **Stored food is rarely eaten:** people put food away in huts but seldom come back for it.
- **Decisions are hand-written.** Agents learn facts (food, danger, words, crafts), but what to do
  about them is a script. Next: learning what works (docs/plans/LEARNING.md).
- **Wolves are hard on loners.** Agents who wander off alone get bitten; wounded people now come
  round after 5 minutes, but some die of thirst while down or soon after.
- **Helping matters only in famines.** With regrowth and game, nobody needs to ask for food; the M7
  helping test runs famine valleys (`--no-regrowth --no-wildlife --food 5`).
- **COME (regroup) requests aren't implemented** (deferred from M7).
- **Sharing improves survival only where knowledge is scarce.** In most scenarios agents already
  survive on their own knowledge. Sharing should matter more with bigger populations, scarcer
  resources, or children who start out knowing nothing.
- **Deserts and tiny islands are deadly** for agents spawned 100+ cells from water. That's
  arguably correct, but terrain cues (downhill, vegetation) could help.
- **The canonical 100-agent run is ~35% slower** than before (≈4.2 s vs 3.1 s, ±15% noise).
  It comes mostly from more activity (18% more events, much more gathering near pond shores).
  No single hotspot was found.
- **Intermittent viewer smoke crash.** `0xc0000409` with no panic message (WSL shows it as exit
  code 9): 1 in 19 runs on 2026-10-09, 2 in 14 on 2026-10-10, then 0 in 8, then 2 in 8 `--valley`
  smokes after M7, then 1 in 12 smokes after the living world. It happens with and without agents. Cause unknown, possibly GPU teardown.
- **No save/load.** Simulation state can't be persisted. The world archive needs regenerating for
  generator v2 (`--pregenerate-world`).
- **Minds are 912 B per agent** (mental map 372 B). That's fine at viewer scale; the spec's 10M
  agents would need ~8 GB, so compaction comes later.
- **Greedy waypoints can pace around obstacles.** Agents no longer step straight back to where they
  were and collapse into sleep when exhausted, but longer cycles are possible.

## Recent changes

- 2026-10-11 (latest): a harder world: swimming, a 12-item load, hut storage, effort, learned
  huts, slower regrowth (D-102).
- 2026-10-11: viewer speeds up to 4096× (a frame budget shows the speed reached), the
  season's cold beside the year, and an info box (I) with fires, needs, births and deaths.
- 2026-10-11: warming up in a hut lasts until well warmed, ending the late-winter
  slowdown (D-101).
- 2026-10-11: no building that shuts anyone in; idle people show as resting (D-100).
- 2026-10-11: a year is one turn of the seasons (D-099); the viewer's top bar shows the year and season (raw clock in F3).
- 2026-10-11: only a look at the spot can refute a word (D-098).
- 2026-10-11: listeners report back, holding up what they found (D-097).
- 2026-10-10: two-valley start, `--apart` (D-096).
- 2026-10-10: fuel (D-094) and stone blades (D-095); misunderstanding episodes are
  rarer than the pinned seeds suggested (about 1 per 20-40 valleys).
- 2026-10-10: concepts from the world's tables (D-092) and seasons (D-093).
- 2026-10-10: new words and sound shifts (D-091).
- 2026-10-10: generations: herds that recover and wolves that hunt deer, age and sex,
  kin, grudges, favours, grief, couples, births, newcomers, and names (D-083-D-089).
- 2026-10-10: misunderstandings that run their course: worded hints are judged at the
  spot pointed at, idle agents check what was pointed out nearby, a corrected speaker trusts its
  word less, errands avoid wolves in view, wounded people come round, and the success test runs 40
  valleys instead of pinning one episode (D-082). Also a plain-language viewer (D-081).
- 2026-10-10: living world L1–L4: materials with properties,
  bitter berries, learned food beliefs, regrowth, deer and wolves, hunting and bites, warnings and
  calls to hunt, hearths, and fixes for agents pacing, chasing, or sleeping through bites
  (D-078–D-080).
- 2026-10-10: M7, children and requests for food (D-077). The vertical slice is complete except
  COME.
- 2026-10-10: M5, learning and repair from consequences, and M6, the success test
  (D-075, D-076).
- 2026-10-10: M4, competing interpretations, two founding dialects, and hints that drive
  decisions (D-074).
- 2026-10-10: M3, concepts, words, and personal lexicons with a noisy founding
  proto-language (D-073).
- 2026-10-10: M2, private intent separated from the public signal (D-072).
- 2026-10-10: M1, the valley scenario, communication log, and explain output (D-071).
- 2026-10-10 (later): personalities (D-067), relationships and trust (D-068), visiting friends
  and "I've been there" gestures (D-069).
- 2026-10-10: behavior study (D-061), waterholes and generator v2 (D-062), mental maps and the
  memory-driven policy (D-063), gestures (D-064), perception `reserved_cells` (D-065), and stale
  events pruned every tick (D-066).
- 2026-10-09: workflow and docs cleanup (D-059), and the `sim-world` crate with the module split (D-060).

## What's next

1. **Learning what works** (docs/plans/LEARNING.md): agents learn which actions relieve which needs
   in which situations, from their own experience and by copying people doing well, replacing most
   of the hand-written decision rules.
2. **Save and load**, so a long run (e.g. 1,000 years) is computed once and opened instantly.
3. **More speaker feedback**: speakers watching how listeners react, overhearing others.
4. **Separated groups (continued)**: a lasting barrier or much longer isolation.
5. **Episodic memory** (phase 3), so agents remember who misled, helped, or warned them.
