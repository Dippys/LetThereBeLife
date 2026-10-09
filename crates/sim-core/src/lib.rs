//! Engine-independent deterministic simulation foundation.

mod agent;
mod cognition;
mod diagnostics;
mod engine;
mod health;
mod needs;
mod placements;
mod policy;
mod resources;
mod routing;
mod scheduler;
mod sleep;
mod spatial;
mod structures;

pub use agent::{
    AgentActivity, AgentId, AgentSpawnError, AgentView, EventId, MAX_PERCEPTION_CELLS,
    MAX_PERCEPTION_RADIUS, MAX_POPULATION, MoveRequestError, MovementEventOutcome,
    MovementOutcomeKind, MovementScheduled, PerceivedResource, PerceivedWater, PerceptionError,
    PhysicalPerception, PopulationInit, PopulationInitError, PopulationInitOutcome,
    RouteEventOutcome, RouteOutcomeKind, RouteScheduled, SimTime, SpawnInvalidReason,
};
pub use cognition::{
    LANDMARK_SLOTS, LandmarkKind, LandmarkSource, LandmarkView, MERGE_RADIUS, MentalMapView,
    PolicyOptions, SEARCH_SPACING, SHARE_COOLDOWN_SECONDS, SIGNAL_TICKS, SignalEvent,
    VISIT_TILE_SIZE, VISITED_TILE_SLOTS,
};
pub use diagnostics::{EngineCapacityMetrics, EngineDiagnostics, EngineWorkMetrics};
pub use engine::{
    Engine, EngineCommand, EngineCommandOutcome, EngineConfig, MAX_SIMULATION_SPEED,
    SimulationSnapshot, TickOutcome,
};
pub use health::{
    DeathCause, DeathRecord, HEALTH_CONSEQUENCE_INTERVAL_TICKS, HEALTH_INCAPACITATION_THRESHOLD,
    HEALTH_MAX, HealthDiagnostic, HealthDiagnosticKind, HealthStatus, HealthView,
};
pub use needs::{
    NEED_MAX, NEED_RATE_PERIOD_TICKS, NeedKind, NeedLevelView, NeedQueryError, NeedThreshold,
    NeedThresholdEventOutcome, NeedThresholdOutcomeKind, PhysicalNeedsView,
};
pub use placements::{SpawnKind, SpawnObjectError, SpawnedObjectView};
pub use policy::{
    CURIOSITY_TARGET, EXCURSION_EVERY, ExplorationHeading, FOOD_RESERVE, HOME_RANGE,
    PHYSICAL_POLICY_ACTION_TICKS, PHYSICAL_POLICY_IDLE_RECHECK_TICKS,
    PHYSICAL_POLICY_MAX_BACKOFF_TICKS, PHYSICAL_POLICY_RADIUS, PHYSICAL_POLICY_ROUTE_BUDGET,
    PREPARE_EXPOSURE, PhysicalGoal, PhysicalPolicyView, PolicyActivationError, PolicyDiagnostic,
    PolicyDiagnosticKind, PolicyFailureReason, PolicyReason, TOP_UP_HUNGER, TOP_UP_THIRST,
};
pub use resources::{
    DRINK_THIRST_RELIEF, EAT_HUNGER_RELIEF, FOOD_CONSUMPTION, GATHER_YIELD,
    INVENTORY_CAPACITY_PER_KIND, InitialInventoryError, InventoryView, ResourceDeltaView,
};
pub use routing::{MAX_ROUTE_EXPANSIONS, RouteRequest, RouteRequestError};
pub use sim_world::{
    ArchiveBakeProgress, ArchiveBakeStats, BaseResource, BiomeType, CHUNK_SIZE, ChunkCoord,
    ChunkGenerator, ChunkInspection, ChunkLoadRequest, ChunkLocalPosition, ChunkOverview,
    ChunkPresence, ClimateSample, DEFAULT_INITIAL_WORLD_SIZE, Feature, FeatureKind,
    GenerateAreaError, GeneratedCell, MAX_CHUNKS_PER_GENERATION, MAX_GENERATED_CELLS,
    MAX_GENERATED_CHUNKS, MAX_GENERATED_TERRAIN_BYTES, MAX_INITIAL_CHUNKS,
    MAX_TRAVERSABLE_ELEVATION_DELTA, PrevailingWind, ResourceKind, Standability, SurfaceType,
    TerrainCell, TerrainClass, TraversalKind, TraversalStep, WORLD_GENERATION_BOUNDS,
    WORLD_GENERATOR_VERSION, WORLD_HALF_EXTENT, WORLD_SIDE_CELLS, WaterSource, World, WorldArchive,
    WorldArchiveError, WorldChunk, WorldChunkLoad, WorldConfig, WorldConfigError, WorldOverview,
    WorldPosition, WorldQueryError, WorldRect,
};
pub use sleep::{
    SleepDiagnostic, SleepDiagnosticKind, SleepInterruptionReason, SleepQuality, SleepRequestError,
    SleepView,
};
pub use structures::{
    BuildShelterError, SHELTER_BUILD_TICKS, SHELTER_STONE_COST, SHELTER_WOOD_COST,
    StructureDiagnostic, StructureDiagnosticKind, StructureId, StructureKind, StructureState,
    StructureView,
};
