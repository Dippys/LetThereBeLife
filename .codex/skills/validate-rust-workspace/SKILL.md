---
name: validate-rust-workspace
description: Run and interpret the complete Let There Be Life repository quality gate. Use after Rust, Cargo, skill, agent-instruction, or living-documentation changes and before claiming completion. Verifies immutable initial documentation, skill structure, formatting, tests, Clippy warnings, and optional runtime behavior.
---

# Validate Rust Workspace

Run `scripts/validate.ps1` from the repository root. Do not replace a failing check with a weaker command.

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .codex/skills/validate-rust-workspace/scripts/validate.ps1
```

Pass `-Runtime` when runtime entry points or lifecycle behavior changed. The script verifies the initial-documentation checksum manifest, all repository skills, formatting, workspace tests, Clippy with warnings denied, and a headless smoke test.

On failure:

1. Report the first failing stage and exact error.
2. Fix failures caused by the active change.
3. Rerun the complete gate from the beginning.
4. Do not claim success for checks that did not execute.

