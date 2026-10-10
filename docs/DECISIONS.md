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

## D-067: Personality as a four-trait vector (2026-10-10)

**Decision:** Each agent has curiosity, caution, sociability, and diligence (0–255; each the
average of two random bytes, so most agents are moderate). Traits are a pure function of the world
seed and agent id, and tune existing behavior through a per-decision `Temperament`. They add no
new actions.
**Why:** The user wanted different people to do different things. The spec asks for "a compact
trait vector rather than a class hierarchy". Deriving traits from the id costs no storage and
replays exactly.
**Consequences:** Measured on 100-agent runs, each trait moves its behavior in the intended
direction on every seed tested. Average traits reproduce the old thresholds. Traits can't be
inherited or changed yet; storing them becomes necessary once they can.

## D-068: Sparse relationships with trust feedback (2026-10-10)

**Decision:** Each mind keeps 6 acquaintance slots (16 B each): familiarity (grows with
sightings), trust (+32 when a hint they gave is confirmed, −48 when one is abandoned), and where
they were last seen. Hints remember their teller's slot, and evicting an acquaintance detaches
their hints.
**Why:** The spec requires sparse ties and no all-pairs matrix. Trust gives gestures
consequences, which is a first step toward reputation and deception.
**Consequences:** Hint confidence now depends on trust. Mean trust stays close to the default in
studies (≈128–134), because hints are confirmed and refuted at similar rates.

## D-069: Visiting friends and "I've been there" gestures (2026-10-10)

**Decision:** Lonely sociable agents travel to where a friend was last seen, and drop that place
if the friend isn't there. Sociable agents with company skip casual excursions. Gestures can also
point at recently explored ground, which watchers then treat as explored.
**Why:** Sharing was weak because agents rarely regrouped after learning different things. The
user also asked that explorers share exploration details.
**Consequences:** In group spawns, time in company went from 4% to 58% (seed 1), 2% to 21%
(seed 42), and 35% to 57% (seed 9), with no change on seed 4. The share of gestures that tell
someone something new went from 10% to 30% (seed 1), 4% to 34% (seed 42), and 21% to 61% (seed 9).
Total survival didn't change (249/300 with or without the social layer), because most scenarios
were already at their ceiling.

## D-070: Re-plan around the spec's definition of success (2026-10-10)

**Decision:** The active plan is `docs/plans/VERTICAL_SLICE.md`: a small valley scenario, a
communication log, private intent separate from public signals, personal lexicons with a noisy
founding proto-language, receivers that keep competing interpretations, learning and repair from
observed consequences, and an automated test for the first believable misunderstanding. Requests
(COME, GIVE) and children follow.
**Why:** The user confirmed this is what the simulator should become. The spec warns against
overbuilding terrain before proving the social simulation, and today's gestures still carry a
hidden meaning channel (receivers are told the kind of place).
**Consequences:** World-scale and terrain work is paused unless a milestone needs it.
`GestureTopic` will be removed from the receiver path in M2. DANGER waits until the world has
hazards. `MINDS.md` becomes a record.

## D-071: M1, the valley scenario and communication log (2026-10-10)

**Decision:** Add `find_valley`/`camp_sites` (sim-world), a `--valley` study and viewer preset
(16 adults in a 768² valley), engine diagnostics that separate private intent from the public
gesture (`SignalEvent`, `InterpretationEvent`, `HintOutcomeEvent`), and a headless
`CommunicationLog` with `--comms` and `--explain`.
**Why:** The plan's first milestone, and the spec's "instrumentation first". Every later
milestone is judged by what this log shows.
**Consequences:** The log immediately showed that hints never drive decisions (agents prefer
first-hand memories) and that many hints are stale. M4 now explicitly requires hints to compete
fairly in decisions. The "acted" link in the log is approximate (it matches by receiver and kind,
not by the specific hint chosen) and is labelled as such.

## D-072: M2, private intent and public signals (2026-10-10)

**Decision:** Gestures carry a private `UtteranceIntent` (effect, topic, place) and a
`PublicSignal` (sender, origin, pointing, mime, tone). Receivers are reached only through
`Engine::deliver(&PublicSignal)` and `understand(&PublicSignal)`. Before lexicons exist, the
"what" is carried by one of six mimes.
**Why:** Spec 05 forbids any receiver-visible object holding both the signal and its meaning, and
the plan's M2 requires a test that no receiver path can read the sender's intent.
**Consequences:** `SignalEvent` now holds `intent` (private, for tools) and `signal` (public)
instead of `topic`, `intended_place`, and `gesture`. Readings are still unambiguous (one meaning
per mime), so behavior is unchanged. Ambiguity arrives with lexicons (M3) and competing
interpretations (M4); the scoop and pick-and-chew mimes are an obvious first source of confusion.

## D-073: M3, personal lexicons and a noisy founding proto-language (2026-10-10)

**Decision:** 12 engine-level concepts, 32 abstract vocal forms, and a fixed 16-entry lexicon per
agent with evidence for and against plus production outcomes. Founders inherit a seed-specific
convention: 10% of concepts get a variant form and 6% get a synonym. Senders say their best form
for the concept, and watchers learn `form → concept` from the mime that came with it.
**Why:** Spec 06: language belongs to individuals, there's no global dictionary, and learning is
grounded in observable evidence. Variation is needed so that M4 can produce believable
misunderstandings.
**Consequences:** `Mind` grows to 544 B (fine for the valley, but it needs compaction long before
10M agents). Interpretation still follows the mime, so behavior is unchanged. In the seed 1 valley
the band's vocabulary converged from 88% to 97% agreement. Production successes and failures are
recorded but not updated until M5.

## D-074: M4, competing interpretations, dialects, and hints that matter (2026-10-10)

**Decision:** Listeners score candidate concepts (at most 3) from public evidence and their own
state, keeping probabilities and reason flags. Mimes are physically ambiguous in pairs. Hint
confidence = trust × reading probability, and a likely runner-up is kept when it meets an urgent
need. Places are ranked by expected cost with food staleness, fresh hints may displace stale
memories, agents stock up from remembered food, and curious agents check out unverified hints. The
founding band is two families of 8 with different dialects (a third of concepts).
**Why:** The plan's M4. The M1 log showed hints never drove decisions. Uniform lexical noise plus
fast convergence made misreadings vanishingly rare (3 of 7,954). Family dialects are a believable
source of misunderstanding and seed the spec's multiple languages.
**Consequences:** In the seed 1 valley, 6.5% of receptions are misread and 36 misreadings were acted
on, while survival stays at 249/300. Because learning still follows the listener's own reading,
confusions self-reinforce until M5. The "acted" link in the log matches hint checks to the
receiver's latest place hint, which is approximate.

## D-075: M5, learning and repair from observable consequences (2026-10-10)

**Decision:** Unsure listeners ask "this?" (the threshold depends on caution and sociability), and
speakers nod or repair with an exaggerated mime. Hints carry their word, runner-up meaning,
bearing, and gesture id (landmarks are 16 B, the mental map 304 B), so checking a hint later can
teach about the word. Stale spots (stripped bushes in view) are explained away. Misled listeners
later correct the speaker, and corrections wait until the speaker is watching. Usage contradictions
are logged. The two valley families camp apart.
**Why:** The plan's M5, following spec 05: success is inferred later, and repair is strong learning
evidence. The first version taught false lessons (eaten berries were read as "the word must mean
water"), and a 4-tip memory lost the word before hints were checked. Both were found through the log
and fixed.
**Consequences:** Vocabularies converge through use (81% → 94% in the single-camp seed 1 valley).
Speakers can drift to words the other family understands. Survival is 251/300.

## D-076: M6, the definition-of-success test (2026-10-10)

**Decision:** `CommunicationLog::success_episodes` links misread → acted → listener lesson → speaker
lesson through gesture ids, accepting corrected and entrenched episodes alike: the spec requires
updates from observable evidence, not correct ones. `tests/success.rs` (release, seed 1 valley,
1.2M ticks) requires at least one episode and is part of `scripts/validate`.
**Why:** The plan's M6 and the spec's success definition.
**Consequences:** The definition is met, but episodes are rare (1 across 12 seeds at 2.4M ticks).
The test is deterministic but sensitive to behavior changes; if it breaks, find out why before
retargeting it.

## D-077: M7, children and requests for food (2026-10-10)

**Decision:** The default valley band is 16 founders and 4 children (`band_layout`; ids after the
founders). Children start with empty lexicons, a close bond to a parent (stored as `Mind::parent`),
go back to where they last saw that parent, take no excursions, and ask unless 90% sure. A hungry
agent with no food that knows of none can ask someone in view (`DesiredEffect::Request`; the
`PublicSignal` gains an `addressee` and an open-hand `reach_toward` gesture with no minimum
distance). The one asked reads the request like any signal, so it can be misread and repaired, and
then gives, refuses, or shows empty hands. Parents feed their children unless starving; others keep
their last meal and weigh trust, familiarity, sociability, urgency, food, and hunger. Requests reuse
the `Signal` goal: the planned addressee and spot sit in `Dialogue` (compact, 48 B) and are matched
when the gesture completes. `PolicyOptions::helping`, `--no-help`, and `--food PERCENT`
(`Engine::strip_food`, scenario setup before the first tick) support the comparison.
**Why:** The plan's M7 done-when: a child acquires a working subset of the valley's forms purely
from observation (62–95% across 19 seeds), and helping changes survival in a scarce valley. Asking
first came before walking to known food and to people with empty hands, which made survival worse.
Asking only when the asker knows of no food fixed that.
**Consequences:** Helping carries more agents through the first famine (seed 7: 14 vs 12 alive at
600k ticks) but doesn't raise mean lifespan, because food doesn't regrow. The success test now pins
the 16-adult band: the valley with children has had no complete episode in 19 seeds. COME is
deferred.

## D-078: Materials with properties and learned food beliefs (2026-10-10)

**Decision:** Resource kinds became `Material`s with `MaterialProperties` (nutrition, toxicity,
builds, regrowth). Physical rules read properties; agents don't. Each mind holds `Affordances`
(3 B per material) changed only by evidence: eating (felt), watching someone eat and retch, being
handed food, and family culture. Founding families disagree about the new bitter berries (30% of
bushes, world generator v3); children start with nothing and taste what they've never tried when
hungry. Policy decides from `FoodValues`; mimes and readings depend on beliefs (new Retch mime).
Minds that haven't been created yet are read as their newborn state (`Minds::affordances`).
**Why:** The user asked for things abstract enough to progress without hand-coding per item; new
content should be a row of properties, not new behavior code.
**Consequences:** Survival is unchanged (253/300). Knowledge now transmits: children learn food by
watching. Valley livability counts 8+ edible bush samples (was 12) to match fewer edible bushes.

## D-079: Wildlife, warnings, and regrowth (2026-10-10)

**Decision:** Deer and wolves are species traits run by one rule set on per-animal schedules; people
hunt (range 2, helpers raise the odds), wolves bite (new `Injury` cause), sleep heals. `Fauna`
beliefs per species come from bites, witnessed bites and kills, warnings, and lore. A new `Animal`
gesture topic with Snarl and Spear mimes is shouted to 16 cells; listeners keep tips and check them
within 6 cells, relearning the word, learning danger, and correcting the speaker when they find
another animal. Urgent needs prefer places seen first-hand. Bushes grow back lazily
(`--no-regrowth` for famine scenarios).
**Why:** Plan L3: a livelier world where danger and hunting give communication real stakes.
**Consequences:** Valley runs stay survivable (19–20 of 20) with 15–45 kills and 10–55 bites. The
M7 helping test now needs a famine valley (no regrowth, no wildlife): with food renewing, nobody
asks. The success detector now requires the listener to have acted before it learned better (it used
to fall back to the lesson time).

## D-080: Hearths, collapsing into sleep, and the success gate (2026-10-10)

**Decision:** Structure kinds carry costs; a hearth (3 stone, 2 wood) relieves cold when an agent
warms up beside it, and whether hearths warm you is a learned `Crafts` belief (one founding family
keeps fire). Hearths are built only near the builder's home shelter when none is known nearby.
Agents past `REST_COLLAPSE` sleep wherever they are, cold or not; a bite wakes a sleeper; waypoints
avoid stepping straight back to the previous decision spot. The success detector got stricter:
the listener must act before it learns better, and the speaker's lesson must concern the meaning
at stake. The success test moved to the first episode that passes it: the 16-adult seed 13
valley at 200k ticks (bitter berries pointed out as food, read as berries).
**Why:** Plan L4's first step, and three deaths traced to pacing, exhaustion, and being bitten in
one's sleep. The regenerated world no longer had the old seed 1 episode, and the old detector
accepted an episode where the listener never acted. The test was moved only after a scan found a
natural episode under the stricter rules; the check itself got harder, not easier.
**Consequences:** Valley survival is 119/120 with wildlife and hearths. Natural episodes remain
rare (1 in the first 32 runs of the scan).

## D-081: A viewer for people, not just developers (2026-10-10)

**Decision:** The viewer's text HUD and hover card were replaced by a plain-language interface:
a top bar with buttons, hover tooltips, a person panel opened by clicking (need bars and sentences
instead of raw values), a recent-events feed, speech bubbles, a build palette, and a help screen
with a legend. The raw values moved to an F3 readout. The bitmap font gained lowercase letters.
The surface format is now non-sRGB, since the palette is authored in sRGB and an sRGB surface
washed every color out. `Esc` closes things instead of quitting; reset needs `Shift+R`.
**Why:** The user found the old HUD crowded and hard to understand.
**Consequences:** The interface records its clickable regions each frame, so input asks the
renderer what is under the mouse. The feed and bubbles show the sender's private intent (who meant
what); that is presentation only, and agents still never see it.

## D-082: Misunderstandings that run their course, and a sturdier success test (2026-10-10)

**Decision:**
- A hint that came with a word names one spot. It is judged only from within 6 cells: whichever of
  the expected thing and the alternative the listener weighed stands closer to the spot (within 2
  cells) confirms or refutes it. Inconclusive spots turn it into an ordinary hint. Such hints are
  no longer dropped because a place of that kind is known nearby.
- Idle agents sometimes go to look at a place pointed out within 48 cells (curious ones more often).
- When a correction shows a speaker its own word was taken otherwise, it trusts the word less for
  what it meant (as after a failed round of a naming game) instead of reinforcing it.
- Errands that can wait (visits, seeking someone, checking tips) don't lead toward a dangerous
  animal in view.
- People knocked down by a wound come round after 5 minutes with health just above collapsing.
- The success detector also accepts a speaker learning from going where the listener pointed with
  the word (a consequence lesson tied to the listener's gesture), under the same conditions as
  before (same word, after the listener's lesson, about the meaning at stake). The success test
  runs every livable valley among seeds 1-78 (40, in parallel, about a minute in release) and
  requires 2 episodes in total; 7 were measured.
- The study reports how many survivors lie collapsed, and how much the families mix.

**Why:** Measurement showed the families already meet (about 25% of the time near the other
family, 2,000-3,500 cross-family receptions per run), so a shared fire wouldn't help. Misread
hints were discarded or "confirmed" by any matching bush within 36 cells, corrections never
changed the speaker's behavior, and the detector could not count a correction at all. A single
pinned episode broke with every behavior change.
**Consequences:** Complete episodes rose from about 1 in 30 runs to about 1 in 7. Agents are less
idle (58-69% instead of 60-75%). Collapsed agents used to stay down for good and were counted as
survivors (168 of 1,303 in 66 valleys); now 1,238 of 1,320 are standing at the end, against 1,135
before. Consequence lessons that drop what the speaker meant stay rare (about 1 in 15).


## D-083: Herds that recover, hunting for need, wolves that hunt deer (2026-10-10)

**Decision:** Each animal breeds (a herd's birth interval is the per-animal interval divided by its
size, never fewer than 8 breeders), so thinned herds recover. People don't start a hunt while they
carry meat or a carcass with meat is in view. Wolves smell deer from 24 cells, and a wolf that
bites someone leaves people alone for 20 minutes (longer than a wounded person lies down).
**Why:** Deer fell from 24 to a handful in two simulated hours (40-50 kills a run, most of the meat
rotting), wolves rarely found deer and bit people instead, and they kept biting the same person
each time they came round.
**Consequences:** 8-15 kills a run, more meat eaten, herds near their cap, wolves taking deer, bites
13-26 a run instead of 30-130. In 66 valleys, 1,311 of 1,320 people are standing at the end
(1,238 before). The planned "prefer company near wolves" change wasn't needed.

## D-084: Age and sex (2026-10-10)

**Decision:** Every person has a sex and a birth time (`life.rs`; one simulated hour is one year).
People present at the start get them from the seed: founders are adults of 18-39, children of the
band 4-10. Life stages: baby (to 3), child (to 15), adult, elder (from 50). Once a year the old may
die of old age (about 4% a year at 60, 16% at 70, 36% at 80). Men strike a little better than
average and women a little worse (plus or minus 5 in 100), children and elders worse; children
under 12 don't hunt; elders heal less in sleep. Sex and birth are kept by the engine (derived for
the starting band, stored for people born later), not in the per-agent hot records.
**Why:** Plan G2: the groundwork for couples, births, and generations, with realistic constraints
and no built-in roles.
**Consequences:** A 30-year valley run loses 3 people to old age and none to anything else. New
death cause `OldAge`. Slower walking and faster tiring for elders aren't modelled yet.

## D-085: Kin, grudges, favours, and grief (2026-10-10)

**Decision:** Each acquaintance's spare byte now holds how the person is related (parent, child,
sibling, partner) and how many favours the agent owes them, so relationships stay 16 bytes.
Parents and children know each other from the start, and a parent's children know each other as
siblings; family is never forgotten to make room. Trust below 64 (of 255) is contempt: the agent
doesn't visit, ask, or feed that person. A gift makes the asker owe the giver; givers are more
willing toward family (+96) and toward people they owe (+32 a favour), and a gift squares one
favour owed. Anyone who comes within view of where someone close (family or a friend) died, within
three hours of the death, mourns them for 30 minutes (no work, trips, or nosy errands; looks for
company) and then lets them go.
**Why:** Plan G3, ahead of couples and births, which need kin, trust, and loss to matter.
**Consequences:** Behavior in the valleys is unchanged within noise (no one asks for food there
while food is plentiful). Bodies aren't in the spatial index, so seeing one is based on the
death record's place and time.

## D-086: Couples and births (2026-10-10)

**Decision:**
- Couples: unpartnered adults of the other sex who know each other well (familiarity 100) and
  don't distrust each other pair when it's mutual. Partners look for each other when apart and
  share food as family. People who were children together (both under 10, noted in the
  acquaintance's spare bit) don't pair unless one has seen nobody else they could pair with for
  eight years. A partnership fades after two years apart.
- Births: a woman of 15-45 with her partner (up to 65) in view, well fed and rested, not pregnant,
  not carrying a baby, and not having weaned one in the past year, conceives with odds of 1 in 400
  per decision together. Pregnancy lasts nine months. The baby is a record on its mother (carried
  and nursed, which makes her hungrier) until age 3, then becomes a person beside her: blank mind,
  bonded to both parents and its siblings, personality blended from both parents with variation.
  A pregnancy or carried baby is lost with its mother.
- The study follows the growing population (survivors are the living; new tracks for children who
  start walking) and reports couples and births. Exchange lookups use binary search (the log is
  sorted by gesture id), which keeps long runs fast.
**Why:** Plan G4: a band that renews itself, with households and dialect mixing left to emerge.
**Consequences:** In valley 1 over 30 years: 10 couples (all within a family), 11 children born and
walking, the band grows from 20 to 28. Babies aren't agents until weaning, so carrying needs no
special movement. Late pregnancy doesn't slow anyone yet. Newcomers (when a band dwindles) are not
in yet.
