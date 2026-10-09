# Phase 3 Beliefs, Memory, and Relationships Implementation Plan

Last synchronized: 2026-07-17.

Status: **Planned**. This is the canonical Phase 3 execution plan. Phase 2 is the implemented physical foundation; Phase 3 implementation begins with Slice 0 and must remain in dependency order unless a reviewed decision updates this document.

## Purpose

Give each physical agent a bounded private account of what it has observed, what it currently believes, what it remembers, and which other agents have become personally meaningful. Use that private state to replace purely reactive survival wandering with compact event-driven goal selection while preserving deterministic simulation, sparse storage, and the distinction between objective truth and individual knowledge.

Phase 3 ends when 20-100 agents can:

- observe only physically available facts;
- retain uncertain and potentially stale beliefs about important places and agents;
- remember useful water, food, material, and shelter locations;
- conduct bounded survival-driven searches when an essential resource has never been observed;
- select and retain survival goals from needs, inventory, remembered opportunities, confidence, effort, and risk;
- forget or consolidate routine experience instead of growing memory without bound;
- form sparse familiarity and trust records only through relevant encounters;
- differ in belief and behavior despite sharing the same objective world;
- explain decisions through bounded causal diagnostics; and
- replay deterministically with measured cognitive storage and work.

This is not yet communication, language, family, society, ownership, economy, personality, emotion, or a general-purpose symbolic reasoning system. Phase 4 remains the first nonverbal communication phase.

## Phase 3 completion contract

The implemented phase must include:

- an explicit direct-observation boundary that never exposes private intent or unseen objective truth;
- a bounded attention and working-observation boundary that prevents local perception from becoming perfect cognition;
- compact typed belief records with confidence, provenance, acquisition time, and revalidation state;
- bounded personal landmark knowledge for water, resources, and shelters;
- compact deterministic search progress for essential unknown resources without world scans or global knowledge;
- one compact current cognitive goal/plan boundary above the Phase 2 physical action executor;
- deterministic plan retention, interruption, failure, and reconsideration;
- important episodic memories plus deterministic forgetting and consolidation;
- sparse per-agent relationships with familiarity and trust, without an all-pairs matrix;
- event-triggered cognition rather than per-tick mind scans;
- bounded read-only diagnostics explaining observation, belief update, memory change, goal selection, and relationship change;
- a bounded terminal cognitive summary that keeps long-run survival and death failures inspectable after transient diagnostics expire;
- a deterministic 20/100-agent headless proof with conflicting or stale beliefs, survival use of remembered landmarks, sparse relationships, bounded memory, and replay-stable reports.

The original 100-agent cognition-correctness gate remains authoritative. Phase 3 must prove believable bounded behavior before population scale increases or later communication and society systems begin.

## Current baseline

Phase 2 already provides the physical facts and execution boundaries Phase 3 must consume:

- stable dense `AgentId` identities and engine-owned physical state;
- bounded row-major `PhysicalPerception` containing nearby agents, drinkable water, available resources, structures, standable cells, and terrain-connected reachable cells;
- deterministic scheduled movement, bounded A* routing, occupancy arbitration, and typed route failures;
- analytical hunger, thirst, rest, and exposure values with scheduled thresholds;
- compact inventory, resource depletion, drinking, eating, gathering, sleep, shelter, health, incapacitation, and death;
- a single compact physical-policy commitment with deterministic diagnostics and interruption;
- viewer-only bounded exploration used when no local physical objective is available;
- equality-stable headless scenario reports, semantic hashes, scheduler/route/perception counters, and 600,000-tick 20/100-agent soaks;
- read-only hover inspection without presentation-owned simulation truth.

The current physical policy is deliberately reactive and locally omniscient only within current perception. It has no remembered landmark, belief confidence, long-term goal, relationship, personality, or social model. Its safe-water wait and deterministic exploration are transitional physical behavior, not the final cognition design.

## Non-negotiable constraints

Every slice must preserve these rules:

1. Objective world state, direct observations, memories, beliefs, communicated claims, and receiver interpretations are distinct domains. A belief may be stale or wrong without changing objective truth.
2. `sim-core` owns authoritative cognitive state and outcomes. `sim-viewer` and `sim-headless` receive bounded copied views and reports only.
3. No agent may read another agent's private goal, belief, memory, relationship value, or future intent through perception.
4. Cognition runs only on explicit triggers such as perception change, need threshold, arrival, action outcome, plan failure, memory due event, or later communication. `Engine::tick` must not scan every mind.
5. Normal cognition retrieves only relevant bounded records. It must not scan every belief, memory, relationship, agent, or world cell.
6. Variable cognitive state is sparse and pooled or otherwise measured. Do not place one `Vec`, map, heap allocation, or fixed 4 KiB buffer inside every agent.
7. Record identity, ordering, capacity, invalidation, compaction, and stale-handle behavior explicitly. Compaction must not make a live handle silently refer to another record.
8. Confidence, utility, decay, and learning use deterministic integer or fixed-point arithmetic with explicit saturation and tie ordering.
9. Random-looking variation uses keyed deterministic purpose domains. No mutable global cognition RNG may make worker or insertion order authoritative.
10. Beliefs about unloaded or distant terrain never cause synchronous generation. Revalidation uses resident physical queries and returns typed unavailable/stale outcomes.
11. Relationships remain sparse. Familiarity alone does not create a record for every visible stranger, and group priors must never become automatic personal knowledge.
12. Routine observations are discarded or consolidated. Memory growth must have hard budgets and observable pruning behavior.
13. Death stops ordinary cognition while preserving stable identity and any explicitly retained historical references without forcing eager archival or persistence.
14. Existing Phase 2 physical scenarios and reports remain valid dependencies. Phase 3 may add a new report/version but must not silently reinterpret Phase 2 evidence.
15. Each slice lands code, focused tests, integration evidence, measurements, living-documentation updates, and a successful complete repository validation run together.
16. `InitialDocumentation/` remains immutable.

## Cognitive execution protocol

An agent implementing one slice must:

1. Read `AGENTS.md`, `docs/STATUS.md`, `docs/ARCHITECTURE.md`, this complete plan, relevant decisions (`docs/DECISIONS.md`, `docs/archive/ARCHITECTURE_DECISIONS.md`), tests, and the predecessor slice's implemented result.
2. Read only the immutable design documents relevant to the active slice, especially agent data, cognition, beliefs/memory/relationships, event scheduling, storage budgets, scaling, testing, and guardrails.
3. Confirm every prerequisite slice is marked **Implemented**. If not, implement only the earliest incomplete prerequisite.
4. Keep objective facts, observations, private beliefs, memories, and relationship state separate in types and APIs.
5. Record data layout, capacity, retrieval, invalidation, event order, and failure semantics before making them public contracts.
6. Add adversarial tests for truth leakage, stale handles, nondeterministic ties, unbounded growth, and partial mutation.
7. Record durable decisions in `docs/DECISIONS.md`; do not settle open checkpoints only in code.
8. Update this plan's status and implemented result only after code and validation exist.
9. Review non-trivial Rust changes for correctness, determinism, and boundaries; resolve or disclose every finding.
10. Run the complete validation gate (`scripts/validate.sh` or `scripts/validate.ps1`) after the final code or documentation change.
11. Stop after the active slice and hand off the next slice unless the user explicitly requests multiple slices.

## Work sequence

| Order | Slice | Primary result |
| ---: | --- | --- |
| 0 | Cognitive storage and event foundation | Measured sparse storage, stable record handles, cognition triggers, and bounded views |
| 1 | Direct observations | Agents receive typed physically grounded observations without truth or intent leakage |
| 2 | Environmental beliefs and landmark memory | Private uncertain knowledge of water, resources, shelters, and locations |
| 3 | Utility goals and compact survival plans | Remembered opportunities guide foraging, return-to-water, shelter use, and construction |
| 4 | Episodic memory, forgetting, and consolidation | Important experience persists while routine detail remains bounded |
| 5 | Sparse familiarity and trust | Meaningful personal ties arise from observed encounters without an all-pairs matrix |
| 6 | Belief and relationship consequences | Outcomes revise confidence, plans, familiarity, and trust through explicit evidence |
| 7 | Phase 3 integrated cognition proof | Deterministic 20/100-agent belief divergence, survival, memory, and relationship evidence |

## Slice 0: Cognitive storage and event foundation

Status: **Planned**. This is the next implementation slice.

### Objective

Establish the smallest safe storage, identity, scheduling, and inspection boundary for sparse beliefs, memories, and relationships before any semantic record begins affecting behavior.

### Decision checkpoints

- Compare packed per-agent ranges, slab/size-class pools, and another compact candidate using representative empty, ordinary, and unusually complex agents.
- Decide whether records use stable generational handles, owner-plus-local keys, or pool offsets that are rewritten during deterministic compaction.
- Define how an agent locates its ranges without widening the six-byte hot physical record.
- Define capacity classes, reserve/growth policy, allocation failure, record limits, fragmentation measurement, and deterministic compaction order.
- Define cognition event classes and equal-time ordering relative to health, needs, wake, action completion, movement, and physical policy decisions.
- Decide whether Phase 3 adds one cognitive scheduler payload to the existing event record or a separate bounded queue without weakening total order.
- Define the read-only bounded view and diagnostic boundary without exposing mutable pool storage.

### Implementation area

- new responsibility-focused private cognitive storage/event modules under `crates/sim-core/src/`;
- narrow integration through `Engine`, scheduler ownership, reset, death, and snapshot/report boundaries;
- layout, allocation, compaction, insertion, lookup, and due-extraction measurements recorded in this plan's implemented result (prior baselines: `docs/archive/PERFORMANCE.md`);
- focused public integration coverage under `crates/sim-core/tests/`.

### Deliverables

- Define opaque typed record handles or an equivalent validated reference domain for belief, memory, and relationship records.
- Add an engine-owned sparse pool/index prototype with zero variable records for an ordinary new agent.
- Support deterministic insertion, lookup, removal, range growth, and compaction without cross-owner aliasing.
- Add explicit capacity and allocation failures with atomic no-partial-publication behavior.
- Define bounded cognition trigger/event identity and total equal-time ordering.
- Ensure reset clears cognitive storage, events, IDs/generations, diagnostics, and capacities according to an explicit replay contract.
- Expose only bounded copied layout/capacity diagnostics; semantic belief or relationship APIs remain deferred to later slices.

### Acceptance criteria

- Empty agents retain no per-agent heap allocation and no unused fixed cognitive payload.
- A live record reference never aliases another record after removal, reuse, growth, or compaction.
- Reversing insertion order cannot change canonical record iteration or equal-time event application.
- Allocation/capacity/sequence/time failures publish neither a partial record nor a partial event.
- Dead or missing owners cannot create new ordinary cognitive state.
- Synthetic 20, 100, 10,000, and at least one larger pool workload records logical bytes, allocator/growth evidence, fragmentation, compaction cost, lookup cost, and event cost.
- The design remains compatible with the below-4-KiB average long-term personal-state budget without claiming the final ten-million-agent target is achieved.
- Existing Phase 2 semantic reports remain stable unless an explicitly versioned encoding change is recorded.
- The complete validation gate passes.

## Slice 1: Direct observations

Status: **Planned**. Depends on Slice 0.

### Objective

Convert currently perceptible physical facts and action outcomes into explicit bounded direct observations without giving an observer unseen truth, another agent's private intent, or permanent memory automatically.

### Decision checkpoints

- Define the first compact observation kinds: agent presence/activity, drinkable-water access, resource kind/availability, shelter state/access, movement/action outcome, and objective becoming unavailable.
- Separate an ephemeral observation payload from stored belief or episodic-memory records.
- Define bounded attention/salience selection from eligible physical facts and a short working-observation lifetime across one cognition decision without creating permanent memory automatically.
- Define observer eligibility through position, perception radius, terrain connectivity, visibility assumptions, and event timing.
- Decide which observations are sampled during an existing cognition wake and which action outcomes notify nearby eligible observers.
- Define canonical observation ordering, duplicate suppression, per-trigger count limits, and overflow behavior.
- Decide how confidence for direct perception is initialized without pretending every observation is perfect or permanent.

### Deliverables

- Add private typed `Observation` data with observer, subject/fact, position, time, source, and bounded confidence/evidence fields.
- Add a bounded working-observation set with explicit replacement, expiry, interruption, and overflow behavior.
- Derive spatial observations only from resident authoritative physical queries and current public/private engine facts allowed to the observer.
- Prevent `PhysicalGoal`, private policy reason, inventory, health internals, future events, beliefs, and memories from entering another agent's direct observation unless a later observable signal explicitly represents them.
- Emit bounded observation diagnostics showing what was observed and why an observer was eligible.
- Add a read-only developer/headless observation view capped independently from authoritative storage.
- Schedule at most one bounded cognition reconsideration for a coalesced same-time observation set.

### Acceptance criteria

- Two agents with different positions or blocked perception can receive different observation sets for the same objective world.
- An observer can see another agent's physical activity or outcome but cannot read its selected private goal or intention.
- Unloaded and outside-world facts yield typed absence/failure rather than knowledge or terrain generation.
- Equal observations are emitted in canonical fact/position/agent order independent of map, event insertion, and worker completion order.
- Observation bursts coalesce within a hard per-trigger budget and cannot create a same-time cognition loop.
- Task relevance and salience choose among over-budget eligible facts deterministically; being locally perceptible does not guarantee attention or retention.
- Expired or replaced working observations cannot influence a later decision unless an explicit belief, episode, or summary retained their consequence.
- Observing alone creates no permanent belief, memory, or relationship record before the owning later slice.
- The complete validation gate passes.

## Slice 2: Environmental beliefs and landmark memory

Status: **Planned**. Depends on Slices 0-1.

### Objective

Let agents retain private uncertain knowledge of important environmental opportunities while allowing that knowledge to become stale, contradicted, forgotten, or revalidated.

### Decision checkpoints

- Define compact proposition domains for known water access, resource opportunity, shelter access/state, and last-known agent location without creating a generic unbounded symbolic language.
- Define confidence range, direct-perception evidence, last-confirmed time, stale/contradicted flags, and deterministic update arithmetic.
- Decide whether durable landmarks and fast-changing resource quantities use separate record layouts or one tagged layout proven compact.
- Define per-kind and total belief limits, replacement order, duplicate merging, decay scheduling, and forgotten-record invalidation.
- Define when a precise cell belief should consolidate into a coarser location summary, if at all.
- Define revalidation when the remembered location is resident, unloaded, depleted, blocked, or occupied.

### Deliverables

- Convert salient direct observations into private environmental belief records owned by the observing agent.
- Merge repeated evidence and preserve newer contradiction rather than duplicating the same proposition indefinitely.
- Retain known water and completed-shelter access longer than depleted or mobile facts.
- Mark or reduce confidence in stale resource and last-known-agent locations over deterministic simulated time.
- Revalidate a remembered target through authoritative resident queries only when a plan or perception makes it relevant.
- Expose bounded per-agent belief views containing proposition, confidence, source, acquisition/confirmation time, and current validity state.
- Record belief-created, reinforced, contradicted, revalidated, decayed, and forgotten diagnostics.

### Acceptance criteria

- Agents that observed different areas hold different beliefs about the same world.
- A known water location persists outside current perception without becoming global shared knowledge.
- Depleting or replacing a remembered resource does not mutate the belief magically; later observation or revalidation updates it explicitly.
- Repeated identical observations merge into bounded state with saturated deterministic confidence.
- Belief decay and replacement use total stable order and never depend on wall time or per-tick scans.
- A forgotten or replaced handle cannot affect later retrieval or a plan accidentally.
- Per-agent and population belief growth remain within recorded hard limits for long repeated-observation tests.
- The complete validation gate passes.

## Slice 3: Utility goals and compact survival plans

Status: **Planned**. Depends on Slices 0-2.

### Objective

Use current physical needs, inventory, direct perception, and remembered environmental beliefs to select and retain understandable survival goals instead of reacting only to the nearest currently visible fact.

### Decision checkpoints

- Define a compact cognitive goal domain above physical commitments: secure water, forage food, collect shelter material, search for an unknown essential resource, establish shelter, return to water, return to shelter, recover, and wait safely.
- Define deterministic integer utility terms for urgency, expected benefit, carried reserve, confidence, distance/effort, exposure risk, opportunity cost, and plan-switch cost.
- Define plan retention and hysteresis so agents do not oscillate between similarly scored goals.
- Define interruption precedence for health/need emergencies, contradiction, target loss, route failure, and stronger goals.
- Define how remembered targets outside local perception are routed through bounded waypoints without whole-world A* or synchronous generation.
- Define compact deterministic sector/waypoint search progress for an essential resource with no known target, including effort/risk budgets, searched-direction advancement, interruption, abandonment, and later resumption without retaining a per-cell explored map.
- Decide the minimum remembered home/survival-anchor representation and when it may change.
- Define shelter-site requirements from known water, food/material access, standability, and local reachable area without adding a universal score to `World`.

### Deliverables

- Add one compact current cognitive goal/plan record outside the hot physical agent record.
- Generate only relevant goal candidates from changed needs, inventory, observations, active plan, and bounded retrieved beliefs.
- Score candidates with fixed-point/integer arithmetic and explicit tie order.
- Translate the chosen plan into existing Phase 2 route/action/sleep/build commitments rather than duplicating physical execution.
- Remember water as a return target instead of treating its access cell as a permanent unconditional wait.
- Start bounded survival-driven search when no viable remembered or perceived target can satisfy an essential need; discovery must flow through ordinary direct observation before it becomes a landmark belief.
- Permit bounded foraging excursions only when carried reserves, thirst projection, target confidence, and return cost keep the plan viable.
- Prefer remembered completed shelter for rest/exposure and avoid constructing another shelter while a viable known one exists.
- Require a new shelter site to be supported by explicit remembered/perceived survival inputs, not simply the first adjacent empty cell after acquiring eight wood.
- Emit bounded goal-candidate, utility-component, selection, retention, interruption, and failure diagnostics.

### Acceptance criteria

- An agent that knows water and food can leave water to forage and return before severe thirst under the deterministic scenario assumptions.
- A water-anchored agent with no timber in initial perception can conduct a bounded material search, discover timber several storage chunks away through direct observation, remember it, collect eight wood, return to a viable site, and build/use shelter before exhaustion under the focused deterministic scenario assumptions.
- Search progress remains compact and deterministic across interruption, batching, and replay; an exhausted search budget or unavailable terrain produces a typed inspectable replanning cause rather than blind `Wait`/`Explore` cycling.
- An agent with carried food does not remain permanently idle at water when a higher-value shelter or reserve plan is viable.
- An agent prefers a viable remembered shelter over building a duplicate outside its current perception.
- Shelter placement is reproducible and justified by recorded water/resource/reachability inputs.
- Equal relevant state produces equal candidate scores and plan choice; irrelevant beliefs cannot reorder a decision.
- Plan hysteresis prevents alternating goals without suppressing urgent physical interruption.
- Stale, contradicted, unloaded, occupied, or unreachable remembered targets cause typed revalidation/replanning rather than tight retry loops.
- No decision scans the complete belief pool or active world.
- Focused long-run cases fail with an inspectable physical or knowledge cause rather than unexplained `Wait`/`Explore` cycling.
- The complete validation gate passes.

## Slice 4: Episodic memory, forgetting, and consolidation

Status: **Planned**. Depends on Slices 0-3.

### Objective

Preserve a small number of personally important experiences while turning repeated routine events into compact summaries or forgetting them entirely.

### Decision checkpoints

- Define compact initial episode kinds relevant to Phase 3: landmark discovery, plan success/failure, depletion, shelter completion/use, severe need consequence, injury/incapacitation witness, and meaningful encounter.
- Define importance from novelty, consequence, contradiction, goal relevance, repetition, and optional future emotion hooks without implementing emotions now.
- Define working observation lifetime versus episodic retention.
- Define per-agent episode limits, importance replacement order, scheduled decay, and consolidation thresholds.
- Define summary records for repeated successes/failures without retaining every source event.
- Define whether dead agents retain a bounded historical memory summary in memory or defer archival to persistence.

### Deliverables

- Create episodic records only for events that cross explicit importance rules.
- Consolidate repeated routine outcomes into bounded counters/summaries with last/first occurrence and confidence.
- Schedule forgetting or review events analytically rather than scanning all memories each tick.
- Remove forgotten details safely while preserving any explicit consolidated belief/relationship consequence.
- Expose bounded memory views and diagnostics for retained, consolidated, decayed, replaced, and forgotten records.
- Add instrumentation for records created per observation/action, retention distribution, consolidation ratio, pruning, fragmentation, and bytes per agent.

### Acceptance criteria

- Thousands of routine successful drinks or gathers do not create thousands of permanent episodes.
- A novel severe failure can outlive routine successes under explicit deterministic importance ordering.
- Consolidation produces the same summary regardless of allowed event batch partitioning.
- Forgotten detail no longer affects retrieval unless an explicit surviving belief or summary does.
- Capacity pressure removes the canonical lowest-value record without corrupting live handles or another agent's range.
- Long-run 20/100-agent tests demonstrate bounded retained memory and scheduled work.
- The complete validation gate passes.

## Slice 5: Sparse familiarity and trust

Status: **Planned**. Depends on Slices 0-4.

### Objective

Represent only meaningful personal ties and update familiarity/trust from concrete observed encounters without creating an all-pairs relationship matrix or treating group proximity as friendship.

### Decision checkpoints

- Define the compact initial relationship fields: other `AgentId`, familiarity, trust, confidence/evidence, last interaction, and flags required for family/group expansion later.
- Define record-creation thresholds so briefly seeing a stranger does not always allocate a permanent tie.
- Define deterministic evidence for familiarity and trust changes from currently implemented physical encounters and outcomes.
- Audit which Phase 2 physical outcomes are honestly attributable evidence about another agent. If no existing interaction supports a trust change, keep trust neutral for that case rather than deriving intent from co-presence, ordinary contention, or hidden policy state.
- Define neutral stranger priors separately from personal records.
- Define relationship decay, consolidation, capacity, replacement, and behavior after the other agent dies.
- Define stable iteration and lookup without storing both directions unless each agent independently has evidence.

### Deliverables

- Add sparse directed relationship records; A's view of B does not imply B has a record for A.
- Accumulate bounded encounter evidence before promoting a stranger into a retained personal relationship.
- Increase familiarity from repeated salient co-presence or direct interaction, not mere population membership.
- Change trust only from explicit observed evidence with a documented positive/negative interpretation.
- Include at least one concrete, objectively observable Phase 3-compatible interaction whose attributable outcome can change trust, or narrow the implemented behavioral field to familiarity until such evidence exists.
- Preserve uncertainty and asymmetric relationships.
- Expose bounded relationship views and evidence diagnostics without exposing them to the related agent.
- Record relationship count distribution, lookup/update cost, bytes, fragmentation, pruning, and long-run growth.

### Acceptance criteria

- A population of 100 agents with sparse encounters retains far fewer than 9,900 directed pair records.
- A can trust B differently from B's trust in A.
- An unobserved event cannot alter a relationship magically.
- Repeated irrelevant proximity does not cause unbounded familiarity records.
- Mere co-presence, simultaneous resource use, or losing deterministic occupancy contention cannot change trust without an explicitly observed attributable outcome.
- At least one focused case produces an asymmetric relationship from concrete evidence rather than scenario injection or private-intent leakage.
- Equal encounter evidence produces equal fixed-point updates and replacement order.
- Death or record removal leaves no dangling reference that can alias a later agent or relationship.
- The complete validation gate passes.

## Slice 6: Belief and relationship consequences

Status: **Planned**. Depends on Slices 0-5.

### Objective

Close the Phase 3 learning loop so observed consequences revise beliefs, future plans, familiarity, and trust without direct intent transfer or communication.

### Decision checkpoints

- Define which completed/failed physical plans provide evidence about locations, resources, shelters, and other agents.
- Define causal attribution limits: an observer may associate an outcome with an observed actor or place but cannot know hidden motivation.
- Define how conflicting evidence updates confidence and whether a low-confidence alternative belief can coexist.
- Define how trust/familiarity modify utility only where another agent is a relevant physical factor; do not invent communication compliance early.
- Define bounded retrieval of relevant episodes and relationships during reconsideration.
- Define diagnostics that distinguish observation, inference, belief update, relationship update, and final goal effect.

### Deliverables

- Feed plan success/failure and direct observed outcomes into explicit belief evidence updates.
- Allow contradicted resource/shelter beliefs to reduce confidence, become stale, or be replaced.
- Allow concrete observed encounters to affect sparse familiarity/trust through typed evidence.
- Use relevant confidence and prior outcome summaries in future target utility without loading unrelated memories.
- Preserve multiple uncertain alternatives where the proposition domain requires it and the fixed capacity allows it.
- Emit one bounded causal chain linking trigger, retrieved records, updates, candidate utilities, and chosen plan.

### Acceptance criteria

- Two agents can observe different evidence and retain different confidence about one location or agent.
- Failed revalidation changes future selection through an explicit belief update rather than a hidden blacklist.
- Another agent's private intention never appears in causal diagnostics or relationship evidence.
- Relationship influence is bounded, asymmetric, and relevant to the selected goal.
- At least one later relevant physical choice changes for an inspectable relationship-evidence reason; if Phase 3 exposes no honest relationship-dependent choice, trust remains diagnostic state and the behavioral consequence is explicitly deferred rather than fabricated.
- Causal diagnostics reproduce exactly across replay and tick batching.
- Retrieval work remains bounded by relevant record limits, not total population cognitive state.
- The complete validation gate passes.

## Slice 7: Phase 3 integrated cognition proof

Status: **Planned**. Depends on Slices 0-6.

### Objective

Prove that private observations, beliefs, landmark memory, compact plans, episodic consolidation, and sparse relationships work together deterministically for 20-100 physical agents with understandable survival and knowledge-driven divergence.

### Decision checkpoints

- Define one canonical Phase 3 scenario plus focused adversarial scenarios without changing generated terrain solely to force desired cognition outcomes.
- Define cohorts that receive different observations or encounter histories while sharing objective world state.
- Decide whether to add a new report type or increment an explicitly versioned semantic format without invalidating the Phase 2 report contract.
- Define authoritative semantic hashing for cognitive records, handles/generations, plans, diagnostics summaries, and relationships without hashing capacity addresses or debug text.
- Define soak sampling for pool invariants, record counts, event backlog, cognition work, memory consolidation, relationship sparsity, plan churn, and survival causes.
- Define the Phase 3 exit threshold for understandable behavior rather than requiring universal survival.
- Define a compact terminal cognitive summary captured at incapacitation or death with the last retained goal/target, relevant known or missing landmarks, last plan interruption/failure, and the knowledge/action reason the agent could not recover.

### Deliverables

- Add reusable 20- and 100-agent headless scenarios with explicit observation/encounter inputs and no presentation dependence.
- Demonstrate at least two agents holding different beliefs about the same objective fact for understandable evidence reasons.
- Demonstrate remembered water/shelter affecting an excursion, return, sleep, or construction decision outside current perception.
- Add the water-anchor/distant-timber survival regression: no timber is initially perceived, timber exists several storage chunks away, and bounded search must discover, remember, gather, return, build, and sleep without global knowledge.
- Demonstrate stale belief revalidation, plan change, important episode retention, routine consolidation/forgetting, and asymmetric sparse relationship state.
- Report physical outcomes together with observation, belief, memory, plan, and relationship counters and bounded causal samples.
- Hash authoritative cognitive state through explicit stable encodings.
- Repeat complete release scenarios from clean instances and compare reports/hashes.
- Sample pool ownership, handle validity, capacity limits, event due backlog, relationship sparsity, and memory budgets through long runs.
- Retain one bounded terminal cognitive summary per dead agent or an equivalently queryable bounded terminal record; do not require an unbounded event history to explain a specific death.

### Acceptance criteria

- Same seed, configuration, explicit inputs, and tick budget produce equality-identical reports and semantic hashes.
- Reversing allowed insertion/batching order cannot change authoritative cognitive outcomes.
- At least one different-belief case, remembered-landmark plan, stale-belief correction, consolidation, forgetting, and asymmetric relationship is causally visible.
- Agents forage and return from known water under the supported scenario instead of permanently camping or wandering blindly.
- Agents can search for at least one initially unknown essential resource under the supported focused scenario instead of requiring every survival target to be pre-observed.
- Existing viable shelters are used and duplicate construction is avoided when remembered knowledge makes that choice rational.
- Survival and death remain understandable from physical state plus the agent's private knowledge and selected plan; universal survival is not required.
- Every canonical-scenario death exposes a bounded terminal explanation distinguishing unknown, stale, unreachable, unavailable, abandoned, interrupted, and lower-utility survival opportunities where applicable.
- Memory/relationship counts, pool fragmentation, plan switching, cognition events, stale work, and queue growth remain bounded with zero invariant violations.
- Phase 2 reports and tests continue to pass.
- The complete validation gate and repeated ignored release scenarios pass.

## Cross-slice testing matrix

| Area | Required coverage |
| --- | --- |
| Ownership | Pool record belongs to exactly one agent; no cross-owner alias after removal/compaction |
| Determinism | Equal inputs, reversed insertion, batching, reset/replay, stable tie order |
| Truth separation | Unseen objective fact and another agent's private intent never enter observation/belief |
| Observation | Eligibility, obstruction/residency, duplicate coalescing, bounded bursts |
| Attention/working state | Salience, task relevance, replacement, expiry, interruption, no automatic permanent retention |
| Belief | Reinforcement, contradiction, staleness, revalidation, confidence saturation, forgetting |
| Planning | Candidate relevance, integer utility, hysteresis, interruption, stale target, bounded retrieval |
| Unknown-resource search | Water-anchor departure, compact search progress, discovery through observation, return budget, interruption/resumption, typed abandonment |
| Survival bridge | Water excursion/return, reserve foraging, shelter reuse, justified construction |
| Memory | Importance, consolidation, decay, replacement, no effect after forgetting |
| Relationship | Directed asymmetry, sparse creation, evidence update, decay, death/stale identity |
| Scheduling | Event class order, positive-delay rules, stale suppression, due-drain bound |
| Failure atomicity | Capacity, allocation, sequence/time overflow, missing/dead owner, invalid handle |
| Scale | 20/100 correctness and soaks; synthetic larger storage/event measurements only |
| Presentation | Bounded copied views; no viewer-owned cognition or simulation mutation |
| Postmortem | Bounded terminal goal, relevant knowledge, last failure/interruption, and causal survival explanation |

## Performance and storage checkpoints

Every slice that adds persistent or scheduled state must record:

- `size_of` and alignment for public/private records and handles;
- empty-agent overhead and ordinary/complex-agent retained logical bytes;
- pool payload, free space, fragmentation, capacity, and allocator/growth behavior;
- insertion, lookup, update, removal, compaction, and bounded retrieval cost;
- cognition events scheduled/processed/stale/compacted and peak due batch/queue;
- observations considered versus stored beliefs/memories/relationships;
- eligible observations versus attended, replaced, expired, and discarded working observations;
- plan candidates considered, retained-plan duration, interruptions, and churn;
- search starts, waypoints, discoveries, resumptions, abandonments, inspected cells, route work, and retained search bytes;
- memory creation, consolidation, forgetting, and replacement counts;
- relationship count distribution and percentage of possible directed pairs retained;
- semantic report/hash size and repeated release scenario time;
- terminal cognitive-summary bytes and lookup/report cost;
- any temporary scratch upper bounds and whether capacity is reused.

Synthetic larger-population measurements test representation and event architecture only. They do not authorize raising the active cognition correctness gate bEach completed slice must update:

- this plan's status and implemented result;
- `docs/STATUS.md` (what works, known limits, what's next);
- `docs/ARCHITECTURE.md` for new modules, ownership, data flow, event, or pool boundaries;
- `docs/DECISIONS.md` for every durable storage, confidence, utility, memory, relationship, or report choice;
- `docs/DEVELOPMENT.md` for new commands, controls, or test harnesses.

Record layouts, capacities, and measured timings inline in this plan's implemented result or in `size_of` assertions rather than in a separate log.

ntrols or user-visible diagnostics change.

Keep detailed Phase 3 requirements canonical here and link from other living documents rather than duplicating the complete slice text.

## Explicit deferrals

Phase 3 does not implement:

- vocalization, pointing, gaze, emotional tone, alarm, requests, clarification, or interpretation candidates;
- personal lexicons, learned signal meanings, proto-language, language transmission, or children;
- personality traits, emotions, social needs, reputation, skills, occupations, or advanced metacognition;
- family, partners, households, ownership, storage, trade, debt, settlements, factions, law, institutions, or warfare;
- a universal settlement score, world-scale omniscient planner, unbounded GOAP search, or complete mental-world simulation;
- all-pairs relationships, permanent storage of routine observations, or a fixed per-agent 4-KiB allocation;
- persistence, save/load, archival paging, generator versioning, or chunk unloading unless an implemented slice proves a new immediate dependency;
- timing-wheel replacement or population scaling merely because cognitive storage exists;
- presentation-owned beliefs, memories, relationships, plans, or authoritative decision controls;
- richer sprites, animation, or asset pipelines.

## Phase 3 exit gate

Phase 3 is complete only when:

- all Slices 0-7 are marked **Implemented** with predecessor dependencies satisfied;
- objective truth, observation, belief, memory, relationship, and plan domains remain explicit and tested;
- 20/100-agent scenarios replay identically and expose understandable knowledge-driven behavior;
- remembered landmarks improve supported survival behavior without global knowledge;
- bounded search can discover at least one initially unknown essential resource without global knowledge or a retained per-cell explored map;
- memory and relationships remain sparse, bounded, measurable, and free of aliasing;
- forgetting/consolidation prevents routine-history growth;
- private intent and unseen truth never leak across agents;
- canonical deaths retain bounded inspectable physical and cognitive terminal causes after transient diagnostics expire;
- Phase 2 physical regressions and reports remain valid;
- living documentation matches executable reality;
- the immutable initial-documentation checksum passes; and
- the full repository validation gate plus repeated Phase 3 release scenarios succeed.

After this gate, Phase 4 may introduce nonverbal communication as observable signals interpreted through the Phase 3 belief, memory, relationship, and plan boundaries. Phase 5 language work must not begin before that communication loop can produce believable misunderstanding and learning evidence.
