# Living Documentation

This directory describes the code that is currently implemented and the decisions made during development.

`InitialDocumentation/` is immutable design input. Agents and contributors may read it, but all implementation-driven updates belong here.

## Documents

- [Current implementation](CURRENT_IMPLEMENTATION.md): exact functionality present in code.
- [Architecture](ARCHITECTURE.md): current crate boundaries, contracts, and dependencies.
- [Architecture decisions](ARCHITECTURE_DECISIONS.md): append-only decisions made during implementation.
- [Roadmap](ROADMAP.md): completed, active, next, and deferred work.
- [World foundation improvement plan](WORLD_FOUNDATION_PLAN.md): ordered Phase 1 terrain, hydrology, climate, feature, rendering, and agent-readiness slices with acceptance criteria.
- [Physical agent loop implementation plan](PHYSICAL_AGENT_PLAN.md): ordered Phase 2 agent, scheduling, movement, perception, needs, survival, shelter, and death slices with per-slice acceptance gates.
- [Testing](TESTING.md): validation commands, existing coverage, and known gaps.
- [Performance](PERFORMANCE.md): compact-data rules, budgets, measurements, and optimization evidence.

## Status language

- **Implemented** means executable code exists and has been validated.
- **Active** means work is currently underway.
- **Planned** means intended but not implemented.
- **Open** means a decision has not been made.
