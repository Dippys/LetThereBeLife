# Scaling to Ten Million Persistent NPCs

## Feasibility statement

Ten million persistent, individually represented NPCs are plausible only with:

- Event-driven cognition.
- Compact state.
- sparse relationships, beliefs, memories, and language.
- region partitioning.
- analytical need and movement updates.
- limited active rendering.
- bounded interpretation and planning.
- careful memory allocation.
- extensive profiling.

Ten million fully active, high-frequency thinkers are not a realistic desktop target.

## Individual simulation versus statistical aggregation

The project can avoid population-level replacement while still using computational selectivity.

An inactive NPC retains:

- Identity.
- exact state at a reference time.
- deterministic rates.
- future event.
- personal memories and relationships.
- journey or location.
- individual language.
- obligations.

When an event occurs, that exact NPC makes a decision.

This is not the same as representing a town as "2,000 farmers."

## Event-volume examples

For ten million NPCs:

```text
One thought per hour:
  ~2,778 thoughts per simulated second

One thought per minute:
  ~166,667 thoughts per simulated second

One thought every 10 seconds:
  1,000,000 thoughts per simulated second

One thought per second:
  10,000,000 thoughts per simulated second
```

Actual loads depend on simulated-time speed and real-time target.

## Region partitioning

Regions should be mostly independent.

Each region owns:

- Residents currently present.
- local objects.
- local events.
- settlement state.
- chunk cache.
- message inbox/outbox.

Cross-region effects:

- Agent transfer.
- caravan.
- army.
- migration.
- news carrier.
- shipment.
- environmental front.

## Parallelism

Recommended:

- Fixed worker pool.
- region ownership.
- no arbitrary shared mutation.
- thread-local command buffers.
- deterministic merge.
- work stealing only if it preserves deterministic policy or is restricted to independent tasks.

## Cache behavior

The simulation should process batches of similar due events.

Examples:

- Need thresholds.
- journey arrivals.
- perception updates.
- language interpretations.
- work completions.

Batch processing improves cache locality and SIMD opportunities.

## Active-density challenge

The difficult case is not ten million sleeping people. It is a major event that activates millions:

- continent-wide panic.
- instant global broadcast.
- global weather tick.
- universal daily update.
- plague status check for everyone.
- global language-statistics pass.

Design systems to avoid simultaneous global activation.

Use propagation:

- News travels through communication or institutions.
- Weather affects regions.
- Disease schedules individual transitions.
- Daily work uses staggered shifts.
- Analytics can stream incrementally.

## Rendering

Only visible agents need presentation entities.

Possible numbers:

```text
10,000,000 persistent NPCs
10,000 in loaded region
1,000 in camera vicinity
200 with detailed animation
```

Rendering architecture must not dictate simulation storage.

## Development targets

Suggested stages:

1. 100,000 compact agent cores.
2. 1 million scheduled agents.
3. 10 million cores and timing events.
4. 100,000 lightweight decisions per real second.
5. Sparse relationship and memory pools.
6. Save/load at scale.
7. Language processing benchmark.
8. Local active-scene benchmark.
9. Multi-region parallel benchmark.

## Honest limitation

"Every NPC has its own thought" is feasible when thought means an individual event-driven decision.

It is not feasible if it means every person continuously runs perception, full belief search, full memory search, GOAP, and language inference at frame rate.
