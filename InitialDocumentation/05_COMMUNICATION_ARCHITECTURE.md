# Communication Architecture

## Private intent and public signal must be separate

Do not create a receiver-visible object containing both a signal and the true meaning.

### Private sender state

```rust
struct UtteranceIntent {
    sender: AgentId,
    target: Option<AgentId>,
    desired_effect: DesiredEffect,
    proposition: Option<Proposition>,
    expected_outcomes: SmallVec<[ExpectedOutcome; 4]>,
}
```

### Public observable event

```rust
struct SignalEvent {
    sender: Option<AgentId>,
    vocal_sequence: SmallVec<[SignalId; 6]>,
    gestures: SmallVec<[GestureId; 4]>,
    gaze_target: Option<EntityId>,
    pointed_target: Option<EntityId>,
    emotional_tone: EmotionVector,
    intensity: u16,
    origin: WorldPosition,
    timestamp: SimTime,
}
```

Only the public event is delivered to potential listeners.

## Signal modalities

Signals may include:

- Sound patterns.
- Whistles.
- Gestures.
- Facial expressions.
- Posture.
- Body movement.
- Eye direction.
- Pointing.
- Visual symbols.
- Written markings.
- Drumbeats.
- Smoke.
- Object placement.
- Clothing or insignia.
- Demonstration.

A communication event may combine several modalities.

## Signal production

1. The sender determines a desired effect.
2. It identifies relevant concepts and semantic roles.
3. It estimates what the listener knows.
4. It searches known signals and constructions.
5. It estimates ambiguity and social cost.
6. It adds gesture, gaze, or demonstration.
7. It physically produces a noisy signal.
8. It retains the private intent for later outcome evaluation.

Possible desired effects:

- Inform.
- Request.
- Warn.
- Ask.
- Command.
- Reassure.
- Deceive.
- Express emotion.
- Establish attention.
- Threaten.
- Promise.
- Negotiate.

## Production utility

```text
signal utility =
    estimated listener understanding
  + speaker familiarity
  + local conventionality
  + contextual clarity
  + emotional expressiveness
  - production difficulty
  - ambiguity
  - taboo cost
  - social risk
```

## Receiver processing

1. Detect signal.
2. Test attention.
3. Identify source if possible.
4. Apply hearing or visual noise.
5. Read context.
6. Load personal lexical associations.
7. Load relevant constructions.
8. Generate candidate interpretations.
9. Score candidates.
10. Retain probability distribution.
11. Update beliefs.
12. Decide whether and how to respond.
13. Learn later from consequences.

## Interpretation evidence

```text
interpretation score =
    lexical association
  + construction probability
  + visible context
  + speaker identity
  + gaze
  + pointing
  + emotional tone
  + recent events
  + relationship
  + cultural background
  + current shared activity
  + expectation
```

Use bounded candidate generation. Candidates should come from:

- Existing signal associations.
- Visible objects and people.
- Recent actions.
- Current needs.
- Current activity.
- Shared plans.
- Known constructions.
- Speaker model.

Never compare every signal with every concept.

## Uncertainty

Receivers may retain:

```text
Come inside:       0.61
Stay close:        0.23
Danger nearby:     0.11
Unknown:           0.05
```

Action policy can depend on confidence and consequences.

A low-probability danger interpretation may still cause precaution.

## Clarification and repair

Possible repair actions:

- Repeat.
- Slow or exaggerate.
- Point.
- Demonstrate.
- Use a synonym.
- Use a descriptive sequence.
- Ask a third party.
- Produce a question gesture.
- Partially perform the suspected action.
- Refuse until clearer.
- Confirm or deny a guess.

Repair interactions create strong learning evidence.

## Communication success is inferred later

Neither agent instantly knows whether understanding succeeded.

The sender observes consequences:

```text
Wanted: listener brings food.
Observed: listener enters storage and returns with grain.
Inference: communication probably succeeded.
```

A pending communication record may contain:

```rust
struct PendingCommunication {
    private_intent: UtteranceIntent,
    signal_event_id: SignalEventId,
    evaluation_deadline: SimTime,
}
```

Success depends on:

- Outcome similarity.
- Temporal proximity.
- Target participation.
- Alternative causes.
- Sender observation quality.

The sender's success estimate may itself be wrong.

## Deception

A liar's private proposition can differ from its believed world state.

The receiver still sees only signals.

Deception success depends on:

- Trust.
- Plausibility.
- speaker reputation.
- emotional control.
- supporting context.
- listener knowledge.
- corroboration.
