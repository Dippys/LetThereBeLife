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
mod season;
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
    ACQUAINTANCE_SLOTS, AcquaintanceView, Activity, AffordanceView, BeliefView, CONSEQUENCE_WEIGHT,
    Choice, Concept, CoupleEvent, CraftEvent, DEFAULT_TRUST, DISTRUST, DRIFT_LIMIT, DesiredEffect,
    EFFECTS, EffectView, FAMILY_SIZE, FRIEND_FAMILIARITY, Facts, FarmEvent, FaunaView, FireEvent,
    Gesture, GestureTopic, Goal, GriefEvent, HABIT_SLOTS, HabitView, HintOutcomeEvent,
    IntentionView, InterpretationEvent, LANDMARK_SLOTS, LEXICON_SLOTS, LandmarkKind,
    LandmarkSource, LandmarkView, LeadFollowedEvent, LessonCause, LessonEvent, LexiconEntryView,
    MERGE_RADIUS, MealEvent, MentalMapView, Mime, PLAN_STEPS, Personality, PlanEvent,
    PlanEventKind, PublicSignal, READING_CANDIDATES, REPAIR_WEIGHT, Reading, ReadingReasons,
    RepairEvent, RepairResponse, RequestEvent, RequestResponse, SEARCH_SPACING,
    SHARE_COOLDOWN_SECONDS, SIGNAL_TICKS, SignalEvent, Situation, Step, Tie, Tone, Understanding,
    UtteranceIntent, VISIT_TILE_SIZE, VISITED_TILE_SLOTS, VOCAL_FORMS, VocalForm, WordEvent,
    concept_topic,
};
pub use diagnostics::{EngineCapacityMetrics, EngineDiagnostics, EngineWorkMetrics};
pub use engine::{
    BodyState, BurialEvent, Engine, EngineCommand, EngineCommandOutcome, EngineConfig, FamilyEvent,
    HUNT_TICKS, MAX_SIMULATION_SPEED, MotherhoodView, NameEvent, STRIKE_RANGE, SimulationAdvanced,
    SimulationSnapshot, TickOutcome,
};
pub use health::{
    DeathCause, DeathRecord, HEALTH_CONSEQUENCE_INTERVAL_TICKS, HEALTH_INCAPACITATION_THRESHOLD,
    HEALTH_MAX, HealthDiagnostic, HealthDiagnosticKind, HealthStatus, HealthView, SLEEP_HEALING,
};
pub use life::{
    ADULT_AGE, ELDER_AGE, HUNTING_AGE, LifeStage, LifeView, MONTH_SECONDS, Name, SECONDS_PER_YEAR,
    Sex, TICKS_PER_YEAR, WEANING_AGE,
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
    CARRY_CAPACITY, DRINK_THIRST_RELIEF, EAT_HUNGER_RELIEF, FOOD_CONSUMPTION, GATHER_YIELD,
    InitialInventoryError, InventoryView, ResourceDeltaView,
};
pub use routing::{MAX_ROUTE_EXPANSIONS, RouteRequest, RouteRequestError};
pub use season::{
    DAY_SECONDS, NIGHTFALL_SECONDS, SEASON_SECONDS, SEASON_TICKS, Season, day_fraction, is_night,
};
pub use sim_world::{
    ArchiveBakeProgress, ArchiveBakeStats, BandLayout, BaseResource, BiomeType, CAMP_RADIUS,
    CHUNK_SIZE, ChunkCoord, ChunkGenerator, ChunkInspection, ChunkLoadRequest, ChunkLocalPosition,
    ChunkOverview, ChunkPresence, ClimateSample, DEFAULT_INITIAL_WORLD_SIZE, Feature, FeatureKind,
    GenerateAreaError, GeneratedCell, Handling, Kinds, MAX_CHUNKS_PER_GENERATION,
    MAX_GENERATED_CELLS, MAX_GENERATED_CHUNKS, MAX_GENERATED_TERRAIN_BYTES, MAX_INITIAL_CHUNKS,
    MAX_TRAVERSABLE_ELEVATION_DELTA, Material, MaterialProperties, Method, PrevailingWind, RECIPES,
    Recipe, Standability, SurfaceType, TerrainCell, TerrainClass, TraversalKind, TraversalStep,
    VALLEY_BAND, VALLEY_CHILDREN_PER_FAMILY, VALLEY_FAMILIES, VALLEY_SIDE, Valley, ValleyScore,
    WORLD_GENERATION_BOUNDS, WORLD_GENERATOR_VERSION, WORLD_HALF_EXTENT, WORLD_SIDE_CELLS,
    WaterSource, World, WorldArchive, WorldArchiveError, WorldChunk, WorldChunkLoad, WorldConfig,
    WorldConfigError, WorldOverview, WorldPosition, WorldQueryError, WorldRect, apart_layout,
    band_layout, camp_sites, canonical, family_camps, find_valley, find_valley_pair, score_square,
};
pub use sleep::{
    FIRE_WARMTH_REACH, REST_COLLAPSE, SleepDiagnostic, SleepDiagnosticKind,
    SleepInterruptionReason, SleepQuality, SleepRequestError, SleepView,
};
pub use structures::{
    BuildShelterError, HEARTH_BUILD_TICKS, HEARTH_STONE_COST, HEARTH_WARMTH, HEARTH_WOOD_COST,
    HUT_STORE_CAPACITY, Purpose, RACK_WOOD_COST, REPAIR_COST, SHELTER_BUILD_TICKS,
    SHELTER_STONE_COST, SHELTER_WOOD_COST, STONE_HUT_STONE, StructureDiagnostic,
    StructureDiagnosticKind, StructureId, StructureKind, StructureState, StructureView,
};
pub use wildlife::{
    AnimalMode, AnimalView, CARCASS_TICKS, Species, SpeciesTraits, VALLEY_DEER, VALLEY_WOLVES,
    WILDLIFE_TICKS, WildlifeEvent,
};
