# World Foundation Improvement Plan

Last synchronized: 2026-07-15.

Status: **Implemented**. Slices 0 through 7 are complete; the next observed work belongs to the first physical-agent loop. This document remains the record of the Phase 1 world-foundation sequence.

## Purpose

Preserve the current generator's convincing continental structure while closing the gaps that prevent the world from being coherent across the complete envelope, environmentally varied at local scales, legible at every viewer zoom, and useful to the first physical agents.

The target is not a perfect geology or ecology simulator. The target is enough deterministic world structure to support:

- understandable movement and settlement placement;
- reliable access to water, food, wood, and stone;
- meaningful climate and terrain differences;
- connected rivers, lakes, and drainage basins;
- stable close, regional, seam, and full-world inspection;
- later sparse modification and resource-depletion state without replacing the generated base.

## Current baseline

The implemented generator already provides:

- a centered `65,536 x 65,536`-cell world envelope;
- deterministic 64 x 64 chunks and 4,096-cell drainage regions;
- a canonical 256-cell whole-envelope drainage skeleton with basin identities, deterministic lake outlets, and sparse explicitly lake-fed graded river routes;
- analytic continents, oceans, coastlines, relief, plate-boundary mountains, temperature, and moisture;
- whole-envelope priority-flood lakes and accumulated-flow major rivers refined through regional sampling;
- packed surface and biome semantics covering ocean/lake/river water, beach/desert sand, soil-backed grassland/savanna/forest/wetland/tundra, alpine hill/rock, and snow/ice;
- sparse deterministic trees, rocks, and berry bushes;
- bounded parallel generation and camera-bounded `wgpu` rectangle rendering.

Known limitations that motivate this plan:

- current rivers are deliberately lake-fed only; spring, snowmelt, and finer-than-256-cell catchment sources are not yet modeled;
- traversal, water identity/drinkability, and immutable resource yield are exposed as derived resident queries; fertility and a universal settlement score remain intentionally undefined;
- sparse features have only a kind and position, with no resource quantity, species, lifecycle, or modification state;
- world-quality tests check structural properties but do not yet enforce a representative multi-seed, multi-scale review contract.

## Non-negotiable constraints

Every slice in this plan must preserve these rules:

1. `sim-core` owns authoritative generated terrain and features. `sim-viewer` owns only camera, renderer caches, visual summaries, overlays, and other presentation state.
2. Generated base content remains a pure function of generator inputs such as seed, coordinates, and an eventual generator version. Thread count, cache residency, generation order, and viewer state must not change output.
3. Hydrology ownership and boundary behavior must be explicit. No adjacent region may independently invent conflicting water state for the same world location.
4. Compactness is measured, not assumed. Any `TerrainCell`, feature-record, regional-cache, or renderer-cache change requires size and retained-memory accounting.
5. Overview rendering must not become simulation truth. A coarse visual summary may differ in representation, but it may not alter full-resolution cells or feature records.
6. New behavior needs deterministic regression coverage. Representative visual review supplements tests; it does not replace them.
7. Generator output may intentionally change during this plan because persistence has not shipped. Generator versioning must be introduced before durable generated worlds are saved.
8. Erosion, dynamic water, seasons, caves, and full ecology stay deferred unless a completed slice proves one is required for the first physical-agent loop.

## Work sequence

The slices are ordered by dependency. A later slice may be designed while an earlier one is being reviewed, but it should not be merged against assumptions that the earlier slice is expected to replace.

| Order | Slice | Primary result |
| ---: | --- | --- |
| 0 | Repeatable world-quality baseline | Stable evidence before generator changes |
| 1 | Cross-region drainage skeleton | Connected watersheds, rivers, and lake outlets |
| 2 | Finite-world climate contract | Climate variation that fits the actual envelope |
| 3 | Terrain and biome semantics | More meaningful land classes without uncontrolled type growth |
| 4 | Sparse sourced rivers, wetlands, and transitions | Regional geography that remains convincing close up |
| 5 | Surface-feature ecology and resource readiness | Useful distributions and an explicit path to depletion state |
| 6 | Multi-scale renderer summaries | Terrain and features remain legible when zoomed out |
| 7 | Phase 1 exit contract | Passability, resource queries, and a documented handoff to agents |

## Slice 0: Repeatable world-quality baseline

Status: **Implemented** on 2026-07-14. Run `cargo run --release -p sim-core --example render_map -- --review-set`; the canonical BMPs and deterministic TSV reports are written under `target/world-quality/`. Exact coverage, output files, measurements, and limitations are recorded in `TESTING.md` and `PERFORMANCE.md`.

### Objective

Make generator changes reviewable with the same seeds, coordinates, scales, statistics, and seam locations every time.

### Implementation area

- `crates/sim-core/examples/render_map.rs`
- focused world-generation test helpers under `crates/sim-core/src/worldgen/`
- `Documentation/TESTING.md`
- `Documentation/PERFORMANCE.md`

### Deliverables

- Define a small representative seed set containing the repository seed plus seeds chosen for different continent, mountain, desert, forest, lake, and river layouts.
- Define canonical full-envelope, regional, drainage-seam, coastline, river-mouth, river-source, mountain, and close-up views.
- Extend developer tooling only as needed to emit those views and a compact terrain/feature distribution report.
- Record the exact release-mode commands and expected output locations.
- Record baseline generation time, sampled terrain distribution, feature density, and relevant cache/type sizes.

### Acceptance criteria

- One documented command or script produces the complete review set without modifying authoritative world state.
- The output identifies seed, bounds, scale, and generator revision or source revision.
- At least one review view crosses each axis of a 4,096-cell drainage-region boundary.
- Tests cover the tooling's bounds, sample alignment, and deterministic output metadata.
- No quality threshold is chosen solely to preserve the current seed's appearance.

## Slice 1: Cross-region drainage skeleton

Status: **Implemented** on 2026-07-14. `crates/sim-core/src/worldgen/drainage.rs` now builds one immutable 257 x 257 whole-envelope skeleton per seed at a 256-cell step. It owns canonical basin sink IDs, lake IDs/outlets/spill elevations, channel identities, confluences, ocean/world-edge destinations, and shared 32-cell-subdivided river geometry. Regional maps sample the same filled surface and segments on both sides of every edge; the former two-node lake and four-node river dry margins are removed.

The release-only comparison command is:

```powershell
$env:SIM_DRAINAGE_STEP='128' # repeat with 256
cargo test --release -p sim-core worldgen::drainage::tests::compare_candidate_skeleton_steps -- --ignored --nocapture --test-threads=1
```

On the recorded machine, fresh test processes measured 175.9 ms / 1,918,704 retained logical bytes / 17,369,154 scratch-upper-bound bytes for step 128, versus 44.3 ms / 1,109,240 retained logical bytes / 4,359,234 scratch-upper-bound bytes for step 256. Both candidates sample expensive upwind moisture every fourth skeleton node and interpolate it before routing. Step 256 retained 167 canonical lakes and 3,832 coarse channel links for seed 1 while using roughly one quarter of the build time and scratch of step 128; its 12-view review set retained convincing complete-envelope topology. The four representative seeds retain 4,155,352 logical skeleton bytes together; the representation's conservative all-nodes-channel theoretical ceiling is 62,350,256 bytes for four cached seeds. Both exclude `Arc`, vector-box, cache-entry, and allocator metadata. Existing `RegionMap` payload remains five 129 x 129 `i32` arrays (about 325 KiB) plus its bounded river vector.

### Objective

Remove artificial drainage cutoffs at 4,096-cell region edges while preserving deterministic on-demand chunk generation and bounded shared derivation state.

### Proposed direction

Add a seed-keyed world-drainage skeleton above the current regional solve. Because the world is finite, a coarse whole-envelope lattice can establish canonical basin outlets, major flow direction, and cross-region inflow/outflow without densely generating terrain cells. Regional drainage then refines that skeleton on the existing 32-cell lattice.

The first implementation should evaluate a coarse step such as 128 or 256 cells and choose it from measured topology quality, build time, and retained memory. The chosen representation must have canonical ownership for:

- basin outlet and spill elevation;
- flow crossing each regional edge;
- major channel identity and continuation;
- lake outlet or terminal-basin status;
- water-body identity where ocean, lake, and river behavior must differ.

`RegionMap` may use a halo or shared edge descriptors, but neighboring regions must consume the same canonical boundary data rather than solve independent edge conditions.

### Implementation area

- `crates/sim-core/src/worldgen/hydrology.rs`
- `crates/sim-core/src/worldgen/mod.rs`
- possibly a new private `crates/sim-core/src/worldgen/drainage.rs`
- regional cache construction and preparation in `crates/sim-core/src/worldgen/mod.rs`
- world-generation tests and release measurements

### Acceptance criteria

- No fixed dry river or lake margin remains solely because a coordinate is near a drainage-region edge.
- Every emitted major river continues to a downstream channel, lake, ocean, or explicitly modeled terminal basin.
- Cross-region flow is identical regardless of which adjacent chunk or region is generated first.
- Lakes that need outlets have deterministic outlets; terminal basins are explicit rather than accidental priority-fill artifacts.
- Positive and negative coordinate seams are covered.
- One-worker and multi-worker generation produce byte-identical chunks.
- Regional preparation remains bounded and does not perform per-chunk global solves.
- Build time, temporary allocations, and retained bytes for the drainage skeleton and regional maps are recorded.

## Slice 2: Finite-world climate contract

Status: **Implemented** on 2026-07-14. The finite vertical envelope is one stylized cold-to-warm-to-cold band: lowland baseline temperature rises from 6,000 at either vertical edge to 46,000 at the center before broad deterministic variation and altitude lapse. Four 16,384-cell circulation bands alternate northwest/southeast prevailing flow; 8,192-cell noise displaces their boundaries by up to 6,000 cells so transitions are broad and non-linear. Horizontal edges do not wrap, and off-envelope analytic samples remain boundary input for the existing 17,000-cell ocean-fetch probes rather than wrapped geography.

The provisional physical interpretation is approximately 2 metres per cell, making the envelope about 131 km square. The climate zones are intentionally compressed for readable gameplay geography rather than claimed as an Earth-scale latitude model: cold lowland shoulders occupy roughly the outer 7,000 cells of each side before variation, temperate transitions roughly the next 12,000 cells per side, and the central warm band roughly 26,000 cells wide. This scale remains a world-foundation convention, not a persistence or movement-resolution commitment.

`World::climate_at` exposes a four-byte `ClimateSample` for resident-cell inspection without growing the four-byte `TerrainCell` or materializing a regional cache. Temperature exactly reproduces the generator's 32-cell lattice interpolation from four analytic nodes; moisture is the retained eight-bit cell value and prevailing wind is derived directly. Complete-envelope multi-seed tests enforce cold, temperate, and warm lowland coverage, signed-edge inspection equality, wobbled circulation transitions, and existing worker/cache determinism. Slice 3 consumes this contract for tundra, snow, wetland, and other biome meanings.

### Objective

Make temperature and prevailing-wind behavior meaningful across the actual `[-32,768, 32,768)` world rather than across a much larger implicit latitude cycle.

### Decision checkpoint

Before implementation, choose and record one physical interpretation:

- **Recommended:** the finite vertical envelope represents a cold-to-warm-to-cold world band, with cold high latitudes near both vertical edges and the warmest lowlands nearer the center; or
- the envelope represents only a regional climate slice, in which case latitude terminology and expectations must be narrowed and biome variety must come from another explicit large-scale field.

The choice must also define approximate tile scale, expected climate-zone widths, and whether the horizontal edges wrap conceptually. No coordinate wrapping should be implemented without a separate world-topology decision.

### Implementation area

- `crates/sim-core/src/worldgen/climate.rs`
- climate inputs retained in `RegionMap`
- classification and feature conditions in `crates/sim-core/src/worldgen/mod.rs`
- hover inspection in `crates/sim-viewer/src/renderer.rs`
- world-quality and climate-distribution tests

### Acceptance criteria

- The complete playable envelope contains intentional cold, temperate, and warm lowland climate zones for representative seeds when using the recommended model.
- Altitude lapse, prevailing wind, ocean fetch, and rain shadows remain deterministic and spatially coherent.
- Wind-band transitions do not create straight world-space seams.
- Climate-zone coverage is tested against `WORLD_GENERATION_BOUNDS`, not only a convenient positive-coordinate probe window.
- The hover inspector can expose the climate information needed to diagnose classification, whether stored compactly or deterministically derived for inspection.
- Changing thread count or cache state does not change climate output.

## Slice 3: Terrain and biome semantics

Status: **Implemented** on 2026-07-14. `TerrainCell` still occupies four bytes: its former one-byte ground enum is now a private packed `TerrainClass` whose low nibble is exposed as `SurfaceType` and high nibble as `BiomeType`. The safe `TerrainCell::surface`, `TerrainCell::biome`, and `TerrainCell::classification` accessors distinguish deep/shallow water, sand, soil, hill, rock, and snow/ice surfaces from ocean, lake, river, beach, desert, grassland, savanna, forest, wetland, tundra, and alpine environments.

Classification uses the existing broad interpolated elevation, temperature, and moisture fields, so the new meanings form regional bands rather than independent cell scatter. Lake identity survives overlapping river segments while deep-water precedence remains order independent. Slice 4 subsequently replaced the provisional moisture/elevation-only wetland rule with low-slope floodplain and basin-edge evidence. Slice 7 subsequently derived traversal, drinkability, and immutable resource yield without stored cell flags; fertility and a universal settlement score remain deferred.

The viewer HUD and both viewer/developer palettes expose the split semantics. World-quality review format 2 records seven surface and eleven biome distributions plus the packed semantic byte in its stable hash. The four canonical complete-envelope hashes are `a2373a6b276fcb06`, `a3bef667d00b0691`, `0c174931efc451af`, and `7ce7b66e324e6cb6` for seeds 1, 7, 42, and 10,001 respectively.

### Objective

Represent the environmental differences required by movement, gathering, settlement, and rendering without turning one terrain enum into an unbounded catalogue.

### Data-model checkpoint

Decide whether to:

1. expand the current `GroundType` as a short-term rendered-surface classification; or
2. separate compact surface/substrate and biome/climate classes behind typed accessors.

The second direction is preferred if it can remain compact and clear. A candidate is a packed classification byte exposed through safe enum-returning methods, but its layout must be proven with size assertions and benchmarks before adoption. Traversal and fertility should initially be derived from terrain, slope, water, biome, and features unless storing flags is measurably justified.

### Minimum useful distinctions

- ocean/deep water and shallow coastal water;
- lake and river water where behavior differs from ocean water;
- beach or loose sand versus hot arid ground;
- ordinary soil/grassland;
- dry grassland or savanna;
- forest-supporting ground;
- wetland or marsh;
- hill and exposed rock;
- tundra and persistent snow/ice where climate supports them.

These are simulation-facing meanings, not a requirement for separate art assets in the same slice.

### Implementation area

- terrain records and inspection APIs under `crates/sim-core/src/world.rs` and `crates/sim-core/src/world/`
- classification in `crates/sim-core/src/worldgen/mod.rs`
- renderer palette and HUD labels in `crates/sim-viewer/src/renderer.rs`
- developer map palette in `crates/sim-core/examples/render_map.rs`
- size assertions, distribution tests, and documentation

### Acceptance criteria

- A terrain or biome distinction exists because it changes rendering, traversal, resources, settlement suitability, or a documented future rule—not only to add another color.
- Beach and desert are no longer indistinguishable if they need different traversal, fertility, or resource rules.
- Cold lowlands and cold mountains do not collapse into generic bare rock.
- Wet terrain can be distinguished from ordinary grassland without using surface features as hidden terrain truth.
- Terrain transitions form coherent regions and do not become single-cell threshold noise.
- `TerrainCell` size and full-world logical payload are asserted and documented after the chosen representation changes.
- Existing water-depth precedence and feature exclusion on water remain correct.

## Slice 4: Sparse sourced rivers, wetlands, and transitions

Status: **Implemented**, structurally revised on 2026-07-15 after visual review rejected the threshold-exposed network. The complete priority-filled graph remains hidden canonical topology, but no river begins merely because runoff crossed a numeric frontier. Moisture at or below 12,000 contributes no perennial runoff. Visible rivers must originate at the exact spill edge of a nonterminal canonical lake with at least 260,000 flow, drain to ocean or world edge, remain at least 2,048 cells from another selected source, and fit within a 24-source complete-envelope cap. The four review seeds retain 4-15 sources and 33-144 links; seed 1 retains 87 links rather than the rejected 4,479-link revision or 13,653-link leaf pattern.

Eight-neighbor routing rejects a diagonal that would cross one already selected in the same coarse cell. Selected links use exact canonical endpoints, bounded perpendicular curves, and at most six cells of interior detail; the complete four-seed intersection regression rejects crossings, collinear overlap, and unrelated endpoint contact. Each 28-byte segment owns `u16` start/end water surfaces that never rise downstream. Chunk rasterization projects that grade, so an upland river remains above sea level until its route descends naturally. Wetlands require low slope plus either the interpolated edge of canonical standing water or an 18-cell floodplain halo, together with the existing climate/elevation limits. A four-cell riparian bank turns qualifying dry shore soil into grassland. The already-computed coherent detail field boundedly displaces beach, desert, forest, wetland, snowline, and treeline thresholds without changing water ownership or adding another noise evaluation or retained state.

The four representative seeds use the 13-entry allocation-free `ChunkContext` fast path; an empty `Vec` fallback preserves valid arbitrary-seed generation instead of panicking if that measured capacity is exceeded. Canonical review format 2 hashes are `f95c7be0eb48381c`, `5db53acad12b153a`, `cd0ab6e48e2ca92b`, and `ff4a7c35517124f0` for seeds 1, 7, 42, and 10,001. Visual review on 2026-07-15 inspected explicit source-lake and mouth views and confirmed sparse continuous curved routes with visible water-body origins and correct upland grade. Lake-only sourcing is intentionally conservative; spring/snowmelt sources and optional major tributaries remain future reviewed work rather than unexplained blue lines.

### Objective

Make visible water sparse, sourced, graded, and believable after major drainage continuity is correct, while retaining hydrologic terrain transitions.

### Deliverables

- Select a bounded set of explicit sources from canonical accumulated runoff rather than exposing threshold frontiers.
- Retain complete source-to-destination paths with deterministic hierarchy, width, grade, and minimum visibility rules.
- Generate wetlands from shallow water table, low slope, floodplain proximity, basin edges, or poorly drained terrain rather than unrelated noise.
- Refine coastline, beach, riverbank, lake-edge, biome-edge, and treeline transitions within bounded deterministic rules.
- Preserve water-body connectivity through local detail; detail noise must not sever a channel selected by the drainage hierarchy.

### Acceptance criteria

- Tributaries join downstream channels rather than crossing or stopping beside them.
- Every visible river has an explicit source body, a non-rising downstream grade, and a valid path to open water.
- Wetlands occur in hydrologically plausible places and remain absent from steep or arid terrain unless explicitly justified.
- Coast and bank transitions remain recognizable at close and regional scales.
- Chunk and region seams do not change channel width, wetland classification, or transition ownership.
- Per-chunk river lookup remains finite and deterministic; representative generation stays inside the 13-segment allocation-free fast path and arbitrary-seed overflow remains safe.

## Slice 5: Surface-feature ecology and resource readiness

Status: **Implemented** on 2026-07-15. Tree, rock, and berry placement now interprets the already-computed coherent local-detail sample as bounded canopy, grove, berry-patch, and outcrop bands, combined with slope, biome, climate, and water proximity. Forest canopy contains coherent clearings and dense patches; grassland and savanna can contain groves; riparian suitable ground increases berry-patch availability; and soil outcrops make stone available outside hill/rock terrain. Water, sand, and snow/ice remain excluded. Reusing local detail avoids another noise field or retained value.

`FeatureKind::base_resource`, `Feature::base_resource`, and `World::base_resource_at` expose immutable generated capacities of 120 wood, 80 stone, or 12 food units through the four-byte `BaseResource`. These are abstract base capacities, not mutable inventory. Future depletion, removal, regrowth timing, ownership, damage, and persistence remain a sparse delta layer keyed by the stable feature position plus generator identity; none is stored in or allowed to alter generated base features. Species remain deferred because no implemented rule needs them.

World-quality review format 3 adds full-resolution 512 x 512 forest and outcrop probes to the existing berry-bearing close-up. The three 64-chunk probes retain 18,894, 10,164, and 2,412 features respectively, averaging 295.22, 158.81, and 37.69 24-byte records per chunk before vector capacity and allocator metadata. Exact measurements and tests are recorded in `PERFORMANCE.md` and `TESTING.md`.

### Objective

Make sparse features look environmentally grounded and define how future agents will obtain food, wood, and stone without prematurely implementing full ecology.

### Generated base

- Keep base placement deterministic from seed, coordinates, terrain, biome, climate, slope, and water proximity.
- Replace uniform per-cell scatter where appropriate with bounded patch, canopy, grove, outcrop, riparian, or clearing descriptors.
- Introduce species or variants only when they affect climate tolerance, resource yield, regrowth, fire, or presentation.
- Keep feature records row-major and sparse.

### Mutable state boundary

Do not put depletion, damage, ownership, growth, or fire state into the immutable generated base. Design a later sparse state/delta layer keyed by stable feature identity or position. Before physical agents land, define the minimum query/mutation contract for:

- available food from berry-bearing vegetation;
- available wood from trees;
- available stone from rocks or outcrops;
- depleted, removed, or regrowing state;
- deterministic behavior after unload/reload once persistence exists.

### Implementation area

- `FeatureKind` and `Feature` in the private `sim-core::world` module
- feature placement in `crates/sim-core/src/worldgen/mod.rs`
- renderer feature views in `crates/sim-viewer/src/renderer.rs`
- future sparse resource/delta ownership in `sim-core`

### Acceptance criteria

- Feature density and composition vary by biome and local environmental conditions.
- Forests contain deterministic clearings and edges; trees do not appear as uniform independent noise.
- Berry-bearing vegetation is available in enough suitable areas for a first-agent survival scenario.
- Rock/stone availability is not restricted to a single rare terrain class without a documented reason.
- Water, persistent snow/ice, and other invalid surfaces cannot emit incompatible features.
- Feature record size, density, and bytes per representative chunk are measured.
- Base generation remains unchanged by resource depletion or presentation state.

## Slice 6: Multi-scale renderer summaries

Status: **Implemented** on 2026-07-15. `sim-viewer` now retains one active power-of-two summary level for each resident chunk intersecting the camera rectangle plus its approximately 128-screen-pixel reuse margin. Close step-1 rendering visits authoritative cells and sparse features exactly. Coarser blocks scan every resident cell once when a chunk summary is built, render the dominant ordinary terrain as a base rectangle, and add at most one bounded minority rectangle with deterministic priority for rivers, lakes, mixed coastlines, snow, rock, or hills. A continuous river therefore survives even when none of its cells lies on the old sample coordinate. Sparse features produce at most one marker per block; marker kind is the deterministic local majority and marker area grows with total feature density, so forests and other feature-rich regions remain legible without individual rectangles.

The summary cache is presentation-only and owns no simulation truth. It stores only the active zoom step, evicts chunks outside the renderer reuse margin, invalidates chunks intersecting authoritative change bounds, and builds missing independent chunk summaries in parallel before deterministic `ChunkCoord` insertion. `World::visit_loaded_regions_in`, `visit_cells_in_chunk`, and `visit_features_in_chunk` expose exact read-only clipped/full resident coverage without exposing storage or scanning the complete chunk map for every summary. Existing revision/change-bound synchronization still skips uploads for off-cache changes and ordinary camera motion inside the margin. `SIM_VIEWER_SUMMARY_METRICS=1` reports cache bytes, instance counts, build time, and CPU upload-enqueue time; the ignored release benchmark is documented in `PERFORMANCE.md`.

### Objective

Keep terrain structure and feature density legible from close view through maximum zoom-out without scanning every full-resolution cell every frame or promoting presentation data into `sim-core` truth.

### Proposed direction

Replace coordinate-modulus feature filtering and single-cell terrain expansion with deterministic presentation summaries. Evaluate a viewer-owned, power-of-two per-chunk summary cache or an equivalent bounded structure.

Terrain summaries should preserve important minority structure such as water channels and coastlines instead of selecting only the numerically dominant ground. Feature summaries should communicate density or presence through bounded markers, tint, or another simple rectangle-based representation until an asset strategy exists.

### Implementation area

- `build_world_instances`, cache synchronization, and renderer tests in `crates/sim-viewer/src/renderer.rs`
- load/change notifications already crossing through `sim-viewer`
- renderer-cache memory and upload measurements in `Documentation/PERFORMANCE.md`

### Acceptance criteria

- A continuous major river does not disappear merely because it misses the coarse sample coordinate.
- Coastlines, lakes, mountain ranges, and major biome regions remain recognizable at full-world view.
- Forest or feature-rich areas remain distinguishable without drawing every individual feature.
- The summary cache is viewer-owned, bounded to a documented residency policy, and invalidated from authoritative world revision/change bounds.
- Camera motion within the existing cache margin does not trigger unnecessary authoritative generation or full cache rebuilds.
- Full-resolution hover and close rendering still use authoritative `sim-core` cells and features.
- Release measurements cover summary-build time, cached bytes, GPU instance count, upload time, and frame hitches during streaming.

## Slice 7: Phase 1 exit contract

Status: **Implemented** on 2026-07-15 and updated by D-055 on 2026-07-17. `sim-core` exposes allocation-free, residency-aware `World::traversal_step`, `World::water_at`, and `World::resource_at` queries. Cardinal walking reports an eight-byte derived result with signed elevation change, integer cost, and explicit water or slope blocking; the provisional maximum adjacent elevation change is 512 generator elevation units. All current deep and shallow water blocks walking, while lake and river cells are drinkable and ocean cells are not. Trees, berry bushes, and rocks remain traversable sparse resource features.

`WorldQueryError` distinguishes outside-envelope, unloaded, and non-cardinal requests. Generated terrain identity is seed plus `WorldPosition`; `Feature::identity` makes position the stable feature key within a seed and eventual generator version. Base resources remain immutable derivation, future depletion/removal remains a sparse delta, and dynamic entities remain separate. The headless `world_foundation_exit` integration scenario composes only public queries to select a deterministic dry canonical river-mouth candidate whose bounded reachable component provides fresh-water adjacency and resource access, without adding a core settlement score or retained terrain flags.

### Objective

Define the smallest stable world API needed by the first physical-agent loop and verify that the improved generator is ready to stop expanding in scope.

### Required contracts

- local traversal/passability or traversal-cost query;
- slope and water-crossing behavior;
- drinkable-water query with explicit ocean/lake/river policy;
- food, wood, and stone availability query;
- settlement-suitability inputs such as water proximity, traversable area, and resource access;
- deterministic terrain and feature identity across regeneration;
- explicit residency behavior for terrain-dependent simulation;
- documented boundary between immutable generated base, mutable sparse deltas, and dynamic entities.

### Acceptance criteria

- A headless scenario can select plausible spawn or settlement candidates using only public `sim-core` queries.
- The planned 20-100-agent physical loop can ask for movement and basic gathering information without reading renderer state or private generator internals.
- Invalid or unloaded queries have explicit behavior and tests.
- The world-quality review set passes its documented acceptance checks across representative seeds.
- Remaining world work is either required by an observed agent-loop failure or explicitly deferred.
- `Documentation/ROADMAP.md` can move the project focus from world foundation to the physical-agent loop without an unresolved critical terrain dependency.

## Cross-cutting test matrix

Each generator-changing slice should select the applicable rows rather than relying on one repository seed.

| Scale or invariant | Required evidence |
| --- | --- |
| Full envelope | Climate/terrain coverage, continent and ocean structure, dominant basin connectivity |
| Regional | Mountains, deserts, forests, lakes, tributary structure, feature density |
| Drainage seam | Identical shared flow, lake, channel, and transition behavior from both sides |
| Chunk seam | Byte-identical neighboring synthesis with no raster gap or duplicate ownership |
| Close view | Banks, wetlands, terrain transitions, feature placement, traversal meaning |
| Signed coordinates | Equivalent invariants in negative and positive regions |
| Determinism | Same seed and requests across cold/warm caches and one/many worker pools |
| Invalid input | World-edge, overflow, empty bounds, unloaded coverage, and stale load rejection |
| Performance | Release build time, allocations, retained bytes, instance count, and frame/upload timing |

Tests should prefer structural invariants over exact whole-map golden files. Small exact fixtures are appropriate for ownership, boundary, packing, precedence, and ordering rules. Visual maps remain review artifacts and should not be the only regression mechanism.

## Performance and storage checkpoints

Before accepting a representation change, record:

- `TerrainCell` size and logical bytes per full chunk;
- sparse feature record size and representative features per chunk;
- world-drainage skeleton retained and peak temporary bytes;
- `RegionMap` retained and temporary bytes after new hydrology fields;
- cold and warm region/chunk generation throughput;
- one-worker versus normal-pool deterministic throughput;
- renderer summary bytes per visible/resident chunk;
- terrain and feature GPU instance counts at close, regional, and full-world zoom;
- main-thread insertion, renderer-summary build, upload time, and worst observed frame hitch.

Measurements must use release builds, fixed seeds and bounds, and the same workload before and after a change.

## Documentation and decision updates

As slices land:

- update `Documentation/CURRENT_IMPLEMENTATION.md` for implemented behavior and remaining limitations;
- update `Documentation/ARCHITECTURE.md` for ownership, data flow, cache, and API invariants;
- append `Documentation/ARCHITECTURE_DECISIONS.md` when choosing the drainage hierarchy, climate interpretation, terrain representation, mutable feature-state boundary, or renderer-summary ownership;
- update `Documentation/TESTING.md` with the representative review commands and regression coverage;
- update `Documentation/PERFORMANCE.md` with measured sizes, timings, and budgets;
- update `Documentation/ROADMAP.md` as each slice moves from planned to active or completed.

`InitialDocumentation/` remains immutable design input throughout.

## Explicitly deferred

The following are outside this plan unless a measured or gameplay-critical dependency emerges:

- dynamic fluid simulation, floods, tides, and water pressure;
- hydraulic or thermal erosion;
- seasons and long-term climate change;
- full plant succession, animal ecology, disease, and fire spread;
- caves, volumetric geology, detailed groundwater, and ore-vein simulation;
- roads, ruins, buildings, farming, and other human modifications;
- texture, sprite, animation, and advanced lighting strategy;
- generator versioning, unloading, and persistence implementation, although their future boundaries must not be blocked;
- mutable resource depletion itself until the physical-agent slice needs it.

## Remaining decisions after Phase 1

These must be resolved in the owning slice rather than silently assumed:

- whether the provisional physical tile scale should become a durable movement/persistence contract;
- hydrologic wetland inputs and sourced-river/transition ownership;
- fertility meaning and any domain-specific settlement score beyond the implemented traversal/water/resource inputs;
- generator-version participation in persisted terrain/feature identity and the sparse mutable-state store implementation;
- quantitative world-quality thresholds that generalize across representative seeds.

## Definition of done

This plan is complete when:

- major drainage crosses regional boundaries without discontinuities;
- climate meaning matches the finite world envelope;
- terrain classes carry the minimum semantics required for movement, water, gathering, and settlement;
- sourced rivers, wetlands, and transitions are coherent and deterministic;
- features are environmentally distributed and have a clear immutable-base/mutable-state boundary;
- the viewer preserves important terrain and feature information at every supported zoom;
- representative multi-seed review, deterministic tests, and performance measurements pass;
- living documentation matches implementation;
- the complete repository validation gate succeeds; and
- the next observed blocker belongs to the physical-agent loop rather than unfinished foundational terrain.
