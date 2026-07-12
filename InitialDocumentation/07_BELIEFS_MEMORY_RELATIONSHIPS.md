# Beliefs, Memory, and Relationships

## Separate truth from belief

The simulation distinguishes:

1. Objective world state.
2. What an agent perceived.
3. What it remembers.
4. What it currently believes.
5. What it believes other people know.
6. What it communicates.
7. What listeners interpret.

Example:

```text
Reality:
  Bandits are near the river.

Agent A:
  believes bandits are near the river.

Agent B:
  believes bandits are in the forest.

Agent C:
  knows nothing.

Agent D:
  knows the river report but lies about it.
```

## Belief structure

```rust
struct Belief {
    proposition: Proposition,
    confidence: u16,
    source: BeliefSource,
    acquired_at: SimTime,
    last_confirmed: Option<SimTime>,
    emotional_weight: i16,
}
```

Sources:

- Direct perception.
- Inference.
- Communication.
- Cultural teaching.
- Memory reconstruction.
- Rumor.
- Institutional record.

## Belief update

A communicated belief may be weighted by:

- Interpretation confidence.
- Trust in speaker.
- Perceived speaker knowledge.
- Plausibility.
- Existing evidence.
- Corroboration.
- Emotional bias.
- Cultural prejudice.
- Consequence of being wrong.

## Belief decay

Different beliefs decay at different rates:

- Exact mobile-object location: rapid.
- Routine schedules: moderate.
- Kinship: very slow.
- Cultural taboo: very slow.
- Trauma: persistent but possibly distorted.
- Rumor: depends on repetition and source credibility.

## Memory layers

### Working memory

- Detailed recent observations.
- Current conversation.
- Active plan.
- Seconds to minutes.

### Episodic memory

- Important events.
- People and locations.
- Emotional consequences.
- Novel or contradictory experiences.

### Semantic and social summaries

- Person is reliable.
- Northern road is dangerous.
- This signal often means food.
- Household owes a favor.
- Winter is usually harsh.

## Forgetting and consolidation

Routine events should not remain forever.

```text
Detailed event
    ↓
Short-term retention
    ↓
Importance evaluation
    ↓
Discard, consolidate, or preserve
```

Importance factors:

- Emotional intensity.
- Novelty.
- danger.
- social importance.
- relevance to goals.
- repetition.
- contradiction.
- identity relevance.

Example:

```text
27 ordinary successful trades
3 late deliveries
1 attempted deception
```

May consolidate into:

```text
Familiarity: high
Trade reliability: moderate-high
Trust: moderate
Suspicion: present
```

The attempted deception may remain as a specific episode.

## Relationship records

Store only meaningful ties.

Possible fields:

- Familiarity.
- Trust.
- Friendship.
- Affection.
- Fear.
- Respect.
- Authority.
- Loyalty.
- Resentment.
- Debt.
- Romantic attachment.
- Family role.
- Reputation.
- Last interaction.

No all-pairs matrix is allowed.

## Stranger priors

Before a detailed relationship exists, use group priors:

- Rival tribe.
- Same occupation.
- Kin of friend.
- Healer.
- Criminal reputation.
- Foreign accent.
- Shared religion.

Personal experience should override priors over time.

## Important remembered interactions

Examples:

- This person lied to me.
- This person saved my child.
- This person taught me a word.
- This person repeatedly misunderstood me.
- This person broke a promise.
- This person publicly insulted me.

These events can influence communication interpretation and cooperation.

## Beliefs about minds

Advanced agents may track:

- What another person probably knows.
- What signal they probably understand.
- Whether they noticed an event.
- Whether they are likely to be lying.
- Whether they understood a previous request.

These models should be sparse and approximate.
