# Plan: agents that think, and lives that aren't predetermined

Status: T0 (D-114), T1 (D-115) and T2 (D-116, D-118: plans for cold, on) done, 2026-10-12.
D-108) and answers the owner's brief: agents react instead of thinking, and every run of a seed
replays the same lives.

## The problem

- **What agents do is scripted.** `policy/deliberate.rs::deliberate` takes the most urgent need
  and walks a fallback list we wrote (thirst: water in view, remembered water, explore, wait).
  Learned habits re-rank options only when tired or cold, and foresight (D-108) looks one step
  ahead over two facts.
- **Nothing is held in mind.** Each decision starts from scratch; the only things that persist
  are a trip destination and a pending gesture.
- **Life is predetermined.** One seed drives terrain and every roll in every life, so seed 1
  always replays the same history.

What agents *know* is already learned from evidence (edibility, danger, words, trust, crafts).
This plan makes what they *do* come from what they know.

## Not wanted

No neural network and no language model per agent. Minds stay readable ("I'm gathering wood
because a fire needs it and I'm cold"), compact (a few hundred bytes more per mind), and grounded in
what each agent experienced or watched.

## Part 1: lives that aren't predetermined

- **World seed:** fixed (as today's `seed`). Terrain and the plants and rocks it generates are
  strictly deterministic.
- **Life seed:** drives people, personalities, animals, plant death and spread, and every roll in
  the engine. The viewer draws a new one from the clock every run; `--life-seed N` replays a run.
- **Recorded:** saves, study reports and `--trace` output carry the life seed, so a run can be
  reproduced exactly. Same world seed, life seed and commands give the same history.
- **The headless study** defaults to a life seed derived from the world seed, so comparisons
  between code versions stay paired (same histories on both sides) and far less noisy; a flag
  samples random life seeds when we want many histories.
- Rule 3 in AGENTS.md and ARCHITECTURE.md are rewritten: world generation strictly
  deterministic; life deterministic given its seed (fixed ticks, integer rules, ordered
  iteration) so saves, replays, tests and bug reports keep working. Strictness inside life can be
  relaxed later if a better mind needs it.

## Part 2: agents that think

### Facts

A person's circumstances, as it perceives them, in 16 bits: the four needs pressing (hungry,
thirsty, tired, cold), what's at hand (water, food to pick, a burning fire, a shelter, a dead fire
it could feed), and what it carries (food, wood, stone, a blade). Every fact comes from perception
or the agent's own body, never from world truth.

### A cause-and-effect model

For each kind of action (today's 18 `Choice` kinds), each person learns:

- **what it needs:** the facts that held every time it worked (start from the facts present the
  first time, intersect on each later success; a 16-bit mask per action);
- **what it produces:** how often each fact became true when it didn't hold before (counts per
  action and fact, as D-108 does for two facts);
- **what it relieves:** which need it eased and by how much (today's habit values);
- **how long it takes:** a running average.

It learns from its own actions and from watching others (watching already teaches crafts and
food). Founders start with instincts as weak prior counts ("drinking at water eases thirst",
"eating what you carry eases hunger"), which is the scripted order turned into beliefs that
experience can overturn. About 350 bytes per mind, replacing D-108's 72; the mind goes from 1,192
to about 1.5 KB.

### A small planner

When a need presses, or a step finishes or fails, the person searches backwards through *its
own* model: an action that relieves the need; if it needs facts that don't hold, actions that
produce them; and so on, up to 4 steps. Each plan is scored by expected relief per expected time,
weighted by how reliably each step has worked. The search is bounded (a few hundred nodes) and runs
only when an intention is formed or revised, not every tick.

"warm ← warm up at a fire ← fire burning ← feed the fire ← carrying wood ← gather at the grove I
remember." A plan built on a wrong belief fails for a believable reason, which is the north-star
test.

### One intention

Each person holds at most one intention: the goal, up to 4 steps, which step it's on, why it was
formed, and when (about 12 bytes). Each decision it carries out the current step with the
existing primitives (walk to a remembered place, gather, feed a fire, build, sleep in a hut). The
step is done when the fact it should produce becomes true. The intention is:

- **revised** when a step fails (a stall, nothing to gather at the remembered grove, the fact
  never comes): the belief behind it is weakened, then it replans;
- **set aside** when a more urgent need takes over (thirst while gathering wood), and taken up
  again if it still applies;
- **dropped** when its goal no longer matters (it warmed up some other way).

Reflexes stay outside the planner: flee when bitten or a wolf is close, drink when dying of
thirst, sleep where you stand when collapsing.

### Seeing it

The person panel gets a "Plan" line in plain words ("Plan: get wood, light the fire, warm up"),
`--trace` prints intentions formed, steps done, revisions and why, and the study counts plans
formed, completed, revised and dropped, with reasons.

### Later

- **Anticipation:** people remember the seasons they've lived through (how cold winter was,
  whether food ran short) and form intentions before the need ("winter is coming: stock the hut").
- **Other minds:** beliefs about what others know, learned from what they've seen others see and
  say ("she wasn't there when I found the water, so I'll show her").

## Phases

| Phase | What | Behaviour change |
|---|---|---|
| T0 | Split world and life seeds; record the life seed; rule 3 rewritten | Different lives every viewer run |
| T1 | Facts and the cause-and-effect model, learned silently from acting and watching; shown in `--trace` and the study | None |
| T2 | Planner and one intention for cold (fire and shelter chains), behind `PolicyOptions::planning`; Plan line in the panel | Cold handled by plans |
| T3 | Hunger and thirst by plans; the scripted order becomes instinct priors | Food and water by plans |
| T4 | Calm time by plans: storing, building, crafting, visiting | Most of `deliberate` replaced |
| T5 | Anticipation from lived seasons | Preparing for winter |
| T6 | Beliefs about what others know | Showing and teaching on purpose |

Each phase is measured before it's switched on by default.

## How we'll know it works

- **Survival not worse than the script** beyond noise: 40 valleys, 10 years, paired life seeds.
  Early phases may cost lives; a phase stays off until it's at least level.
- **Behaviour nobody wrote, traced to beliefs:** wood gathered because a fire was wanted, fires fed
  through winter, food stored before winter (T5), plans revised after a belief proved wrong.
- **Readable:** any agent's current plan and the beliefs behind it can be printed in a few lines.
- **The success test still passes**, and plans that fail on a misread word add to its episodes.

## Risks

- **Survival dips** while plans are worse than the tuned script; instinct priors and the
  phase-by-phase switch limit that.
- **Thrashing** between plans; an intention only changes on failure, a more urgent need, or a much
  better plan.
- **Cost** at scale: planning only on change, a bounded search, and compact masks; 10M agents
  would plan rarely and reuse intentions.
- **Noise in measurement:** today's runs vary by about ±10 survivors in 20 valleys; paired life
  seeds and behaviour counts matter more than survival totals.
