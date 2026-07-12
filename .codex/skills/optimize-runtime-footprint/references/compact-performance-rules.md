# Compact Performance Rules

## Representation

- Derive widths from valid ranges: use `u8` for bounded 0–255 values, `u16` for bounded 0–65,535 values, and wider types only when the domain requires them.
- Use newtypes for compact IDs when raw integers could be mixed accidentally.
- Quantize probabilities, needs, confidence, and weights when tests show acceptable precision.
- Give field ordering and alignment deliberate attention; verify with `size_of`, not intuition.
- Use `#[repr(...)]` only when a stable layout or measured size benefit is required.
- Keep hot records fixed, compact, and pointer-free where practical. Move optional or variable state into sparse pools.

## Allocation and locality

- Reserve known capacities, but do not retain worst-case capacity everywhere.
- Reuse scratch buffers in hot paths.
- Prefer slices and borrowed views to cloning.
- Avoid a heap allocation per agent, relationship, memory, event, cell, or rendered proxy.
- Separate frequently scanned fields from cold descriptive data.

## Work

- Eliminate work before making work faster.
- Schedule events and threshold crossings instead of scanning every NPC each frame.
- Bound searches and candidate sets.
- Batch similar work and preserve deterministic ordering.
- Use spatial and regional indexes before global iteration.

## Cleanup

- Delete unreachable branches, unused fields, stale adapters, duplicate helpers, and obsolete dependencies when evidence shows they are no longer needed.
- Avoid compressing readable expressions solely to reduce source lines. Source length is not runtime size.
- Keep a small abstraction only when it enforces an invariant or removes repeated complexity.

## Evidence

- Add compile-time or unit size assertions for foundational hot records.
- Benchmark representative distributions and release builds.
- Include allocator capacity and fragmentation in memory figures.
- Compare before and after with the same seed, workload, compiler profile, and machine.

