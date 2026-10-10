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

### L1. Materials with properties

- Replace the fixed resource kinds with `Material`s that have properties (nutrition, toxicity).
  Inventory holds a count per material.
- Add a **bitter berry** bush: it looks like food, barely feeds you, and makes you sick (thirsty
  and drained). About 30% of bushes are bitter.

### L2. Agents learn what's edible

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

### L3. Wildlife

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
