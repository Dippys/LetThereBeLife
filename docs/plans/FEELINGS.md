# Plan: feelings, temperament and talents

Status: P1-P6 done (D-137, D-143, D-144, D-145), 2026-10-12. Taking food from others' huts is
built but hasn't happened in measured runs. The owner asked for feelings (sadness, envy and the rest),
everything they need, and personalities that make each person unique and better at some things
than others.

## Rules every feeling follows

1. **A cause in the world:** something the person perceived or felt (a bite, a gift, being misled,
   a hungry winter remembered). Never a timer or a dice roll on its own.
2. **An effect on what they do:** each feeling shifts choices that already exist (stay near
   others, share, refuse, explore, stock, teach) or the new ones below. No decorative moods.
3. **A sign others can see:** the strongest feeling shows as an expression (posture, pace, tone).
   Others read the expression, never the feeling, and can misread it (rule 5: no hidden
   meaning channel).
4. **Compact and fading:** a few bytes per person, faded lazily from the time last updated (no
   per-tick scans), deterministic given the life seed.

## Temperament (P1)

Today's four traits (curiosity, caution, sociability, diligence) gain four that tune feelings:

| Trait | High means |
|---|---|
| Boldness | frightened less easily, calms faster |
| Warmth | grateful and affectionate readily, slow to anger |
| Temper | angered easily and strongly |
| Steadiness | feelings fade faster, worries less |

Children inherit the average of their parents' traits plus their own variation (from the life
seed), so families have a likeness and every person is still unique. Traits also drift slowly
with what life brings, within a bounded distance of where they started: repeated frights make
someone timid, plans that keep working make them bolder, betrayals shorten their temper, long
company warms them.

## Talents and skills (P1)

Activities: foraging, hunting, crafting (knapping, spears, baskets, hides), building, cooking,
farming, wayfinding, and words. For each, a person has an inborn **talent** (inherited like
traits) and a **skill** that grows with practice and fades slowly without it. Together they set
how fast, how much and how reliably the person does it: gather yield, hunting strikes, craft and
build time, cooking time, sowing success, how far they notice things, how quickly words stick.
Learned habits already weigh relief against time, so a good hunter finds hunting pays and leans
toward it: a division of labour can arise without being assigned. People notice who's good at
what by watching.

## Feelings

**About body and surroundings (P2):**
fear (a wolf close, a bite, a death seen, dark and alone: caution, staying near others,
avoiding the place), calm (home, fire, family: roaming wider), contentment (fed, warm, rested,
among family: sharing, teaching), misery (long hunger or cold: sharing less, snapping), boredom
(nothing new: exploring, trying things), loneliness (long without company: seeking people), grief
(someone close dies: withdrawal, staying near others; today's `grief_until`, folded in).

**Toward particular people (P3), kept in each acquaintance slot:**
gratitude (given food, shown water: trust, giving back), anger (misled, refused, robbed: refusing,
avoiding, confronting), contempt (anger that keeps being renewed: shunning), affection (time
together, help: following, sharing, choosing a partner).

**About time, plans and learning (P4):**
disgust (rot, sickness, an unburied body: avoiding, burying, others learn by seeing the recoil),
worry (a remembered hard season coming, low stores: stocking; this is what switches on getting
ready for hard seasons, D-120), hope (a plan going well: persisting), frustration (plans failing
again and again: dropping them sooner, trying something else), surprise (a belief proved wrong:
learning faster from it), relief (a need or danger ending: the habit that ended it counts more).

**Pride and shame (P5):** pride (something made or built while watched: showing and teaching it,
working harder at it), shame (corrected in front of others, refused, caught taking: speaking
less, avoiding the witness).

**Envy, jealousy and their consequences (P6):** envy (seeing someone with food, a spear or a hut
while lacking it: gathering more, copying, and when hungry and not fond of them, taking from
their hut), jealousy (a partner often close to someone else: staying near the partner, anger at
the rival).

## Subsystems they need

- **Expressions (P2):** each person's strongest feeling (and whom it's toward) is part of how
  others perceive them. Watchers read it with familiarity and distance, and can mistake fear for
  anger or grief for tiredness. Seeing fear spreads a little fear; seeing grief draws people
  closer; anger aimed at you makes you keep away.
- **Taking things (P6):** from another family's hut store. Anyone who sees it knows who did it;
  an owner who wasn't there only finds less in the hut.
- **Confrontation (P6):** an angry person facing whoever wronged them: a raised voice and
  posture. The other backs down (gives back food, leaves, feels shame) or answers in anger; a
  scuffle bruises both. Rare, and never deadly by itself.
- **Partner choice (P3, P6):** couples form from affection rather than only nearness; jealousy
  needs it.
- **Seeing it:** the person panel says how they feel in plain words ("frightened", "angry with
  Ana", "proud of the spear"), signs over people show it, `--trace` logs what caused each feeling,
  and the study counts feelings, expressions misread, thefts and confrontations.

## Phases

| Phase | What |
|---|---|
| P1 | Temperament (8 traits, inherited), talents and skills (inherited, practised) |
| P2 | Feelings store; fear, calm, contentment, misery, boredom, loneliness, grief; expressions |
| P3 | Gratitude, anger, contempt, affection toward people; partner choice from affection |
| P4 | Disgust, worry, hope, frustration, surprise, relief |
| P5 | Pride, shame |
| P6 | Envy, jealousy; taking from huts, witnesses, confrontation |

Each phase is measured over 10 years in 20 valleys (survival, deaths by cause, births, plus what
the feelings change) and kept on even if it costs lives, with the numbers recorded: the owner
prefers a harder, more interesting world.

## How we'll know it works

- Different people in the same situation do different things, for reasons the panel can show.
- Feelings change behaviour measurably: the frightened stay close, the angry share less with the
  one who wronged them, the worried stock up, the skilled do more of what they're good at.
- Misread expressions happen and lead somewhere (a frightened face taken for anger).
- Minds stay bounded: about 60 more bytes per person.
