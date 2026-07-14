# World Foundation Improvement Plan

Last synchronized: 2026-07-14.

Status: **Planned**. This document sequences the remaining Phase 1 world-foundation work. It does not describe behavior as implemented unless explicitly stated as current baseline.

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
- analytic continents, oceans, coastlines, relief, plate-boundary mountains, temperature, and moisture;
- region-local priority-flood lakes and accumulated-flow rivers;
- deep water, shallow water, sand, grass, forest floor, hill, and bare-rock terrain;
- sparse deterministic trees, rocks, and berry bushes;
- bounded parallel generation and camera-bounded `wgpu` rectangle rendering.

Known limitations that motivate this plan:

- regional drainage has deliberate dry margins, so rivers, lakes, and watersheds do not cross drainage-region boundaries;
- the latitude cycle is four times the height of the finite world envelope, so playable lowlands never reach the intended polar end of the climate function;
- most moderate land collapses into grass or forest floor, while substrate, soil, wetland, tundra, snow, dry grassland, fertility, and traversal meaning are absent or conflated;
- sparse features have only a kind and position, with no resource quantity, species, lifecycle, or modification state;
- zoomed-out terrain expands one sampled cell over each coarse block, while features survive only at coordinates aligned to the sample step;
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
| 4 | Tributaries, local streams, wetlands, and transitions | Regional geography that remains convincing close up |
| 5 | Surface-feature ecology and resource readiness | Useful distributions and an explicit path to depletion state |
| 6 | Multi-scale renderer summaries | Terrain and features remain legible when zoomed out |
| 7 | Phase 1 exit contract | Passability, resource queries, and a documented handoff to agents |

## Slice 0: Repeatable world-quality baseline

### Objective

Make generator changes reviewable with the same seeds, coordinates, scales, statistics, and seam locations every time.

### Implementation area

- `crates/sim-core/examples/render_map.rs`
- focused world-generation test helpers under `crates/sim-core/src/worldgen/`
- `Documentation/TESTING.md`
- `Documentation/PERFORMANCE.md`

### Deliverables

- Define a small representative seed set containing the repository seed plus seeds chosen for different continent, mountain, desert, forest, lake, and river layouts.
- Define canonical full-envelope, regional, drainage-seam, coastline, river-mouth, lake, mountain, and close-up views.
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

- `GroundType`, `TerrainCell`, and inspection APIs in `crates/sim-core/src/world.rs`
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

## Slice 4: Tributaries, local streams, wetlands, and transitions

### Objective

Add the smaller-scale water and transition structures that make regional geography believable after major drainage continuity is correct.

### Deliverables

- Derive tributary hierarchy or stream order from canonical accumulated flow.
- Add narrower local streams at a lower threshold than major rivers, with explicit minimum visible width rules by zoom level.
- Generate wetlands from shallow water table, low slope, floodplain proximity, basin edges, or poorly drained terrain rather than unrelated noise.
- Refine coastline, beach, riverbank, lake-edge, biome-edge, and treeline transitions within bounded deterministic rules.
- Preserve water-body connectivity through local detail; detail noise must not sever a channel selected by the drainage hierarchy.

### Acceptance criteria

- Tributaries join downstream channels rather than crossing or stopping beside them.
- Local streams have a valid downstream path and do not originate from visual noise alone.
- Wetlands occur in hydrologically plausible places and remain absent from steep or arid terrain unless explicitly justified.
- Coast and bank transitions remain recognizable at close and regional scales.
- Chunk and region seams do not change channel width, wetland classification, or transition ownership.
- Per-chunk river lookup remains bounded; any replacement for the current 25-segment bound has a proof, assertion, or measured bounded structure.

## Slice 5: Surface-feature ecology and resource readiness

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

- `FeatureKind` and `Feature` in `crates/sim-core/src/world.rs`
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

## Open decisions

These must be resolved in the owning slice rather than silently assumed:

- physical tile scale and the geographic meaning of the complete envelope;
- cold-warm-cold world latitude versus a narrower regional climate slice;
- coarse drainage-skeleton resolution and cache lifetime;
- explicit water-body identity representation;
- expanded `GroundType` versus separate compact surface and biome classifications;
- which terrain properties are stored versus derived on query;
- stable feature identity and sparse mutable-state keying;
- renderer summary representation and residency budget;
- quantitative world-quality thresholds that generalize across representative seeds.

## Definition of done

This plan is complete when:

- major drainage crosses regional boundaries without discontinuities;
- climate meaning matches the finite world envelope;
- terrain classes carry the minimum semantics required for movement, water, gathering, and settlement;
- local streams, wetlands, and transitions are coherent and deterministic;
- features are environmentally distributed and have a clear immutable-base/mutable-state boundary;
- the viewer preserves important terrain and feature information at every supported zoom;
- representative multi-seed review, deterministic tests, and performance measurements pass;
- living documentation matches implementation;
- the complete repository validation gate succeeds; and
- the next observed blocker belongs to the physical-agent loop rather than unfinished foundational terrain.
