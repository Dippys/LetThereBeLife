//! Shared application configuration loading for runtime entry points.

use std::{error::Error, fmt, fs, path::Path};

use serde::Deserialize;
use sim_core::{EngineConfig, WorldConfig, WorldConfigError};

pub const DEFAULT_CONFIG_PATH: &str = "config/simulation.toml";

#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AppConfig {
    pub simulation: SimulationConfig,
    pub world: InitialWorldConfig,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SimulationConfig {
    pub seed: u64,
    pub ticks_per_second: u32,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct InitialWorldConfig {
    pub initial_width: u32,
    pub initial_height: u32,
}

impl AppConfig {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, ConfigError> {
        let path = path.as_ref();
        let source = fs::read_to_string(path).map_err(|source| ConfigError::Read {
            path: path.display().to_string(),
            source,
        })?;
        toml::from_str(&source).map_err(ConfigError::Parse)
    }

    pub fn engine_config(self) -> Result<EngineConfig, ConfigError> {
        let world = WorldConfig::new(self.world.initial_width, self.world.initial_height)
            .map_err(ConfigError::InvalidWorld)?;
        Ok(EngineConfig {
            seed: self.simulation.seed,
            ticks_per_second: self.simulation.ticks_per_second,
            world,
        })
    }
}

impl Default for SimulationConfig {
    fn default() -> Self {
        let config = EngineConfig::default();
        Self {
            seed: config.seed,
            ticks_per_second: config.ticks_per_second,
        }
    }
}

impl Default for InitialWorldConfig {
    fn default() -> Self {
        let config = WorldConfig::default();
        Self {
            initial_width: config.initial_width(),
            initial_height: config.initial_height(),
        }
    }
}

#[derive(Debug)]
pub enum ConfigError {
    Read {
        path: String,
        source: std::io::Error,
    },
    Parse(toml::de::Error),
    InvalidWorld(WorldConfigError),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read { path, source } => write!(formatter, "failed to read {path}: {source}"),
            Self::Parse(source) => write!(formatter, "invalid configuration: {source}"),
            Self::InvalidWorld(source) => write!(formatter, "invalid initial world: {source}"),
        }
    }
}

impl Error for ConfigError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Read { source, .. } => Some(source),
            Self::Parse(source) => Some(source),
            Self::InvalidWorld(source) => Some(source),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_complete_configuration() {
        let config: AppConfig = toml::from_str(
            "[simulation]\nseed = 42\nticks_per_second = 20\n[world]\ninitial_width = 640\ninitial_height = 480\n",
        )
        .unwrap();
        let engine = config.engine_config().unwrap();
        assert_eq!(engine.seed, 42);
        assert_eq!(engine.ticks_per_second, 20);
        assert_eq!(engine.world.initial_width(), 640);
        assert_eq!(engine.world.initial_height(), 480);
    }

    #[test]
    fn rejects_zero_sized_initial_area() {
        let config: AppConfig = toml::from_str("[world]\ninitial_width = 0\n").unwrap();
        assert!(matches!(
            config.engine_config(),
            Err(ConfigError::InvalidWorld(_))
        ));
    }
}
