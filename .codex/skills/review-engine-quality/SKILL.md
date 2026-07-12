---
name: review-engine-quality
description: Review Let There Be Life Rust changes for correctness, regressions, deterministic behavior, ownership boundaries, scalability hazards, error handling, tests, dependency discipline, and living-documentation accuracy. Use for explicit reviews and as the final review pass after non-trivial implementation.
---

# Review Engine Quality

Read [references/review-checklist.md](references/review-checklist.md), the diff or changed files, tests, and affected documentation. Review behavior before style.

1. Identify concrete defects, regressions, invariant violations, nondeterminism, and missing tests.
2. Confirm `sim-core` remains headless and presentation state is not simulation truth.
3. Look for per-frame/per-agent work, unbounded collections, accidental allocations, unordered iteration, overflow, stale handles, and hidden global state.
4. Check API visibility, invalid-input behavior, dependency placement, and platform lifecycle handling.
5. Compare code with `Documentation/`; report stale claims. Never edit `InitialDocumentation/`.
6. Run or inspect `$validate-rust-workspace` evidence when completion is in scope.
7. Present findings by severity with file and line anchors. State explicitly when no findings remain, then list residual test gaps.

Do not invent issues to fill a checklist. Distinguish confirmed defects from risks requiring measurement.
