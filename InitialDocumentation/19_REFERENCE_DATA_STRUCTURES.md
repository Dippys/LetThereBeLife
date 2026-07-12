# Reference Data Structures

These structures are sketches, not frozen layouts.

## IDs

```rust
type AgentId = u32;
type RegionId = u32;
type SettlementId = u32;
type HouseholdId = u32;
type EntityId = u32;
type SignalId = u32;
type ConceptId = u32;
type MemoryId = u32;
type BeliefId = u32;
type SimTime = u64;
```

## Agent core

```rust
#[repr(C)]
struct AgentCore {
    next_event: SimTime,
    birth_time: SimTime,

    region: RegionId,
    spatial_ref: u32,

    household: HouseholdId,
    settlement: SettlementId,

    current_goal: u16,
    current_activity: u16,

    need_state_index: u32,
    mind_index: u32,

    flags: u32,
}
```

## Need state

```rust
#[repr(C)]
struct NeedValue {
    value_at_reference: u16,
    rate: i16,
    reference_time: SimTime,
}
```

## Relationship

```rust
#[repr(C)]
struct RelationshipRecord {
    other: AgentId,
    trust: i16,
    affection: i16,
    fear: i16,
    respect: i16,
    resentment: i16,
    obligation: i16,
    last_interaction_day: u32,
}
```

## Belief

```rust
enum BeliefSource {
    DirectPerception,
    Inference,
    Communication {
        speaker: Option<AgentId>,
        interpretation_confidence: u16,
    },
    CulturalTeaching,
    Rumor,
    MemoryReconstruction,
}

struct Belief {
    proposition: Proposition,
    confidence: u16,
    source: BeliefSource,
    acquired_at: SimTime,
    last_confirmed: Option<SimTime>,
    emotional_weight: i16,
}
```

## Proposition

```rust
enum Proposition {
    EntityProperty {
        entity: EntityRef,
        property: ConceptId,
    },

    Relation {
        relation: ConceptId,
        subject: EntityRef,
        object: EntityRef,
    },

    Event {
        action: ConceptId,
        actor: Option<EntityRef>,
        patient: Option<EntityRef>,
        destination: Option<EntityRef>,
        time: TemporalReference,
    },

    Negated(Box<Proposition>),

    Uncertain {
        proposition: Box<Proposition>,
        confidence: u16,
    },
}
```

Production code may use a compact arena representation rather than boxed recursive enums.

## Memory

```rust
#[repr(C)]
struct EpisodicMemory {
    event_type: u16,
    flags: u16,
    subject: u32,
    object: u32,
    location: u32,
    time: u32,
    emotional_strength: i16,
    confidence: u16,
}
```

## Lexicon

```rust
#[repr(C)]
struct LexicalAssociation {
    signal: SignalId,
    concept: ConceptId,
    positive_evidence: u16,
    contradictory_evidence: u16,
    familiarity: u16,
    flags: u16,
}
```

## Private communication intent

```rust
struct UtteranceIntent {
    sender: AgentId,
    target: Option<AgentId>,
    desired_effect: DesiredEffect,
    proposition: Option<PropositionId>,
    expected_outcomes: SmallVec<[ExpectedOutcome; 4]>,
}
```

## Public signal event

```rust
struct SignalEvent {
    sender: Option<AgentId>,
    vocal_sequence: SmallVec<[SignalId; 6]>,
    gestures: SmallVec<[GestureId; 4]>,
    gaze_target: Option<EntityId>,
    pointed_target: Option<EntityId>,
    emotional_tone: EmotionVector,
    intensity: u16,
    origin: WorldPosition,
    timestamp: SimTime,
}
```

## Interpretation

```rust
struct InterpretationHypothesis {
    proposition: PropositionId,
    communicative_force: DesiredEffect,
    score: i32,
    probability: u16,
}
```

## Pending communication

```rust
struct PendingCommunication {
    private_intent_id: u32,
    signal_event_id: u32,
    evaluation_deadline: SimTime,
}
```

## World terrain

```rust
#[repr(C)]
struct TerrainCell {
    elevation: u16,
    ground_type: u8,
    moisture: u8,
    flags: u8,
}
```

## Sparse feature

```rust
struct FeatureRecord {
    kind: FeatureKind,
    local_position: LocalPosition,
    state_index: u32,
}
```

## Journey

```rust
struct Journey {
    agent: AgentId,
    route: u32,
    departure: SimTime,
    arrival: SimTime,
    origin: RegionId,
    destination: RegionId,
    progress_flags: u16,
}
```

## Event

```rust
enum AgentEvent {
    ReevaluateGoal(AgentId),
    NeedThreshold(AgentId, NeedKind),
    ArriveAtDestination(AgentId),
    WorkShiftStarts(AgentId),
    CommunicationReceived(AgentId, SignalEventId),
    DangerPerceived(AgentId, DangerEventId),
    SocialCommitmentDue(AgentId),
    MemoryReview(AgentId),
}
```

## Region message

```rust
enum RegionMessage {
    AgentArriving(AgentTransfer),
    CaravanArriving(CaravanId),
    NewsCarried(InformationPacket),
    ArmyEntering(ArmyId),
    ResourceShipment(ShipmentId),
}
```
