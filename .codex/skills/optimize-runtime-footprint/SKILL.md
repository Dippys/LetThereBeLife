---
name: optimize-runtime-footprint
description: Minimize measured CPU time, memory footprint, allocation overhead, cache misses, serialized size, and binary/runtime bloat in Let There Be Life. Use when designing hot or population-scale data, choosing integer widths and layouts, reviewing loops or allocations, cleaning dead code, benchmarking, or optimizing simulation and rendering systems. Do not use source-line count as a performance proxy.
---

# Optimize Runtime Footprint

Read [references/compact-performance-rules.md](references/compact-performance-rules.md), affected code, and `Documentation/PERFORMANCE.md`.

1. Define the scale, hot path, lifetime, valid value range, and measurable budget before optimizing.
2. Measure the baseline: type size, allocation count/capacity, resident bytes, throughput, latency, cache behavior, or artifact size as relevant.
3. Choose the smallest integer or quantized representation that covers the proven domain with explicit overflow behavior.
4. Prefer dense contiguous hot data, sparse cold records, stable compact handles, interned immutable values, and reused buffers.
5. Remove dead code, unused state, duplicate representations, unnecessary clones, transient allocations, and excess retained capacity.
6. Avoid per-frame global work, per-agent object graphs, unbounded collections, pointer-heavy layouts, and hidden allocator overhead.
7. Preserve correctness, determinism, clear invariants, and safe ownership. Do not code-golf or add `unsafe` merely to shorten code.
8. Add size assertions or benchmarks for important budgets so regressions become visible.
9. Record measured results and remaining gaps in `Documentation/PERFORMANCE.md`.
10. Apply `$review-engine-quality` and `$validate-rust-workspace` before completion.

Optimize in this order: algorithm and scheduling, amount of stored state, allocation/layout, cache locality, representation width, then micro-operations.

