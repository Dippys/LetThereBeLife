# Plan: agent minds and sharing

_Started 2026-10-10. This replaces the archived slice-by-slice Phase 3 plan
([`archive/PHASE3_BELIEFS_RELATIONSHIPS_PLAN.md`](../archive/PHASE3_BELIEFS_RELATIONSHIPS_PLAN.md)),
which is still useful as a checklist of storage, determinism, and bounding requirements._

## Idea

Agents failed because they knew nothing beyond their 8-cell view, not because of any single rule.
So the work is built around **what each agent knows**: a private mental map of places, the areas it
has explored, and hints it got from others through observable gestures. Every step is measured with
the behavior study (`sim-headless --study`, see [DEVELOPMENT.md](../DEVELOPMENT.md#behavior-study)).

## Done (2026-10-10)

| Piece | Where | Effect measured |
|---|---|---|
| Behavior study tool | `sim-headless/src/study.rs` | Reproduced the camping: idle ~80%, 0 meals, everyone dead by ~250k ticks |
| Agent-scale water (waterholes + oasis vegetation) | `sim-world/src/worldgen/ponds.rs` | Seed 1: land within ~64 cells of fresh water 3% → 79%; food bushes 699 → 4,518 |
| Mental map: remembered places, forgetting by looking again | `sim-core/src/cognition/map.rs` | Thirst deaths near water 15 → 0 of 20 |
| Memory-driven decisions: waypoint travel, novelty exploration with a walk-back leash, top-ups, excursions | `sim-core/src/policy/deliberate.rs` | Survivors 154 (legacy) → 244 (memory) → 248 (with gestures) of 300 across 15 scenarios |
| Spiral search when no water is known | `cognition/map.rs` (`search_target`) | Fixed a regression on an island seed (2 → 15 survivors) |
| Home shelters and sleep/build spots that physics accepts | `deliberate.rs`, perception `reserved_cells` | Exhaustion deaths over 2.4M ticks 3–6 → 0 per scenario |
| Pointing gestures: watchers infer a rough place plus a search radius | `sim-core/src/cognition/gesture.rs` | Mechanically correct; small measured benefit (see below) |

## Open findings

- **Sharing rarely tells anyone something new.** Agents that meet usually explored the same area,
  so in group spawns only a few percent of gestures inform a watcher. Sharing pays off when agents
  separate, learn different places, and meet again, and nothing brings them back together yet.
- **Deserts and tiny islands stay deadly.** Agents spawned 100+ cells from any water often die
  before their first drink. That may be fine (deserts should be harsh), or terrain cues could help.
- **Food never regrows.** This hasn't mattered yet over 2.4M ticks with 20 agents. Re-measure with
  more agents or longer runs before adding regrowth.

## Next, in order

1. **Reasons to regroup.** Shared home shelters, or returning to where you last saw companions,
   so agents meet after exploring apart. Success: a higher share of gestures that inform someone,
   and a survival gain from `full` over `memory` in group spawns.
2. **"Nothing that way" gestures.** Share explored dead ends (marking tiles as explored for
   watchers), the other half of sharing exploration.
3. **Familiarity and trust.** Remember who pointed at what and whether it paid off; weigh hints by
   the source's track record. This starts the relationships part of Phase 3.
4. **Terrain cues.** Downhill and lush vegetation suggest water, which gives desert and island
   agents a chance.
5. **Regrowth** if longer or bigger studies show starvation.
6. **Toward language (Phase 4–5).** Gesture meanings learned and possibly misread instead of being
   understood perfectly.

## Constraints to keep

- Beliefs and truth stay separate; nothing copies one agent's memory into another.
- Fixed-size per-agent state (`MentalMap` is 256 B). At the spec's 10M agents that's 2.5 GB, so
  later compaction (sparse or tiered memory) will be needed; measure before changing.
- Deterministic: no hash-map iteration, no floats, no wall clock. Tests replay memory-driven runs
  exactly.
