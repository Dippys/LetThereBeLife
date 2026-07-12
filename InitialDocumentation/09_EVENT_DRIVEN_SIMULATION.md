# Event-Driven Simulation

## Why event-driven execution is mandatory

Ten million agents cannot each run a complete cognition cycle every frame.

Even:

```text
1 thought per NPC per second
= 10 million thoughts per simulated second
```

The simulation should execute work only when state changes or a decision is required.

This is exact individual simulation, not population aggregation.

## Analytical state evolution

Do not increment predictable values every tick.

Example hunger state:

```rust
struct NeedState {
    value_at_reference: f32,
    rate: f32,
    reference_time: SimTime,
}
```

Current value:

```text
value(now) =
    value_at_reference
  + rate × elapsed_time
```

Predict the threshold time and schedule it.

## Event types

Possible events:

- Need threshold.
- Wake.
- Sleep transition.
- Arrival.
- Work shift.
- Communication received.
- Danger perceived.
- Social commitment.
- Birth.
- Death.
- Healing.
- Disease progression.
- Resource completion.
- Construction completion.
- Weather impact.
- Plan reconsideration.
- Memory consolidation.
- Relationship review.
- Language-learning consequence.

## Timing wheel

A hierarchical timing wheel is likely more suitable than a single binary heap for millions of rescheduled events.

Possible tiers:

```text
Immediate:
  milliseconds and seconds

Near:
  minutes and hours

Medium:
  days and months

Long:
  years and generational events
```

Events move into finer buckets as their due time approaches.

## Waking agents

An agent wakes because:

- A predicted internal threshold is reached.
- A local physical event intersects perception.
- A person communicates.
- A plan reaches a decision point.
- A journey reaches a waypoint.
- A scheduled obligation occurs.
- A dependent requires care.
- A regional event affects it.

## Sleeping agents

An inactive agent still has exact state:

- Last reference time.
- Rates.
- location or journey.
- current commitment.
- next event.
- relationships.
- memories.
- beliefs.
- lexicon.

No anonymous statistical replacement is needed.

## Thought frequency

Activity should determine computational frequency.

Examples:

```text
Sleeping person:
  one scheduled wake event unless interrupted

Safe traveler:
  waypoint and hazard events

Farmer doing repetitive work:
  periodic work checks

Trader negotiating:
  frequent social decisions

Soldier in combat:
  rapid reactions

Hungry infant:
  frequent distress events
```

## Event causality

Each event should have enough provenance for debugging:

- Triggering event ID.
- Source agent or system.
- region.
- timestamp.
- causal parent.
- random stream key.
- resulting commands.

## Deterministic command buffering

During a processing phase, systems should emit commands:

- Update belief.
- Create signal.
- Start journey.
- Move item.
- Apply injury.
- Schedule event.
- Create relationship.
- Transfer agent.

Commands are sorted and applied at deterministic boundaries.

## Avoid event explosions

Risks:

- Every sound notifying an entire region.
- Repeated rescheduling.
- Cascading social updates.
- Immediate rumor broadcast.
- Per-tick movement events.
- Memory events for routine actions.

Controls:

- Spatially limited listeners.
- attention filters.
- coalescing.
- minimum reschedule intervals.
- route-level movement.
- summary updates.
- bounded reaction chains.
