# Project Vision and Non-Negotiable Principles

## Goal

Create a simulation where society emerges from the interactions of individual people rather than being fully scripted.

The simulation should support:

- Individuals with needs, personality, emotions, memories, relationships, beliefs, skills, plans, and goals.
- Families, children, tribes, settlements, professions, ownership, trade, conflict, migration, institutions, and cultural norms.
- Communication without natural-language processing or large language models.
- Multiple cultures with distinct languages.
- Children gradually acquiring language through exposure and grounded interaction.
- Misunderstanding, ambiguity, repair, deception, distrust, authority, negotiation, and refusal.
- Vocabulary invention, borrowing, drift, semantic change, dialect formation, splitting, mixing, prestige effects, professional jargon, and language extinction.
- Persistent individual identity at very large population scales.

## Core communication principle

Agents must never directly transmit intended meaning.

The pipeline is:

```text
Private sender intention
        ↓
Physical and observable signals
        ↓
Receiver inference
        ↓
Belief update
        ↓
Independent decision
```

A sender may privately want:

```text
The child should come inside.
```

The public event may contain:

- A vocal pattern.
- Pointing toward the house.
- Gaze direction.
- A worried expression.
- A sharp emotional tone.
- Movement toward the door.

A receiver may infer:

```text
Come inside:       0.70
Stay close:        0.15
Danger nearby:     0.10
Unknown:           0.05
```

The child then chooses an action using its own needs, goals, trust, fear, attention, personality, relationship with the parent, and interpretation confidence.

Communication never inserts a task directly into another agent's task queue.

## Individuality at scale

"Ten million individuals" means:

- Each NPC has a persistent identity.
- Each has its own state and history.
- Each can make decisions when relevant.
- Each can differ from culturally similar neighbors.
- Each can migrate, learn, forget, form relationships, and alter society.
- No NPC is silently replaced by an anonymous population count.

It does **not** require every NPC to execute a complete cognition cycle every rendered frame or every simulated second. Inactive agents can retain exact state and wake only when an event or predicted threshold requires a decision.

## Design values

### Emergence over scripting

Scripts may provide physical rules, cognitive capabilities, and institutional mechanisms. Scripts should not predefine the exact historical result.

### Private knowledge

The simulation distinguishes:

- Objective world state.
- Perceived observations.
- Memories.
- Current beliefs.
- Beliefs about other people's knowledge.
- Cultural claims.
- Lies and rumors.

### Grounded meaning

Signals derive meaning from perception, action, needs, social context, and consequences.

### Sparse detail

Only meaningful relationships, memories, lexical exceptions, and beliefs are stored. Routine experience is forgotten or consolidated.

### Deterministic reproducibility

Given the same seed, inputs, version, and worker scheduling policy, the simulation should be replayable for debugging.

### Instrumentation first

The simulation must explain why an NPC acted, what it believed, how it interpreted a signal, and how a word spread.

## Things the project should avoid

- A magical tribe-wide dictionary automatically known by all members.
- Hidden intent fields delivered to receivers.
- A global truth database exposed to all NPCs.
- Random global language mutations disconnected from social use.
- One `Update()` call per NPC.
- A fully materialized trillion-cell world.
- One game object or scene node per persistent NPC.
- Unbounded permanent memory.
- Every person storing relationships with every other person.
- Every interpretation comparing against every concept.
- Overbuilding procedural terrain before proving the social simulation.
