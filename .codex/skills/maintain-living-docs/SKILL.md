---
name: maintain-living-docs
description: Synchronize writable Let There Be Life documentation with implemented code. Use after implementation, refactoring, dependency, architecture, testing, roadmap, control, setup, or review changes, and whenever code-to-documentation drift is found. Writes only under Documentation/ and the root README; InitialDocumentation/ is strictly read-only.
---

# Maintain Living Documentation

Read [references/routing.md](references/routing.md) and the changed code.

1. Describe executable reality, not intended future behavior.
2. Update only affected files under `Documentation/` plus the root `README.md` when onboarding or controls change.
3. Mark status as Implemented, Active, Planned, or Open.
4. Append durable decisions with date, decision, reason, consequences, and supersession where applicable.
5. Use crate, module, type, and command names. Document boundaries and invariants rather than line-by-line mechanics.
6. Keep details canonical in one document and link instead of duplicating.
7. Verify every claim against code or validation output.

Never create, edit, format, rename, move, or delete anything under `InitialDocumentation/`.

