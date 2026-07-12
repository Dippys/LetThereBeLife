# Debugging and Analytics Tools

## Simulation inspector

For any NPC, show:

- Identity and age.
- location.
- household and group memberships.
- needs.
- emotions.
- current goal.
- current plan.
- beliefs with confidence and source.
- important memories.
- relationships.
- known signals.
- recent communication.
- next scheduled events.
- recent decisions and utility scores.

## Decision explanation

Every major action should produce a compact explanation:

```text
Selected goal: Help child

Utility:
  child danger             +0.82
  parental attachment      +0.75
  distance                 -0.10
  current hunger           -0.08
  fear                     -0.14
  total                     1.25

Rejected:
  Continue farming          0.31
  Seek shelter              0.74
```

## Communication inspector

Show:

- Sender's private intent in debug mode.
- Public signal event.
- Receiver observations after noise.
- candidate interpretations.
- score contributions.
- selected response.
- later outcome.
- learning updates.

Production builds may hide private intent from the player if desired.

## Language history

For each signal:

- Originator.
- first usage.
- original context.
- meaning probabilities by year.
- age distribution.
- community distribution.
- variants.
- descendants.
- borrowings.
- prestige spread.
- decline or extinction.

Example:

```text
Signal /kama/

Year 1:
  FOOD 72%
  GIVE 18%
  UNKNOWN 10%

Year 8:
  FOOD 48%
  MEAL 31%
  HUNGER 14%
  UNKNOWN 7%
```

## Society analytics

- Population.
- births and deaths.
- household size.
- migration.
- occupation distribution.
- food security.
- wealth.
- trust networks.
- faction membership.
- conflict.
- crime.
- trade.
- language diversity.
- bilingualism.
- mutual intelligibility.
- settlement growth.

Analytics are observational tools. They do not need to replace individual simulation.

## Causal trace

Important events should link causal parents:

```text
War declaration
← leader believed raid report
← report from scout
← scout misunderstood foreign warning
← ambiguous gesture during trade meeting
```

A causal graph is extremely valuable for debugging emergence.

## Time controls

- Pause.
- single event.
- single agent thought.
- single region step.
- fixed number of events.
- accelerated time.
- rewind to snapshot.
- replay.
- breakpoint on event type.
- breakpoint on agent.
- breakpoint on signal.
- breakpoint on threshold.

## Performance inspector

Track:

- Events per second.
- thoughts per second.
- time by system.
- cache misses where available.
- allocation.
- pool fragmentation.
- average agent bytes.
- loaded chunks.
- path requests.
- communication fan-out.
- interpretation candidates.
- event queue depth.
- region imbalance.

## Validation overlays

- Passability.
- chunk boundaries.
- region ownership.
- hearing radius.
- line of sight.
- active signals.
- route graph.
- resource density.
- settlement claims.
- language prevalence.
