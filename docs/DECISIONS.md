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

## D-061: Behavior study before behavior changes (2026-10-10)

**Decision:** Add `sim-headless --study`, which runs viewer-like agents (random land, near-water, or
group spawns; no supplies; selectable mind) and reports survival, death causes, roaming, idleness,
meals, gestures, and per-agent decision traces.
**Why:** "Agents camp and die" was a feeling. The study turned it into numbers and found causes no
one had guessed: water was almost absent at agent scale, agents slept on tree stumps, and
straight-line exploration missed nearby water.
**Consequences:** Behavior changes should come with before/after study numbers. The study is a
measurement tool, not a fingerprint; its regression tests only guard determinism and that memory
beats the legacy policy on seed 1.

## D-062: Agent-scale waterholes; world generator version 2 (2026-10-10)

**Decision:** Add at most one small pond per chunk, with probability by local moisture (about 3%
in desert up to 80% in wet forest), kept inside its chunk. Shores in dry or open country grow trees
and berries.
**Why:** Continental drainage put most land thousands of cells from fresh water (3% of seed 1's
start area within ~64 cells), while agents see 8 cells. Forests without water were incoherent.
**Consequences:** `WORLD_GENERATOR_VERSION` is now 2, so existing archives are rejected and must be
regenerated. World-quality hashes and headless fingerprints changed. Deserts stay harsh.

## D-063: Private mental maps and memory-driven decisions (2026-10-10)

**Decision:** Give each agent a fixed 256-byte `MentalMap` (12 remembered places in per-kind
slots, 24 explored tiles, spiral-search and sharing state) and a separate `deliberate` policy that
uses it. Select it with `PolicyOptions`; the legacy reactive policy stays unchanged and remains the
default for `activate_physical_policy`.
**Why:** The camping came from agents knowing nothing beyond their view. One general knowledge
layer covers water, food, wood, stone, and shelters, and is the base for communication.
**Consequences:** Canonical scenarios still run the legacy policy. The viewer and the study use the
full mind. Memories are checked only by looking again, never against world truth. This is a lighter
version of the archived Phase 3 slices 0–2; see `docs/plans/MINDS.md`.

## D-064: Sharing through pointing gestures, not telepathy (2026-10-10)

**Decision:** An agent with no pressing need, and someone awake in view, may spend 120 ticks
pointing at a remembered place outside the view. Awake agents in view infer a direction and an
order-of-magnitude distance and store a hint with a search radius. They search it, confirm it by
seeing it, or forget it after failed searches.
**Why:** The user asked that explorers share what they found. The vision forbids transmitting
meaning directly, so a lossy observable gesture is the faithful version.
**Consequences:** Only first-hand places are pointed at (no rumor chains yet). The measured benefit
is small because agents who meet usually know the same places; regrouping is the next step.

## D-065: Perception lists reserved cells (2026-10-10)

**Decision:** `PhysicalPerception` gains `reserved_cells`: cells holding a tree or rock, depleted
or not. Build-site and sleep-spot choices avoid them.
**Why:** Agents gather by standing on trees. The physics refuses sleeping or building there, so
agents retried the same spot until they died of exhaustion.
**Consequences:** This also changes the legacy policy's build-site choice (a bug fix). Struct
literals of `PhysicalPerception` need the new field.

## D-066: Prune stale events every tick (2026-10-10)

**Decision:** Run scheduler compaction at the end of every tick (one length check unless due), with
the threshold tightened from `5 × population + 4,096` to `8 × population + 256`.
**Why:** Compaction ran only on manual commands, so under autonomous policy, lazily cancelled
events piled up unchecked. The new world's extra activity pushed the 100-agent canonical peak queue
from 1,967 to 7,186, past its 2,000 bound. An agent holds at most about 7 live events.
**Consequences:** The peak queue is now 1,247 with the same outcomes (74 living, 26 deaths). Stale
diagnostics for pruned events no longer appear, which changes canonical report hashes.
