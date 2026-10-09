# Project Status

_Last updated: 2026-10-09 (repository cleanup after a ~3-month pause; last code work 2026-07-17)._

## Where the project is

The design spec ([`InitialDocumentation/16_IMPLEMENTATION_ROADMAP.md`](../InitialDocumentation/16_IMPLEMENTATION_ROADMAP.md))
lays out phases 0–10, from world → physical agents → cognition → communication → language → society. **Phases 1 and 2 are done.**

| Phase | State |
|---|---|
| 0. Technical spikes: workspace, headless core, viewer, quality gate | Done |
| 1. World foundation: terrain, climate, rivers/lakes, biomes, resources | Done |
| 2. Physical agents: movement, perception, needs, gathering, sleep, shelter, health, death | Done |
| 3. Beliefs, memory, relationships | **Next.** Planned in [`plans/PHASE3_BELIEFS_RELATIONSHIPS_PLAN.md`](plans/PHASE3_BELIEFS_RELATIONSHIPS_PLAN.md), not started |
| 4. Nonverbal communication | Not started |
| 5–7. Proto-language, children and transmission, invention and diffusion | Not started |
| 8–10. Settlements and economy, migration and language divergence, conflict and institutions | Not started |

## What works today

- **World:** a finite 65,536 × 65,536-cell world (1,048,576 chunks of 64×64), generated
  deterministically from a seed: tectonic plates, continents/oceans, mountains, latitude climate,
  prevailing winds, whole-world drainage with lakes and continuous rivers, biomes, and sparse
  trees, berry bushes, and rocks. Chunks stream in on demand.
- **Agents:** dense compact agents with hunger/thirst/rest/exposure needs, a local (radius-8)
  decision policy, A* routing, gathering/eating/drinking, sleep, one-cell lean-to shelters,
  health damage, incapacitation, and death. Everything runs on a totally ordered event scheduler
  and is fully deterministic.
- **Viewer:** pan/zoom over the whole world, HUD and hover inspection, `T` to spawn agents,
  numpad menu to place trees/berries/rocks/water, speed 1×–256×, right-drag to generate areas.
- **Headless:** smoke runs and canonical 20/100-agent 600,000-tick survival scenarios with
  stable report hashes.
- **Full-world archive (optional):** `sim-viewer --pregenerate-world` writes a ~16 GiB
  checksummed file of every chunk so the viewer can show the whole world instantly. It is
  **not present** on this machine right now (`target/world-cache/` is empty); the viewer falls back
  to procedural generation automatically.

## Work that was uncommitted when the project paused

When cleanup started, about 500 lines of finished but uncommitted work sat in the tree. It was
the full-world archive (`sim-core/src/world/archive.rs` plus viewer integration) and cursor-local
first spawns (`T` works as soon as the terrain under the cursor is loaded). The old docs record it as
validated on 2026-07-17, and `cargo test --workspace` still passes. Commit it on its own before
doing new work.

## Known problems and limitations

- **Agents die in dry areas.** Perception is radius 8 with no memory, so an agent spawned far from
  fresh water wanders until it dies of thirst. That's expected for Phase 2. Phase 3 landmark
  memory is meant to fix it.
- **Intermittent viewer smoke crash.** `sim-viewer --smoke-frames 2` exited with
  `0xc0000409` (STATUS_STACK_BUFFER_OVERRUN, no panic message) in 1 of 19 runs on 2026-10-09,
  the first run right after `cargo test`. The other 18 passed (debug and release). Cause unknown;
  it could be GPU/driver teardown. Worth a look if it recurs.
- **No save/load.** Only the immutable world archive exists; simulation state can't be persisted.
- **Large files:** `sim-core/src/lib.rs` (~4,100 lines; the `Engine` impl is ~1,900 of them) and
  `sim-viewer/src/renderer.rs` (~2,900 lines) are due for a split before Phase 3 adds more to `Engine`.
- **Test files are named by slice** (`physical_agent_slice0.rs` … `slice8.rs`), not by feature.
  That's fine, but renaming them by topic would make them easier to find.
- **World size far exceeds the agent count.** Viewer max is 4,096 agents; canonical scenarios
  use a 2,048-cell square. The big-world machinery is ahead of what the agents need.

## What's next (options)

1. **Phase 3 as planned.** Slice 0 sets up compact cognitive storage, then observations,
   landmark beliefs (which fix the dry-spawn deaths), survival planning, episodic memory, and trust.
   The plan is detailed and ready.
2. **Prototype the core idea earlier.** Build a deliberately rough signals → inference → belief
   loop in a tiny world, to test the project's central bet before investing in full Phase 3
   infrastructure.
3. **Housekeeping first (small):** commit the pending work, then split `Engine` out of `lib.rs`.

Whichever path is chosen, update this file when it starts and when it lands.
