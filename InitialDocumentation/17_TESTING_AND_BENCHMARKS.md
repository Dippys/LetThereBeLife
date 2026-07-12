# Testing and Benchmarks

## Unit tests

Test:

- Need interpolation.
- threshold scheduling.
- signal similarity.
- lexical evidence update.
- interpretation normalization.
- relationship consolidation.
- memory importance.
- deterministic RNG.
- chunk generation.
- route position derivation.
- pool allocation and compaction.
- serialization round trips.

## Property tests

Examples:

- Confidence remains bounded.
- Probabilities normalize.
- Same seed creates same chunk.
- Save/load preserves state.
- No receiver can access private intent.
- Event order remains deterministic.
- Pool handles do not alias.
- Relationship count remains sparse.
- Forgotten memory no longer affects retrieval unless consolidated.

## Communication tests

### Grounding

A child repeatedly observes a signal near food and gradually increases FOOD confidence.

### Ambiguity

A signal in different contexts develops multiple hypotheses.

### Misunderstanding

Different personal lexicons produce different actions from the same public event.

### Repair

Clarification raises confidence more than an unconfirmed ambiguous exposure.

### No cheating

Receiver state is unchanged if only private intent changes while public signals remain identical.

This is one of the most important invariant tests.

## Language evolution tests

- Competing forms spread at different rates.
- Isolated groups diverge.
- Contact increases borrowing.
- Prestige changes adoption probability.
- Children regularize inconsistent order.
- Related forms have higher mutual recognition.
- Weak unused associations decay.

## Simulation invariants

- Dead agents do not execute normal events.
- Agent location belongs to one region or journey.
- Inventory ownership remains consistent.
- Parent-child links remain valid.
- Event times do not move backward.
- Cross-region transfer is atomic.
- No item exists in two inventories.
- No settlement resident list contains duplicate entries.

## Performance benchmarks

### Core storage

- 10 million agent cores.
- memory usage.
- sequential scan.
- random access.
- snapshot size.

### Scheduler

- 10 million future events.
- insertion.
- cancellation.
- rescheduling.
- due-event extraction.
- bucket rollover.

### Sparse pools

- Average 20 relationships.
- average 10 beliefs.
- average 8 memories.
- average 50 lexical records.
- fragmentation.
- compaction.
- iteration.

### Communication

- Listener spatial query.
- attention filtering.
- 3–10 interpretation candidates.
- learning update.
- repair loop.

### World

- Chunk generation time.
- memory per loaded chunk.
- delta application.
- load/unload churn.
- region pathfinding.

### Persistence

- Full save.
- incremental save.
- load.
- replay.
- checksum verification.

## Scenario tests

- Family survives winter.
- Child learns household signals.
- Two families invent competing words.
- Trader learns foreign vocabulary.
- False rumor spreads.
- Misunderstanding causes failed trade.
- Isolated village develops dialect.
- Migrants introduce loanword.
- Leader's prestigious form replaces local form.
- Fire activates only affected local agents.

## Long-run soak tests

Run decades or centuries and monitor:

- Memory growth.
- event queue growth.
- language explosion.
- relationship explosion.
- settlement count.
- save size.
- numerical drift.
- deterministic replay.
- performance degradation.
