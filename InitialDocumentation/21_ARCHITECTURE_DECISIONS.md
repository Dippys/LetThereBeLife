# Architecture Decision Records

## ADR-001: Native simulation core

**Decision:** Use Rust or modern C++ for the persistent simulation core.

**Reason:** Ten million persistent agents require compact layouts, predictable memory, and high-throughput event processing.

## ADR-002: Engine-independent headless core

**Decision:** Rendering and UI are clients of the simulation.

**Reason:** Enables headless benchmarks, deterministic tests, server execution, and replacement of the viewer.

## ADR-003: Event-driven cognition

**Decision:** NPCs think when triggered or scheduled, not every frame.

**Reason:** Continuous full updates are incompatible with the population target.

## ADR-004: Exact persistent individuals

**Decision:** Inactive people retain individual records rather than becoming anonymous population statistics.

**Reason:** Individual history and future reactivation are core product goals.

## ADR-005: Sparse personal state

**Decision:** Relationships, beliefs, memories, lexicons, and inventories use packed variable-length pools.

**Reason:** Fixed-capacity minds waste memory and cannot express a bell-curve distribution.

## ADR-006: Four-kilobyte average target

**Decision:** Aim below 4 KiB average personal state, with smaller children and larger exceptional agents.

**Reason:** Approximately 38.15 GiB at ten million agents is plausible but demands strict control.

## ADR-007: Chunked procedural world

**Decision:** Do not allocate a 1,000,000 × 1,000,000 dense map.

**Reason:** One trillion cells create terabyte-scale storage.

## ADR-008: Dense terrain, sparse features

**Decision:** Store basic terrain densely within loaded chunks; store trees, rocks, buildings, and deposits sparsely or semi-sparsely.

**Reason:** Stateful objects do not fit a rigid top tile layer.

## ADR-009: Private intent/public signal boundary

**Decision:** Sender intent and receiver-visible signal events are distinct types and stores.

**Reason:** Prevents accidental perfect information transfer.

## ADR-010: Community language is derived

**Decision:** Community language statistics summarize personal usage.

**Reason:** Language is a convention, not a universal dictionary.

## ADR-011: Region-owned mutation

**Decision:** Workers mutate owned regions and exchange buffered messages.

**Reason:** Reduces locking and makes parallelism safer and more deterministic.

## ADR-012: Generic ECS only where beneficial

**Decision:** Use custom arrays/pools for the full persistent population; optionally use ECS for visible or highly active entities.

**Reason:** The persistent minds are sparse, irregular, and mostly inactive.
