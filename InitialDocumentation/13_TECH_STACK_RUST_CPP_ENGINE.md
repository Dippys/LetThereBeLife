# Rust, C++, and Engine Strategy

## Recommendation

For this project:

- Build the simulation core in Rust or modern C++.
- Keep it independent from the rendering engine.
- Run it headlessly.
- Add a lightweight viewer/client.
- Do not begin with a custom full game engine unless engine construction is itself a goal.

## Rust strengths

- Memory safety without garbage collection.
- Strong ownership model.
- Safer parallel region processing.
- Enums suited to events and state machines.
- Good data-oriented performance.
- Reduced risk of use-after-free and invalid references.
- Cargo workspace and testing ergonomics.
- Good headless portability.

Potential drawbacks:

- Borrow-checker friction with complex graph-like state.
- Smaller ecosystem for some game middleware.
- Custom allocators and extreme low-level tuning may require more work.
- Bevy version churn should not leak into the simulation core.

## C++ strengths

- Maximum control over allocation, layout, SIMD, and platform integration.
- Mature ecosystem.
- Strong Unreal integration.
- Many established ECS and simulation libraries.
- Easier use of certain native tools.

Potential drawbacks:

- Manual lifetime and ownership bugs.
- difficult multithreaded debugging.
- iterator invalidation.
- undefined behavior.
- more fragile long-running simulation state.

## Language choice rule

Choose Rust if experience is comparable.

Choose C++ if:

- You are substantially stronger in it.
- Unreal is a hard requirement.
- Critical libraries are C++-only.
- You are comfortable implementing safe ownership discipline.

The architecture matters more than the language.

## Engine choices

### Bevy

Good for:

- Rust-native viewer.
- ECS for visible/local entities.
- wgpu rendering.
- UI integration.

Do not automatically put every persistent NPC into a rich Bevy entity with many dynamic components.

### Custom wgpu + egui

Good for:

- Minimal viewer.
- direct control.
- low engine coupling.
- maps, overlays, and inspectors.

Requires more rendering and asset work.

### Unreal

Useful if choosing C++ and desiring strong tooling, but heavy for simple graphics. MassEntity may help local active entities, yet the simulation core should remain independently testable.

### Godot

Can act as a viewer with Rust/C++ extension integration. Avoid making scene nodes the source of truth for millions of NPCs.

### Unity

C# and Burst are capable for smaller targets, but native Rust/C++ is more appropriate for the stated ten-million persistent-agent ambition. Unity could still serve as a presentation client through an API or native plugin, though integration complexity increases.

## Generic ECS guidance

Use a generic ECS where it helps:

- Visible entities.
- local combat.
- active physical objects.
- temporary effects.
- rendering components.

Use custom arrays and pools for:

- all-agent cores.
- scheduling.
- sparse minds.
- region ownership.
- variable memories.
- language associations.

## Suggested Rust stack

```text
sim-core       pure simulation rules
sim-storage    dense arrays and packed pools
sim-world      generation and chunking
sim-language   signals, learning, and evolution
sim-server     headless runner
sim-viewer     Bevy or wgpu/egui
sim-bench      targeted benchmarks
```

Possible libraries should be selected after benchmarking, not merely because they are popular.

## API boundary

The simulation should expose snapshots or queries:

- Get visible agents.
- inspect NPC.
- request chunk.
- advance time.
- pause.
- set speed.
- save.
- load.
- inject debug event.
- subscribe to historical events.

The presentation layer should not mutate simulation internals directly.
