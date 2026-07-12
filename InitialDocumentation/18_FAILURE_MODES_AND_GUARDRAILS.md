# Failure Modes and Design Guardrails

## Hidden intent leakage

### Failure

Receiver learning or behavior reads the sender's intended concept.

### Consequence

Language acquisition becomes perfect despite probabilistic decoration.

### Guardrail

Private intent and public signal must be different types in different modules. Tests must verify that identical public signals produce identical receiver input regardless of private intent.

## Magical community dictionary

### Failure

One agent invents a word and everyone knows it.

### Guardrail

Community statistics are derived data. Personal familiarity changes only through exposure, teaching, or explicit institutional learning.

## Universal cognition disguised as different vocabulary

### Failure

All cultures categorize reality identically.

### Guardrail

Allow individual or cultural concept prototypes and category boundaries to differ.

## Random global language mutation

### Failure

Every generation randomly changes words in a global table.

### Guardrail

Change must result primarily from production noise, learning, adoption, isolation, prestige, and replacement.

## One update per NPC

### Failure

Every agent perceives, thinks, and plans each frame.

### Guardrail

Event-driven wakeups and predicted thresholds.

## Fully materialized giant world

### Failure

Allocate 1 trillion cells with multiple layers.

### Guardrail

Chunked procedural generation and sparse deltas.

## Trees as a simple tile layer

### Failure

Trees cannot age, burn, grow, or hold state without bloating the tile.

### Guardrail

Dense terrain plus sparse feature records.

## Unbounded memory

### Failure

Every routine event is permanent.

### Guardrail

Working memory, episodic selection, consolidation, forgetting, and archival.

## Relationship matrix

### Failure

Store a record for every pair.

### Guardrail

Sparse relationships plus group priors.

## Unbounded interpretation search

### Failure

Every signal compares with every concept.

### Guardrail

Generate candidates from context, personal associations, recent activity, and construction slots.

## Excessive invention

### Failure

Agents constantly create new words.

### Guardrail

Strong reuse advantage; invention only when existing expressions are inadequate.

## Overpowered agents

### Failure

Every NPC performs perfect Bayesian reasoning, theory of mind, grammar induction, and planning.

### Guardrail

Capabilities vary by age, cognition, stress, experience, education, and health.

## Worldgen trap

### Failure

Years are spent generating terrain before society works.

### Guardrail

Build the smallest world capable of testing social emergence.

## Generic ECS overuse

### Failure

Millions of irregular minds are forced into expensive generic component structures.

### Guardrail

Custom arrays/pools for persistent population; ECS for active presentation and local physical entities.

## Premature engine construction

### Failure

The team builds rendering, UI, asset pipelines, and editors before the simulation.

### Guardrail

Use a minimal viewer and headless core.

## Instant global propagation

### Failure

News, disease, or culture updates every person simultaneously.

### Guardrail

Use local propagation, region events, carriers, institutions, and scheduled transitions.

## Memory budget illusion

### Failure

Assume "4 KB average" while allocator overhead and empty capacity double it.

### Guardrail

Measure resident bytes, fragmentation, indexes, and overhead with production-like distributions.

## Scaling before correctness

### Failure

Ten million agents perform shallow or broken behavior.

### Guardrail

Prove believable behavior at 20–100 agents before increasing population.
