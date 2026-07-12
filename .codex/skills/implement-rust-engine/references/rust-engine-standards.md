# Rust Engine Standards

## Ownership

- `sim-core`: deterministic engine state, commands, time, and snapshots; standard library only unless a measured need justifies more.
- `sim-server`: headless process concerns and command-line input.
- `sim-viewer`: windowing, user input, wall-clock accumulation, and presentation.
- Add a crate only when it creates a real dependency boundary, not merely a new folder.

## Determinism

- Never derive simulation outcomes directly from frame timing or unordered iteration.
- Make seeds, tick/event ordering, and numeric conversions explicit.
- Test equal inputs for equal outputs.
- Keep presentation snapshots immutable and free of mutation handles.

## Quality

- Prefer small modules with domain names over generic `utils` modules.
- Keep public APIs minimal and document invariants.
- Use checked or saturating arithmetic when domain limits require it.
- Avoid `unsafe`; require a documented invariant, benchmark evidence, and focused tests before introducing it.
- Benchmark before optimizing layouts or adding concurrency.

