---
name: implement-rust-engine
description: Implement or refactor Rust engine functionality in Let There Be Life while preserving deterministic headless simulation, clean crate ownership, fixed-step presentation boundaries, explicit APIs, tests, and maintainable data-oriented code. Use for changes under crates/, Cargo manifests, runtime behavior, engine systems, or dependencies.
---

# Implement Rust Engine

Read [references/rust-engine-standards.md](references/rust-engine-standards.md), affected code, and relevant living documentation before editing.

1. Define the owning crate and public contract before adding types.
2. Keep deterministic simulation state and rules in headless crates. Keep OS, window, input, rendering, and UI concerns in clients.
3. Prefer explicit domain types, stable integer handles, compact data, bounded work, and deterministic iteration order.
4. Avoid speculative abstractions and dependencies. Record every new dependency and architectural boundary in living documentation.
5. Apply `$optimize-runtime-footprint` to population-scale records, hot loops, collections, allocations, or cleanup work.
6. Handle invalid input deliberately. Avoid panics in normal runtime paths; reserve assertions for violated internal invariants.
7. Add focused unit or integration tests for behavior, edge cases, determinism, and regressions.
8. Apply `$maintain-living-docs`, `$validate-rust-workspace`, and `$review-engine-quality` before handoff.

Never change `InitialDocumentation/`.
