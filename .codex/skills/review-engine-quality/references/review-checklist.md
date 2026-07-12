# Review Checklist

- Correct result and state transitions
- Deterministic ordering, time, RNG, and serialization assumptions
- Headless simulation and presentation separation
- Bounded CPU and memory behavior at target scale
- Justified integer widths, alignment, collection capacity, allocations, clones, and hot/cold layout
- Dead code, duplicate state, obsolete dependencies, and unnecessary abstraction
- Ownership, handles, overflow, invalid input, and error paths
- Minimal public API and dependency direction
- Regression, edge-case, and invariant tests
- Formatting, tests, Clippy, and relevant runtime evidence
- Accurate current implementation, architecture, decisions, roadmap, and testing docs
- No changes under `InitialDocumentation/`
