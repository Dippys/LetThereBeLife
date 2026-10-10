# Plan: the first believable misunderstanding

_Written 2026-10-10. This is the active plan. It supersedes the "Next" list in
[`MINDS.md`](MINDS.md), which now records what was built on the way here._

## North star

The project's own definition of success
([`InitialDocumentation/16_IMPLEMENTATION_ROADMAP.md`](../../InitialDocumentation/16_IMPLEMENTATION_ROADMAP.md)):

> **An agent misunderstands a signal for a believable reason, acts on that misunderstanding, and
> both participants update future behavior using only observable evidence.**

Everything in this plan exists to reach that moment, in a small valley like the spec's first
vertical slice, and to be able to *explain* it afterwards ("instrumentation first").

## Where we start

| Spec requirement | What exists | Gap |
|---|---|---|
| Private intent separate from the public signal | Gestures point at a place; watchers infer a rough position | The receiver is still told the *kind* (water/food/explored). That's a hidden meaning channel and must go |
| Signals that are forms, not meanings | Only pointing | No vocal signals, no gaze, no emotional tone |
| Personal lexicon, learned from evidence | None | Everything |
| Receiver keeps a probability distribution over meanings | Hints have one confidence number | No competing interpretations |
| Success inferred later from consequences | Trust moves when hints are confirmed or abandoned | The sender never checks whether it was understood; no repair |
| Small social valley with 16 adults and 4 children | Study spawns on a 2,048² map; no children | No valley preset, no children |
| Explain why someone acted | Decision traces in the study | No communication log or explanation chain |

Kept and reused: mental maps and hints (`cognition/map.rs`), trust and relationships
(`social.rs`), personality, pointing gestures (`gesture.rs`), the behavior study, and determinism.

## Milestones

Each milestone ends with a study measurement and tests, like the work so far. Only one is active
at a time; update this file when one lands.

### M1 — Valley scenario and communication log ✅ (2026-10-10)

**Result:** `sim_core::find_valley` and `camp_sites` choose a livable 768² valley and a camp
beside water (seeds 1, 4, 7, 9, and 42 have one; seed 2's surroundings are ocean). In
`sim-headless --study --valley` and `sim-viewer --valley`, the band of 16 lives there.
Each gesture now has an id, a private intent, a public gesture, and one interpretation per
watcher, and hint outcomes are reported with their teller. `CommunicationLog` links them into
exchange chains (`--comms N`), and `--explain AGENT` tells one agent's story.

**What the log showed right away (seed 1 valley, 600k ticks):** 2,538 gestures, 2,020 receptions
that changed beliefs, **zero decisions based on a hint**, and 3 confirmed vs 24 abandoned hints.
Agents always prefer places they saw themselves (hints are penalized for uncertainty), and in a
valley everyone has seen water and food within minutes, so hearsay never drives behavior. Most
abandoned hints were **stale**: a sender pointing at a bush it saw earlier that has since been
eaten. Trust in such senders fell (one agent's trust in a teller dropped from 128 to 32). The
small survival gain earlier credited to sharing can't have come from hints. **M4 must make hints
matter in decisions.**


- A `--valley` preset for the study and the viewer: a small watered area (about 512–1,024 cells
  across, chosen deterministically per seed) and the spec's population of 16 adults. Children
  come in M7.
- A **communication log**: every exchange is recorded as sender intent → public signal →
  each receiver's interpretation → what each did → what each concluded later. The log is built
  only from engine diagnostics; it's never visible to agents.
- An "explain" output for one agent and tick: what it believed, which signal it heard, how it read
  it, and why it acted.

**Done when:** the valley runs headless and in the viewer, and the log captures today's gestures.

### M2 — Private intent and public signals ✅ (2026-10-10)

**Result:** `cognition/signal.rs` separates the sender's private `UtteranceIntent` (inform; topic;
exact place) from the `PublicSignal` (sender, origin, pointing gesture, a **mime**, and an
emotional **tone** derived from the sender's most pressing need). The engine completes a gesture
in two steps: `apply_signal` expresses the intent, then `deliver` hands watchers only the public
signal, and `understand(&PublicSignal)` is the sole path from signal to belief. A test delivers the
same public signal from a sender who knows a lake and from one who knows nothing, and requires
identical beliefs. Mimes stand in for words until M3 (scoop = water, pick-and-chew = food, chop,
strike, head-on-hands = shelter, arm sweep = "been there"). Behavior is unchanged: the valley and
group studies reproduce M1's numbers exactly, as expected while every mime has exactly one
reading. Tone is public but not yet used by receivers (M4).


- The sender keeps a private `UtteranceIntent { desired_effect, concept, place or target }`
  (desired effects to start: **inform** and **request**).
- Watchers receive only a public `Signal { vocal form(s), pointing, gaze target, emotional tone,
  intensity, origin }`. Remove `GestureTopic` from anything a receiver reads.
- Emotional tone comes from the sender's state (thirst, hunger, fatigue → urgency or distress)
  and is observable evidence.

**Done when:** a test proves no receiver code path can read the sender's intent, and agents behave
no worse than today in the study.

### M3 — Concepts and personal lexicons ✅ (2026-10-10)

**Result:** `cognition/lexicon.rs` adds 12 concepts, 32 abstract vocal forms (rendered as
syllables like "kani" for humans only), and a 192-byte personal lexicon per agent (16 entries of
12 bytes: form, concept, evidence for and against, times heard, uses that worked or failed).
Founders inherit a seed-specific convention with noise: about 1 in 10 concepts gets another form,
and about 1 in 16 gets an extra synonym. Senders now say their word with the point and the mime.
Watchers learn from hearing a word alongside a mime they understood (grounded, observable
evidence only). In the seed 1 valley, band agreement on each place word rose from **88% to 97%**
over 600k ticks with no global dictionary. Words don't drive interpretation yet (the mime does),
so behavior is unchanged. In the viewer, the hover card lists an agent's words and the HUD shows
what was said.


- A small concept set that this world can ground: **WATER, FOOD, SHELTER/HOME, COME, GIVE, ME,
  YOU, YES, NO**. DANGER waits until the world has hazards (see "Not yet").
- Signals are abstract vocal forms (ids, not English words).
- Each agent owns a sparse, bounded lexicon of `signal → concept` hypotheses with positive and
  contradictory evidence, kept separately for recognition and production (spec 06).
- A founding proto-language: founders inherit mostly shared associations with deliberate noise
  (a few agents link a form to a different concept or have two forms for one concept). There is
  no global dictionary; the "community language" exists only as overlap between individuals.

**Done when:** lexicons are compact (size asserted), deterministic, and inspectable in the viewer.

### M4 — Inference with competing interpretations ✅ (2026-10-10)

**Result:** `cognition/reading.rs` scores a bounded set of candidate concepts using only the
listener's own evidence: the mime (scooping and eating look alike, as do chopping and striking), its
own reading of the word, its thirst and hunger, what it already remembers near the indicated spot,
and the sender's urgency. Each reading keeps the top 3 candidates with probabilities and reason
flags (ambiguous mime, unknown word, word disagrees, need bias, memory bias). Hint confidence is
trust × the probability of the reading, and a likely runner-up is also kept if the listener urgently
needs it. To give hints real work:
- Places compete by expected cost (distance plus search effort, over belief), and food sightings go
  stale.
- A fresh hint may displace a stale first-hand memory.
- Agents stock up from remembered food.
- Curious agents check out the most promising unverified hint before wandering.
- The founding band is now **two families of 8** whose dialects differ on about a third of the
  concepts.

**Measured (seed 1 valley, 600k ticks):** 348 of 5,348 receptions misread (6.5%), every one with a
recorded reason (mostly ambiguous mime plus a word the listener's family uses differently), and **36
misreadings acted on**. Tip-based decisions went from 0 to 801. Survival is unchanged (249/300 across
the 15 study scenarios). Example from the log: agent 6 mimes eating and says "kani" (food, in its
dialect); agent 10, from the other family, reads WATER 58% / FOOD 40%, goes looking, and gives up.
Misreadings reinforce the wrong word (listeners learn from their own reading), so vocabulary
stays split (81% → 83%) until M5 corrects from consequences.


- Receivers generate a **bounded** set of candidate meanings: from their lexicon entries for the
  heard form, what's visible near where the sender points or looks, their own needs, recent
  events, the sender's tone, and their trust in the sender. They never compare against every
  concept.
- Candidates are scored and kept as a small fixed-point probability distribution (for example
  WATER 0.61, FOOD 0.29, other 0.10).
- Decisions use the distribution and the stakes: a 30% chance of water still matters to someone
  very thirsty.

This is where believable misunderstandings become possible. For example, a sender says a form
that the receiver links to both WATER and FOOD and points toward a pond ringed with berry bushes.
The receiver is hungry, so it reads FOOD, goes to eat, and the sender (who meant "water's there,
you look thirsty") sees it walk to the bushes.

Hints must also **compete fairly with first-hand memories** in decisions: a fresh, trusted hint
about a closer place should beat an old sighting far away (M1 found hints were never acted on).

**Done when:** the study shows misinterpretations happening at a plausible rate (neither zero nor
chaos), each with a recorded reason (ambiguous form, ambiguous context, need bias, low trust), and
`hint-decisions` is well above zero in the valley.

### M5 — Consequences, learning, and repair ✅ (2026-10-10)

**Result:** three observable-only learning paths.
- **Ask and repair:** a listener less sure than its threshold (cautious or sociable: up to 70%;
  bold or reserved: down to 45%; average about 58%) mimes its guess back with its own word for
  it. The speaker, who knows what it meant, nods or repeats with an exaggerated mime. Both
  update: the listener corrects the word, and the speaker records whether its word worked and
  learns the listener's word.
- **Consequences:** every hint carries the word it came with, the runner-up meaning the listener
  weighed, the pointing bearing, and the gesture id (places grew to 16 B). When a hint is checked,
  a confirmation strengthens the word. An abandoned hint relearns it only if the alternative the
  listener had weighed is in view, and stripped bushes in view explain an empty spot as stale
  (no lesson, no trust penalty). Searches follow the pointed line.
- **Corrections:** a listener who learned it had misunderstood later points back, says the
  speaker's word, and mimes "this, not that". Anyone who says that word for the real thing counts
  it as a failed use (a speaker can then drift to another word). If the speaker isn't watching,
  the correction waits.

Also: usage lessons (hearing someone use a word for something else), and two founding families
**camped apart** at their own water sources. Lexicons now converge through use and repair (seed 1
single camp: 81% → 94%; two camps: 75–90% by the end), and survival rose slightly (251/300).

### M6 — The success test ✅ (2026-10-10, with caveats)

**Result:** `CommunicationLog::success_episodes` follows each episode through gesture ids:
misread (with recorded reasons) → acted on → the listener learned from what it found there → the
speaker changed its belief about that word because of what that listener visibly did (a
correction or a use). `sim-headless/tests/success.rs` runs the seed 1 valley for 1.2M ticks and
requires at least one episode. It's a gate stage in `scripts/validate`. The first episode:

```text
1. t=232771 agent 5 pointed, mimed PickAndChew and said "kani", privately meaning FOOD.
2. Agent 12 read it as WATER because the mime looked ambiguous and the word meant something else to it.
3. At t=555343 it acted on that reading at the place.
4. At t=591816 it happened to find WATER there too, which convinced it "kani" means WATER.
5. At t=662294 agent 5 heard agent 12 use "kani" for WATER and now doubts it means FOOD.
```

**Caveat, measured honestly:** complete episodes are **rare**. There was 1 across 12 valley seeds at
2.4M ticks, and only seed 1 produces one. Most misunderstandings are repaired on the spot, and
those acted on are hard to discover: water and food co-locate at oases, and cross-family tips are
about faraway places. The test is deterministic but sensitive to behavior changes. If a change
removes the episode, investigate why with `--misreads` and `--lessons` rather than picking a new
seed.

**Since the living world (D-080):** the world was regenerated (bitter berries) and the detector
got stricter (the listener must act first; the speaker's lesson must concern the meaning at
stake). The test now runs the 16-adult seed 13 valley for 200k ticks, where agent 10 points out
bitter berries as food, agent 14 reads it as berries, goes, finds berries too, and agent 10 later
doubts its word after hearing agent 14 use it. **Before that, at M7:** the test ran the 16-adult
band, which still produced the episode above. The default valley now includes 4 children and has produced no complete
episode in 19 seeds × 2.4M ticks. The likely cause is that children ask about almost everything,
so misunderstandings get repaired before anyone acts on them, but this isn't proven: adding 4
agents also changes the whole deterministic run.


- The sender keeps a `PendingCommunication` with an expected outcome and checks it later: did the
  listener go there, come over, hand something over? Its estimate of success can itself be wrong.
- Both sides learn **only from what they observe**. The receiver strengthens or weakens
  `form → concept` when the place turns out to hold what it inferred, or something else. The
  sender adjusts its production familiarity and its trust or expectations of that listener.
- Repair: when the outcome doesn't match, the sender may repeat, exaggerate, point again, or
  demonstrate (walk there), and the receiver may make a "question" gesture. Repairs are strong
  learning evidence (spec 05).

**Done when:** lexicons converge between agents who talk often and stay different between those
who don't, measured as overlap over time in the study.

### M6 — The success test

- An automated detector over the communication log finds episodes where:
  1. the receiver's top interpretation differs from the sender's private intent,
  2. a believable reason is recorded,
  3. the receiver acted on its interpretation, and
  4. both participants later changed a lexicon entry, trust, or expectation **because of
     something they observed**.
- A regression test runs the valley scenario and requires at least one complete episode, with a
  human-readable explanation of each step.

**Done when:** that test passes. That is the project's definition of success.

### M7 — Requests, helping, and children ✅ (2026-10-10, COME deferred)

**Result:**
- **Children:** the valley band is now 16 founders plus 4 children (2 per family), who start
  with **no words**, begin with a close bond to a parent (familiarity 255, trust 220), go back to
  where they last saw that parent when it's out of view, take no excursions, and ask "this?"
  unless at least 90% sure. They learn only from what they observe. Across 19 seeds at 2.4M ticks
  they end up saying the founders' word for **62–95%** of place words (about 80% on average; seed 1
  reaches 79% by 600k ticks).
- **Requests (GIVE) and answers:** a hungry agent who carries no food and knows of none holds out
  a hand to someone in view (its parent first, then the most trusted and familiar), mimes eating,
  and says its word for food. The one asked reads that with its own words and needs, so a
  thirsty parent can take it as a request for water; the asker then repeats it with an
  exaggerated mime, and both learn. Answers are visible: food handed over (with a nod), a head
  shake, or empty hands. Parents feed their children unless starving themselves. Others keep
  their last meal and weigh trust, familiarity, sociability, the asker's urgency, their own food,
  and their own hunger. Being helped raises trust (+24); a refusal lowers it (−8).
- **Effect on survival** (`--food 5`, food doesn't regrow): helping carries more of the band
  through the first famine (seed 7: 14 vs 12 alive at 600k ticks; lower-quartile lifespan 185k →
  558k ticks; seed 13: 997k → 1,159k). Over longer runs it mostly evens out who starves: mean
  lifespan is about the same (seed 7: 1,475k vs 1,465k) or a bit lower (seed 1: 3,237k vs
  3,487k), because the best-supplied agents give food away. With food that doesn't regrow, sharing
  can only move meals around.
- **COME (regroup) is deferred.** Children following parents covers the slice's need to stay
  together; a spoken COME request is still to do.

Tests: `engine/tests/requests.rs` (asking, giving, refusing, empty hands, a parent feeding its
child after repairing a misread request) and the release-only `sim-headless/tests/slice.rs`.

**Plan as written:**

- **Requests:** COME (regroup), GIVE (help a struggling friend with food or water), and YES/NO
  replies, with refusal depending on personality, trust, and the giver's own needs.
- **Children** (the spec's 4): start with no lexicon and learn by joint attention and
  consequences (spec 06 developmental stages, simplified), from parents and others nearby.

**Done when:** a child acquires a working subset of the valley's forms purely from observation,
and helping changes survival in a scarce-food valley.

## Not yet (deliberately)

- **World scale and terrain:** no more work on the 65,536² world, the archive, or generation unless
  the valley needs it. The spec explicitly warns against "overbuilding procedural terrain before
  proving the social simulation".
- **10-million-agent optimization:** keep data compact and bounded, but don't optimize for scale
  before M6.
- **DANGER, hazards, animals, fire:** add them only when a milestone needs a warning to be
  grounded.
- **Grammar, sound change, dialects, deception:** after the vertical slice. The lexicon and
  pending-communication design must leave room for them.

## Rules for this plan

- **No hidden meaning channel.** Receivers read public signals only. Tests enforce it.
- **Bounded everything:** candidate sets, lexicon sizes, pending communications.
- **Deterministic and explainable:** every interpretation and every update has a recorded reason.
- **Measure each milestone** with the study, before and after.
