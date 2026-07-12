# Let There Be Life Agent Instructions

These instructions apply to the entire repository.

## Immutable input

`InitialDocumentation/` is read-only source material. Never create, edit, format, rename, move, or delete anything inside it. The checksum gate in `.codex/initial-documentation.sha256` enforces this rule. Record all evolving knowledge under `Documentation/`.

## Required task flow

1. Read this file and `.codex/skills/orient-let-there-be-life/SKILL.md` before non-trivial work.
2. Inspect relevant code, tests, writable documentation, and only the necessary immutable design documents.
3. Select the smallest applicable skill set from the routing table below.
4. Implement or diagnose from repository evidence. Do not claim behavior without inspecting it.
5. Apply `.codex/skills/maintain-living-docs/SKILL.md` after any change that affects behavior, architecture, dependencies, tests, controls, setup, status, or roadmap.
6. Apply `.codex/skills/review-engine-quality/SKILL.md` after non-trivial code changes or when asked to review.
7. Apply `.codex/skills/validate-rust-workspace/SKILL.md` before declaring a change complete.
8. Report implementation, documentation, review findings, and exact validation evidence.

## Skill routing

| Task | Skill |
|---|---|
| Repository orientation, planning, impact mapping | `.codex/skills/orient-let-there-be-life/SKILL.md` |
| Rust/Cargo implementation or refactoring | `.codex/skills/implement-rust-engine/SKILL.md` |
| Living documentation updates or drift | `.codex/skills/maintain-living-docs/SKILL.md` |
| Formatting, tests, linting, immutable-doc checks | `.codex/skills/validate-rust-workspace/SKILL.md` |
| Code, architecture, determinism, or quality review | `.codex/skills/review-engine-quality/SKILL.md` |
| Compact types, memory, allocations, hot paths, cleanup, or optimization | `.codex/skills/optimize-runtime-footprint/SKILL.md` |
| Repeated workflow gap or new project skill | `.codex/skills/evolve-repository-skills/SKILL.md` plus the system `$skill-creator` |
| Broad implementation lifecycle | `.codex/skills/maintain-let-there-be-life/SKILL.md` as coordinator |

Read every selected `SKILL.md` completely before acting. Read referenced resources only when routed by that skill.

## Non-negotiable engineering rules

- Keep `sim-core` independent from windowing, rendering, UI, and OS lifecycle dependencies.
- Treat presentation objects as views, never persistent simulation truth.
- Advance simulation through fixed deterministic ticks or scheduled events, never variable render frames.
- Make seeds, ordering, ownership, mutation boundaries, and invalid-input behavior explicit.
- Minimize work and stored state first, then allocations and layout, then variable width. Use the smallest proven representation and verify it with size assertions or benchmarks.
- Treat source brevity as a maintenance preference, not a performance metric. Never sacrifice clarity, safety, determinism, or testability for fewer lines.
- Remove dead code, unused state, unnecessary clones/allocations, and obsolete dependencies when touching the owning area.
- Add tests for new behavior and regressions. Benchmark before scale-driven optimization.
- Avoid unrelated cleanup and speculative abstraction.
- Do not weaken or skip a failing quality gate to obtain a pass.

## Definition of done

A change is complete only when code works, relevant tests exist, living documentation matches reality, review findings are resolved or disclosed, the immutable design checksum passes, and the full validation gate succeeds. If a check cannot run, report it as an explicit incomplete validation gap.
