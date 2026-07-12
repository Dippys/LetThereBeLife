//! Engine-independent deterministic simulation foundation.

mod world;

pub use world::{
    CHUNK_SIZE, ChunkCoord, DEFAULT_INITIAL_WORLD_SIZE, Feature, FeatureKind, GenerateAreaError,
    GroundType, TerrainCell, World, WorldChunk, WorldConfig, WorldConfigError, WorldPosition,
    WorldRect,
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
    pub fn new(config: EngineConfig) -> Self {
        Self {
            world: World::generate(config.seed, config.world),
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

    pub fn apply_world_chunks(&mut self, chunks: Vec<WorldChunk>) -> usize {
        self.world.insert_chunks(chunks)
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
}
