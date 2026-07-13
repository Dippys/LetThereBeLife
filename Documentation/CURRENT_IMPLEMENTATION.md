# Current Implementation

Last synchronized: 2026-07-13.

## Implemented

- Rust 2024 Cargo workspace containing `sim-core`, `sim-config`, `sim-headless`, and `sim-viewer`.
- Engine-independent `sim-core` configuration, deterministic tick counter, pause/reset/speed commands, and immutable `SimulationSnapshot` output.
- Configurable initial world-area generation in `sim-core`, using the repository configuration's 4,096 x 4,096 startup area (with a 1,024 x 1,024 Rust fallback), divided into 64 x 64 generation chunks and stored as compact row-major terrain.
- Deterministic world-space integer terrain generation where a 2,048-cell continental field alone controls continental ocean/coast membership, finer fields shape inland relief, and sparse descriptors in 1,024-cell regions create lakes with guaranteed water cores and perturbed elliptical shores. Each suitable 2,048-cell coastal region can also produce one major river from a 16 x 16 pre-lake continental-relief lattice: accepted routes begin in hill-or-higher baseline terrain, strictly descend at coarse nodes, reject nonlocal width-expanded self-contact and tight returns, refine bends toward locally lower relief, and terminate in pre-existing continental water. A nominal 20-cell shallow headwater feeds a nominal 16-to-48-cell widening channel; its narrow sand bank widens with the channel and its deep core begins downstream. The repository seed accepts one startup-area route. Its 32-cell topology sample sees one boundary-touching continental-water component, while the full-resolution river test proves the channel reaches continental water; complete arbitrary-seed ocean connectivity is not claimed. Deep water, shallow water, sand, grass, forest floor, hill, and bare-rock terrain remain independent of the initial-area boundary.
- Shared `sim-config` TOML loading for `sim-viewer` and `sim-headless`, with explicit validation of initial dimensions.
- `sim-config` build integration copies `config/simulation.toml` beside Cargo binaries under the active profile directory, including `target/debug/config/simulation.toml` and the equivalent release path.
- VS Code launch entries build and run viewer/headless binaries in debug or release mode with the repository configuration and working directory, plus a task for the complete workspace validation gate.
- Sparse deterministic tree, rock, and berry-bush feature records owned by the generated world.
- Fixed-step accumulator in `sim-viewer`, separating variable render timing from 60 Hz simulation ticks and capping large frame delays.
- Native `winit` window and event lifecycle with redraws capped at 60 Hz while active and suspended when paused and visually unchanged.
- GPU presentation through `wgpu`, using size-checked camera uniforms and instanced terrain, feature, selection, hover, and status rectangles instead of CPU framebuffer rasterization.
- Camera-bounded GPU instance caches with an approximately 128-screen-pixel scale-relative margin, deterministic power-of-two zoomed-out sampling with generated-edge clipping, and static buffers segmented at 1,000,000 instances. Caches rebuild only when the camera leaves the cached area, the sampling step changes, or a synchronized world revision changes.
- Deterministic 64 x 64 generated chunks keyed by signed `ChunkCoord`, with bounded chunk lookup instead of a growing linear patch search.
- A dedicated world-generation worker receives only missing chunk coordinates, keeps selected-area generation off the window/event-loop thread, and merges bounded completed batches into `sim-core` on the main thread.
- Large-generation guardrails: 4,096 missing chunks per request, 16,384 retained generated chunks, 16 chunks applied per frame, stop-remaining-work cancellation with `C` (already applied chunks remain), and deferred GPU cache synchronization. Retained chunks and selected portions fully covered by the initial rectangle do not consume request budget; a partially initial boundary chunk counts when its selection extends into unloaded terrain.
- Viewer camera starting at full-map fit with cursor-anchored mouse-wheel zoom from 1/16x fit to 64x magnification and unbounded left-button drag panning through generated or empty space.
- Right-button drag selection with a translucent yellow valid preview and red invalid preview for missing-chunk-budget, retained-capacity, or coordinate-safety failures. New previews are disabled while the background generator is busy or unavailable; release generates deterministic terrain and sparse features only for an accepted rectangle.
- Signed world positions supporting generated patches in negative and positive coordinate space.
- Hover inspection with a highlighted cell and window-title output for coordinates, terrain, elevation, moisture, and sparse feature kind.
- Keyboard controls: pause/resume, 1x–8x speed selection, cancel active generation, reset, and exit.
- Headless runner accepting `--ticks` and `--seed`.
- Unit tests covering equal-input tick determinism, pause behavior, and reset behavior.
- Repository-local skills for orientation, Rust implementation, living documentation, validation, architecture/code review, and workflow evolution.
- Root agent routing with a mandatory inspect, implement, document, review, and validate lifecycle.
- A checksum manifest that detects any change under immutable `InitialDocumentation/`.

## Not implemented

- Automatic proximity-driven chunk generation, chunk unloading, or chunk persistence; expansion currently requires a right-drag selection and retains generated chunks in memory.
- Full drainage basins, tributaries, local streams, wetlands, dynamic water flow, erosion, dams/canals, resource quantities, or terrain modification. Current lakes and major rivers are bounded deterministic terrain shaping, not watershed simulation.
- Persistent agents, needs, cognition, movement, or event scheduling.
- General-purpose deterministic RNG streams, save/load, snapshots on disk, or replay logs.
- Text, asset, animation, and advanced inspection systems beyond the current GPU rectangle renderer.
- Region workers, parallel simulation, networking, or long-term persistence.
