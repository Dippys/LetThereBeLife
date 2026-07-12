---
name: orient-let-there-be-life
description: Establish grounded repository context before work in Let There Be Life. Use at the start of implementation, diagnosis, review, planning, dependency, or architecture tasks when Codex must identify relevant code, immutable design input, living documentation, crate boundaries, and validation requirements.
---

# Orient Let There Be Life

Build an evidence-backed task map before changing files.

1. Read root `AGENTS.md` and obey its routing and immutable-directory rules.
2. Inspect the workspace with `rg --files`, then read the affected crate manifests, modules, tests, and `Documentation/` files.
3. Read only the relevant `InitialDocumentation/` design files. Never modify anything in that directory.
4. Compare intended design, current living documentation, and executable code. Trust code for current behavior and record drift rather than hiding it.
5. Identify affected crates, ownership boundaries, invariants, tests, documentation, and likely validation commands.
6. State any assumption that could materially alter the implementation. Otherwise proceed with the smallest coherent interpretation.
7. Hand the task to the appropriate repository skills listed in `AGENTS.md`.

Do not produce a generic repository summary. Focus orientation on the active request.

