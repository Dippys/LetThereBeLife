# Architecture Decisions

Append new decisions. If a decision changes, add a superseding entry rather than rewriting history.

## D-001: Rust workspace and headless core

Date: 2026-07-12

**Decision:** Use Rust and keep deterministic simulation behavior in a standalone `sim-core` crate.

**Reason:** The simulation must run without rendering for testing, benchmarking, servers, and deterministic replay work.

## D-002: Replaceable bootstrap presentation

Date: 2026-07-12

**Decision:** Confine `winit` and temporary `softbuffer` rendering to `sim-viewer`.

**Reason:** A working native window is useful now, while the production rendering strategy remains replaceable.

## D-003: Immutable initial documentation

Date: 2026-07-12

**Decision:** Treat `InitialDocumentation/` as read-only design input and maintain implementation documentation under `Documentation/`.

**Reason:** The initial design must remain an unchanged baseline while living records evolve with the code.

## D-004: Routed repository skills and enforced quality gate

Date: 2026-07-12

**Decision:** Use focused repository-local skills for orientation, implementation, documentation, validation, review, and workflow evolution, coordinated through `AGENTS.md`. Protect immutable design input with a checksum manifest.

**Reason:** Focused triggers reduce instruction overlap, while a mandatory lifecycle ensures agents inspect context, maintain documentation, review quality, and validate before completion.

**Consequences:** Skill or agent-governance changes must pass the same repository validation gate. Intentional changes to the immutable baseline require explicit user direction and a separately reviewed checksum update.
