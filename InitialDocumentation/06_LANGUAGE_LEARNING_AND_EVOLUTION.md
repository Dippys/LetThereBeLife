# Language Learning and Evolution

## Concepts versus signals

A concept is an internal category or semantic representation.

Examples:

- FOOD.
- WATER.
- DANGER.
- GIVE.
- FOLLOW.
- PARENT.
- OWNERSHIP.
- LOCATION.

A signal is an observable form associated with possible meanings.

Different cultures may use different forms for similar concepts.

Shared engine-level concept IDs are acceptable for implementation and debugging, but agents should eventually be allowed to categorize reality differently.

## Founding proto-language

The founding population should begin with:

- Basic concepts.
- Emotional expressions.
- Imitation.
- Joint attention.
- Associative learning.
- A small inherited proto-language.
- A few signal-ordering patterns.

Possible initial concepts:

```text
FOOD, WATER, DANGER, COME, GO, GIVE, ME, YOU,
YES, NO, MOTHER, CHILD, ANIMAL, FIRE, HOME
```

Possible constructions:

```text
ACTION + TARGET
OBJECT + LOCATION
NEGATION + SIGNAL
PERSON + ACTION
```

The signal IDs are not English words in the simulation.

## Personal lexicon

Language belongs to individuals.

```rust
struct LexicalHypothesis {
    signal: SignalId,
    concept: ConceptId,
    positive_evidence: u16,
    contradictory_evidence: u16,
    heard_count: u16,
    successful_uses: u16,
    failed_uses: u16,
    origin: CommunityId,
    last_observed: SimTime,
}
```

A signal may map to several concepts. A concept may have several competing signals.

Recognition and production familiarity should be separate.

## Grounded acquisition

A learner does not update directly from the speaker's private intended concept.

Evidence comes from:

- Visible objects.
- Pointing.
- Gaze.
- Recent actions.
- Current needs.
- Emotional change.
- Consequences.
- Repetition.
- Social closeness.
- Context clarity.
- Later confirmation.

Example:

```text
Parent produces signal_20.
Parent looks at food.
Parent gives food.
Child's hunger decreases.
```

The child strengthens `signal_20 → FOOD` because of observed associations, not hidden truth.

## Babies and developmental stages

### Reflexive stage

- Cry.
- Scream.
- Laugh.
- Reach.
- Cling.
- Turn away.
- Broad emotional vocalization.

### Joint-attention stage

- Follow gaze.
- Follow pointing.
- Intentionally point.
- Recognize recurring caregiver signals.

### Single-signal stage

- Produce familiar signals.
- Request salient objects.
- Overgeneralize categories.

### Combination stage

- Combine signals.
- Learn common order.
- Ask simple clarification.

### Productive stage

- Apply constructions to new cases.
- Invent descriptions.
- Learn social variants.
- Code-switch.

Learning depends on:

- Age.
- Cognitive ability.
- Attention.
- repetition.
- context clarity.
- trust.
- emotion.
- speaker prestige.
- frequency.
- communication success.

## Community language

A community language is derived from individual usage.

Possible statistics:

- Most common form for a concept.
- Recognition rate.
- Production rate.
- Age distribution.
- Regional variants.
- Occupational variants.
- Formal/informal use.
- Loanwords.
- Obsolete forms.
- Competing synonyms.

The community model must not automatically teach all members.

## Shared baseline optimization

At ten million agents, duplicating a full dictionary in every mind is wasteful.

A safe optimization:

- Store immutable physical signal forms once.
- Store community usage statistics once.
- Store per-agent familiarity, uncertainty, exceptions, foreign forms, and personal inventions.
- Require experience or teaching before an agent can use a shared form.

The optimization removes duplicated bytes; it must not create magical knowledge.

## New vocabulary

When no adequate signal exists, an agent may:

- Reuse an existing signal.
- Extend a meaning.
- Combine known signals.
- Point.
- Demonstrate.
- Imitate a sound.
- Invent a gesture.
- Invent a vocal form.
- Borrow a foreign form.
- Use metaphor.

Adoption depends on:

```text
communication success
+ repetition
+ prestige
+ peer usage
+ usefulness
+ ease of production
- ambiguity
- conflict with existing forms
- taboo or social cost
```

## Grammar as constructions

Do not begin with a full human grammar parser.

Store probabilistic constructions:

```rust
struct Construction {
    pattern: Vec<ConstructionElement>,
    semantic_template: SemanticTemplate,
    observed_count: u32,
    successful_count: u32,
}
```

Agents may learn:

```text
P(ACTION before OBJECT) = 0.76
P(OBJECT before ACTION) = 0.19
P(other)                = 0.05
```

Children may regularize highly variable patterns.

## Signal form and sound change

Do not model every pronunciation variant as unrelated.

Represent vocal forms using fictional phonetic features or syllable components.

Similarity enables:

- Mishearing.
- Accent.
- Pronunciation drift.
- Child errors.
- Related-language recognition.
- Gradual sound change.

## Language change mechanisms

- Pronunciation drift.
- Semantic broadening.
- Semantic narrowing.
- Metaphor.
- Replacement.
- Synonyms.
- Homonyms.
- Compounding.
- Simplification.
- Prestige effects.
- Taboo replacement.
- Professional vocabulary.
- Secret codes.
- Borrowing.
- Pidgins.
- Mixed languages.
- Extinction.

## Splitting and mixing

Language divergence depends on:

- Distance.
- Isolation.
- migration.
- intermarriage.
- trade.
- war.
- religion.
- political unification.
- travel.
- prejudice.
- generational turnover.

Cross-cultural contact may create:

- Misunderstanding.
- failed trade.
- accidental insult.
- bilingualism.
- translators.
- loanwords.
- trade languages.
- minority languages.
- language death.

## Individual life history

A trader may know:

- Native community language.
- Neighbor language.
- Trade pidgin.
- Professional terms.
- Secret merchant signals.

Language knowledge should always reflect actual biography.
