# Save, Load, and Determinism

## Why determinism matters

Emergent simulations are difficult to debug. A rare language misunderstanding may cause a conflict decades later.

Deterministic replay allows:

- Reproducing bugs.
- comparing optimization changes.
- validating parallel execution.
- bisecting historical divergence.
- testing language adoption.
- verifying save/load.

## Deterministic inputs

A replay depends on:

- World seed.
- simulation version.
- generator version.
- configuration.
- player inputs.
- random stream states or keys.
- deterministic command ordering.
- event ordering policy.

## Randomness

Do not use a single global mutable RNG accessed by many threads.

Prefer keyed random streams:

```text
random(
  world_seed,
  system_id,
  region_id,
  agent_id,
  event_id,
  purpose_tag
)
```

This makes random outcomes less sensitive to unrelated scheduling changes.

## Snapshot strategy

Possible layers:

### Full snapshot

Contains:

- Agent cores.
- pools.
- event wheels.
- chunks and modifications.
- institutions.
- region indexes.
- random state.
- version metadata.

### Incremental snapshot

Stores changes since a base snapshot.

### Event log

Stores player actions and selected simulation commands for replay.

Use periodic full snapshots plus incremental logs.

## Chunk persistence

Unmodified chunks require only seed and generator version.

Modified chunks store:

- Feature removal/addition.
- construction.
- excavation.
- resource depletion.
- ownership.
- environmental changes.
- local dynamic objects.

## Pool serialization

Packed pools need:

- Record arrays.
- free lists.
- generation tables if used.
- per-agent ranges.
- compaction metadata.
- schema version.

## Save compatibility

Version every major record.

Migration tools should convert old saves where possible.

Do not rely on raw in-memory struct dumps across compiler versions unless the format is explicitly frozen and validated.

## Checkpoint consistency

At a snapshot boundary:

1. Stop or complete the current deterministic phase.
2. Flush command buffers.
3. apply cross-region messages.
4. capture event-wheel state.
5. write metadata and checksums.
6. atomically publish the snapshot.

## Historical archives

Deceased agents and old memories may be archived separately, but references must remain resolvable for genealogy, history, and cultural analysis.
