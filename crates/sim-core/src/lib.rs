//! Engine-independent deterministic simulation foundation.

mod agent;
mod cognition;
mod diagnostics;
mod engine;
mod health;
mod life;
mod needs;
mod placements;
mod policy;
mod resources;
mod routing;
mod scheduler;
mod sleep;
mod spatial;
mod structures;
mod wildlife;

pub use agent::{
    AgentActivity, AgentId, AgentSpawnError, AgentView, EventId, MAX_PERCEPTION_CELLS,
    MAX_PERCEPTION_RADIUS, MAX_POPULATION, MoveRequestError, MovementEventOutcome,
    MovementOutcomeKind, MovementScheduled, PerceivedResource, PerceivedWater, PerceptionError,
    PhysicalPerception, PopulationInit, PopulationInitError, PopulationInitOutcome,
    RouteEventOutcome, RouteOutcomeKind, RouteScheduled, SimTime, SpawnInvalidReason,
};
pub use cognition::{
    ACQUAINTANCE_SLOTS, AcquaintanceView, AffordanceView, CONSEQUENCE_WEIGHT, Concept,
    DEFAULT_TRUST, DISTRUST, DesiredEffect, FAMILY_SIZE, FRIEND_FAMILIARITY, FaunaView, Gesture,
    GestureTopic, GriefEvent, HintOutcomeEvent, InterpretationEvent, LANDMARK_SLOTS, LEXICON_SLOTS,
    LandmarkKind, LandmarkSource, LandmarkView, LeadFollowedEvent, LessonCause, LessonEvent,
    LexiconEntryView, MERGE_RADIUS, MealEvent, MentalMapView, Mime, Personality, PolicyOptions,
    PublicSignal, READING_CANDIDATES, REPAIR_WEIGHT, Reading, ReadingReasons, RepairEvent,
    RepairResponse, RequestEvent, RequestResponse, SEARCH_SPACING, SHARE_COOLDOWN_SECONDS,
    SIGNAL_TICKS, SignalEvent, Tie, Tone, Understanding, UtteranceIntent, VISIT_TILE_SIZE,
    VISITED_TILE_SLOTS, VOCAL_FORMS, VocalForm, concept_topic,
};
pub use diagnostics::{EngineCapacityMetrics, EngineDiagnostics, EngineWorkMetrics};
pub use engine::{
    Engine, EngineCommand, EngineCommandOutcome, EngineConfig, HUNT_TICKS, MAX_SIMULATION_SPEED,
    STRIKE_RANGE, SimulationAdvanced, SimulationSnapshot, TickOutcome,
};
pub use health::{
    DeathCause, DeathRecord, HEALTH_CONSEQUENCE_INTERVAL_TICKS, HEALTH_INCAPACITATION_THRESHOLD,
    HEALTH_MAX, HealthDiagnostic, HealthDiagnosticKind, HealthStatus, HealthView, SLEEP_HEALING,
};
pub use life::{
    ADULT_AGE, ELDER_AGE, HUNTING_AGE, LifeStage, LifeView, SECONDS_PER_YEAR, Sex, WEANING_AGE,
};
pub use needs::{
    NEED_MAX, NEED_RATE_PERIOD_TICKS, NeedKind, NeedLevelView, NeedQueryError, NeedThreshold,
    NeedThresholdEventOutcome, NeedThresholdOutcomeKind, PhysicalNeedsView,
};
pub use placements::{SpawnKind, SpawnObjectError, SpawnedObjectView};
pub use policy::{
    ExplorationHeading, HOME_RANGE, PHYSICAL_POLICY_ACTION_TICKS,
    PHYSICAL_POLICY_IDLE_RECHECK_TICKS, PHYSICAL_POLICY_MAX_BACKOFF_TICKS, PHYSICAL_POLICY_RADIUS,
    PHYSICAL_POLICY_ROUTE_BUDGET, PhysicalGoal, PhysicalPolicyView, PolicyActivationError,
    PolicyDiagnostic, PolicyDiagnosticKind, PolicyFailureReason, PolicyReason,
};
pub use resources::{
    DRINK_THIRST_RELIEF, EAT_HUNGER_RELIEF, FOOD_CONSUMPTION, GATHER_YIELD,
    INVENTORY_CAPACITY_PER_KIND, InitialInventoryError, InventoryView, ResourceDeltaView,
};
pub use routing::{MAX_ROUTE_EXPANSIONS, RouteRequest, RouteRequestError};
pub use sim_world::{
    ArchiveBakeProgress, ArchiveBakeStats, BandLayout, BaseResource, BiomeType, CAMP_RADIUS,
    CHUNK_SIZE, ChunkCoord, ChunkGenerator, ChunkInspection, ChunkLoadRequest, ChunkLocalPosition,
    ChunkOverview, ChunkPresence, ClimateSample, DEFAULT_INITIAL_WORLD_SIZE, Feature, FeatureKind,
    GenerateAreaError, GeneratedCell, MAX_CHUNKS_PER_GENERATION, MAX_GENERATED_CELLS,
    MAX_GENERATED_CHUNKS, MAX_GENERATED_TERRAIN_BYTES, MAX_INITIAL_CHUNKS,
    MAX_TRAVERSABLE_ELEVATION_DELTA, Material, MaterialProperties, PrevailingWind, Standability,
    SurfaceType, TerrainCell, TerrainClass, TraversalKind, TraversalStep, VALLEY_BAND,
    VALLEY_CHILDREN_PER_FAMILY, VALLEY_FAMILIES, VALLEY_SIDE, Valley, ValleyScore,
    WORLD_GENERATION_BOUNDS, WORLD_GENERATOR_VERSION, WORLD_HALF_EXTENT, WORLD_SIDE_CELLS,
    WaterSource, World, WorldArchive, WorldArchiveError, WorldChunk, WorldChunkLoad, WorldConfig,
    WorldConfigError, WorldOverview, WorldPosition, WorldQueryError, WorldRect, band_layout,
    camp_sites, family_camps, find_valley, score_square,
};
pub use sleep::{
    REST_COLLAPSE, SleepDiagnostic, SleepDiagnosticKind, SleepInterruptionReason, SleepQuality,
    SleepRequestError, SleepView,
};
pub use structures::{
    BuildShelterError, HEARTH_BUILD_TICKS, HEARTH_STONE_COST, HEARTH_WARMTH, HEARTH_WOOD_COST,
    SHELTER_BUILD_TICKS, SHELTER_STONE_COST, SHELTER_WOOD_COST, StructureDiagnostic,
    StructureDiagnosticKind, StructureId, StructureKind, StructureState, StructureView,
};
pub use wildlife::{
    AnimalMode, AnimalView, CARCASS_TICKS, Species, SpeciesTraits, VALLEY_DEER, VALLEY_WOLVES,
    WILDLIFE_TICKS, WildlifeEvent,
};
