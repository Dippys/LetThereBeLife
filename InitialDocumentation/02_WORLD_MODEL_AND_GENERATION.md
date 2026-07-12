# World Model and Generation

## Assessment of the proposed 1,000,000 × 1,000,000 world

A one-million by one-million grid contains:

```text
1,000,000,000,000 cells
= 1 trillion cells
```

A dense three-layer representation using only one byte per layer would require approximately 3 TB before metadata, features, modifications, pathfinding, ownership, or save overhead.

Therefore:

- A world with million-scale coordinates is acceptable.
- A fully allocated trillion-cell bitmap is not.
- The world should be chunked, deterministic, generated on demand, and sparsely modified.

## Do not treat the world as an image

"Pixel" should mean a simulation cell or tile, not a permanently allocated image pixel.

Use:

- 64-bit world coordinates.
- Chunks.
- Regions.
- Deterministic procedural generation.
- Sparse feature records.
- Saved deltas for modifications.
- Derived state where possible.

## Recommended layer model

The original three-layer idea is useful conceptually but too rigid if implemented literally.

### Dense terrain layer

Store compact properties required for most cells:

- Elevation.
- Ground/substrate type.
- Moisture.
- Water state.
- Basic traversal flags.
- Optional fertility or temperature class.

Example:

```rust
#[repr(C)]
struct TerrainCell {
    elevation: u16,
    ground_type: u8,
    moisture: u8,
    flags: u8,
}
```

### Sparse feature layer

Trees, rocks, bushes, ruins, roads, buildings, stumps, crops, and fallen logs are better represented as sparse objects or chunk-local feature records.

A tree is not merely a top-layer value because it may have:

- Species.
- Age.
- Health.
- Growth.
- Seed production.
- Fruit.
- Fire state.
- Ownership.
- Multiple-tile footprint.
- Stored wood.
- Damage.

Example:

```rust
struct FeatureRecord {
    kind: FeatureKind,
    local_position: LocalPosition,
    state_index: u32,
}
```

### Geology and resource layer

Underground state can combine:

- Chunk-level geological parameters.
- Procedural density functions.
- Sparse deposit records.
- Saved excavation deltas.

Avoid storing an ore type beneath every world tile unless detailed mining requires it.

Possible data:

- Bedrock family.
- Soil depth.
- Groundwater.
- Clay probability.
- Ore veins.
- Caves.
- Deposit richness.
- Depth intervals.

## Chunk hierarchy

A reasonable starting hierarchy:

```text
Tile
  smallest detailed physical cell

Chunk
  64 × 64 or 128 × 128 tiles

Region
  16 × 16, 32 × 32, or 64 × 64 chunks

World
  addressed using signed or unsigned 64-bit coordinates
```

Example:

```text
128 × 128 tile chunk
16,384 cells per chunk
```

At 5 bytes per dense terrain cell, one raw terrain chunk is about 80 KiB before compression.

## Procedural chunk pipeline

```text
World seed + chunk coordinates
        ↓
Continental/elevation field
        ↓
Climate and rainfall
        ↓
Hydrology and water
        ↓
Substrate and soil
        ↓
Biome assignment
        ↓
Vegetation and surface features
        ↓
Geology and deposits
        ↓
Human modifications and saved deltas
```

Each stage should be deterministic and versioned.

## Stored base versus stored deltas

For untouched terrain, store only:

- World seed.
- Generator version.
- Global parameters.

For modified terrain, save:

- Removed or planted features.
- Excavation.
- Construction.
- Roads.
- Fire damage.
- Ownership or zoning where required.
- Resource depletion.
- Pollution.
- Local ecological changes.

A loaded chunk is:

```text
Generated base
+ persisted modifications
+ current dynamic objects
```

## Rivers and hydrology

Pure local noise produces visually plausible but often topologically poor rivers. Large-scale hydrology should be generated at a coarser regional level before local chunks.

Recommended approach:

1. Generate low-resolution elevation.
2. Determine drainage basins and flow.
3. Create major river paths.
4. Refine rivers within chunks.
5. Add local streams and wetlands.
6. Persist changes caused by dams, canals, or erosion if supported.

## World scale and meaning

A million-by-million-tile world may be unnecessarily large depending on tile scale.

Examples:

- 1 tile = 1 meter → 1,000 km × 1,000 km.
- 1 tile = 2 meters → 2,000 km × 2,000 km.
- 1 tile = 10 meters → 10,000 km × 10,000 km.

Choose the tile scale based on:

- Building footprint.
- Local movement.
- Farming.
- Combat.
- Visibility.
- Travel time.
- Required continent size.

Avoid choosing coordinates before defining physical scale.

## Development warning

World generation is enjoyable and can become a project-killing distraction.

The first version should contain only enough terrain to support:

- Walking.
- Water access.
- Food gathering.
- Wood and stone.
- Shelter placement.
- Settlement formation.
- Basic migration.

Do not spend months on perfect erosion, caves, and biomes before communication and agent behavior work.

## First world milestone

- Deterministic chunk coordinates.
- 64 × 64 or 128 × 128 chunks.
- Elevation.
- Dirt, grass, sand, and water.
- Sparse trees, rocks, and berries.
- Simple resource gathering.
- Chunk modification persistence.
- Chunk load/unload tests.
- Stable generation across program restarts.
