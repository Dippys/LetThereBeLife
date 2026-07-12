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

## D-005: Bounded deterministic starter world

Date: 2026-07-12

**Decision:** Begin with a fully generated 1,024 x 1,024-cell world in `sim-core`. Generate it in 64 x 64 chunk order from integer-only multi-scale noise, store terrain densely, and store surface objects as sparse feature records.

**Reason:** This creates an immediately visible and testable world-generation foundation while preserving deterministic headless ownership and the terrain/feature split needed for future chunk streaming.

**Consequences:** The starter world is allocated at engine construction and is not yet streamed, persisted, or modified. Future large-world work must replace full allocation with on-demand chunks without changing presentation into simulation truth.

## D-007: Initial area is configurable, not the world boundary

Date: 2026-07-12

**Decision:** Supersede the bounded-world interpretation in D-005. Treat `WorldConfig::initial_width` and `initial_height` as the area generated at startup, defaulting to 1,024 x 1,024, while preserving a future path to adjacent on-demand chunks. Load runtime settings through a shared `sim-config` crate and `config/simulation.toml`.

**Reason:** Startup allocation size and total world extent are different concerns. Terrain generation must not create artificial coastlines at the current loaded boundary, and viewer/headless configuration must remain consistent without adding filesystem dependencies to `sim-core`.

**Consequences:** The current engine still holds only the initial rectangle and does not expand at runtime. World-coordinate noise remains continuous beyond its edges. `sim-config` adds `serde` and `toml` dependencies outside the deterministic core.

## D-008: Selection-driven generated-area patches

Date: 2026-07-12

**Decision:** Generate deterministic signed-coordinate terrain patches through right-dragged `WorldRect` commands, capped at 1,048,576 cells per command.

**Reason:** Free camera space becomes actionable while simulation ownership and allocation bounds remain explicit.

**Consequences:** Patches remain loaded, and partially overlapping selections can duplicate cells until chunk-keyed streaming supersedes this bootstrap representation.

## D-006: Measured compactness over source-code golfing

Date: 2026-07-12

**Decision:** Optimize runtime work, stored state, allocations, locality, and representation width in that order. Use the smallest proven variable sizes and prefer concise code only when clarity, correctness, safety, determinism, and testability remain equal.

**Reason:** Ten million persistent individuals require compact layouts and bounded work, but fewer source lines do not inherently produce faster or smaller machine code. Unmeasured narrowing can overflow, increase conversion cost, or preserve padding without saving memory.

**Consequences:** Population-scale and hot-path changes require explicit ranges, size assertions or benchmarks, and before/after evidence. Cleanup should remove dead state and unnecessary allocations without introducing cryptic code or unjustified `unsafe`.

## D-009: Copy runtime configuration into Cargo profile outputs

Date: 2026-07-12

**Decision:** Make the shared `sim-config` build script copy `config/simulation.toml` into the active Cargo profile directory while preserving its `config/` relative path.

**Reason:** Viewer and headless binaries use the same default relative configuration path whether launched through Cargo or directly from `target/debug` or `target/release`.

**Consequences:** Configuration changes retrigger the shared crate build. The quality gate verifies that the debug copy exists and exactly matches the repository source configuration.

## D-010: Name the non-graphical runner sim-headless

Date: 2026-07-12

**Decision:** Rename `sim-server` to `sim-headless` and reserve the server name for a future executable that actually owns networking, client sessions, or an authoritative service lifecycle.

**Reason:** The current executable only loads configuration, advances an in-process engine for a fixed number of ticks, and exits. Its name should describe current behavior rather than imply a network contract that does not exist.

**Consequences:** Cargo commands, executable names, VS Code launch entries, validation, and living documentation use `sim-headless`.
