//! Behavior study: viewer-like agents (random land spawns, no supplies, exploration
//! enabled) run headless and summarized as survival, roaming, and idleness metrics.
//! Unlike the canonical scenarios this is a measurement tool, not a regression fingerprint.

use std::{collections::BTreeSet, fmt};

use sim_core::{
    AgentActivity, AgentId, DeathCause, Engine, EngineCommand, EngineConfig, PhysicalGoal,
    PolicyDiagnosticKind, PolicyOptions, PopulationInit, Standability, TickOutcome, WaterSource,
    WorldConfig, WorldPosition, WorldRect,
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
    /// The spec's vertical slice: one band together beside water in a small
    /// livable valley (see `sim_core::find_valley`); only the valley is simulated.
    Valley,
}

/// The spec's first vertical slice has 16 adults.
pub const VALLEY_POPULATION: u32 = sim_core::VALLEY_BAND as u32;
pub use sim_core::VALLEY_SIDE;

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
    explored_gestures: u64,
    company_samples: u64,
    /// Decisions to head for a place someone pointed out.
    hint_decisions: u64,
    /// Highest thirst seen in any sample while alive.
    peak_thirst: u16,
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
    /// Gestures that said "I've been over there" rather than pointing at a place.
    pub explored_gestures: u64,
    /// Decisions (waypoints included) heading for a place someone pointed out.
    pub hint_decisions: u64,
    /// Percentage of sampled living time with another living agent within 8 cells.
    pub company_percent: u64,
    /// Mean acquaintances and mean trust in them at the end (social mind only).
    pub mean_acquaintances: u64,
    pub mean_trust: u64,
    /// Per trait: (metric name, mean for agents below 128, mean for agents at or above 128).
    pub trait_effects: Vec<(&'static str, &'static str, u64, u64)>,
    /// Mean remembered places per agent at the end (first-hand + hearsay).
    pub mean_known_places: u64,
    pub per_agent: Vec<StudyAgentLine>,
    /// The traced agent's last decisions, oldest first.
    pub trace: Vec<String>,
    /// Every gesture and what came of it.
    pub comms: crate::comms::CommunicationLog,
    /// Each agent's beliefs at the end of the run.
    pub minds: Vec<Option<sim_core::MentalMapView>>,
    /// Percent of agents saying each place concept's most common word, averaged
    /// over concepts: `[first sample, end]`.
    pub vocabulary_agreement: [u64; 2],
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
    pub personality: sim_core::Personality,
}

pub fn run_study(config: StudyConfig) -> Result<StudyReport, ScenarioError> {
    if config.population == 0 {
        return Err(ScenarioError("study population must be positive".into()));
    }
    let (mut engine, active_area, fresh_water, spawns) = prepare_world(config)?;
    engine
        .initialize_population(
            PopulationInit {
                active_area,
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
    let mut comms = crate::comms::CommunicationLog::default();
    let mut early_vocabulary = 0;
    for tick in 1..=config.ticks {
        match engine.tick() {
            TickOutcome::Advanced { .. } => {
                collect_tick(&engine, &mut tracks);
                comms.record_tick(&engine);
                if let Some(traced) = config.trace {
                    record_trace(&engine, AgentId::new(traced), &mut trace);
                }
            }
            TickOutcome::Paused => {}
            TickOutcome::TimeExhausted => {
                return Err(ScenarioError("simulation time exhausted".into()));
            }
        }
        if tick == STUDY_SAMPLE_TICKS {
            early_vocabulary = vocabulary_agreement(&engine, population);
        }
        if tick % STUDY_SAMPLE_TICKS == 0 {
            sample(&engine, &mut tracks, &mut tiles);
        }
    }
    let mut report = build_report(&engine, config, &spawns, &fresh_water, &tracks, &tiles);
    report.trace = trace.into_iter().collect();
    report.comms = comms;
    report.vocabulary_agreement = [early_vocabulary, vocabulary_agreement(&engine, population)];
    Ok(report)
}

/// Builds the engine and chooses where everyone starts.
fn prepare_world(
    config: StudyConfig,
) -> Result<(Engine, WorldRect, Vec<WorldPosition>, Vec<WorldPosition>), ScenarioError> {
    let side = if config.spawn == StudySpawn::Valley {
        64
    } else {
        config.world_side
    };
    let mut engine = Engine::new(EngineConfig {
        seed: config.seed,
        ticks_per_second: 60,
        world: WorldConfig::new(side, side)
            .map_err(|error| ScenarioError(format!("invalid study world: {error}")))?,
    });
    engine
        .materialize_initial_area()
        .map_err(|error| ScenarioError(format!("world materialization failed: {error}")))?;
    if config.spawn != StudySpawn::Valley {
        let fresh_water = fresh_water_cells(&engine);
        let spawns = random_land_spawns(&engine, config, &fresh_water)?;
        let area = engine.world().initial_bounds();
        return Ok((engine, area, fresh_water, spawns));
    }
    let valley = sim_core::find_valley(config.seed, VALLEY_SIDE).ok_or_else(|| {
        ScenarioError(format!(
            "no livable valley found near the origin for seed {}",
            config.seed
        ))
    })?;
    engine.command(EngineCommand::GenerateWorldArea(valley.bounds));
    let fresh_water: Vec<_> = fresh_water_cells(&engine)
        .into_iter()
        .filter(|cell| valley.bounds.contains(*cell))
        .collect();
    let spawns = sim_core::camp_sites(
        engine.world(),
        valley.bounds,
        config.population as usize,
        config.seed,
    )
    .ok_or_else(|| ScenarioError("valley has no room for the band beside water".into()))?;
    Ok((engine, valley.bounds, fresh_water, spawns))
}

fn collect_tick(engine: &Engine, tracks: &mut [AgentTrack]) {
    for signal in engine.signal_events() {
        let track = &mut tracks[signal.signal.sender.get() as usize];
        track.signals += 1;
        track.informed += u64::from(signal.informed);
        if signal.intent.topic == sim_core::GestureTopic::Explored {
            track.explored_gestures += 1;
        }
    }
    for outcome in engine.movement_outcomes() {
        if outcome.kind == sim_core::MovementOutcomeKind::Moved {
            tracks[outcome.agent.get() as usize].moves += 1;
        }
    }
    for diagnostic in engine.policy_diagnostics() {
        let track = &mut tracks[diagnostic.agent.get() as usize];
        match diagnostic.kind {
            PolicyDiagnosticKind::Selected => {
                match diagnostic.goal {
                    PhysicalGoal::Explore => track.explores += 1,
                    PhysicalGoal::Wait => track.waits += 1,
                    _ => {}
                }
                if diagnostic.reason == sim_core::PolicyReason::ToldPlace {
                    track.hint_decisions += 1;
                }
            }
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
    let living: Vec<_> = engine
        .agent_views(usize::MAX)
        .filter(|view| view.activity != AgentActivity::Dead)
        .collect();
    for view in &living {
        let in_company = living.iter().any(|other| {
            other.id != view.id
                && other.position.x.abs_diff(view.position.x) <= 8
                && other.position.y.abs_diff(view.position.y) <= 8
        });
        tracks[view.id.get() as usize].company_samples += u64::from(in_company);
    }
    for view in engine.agent_views(usize::MAX) {
        if matches!(view.activity, AgentActivity::Dead) {
            continue;
        }
        let index = view.id.get() as usize;
        if let Ok(needs) = engine.physical_needs(view.id) {
            tracks[index].peak_thirst = tracks[index].peak_thirst.max(needs.thirst.value);
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
            personality: engine
                .personality(AgentId::new(index as u32))
                .unwrap_or(sim_core::Personality::AVERAGE),
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
        explored_gestures: tracks.iter().map(|track| track.explored_gestures).sum(),
        hint_decisions: tracks.iter().map(|track| track.hint_decisions).sum(),
        company_percent: percent(
            tracks.iter().map(|track| track.company_samples).sum(),
            alive_samples,
        ),
        mean_acquaintances: social_mean(engine, tracks.len(), |map| map.acquaintances.len() as u64),
        mean_trust: {
            let maps: Vec<_> = (0..tracks.len())
                .filter_map(|index| engine.mental_map(AgentId::new(index as u32)))
                .collect();
            let trusts: Vec<u64> = maps
                .iter()
                .flat_map(|map| map.acquaintances.iter().map(|known| u64::from(known.trust)))
                .collect();
            trusts.iter().sum::<u64>() / (trusts.len() as u64).max(1)
        },
        trait_effects: trait_effects(engine, tracks, tiles),
        agents_informed: tracks.iter().map(|track| track.informed).sum(),
        mean_known_places: (0..tracks.len())
            .filter_map(|index| engine.mental_map(AgentId::new(index as u32)))
            .map(|map| map.landmarks.len() as u64)
            .sum::<u64>()
            / count,
        per_agent,
        trace: Vec::new(),
        comms: crate::comms::CommunicationLog::default(),
        vocabulary_agreement: [0, 0],
        minds: (0..tracks.len())
            .map(|index| engine.mental_map(AgentId::new(index as u32)))
            .collect(),
    }
}

/// A readable account of one agent: who it is, what it believes, who it knows,
/// and the exchanges it took part in. Pair with the decision trace for the "why".
pub fn explain(report: &StudyReport, agent: u32) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    let Some(line) = report.per_agent.get(agent as usize) else {
        return format!("no agent {agent}\n");
    };
    let p = line.personality;
    let fate = match line.death {
        Some((cause, tick)) => format!("died of {cause:?} at t={tick}"),
        None => "alive at the end".to_owned(),
    };
    let _ = writeln!(
        out,
        "explain agent {agent}: curiosity {} caution {} sociability {} diligence {}; {fate}",
        p.curiosity, p.caution, p.sociability, p.diligence
    );
    if let Some(Some(mind)) = report.minds.get(agent as usize) {
        let _ = writeln!(out, "  believes ({} explored tiles):", mind.explored_tiles);
        for place in &mind.landmarks {
            let source = match place.source {
                sim_core::LandmarkSource::Seen => "seen".to_owned(),
                sim_core::LandmarkSource::Told => {
                    format!("told, search +/-{}", place.search_radius)
                }
            };
            let _ = writeln!(
                out,
                "    {:?} at ({},{}) [{source}, confidence {}, at {}s]",
                place.kind, place.position.x, place.position.y, place.confidence, place.seen_second
            );
        }
        let _ = writeln!(out, "  knows:");
        for known in &mind.acquaintances {
            let whereabouts = known
                .last_seen_position
                .map_or("whereabouts unknown".to_owned(), |at| {
                    format!("last seen ({},{})", at.x, at.y)
                });
            let _ = writeln!(
                out,
                "    agent {} familiarity {} trust {} {whereabouts}",
                known.agent.get(),
                known.familiarity,
                known.trust
            );
        }
    }
    let involved: Vec<_> = report.comms.involving(AgentId::new(agent)).collect();
    let sent = involved
        .iter()
        .filter(|exchange| exchange.signal.signal.sender.get() == agent)
        .count();
    let _ = writeln!(
        out,
        "  exchanges: {sent} as sender, {} as receiver; latest:",
        involved.len() - sent
    );
    for exchange in involved.iter().rev().take(6).rev() {
        let _ = writeln!(out, "    {exchange}");
    }
    out
}

fn social_mean(
    engine: &Engine,
    agents: usize,
    measure: impl Fn(&sim_core::MentalMapView) -> u64,
) -> u64 {
    let values: Vec<u64> = (0..agents)
        .filter_map(|index| engine.mental_map(AgentId::new(index as u32)))
        .map(|map| measure(&map))
        .collect();
    values.iter().sum::<u64>() / (values.len() as u64).max(1)
}

/// Does each trait change the behavior it should? Splits agents at the trait
/// midpoint and averages one metric per half.
fn trait_effects(
    engine: &Engine,
    tracks: &[AgentTrack],
    tiles: &[BTreeSet<(i64, i64)>],
) -> Vec<(&'static str, &'static str, u64, u64)> {
    let personalities: Vec<_> = (0..tracks.len())
        .map(|index| {
            engine
                .personality(AgentId::new(index as u32))
                .unwrap_or(sim_core::Personality::AVERAGE)
        })
        .collect();
    let split = |name: &'static str,
                 metric: &'static str,
                 trait_of: fn(&sim_core::Personality) -> u8,
                 value: &dyn Fn(usize) -> u64| {
        let (mut low, mut high) = ((0, 0), (0, 0));
        for (index, personality) in personalities.iter().enumerate() {
            let bucket = if trait_of(personality) < 128 {
                &mut low
            } else {
                &mut high
            };
            bucket.0 += value(index);
            bucket.1 += 1_u64;
        }
        (name, metric, low.0 / low.1.max(1), high.0 / high.1.max(1))
    };
    vec![
        split("curiosity", "tiles visited", |p| p.curiosity, &|i| {
            tiles[i].len() as u64
        }),
        split("caution", "peak thirst", |p| p.caution, &|i| {
            u64::from(tracks[i].peak_thirst)
        }),
        split(
            "sociability",
            "% time in company",
            |p| p.sociability,
            &|i| percent(tracks[i].company_samples, tracks[i].alive_samples),
        ),
        split("diligence", "% time idle", |p| p.diligence, &|i| {
            percent(tracks[i].idle_samples, tracks[i].alive_samples)
        }),
    ]
}

/// For each place concept, the share of living agents whose strongest word for
/// it is the band's most common one; averaged over concepts.
fn vocabulary_agreement(engine: &Engine, population: usize) -> u64 {
    use sim_core::Concept;
    let concepts = [
        Concept::Water,
        Concept::Food,
        Concept::Wood,
        Concept::Stone,
        Concept::Home,
        Concept::Been,
    ];
    let lexicons: Vec<_> = (0..population)
        .filter_map(|index| engine.mental_map(AgentId::new(index as u32)))
        .map(|map| map.lexicon)
        .collect();
    if lexicons.is_empty() {
        return 0;
    }
    let mut total = 0;
    for concept in concepts {
        let words: Vec<_> = lexicons
            .iter()
            .filter_map(|lexicon| {
                lexicon
                    .iter()
                    .filter(|entry| entry.concept == concept)
                    .max_by_key(|entry| {
                        (
                            i32::from(entry.positive) - i32::from(entry.contradictory),
                            entry.form,
                        )
                    })
                    .map(|entry| entry.form)
            })
            .collect();
        let mut counts = std::collections::BTreeMap::new();
        for word in &words {
            *counts.entry(*word).or_insert(0_u64) += 1;
        }
        let modal = counts.values().copied().max().unwrap_or(0);
        total += percent(modal, lexicons.len() as u64);
    }
    total / concepts.len() as u64
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
            StudySpawn::AnyLand | StudySpawn::Groups | StudySpawn::Valley => true,
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
            match (config.mind.memory, config.mind.sharing, config.mind.social) {
                (false, _, _) => "legacy",
                (true, false, false) => "memory",
                (true, true, false) => "sharing",
                (true, _, true) => "full",
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
        )?;
        write!(
            formatter,
            "\n  social: company={}% acquaintances={} trust={} explored-gestures={} hint-decisions={}",
            self.company_percent,
            self.mean_acquaintances,
            self.mean_trust,
            self.explored_gestures,
            self.hint_decisions
        )?;
        write!(formatter, "\n{}", self.comms.summary())?;
        write!(
            formatter,
            "\n  vocabulary: band agreement on each place word {}% at start -> {}% at end",
            self.vocabulary_agreement[0], self.vocabulary_agreement[1]
        )?;
        for (name, metric, low, high) in &self.trait_effects {
            write!(
                formatter,
                "\n  trait {name:<11} {metric:<24} low={low:<6} high={high}"
            )?;
        }
        Ok(())
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
            "  agent {:>3} spawn=({},{}) water_dist={:<5} {:<20} moves={:<6} tiles={:<4} max_dist={:<5} idle={:>3}% drinks={} eats={} gathers={} known w/f/o={}/{}/{} needs h/t/r/e={:?} shelters={} cur/cau/soc/dil={}/{}/{}/{}",
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
            self.shelters_built,
            self.personality.curiosity,
            self.personality.caution,
            self.personality.sociability,
            self.personality.diligence
        )
    }
}
