# Physical Agent Loop Implementation Plan

Last synchronized: 2026-07-16.

Status: **Active plan**. Phase 1 world foundation and Phase 2 Slice 0 are complete. Slice 1 is the next implementation target; later slices are planned and must be completed in order unless this document records a reviewed dependency change.

## Purpose

Build the complete Phase 2 physical-agent loop for 20-100 deterministic headless agents. The phase ends when agents can move, perceive nearby physical facts, experience physical needs, find and consume water and food, gather materials, sleep, construct and use minimal shelter, and survive or die for inspectable reasons.

This is a physical simulation proof, not the cognition or society layer. Behavior may use a small deterministic action policy, but it must not introduce beliefs, personal memories, relationships, language, institutions, or presentation-owned simulation truth.

## Phase 2 completion contract

The implemented loop must include:

- 20-100 persistent agents with stable compact identity;
- deterministic event scheduling rather than a complete per-agent update every tick;
- cardinal movement and bounded local routing through public `World` queries;
- bounded local physical perception;
- analytically evolving hunger, thirst, rest, and safety/exposure state;
- gathering, compact carried resources, eating, and drinking;
- sleep and wake transitions;
- minimal shelter construction and use;
- health consequences and simple death;
- a deterministic headless scenario in which survival and failure reasons are reported clearly.

The long-term ten-million-agent target constrains ownership, scheduling, and layout choices, but Phase 2 must prove understandable behavior at 20-100 agents before population scale increases.

## Current baseline

The repository already provides the world-side contracts needed to begin:

- `Engine` owns deterministic simulation state and advances fixed ticks; `SimulationSnapshot` is the read-only presentation boundary.
- `World::traversal_step` reports cardinal passability, traversal cost, elevation change, and explicit water, slope, or feature blocking.
- `World::water_at` distinguishes drinkable lake/river water from salt water and from unloaded/outside terrain.
- `World::resource_at` exposes immutable generated food, wood, or stone capacity while distinguishing resident absence from unavailable terrain.
- `Feature::identity` provides a stable generated-feature key within one seed and eventual generator version.
- deterministic resident cell/feature visitation and the Phase 1 settlement-candidate scenario provide bounded search building blocks.
- `sim-headless` eagerly materializes the configured bootstrap area before ticking; `sim-viewer` streams terrain asynchronously and currently has no agent presentation.

Slices 0-1 now provide compact agent storage, scheduled movement, one-agent-per-cell occupancy, bounded objective perception, and deterministic local routes. Not yet implemented are needs, action selection, inventory, resource depletion, structures, health, death, or richer agent-specific snapshots.

## Non-negotiable constraints

Every slice must preserve these rules:

1. `sim-core` owns agents, scheduler state, physical needs, inventory, mutable resource deltas, structures, and all authoritative outcomes. `sim-viewer` may later present read-only views only.
2. `World` remains immutable generated base plus resident materialization. Agent state and resource depletion must not be stored in generated `TerrainCell` or `Feature` records.
3. Simulation results must depend on seed, configuration, commands, and deterministic event order—not render frames, wall-clock timing, Rayon completion, cache residency timing, map iteration accidents, or thread count.
4. Terrain-dependent agent execution requires an explicit deterministic residency contract. An unloaded or outside-world query must yield a typed outcome; it must never synchronously generate terrain or opportunistically succeed because a viewer worker finished first.
5. Event ordering must be total and documented. Equal-time events require stable tie-breakers such as event class, `AgentId`, and monotonic event sequence.
6. Predictable needs evolve analytically from value, rate, and reference time. Do not scan every agent or increment every need on every simulation tick.
7. Random-looking choices use explicit deterministic keyed streams or hashes with documented purpose domains. Do not share mutable global RNG state across systems.
8. Agent and event storage must be data-oriented and measured. Do not use a generic ECS, one heap allocation per agent/event, or an oversized monolithic agent structure.
9. Spatial queries must be bounded and return deterministic order. No all-pairs agent comparisons or whole-world searches may enter a normal event path.
10. Each slice adds tests, size/throughput evidence where relevant, living-documentation updates, and a successful complete repository validation run.
11. `InitialDocumentation/` remains immutable throughout.

Phase 2 is headless-first. `Engine::new` must not auto-spawn agents into its initially nonresident world. Population initialization happens through an explicit fallible boundary only after a fixed simulation rectangle is completely resident; `sim-headless` calls it after eager materialization. Until the viewer has an equivalent deterministic startup gate, it may inspect an empty population but must not start physical-agent execution. Unexpected `Unloaded` results inside the declared active simulation rectangle are invariant/scenario failures, not timing-dependent retry signals.

## Agent execution protocol

An agent implementing one slice must:

1. Read `AGENTS.md`, the repository orientation skill, this complete plan, the current roadmap, current implementation, architecture decisions, tests, and the predecessor slice's recorded result.
2. Confirm that every prerequisite slice is marked **Implemented**. If not, implement only the earliest incomplete prerequisite.
3. Keep the active slice coherent: code, focused tests, integration scenario, measurements, and living documentation land together.
4. Record durable decisions in `ARCHITECTURE_DECISIONS.md`; do not silently settle an open checkpoint inside code.
5. Update this plan's slice status and implementation notes only after executable behavior and validation exist.
6. Apply the engine-quality review skill after non-trivial Rust changes and resolve or disclose every finding.
7. Run the complete validation gate from the beginning after the final code or documentation edit.
8. Stop after the active slice. Report the next slice, but do not begin it in the same task unless the user explicitly requests multiple slices.

If implementation proves that a later slice depends on a missing contract, amend this plan and the roadmap with evidence. Do not hide the dependency in speculative abstraction.

## Work sequence

| Order | Slice | Primary result |
| ---: | --- | --- |
| 0 | Compact agents and scheduled movement | Stable identity, engine-owned storage, deterministic scheduler, and one-step movement |
| 1 | Spatial occupancy, perception, and local routes | Bounded nearby queries, collision ownership, and deterministic short paths |
| 2 | Analytical physical needs | Hunger, thirst, rest, and exposure thresholds without per-tick agent scans |
| 3 | Deterministic physical action policy | Agents select and schedule understandable physical actions without cognition |
| 4 | Water, gathering, inventory, and consumption | Drink/eat/gather behavior plus sparse generated-resource depletion |
| 5 | Rest and sleep | Scheduled sleep/wake behavior with interruption and recovery |
| 6 | Minimal shelter | Gathered-material construction and shelter use without terrain mutation |
| 7 | Health, safety, and simple death | Physical failure consequences, terminal state, and causal reporting |
| 8 | Phase 2 integrated survival proof | Deterministic 20-100-agent runs, soak evidence, budgets, and phase exit |

## Slice 0: Compact agents and scheduled movement

Status: **Implemented** on 2026-07-16.

### Objective

Create the smallest authoritative agent representation and event boundary that can move 20-100 agents deterministically through resident terrain without committing to cognition, needs, or presentation structures.

### Decision checkpoints

Before freezing public types, measure and record:

- `u32` stable IDs versus any generational validation kept outside the hot record;
- dense record layout candidates, whether stable ID is implicit in a slot table, and whether the proven `[-32,768, 32,768)` envelope justifies compact internal `i16` coordinates behind checked `WorldPosition` conversions;
- scheduler candidates suitable for the initial population while preserving a path to bucketed or hierarchical scheduling;
- event cancellation/rescheduling strategy, including stale-event detection;
- exact simulation-time unit and overflow behavior.

The first implementation may use a simple bounded scheduler if benchmarks justify it, but its API and event key must not require per-agent tick scans or nondeterministic heap ties.

### Implementation area

- a responsibility-focused agent module under `crates/sim-core/src/`;
- a scheduler module under `crates/sim-core/src/`;
- `Engine`, `EngineCommand`, reset behavior, and read-only agent inspection in `crates/sim-core/src/lib.rs`;
- a focused headless integration scenario;
- layout and scheduler measurements in `Documentation/PERFORMANCE.md`.

### Deliverables

- Define opaque `AgentId`, compact position/activity records, and an engine-owned dense population store.
- Define explicit fallible population initialization after a fixed simulation rectangle is completely resident, with passable-position validation and deterministic ID allocation. Do not auto-spawn in `Engine::new`.
- Define simulation time, event identity, movement event payload, and a total equal-time ordering rule.
- Schedule at most the work due at the current simulation time; `Engine::tick` must not scan every agent. Bound due-event draining and zero-delay rescheduling so one timestamp cannot create an infinite reaction chain.
- Execute one cardinal movement through `World::traversal_step`, scheduling completion from stable integer traversal cost.
- Report success, blocked terrain, unloaded terrain, outside-world, invalid/non-cardinal input, missing/dead agent, and stale event through typed outcomes.
- Expose bounded read-only agent state for headless tools and later presentation without exposing mutable storage.
- Define reset semantics for IDs, agent state, scheduler state, and deterministic replay.

### Acceptance criteria

- Equal configuration, commands, spawn order, and ticks produce byte/equality-equivalent agent views and snapshots.
- Reversing internal insertion order cannot change equal-time event application order.
- Pausing prevents simulation events from executing; reset reproduces the initial state and ID sequence.
- A valid move changes exactly one agent position at its scheduled completion time.
- Blocked, unloaded, outside-world, invalid, stale, and duplicate events do not partially mutate state.
- Agent state never causes terrain materialization.
- Initialization rejects incomplete residency, insufficient valid spawn cells, and duplicate/invalid requested positions without partially creating a population.
- Type sizes, alignments, retained population capacity, event bytes, scheduler insertion/reschedule/due-extraction time, and allocation counts are recorded for 20, 100, and at least one larger synthetic population.
- The complete validation gate passes.

### Implemented result

`sim-core::agent` now owns dense zero-based `AgentId` slots, six-byte compact position/activity records, parallel movement generations, atomic resident-area initialization, and bounded read-only views/outcomes. `sim-core::scheduler` owns 32-byte heap events with total `(time, class, agent, sequence)` order, checked sequence/time overflow, lazy stale-event invalidation, bounded stale retention, and a 4,096-event due-drain ceiling. `Engine` exposes explicit initialization and typed movement scheduling, keeps the viewer population-empty, and clears population/scheduler/IDs on reset while preserving resident terrain. `sim-headless` initializes 20 agents by default and executes one deterministic physical step per movable agent.

Unit regressions cover initialization atomicity, invalid/duplicate/insufficient spawns, compact layouts, exact completion time, pause/reset, blocked and invalid requests, dead/missing IDs, active/world bounds, stale duplicate events, equal-time insertion reversal, overflow, and due backlog. The public-only `physical_agent_slice0` scenario replays 20 agents across forward/reverse command insertion. The ignored release harness records 20, 100, and 10,000-agent capacities, retained bytes, structural/growth allocation counts, and insertion/reschedule/extraction timings in `PERFORMANCE.md`. D-037 records the durable layout, time, ordering, cancellation, reset, and temporary heap decisions. The complete runtime validation gate passed after the final implementation and documentation synchronization.

## Slice 1: Spatial occupancy, perception, and local routes

Status: **Implemented** on 2026-07-16.

### Objective

Let agents discover nearby physical facts and navigate short distances without all-pairs checks, duplicate occupancy, whole-world scans, or a complete path vector stored per agent.

### Decision checkpoints

- Choose chunk/tile occupancy ownership and deterministic transfer order.
- Define whether multiple agents may share a cell in Phase 2; if not, define equal-time collision arbitration.
- Select a bounded short-route algorithm and hard search budget.
- Decide the compact route representation: shared route, bounded waypoint record, or recomputable destination/progress state.

### Deliverables

- Maintain a simulation-owned spatial index from world/chunk locations to present agent IDs.
- Apply position and index changes atomically at deterministic command boundaries.
- Provide bounded radius/rectangle queries returning currently implemented agents, drinkable water, immutable resources, and traversable cells in canonical order. Extend the same objective boundary with structures when Slice 6 introduces their authoritative store; do not invent structure identity or state in Slice 1.
- Add short deterministic routing over cardinal `World::traversal_step` results with explicit no-path, budget-exhausted, unloaded, and invalid-target outcomes.
- Schedule route or waypoint progress from traversal costs; do not create a movement event every engine tick.
- Invalidate only routes affected by changed dynamic occupancy or later structures, not by irrelevant world revisions.
- Keep perception as objective nearby physical input. Do not add beliefs, memory, attention, or private interpretation.

### Acceptance criteria

- Each living non-traveling agent belongs to exactly one spatial bucket and one world position.
- Equal-time movement contention has one documented deterministic winner and leaves every index consistent.
- Query and route output is stable across insertion order and map/cache state.
- Normal perception and routing inspect bounded cells/buckets only.
- Route execution cannot walk through water, excessive slope, blocking features, agents under the chosen collision rule, or unloaded terrain.
- Focused tests cover signed coordinates, chunk boundaries, route ties, no-path, search-budget exhaustion, dynamic occupancy, and atomic transfer.
- Query cost, route-search expansions, temporary allocations, and spatial-index bytes per agent are measured.

### Implemented result

`sim-core::spatial` owns one-agent-per-cell occupancy in a sparse compact signed chunk map. Each bucket keeps sorted eight-byte `(local cell, AgentId)` entries; moving agents occupy their source until completion, and checked source-to-target transfer is atomic. The existing total scheduler order makes the lower `AgentId` the deterministic equal-time winner for an initially empty target, independent of request insertion order. Already occupied targets are rejected without reserving empty targets.

`Engine::perceive_physical` performs active-area-clipped radius queries through 31 cells and returns agents, drinkable water, immutable resources, and traversable cells in global row-major order. `Engine::request_route` uses deterministic traversal-cost Dijkstra with a caller budget capped at 4,096 expansions and distinct invalid, occupied, no-path, budget-exhausted, unloaded, and terrain-blocked outcomes. Optional per-agent route state stores only a compact destination and budget; a reusable engine planner recomputes the next step after each positive-cost completion, so routes carry no path vector and cause no per-tick population scan. Occupancy conflicts trigger bounded replanning, while irrelevant world revision changes have no route state to invalidate.

Unit and public integration regressions cover compact layouts, signed `-65/-64/-1/0/63/64` bucket boundaries, atomic failed transfer, request-order-independent equal-time contention, row-major bounded perception, budget exhaustion, occupied-corridor no-path, and scheduled arrival. `sim-headless` now uses perception and routes for its 20-agent smoke. The ignored release harness records 20/100/10,000-agent spatial capacity and perception work plus a 75-expansion reusable route search. D-038 records the durable ownership, collision, query, route, and structure-deferral decisions.

## Slice 2: Analytical physical needs

Status: **Planned**. Depends on Slices 0-1.

### Objective

Add hunger, thirst, rest, and safety/exposure as compact analytically evaluated state that schedules threshold work instead of being incremented for every agent every tick.

### Decision checkpoints

- Choose fixed-point ranges, rate units, saturation policy, and threshold semantics.
- Separate hot next-event state from colder need details if measurement supports it.
- Define threshold priority when several needs become due at the same time.
- Define provisional age/body modifiers only if Phase 2 behavior requires them; reproduction and development remain deferred.

### Deliverables

- Add a compact `NeedState` representation storing value/rate at a reference time.
- Derive current values with checked/saturating integer arithmetic.
- Predict and schedule the next threshold for each relevant need.
- Reschedule only when a rate, threshold, or reference value changes; stale threshold events must be harmless.
- Add minimal activity-dependent rates for idle, moving, gathering, building, and sleeping states.
- Represent exposure/safety as a physical input affected later by climate, sleep location, and shelter, without implementing fear, emotion, or social safety.
- Expose read-only current needs and next threshold for debugging.

### Acceptance criteria

- Need interpolation, saturation, threshold prediction, rescheduling, and equal-time priority have exact tests.
- Large time jumps produce the same result as reaching the same event time through smaller engine steps.
- Sleeping or idle agents do not require routine per-tick need updates.
- Paused time changes no need value; reset and replay reproduce all thresholds.
- No floating-point value enters authoritative need state or event ordering unless a separately recorded determinism decision justifies it.
- Need-state bytes per agent, threshold-event bytes, reschedule rate, and due-event throughput are recorded.

## Slice 3: Deterministic physical action policy

Status: **Planned**. Depends on Slices 0-2.

### Objective

Connect needs, perception, routes, and activities with a deliberately small deterministic policy so agents can pursue physical survival without introducing the later cognition architecture.

### Deliverables

- Define a compact physical goal/activity state for seeking water, seeking food, gathering a material, eating, drinking, sleeping, seeking shelter, building, waiting, and incapacitation.
- Rank urgent physical actions from current need thresholds and immediately perceived options using integer scores and explicit tie-breakers.
- Use deterministic keyed variation only where identical choices need stable diversification.
- Separate decision events from movement/action-completion events.
- Define interruption, commitment, retry/backoff, and no-valid-action behavior without unbounded same-time reaction chains.
- Record the reason for each selected action and each failure in a compact diagnostic event or counter suitable for headless reports.

### Acceptance criteria

- The same perceived facts and agent state always select the same action.
- Changing an irrelevant fact cannot reorder equal-scored candidates accidentally.
- Unreachable, unloaded, depleted, occupied, or stale targets cause bounded reconsideration rather than tight loops.
- One agent cannot schedule multiple conflicting physical commitments.
- This module reads objective physical state only and exposes no belief, memory, relationship, personality, or language type.
- Scenario tests make every supported activity and failure reason reachable.

## Slice 4: Water, gathering, inventory, and consumption

Status: **Planned**. Depends on Slices 0-3.

### Objective

Let agents satisfy thirst and hunger, gather food/wood/stone, carry compact resources, and deplete generated resource capacity without mutating generated features.

### Decision checkpoints

- Choose compact inventory units, per-agent capacity, and overflow behavior.
- Define gather duration/yield and eating/drinking effects in the same integer time/need units.
- Decide whether depleted generated resources remain unavailable permanently for Phase 2 or use a separately scheduled regrowth rule. Permanent depletion is the smaller default.

### Deliverables

- Add a sparse simulation-owned resource-delta store keyed by stable generated feature identity.
- Keep immutable `BaseResource` capacity in `World`; store only changed remaining quantity/removal state.
- Add compact carried food, wood, and stone amounts without per-item heap allocation.
- Add scheduled gather completion with target revalidation and deterministic contention.
- Add drink actions adjacent to or at the supported fresh-water access position without storing water quantity.
- Respect the current traversal contract: all water blocks walking, so ordinary drinking occurs from a revalidated adjacent passable land position unless a later recorded movement decision adds another access mode.
- Add eating from carried food and explicit failure when no edible amount remains.
- Make resource reads compose generated base plus sparse delta through a simulation-owned API.
- Preserve atomicity: movement, gathering, inventory transfer, depletion, and need changes apply at defined event boundaries.

### Acceptance criteria

- Gathering cannot create resources, reduce a feature below zero, or let two equal-time gatherers consume the same final units.
- Depleting one feature creates one bounded sparse delta and changes no generated `Feature` or `TerrainCell`.
- Unmodified resources require no mutable record.
- Drinking rejects ocean water, unloaded terrain, and stale/nonadjacent access.
- Eating and drinking change only the intended need/reference values and schedule correct next thresholds.
- Inventory capacity and overflow are explicit and tested.
- Delta bytes per modified feature, inventory bytes per agent, gather throughput, and allocation behavior are recorded.

## Slice 5: Rest and sleep

Status: **Planned**. Depends on Slices 0-4.

### Objective

Add scheduled sleep/wake behavior that restores rest, remains interruptible by physical conditions, and does not repeatedly update sleeping agents.

### Deliverables

- Add sleep intent, transition, scheduled wake, and interrupted-wake events.
- Derive sleep recovery analytically from start time, location quality, and later shelter use.
- Define valid sleep locations and explicit rejection for water, blocked, occupied, unloaded, or unsafe positions.
- Apply reduced hunger/thirst rates and appropriate exposure while sleeping.
- Ensure urgent physical thresholds can interrupt sleep through bounded rescheduling.
- Expose sleep start, planned wake, quality, and interruption reason to headless diagnostics.

### Acceptance criteria

- A sleeping agent has one scheduled wake unless an earlier valid interruption supersedes it.
- Repeated stale wake/threshold events cannot wake twice or duplicate work.
- Analytical recovery matches exact boundary cases and is deterministic across tick batching.
- Sleep cannot bypass hunger, thirst, exposure, or death thresholds.
- Sleep event volume is independent of render frames and does not require per-tick updates.

## Slice 6: Minimal shelter

Status: **Planned**. Depends on Slices 0-5.

### Objective

Let agents gather materials, choose a valid nearby site, construct a minimal shelter, and use it for safer/restorative sleep without introducing settlements, ownership economies, or terrain mutation.

### Decision checkpoints

- Define one provisional shelter recipe and build duration from measured Phase 2 resource availability.
- Choose sparse structure identity, position/footprint, occupancy, and lifecycle representation.
- Decide whether a shelter is personal, shareable, or unowned in Phase 2. Social ownership remains deferred.

### Deliverables

- Add a simulation-owned sparse structure store separate from generated features and terrain.
- Validate dry, resident, traversable, unoccupied construction sites and bounded access.
- Reserve/consume required inventory atomically at construction start or completion according to a recorded rule.
- Schedule construction progress/completion without per-tick work.
- Make completed shelter affect sleep quality and physical exposure only.
- Include structures in bounded physical perception, routing obstacles/access, snapshots, and reset.

### Acceptance criteria

- Equal-time builders cannot create overlapping structures or double-spend materials.
- Failed/cancelled construction follows an explicit refund or loss rule.
- Structures do not enter generated world records and cannot be created on unloaded or invalid terrain.
- Shelter benefit is measurable in the need/exposure model and cannot grant unrelated social or cognitive state.
- Structure record size, spatial-index bytes, and build-event cost are recorded.

## Slice 7: Health, safety, and simple death

Status: **Planned**. Depends on Slices 0-6.

### Objective

Turn prolonged physical failure into explicit health consequences and terminal death while preserving stable identity and understandable causality.

### Decision checkpoints

- Define the minimum health representation and the exact lethal thresholds/durations for dehydration, starvation, exhaustion, and exposure.
- Define terminal record retention and whether dead-agent hot state is compacted while stable identity remains resolvable.

### Deliverables

- Add compact health/alive state and scheduled deterioration or threshold consequences.
- Connect unmet thirst, hunger, rest, and exposure to health through explicit integer rules.
- Add incapacitation where necessary to avoid agents walking/building through terminal physical states.
- Add one terminal death transition with cause, time, and position.
- Cancel or invalidate ordinary future events for dead agents while retaining inspectable historical identity.
- Keep death physical only; inheritance, grief, relationships, burial, and historical archival belong to later phases.

### Acceptance criteria

- Dead agents never execute normal movement, perception, gathering, sleep, building, or decision events.
- Death is applied once and has a stable cause even when multiple lethal thresholds share a timestamp.
- Spatial occupancy and active-agent counts remain consistent after death.
- A reproduced run yields identical death ordering, causes, times, and positions.
- Headless reports distinguish survival, blocked progress, depletion, dehydration, starvation, exhaustion, exposure, and other implemented terminal causes.

## Slice 8: Phase 2 integrated survival proof

Status: **Planned**. Depends on Slices 0-7.

### Objective

Prove the complete physical loop with repeatable 20-100-agent headless scenarios, deterministic evidence, long-run stability, and measured storage/event costs.

### Deliverables

- Add canonical fixed-seed scenarios for at least 20 and 100 agents using only public engine/world contracts.
- Select deterministic spawn locations with bounded access to fresh water and initial resources; record the selection inputs rather than adding a universal settlement score to `World`.
- Run long enough for movement, need thresholds, gathering, consumption, sleep, construction, depletion/contention, and at least one understandable survival or failure path.
- Emit a deterministic compact report containing initial conditions, final agent counts/states, action/failure/death counts, resource deltas, structures, scheduler totals, and a semantic state hash.
- Add same-seed replay, different-command divergence, tick-batching equivalence, and reset/replay integration tests.
- Add a bounded soak test that monitors event queue growth, stale-event ratio, retry chains, spatial-index consistency, resource-delta growth, and memory capacity.
- Record release measurements for agent bytes, event bytes, event throughput, route/perception work, scenario elapsed time, allocations, and peak working set.
- Update the roadmap so Phase 2 becomes completed and Phase 3 beliefs/relationships becomes the next active planning boundary only after all acceptance criteria pass.

### Acceptance criteria

- Canonical 20-agent and 100-agent scenarios finish with byte/equality-stable reports and hashes across repeated runs.
- Agents survive or die for reasons derivable from recorded physical state and events.
- No normal engine tick performs work proportional to the complete population when no event is due.
- Event timestamps never move backward; causal chains and retry depth remain bounded.
- No agent occupies two positions, no position violates the chosen occupancy rule, no resource is consumed twice, and no structure overlaps illegally.
- Population, event queue, indexes, inventories, deltas, and structures remain within recorded capacities during the soak.
- The full repository gate and the relevant release scenario/benchmark commands pass from a clean process.
- Living documentation exactly matches the implemented Phase 2 boundary and lists remaining limitations.

## Data ownership target

```text
sim-core::Engine
  AgentStore
    compact hot agent records
    optional colder physical state/indexes
  EventScheduler
    deterministic future events
    cancellation/stale-event validation
  SpatialIndex
    present agents
    dynamic structures
  ResourceDeltas
    only modified generated feature capacities
  StructureStore
    sparse constructed shelters
  World
    immutable generated terrain/features
    deterministic resident materialization cache

sim-headless
  configuration, deterministic scenario driving, reports, benchmarks

sim-viewer
  no authoritative Phase 2 state
  later read-only agent/structure presentation only
```

Exact module names may change during implementation, but ownership may not drift across these boundaries without a recorded architecture decision.

## Cross-cutting test matrix

| Invariant | Required evidence |
| --- | --- |
| Identity | Stable IDs, invalid/stale references, reset sequence, terminal identity |
| Time | No backward events, exact equal-time ordering, pause, large jumps, overflow |
| Determinism | Repeat, insertion-order variation, tick batching, reset/replay |
| Residency | Resident success, unloaded/outside typed failure, no implicit generation |
| Movement | Passable, water/slope/feature blocked, collision, signed/chunk boundaries |
| Spatial state | Atomic transfer, canonical query order, no duplicate occupancy |
| Needs | Interpolation, thresholds, saturation, activity rates, stale rescheduling |
| Resources | Base-plus-delta composition, contention, depletion, inventory conservation |
| Sleep/shelter | Valid sites, recovery, interruption, construction contention |
| Death | Single terminal transition, event invalidation, stable cause and report |
| Scale | 20/100 scenarios, synthetic larger storage/scheduler workload, bounded soak |
| Performance | Type sizes, retained capacities, allocations, throughput, peak working set |

Small exact fixtures should prove ordering, arithmetic, conservation, and ownership. Integrated scenarios should prove composition. Avoid giant golden dumps when a structural invariant and compact semantic hash are sufficient.

## Performance and storage checkpoints

Every slice that changes hot data or events must record, in release mode where timing matters:

- complete size/alignment of every hot agent, need, inventory, event, spatial-entry, delta, and structure record;
- logical bytes and retained capacity for 20, 100, and a larger synthetic population;
- scheduler insertion, cancellation/reschedule, and due-event extraction throughput;
- events executed, stale events discarded, and maximum same-time chain depth;
- spatial-query candidates, route expansions, and temporary allocations;
- sparse resource/structure bytes per modified feature or constructed shelter;
- scenario elapsed time, process peak working set, and allocation observations;
- before/after evidence when an implementation is called an optimization.

The first implementing slice must set explicit budgets for maximum hot agent-core size and event record size from measured candidates. Every narrowed ID, coordinate, need, counter, or quantity must document its valid range and overflow behavior. Later slices may revise budgets only with documented evidence. Source-line count is never a performance proxy.

## Documentation updates per slice

As each slice lands:

- update `CURRENT_IMPLEMENTATION.md` with executable behavior and remaining gaps;
- update `ARCHITECTURE.md` with ownership, event flow, residency, mutation, and read-only API contracts;
- append `ARCHITECTURE_DECISIONS.md` for time representation, ID/storage layout, scheduler, occupancy, need arithmetic, resource deltas, structures, or death retention choices;
- update `TESTING.md` with exact focused, scenario, soak, benchmark, and validation commands;
- update `PERFORMANCE.md` with layouts, capacities, allocations, timings, and budgets;
- update `ROADMAP.md` and this plan's status/implementation notes;
- update the root `README.md` only when user-facing run commands, configuration, controls, or output change.

Keep each fact canonical in its owning document and link to it elsewhere.

## Explicitly deferred

The following are outside Phase 2 unless an implemented physical-loop failure proves a direct dependency:

- beliefs, episodic memory, relationships, personality-driven cognition, emotions, and social needs;
- families, children, reproduction, aging stages, inheritance, and caregiving;
- communication, signals, language, teaching, misunderstanding, and culture;
- settlements, households, factions, economy, trade, occupations, laws, institutions, and warfare;
- farming, cooking, crafting trees, equipment, detailed buildings, fire simulation, and itemized inventories;
- combat, injuries, disease, pregnancy, healing, and advanced physiology;
- long-distance journeys, region transfers, roads, hierarchical world routes, crowds, and flow fields;
- dynamic ecology, regrowth beyond a deliberately minimal measured rule, seasons, weather, floods, and terrain modification;
- agent or structure rendering until the headless Phase 2 proof is correct;
- generator versioning, save/load, persistence, networking, and replay files. Their future boundaries must not be blocked, but they are not implementation work yet;
- parallel mutable simulation and million-agent scaling. Phase 2 should preserve a path to them without introducing unmeasured concurrency.

## Open decision gates

These questions must be resolved in their owning slice, not guessed ahead of evidence:

- exact simulation-time resolution and event scheduler representation;
- maximum hot agent-core and event sizes;
- generational stale-reference strategy;
- occupancy and equal-time collision policy;
- local route-search budget and compact route representation;
- fixed-point need scales, rates, thresholds, and physical time calibration;
- deterministic choice-stream keying;
- inventory capacity, gather yields, and depletion/regrowth policy;
- sleep validity and recovery rates;
- shelter recipe, footprint, sharing, and cancellation/refund behavior;
- health deterioration rates and terminal-cause precedence;
- canonical scenario duration, spawn count/roles, success thresholds, and benchmark limits.

Each decision must be justified by correctness, deterministic behavior, measured layout/cost, and the smallest requirements of the current slice.

## Definition of done

Phase 2 is complete only when:

- Slices 0-8 are marked **Implemented** with code, tests, documentation, and validation evidence;
- 20-100 agents execute through deterministic scheduled events rather than per-frame or full-population polling;
- movement, physical perception, needs, gathering, consumption, sleep, shelter, health, and simple death work together;
- generated terrain remains immutable and mutable physical state uses explicit simulation-owned stores;
- unavailable terrain, failed actions, depletion, and death have typed, understandable outcomes;
- deterministic reports prove repeatability and expose why agents survived or failed;
- compact layouts, retained capacities, allocations, event throughput, scenario timing, and soak behavior are measured;
- `InitialDocumentation/` passes its immutable checksum;
- the complete repository validation gate succeeds after the final documentation edit; and
- the next unresolved work belongs to Phase 3 beliefs and relationships rather than an unfinished physical-loop dependency.
