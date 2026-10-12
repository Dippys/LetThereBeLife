//! Saving a world and opening it again exactly where it left off. Terrain is
//! regenerated from the seed; everything the simulation changed is written out.

use std::io::{Read, Write};

use serde::{Deserialize, Serialize};

use super::Engine;

/// Bumped whenever the saved layout changes; older saves are refused.
const SAVE_VERSION: u32 = 14;

/// What a save holds, borrowed from a running engine.
#[derive(Serialize)]
struct SavedRef<'a> {
    version: u32,
    config: &'a crate::EngineConfig,
    time: &'a crate::SimTime,
    paused: &'a bool,
    speed: &'a f32,
    population: &'a crate::agent::Population,
    scheduler: &'a crate::scheduler::Scheduler,
    death_records: &'a Vec<crate::DeathRecord>,
    buried: &'a std::collections::BTreeSet<crate::AgentId>,
    sickened: u64,
    policy_active: &'a bool,
    policy_options: &'a crate::PolicyOptions,
    minds: &'a crate::cognition::Minds,
    families: &'a super::births::Families,
    gave_up: &'a u64,
    nights: bool,
    plants: (u64, u64),
    stored: &'a u64,
    fetched: &'a u64,
    wildlife: &'a crate::wildlife::Wildlife,
    lives: &'a Vec<Option<crate::life::Life>>,
    recovering: &'a std::collections::VecDeque<(crate::SimTime, crate::AgentId)>,
    next_signal_id: &'a u64,
    resource_deltas: &'a crate::resources::ResourceDeltas,
    spawned_objects: &'a crate::placements::SpawnedObjects,
    structures: &'a crate::structures::StructureStore,
    runtime_counters: &'a crate::diagnostics::RuntimeCounters,
}

/// What a save holds, read back.
#[derive(Deserialize)]
struct Saved {
    version: u32,
    config: crate::EngineConfig,
    time: crate::SimTime,
    paused: bool,
    speed: f32,
    population: crate::agent::Population,
    scheduler: crate::scheduler::Scheduler,
    death_records: Vec<crate::DeathRecord>,
    buried: std::collections::BTreeSet<crate::AgentId>,
    sickened: u64,
    policy_active: bool,
    policy_options: crate::PolicyOptions,
    minds: crate::cognition::Minds,
    families: super::births::Families,
    gave_up: u64,
    nights: bool,
    plants: (u64, u64),
    stored: u64,
    fetched: u64,
    wildlife: crate::wildlife::Wildlife,
    lives: Vec<Option<crate::life::Life>>,
    recovering: std::collections::VecDeque<(crate::SimTime, crate::AgentId)>,
    next_signal_id: u64,
    resource_deltas: crate::resources::ResourceDeltas,
    spawned_objects: crate::placements::SpawnedObjects,
    structures: crate::structures::StructureStore,
    runtime_counters: crate::diagnostics::RuntimeCounters,
}

/// Why a world couldn't be saved or opened.
#[derive(Debug)]
pub enum SaveError {
    Io(std::io::Error),
    Format(postcard::Error),
    /// The save was written by a version of the simulation with another layout.
    Version {
        found: u32,
        expected: u32,
    },
    /// The terrain the save needs couldn't be generated again.
    World(String),
}

impl std::fmt::Display for SaveError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "save file error: {error}"),
            Self::Format(error) => write!(formatter, "save file is damaged: {error}"),
            Self::Version { found, expected } => write!(
                formatter,
                "save is from another version of the simulation ({found}, this one reads {expected})"
            ),
            Self::World(error) => write!(formatter, "terrain could not be regenerated: {error}"),
        }
    }
}

impl std::error::Error for SaveError {}

impl Engine {
    /// Writes everything the simulation has changed. Terrain isn't written:
    /// it comes back from the seed.
    pub fn save(&self, output: impl Write) -> Result<(), SaveError> {
        let saved = SavedRef {
            version: SAVE_VERSION,
            config: &self.config,
            time: &self.time,
            paused: &self.paused,
            speed: &self.speed,
            population: &self.population,
            scheduler: &self.scheduler,
            death_records: &self.death_records,
            buried: &self.buried,
            sickened: self.sickened,
            policy_active: &self.policy_active,
            policy_options: &self.policy_options,
            minds: &self.minds,
            families: &self.families,
            gave_up: &self.gave_up,
            nights: self.nights,
            plants: (self.plants_sprouted, self.plants_died),
            stored: &self.stored,
            fetched: &self.fetched,
            wildlife: &self.wildlife,
            lives: &self.lives,
            recovering: &self.recovering,
            next_signal_id: &self.next_signal_id,
            resource_deltas: &self.resource_deltas,
            spawned_objects: &self.spawned_objects,
            structures: &self.structures,
            runtime_counters: &self.runtime_counters,
        };
        let mut output = std::io::BufWriter::new(output);
        postcard::to_io(&saved, &mut output).map_err(SaveError::Format)?;
        output.flush().map_err(SaveError::Io)
    }

    /// Opens a saved world exactly where it left off: the same seed regenerates
    /// the terrain people live in, then everything else is restored.
    pub fn load(mut input: impl Read) -> Result<Self, SaveError> {
        let mut bytes = Vec::new();
        input.read_to_end(&mut bytes).map_err(SaveError::Io)?;
        // The version comes first, so another layout is refused before decoding.
        let (version, _) = postcard::take_from_bytes::<u32>(&bytes).map_err(SaveError::Format)?;
        if version != SAVE_VERSION {
            return Err(SaveError::Version {
                found: version,
                expected: SAVE_VERSION,
            });
        }
        let saved: Saved = postcard::from_bytes(&bytes).map_err(SaveError::Format)?;
        if saved.version != version {
            return Err(SaveError::Version {
                found: saved.version,
                expected: version,
            });
        }
        let mut engine = Engine::new(saved.config);
        engine
            .materialize_initial_area()
            .map_err(|error| SaveError::World(error.to_string()))?;
        if let Some(area) = saved.population.active_area() {
            engine.command(super::EngineCommand::GenerateWorldArea(area));
        }
        engine.time = saved.time;
        engine.paused = saved.paused;
        engine.speed = saved.speed;
        engine.population = saved.population;
        engine.scheduler = saved.scheduler;
        engine.death_records = saved.death_records;
        engine.buried = saved.buried;
        engine.sickened = saved.sickened;
        engine.policy_active = saved.policy_active;
        engine.policy_options = saved.policy_options;
        engine.minds = saved.minds;
        engine.families = saved.families;
        engine.gave_up = saved.gave_up;
        engine.nights = saved.nights;
        (engine.plants_sprouted, engine.plants_died) = saved.plants;
        engine.stored = saved.stored;
        engine.fetched = saved.fetched;
        engine.wildlife = saved.wildlife;
        engine.lives = saved.lives;
        engine.recovering = saved.recovering;
        engine.next_signal_id = saved.next_signal_id;
        engine.resource_deltas = saved.resource_deltas;
        engine.spawned_objects = saved.spawned_objects;
        engine.spawned_objects.recount_ponds();
        engine.structures = saved.structures;
        engine.runtime_counters = saved.runtime_counters;
        Ok(engine)
    }
}
