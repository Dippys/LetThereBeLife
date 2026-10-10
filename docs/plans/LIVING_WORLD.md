# Living world: properties, learned affordances, wildlife, construction

Status: active (started 2026-10-10). Follows the completed vertical slice
([`VERTICAL_SLICE.md`](VERTICAL_SLICE.md)).

## Goal

Make the world livelier without hand-writing behavior per thing. Things are described by
**properties** (how much they feed you, whether they make you sick, whether they move, whether
they hurt), and agents **learn** what things are good for from their own experience, from
watching others, and from their family's culture. New content should mostly be a new row of
properties, not new behavior code.

The hand-written parts stay small and physical: needs, the effects of properties on needs, and
generic actions (go, gather, eat, give, signal, flee, attack, place). Knowledge, categories, words,
and (later) structures are what emerge.

## Steps

### L1. Materials with properties ✅ (2026-10-10)

**Result:** `Material` (berries, bitter berries, wood, stone, meat) with `MaterialProperties`
(nutrition, toxicity, builds, regrowth time). The inventory is a count per material; eating applies
the properties (nutrition lowers hunger; toxicity raises thirst and tiredness). 30% of bushes are
bitter (world generator v3). Picked bushes grow back one unit per 10 minutes and trees one per hour,
computed lazily when someone looks (`--no-regrowth` makes a famine valley).

**Plan as written:**

- Replace the fixed resource kinds with `Material`s that have properties (nutrition, toxicity).
  Inventory holds a count per material.
- Add a **bitter berry** bush: it looks like food, barely feeds you, and makes you sick (thirsty
  and drained). About 30% of bushes are bitter.

### L2. Agents learn what's edible ✅ (2026-10-10)

**Result:** `Affordances` (3 B per material) change only from evidence: eating (felt), watching
someone eat and retch, being handed food, and family culture. One founding family takes bitter
berries for food and the other thinks they make you sick; a few founders don't know. Children start
with nothing, learn berries by watching (hundreds of watched meals per run), and taste what they
have never tried when hungry. Decisions, mimes (eating or retching), and readings all use beliefs.
While berries are plentiful nobody eats bitter berries; at 8% of the berries, believers ate them,
got sick, and the founders' beliefs shifted (6 food / 7 sickening → 3 / 10).

**Plan as written:**

- Each agent holds bounded beliefs per material: how much it feeds, how much it sickens, and how
  sure it is.
- Beliefs change only from evidence: eating something (the agent feels the effect), watching
  someone eat (and retch, which is visible), being handed food, and family culture.
- Founders inherit their family's food culture, which can be wrong (one family may think bitter
  berries are fine). Children start knowing nothing and learn from their parents.
- Decisions use beliefs: "hungry → go to whatever I believe feeds me". Mimes come from the
  sender's beliefs (eating mime for whatever it thinks is edible). Readings weigh candidates by the
  listener's beliefs, so pointing at a bitter bush with an eating mime can be believably
  misunderstood, acted on, and regretted.

### L3. Wildlife ✅ (2026-10-10)

**Result:** deer and wolves as species traits with one rule set (`wildlife.rs`): shy animals flee
people, wolves flee crowds of 3+, hungry wolves hunt deer and then lone people, calm animals drift
toward their kind, populations breed to a cap, and locally extinct species wander back in. Animals
act on their own schedule. People see animals up to 16 cells, hunt what they believe is prey (strike
range 2, each helper adds 20% to the odds, wounds slow animals), and butcher carcasses for meat,
which spoils where it lies. A wolf bite takes 15% of health (new `Injury` death cause); a full sleep
heals 15%. `Fauna` beliefs (3 B per species) come from bites (felt), seeing bites and kills, warnings,
and lore (one founding family fears wolves).

Warnings and calls to hunt: a new `Animal` topic with Snarl (danger) and Spear (hunt) mimes, shouted
to 16 cells. Listeners react with their own beliefs about the animal they read, keep a tip, and
check it once within 6 cells: another animal where one was pointed out relearns the word, teaches
danger if the speaker snarled, and plans a correction ("not deer: wolf"). A hungry listener hears an
animal call as a call to hunt.

In the valley (seed 1, 1.2M ticks): everyone survives; 15–45 deer killed by people per run, some
together; 10–55 bites; meat is a third of meals; fear of wolves spreads (founders 8 → 12–16, a few
children). Fixed along the way: wolves biting every 2 s (now one bite, then they lose interest),
dying of thirst while pacing after a vague tip (urgent needs now prefer places seen first-hand), and
endless chases (no hunting when worn out).

**Plan as written:**

- Animals are cheap creatures without minds: species with properties (speed, flees, hurts, meat
  yield). They graze, wander, breed slowly, and flee.
- Agents can hunt them (better together); a kill leaves meat, which spoils.
- A predator hurts people. Agents learn which species are dangerous from experience and from
  seeing others hurt, avoid them, and can warn others (a danger signal that can be misread).

### L4. Abstract construction

- Structures get their properties from the materials placed in them (wind-blocking, warmth).
  Shelter is "a place that blocks exposure", not a recipe; a hearth is a discovery others can copy.

## Measures

Run the behavior study before and after each step. Track survival, sickness events, how beliefs
spread (children's food knowledge), hunting success, injuries, and misunderstandings with
consequences (the success detector).
