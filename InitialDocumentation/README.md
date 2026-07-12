# Emergent Society Simulation With Evolving Language

This documentation set describes a large-scale society simulation in which individuals have private cognition, personal memories, beliefs, relationships, goals, and learned language.

The central design rule is:

> An agent's intended meaning is private. Other agents can observe only physical signals and context. Communication changes beliefs; it never directly controls behavior.

The long-term target is up to **10 million persistent, individually represented NPCs** without replacing distant people with anonymous population statistics. Achieving that target requires an event-driven simulation, compact data layouts, sparse variable-length state, deterministic procedural world generation, and selective computation based on activity.

## Documentation map

1. [Project vision and principles](00_PROJECT_VISION.md)
2. [Architecture overview](01_ARCHITECTURE_OVERVIEW.md)
3. [World model and generation](02_WORLD_MODEL_AND_GENERATION.md)
4. [Agent data model](03_AGENT_DATA_MODEL.md)
5. [Cognition and decision-making](04_COGNITION_AND_DECISION_MAKING.md)
6. [Communication architecture](05_COMMUNICATION_ARCHITECTURE.md)
7. [Language learning and evolution](06_LANGUAGE_LEARNING_AND_EVOLUTION.md)
8. [Beliefs, memory, and relationships](07_BELIEFS_MEMORY_RELATIONSHIPS.md)
9. [Society systems](08_SOCIETY_SYSTEMS.md)
10. [Event-driven simulation](09_EVENT_DRIVEN_SIMULATION.md)
11. [Storage and memory budget](10_STORAGE_AND_MEMORY_BUDGET.md)
12. [Spatial partitioning and pathfinding](11_SPATIAL_PARTITIONING_AND_PATHFINDING.md)
13. [Scaling to ten million NPCs](12_SCALING_TO_TEN_MILLION.md)
14. [Rust, C++, and engine strategy](13_TECH_STACK_RUST_CPP_ENGINE.md)
15. [Save/load and determinism](14_SAVE_LOAD_DETERMINISM.md)
16. [Debugging and analytics](15_DEBUGGING_ANALYTICS_TOOLS.md)
17. [Implementation roadmap](16_IMPLEMENTATION_ROADMAP.md)
18. [Testing and benchmarks](17_TESTING_AND_BENCHMARKS.md)
19. [Failure modes and guardrails](18_FAILURE_MODES_AND_GUARDRAILS.md)
20. [Reference data structures](19_REFERENCE_DATA_STRUCTURES.md)
21. [Open questions and future systems](20_OPEN_QUESTIONS_AND_FUTURE_SYSTEMS.md)
22. [Architecture decision records](21_ARCHITECTURE_DECISIONS.md)
23. [Glossary](GLOSSARY.md)

## Recommended first proof

Before attempting a giant world or millions of agents, prove this vertical slice:

- Deterministic chunked world.
- 20–100 agents.
- Hunger, thirst, rest, safety, and social needs.
- Gathering and shelter.
- Parent/child relationships.
- Personal lexicons.
- Signals, gestures, gaze, and emotional tone.
- Ambiguous interpretation.
- Clarification.
- Delayed learning from observable outcomes.
- Headless deterministic replay.

The project should scale by preserving the same conceptual model while changing scheduling, storage, and physical detail—not by replacing named people with anonymous statistics.
