# Architecture Overview

## High-level decomposition

```text
Simulation Core
├── Time and event scheduler
├── World and environment
├── Spatial indexes
├── Agent storage
├── Perception
├── Needs and emotions
├── Beliefs and memory
├── Goal selection
├── Planning and actions
├── Communication and language
├── Relationships and social systems
├── Family and demographics
├── Economy and institutions
├── Region ownership and parallelism
└── Persistence and replay

Presentation Client
├── Camera and rendering
├── Local entity views
├── Map inspection
├── Agent inspector
├── Language-history tools
├── Society graphs
└── Debug controls
```

The simulation core should run headlessly without rendering.

## Separation of simulation and presentation

The renderer must observe simulation state rather than own it.

A visible actor is only a view:

```text
Persistent NPC record
        ↓
Local presentation proxy
        ↓
Sprite, pixel, tile marker, or lightweight mesh
```

Removing a visual proxy must not remove the NPC.

Only a small visible subset of ten million agents should have rendered entities at once.

## Data-oriented core

Avoid a deep object graph where every NPC owns vectors, strings, hash maps, pointers, and polymorphic behavior.

Prefer:

- Dense arrays for universal state.
- Structure-of-arrays for hot fields.
- Packed sparse pools for variable state.
- Interned identifiers for concepts, signals, occupations, places, and item types.
- Region-owned mutable data.
- Buffered cross-region messages.
- Stable integer handles rather than object pointers.

## Major stores

```text
AgentCoreStore
NeedStore
EmotionStore
GoalStore
RelationshipPool
BeliefPool
MemoryPool
LexiconPool
InventoryPool
FamilyStore
SettlementStore
WorldChunkStore
EventScheduler
SpatialIndex
LanguageUsageLog
HistoricalEventLog
```

An agent core contains offsets or handles into sparse stores.

## Hot and cold data

### Hot data

Frequently accessed:

- Position or journey state.
- Region.
- Next scheduled event.
- Current activity.
- Current goal.
- Need reference values.
- Immediate emotional state.
- Flags.

Target: roughly 64–256 bytes per NPC.

### Cold data

Loaded only when relevant:

- Episodic memories.
- Detailed relationships.
- Personal lexical hypotheses.
- Long-term beliefs.
- Family history.
- Contracts and obligations.
- Cultural knowledge.
- Rare skills.

Cold data should not be dragged through CPU caches during routine scheduling.

## Suggested execution model

1. Advance simulation time.
2. Pop due events from regional timing wheels.
3. Process due events in parallel by region.
4. Generate local commands and cross-region messages.
5. Apply buffered commands at deterministic boundaries.
6. Schedule future events.
7. Update presentation snapshots if needed.
8. Periodically consolidate memory, aggregate analytics, and create snapshots.

## Region ownership

A worker can exclusively own one or more regions during a processing phase.

```text
Worker A → Regions 0, 4, 8
Worker B → Regions 1, 5, 9
Worker C → Regions 2, 6, 10
Worker D → Regions 3, 7, 11
```

Arbitrary cross-thread mutation should be prohibited. Cross-region effects become messages or commands.

## Suggested repository layout

```text
emergent-society/
├── crates/
│   ├── sim-core/
│   ├── sim-world/
│   ├── sim-agents/
│   ├── sim-language/
│   ├── sim-society/
│   ├── sim-storage/
│   ├── sim-persistence/
│   ├── sim-bench/
│   ├── sim-server/
│   └── sim-viewer/
├── docs/
├── tools/
├── assets/
└── tests/
```

A C++ repository can use the same boundaries as libraries or modules.
