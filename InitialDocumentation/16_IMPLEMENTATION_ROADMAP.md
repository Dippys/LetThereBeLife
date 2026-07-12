# Implementation Roadmap

## Phase 0: technical spikes

Before building the game:

- Benchmark ten million compact agent cores.
- Benchmark timing wheel insertion/rescheduling.
- Benchmark packed relationship and memory pools.
- Test snapshot size and load time.
- Validate deterministic parallel region processing.
- Measure 32-bit versus 64-bit ID impact.
- Prototype chunk generation.

Deliverable: a headless benchmark report.

## Phase 1: world foundation

Build:

- World coordinates.
- chunk and region hierarchy.
- deterministic generator.
- elevation, substrate, water.
- sparse trees, rocks, berries.
- local passability.
- chunk load/unload.
- modification persistence.

Do not build perfect worldgen.

## Phase 2: physical agent loop

Build:

- 20–100 agents.
- needs.
- movement.
- perception.
- gathering.
- eating and drinking.
- sleep.
- shelter.
- simple death.
- event scheduling.

Deliverable: agents survive or fail for understandable reasons.

## Phase 3: beliefs and relationships

Build:

- Perceived observations.
- beliefs with confidence.
- sparse relationships.
- trust and familiarity.
- important episodic memories.
- forgetting and consolidation.

## Phase 4: nonverbal communication

Build:

- Cry.
- pointing.
- gaze.
- emotional tone.
- alarm.
- attention.
- interpretation candidates.
- clarification.
- delayed success evaluation.

Deliverable: believable misunderstandings.

## Phase 5: proto-language

Build:

- Signal IDs.
- personal lexical hypotheses.
- small inherited proto-language.
- grounded learning.
- recognition/production split.
- simple two-signal constructions.

## Phase 6: children and transmission

Build:

- Families.
- babies.
- developmental stages.
- caregiver interaction.
- imitation.
- overgeneralization.
- generational learning.

Deliverable: children acquire overlapping but non-identical language.

## Phase 7: invention and diffusion

Build:

- Unknown concepts.
- descriptive sequences.
- signal invention.
- synonyms.
- prestige.
- adoption.
- community analytics.

Deliverable: competing terms emerge and one may spread.

## Phase 8: settlements and economy

Build:

- Households.
- occupation.
- storage.
- ownership.
- trade.
- debt.
- specialization.
- settlement records.

## Phase 9: migration and language divergence

Build:

- Multiple settlements.
- routes.
- migration.
- isolation.
- pronunciation drift.
- borrowing.
- bilingualism.
- partial mutual intelligibility.

## Phase 10: conflict and institutions

Build:

- Law.
- leaders.
- factions.
- crime.
- punishment.
- war.
- religion.
- writing.
- administration.

Only after prior systems are stable.

## Scaling gates

Do not increase population merely because the map supports it.

Suggested gates:

- 100 agents: cognition correctness.
- 10,000 agents: event architecture.
- 100,000 agents: storage and pathfinding.
- 1 million agents: region parallelism and persistence.
- 10 million agents: memory budget and event throughput.

## First vertical slice configuration

```text
World:
  64 × 64 or 128 × 128 chunks
  small loaded valley

Population:
  16 adults
  4 children

Resources:
  berries, water, wood, stone, fire

Concepts:
  FOOD, WATER, DANGER, COME, GIVE,
  ME, YOU, HOME, FIRE, CHILD

Signals:
  small proto-language
  pointing
  gaze
  emotional tone
  alarm

Needs:
  hunger, thirst, rest, safety, belonging
```

## Definition of success

An agent misunderstands a signal for a believable reason, acts on that misunderstanding, and both participants update future behavior using only observable evidence.
