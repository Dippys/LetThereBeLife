# Plan: learning what works

Status: phases 1-2 done (D-103, D-105, 2026-10-11). Replaces most hand-written decision rules with choices people learn
from their own experience and from watching others.

## Why

Agents already learn *facts* from evidence: what is edible, which animals are dangerous, words,
how to make fire, knap a blade, or build a hut. But *what to do* is a script in
`policy/deliberate.rs`. Every new challenge (a carrying limit, storage, winter) has meant another
hand-written rule, and the D-102 batch was mostly patches. The goal is a world with challenges where
people work out solutions themselves, and solutions spread as culture.

## What stays fixed, and what is learned

| Stays hand-written | Becomes learned |
|---|---|
| The world: terrain, water, cold, seasons, regrowth, loads, effort | Which option to take, in which situation |
| Bodies: needs, health, collapse, death | When to stock up, store, fetch, build, tend a fire |
| Perception, movement, routing | Where to sleep, whom to follow, when to explore |
| Knowledge of *how* (crafts, words): already learned by observation | |
| A few reflexes: drink when dying of thirst, flee when bitten | |

## How a decision works

1. **Options.** From what it perceives and carries, the agent lists what it physically can do now:
   drink at water in view, eat something carried, pick or chop something in view, store or fetch at a
   hut in view, sleep here or in a hut, warm up at a fire, feed a fire, build a hut or hearth it knows
   how to make (or gather toward one), go to a remembered place, follow or visit someone, explore,
   point something out, or rest. This only says what is possible; it prefers nothing.
2. **Situation.** A compact description of how the agent is: which needs are past their thresholds
   (4 bits), the season (2 bits), and whether a shelter or a fire is in view (2 bits): 256 situations.
3. **Values.** Each agent keeps a small table of how well each kind of option has gone in each kind
   of situation (integer estimates, bounded memory: a sparse set of entries, about 128 bytes).
4. **Choice.** It picks the option with the best value, plus a small curiosity bonus for options it
   has rarely tried (a deterministic roll; curious personalities explore more). Ties break by a
   fixed order.
5. **Learning.** When the action ends, the agent compares its discomfort before and after (how far
   its needs are past comfortable, weighted by urgency). Relief raises that option's value for that
   situation; no relief or harm lowers it. Integer moving averages, so recent experience counts more.
6. **Copying.** When an agent watches someone take an option and visibly get relief (eating,
   warming hands, leaving a hut rested), it nudges its own value toward that option, more for people
   it trusts, and children most for their parents. Good habits spread; bad ones die with whoever
   held them.

Everything uses only what the agent perceived or felt (no reading the world's truth), stays
deterministic (seeded rolls, ordered iteration, integer maths), and every choice can be explained
("slept in the hut: eased the cold 7 times of 8").

## Longer plans

Some solutions take several steps: a hut needs 10 wood gathered first, and pays off only later,
when it shelters someone from the cold. Phase 1 learns choices between immediate options only.
Later phases add **projects** (gather toward a hut or hearth, stock a hut with food): a project is
one option whose value is credited when its result is used (sleeping in the finished hut, eating
the stored food), with the credit decaying the longer that takes.

## Phases

1. **Shadow mode.** Build options, situations, values and learning, but keep the current script in
   charge. Log what the learned choice *would* have been, and how values converge. No behaviour change.
2. **Cold, shelter and storage** decided by learning (where the patches piled up). Measure survival,
   fires kept, food fetched in winter, against the scripted version.
3. **Food and water** by learning, keeping the dying-of-thirst reflex.
4. **Copying** between people, and parents teaching children.
5. **Projects**, then remove the script except reflexes.

## How we'll know it works

- Survival over 1 and 30 years comparable to the scripted version (or better), across 40 valleys.
- Behaviour nobody wrote appears and is traced to experience: fires fed through winter, food stored
  in autumn and fetched in winter, people wintering in huts.
- Valleys differ: different habits arise and persist in different bands (culture).
- Episodes of misunderstanding stay common (the success test).

## Costs and risks

- Early generations make more mistakes and die more often, because they really must discover
  things. Reflexes and parents keep the first steps survivable.
- Per-agent memory grows by about 128 bytes (from 984), within the compact-data rule.
- Learning can settle on odd habits; curiosity and copying from successful neighbours counter that.
