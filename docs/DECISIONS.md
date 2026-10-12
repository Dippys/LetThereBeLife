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

## D-087: Newcomers, wider social circles, and fertility (2026-10-10)

**Decision:** If fewer than 4 adults remain, a couple from elsewhere arrives at the edge of the area
(at most every 5 years), as partners with their own family's lore and words (a new dialect).
People remember 12 acquaintances instead of 6 (minds are 912 bytes). "Raised together" now needs
children under 7 who already know each other well (familiarity 60), not any meeting between
children under 10. Conception odds rose to 1 in 150 decisions a couple spends together.
**Why:** In a 60-year run the second generation never paired: with family never forgotten, a child
with two parents and three siblings had one slot left for everyone else, and brief meetings between
the families' children ruled them out as partners. About 2 children per woman let the band shrink.
**Consequences:** Over 60 years, valley 1 grew from 20 to 62 (64 births, 41 couples, 19 across
families; the families spend 68% of their time near each other) and valley 4 from 20 to 26. The
success test found 18 episodes in 7 valleys (5 in 3 before).

## D-088: Names (2026-10-10)

**Decision:** Names are their own sounds (`Name`, 1,024 two-syllable names such as "Tavo"), separate
from the 32 word sounds. Everyone has a given name: from the seed for people present at the start,
chosen by the mother (one she doesn't already use) for children. Each acquaintance records what the
agent calls them (records grow to 20 bytes; minds to 960). Founders know their own family's
founders' names; newcomers know each other's. A mother calls out her child's name when it starts
walking, and people call out a name when they meet someone they haven't seen for 10 minutes;
everyone awake within 12 cells learns it, but one in four bystanders standing next to someone else
pins it on that person instead. Hearing a person called another name twice running replaces one's
own name for them.
**Why:** Plan G5: names that spread by use and can be misheard, like the rest of the language.
**Consequences:** In a 30-year valley run people know the names of 318 of their 441 acquaintances;
104 of those differ from the given name, partly from mishearing and partly because a name that
catches on spreads (a nickname). The viewer shows given names, and in the person panel what that
person calls the people they know. Names aren't used in requests or corrections yet.

## D-089: Watching generations (2026-10-10)

**Decision:** The map draws women's squares with notched corners, children smaller, elders with a
grey edge, a pink dot for a carried baby, and a pink line from the picked person to their partner.
Family members know each other's names from the start. The study tracks each person's generation
(founders and newcomers 0, their children 1, and so on), reports per generation how many of their
place words are still the founders' most common, and takes `--years N`.
**Why:** Plan G6: make generations visible and measurable.
**Consequences:** In a 90-year valley run (seed 1, 8 minutes headless): five generations, 61
people alive, 84 births, 39 deaths all of old age, 56 couples (21 across families), 84 complete
episodes. Words hardly drift: generations 1-3 keep the founders' word for every place, generation 4
for 97%. Without a way to coin words, the band settles on one language and keeps it; names drift
far more (227 of 625 known names differ from the given name).

## D-090: The famine helping test counts people standing across 20 valleys (2026-10-10)

**Decision:** The release test that asking for food helps in a famine now runs every livable valley
among seeds 1-40 (20, in parallel) with and without helping and compares the people standing at
the end (alive and not collapsed), instead of survivors in seeds 7, 10, and 12.
**Why:** After the generations work it read 49 "survivors" with helping against 50 without. Two of
the three seeds no longer had anyone asking for food, and the extra "survivor" without helping was
lying collapsed. Across all 20 famine valleys, helping keeps 365 standing against 361 without; the
gain comes from the few valleys where people ask. The claim tested is unchanged (helping beats not
helping); the measurement is broader and no longer counts the collapsed.
**Consequences:** About 45 seconds in release. The effect is small, so a behavior change could
flip it; if it does, look at the valleys with requests before touching the test.

## D-091: New words and sound shifts (2026-10-10)

**Decision:** Someone pointing something out with no usable word for it (a child who hasn't learned
one, or an adult whose word was discredited by corrections) sometimes makes one up from a sound it
doesn't use for anything (curious people more often: 16-112 in 256 tries) and says it; listeners
learn it like any word. Children now and then (1 in 40 words heard) pick a word up with its first
vowel changed. The study reports words coined, coined words said by two or more living people at
the end, sound shifts, and how many place words the two families' most common words still share.
**Why:** In a 90-year run generation 4 still used 97% of the founders' place words: with no way to
make words, the band settled on one language and kept it.
**Consequences:** Over 30 years a valley coins about 50 words, of which 4-5 catch on. Drift varies:
on seed 4 generation 1 keeps 74% of the founders' place words and the families share half, while
seeds 13 and 46 converge on one shared vocabulary (the families live in one valley and mix a lot,
which merges dialects). Long runs are chaotic: complete episodes over 30 years ranged from 0 to 13
on these seeds. The success test still passes (16 episodes in 40 valleys).

## D-092: Things to talk about come from the world's tables (2026-10-10)

**Decision:** `Concept` is now `Water | Been | Material(m) | Species(s) | Structure(k)`, and
`LandmarkKind` is `Water | Material(m) | Structure(k)` for materials found at fixed places (a new
`MaterialProperties::fixed_source`; meat isn't, since carcasses rot). Both are built from
`Material::ALL`, `Species::ALL`, and `StructureKind::ALL`, with short constants (`Concept::BERRIES`,
`LandmarkKind::SHELTER`) for code and tests. Gestures come from properties: a material's
`Handling` (pick, chop, strike, carve) or the sender's belief that it feeds or sickens; a species'
bite (snarl) or not (spear); a structure's `Purpose` (rest, warmth). Readings score every concept by
how well the seen gesture matches what the listener associates with it, with partial credit for
similar motions; this reproduces the old hand-written table. Hunger and urgency favor whatever
the listener believes is food (not just berries), and hungry listeners treat calls about
harmless animals as calls to hunt. Memory slots are 4 for water and 2 for every other kind of
place. Viewer labels and colors are composed from material, species, and structure labels.
**Why:** Adding a material, animal, or structure needed new code in the concept list, the gesture
tables, the reading table, the memory layout, and the viewer. Now it needs a row of properties
(plus a name and a color for the viewer).
**Consequences:** Meat is a concept (people can have a word for it) but not a place. Concept order
changed, so founding vocabularies are dealt differently. The success test found 16 episodes in 6
valleys.

## D-093: Seasons (2026-10-10)

**Decision:** Four seasons of one simulated hour each (runs start in spring). Lives are compressed
more than seasons (a year of age per hour) so that a winter outlasts a hunger cycle and matters.
Each season adds a chill to everyone's exposure rate (summer -1, spring 0, autumn +1, winter +2 per
rate period); need states carry it (still 32 bytes) and are re-based at each change, and people
spawned mid-season get it too. Fruit (`MaterialProperties::seasonal`) doesn't grow back in winter.
Animals don't breed in winter. The viewer shows the season and tints the land in winter and autumn.
**Why:** Life was too easy for information and cooperation to matter (about 99% survival, food
everywhere, 60% idle). Seasons bring scarcity and cold without scripting what people do about it.
**Consequences:** Hearths finally matter: 35-78 warm-ups per 5.5-hour run instead of about 2, and
over 30 years 443 warm-ups, with fire known by 13 founders and 20 children (8 founders before).
Over 30 years 5 people died of cold. Agents don't know about seasons; they feel the cold and find
bushes bare. The success test found 16 episodes in 7 valleys.

## D-094: Fires that need fuel (2026-10-10)

**Decision:** Materials have `fuel_seconds` (wood burns 30 minutes a unit). A finished hearth burns
on the fuel built into it (an hour from its two wood) and then goes out; only a burning fire warms.
Anyone who knows fire, carries something that burns, and sees a fire that's out or has under 20
minutes left may feed it (a new `TendFire` action, up to two hours of fuel ahead), especially when
cold; watchers learn about fire from it. The viewer draws dead fires grey, shows the time left on
hover, and the feed notes relit fires. The study counts fuel added and relights.
**Why:** Plan step 3: things that give people reasons to cooperate. A shared fire that anyone can
keep going gives people a reason to return to the same place, and pairs with winter.
**Consequences:** 82-864 fuel additions per 5.5-hour run; over 30 years 751, with 323 relights, no
deaths from cold (5 before), and fire known by 16 founders and 28 children. More hearths get built
(50-80 per run), since people build one near home when they remember none burning.

## D-095: Stone blades (2026-10-10)

**Decision:** A new made material, `Blade` (`made_from` 1 stone, `cutting`). One founding family
knows how to knap (the other keeps fire); anyone who watches it done learns how. A knapper with no
edge picks up stone in view and knaps a blade (a `Craft` action, 5 minutes). A carried edge doubles
what one go at chopping or carving yields (wood, meat) and adds 15 points to strikes; one use or
kill in 6 wears it out, so blades get remade and seen being made. The study counts blades, watchers,
and who knows how; the feed notes a blade made in front of others.
**Why:** Plan step 3: knowledge that one group has and another can only get by watching. Tools are
properties (`cutting`, `made_from`), so new ones need no new code paths.
**Consequences:** 36-49 blades per 5.5-hour valley; over 30 years 83-227, and 7-22 children learn
to knap. Survival is unchanged (1,001 vs 1,004 standing on 47 fresh valleys). Adding a material
shifts concept indices and so reshuffles founding words: episodes in the success test's valleys
fell from 15 to 3, but fresh valleys (seeds 79-160) show 1 in 47 either way, so the old 15 was a
lucky cluster and episodes are about 1 per 20-40 valleys. The children's-words slice test now
averages all 21 valleys among seeds 1-44 (75%; one valley's 4 children swing by tens of points)
instead of pinning seed 1.

## D-096: Two-valley start, and where episodes really break (2026-10-10)

**Decision:** `--apart` (`StudySpawn::Apart`) starts each family of the standard band in its own
livable valley of a neighboring pair (`find_valley_pair`, centers 1,024 cells apart); both valleys
and the land between are simulated, with as many animals per valley as the one-valley start.
`find_valley` shares the candidate scan and keeps its old choices.
**Why:** Plan step 4: groups apart long enough for dialects to diverge, then meeting.
**Consequences:** The families first hear each other within 0.1-0.8 years (people roam that far in
weeks) and spend 2-22% of their time near each other early on (36-68% in one valley), but their
words still converge over 30 years. Episodes don't change: 4 in 57 apart runs against 4 in 89
single valleys (5.5 hours each). Even with entirely different founding words (an experiment, not
kept) there were 1 in 35 apart and 3 in 42 together, though cross-family misreadings tripled. The
episode funnel shows why: the speaker almost never learns afterwards (0-1 per run), because it
would have to hear the listener use the word the old way, which the listener just unlearned. Next
is giving speakers observable feedback, not more separation.

## D-097: Listeners report back, holding up what they found (2026-10-11)

**Decision:** A listener who follows up a tip and finds what it expected plans a report to whoever
told it (same slot as corrections, which come first; 6-hour patience). When it next sees them it
points at the place, says the word, and holds up the thing if it carries some. Signals gain a public
`shown` material; a held-up thing is read as itself whatever the word or mime.
**Why:** The episode funnel showed every genuine misreading ended with the listener "confirmed" by
coincidence (both kinds of berries grow together), so nobody found out and speakers never learned
(D-096). Most corrections came from listeners who had understood but then mislearned at the place.
Reporting back with the object is ordinary human behavior and gives the speaker observable evidence.
**Consequences:** Complete episodes went from 3 in 2 of the success test's 40 valleys to 79 in 24,
and from 1 to 141 in 37 of 49 fresh valleys (seeds 79-160). Survivors 848 vs 879 and 1,011 vs
1,001 (about 1% fewer overall, within noise). The success test now requires 40 episodes in 12
valleys (was 2).

## D-098: Only a look at the spot can refute a word (2026-10-11)

**Decision:** A listener who gives up on a worded tip after searching learns nothing about the word
(pointing is rough; the thing may just be elsewhere). Up close, the word counts as misread only when
the alternative stands at the spot (within 2 cells) and nothing of the expected kind is within view
of it (6 cells); with both around, the tip becomes an ordinary hint. `HintCheck::up_close` marks
judgements made at the spot.
**Why:** Most corrections came from listeners who had understood but mislearned: they couldn't find
the thing near their guess, saw the alternative somewhere in view, and "corrected" the speaker.
**Consequences:** On seed 1, corrections fell from 38 to 7 and word lessons against what the speaker
meant from 31 to 5 (0-1 on seeds 4, 9, 13). Episodes 86 in 23 of the success test's valleys (79 in
24 before); survivors 839 vs 848 over those 40 valleys.

## D-099: A year is one turn of the seasons (2026-10-11)

**Decision:** `SECONDS_PER_YEAR` is four seasons (four simulated hours, was one), starting each
spring; a month is a twelfth of that. Conception odds per family check scale with the year so births
per year stay put; everything else in life was already counted in years. The viewer's top bar shows
"Year N", a twelve-month strip colored by season, and the season; the raw clock is in F3. The
person panel's "doing / why" block is a fixed three lines so the panel doesn't jump.
**Why:** With one-hour years each year held a single season ("Year 4 · Winter", then "Year 5 ·
Spring"), which made no sense on screen. Shortening seasons instead would make winter shorter than a
hunger cycle and undo D-093/D-094.
**Consequences:** Lives and generations take four times as long in simulated time. Seed 1 over 30
years (now 120 hours): 26 survivors, 15 babies, 239 episodes (before, at one-hour years: 34-43
survivors and 19-26 babies on seeds 1, 4, 5). Short runs see far fewer births.

## D-100: No building that shuts anyone in; idle people show as resting (2026-10-11)

**Decision:** Before a hut or hearth goes up, `would_enclose` flood-fills (four directions) a 9x9
area around the site: if any walkable cell next to it could reach the edge of that area before and
can't with the site blocked, the build is refused (`BuildShelterError::WouldEnclose`). People apply
the same test to what they see when choosing a site (cells out of view count as open), so they pick
another spot rather than retrying. Pockets that were already closed don't count against a site.
Separately, choosing to wait now records `Wait` as the agent's goal; before, the panel kept showing
the last action (so idle people seemed to be "pointing something out" for minutes).
**Why:** People beside water could be boxed in by a hut and a hearth built next to them.
**Consequences:** Six 10-year valleys: 139 vs 140 survivors, 0 vs 1 deaths from thirst (traps were
rare in headless runs; the unit test covers the nook case). The "Pointing" label fell from 28% to 2%
of samples; behavior is unchanged.

## D-101: Warming up in a hut lasts until well warmed (2026-10-11)

**Decision:** Sleeping in a shelter to get warm lasts until exposure is down to half its threshold
(was one point below it).
**Why:** Late in each winter the simulation did 3-4 times the work (about 80,000 events per 15
simulated minutes instead of 20,000, and about 6 times the compute), which made the viewer crawl.
Everyone was cold; each warming nap ended just under the threshold, the winter chill pushed them
back over a tick later, and they lay down again: 14,000 naps in 15 minutes.
**Consequences:** Late winter now costs the same as other seasons. Six 10-year valleys: 146
survivors vs 139, deaths from cold 6 vs 11. Episodes 60 in 28 of the success test's valleys (86 in
23 before).

## D-102: A harder world: swimming, loads, storage, effort, learned huts (2026-10-11)

**Decision:** One batch of world and body rules, at the user's request:
- **Swimming:** lakes and rivers can be waded (6x a soil step) or swum (12x when deep); the sea
  can't. Each step in water chills and tires. Perception's reach crosses water.
- **Loads:** one shared carrying limit of 12 things (was 32 of each). People can set down what
  they have no use for (`Drop`), keeping tools, wood or stone for what they can make, and a little
  food.
- **Storage:** a finished hut holds up to 48 things (`Store`, `Fetch`); the contents are visible to
  anyone nearby. People unload at a hut in view and remember where they left food.
- **Effort:** only grown, non-pregnant people build and chop wood; strength scales build time (80%
  to 200%). A hut takes 10 wood and 5 minutes, a hearth 4 stone, 3 wood and 3 minutes. Builders
  keep working through cold, hunger and tiredness; thirst stops them.
- **Learned huts:** founders know how to build huts; children and newcomers learn by watching one go
  up or sleeping in one. Huts aren't built within 96 cells of one already remembered.
- **Scarcity:** berries regrow one per bush every 20 minutes (was 10), trees one every 2 hours (was
  1); stone never regrows.
- **Giving up:** a trip to a place someone pointed out is dropped after 16 decisions without
  getting closer. Thirst past its threshold comes before cold and tiredness.
- The viewer's `--advance` prints progress, yearly summaries, births and deaths to the console.
**Why:** The world was too easy (people carried pantries; huts went up in seconds anywhere someone
felt tired) and people could be stuck behind lakes.
**Consequences:** Survivors in valleys 1, 4, 9, 13 after 1.4 years: 18, 20, 17, 20 (about 22
before). A 100-year run of seed 1 survives with 14-25 people and about five generations. Most
non-old-age deaths are winter cold; most fires are out; 37 huts go up in year one and never decay;
stored food is rarely fetched. Children score 64% on the band's place words (75% before; berries
and huts get pointed out less) and the success test finds 42 episodes in 22 valleys (86 before; it
needs 40), so its margin is thin. Agent memory is 984 bytes. Decision rules were patched heavily in
this batch; the next step replaces them with learning what works (docs/plans/LEARNING.md).

## D-103: Learning what works, phase 1: habits learned silently (2026-10-11)

**Decision:** Each mind keeps `Habits` (32 entries of 4 bytes): for a situation (needs past
threshold, season, shelter or fire in view: one byte) and a kind of choice (18 kinds mapped from
the policy's goal and reason), a moving average of the relief it brought. Relief is the drop in
discomfort (how far each need is past half its threshold) between choosing and choosing something
else; re-deciding the same thing in the same situation continues the trial. The scripted policy
still decides; the study reports the best-valued choices per need (`habits:`). Mind 1,112 bytes.
**Why:** docs/plans/LEARNING.md phase 1: see what people would learn before letting it decide.
**Consequences:** Behaviour is unchanged (seed 1's report is identical to before apart from the new
line). Habits form sensibly: thirsty, drinking +12; tired, sleeping +6; cold, sleeping +4 (seed 4),
-5 where warming naps are cut short by thirst (seed 1). Judging every re-decision instead gave
almost only zero relief (107 of 28,626 trials), so trials span a run of the same choice.

## D-104: Huts nobody lives in fall down (2026-10-11)

**Decision:** Each season, every finished hut nobody has slept in since ages by one; after eight
seasons (two years) unused it falls down, with anything stored in it. Sleeping under a hut resets it.
No repair action: lived-in homes last, abandoned ones go.
**Why:** Huts never decayed, so the first year's burst of building stayed forever (37 in year one,
51 by year four in the 100-year run).
**Consequences:** Over 10 years in valleys 1, 4, 9: 53, 50, 55 huts built and 43, 30, 38 fell down,
leaving about 10-20 standing; survivors 15, 30, 13 (15, 28, 14 without decay).

## D-105: Learning what works, phase 2: being cold or tired is answered by experience (2026-10-11)

**Decision:** When cold or tired, the planner lists what the agent could do (warm up at a fire, feed
one, sleep in a hut in view or a remembered one, build or gather toward a hut, head for a hearth,
fetch wood, explore, or sleep in the open when mild) and picks by an instinctive lean toward the
old order (3 points per place), plus that choice's learned value in this situation, plus a
curiosity bonus (8) for choices tried fewer than 3 times (a deterministic roll against curiosity).
About to collapse, an agent still sleeps where it stands (a reflex). `PolicyOptions::learned`
(on in `full()`); `--scripted` turns it off in the study.
The success test now spans seeds 1-158 (87 valleys) and needs 40 episodes in 20 valleys, about
half of the 84 in 41 measured. Its old bar (40 in 40 valleys) was set at half the rate of D-097,
before the harder world of D-102 halved it; it then sat at the average and failed on chance.
**Why:** docs/plans/LEARNING.md phase 2: replace the decision rules that had been patched most.
**Consequences:** Over 10 years in valleys 1, 4, 9, 13: survivors 24, 30, 20, 23 against the
script's 15, 30, 13, 22; deaths from cold 2, 1, 6, 0 against 9, 1, 11, 3. On 47 fresh valleys
(seeds 79-158) episodes 48 in 25 valleys against the script's 50 in 22, survivors 917 against 875.

## D-106: What the learning experiments showed (2026-10-11)

**Decision:** Three follow-ups to phase 2 were tried and not merged (branches `copying`,
`view-habits`): copying habits from people seen getting relief; showing each person's best habits in
the panel; and learning only from choices made while a need presses.
**Why it matters:** Habit slots fill with calm moments (resting, chatting, gathering with no need
pressing), which bring no relief but are so frequent they evict the few that matter ("cold:
sleeping helped"). So copied lessons found no room (no effect in 7 of 8 valleys) and the panel had
nothing to show. Keeping only pressured choices retained the right memories but did worse over 10
years in 8 valleys (177 survivors against 185; seed 9 lost 13 to cold against 6): with experience
kept, people settle on sleeping and stop trying fires. Much of phase 2's gain came from curiosity
trying varied options in situations nobody remembered, not from accurate values.
**Next:** better credit for what a choice achieves (relief per time, relief that lasts, the
need it was chosen for), so kept experience is trustworthy; then copying and the panel again.

## D-107: Saving and opening worlds (2026-10-11)

**Decision:** `Engine::save` writes everything the simulation changed (people, minds, the event
schedule, families, animals, structures, resource levels, placed objects, deaths, counters) in the
compact `postcard` format, with a version number first; `Engine::load` builds a new engine from the
saved config, regenerates the terrain people live in from the seed, and restores the rest. Per-tick
event lists and the route planner's scratch space aren't saved. The viewer opens saves with
`--load FILE`, writes one after a fast-forward with `--save FILE`, and saves/opens a quick save with
F5/F9 (`saves/quick.ltbl`, ignored by git). serde derives were added across sim-core and sim-world.
**Why:** Long runs (100 or 1,000 years) take minutes to hours; now they're computed once.
**Consequences:** A world saved and reopened continues identically (tested over 40,000 ticks on
either side). A 5-year valley is a 30 KB file and opens in about a second. Saves from another
layout are refused with a clear message; bump `SAVE_VERSION` when saved types change.

## D-108: People learn what their choices bring about (2026-10-11)

**Decision:** Besides how much each choice eased them, people note what each kind of choice
brought about that wasn't so before: a burning fire in view, or a shelter at hand (72 bytes per
mind: how often, out of how many tries). When choosing, an option also earns half of how much
better they've fared once that was so, weighted by how often the choice brought it about. So
feeding a dead fire, which eases nothing by itself, is worth it because a burning fire is where
warming up has helped. Walking over to a fire now counts as part of feeding it or warming at it.
`SAVE_VERSION` 2.
**Why:** The user chose real understanding over passing credit back (D-106's `credit` branch):
people hold a belief ("feeding a fire makes it burn") and combine it with what helps.
**Consequences:** People learn the links quickly (feeding fire lights it 14 times in 15; building
gives a shelter 614 in 618). Survival over 10 years in 20 valleys is unchanged within chance:
474 survivors against 462, deaths from cold 34 against 31; each half alone gave 458 and 467. It
doesn't yet weigh how long a step takes, so building a hut while freezing looks as good as warming
at a fire. Mind size 1,184 bytes.

## D-109: Homes (2026-10-11)

**Decision:** Each person keeps a home: the hut it sleeps under, with an attachment that grows by
a night under its roof (up to 16) and wears down by a night elsewhere (by 4 when beyond
`HOME_RANGE` 200 cells). Once worn away, the next hut slept in becomes home; seeing the hut gone,
or giving up on reaching it, loses it. Cold or tired, going home is the first option. With a home,
people don't build, unless their home is full or they're freezing far from it; a full hut in view
lets them build right beside it (huts can cluster). Mind 1,192 bytes; `SAVE_VERSION` 3.
**Why:** The user saw hut spam: anyone cold more than 96 cells from a hut they remembered built
one, and wide-ranging bands put up hundreds that fell down empty.
**Consequences:** Over 10 years in 20 valleys, huts built fell from 4,527 to 505 with survivors
unchanged (470 against 474), but deaths from cold rose from 34 to 42: with fewer huts, people far
from home in winter sleep rough. Variants that let anyone build beyond 200 cells kept cold deaths
at 28 but built 3,909 huts; dropping going home when cold gave 52.

## D-110: The famine-helping test no longer asserts survival (2026-10-11)

**Decision:** The famine-valley test checks that people ask for food and get some, and that
nobody asks without helping; it no longer asserts that helping keeps more people standing.
**Why:** Since storage and huts (D-102), famine valleys (5% food) are mild: most lose nobody, and
where people ask, others have nothing to spare. Across 20 valleys only 6-11 gifts happen, and the
comparison was a coin flip (305 against 303 on main; 326 against 327 with homes; at 2% food 264
against 263). The user chose to record it and fix helping later rather than hold homes.
**Next:** make sharing matter again (spare food, food stored in huts), then restore the assertion.

## D-111: Plants live, spread and die (2026-10-11)

**Decision:** A tree chopped down to nothing is felled and gone; trees still grow wood back while
standing. Once a year, at the start of spring, each bush dies with odds 1 in 8 (trees 1 in 60),
and each living plant may seed a new one within 5 cells (bushes 1 in 2, trees 1 in 15), on open
ground of the same biome, only where its 7x7 patch is less crowded than its parent's (so thickets
refill and sparse ground stays sparse). Seedlings are placed objects that grow like generated
plants: bushes fruit from their first growing season, saplings start with 2 wood. Placed objects
gained a growth clock and bitter bushes. The viewer draws bare ground over dead generated plants
and young trees smaller. Famine valleys (no regrowth) don't grow. `SAVE_VERSION` 4.
**Why:** The user wanted food and wood to move around the valley over years rather than sit on
fixed spots; bushes survive picking (picked bare, they fruit again next season) so a band can't
wipe out its food in a season.
**Consequences:** Over 10 years in 20 valleys, trees went from 311,332 to 341,643 and bushes from
117,749 to 153,266. Survival 457 against 470 and deaths from cold 52 against 42, within the noise
between two runs differing only in tree seeding (468 and 45). Valley 1 does consistently worse
(7-11 survivors against 14) as bushes near its camp die off.

## D-112: Huts by a fire are warmer, and hearths go beside home (2026-10-11)

**Decision:** Sleeping under a roof with a burning fire within `FIRE_WARMTH_REACH` (6 cells) is
`SleepQuality::Hearthside`: it warms twice as fast as a hut alone (exposure -8 per period
against -4). A hearth is built within 4 cells of the builder's own home (D-109), instead of
anywhere within 16 cells of a hut it knows; with the stone and wood in hand away from home, it
walks back first. The study prints sleeps in the open, under a roof, and by a fire.
**Why:** The user suggested huts gathered around hearths. The world now gives a reason (warmth)
and people's choices learn it (sleeping where a shelter and a fire are in view eases more); the
hearth goes where the builder lives rather than by a rule to cluster.
**Consequences:** Over 10 years in 20 valleys: survivors 470 against 457 without warm huts,
deaths from cold 47 against 52, sleeps by a fire 3,909 against none (2,398 with warm huts but
hearths placed as before). Within noise, but nothing got worse; valley 1 recovered from 7 to 23.
The check that talking brings words together now sums agreement over the 10 valleys among seeds
1-16 (it rises in 7 of the first 8 valleys, by up to 11 points) instead of requiring it of seed 1
alone, which dipped 65% to 64% with this change.

## D-113: Thinking agents and life seeds: the plan (2026-10-11)

**Decision:** Adopt docs/plans/THINKING.md. The world seed stays strictly deterministic; a separate
life seed drives people, animals, plants and every roll, new each viewer run, recorded in saves
and reports and replayable with `--life-seed`, with the study defaulting to one derived from the
world seed so comparisons stay paired. Agents get a learned cause-and-effect model (what each kind
of action needs, produces, relieves, and how long it takes), a bounded backward planner over their
own beliefs, and one intention they follow and revise. The scripted order becomes instinct priors.
**Why:** The owner's brief: agents react instead of thinking, and every run of a seed replays the
same lives. No neural network or language model: minds stay readable, compact and grounded.
**Consequences:** Built in phases T0-T6, each measured before it's switched on.

## D-114: World seed and life seed (2026-10-11)

**Decision:** `EngineConfig` has a `world_seed` (terrain and generated features only) and a
`life_seed` (every roll in the engine and minds). The viewer draws a new life seed from the clock
each run and prints it; `--life-seed N` replays a run, in the viewer and headless. Saves, the
snapshot, F3 details, study and scenario reports carry both. Headless runs default the life seed
to the world seed so comparisons stay paired; `config/simulation.toml`'s `seed` is the world seed.
Rule 3 in AGENTS.md now reads: the world is fixed; lives are not predetermined, but replayable.
**Why:** The owner's brief (D-113, T0): the same seed replayed the same lives every run.
**Consequences:** Seed 1 with life seeds 1, 2, 3 gives different histories in the same valley.
Valley spawn positions still come from the world seed.

## D-115: A cause-and-effect model per person (2026-10-11)

**Decision:** Each mind holds a `Model` (536 bytes): for each of the 18 kinds of action, what it
brought about that didn't hold before (counts per fact), how much it eased each need, how long it
takes, and what it needs. Facts are 16 bits a person perceives: its pressing needs, what's at hand
(water, food it believes edible, wood, stone, a burning fire, a fire gone out, a hut, stored food)
and what it carries. What an action needs is learned only from evidence: using something up (felt
when fuel, wood, stone or food leave its hands), finding it couldn't do it without something, or
watching others (a fire relit, a hut finished, a blade knapped); never from what happened to be
around, which made coincidences (water by every camp) look like needs. Trips to remembered places
count as travel, not as the action at the end (habits keep their own classification). Everyone
starts with instincts (drink at water, eat what you carry, pick food, sleep, gather wood); knowing a
craft adds its know-how. Mind 1,752 bytes.
**Why:** docs/plans/THINKING.md T1: what agents do should come from what they know.
**Consequences:** After two years, people believe drinking needs water at hand, picking food gets
food, building needs wood and gives a hut, knapping needs stone and gives a blade, feeding a fire
needs a fire gone out and wood. No behaviour change.

## D-116: Plans and one intention, off until they're better (2026-10-11)

**Decision:** `PolicyOptions::plans` (a bit per need; `--plans MASK` in the study) lets a pressing
need be eased by a plan: a backward search through the person's own model, up to 4 steps, each a
trip (costed by the distance to the remembered place) or an action believed to bring the next fact
about at least half the time, scored by relief per minute and its chance; plans over 7 minutes
aren't started. The person holds one `Intention` (20 bytes) and follows it step by step with the
existing actions, notes what it lacked when a step can't be done, changes its mind when a different
plan looks twice as good, and after two failed plans for a need leaves it to instinct until it
eases. Plans show in the person panel ("Plan: gather wood, then feed the fire, then warm up"), in
`--trace`, and in the study. Off by default.
Fixes found on the way, on for everyone: every trip to an out-of-view place is given up when the
person stops getting closer (trips relabelled as warming used to loop until the person froze), and
a fire nobody can get beside isn't chosen for warming or feeding.
**Why:** THINKING.md T2 (cold first), measured before switching on.
**Consequences:** Over 3 years in 8 valleys, without plans: 156 survivors, 2 cold deaths (the two
fixes; 153 and 6 before). With plans for cold: 148 and 11; 98 plans formed, 51 completed, 39 given
up. Plans are mostly "go to a fire, warm up", "go to a hut, sleep", "feed the fire, warm up".

## D-117: Faster perception; plants' spring step near people only (2026-10-11)

**Decision:** Perception looks up each cell's terrain and feature once, chunk by chunk
(`World::visit_area`), instead of about a dozen separate lookups per cell; results are identical.
The yearly plant step counts a seedling's patch with a sorted index of living plants instead of
scanning every feature in the world twice per seedling, and only plants within 512 cells of
someone living die and seed.
**Why:** The user asked for faster runs, and reported a stall at the turn of spring in the viewer:
the plant step was quadratic in the plants loaded, and the viewer loads far more than a valley.
**Consequences:** A valley year runs in 4.1 s instead of 6.2 s (perception 2.8 s to 0.9 s).

## D-118: Plans for cold on: instinct first, plans when it has nothing good (2026-10-12)

**Decision:** A pressing need is first met by what instinct and habit would do; if experience
vouches for that option here (tried twice or more in this situation, value 3 or more), the person
does it and drops any plan it held. Only otherwise does it plan, judging the last step by its
experience in the situation the plan leads to (by a fire, in a hut, this season). Every walk now
carries where it's headed: once the person stops getting closer it gives up and keeps clear of
that place for 10 minutes (`Avoid`, also on for everyone), and a trip ends on arrival, so a plan
that expected a burning fire where it had gone out is rethought from what's in view. Plans for cold
are on in `PolicyOptions::full()`.
**Why:** Plans lost to instinct (D-116) because they judged relief without context, replaced
instinct's better nearby options, and people circled fires they couldn't reach or that had gone out.
**Consequences:** Over 10 years in 20 valleys: 482 survivors with plans for cold against 480 without,
deaths from cold 33 against 34; 1,372 plans formed, 585 completed, 783 given up (mostly rethought on
arrival or for a better option). Without plans, the fixes alone took survival from 470 to 480.

## D-119: Plans for hunger and thirst on (2026-10-12)

**Decision:** Hunger and thirst are met by the same planner as cold (D-118): instinct first, a
plan when experience vouches for nothing at hand ("go to the bushes I remember, pick, eat").
**Why:** THINKING.md T3. Instinct already handles food and water well, so this matters where it
has nothing: food stored in a hut, a remembered spring.
**Consequences:** Neutral on survival (480 against 482 over 10 years in 20 valleys), as expected.

## D-120: Getting ready for hard seasons, built but off (2026-10-12)

**Decision:** Each mind remembers how cold and hungry each season got last time it came round (11
bytes). With the readiness bit (`--plans 27`), a calm person whose next season was hard last time
forms an intention to get something ready (`Goal::Ready`): be where a hut is if it was cold, have
food on hand if it was hungry, only in the last month before it. Shown as "Getting ready: ...".
Off by default.
**Why:** THINKING.md T5, anticipation from lived seasons.
**Consequences:** Over 10 years in 20 valleys, readiness all through autumn did worse (460
survivors against 482: pulled home, people skipped autumn's work of wood, food and hearths); in
the last month only, neutral (487 against 493, cold deaths 27 both). Next: get ready by stocking,
not by staying, once storing and fetching are worth it.

## D-121: People believe what others know, and show them what they don't (2026-10-12)

**Decision:** Each acquaintance record keeps the kinds of place the person believes they know (the
byte that was spare): seen standing right at water, a source or a hut, or seen pointing one out.
When sharing, a person shows first a place of a kind someone around isn't believed to know
(strangers know nothing), and otherwise rotates through its places as before.
**Why:** THINKING.md T6: "she doesn't know where the water is, so I'll show her."
**Consequences:** Over 10 years in 20 valleys: 493 survivors against 480, deaths from cold 27
against 33; receptions that told the listener something new rose from 380,000 to 691,000 (20% to
30% of receptions), spread over every kind of place instead of mostly water.

## D-122: Social minds no longer keep more company than others (2026-10-12)

**Decision:** The study test keeps only "sociable people spend more time in company than
loners"; it no longer asserts that social minds keep more company than minds without social
features. The user chose to record it and keep the part that still holds.
**Why:** With D-121 the one seed it checked flipped (29% against 30%), and across seeds the claim
had already stopped holding (seed 5: 25% against 62% before D-121): families, homes and shared
camps now keep everyone together, so visiting friends and following hints mostly pull people
apart. Informative pointing-out (D-121) sends listeners off to see places they didn't know.

## D-123: Idle time works toward what has proved useful (2026-10-12)

**Decision:** When nothing presses and instinct would just wait, a person forms an intention to
get the thing it lacks that its own beliefs say has been most useful: what is needed by the
actions that eased its needs, or that brings about what those need, two steps deep (wood feeds a
fire, at which warming up helps; stone makes a blade). It's the same planner and intention
(`Goal::Ready`), e.g. "Getting ready: gather wood". On by default (`plans` bit 5).
**Why:** THINKING.md T4. Replacing the calm-time script outright cost lives when tried as
readiness (D-120); filling idle time (about half of it) adds work instead of displacing it.
**Consequences:** Over 10 years in 20 valleys: 510 survivors against 493, deaths from cold 18
against 27; fuel put on fires 4,136 times against 3,811.

## D-124: Day and night (2026-10-12)

**Decision:** A day lasts 240 simulated seconds (15 to a season); the last 3/8 of each is night.
At nightfall and dawn everyone's needs take the new weather: night is colder (exposure +1 per
period on top of the season's chill) and makes people sleepy (tiredness +1 per period while
awake). `Engine::is_night`, `disable_night` (`--no-night` in the study); saved with the world.
The viewer darkens the land at night (fading in at dusk and out at dawn, not drawn when days pass
in under a few real seconds), fires glow brighter, and the top bar says "night".
**Why:** The user asked for a real day and night, and to keep features that make the world harder.
**Consequences:** Over 10 years in 20 valleys: 487 survivors with nights against 510 without,
deaths from cold 34 against 18; 45% of sleeps now begin at night (night is 37% of the time).
Nights that are sleepy but not colder gave 496 and 15. Colder nights are kept, as asked.

## D-125: Show unknown places to those who look in need (2026-10-12)

**Decision:** A person shows a place someone is believed not to know (D-121) only to people around
who look in need (visible urgency at 75% of a need's threshold); otherwise it shares in turn as
before, including "I've already been over there".
**Why:** D-121 made people almost always point out unknown places, so the "explored over there"
gesture halved (1,393 to 794 in valley 9) and people re-explored the same ground: tiles visited
fell from 115 to 59 and the user saw people staying near where they started.
**Consequences:** Valley 9 back to 84 tiles over 2 years (72 without nights; nights keep people
nearer camp), valley 1 to 273 without nights (292 before D-121). Over 10 years in 20 valleys with
nights: tiles visited up 9% (8,431 against 7,749), survivors 472 against 487, deaths from cold 40
against 34: some of D-121's gain traded back for exploring.

## D-126: Faster ticks, same results (2026-10-12)

**Decision:** Hot lookups now avoid searching ordered maps cell by cell, without changing any
order that affects results:
- route search keeps the last terrain chunk it touched, fetches each expanded cell once, uses a
  stamped grid for its node index, and skips the placed-object check when there are no ponds;
- perception walks each row's placed objects and structures alongside the cells;
- `agents_in` scans only the overlapping buckets, then sorts by row, column and id;
- nearest-target searches check availability only for candidates that would beat the best;
- exploration works out the available cells once for every heading it tries.
The step-cost rule is shared (`sim_world::traversal_between`).
**Why:** The user asked for speed with identical results.
**Consequences:** 3-year studies run 1.5 to 1.8 times faster: valley 1 in 8.2 s (was 14.9 s),
valley 9 in 8.4 s (was 12.5 s). Output is byte-identical on valleys 1, 4, 9, 13, 21 and 30.

## D-127: Whose hut is whose (2026-10-12)

**Decision:** Each acquaintance record keeps the hut that person was last seen asleep beside: that
is where they live, as far as the watcher knows. A hut where someone outside the family who isn't a
friend lives is theirs: people don't sleep there unless freezing, and don't put things in it or
take food from it. Without a home of its own, a person's family's hut (partner's, then parent's,
child's, sibling's) is home. Late in autumn, someone who remembers a hungry winter carries food
home and puts it away (until the hut holds 24). The viewer names the residents in a hut's tooltip
and flies their family color on its roof. Acquaintances grow to 24 B, minds to 1,816 B; save version 9.
**Why:** The user asked for settlements and ownership: whose hut is whose, families sharing
homes, stocking a hut before winter. Ownership is a belief from what's seen, not a world fact.
**Consequences:** Over 10 years in 20 valleys: 467 survivors against 472, deaths from cold 41
against 40; 135 huts lived in by 446 people (141 by 453 before). Without the friends exception it
was 455 and 52 (people slept out rather than in a stranger's hut). Food is put away but still
hardly ever fetched back out.

## D-128: Farming from what people notice (2026-10-12)

**Decision:** Berries eaten on open ground (no bush within 2 cells) drop a seed that comes up as a
bush beside the eater with odds 1 in 4. The eater remembers the spot; seeing a bush bearing berries
within a cell of it later teaches it that seeds grow (`Crafts::planting`). Someone who knows plants
berries on a free cell beside it (new goal `Plant`) when within 8 cells of home, carrying 3 or more
food, with fewer than 4 food bushes in view, outside winter; anyone watching learns how. Nobody
starts out knowing. Minds are 1,832 B.
**Why:** The user asked for farming that begins with accidental sprouting and is learned by
noticing it, not given.
**Consequences:** Over 10 years in 20 valleys: 469 survivors against 467, deaths from cold 35
against 41; 646 people noticed seeds grow, 639 bushes were planted, watched 1,029 times. People
stay nearer home (tiles visited 7,414 against 8,161).

## D-129: Hides keep the cold off (2026-10-12)

**Decision:** New material `Hide` with property `warmth` 1. Carving meat from a carcass, someone
not yet wearing a hide takes its skin first (every time with a blade, one go in three by hand). A
carried hide is worn: the weather's chill on that person is 1 less (never below 0), applied at
every change of weather and when the hide is taken (`Engine::weather_for`). Worn things (blades,
hides: `MaterialProperties::worn`) are never set down or put away. Save version 10.
**Why:** The user asked for tools, hide clothing first, from what the world's things can do.
**Consequences:** Over 10 years in 20 valleys: 484 survivors against 469, deaths from cold 34
against 35; 51 people wear hides at the end. Before skinning came first, a full load of meat left
no room and nobody got one.

## D-130: Spears and baskets (2026-10-12)

**Decision:** Two made things, both from 2 wood, kept on the body like blades and hides:
- a `Spear` (shaped with a blade carried, `needs_edge`) adds 15 to a strike's chance to land;
- a `Basket` adds 6 to what its carrier can hold (`InventoryView::capacity`).
Whoever knows knapping, with an edge in hand and wood to spare, makes a basket, and if old enough
to hunt, a spear (`make_tool`; `apply_craft` makes the first of blade, basket, spear it lacks).
Material properties `strike`, `room`, `needs_edge`. Minds 1,840 B; save version 11. Founders no
longer start with words for made things (`Concept::is_made`): with 15 concepts in 16 lexicon
slots, inherited tool words crowded out words like water.
**Why:** The user asked for tools after hide clothing: spears and baskets.
**Consequences:** Over 10 years in 20 valleys: 491 survivors against 484, deaths from cold 26
against 34 (more food carried and stored). Before founders lost the tool words it was 502 and 21,
with 175 spears and 184 baskets made.

## D-131: Words for new things, built from old ones (2026-10-12)

**Decision:** Words grow with the world. Making a blade, basket or spear, or skinning a hide, with
others watching, the maker says the thing's word; with none, it makes one up, most often a compound:
its word for what the thing is made from (a hide: deer) with one of 6 endings (`VocalForm::compound`,
forms 32 and up; named like "kini-mo"). Watchers see the thing and hear the word, so they learn it;
a child now and then learns it with a vowel changed, as with other words (dialects drift). Lexicons
grow from 16 to 20 slots, minds to 1,888 B; save version 12.
**Why:** The user asked for language that grows: compound words, dialect drift.
**Consequences:** Over 10 years in 20 valleys: 127 people end with a word for a made thing, 63
different such words in use, all compounds; 497 survivors against 491, deaths from cold 24 against
26. With 16 slots the tool words crowded out place words: 470 survivors.

## D-132: Sown berries are a season's crop (2026-10-12)

**Decision:** What grows from a sown seed (planted, or dropped by someone eating) is marked sown: it
bears nothing until the season after it was sown, and every sown bush dies at the turn of winter.
Seeds take only in spring, and people plant only in spring. Wild bushes still live for years.
**Why:** The user saw planted bushes take over the land and asked that berries be planted in one
season and harvested in another.
**Consequences:** See D-133 for the numbers (measured together): 4,069 sown bushes withered over 10
years in 20 valleys. Learning to plant is rarer, since a sown bush only bears in summer.

## D-133: The dead rot until buried (2026-10-12)

**Decision:** A body lies where the person died. After a day it rots, and anyone within 2 cells
falls sick (thirst and tiredness up 1,500, at most once a day); after a season only bones are left,
after a year nothing. People bury bodies in view (new goal `Bury`), before chores: family and close
friends always, and anyone once it believes the dead are better buried (`Crafts::burial`), learned
by falling sick by one or watching a burial. The viewer draws bodies, rotting bodies, bones and
graves instead of every dead person forever. Minds 1,896 B; save version 13.
**Why:** The user saw bodies all over the place and asked for burial and sickness.
**Consequences:** With D-132, over 10 years in 20 valleys: 491 survivors against 497, deaths from
cold 31 against 24; 25 burials, 24 people sickened by a rotting body.

## D-134: Food goes bad unless cooked (2026-10-12)

**Decision:** Materials have `keeps_days`: berries 8, raw meat 3, roast 15, others forever. Each dawn
about one part in that many of what each person carries rots; in huts food keeps three times as
long. New material `Roast` (nutrition 7,000): someone who keeps fire, carrying raw meat with a fire
burning in view, goes and cooks it (new goal `Cook`); it and anyone watching expect roast to feed as
meat did, and the cook names it (a compound on the word for meat). Save version 14.
**Why:** The user asked that berries and meat go bad unless cooked.
**Consequences:** Over 10 years in 20 valleys: 441 survivors against 491. Deaths barely changed (39
against 42); people spend their time gathering (gathers 628,348 against 232,426, tiles visited
nearly double) and fewer children are born. Meat was cooked only rarely. Harsher keeping (berries 4
days, meat 2, roast 10, huts twice) gave 417.

