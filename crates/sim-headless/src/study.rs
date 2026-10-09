//! Behavior study: viewer-like agents (random land spawns, no supplies, exploration
//! enabled) run headless and summarized as survival, roaming, and idleness metrics.
//! Unlike the canonical scenarios this is a measurement tool, not a regression fingerprint.

use std::{collections::BTreeSet, fmt};

use sim_core::{
    AgentActivity, AgentId, DeathCause, Engine, EngineConfig, PhysicalGoal, PolicyDiagnosticKind,
    PolicyOptions, PopulationInit, Standability, TickOutcome, WaterSource, WorldConfig,
    WorldPosition,
};

use crate::scenario::ScenarioError;

/// Activity is sampled at this interval to estimate idle time and roaming.
pub const STUDY_SAMPLE_TICKS: u64 = 600;
/// Visited-area resolution: an agent "covers" a tile when sampled inside it.
pub const STUDY_TILE_SIZE: i64 = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StudyConfig {
    pub seed: u64,
    pub world_side: u32,
    pub population: u32,
    pub ticks: u64,
    pub spawn: StudySpawn,
    pub mind: PolicyOptions,
    /// Agent whose last decisions are kept in `StudyReport::trace`.
    pub trace: Option<u32>,
}

/// Where study agents start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StudySpawn {
    /// Uniformly random land, like pressing `T` anywhere in the viewer.
    AnyLand,
    /// Random land within `NEAR_WATER_DISTANCE` cells of fresh water, like a user
    /// deliberately placing agents beside a lake or river.
    NearWater,
    /// Groups of `GROUP_SIZE` agents dropped together on random land, like a
    /// user pressing `T` several times in one spot.
    Groups,
}

pub const GROUP_SIZE: usize = 5;

pub const NEAR_WATER_DISTANCE: u64 = 6;

impl StudyConfig {
    pub const fn new(seed: u64, population: u32, ticks: u64) -> Self {
        Self {
            seed,
            world_side: 2_048,
            population,
            ticks,
            spawn: StudySpawn::AnyLand,
            mind: PolicyOptions::full(),
            trace: None,
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct AgentTrack {
    spawn: Option<WorldPosition>,
    max_distance: u64,
    alive_samples: u64,
    idle_samples: u64,
    moves: u64,
    drinks: u64,
    eats: u64,
    gathers: u64,
    explores: u64,
    waits: u64,
    signals: u64,
    informed: u64,
    /// Hunger, thirst, rest, exposure at the last sample while alive.
    last_needs: [u16; 4],
    shelters_built: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StudyReport {
    pub config: StudyConfig,
    pub world: StudyWorldSummary,
    pub survivors: u32,
    pub deaths: [u32; 4],
    /// Death ticks sorted ascending; empty when nobody died.
    pub death_ticks: Vec<u64>,
    pub mean_moves: u64,
    pub mean_tiles_visited: u64,
    pub mean_max_distance: u64,
    /// Percentage of sampled living time spent idle (not moving, acting, or sleeping).
    pub idle_percent: u64,
    pub drinks: u64,
    pub eats: u64,
    pub gathers: u64,
    pub explore_decisions: u64,
    pub wait_decisions: u64,
    pub signals: u64,
    pub agents_informed: u64,
    /// Mean remembered places per agent at the end (first-hand + hearsay).
    pub mean_known_places: u64,
    pub per_agent: Vec<StudyAgentLine>,
    /// The traced agent's last decisions, oldest first.
    pub trace: Vec<String>,
}

const TRACE_LINES: usize = 60;

/// Resource availability in the study area, to separate world scarcity from behavior.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct StudyWorldSummary {
    pub land_cells: u64,
    pub fresh_water_cells: u64,
    pub food_features: u64,
    pub wood_features: u64,
    /// Percentage of land cells within 64 cells (Chebyshev, sampled every 8 cells) of fresh water.
    pub land_near_water_percent: u64,
    /// Percent of all cells per biome, in `BiomeType` declaration order (Ocean..Alpine).
    pub biome_percent: [u8; 11],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StudyAgentLine {
    pub agent: AgentId,
    pub spawn: WorldPosition,
    /// Manhattan distance from spawn to the nearest fresh-water cell, if any exists.
    pub spawn_water_distance: Option<u64>,
    pub death: Option<(DeathCause, u64)>,
    pub moves: u64,
    pub tiles: u64,
    pub max_distance: u64,
    pub idle_percent: u64,
    pub drinks: u64,
    pub eats: u64,
    pub gathers: u64,
    /// Remembered places at the end: (water, food, other).
    pub known: (usize, usize, usize),
    /// Hunger, thirst, rest, exposure at the last sample while alive.
    pub last_needs: [u16; 4],
    pub shelters_built: u64,
}

pub fn run_study(config: StudyConfig) -> Result<StudyReport, ScenarioError> {
    if config.population == 0 {
        return Err(ScenarioError("study population must be positive".into()));
    }
    let mut engine = Engine::new(EngineConfig {
        seed: config.seed,
        ticks_per_second: 60,
        world: WorldConfig::new(config.world_side, config.world_side)
            .map_err(|error| ScenarioError(format!("invalid study world: {error}")))?,
    });
    engine
        .materialize_initial_area()
        .map_err(|error| ScenarioError(format!("world materialization failed: {error}")))?;
    let fresh_water = fresh_water_cells(&engine);
    let spawns = random_land_spawns(&engine, config, &fresh_water)?;
    engine
        .initialize_population(
            PopulationInit {
                active_area: engine.world().initial_bounds(),
                population: config.population,
            },
            &spawns,
        )
        .map_err(|error| ScenarioError(format!("population initialization failed: {error}")))?;
    engine
        .activate_physical_policy_with_options(config.mind)
        .map_err(|error| ScenarioError(format!("policy activation failed: {error}")))?;

    let population = config.population as usize;
    let mut tracks = vec![AgentTrack::default(); population];
    let mut tiles: Vec<BTreeSet<(i64, i64)>> = vec![BTreeSet::new(); population];
    for (track, spawn) in tracks.iter_mut().zip(&spawns) {
        track.spawn = Some(*spawn);
    }
    let mut trace = std::collections::VecDeque::new();
    for tick in 1..=config.ticks {
        match engine.tick() {
            TickOutcome::Advanced { .. } => {
                collect_tick(&engine, &mut tracks);
                if let Some(traced) = config.trace {
                    record_trace(&engine, AgentId::new(traced), &mut trace);
                }
            }
            TickOutcome::Paused => {}
            TickOutcome::TimeExhausted => {
                return Err(ScenarioError("simulation time exhausted".into()));
            }
        }
        if tick % STUDY_SAMPLE_TICKS == 0 {
            sample(&engine, &mut tracks, &mut tiles);
        }
    }
    let mut report = build_report(&engine, config, &spawns, &fresh_water, &tracks, &tiles);
    report.trace = trace.into_iter().collect();
    Ok(report)
}

fn collect_tick(engine: &Engine, tracks: &mut [AgentTrack]) {
    for signal in engine.signal_events() {
        let track = &mut tracks[signal.sender.get() as usize];
        track.signals += 1;
        track.informed += u64::from(signal.informed);
    }
    for outcome in engine.movement_outcomes() {
        if outcome.kind == sim_core::MovementOutcomeKind::Moved {
            tracks[outcome.agent.get() as usize].moves += 1;
        }
    }
    for diagnostic in engine.policy_diagnostics() {
        let track = &mut tracks[diagnostic.agent.get() as usize];
        match diagnostic.kind {
            PolicyDiagnosticKind::Selected => match diagnostic.goal {
                PhysicalGoal::Explore => track.explores += 1,
                PhysicalGoal::Wait => track.waits += 1,
                _ => {}
            },
            PolicyDiagnosticKind::ActionCompleted if diagnostic.failure.is_none() => {
                match diagnostic.goal {
                    PhysicalGoal::Drink => track.drinks += 1,
                    PhysicalGoal::Eat => track.eats += 1,
                    PhysicalGoal::GatherMaterial => track.gathers += 1,
                    PhysicalGoal::BuildShelter => track.shelters_built += 1,
                    _ => {}
                }
            }
            _ => {}
        }
    }
}

fn record_trace(engine: &Engine, agent: AgentId, trace: &mut std::collections::VecDeque<String>) {
    let now = engine.snapshot().tick;
    let needs = engine.physical_needs(agent).ok().map(|needs| {
        [
            needs.hunger.value,
            needs.thirst.value,
            needs.rest.value,
            needs.exposure.value,
        ]
    });
    let mut lines = Vec::new();
    for diagnostic in engine.policy_diagnostics() {
        if diagnostic.agent == agent && diagnostic.kind != PolicyDiagnosticKind::RouteScheduled {
            lines.push(format!(
                "t={now} {:?} {:?} {:?} target={:?} failure={:?} needs(h/t/r/e)={needs:?}",
                diagnostic.kind,
                diagnostic.goal,
                diagnostic.reason,
                diagnostic.target.map(|target| (target.x, target.y)),
                diagnostic.failure,
            ));
        }
    }
    for diagnostic in engine.sleep_diagnostics() {
        if diagnostic.sleep.agent == agent {
            lines.push(format!(
                "t={now} sleep {:?} {:?} interruption={:?}",
                diagnostic.kind, diagnostic.sleep.quality, diagnostic.interruption
            ));
        }
    }
    for record in engine.death_records() {
        if record.agent == agent && record.at.ticks() == now {
            lines.push(format!("t={now} DIED {:?}", record.cause));
        }
    }
    for line in lines {
        if trace.len() == TRACE_LINES {
            trace.pop_front();
        }
        trace.push_back(line);
    }
}

fn sample(engine: &Engine, tracks: &mut [AgentTrack], tiles: &mut [BTreeSet<(i64, i64)>]) {
    for view in engine.agent_views(usize::MAX) {
        if matches!(view.activity, AgentActivity::Dead) {
            continue;
        }
        let index = view.id.get() as usize;
        if let Ok(needs) = engine.physical_needs(view.id) {
            tracks[index].last_needs = [
                needs.hunger.value,
                needs.thirst.value,
                needs.rest.value,
                needs.exposure.value,
            ];
        }
        let track = &mut tracks[index];
        track.alive_samples += 1;
        if view.activity == AgentActivity::Idle {
            track.idle_samples += 1;
        }
        if let Some(spawn) = track.spawn {
            let distance = spawn.x.abs_diff(view.position.x) + spawn.y.abs_diff(view.position.y);
            track.max_distance = track.max_distance.max(distance);
        }
        tiles[index].insert((
            view.position.x.div_euclid(STUDY_TILE_SIZE),
            view.position.y.div_euclid(STUDY_TILE_SIZE),
        ));
    }
}

fn build_report(
    engine: &Engine,
    config: StudyConfig,
    spawns: &[WorldPosition],
    fresh_water: &[WorldPosition],
    tracks: &[AgentTrack],
    tiles: &[BTreeSet<(i64, i64)>],
) -> StudyReport {
    let mut deaths = [0_u32; 4];
    let mut death_ticks = Vec::new();
    let mut death_by_agent = vec![None; tracks.len()];
    for record in engine.death_records() {
        deaths[record.cause as usize] += 1;
        death_ticks.push(record.at.ticks());
        death_by_agent[record.agent.get() as usize] = Some((record.cause, record.at.ticks()));
    }
    death_ticks.sort_unstable();
    let count = tracks.len() as u64;
    let alive_samples: u64 = tracks.iter().map(|track| track.alive_samples).sum();
    let idle_samples: u64 = tracks.iter().map(|track| track.idle_samples).sum();
    let per_agent = tracks
        .iter()
        .enumerate()
        .map(|(index, track)| StudyAgentLine {
            agent: AgentId::new(index as u32),
            spawn: spawns[index],
            spawn_water_distance: nearest_distance(spawns[index], fresh_water),
            death: death_by_agent[index],
            moves: track.moves,
            tiles: tiles[index].len() as u64,
            max_distance: track.max_distance,
            idle_percent: percent(track.idle_samples, track.alive_samples),
            drinks: track.drinks,
            eats: track.eats,
            gathers: track.gathers,
            last_needs: track.last_needs,
            shelters_built: track.shelters_built,
            known: engine
                .mental_map(AgentId::new(index as u32))
                .map_or((0, 0, 0), |map| {
                    let count = |kind| {
                        map.landmarks
                            .iter()
                            .filter(|place| place.kind == kind)
                            .count()
                    };
                    let water = count(sim_core::LandmarkKind::Water);
                    let food = count(sim_core::LandmarkKind::Food);
                    (water, food, map.landmarks.len() - water - food)
                }),
        })
        .collect();
    StudyReport {
        config,
        world: summarize_world(engine, fresh_water),
        survivors: config.population - deaths.iter().sum::<u32>(),
        deaths,
        death_ticks,
        mean_moves: tracks.iter().map(|track| track.moves).sum::<u64>() / count,
        mean_tiles_visited: tiles.iter().map(|set| set.len() as u64).sum::<u64>() / count,
        mean_max_distance: tracks.iter().map(|track| track.max_distance).sum::<u64>() / count,
        idle_percent: percent(idle_samples, alive_samples),
        drinks: tracks.iter().map(|track| track.drinks).sum(),
        eats: tracks.iter().map(|track| track.eats).sum(),
        gathers: tracks.iter().map(|track| track.gathers).sum(),
        explore_decisions: tracks.iter().map(|track| track.explores).sum(),
        wait_decisions: tracks.iter().map(|track| track.waits).sum(),
        signals: tracks.iter().map(|track| track.signals).sum(),
        agents_informed: tracks.iter().map(|track| track.informed).sum(),
        mean_known_places: (0..tracks.len())
            .filter_map(|index| engine.mental_map(AgentId::new(index as u32)))
            .map(|map| map.landmarks.len() as u64)
            .sum::<u64>()
            / count,
        per_agent,
        trace: Vec::new(),
    }
}

fn percent(part: u64, whole: u64) -> u64 {
    (part * 100).checked_div(whole).unwrap_or(0)
}

/// Picks `population` distinct standable cells uniformly from the bootstrap area
/// (optionally restricted to cells near fresh water).
fn random_land_spawns(
    engine: &Engine,
    config: StudyConfig,
    fresh_water: &[WorldPosition],
) -> Result<Vec<WorldPosition>, ScenarioError> {
    let bounds = engine.world().initial_bounds();
    let width = (bounds.max.x - bounds.min.x) as u64;
    let height = (bounds.max.y - bounds.min.y) as u64;
    let population = config.population;
    let mut chosen = Vec::with_capacity(population as usize);
    let mut attempt = 0_u64;
    let mut member_failures = 0_u32;
    while chosen.len() < population as usize {
        if config.spawn == StudySpawn::Groups && member_failures > 200 {
            // The anchor sits on a speck of land too small for a group: drop the group.
            chosen.truncate(chosen.len() - chosen.len() % GROUP_SIZE);
            member_failures = 0;
        }
        if attempt > u64::from(population) * 1_000_000 {
            return Err(ScenarioError(
                "could not find enough standable spawn cells (the study area may be mostly ocean for this seed)".into(),
            ));
        }
        let key = mix(config.seed ^ 0x5eed_5eed_u64.wrapping_mul(attempt.wrapping_add(1)));
        attempt += 1;
        let position = WorldPosition {
            x: bounds.min.x + (key % width) as i64,
            y: bounds.min.y + ((key >> 32) % height) as i64,
        };
        let group_anchor = (config.spawn == StudySpawn::Groups && chosen.len() % GROUP_SIZE != 0)
            .then(|| chosen[chosen.len() - chosen.len() % GROUP_SIZE]);
        let position = group_anchor.map_or(position, |anchor: WorldPosition| WorldPosition {
            x: anchor.x + (key % 13) as i64 - 6,
            y: anchor.y + ((key >> 32) % 13) as i64 - 6,
        });
        let near_enough = match config.spawn {
            StudySpawn::AnyLand | StudySpawn::Groups => true,
            StudySpawn::NearWater => nearest_distance(position, fresh_water)
                .is_some_and(|distance| distance <= NEAR_WATER_DISTANCE),
        };
        if near_enough
            && engine.world().standability_at(position) == Ok(Standability::Standable)
            && !chosen.contains(&position)
        {
            chosen.push(position);
            member_failures = 0;
        } else if group_anchor.is_some() {
            member_failures += 1;
        }
    }
    Ok(chosen)
}

fn summarize_world(engine: &Engine, fresh_water: &[WorldPosition]) -> StudyWorldSummary {
    let mut summary = StudyWorldSummary {
        fresh_water_cells: fresh_water.len() as u64,
        ..StudyWorldSummary::default()
    };
    for feature in engine.world().all_features() {
        match feature.base_resource().kind {
            sim_core::ResourceKind::Food => summary.food_features += 1,
            sim_core::ResourceKind::Wood => summary.wood_features += 1,
            sim_core::ResourceKind::Stone => {}
        }
    }
    // Coarse occupancy grid of fresh water at 64-cell blocks, then test sampled land cells.
    const BLOCK: i64 = 64;
    let water_blocks: BTreeSet<(i64, i64)> = fresh_water
        .iter()
        .map(|cell| (cell.x.div_euclid(BLOCK), cell.y.div_euclid(BLOCK)))
        .collect();
    let (mut sampled_land, mut sampled_near) = (0_u64, 0_u64);
    let mut biomes = [0_u64; 11];
    let mut total = 0_u64;
    for (position, cell) in engine.world().cells() {
        biomes[cell.biome() as usize] += 1;
        total += 1;
        if engine.world().standability_at(position) != Ok(Standability::Standable) {
            continue;
        }
        summary.land_cells += 1;
        if position.x.rem_euclid(8) != 0 || position.y.rem_euclid(8) != 0 {
            continue;
        }
        sampled_land += 1;
        let (bx, by) = (position.x.div_euclid(BLOCK), position.y.div_euclid(BLOCK));
        let near = (-1..=1).any(|dy| (-1..=1).any(|dx| water_blocks.contains(&(bx + dx, by + dy))));
        sampled_near += u64::from(near);
    }
    summary.land_near_water_percent = percent(sampled_near, sampled_land);
    for (slot, count) in summary.biome_percent.iter_mut().zip(biomes) {
        *slot = percent(count, total) as u8;
    }
    summary
}

/// All resident drinkable (lake/river) cells, row-major.
fn fresh_water_cells(engine: &Engine) -> Vec<WorldPosition> {
    engine
        .world()
        .cells()
        .map(|(position, _)| position)
        .filter(|position| {
            engine
                .world()
                .water_at(*position)
                .is_ok_and(|source| source.is_some_and(WaterSource::is_drinkable))
        })
        .collect()
}

fn nearest_distance(from: WorldPosition, cells: &[WorldPosition]) -> Option<u64> {
    cells
        .iter()
        .map(|cell| from.x.abs_diff(cell.x) + from.y.abs_diff(cell.y))
        .min()
}

const fn mix(mut key: u64) -> u64 {
    key = (key ^ (key >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    key = (key ^ (key >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    key ^ (key >> 31)
}

impl fmt::Display for StudyReport {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let config = self.config;
        let median_death = if self.death_ticks.is_empty() {
            "-".to_owned()
        } else {
            self.death_ticks[self.death_ticks.len() / 2].to_string()
        };
        writeln!(
            formatter,
            "study seed={} world={}x{} agents={} ticks={} spawn={:?} mind={}",
            config.seed,
            config.world_side,
            config.world_side,
            config.population,
            config.ticks,
            config.spawn,
            match (config.mind.memory, config.mind.sharing) {
                (false, _) => "legacy",
                (true, false) => "memory",
                (true, true) => "full",
            }
        )?;
        writeln!(
            formatter,
            "  world: land={} fresh_water={} food_features={} wood_features={} land_near_water(<=~64)={}% biomes(ocean,lake,river,beach,desert,grass,savanna,forest,wetland,tundra,alpine)={:?}",
            self.world.land_cells,
            self.world.fresh_water_cells,
            self.world.food_features,
            self.world.wood_features,
            self.world.land_near_water_percent,
            self.world.biome_percent
        )?;
        writeln!(
            formatter,
            "  survivors={}  deaths dehydration/exposure/starvation/exhaustion={}/{}/{}/{}  median_death_tick={}",
            self.survivors,
            self.deaths[DeathCause::Dehydration as usize],
            self.deaths[DeathCause::Exposure as usize],
            self.deaths[DeathCause::Starvation as usize],
            self.deaths[DeathCause::Exhaustion as usize],
            median_death
        )?;
        writeln!(
            formatter,
            "  mean moves={}  mean tiles visited={}  mean max distance from spawn={}  idle={}%",
            self.mean_moves, self.mean_tiles_visited, self.mean_max_distance, self.idle_percent
        )?;
        write!(
            formatter,
            "  drinks={}  eats={}  gathers={}  explore decisions={}  wait decisions={}  gestures={} (informed {})  mean known places={}",
            self.drinks,
            self.eats,
            self.gathers,
            self.explore_decisions,
            self.wait_decisions,
            self.signals,
            self.agents_informed,
            self.mean_known_places
        )
    }
}

impl fmt::Display for StudyAgentLine {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let death = match self.death {
            Some((cause, tick)) => format!("{cause:?}@{tick}"),
            None => "alive".to_owned(),
        };
        write!(
            formatter,
            "  agent {:>3} spawn=({},{}) water_dist={:<5} {:<20} moves={:<6} tiles={:<4} max_dist={:<5} idle={:>3}% drinks={} eats={} gathers={} known w/f/o={}/{}/{} needs h/t/r/e={:?} shelters={}",
            self.agent.get(),
            self.spawn.x,
            self.spawn.y,
            self.spawn_water_distance
                .map_or_else(|| "none".to_owned(), |distance| distance.to_string()),
            death,
            self.moves,
            self.tiles,
            self.max_distance,
            self.idle_percent,
            self.drinks,
            self.eats,
            self.gathers,
            self.known.0,
            self.known.1,
            self.known.2,
            self.last_needs,
            self.shelters_built
        )
    }
}
