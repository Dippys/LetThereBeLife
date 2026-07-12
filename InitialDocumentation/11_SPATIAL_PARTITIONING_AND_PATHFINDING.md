# Spatial Partitioning and Pathfinding

## Avoid N-squared behavior

Never compare every NPC with every other NPC.

Use:

- Tile or chunk buckets.
- Spatial hashes.
- Regions.
- Quadtree only where useful.
- Settlement indexes.
- Household indexes.
- Known-contact indexes.
- Event-specific subscriptions.

Perception should be approximately:

```text
O(N × local neighbors)
```

not `O(N²)`.

## Spatial hierarchy

```text
Tile
↓
Chunk
↓
Region
↓
World
```

Each chunk tracks:

- Present agents.
- dynamic objects.
- active signals.
- hazards.
- loaded features.
- local path data.

## Perception queries

An agent should inspect only:

- Nearby visible objects.
- Nearby audible events.
- Known targets.
- current activity.
- relevant local hazards.
- socially expected contacts.

## Communication fan-out

When an agent produces a signal:

1. Query hearing or visibility radius.
2. Filter by obstacles and modality.
3. Test attention.
4. Apply noise.
5. Create receiver-specific observations.
6. Interpret only for surviving listeners.

## Movement levels

### Detailed local movement

For nearby active agents:

- Tile movement.
- local collision.
- obstacle avoidance.
- exact line of sight.
- short pathfinding.

### Route-level movement

For ordinary travel:

- Named road or path.
- departure.
- expected arrival.
- waypoints.
- hazards.
- derived position.

### Long-distance journey

For distant travel:

- Origin.
- destination.
- route.
- caravan or group.
- departure and arrival.
- planned stops.
- event hooks.

Every traveler remains an individual even when movement is analytically derived.

## Pathfinding techniques

- Hierarchical A*.
- Chunk portals.
- regional navigation graph.
- path caching.
- route reuse.
- flow fields for crowds.
- queued path requests.
- invalidation only after relevant changes.
- abstract long-distance routes.
- local refinement near active areas.

## Path data ownership

Avoid storing a complete path vector inside every traveler.

Use:

- Shared route IDs.
- start progress.
- current segment.
- destination.
- deviation state.

## Dynamic obstacles

Changes such as fire, battle, construction, or flooding should invalidate only affected local paths or route edges.

## Settlement movement

Common repeated routes can become habits:

- Home to field.
- home to well.
- workshop to market.
- village to neighboring village.

Habits reduce planning and pathfinding cost.

## Large crowds

For armies, festivals, or evacuations:

- Shared high-level destination.
- flow fields.
- local separation.
- subgroup leadership.
- communication and panic events.

Individuals retain decisions and morale but need not each calculate an independent full path.
