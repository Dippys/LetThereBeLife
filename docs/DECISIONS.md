# Decisions

Durable technical decisions, newest last. D-001 … D-058 (2026-07-12 → 2026-07-17) are in
[`archive/ARCHITECTURE_DECISIONS.md`](archive/ARCHITECTURE_DECISIONS.md), and
[`ARCHITECTURE.md`](ARCHITECTURE.md) summarizes the important ones.

Format: a few lines per decision. If a decision changes, add a new entry that supersedes the old one.

## D-059: Simplified repository workflow and docs (2026-10-09)

**Decision:** Replace the eight Codex skills, model-routing rules, and nine living-doc files with
one `AGENTS.md` (`CLAUDE.md` imports it), short living docs in `docs/`
(`STATUS`, `ARCHITECTURE`, `DEVELOPMENT`, `DECISIONS`, `plans/`), and a plain validation script
in `scripts/`. The old docs are kept unchanged in `docs/archive/`.
**Why:** The process overhead and doc verbosity had grown larger than the work they supported,
which made the project hard to pick back up.
**Consequences:** Docs describe current state briefly instead of logging every run. The
`InitialDocumentation/` checksum gate is kept (`scripts/initial-documentation.sha256`).
The validate scripts no longer check skill manifests or that the config copied into `target/`
matches, and the GPU viewer smoke is now opt-in (`--gpu` / `-Gpu`).

## D-060: `sim-world` crate and folder modules (2026-10-09)

**Decision:** Move world storage, generation, and the archive into a new `sim-world` crate, which
`sim-core` depends on and re-exports, so `sim_core::…` paths are unchanged. Split every large
file into folder modules of at most about 600 lines (`engine/`, `agent/population/`, `policy/`,
`render/`, `app/`, `generation/`, …) and rename integration tests by topic.
**Why:** World code had no dependency on agents, so the crate boundary is real. Files of
1,000–4,000 lines were hard to work in.
**Consequences:** Agents and `Engine` stay in one crate, because they are tightly coupled.
`MIN_TRAVERSAL_COST` and `TraversalStep::blocked` became `pub` in `sim-world` to cross the boundary.
Some `pub(super)`/`pub(crate)` widening happened inside crates. Changing agent code no longer
recompiles or retests world generation (`cargo test -p sim-core`).
