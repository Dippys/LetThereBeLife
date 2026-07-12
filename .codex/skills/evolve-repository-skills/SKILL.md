---
name: evolve-repository-skills
description: Create or improve repository-local Let There Be Life skills and AGENTS.md routing when repeated workflow gaps, new subsystems, fragile procedures, or stale instructions are discovered. Use only when a reusable process is missing or existing skill guidance no longer matches the repository.
---

# Evolve Repository Skills

1. Confirm the gap is reusable. Fix one-off code or documentation directly instead of creating a skill.
2. Inspect all current `.codex/skills/*/SKILL.md` files and avoid overlapping triggers or duplicated instructions.
3. Use `$skill-creator` and its initialization and validation scripts for every new skill.
4. Keep skills concise and imperative. Put detailed routing, schemas, or checklists in one-level references; add scripts only for repeated deterministic procedures.
5. Add or adjust `AGENTS.md` routing so the new skill has a clear entry condition and sequence.
6. Update `Documentation/ARCHITECTURE_DECISIONS.md` when workflow governance materially changes.
7. Run `$validate-rust-workspace` after skill changes.

Never modify `InitialDocumentation/`. Do not create a skill merely to increase the skill count.

