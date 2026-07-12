# Storage and Memory Budget

## Four kilobytes per NPC

A 4 KiB average personal-state budget is plausible if:

- It is an average, not a fixed allocation.
- Newborns and simple agents use less.
- Rare complex agents may use more.
- Routine memories are forgotten or consolidated.
- Variable state is sparse and pooled.
- Common immutable data is interned.
- IDs and weights are compact.
- Hot and cold data are separated.

For ten million NPCs:

```text
4,096 bytes × 10,000,000
= 40.96 GB
≈ 38.15 GiB
```

This leaves little room on a 64 GB machine once world data, indexes, events, allocator overhead, rendering, and the operating system are included. A 96–128 GB development machine provides safer headroom.

## Expected distribution

Possible rough distribution:

```text
Infants and children:              0.3–1.0 KiB
Ordinary young adults:            1.0–2.5 KiB
Established adults:               2.0–4.0 KiB
Older/socially complex adults:    4.0–8.0 KiB
Exceptional agents:               8.0–32.0+ KiB
```

The target should be a population average below 4 KiB.

## Example ordinary adult

Possible budget:

| Category | Approximate size |
|---|---:|
| Core identity, time, location | 64–160 B |
| Needs, emotion, personality | 64–160 B |
| Current goal and compact plan | 64–192 B |
| Family and memberships | 32–96 B |
| Skills and occupation | 32–128 B |
| 16 relationships | 192–384 B |
| 12 beliefs | 144–288 B |
| 8 episodic memories | 128–256 B |
| 50 lexical associations | 400–800 B |
| Inventory/ownership handles | 32–128 B |
| Indexes and scheduling | 32–96 B |

Likely total: roughly 1.2–2.8 KiB depending on layout.

## Do not allocate `[u8; 4096]` per NPC

A fixed buffer wastes memory for babies and simple agents.

Use:

- Dense core arrays.
- Slab or size-class pools.
- Packed ranges.
- Shared immutable tables.
- Compact exceptions.
- Optional cold-state paging.

## Language pressure

Language may be the largest cognitive section.

A 12-byte lexical association gives:

```text
50 entries  = 600 B
100 entries = 1,200 B
250 entries = 3,000 B
```

Optimizations:

- Intern signal forms.
- Common vocabulary familiarity bitsets.
- Quantized confidence.
- Store ambiguity only when it exists.
- Store foreign and personal exceptions explicitly.
- Use community priors as compression, not automatic knowledge.
- Prune unused weak hypotheses.

## Relationship pressure

A 16-byte relationship record:

```text
20 relationships × 10 million NPCs
≈ 3.0 GiB
```

Store only meaningful relationships.

## Memory pressure

Compact episodic memories may contain:

- Event type.
- Subject.
- Object.
- place.
- time.
- emotional strength.
- confidence.
- flags.

Routine events should update summaries and disappear.

## Belief pressure

Avoid storing obvious permanent facts separately for every person.

Possible compression:

- Shared cultural priors.
- Local environmental knowledge templates.
- Personal exceptions and confidence.
- Region or household shared records with explicit learned membership.
- Compact proposition templates.

The system must still preserve individual uncertainty.

## World and infrastructure budget

Beyond NPC state, reserve memory for:

- Chunk cache.
- terrain features.
- resources.
- buildings.
- inventories and items.
- event wheels.
- spatial indexes.
- region messages.
- path data.
- save snapshots.
- temporary buffers.
- rendering.
- analytics.

A realistic full working set may be 48–96+ GiB depending on loaded terrain and average personal complexity.

## Disk storage

Cold history can move to disk:

- Deceased agent archives.
- Old episodic memories.
- historical event logs.
- unloaded chunk deltas.
- long-term genealogy.
- language history samples.

Disk-backed data should not be required for ordinary immediate cognition.
