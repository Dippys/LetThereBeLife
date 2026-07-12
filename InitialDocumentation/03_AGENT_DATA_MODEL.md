# Agent Data Model

## Core agent properties

Each persistent agent may have:

- Stable identity.
- Birth time and age.
- Sex or reproductive role.
- Physical traits and health.
- Family relationships.
- Tribe, settlement, faction, and household memberships.
- Occupation.
- Personality.
- Needs.
- Emotions.
- Skills.
- Goals.
- Current plans and commitments.
- Memories.
- Beliefs.
- Relationships.
- Personal lexicon and language knowledge.
- Cultural norms.
- Location or journey state.
- Inventory and ownership references.
- Reputation.
- Legal or social obligations.

Not every property belongs inside one monolithic struct.

## Stable identity

Use compact stable IDs, usually 32-bit for up to ten million agents.

```rust
type AgentId = u32;
```

If stale references after deletion are a concern, use generational validation in external tables without expanding every stored relationship record.

## Agent core

A compact core might contain:

```rust
#[repr(C)]
struct AgentCore {
    region_id: u32,
    location_or_journey: u32,
    next_event_tick: u64,
    birth_tick: u64,
    household_id: u32,
    settlement_id: u32,
    current_goal: u16,
    activity: u16,
    flags: u32,
}
```

Exact layout should be benchmarked for alignment and cache behavior.

## Variable-length indexes

```rust
struct AgentMindIndex {
    relationship_range: PoolRange,
    belief_range: PoolRange,
    memory_range: PoolRange,
    lexicon_range: PoolRange,
    inventory_range: PoolRange,
}
```

A pool range can contain an offset and count:

```rust
struct PoolRange {
    start: u32,
    count: u16,
    capacity_class: u16,
}
```

## Needs

Needs should be analytically updated rather than ticked continuously.

```rust
struct LinearNeed {
    value_at_reference: u16,
    rate_per_time_unit: i16,
    reference_tick: u64,
}
```

Possible needs:

- Hunger.
- Thirst.
- Rest.
- Warmth.
- Safety.
- Pain relief.
- Belonging.
- Affection.
- Status.
- Curiosity.
- Autonomy.

Not every need requires equal detail for every agent.

## Personality

Use a compact trait vector rather than a class hierarchy.

Possible dimensions:

- Sociability.
- Aggression.
- Caution.
- Curiosity.
- Conformity.
- Empathy.
- Patience.
- Honesty.
- Ambition.
- Loyalty.
- Risk tolerance.
- Impulsivity.
- Memory retention.
- Learning ability.

Traits should influence utilities, not determine behavior absolutely.

## Skills

Skills may include:

- Foraging.
- Hunting.
- Farming.
- Building.
- Cooking.
- Healing.
- Negotiation.
- Leadership.
- Combat.
- Teaching.
- Translation.
- Navigation.
- Crafting.
- Mining.

Store only learned or relevant skills. Common default competence may be derived from age, culture, and occupation.

## Current activity versus long-term state

An NPC should have a small immediate activity state:

```text
Sleeping
Walking
Working
Eating
Talking
Fighting
Caring for child
Waiting
Traveling
Recovering
```

Long-term goals and plans live separately.

## Family representation

Family links should be explicit:

- Biological or social parents.
- Children.
- Siblings where required.
- Partners.
- Household.
- Clan or lineage.

Avoid duplicating complete family trees inside every NPC. Store central family records or adjacency lists.

## Physical state

Possible physical fields:

- Health.
- Injuries.
- Disease.
- Pregnancy.
- Fatigue.
- Temperature exposure.
- Nutrition.
- Disability.
- Age-related decline.

Physical systems can schedule future events such as recovery, worsening infection, birth, or death.

## Agent lifecycle

```text
Birth
↓
Infancy
↓
Childhood
↓
Adolescence
↓
Adulthood
↓
Old age
↓
Death
```

Death should:

- Stop scheduling ordinary cognition.
- Preserve historical identity.
- Update relatives and institutions.
- Transfer ownership or obligations.
- Create memories and beliefs in survivors.
- Optionally archive cold state to cheaper historical storage.
