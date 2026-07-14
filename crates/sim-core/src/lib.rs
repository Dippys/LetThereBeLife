//! Engine-independent deterministic simulation foundation.

mod world;
mod worldgen;

pub use world::{
    CHUNK_SIZE, ChunkCoord, ChunkGenerator, ChunkInspection, ChunkLoadRequest, ChunkLocalPosition,
    ChunkPresence, ClimateSample, DEFAULT_INITIAL_WORLD_SIZE, Feature, FeatureKind,
    GenerateAreaError, GeneratedCell, GroundType, MAX_CHUNKS_PER_GENERATION, MAX_GENERATED_CELLS,
    MAX_GENERATED_CHUNKS, MAX_GENERATED_TERRAIN_BYTES, MAX_INITIAL_CHUNKS, PrevailingWind,
    TerrainCell, WORLD_GENERATION_BOUNDS, WORLD_HALF_EXTENT, WORLD_SIDE_CELLS, World, WorldChunk,
    WorldChunkLoad, WorldConfig, WorldConfigError, WorldPosition, WorldRect,
};

use std::time::Duration;

/// Immutable settings used to construct or reset a simulation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EngineConfig {
    pub seed: u64,
    pub ticks_per_second: u32,
    pub world: WorldConfig,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            seed: 1,
            ticks_per_second: 60,
            world: WorldConfig::default(),
        }
    }
}

impl EngineConfig {
    pub fn tick_duration(self) -> Duration {
        Duration::from_secs_f64(1.0 / f64::from(self.ticks_per_second.max(1)))
    }
}

/// Commands are the only public mutation route used by clients.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EngineCommand {
    TogglePause,
    SetPaused(bool),
    SetSpeed(f32),
    Reset,
    GenerateWorldArea(WorldRect),
}

/// Read-only data intended for renderers, tools, and remote clients.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SimulationSnapshot {
    pub tick: u64,
    pub simulated_seconds: f64,
    pub paused: bool,
    pub speed: f32,
    pub seed: u64,
}

/// Owns simulation state. Presentation code must not mutate its fields directly.
#[derive(Debug)]
pub struct Engine {
    config: EngineConfig,
    world: World,
    tick: u64,
    paused: bool,
    speed: f32,
}

impl Engine {
    /// Creates deterministic simulation state without synchronously materializing terrain.
    ///
    /// Call [`Self::materialize_initial_area`] for eager headless workflows. The
    /// viewer instead streams explicit [`WorldChunkLoad`] payloads through the
    /// main-thread insertion boundary.
    pub fn new(config: EngineConfig) -> Self {
        Self {
            world: World::new(config.seed, config.world),
            config,
            tick: 0,
            paused: false,
            speed: 1.0,
        }
    }

    pub fn command(&mut self, command: EngineCommand) {
        match command {
            EngineCommand::TogglePause => self.paused = !self.paused,
            EngineCommand::SetPaused(paused) => self.paused = paused,
            EngineCommand::SetSpeed(speed) if speed.is_finite() => {
                self.speed = speed.clamp(0.0, 64.0);
            }
            EngineCommand::SetSpeed(_) => {}
            EngineCommand::Reset => {
                self.tick = 0;
                self.paused = false;
                self.speed = 1.0;
            }
            EngineCommand::GenerateWorldArea(bounds) => {
                let _ = self.world.generate_area(bounds);
            }
        }
    }

    /// Advances exactly one deterministic simulation tick.
    pub fn tick(&mut self) {
        if !self.paused {
            self.tick = self.tick.saturating_add(1);
        }
    }

    pub fn config(&self) -> EngineConfig {
        self.config
    }

    /// Returns immutable world state for headless tools and presentation clients.
    pub fn world(&self) -> &World {
        &self.world
    }

    pub fn apply_world_chunks(
        &mut self,
        chunks: Vec<WorldChunk>,
    ) -> Result<usize, GenerateAreaError> {
        self.world.insert_chunks(chunks)
    }

    /// Applies bootstrap-aware worker payloads while retaining authoritative
    /// world ownership in `sim-core`.
    ///
    /// This changes only the deterministic terrain materialization cache. It
    /// does not advance, rewind, or otherwise alter fixed simulation time.
    pub fn apply_world_chunk_loads(
        &mut self,
        loads: Vec<WorldChunkLoad>,
    ) -> Result<usize, GenerateAreaError> {
        self.world.insert_chunk_loads(loads)
    }

    /// Eagerly materializes the configured bootstrap rectangle for headless
    /// callers that need complete startup coverage before advancing.
    pub fn materialize_initial_area(&mut self) -> Result<(), GenerateAreaError> {
        self.world.materialize_initial_area()
    }

    pub fn snapshot(&self) -> SimulationSnapshot {
        SimulationSnapshot {
            tick: self.tick,
            simulated_seconds: self.tick as f64 / f64::from(self.config.ticks_per_second.max(1)),
            paused: self.paused,
            speed: self.speed,
            seed: self.config.seed,
        }
    }
}

impl Default for Engine {
    fn default() -> Self {
        Self::new(EngineConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_inputs_produce_identical_snapshots() {
        let mut left = Engine::default();
        let mut right = Engine::default();
        for _ in 0..1_000 {
            left.tick();
            right.tick();
        }
        assert_eq!(left.snapshot(), right.snapshot());
    }

    #[test]
    fn pause_prevents_time_advancing() {
        let mut engine = Engine::default();
        engine.command(EngineCommand::SetPaused(true));
        engine.tick();
        assert_eq!(engine.snapshot().tick, 0);
    }

    #[test]
    fn reset_restores_runtime_state_but_preserves_config() {
        let config = EngineConfig {
            seed: 42,
            ticks_per_second: 20,
            ..EngineConfig::default()
        };
        let mut engine = Engine::new(config);
        engine.tick();
        engine.command(EngineCommand::SetSpeed(8.0));
        engine.command(EngineCommand::Reset);
        assert_eq!(engine.config(), config);
        assert_eq!(engine.snapshot().tick, 0);
        assert_eq!(engine.snapshot().speed, 1.0);
    }

    #[test]
    fn equal_seeds_generate_equal_worlds() {
        let left = Engine::new(EngineConfig {
            seed: 99,
            ..EngineConfig::default()
        });
        let right = Engine::new(EngineConfig {
            seed: 99,
            ..EngineConfig::default()
        });

        assert_eq!(left.world(), right.world());
    }

    #[test]
    fn engine_construction_is_deferred_and_headless_materialization_is_explicit() {
        let config = EngineConfig {
            seed: 99,
            world: WorldConfig::new(96, 64).unwrap(),
            ..EngineConfig::default()
        };
        let mut engine = Engine::new(config);

        assert_eq!(engine.world().loaded_chunk_count(), 0);
        assert!(
            !engine
                .world()
                .area_is_generated(engine.world().initial_bounds())
        );
        engine.materialize_initial_area().unwrap();
        let eager = World::generate(config.seed, config.world);
        assert_eq!(
            engine.world().cells().collect::<Vec<_>>(),
            eager.cells().collect::<Vec<_>>()
        );
        assert_eq!(
            engine.world().all_features().copied().collect::<Vec<_>>(),
            eager.all_features().copied().collect::<Vec<_>>()
        );
    }

    #[test]
    fn terrain_materialization_does_not_change_fixed_tick_progression() {
        let config = EngineConfig {
            seed: 99,
            world: WorldConfig::new(96, 64).unwrap(),
            ..EngineConfig::default()
        };
        let mut unloaded = Engine::new(config);
        let mut resident = Engine::new(config);
        let loads = resident
            .world()
            .missing_chunk_load_requests(resident.world().initial_bounds())
            .unwrap()
            .into_iter()
            .map(|request| World::generate_chunk_load(config.seed, request))
            .collect();

        assert_eq!(resident.apply_world_chunk_loads(loads), Ok(4));
        for _ in 0..600 {
            unloaded.tick();
            resident.tick();
        }

        assert_eq!(unloaded.snapshot(), resident.snapshot());
        assert_eq!(unloaded.snapshot().tick, 600);
    }

    #[test]
    fn generate_initial_area_command_uses_bootstrap_batching() {
        let config = EngineConfig {
            seed: 99,
            world: WorldConfig::new(128, 64).unwrap(),
            ..EngineConfig::default()
        };
        let mut engine = Engine::new(config);
        let initial = engine.world().initial_bounds();

        engine.command(EngineCommand::GenerateWorldArea(initial));

        assert!(engine.world().area_is_generated(initial));
        assert_eq!(engine.world().loaded_chunk_count(), 4);
    }

    #[test]
    fn applied_chunks_remain_engine_owned_and_deduplicated() {
        let config = EngineConfig {
            seed: 9,
            world: WorldConfig::new(64, 64).unwrap(),
            ..EngineConfig::default()
        };
        let mut engine = Engine::new(config);
        let coord = ChunkCoord { x: -1, y: 0 };
        let chunk = World::generate_chunk_at(config.seed, coord).unwrap();

        assert_eq!(engine.apply_world_chunks(vec![chunk.clone()]), Ok(1));
        let revision = engine.world().revision();
        assert_eq!(
            engine
                .world()
                .inspect_chunk_at(WorldPosition { x: -1, y: 0 })
                .unwrap()
                .presence,
            ChunkPresence::RetainedPartialInitial
        );
        assert_eq!(engine.apply_world_chunks(vec![chunk]), Ok(0));
        assert_eq!(engine.world().revision(), revision);
    }
}
