# Cognition and Decision-Making

## Recommended hybrid

Use:

- Reactive rules for immediate danger and bodily interruptions.
- Utility AI for selecting goals.
- Behavior trees or compact GOAP for executing selected goals.
- Event-driven reconsideration rather than continuous full cognition.

## Decision pipeline

```text
Trigger or scheduled event
        ↓
Update relevant perceptions
        ↓
Update affected beliefs
        ↓
Evaluate changed needs and emotions
        ↓
Generate relevant goals
        ↓
Score goals
        ↓
Select or retain plan
        ↓
Act, move, or communicate
        ↓
Observe consequences later
        ↓
Learn and reschedule
```

## Thoughts are events

A "thought" should be a bounded computation caused by something relevant.

Triggers include:

- Need threshold reached.
- Someone speaks.
- New object perceived.
- Danger appears.
- Work shift begins.
- Social commitment is due.
- Child cries.
- Journey reaches a waypoint.
- Weather changes.
- Plan fails.
- Memory is recalled.
- Agent wakes.

A thought should not scan the agent's entire mind.

## Relevant-state retrieval

A scream may require:

- Relationship with the source.
- Current safety belief.
- Current task.
- Distance and direction.
- Recent threat memories.
- Fear and courage.
- Nearby dependents.

It should not require loading farming vocabulary or every childhood memory.

## Goal utility

Example:

```text
utility(goal) =
    need urgency
  + expected benefit
  + emotional drive
  + social obligation
  + authority pressure
  + habit
  + personality fit
  - effort
  - danger
  - opportunity cost
  - uncertainty
```

Weights may vary by personality, culture, age, and experience.

## Communication requests as goal candidates

An interpreted command or request becomes:

```rust
struct PerceivedRequest {
    believed_sender: Option<AgentId>,
    requested_action: ActionTemplate,
    interpretation_confidence: u16,
    perceived_authority: i16,
    perceived_urgency: u16,
}
```

Compliance utility may include:

- Understanding confidence.
- Trust.
- Friendship.
- Loyalty.
- Authority.
- Fear.
- Debt.
- Reward.
- Social norm.
- Fatigue.
- Hunger.
- Workload.
- Rebellion.
- Risk.
- Conflicting goals.

Possible responses:

- Accept.
- Reject.
- Ignore.
- Delay.
- Ask for clarification.
- Negotiate.
- Delegate.
- Pretend not to understand.
- Comply from fear.
- Comply from loyalty.
- Misunderstand and perform a different action.

## Planning

GOAP should use a small relevant action set rather than searching the entire world.

Example goal: acquire food.

Relevant actions:

- Eat carried food.
- Ask household member.
- Visit storage.
- Gather berries.
- Hunt.
- Trade.
- Steal.
- Wait for meal.

The action set depends on:

- Location.
- Skill.
- culture.
- Relationships.
- Law.
- Inventory.
- Known opportunities.
- Time.
- Risk.

## Plan invalidation

A plan should be reconsidered when:

- A required resource disappears.
- A path becomes blocked.
- Danger crosses a threshold.
- A stronger need emerges.
- The target refuses.
- Communication is misunderstood.
- Weather changes.
- A scheduled obligation becomes urgent.
- A trusted new belief arrives.

## Attention

Not every signal reaches cognition.

Attention depends on:

- Distance.
- Sound intensity.
- Visibility.
- Current task.
- Fatigue.
- Age.
- relationship.
- expectation.
- emotional salience.
- crowd noise.
- deliberate avoidance.

## Emotion

Emotions can modify:

- Goal utilities.
- Interpretation bias.
- Memory retention.
- trust updates.
- risk tolerance.
- communication tone.
- attention.

Fear may increase the probability of interpreting an ambiguous signal as danger. Anger may increase perceived hostility. Affection may increase compliance.

## Metacognition

Advanced agents may model:

- What another person probably knows.
- Whether a listener understood.
- Whether a speaker may be lying.
- Whether clarification is needed.
- Which signal a child or foreigner recognizes.

This ability should vary by age, cognition, experience, and stress.
