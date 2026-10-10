//! Behavior study: viewer-like agents (random land spawns, no supplies, exploration
//! enabled) run headless and summarized as survival, roaming, and idleness metrics.
//! Unlike the canonical scenarios this is a measurement tool, not a regression fingerprint.

use std::{collections::BTreeSet, fmt};

use sim_core::{
    AgentActivity, AgentId, AgentView, DeathCause, Engine, EngineCommand, EngineConfig,
    PhysicalGoal, PolicyDiagnosticKind, PolicyOptions, PopulationInit, Standability, TickOutcome,
    WaterSource, WorldConfig, WorldPosition, WorldRect,
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
    /// Percent of the generated food left at the start (100 = all; lower is scarcer).
    pub food_percent: u8,
    /// Release deer and wolves into the valley.
    pub wildlife: bool,
    /// Picked bushes and trees grow back.
    pub regrowth: bool,
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

/// The spec's first vertical slice: 16 adults and 4 children.
pub const VALLEY_POPULATION: u32 = (sim_core::VALLEY_BAND
    + sim_core::VALLEY_FAMILIES * sim_core::VALLEY_CHILDREN_PER_FAMILY)
    as u32;

/// Who starts where, and (in the valley) which agents are children of whom.
struct Start {
    engine: Engine,
    area: WorldRect,
    fresh_water: Vec<WorldPosition>,
    spawns: Vec<WorldPosition>,
    /// Agents with ids at or above this are children; `None` means all founders.
    founders: Option<u32>,
    parents: Vec<(usize, usize)>,
}
pub use sim_core::VALLEY_SIDE;

pub const GROUP_SIZE: usize = 5;
pub use sim_core::{VALLEY_DEER, VALLEY_WOLVES};

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
            food_percent: 100,
            wildlife: true,
            regrowth: true,
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
    /// Samples with someone of another family within 8 cells.
    mixed_samples: u64,
    family: u8,
    /// 0 for people present at the start (and newcomers), their children 1, and so on.
    generation: u8,
    /// Decisions to head for a place someone pointed out.
    hint_decisions: u64,
    /// Decisions to head for a place the agent saw itself.
    memory_decisions: u64,
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
    /// Survivors lying collapsed at the end (alive, but unable to act).
    pub collapsed: u32,
    pub deaths: [u32; DeathCause::COUNT],
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
    /// Decisions (waypoints included) heading for a place the agent saw itself.
    pub memory_decisions: u64,
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
    /// Children and the percent of place words where they say what most founders say.
    pub children_vocabulary: Option<(u64, u64)>,
    /// Eating and what agents came to believe about food.
    pub food: FoodStats,
    /// Hunting, bites, and the animals left.
    pub wildlife: WildlifeStats,
    pub families: FamilyStats,
}

/// How much the founding families mix (valley runs).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct FamilyStats {
    /// Manhattan distance between the first two families' camps.
    pub camp_distance: u64,
    /// Percent of sampled living time with someone of another family within 8 cells.
    pub mixed_percent: u64,
    /// Receptions where speaker and listener belong to different families, and
    /// how many of those were misread.
    pub cross_receptions: u64,
    pub cross_misread: u64,
    /// Couples formed within a family and across families.
    pub couples: [u64; 2],
    /// Pregnancies, babies born, children who started walking, and losses.
    pub births: [u64; 4],
    /// At the end, over living people's acquaintances: names known, of how
    /// many, and how many of those are wrong.
    pub names: [u64; 3],
    /// Names heard called, and how many were pinned on the wrong person.
    pub calls: [u64; 2],
    /// Per generation (from 1): people, and the percent of their place words
    /// that are the founders' most common word.
    pub generations: Vec<(u8, u64, u64)>,
    /// Words coined, coined words said by two or more living people at the end,
    /// and sound shifts.
    pub new_words: [u64; 3],
    /// Percent of place words where the two founding families' most common
    /// word is the same, at the end.
    pub shared_words: u64,
}

/// What happened between people and animals.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct WildlifeStats {
    /// Decisions to go after an animal, and to run from one.
    pub hunt_decisions: u64,
    pub flee_decisions: u64,
    /// Decisions to warn others of an animal or call them to hunt one, and how
    /// many of those gestures failed to happen.
    pub call_decisions: u64,
    pub failed_calls: u64,
    pub strikes: u64,
    /// Animals people brought down, and how many of those kills had helpers.
    pub kills: u64,
    pub group_kills: u64,
    /// Deer brought down by wolves.
    pub predator_kills: u64,
    pub bites: u64,
    pub births: u64,
    /// Living deer and wolves at the end.
    pub deer: u64,
    pub wolves: u64,
    /// At the end, for founders then children: how many fear wolves.
    pub fear_wolves: [u64; 2],
    /// Hearths finished, warm-ups at them, and (founders, children) who know
    /// hearths warm at the end.
    pub hearths: u64,
    pub warm_ups: u64,
    pub know_fire: [u64; 2],
    /// Founders and children who know how to knap a blade, at the end.
    pub know_knapping: [u64; 2],
    /// Blades knapped, and how many people watched it done.
    pub blades: u64,
    pub watched_crafts: u64,
    /// Fuel put on fires, and how often that relit a dead one.
    pub tends: u64,
    pub relit: u64,
}

/// What the band ate and believes about food.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct FoodStats {
    /// Meals by material (indexed by `Material as usize`).
    pub meals: [u64; sim_core::Material::COUNT],
    /// Meals that made the eater sick.
    pub sick: u64,
    /// First tastes of something the eater had no belief about.
    pub first_tastes: u64,
    /// Times someone watched another agent eat.
    pub watched: u64,
    /// At the end, for founders then children: how many think bitter berries are
    /// food, how many think they make you sick, and how many have no idea.
    pub bitter_beliefs: [[u64; 3]; 2],
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
    let Start {
        mut engine,
        area: active_area,
        fresh_water,
        spawns,
        founders,
        parents,
    } = prepare_world(config)?;
    engine
        .initialize_population(
            PopulationInit {
                active_area,
                population: config.population,
            },
            &spawns,
        )
        .map_err(|error| ScenarioError(format!("population initialization failed: {error}")))?;
    if config.food_percent < 100 {
        engine
            .strip_food(active_area, config.food_percent)
            .map_err(|_| ScenarioError("food can only be stripped before the first tick".into()))?;
    }
    if !config.regrowth {
        engine.disable_regrowth();
    }
    if config.wildlife && config.spawn == StudySpawn::Valley {
        engine.release_wildlife(active_area, VALLEY_DEER, VALLEY_WOLVES);
    }
    if let Some(founders) = founders {
        engine.set_founders(founders);
    }
    for &(child, parent) in &parents {
        engine.bond(AgentId::new(child as u32), AgentId::new(parent as u32));
    }
    engine
        .activate_physical_policy_with_options(config.mind)
        .map_err(|error| ScenarioError(format!("policy activation failed: {error}")))?;

    let population = config.population as usize;
    let mut tracks = vec![AgentTrack::default(); population];
    let mut tiles: Vec<BTreeSet<(i64, i64)>> = vec![BTreeSet::new(); population];
    for (track, spawn) in tracks.iter_mut().zip(&spawns) {
        track.spawn = Some(*spawn);
    }
    // Founders come in family order; children belong to their parent's family.
    let founder_count = founders.map_or(population, |count| count as usize);
    let family_size = founder_count.div_ceil(sim_core::VALLEY_FAMILIES).max(1);
    for (index, track) in tracks.iter_mut().enumerate().take(founder_count) {
        track.family = (index / family_size) as u8;
    }
    for &(child, parent) in &parents {
        tracks[child].family = tracks[parent].family;
        tracks[child].generation = 1;
    }
    let camp_distance = spawns.get(family_size).map_or(0, |other| {
        spawns[0].x.abs_diff(other.x) + spawns[0].y.abs_diff(other.y)
    });
    let mut trace = std::collections::VecDeque::new();
    let mut comms = crate::comms::CommunicationLog::default();
    let mut couples: Vec<(AgentId, AgentId)> = Vec::new();
    // Pregnancies, births, children walking, and losses.
    let mut families_born = [0_u64; 4];
    let mut arrivals = 0_u8;
    let mut name_calls = [0_u64; 2];
    let mut coined: Vec<(sim_core::VocalForm, sim_core::Concept)> = Vec::new();
    let mut shifts = 0_u64;
    let mut early_vocabulary = 0;
    let mut food = FoodStats::default();
    let mut wildlife = WildlifeStats::default();
    for tick in 1..=config.ticks {
        match engine.tick() {
            TickOutcome::Advanced { .. } => {
                // Children who start walking join the study.
                for event in engine.family_events() {
                    match *event {
                        sim_core::FamilyEvent::Walking { mother, child } => {
                            let index = child.get() as usize;
                            if tracks.len() <= index {
                                tracks.resize(index + 1, AgentTrack::default());
                                tiles.resize(index + 1, BTreeSet::new());
                            }
                            tracks[index].family = tracks[mother.get() as usize].family;
                            tracks[index].generation =
                                tracks[mother.get() as usize].generation.saturating_add(1);
                            tracks[index].spawn = engine
                                .agent_views(usize::MAX)
                                .nth(index)
                                .map(|view| view.position);
                            families_born[2] += 1;
                        }
                        sim_core::FamilyEvent::Born { .. } => families_born[1] += 1,
                        sim_core::FamilyEvent::Conceived { .. } => families_born[0] += 1,
                        sim_core::FamilyEvent::Lost { .. } => families_born[3] += 1,
                        sim_core::FamilyEvent::Arrived { woman, man } => {
                            // Newcomers are a family of their own.
                            let family = 2 + arrivals;
                            arrivals += 1;
                            for agent in [woman, man] {
                                let index = agent.get() as usize;
                                if tracks.len() <= index {
                                    tracks.resize(index + 1, AgentTrack::default());
                                    tiles.resize(index + 1, BTreeSet::new());
                                }
                                tracks[index].family = family;
                                tracks[index].spawn = engine
                                    .agent_views(usize::MAX)
                                    .nth(index)
                                    .map(|view| view.position);
                            }
                        }
                    }
                }
                collect_tick(&engine, &mut tracks);
                for decision in engine.policy_diagnostics() {
                    if decision.kind == PolicyDiagnosticKind::Selected {
                        wildlife.hunt_decisions +=
                            u64::from(decision.reason == sim_core::PolicyReason::Hunting);
                        wildlife.flee_decisions +=
                            u64::from(decision.reason == sim_core::PolicyReason::Fleeing);
                    }
                    let call = matches!(
                        decision.reason,
                        sim_core::PolicyReason::Warning | sim_core::PolicyReason::Recruiting
                    );
                    if call && decision.kind == PolicyDiagnosticKind::Selected {
                        wildlife.call_decisions += 1;
                    }
                    if call
                        && decision.kind == PolicyDiagnosticKind::ActionCompleted
                        && decision.failure.is_some()
                    {
                        wildlife.failed_calls += 1;
                    }
                }
                for built in engine.structure_diagnostics() {
                    wildlife.hearths += u64::from(
                        built.kind == sim_core::StructureDiagnosticKind::Completed
                            && built.structure.kind == sim_core::StructureKind::Hearth,
                    );
                }
                for decision in engine.policy_diagnostics() {
                    wildlife.warm_ups += u64::from(
                        decision.goal == PhysicalGoal::WarmUp
                            && decision.kind == PolicyDiagnosticKind::ActionCompleted
                            && decision.failure.is_none(),
                    );
                }
                for event in engine.wildlife_events() {
                    match *event {
                        sim_core::WildlifeEvent::Struck {
                            killed, helpers, ..
                        } => {
                            wildlife.strikes += 1;
                            wildlife.kills += u64::from(killed);
                            wildlife.group_kills += u64::from(killed && helpers > 0);
                        }
                        sim_core::WildlifeEvent::Killed { .. } => wildlife.predator_kills += 1,
                        sim_core::WildlifeEvent::Bite { .. } => wildlife.bites += 1,
                        sim_core::WildlifeEvent::Born { .. } => wildlife.births += 1,
                    }
                }
                for meal in engine.meal_events() {
                    food.meals[meal.material as usize] += 1;
                    food.sick += u64::from(meal.retched);
                    food.first_tastes += u64::from(meal.first_taste);
                    food.watched += u64::from(meal.watchers);
                }
                for craft in engine.craft_events() {
                    wildlife.blades += 1;
                    wildlife.watched_crafts += u64::from(craft.watchers);
                }
                for fire in engine.fire_events() {
                    wildlife.tends += 1;
                    wildlife.relit += u64::from(fire.relit);
                }
                for event in engine.word_events() {
                    match *event {
                        sim_core::WordEvent::Coined { form, concept, .. } => {
                            coined.push((form, concept));
                        }
                        sim_core::WordEvent::Shifted { .. } => shifts += 1,
                    }
                }
                for call in engine.name_events() {
                    name_calls[0] += 1;
                    name_calls[1] += u64::from(call.heard_as != call.called);
                }
                for couple in engine.couple_events() {
                    couples.push((couple.first, couple.second));
                }
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
    let family_of = |agent: AgentId| tracks.get(agent.get() as usize).map(|track| track.family);
    let mut families = FamilyStats {
        camp_distance,
        mixed_percent: percent(
            tracks.iter().map(|track| track.mixed_samples).sum(),
            tracks.iter().map(|track| track.alive_samples).sum(),
        ),
        ..FamilyStats::default()
    };
    for &(first, second) in &couples {
        families.couples[usize::from(family_of(first) != family_of(second))] += 1;
    }
    for exchange in comms.exchanges() {
        let speaker = family_of(exchange.signal.signal.sender);
        for reception in &exchange.receptions {
            if family_of(reception.interpretation.receiver) != speaker {
                families.cross_receptions += 1;
                families.cross_misread +=
                    u64::from(reception.interpretation.understood != exchange.signal.intent.topic);
            }
        }
    }
    families.births = families_born;
    families.generations = generation_words(&engine, &tracks);
    let living: Vec<(u8, sim_core::MentalMapView)> = engine
        .agent_views(usize::MAX)
        .filter(|view| view.activity != AgentActivity::Dead)
        .filter_map(|view| {
            let family = tracks.get(view.id.get() as usize)?.family;
            Some((family, engine.mental_map(view.id)?))
        })
        .collect();
    coined.sort_unstable();
    coined.dedup();
    let caught_on = coined
        .iter()
        .filter(|&&(form, concept)| {
            living
                .iter()
                .filter(|(_, mind)| top_word(&mind.lexicon, concept) == Some(form))
                .count()
                >= 2
        })
        .count() as u64;
    families.new_words = [coined.len() as u64, caught_on, shifts];
    let family_minds = |family: u8| -> Vec<&sim_core::MentalMapView> {
        living
            .iter()
            .filter(|(of, _)| *of == family)
            .map(|(_, mind)| mind)
            .collect()
    };
    let (first, second) = (modal_words(&family_minds(0)), modal_words(&family_minds(1)));
    let compared: Vec<bool> = first
        .iter()
        .zip(second)
        .filter_map(|(a, b)| Some(*a.as_ref()? == b?))
        .collect();
    families.shared_words = percent(
        compared.iter().filter(|same| **same).count() as u64,
        compared.len() as u64,
    );
    families.calls = name_calls;
    for view in engine.agent_views(usize::MAX) {
        if view.activity == AgentActivity::Dead {
            continue;
        }
        let Some(map) = engine.mental_map(view.id) else {
            continue;
        };
        for known in &map.acquaintances {
            families.names[1] += 1;
            if let Some(name) = known.name {
                families.names[0] += 1;
                families.names[2] += u64::from(
                    engine
                        .life(known.agent)
                        .is_some_and(|life| life.name != name),
                );
            }
        }
    }
    report.families = families;
    report.comms = comms;
    let everyone = engine.snapshot().agent_count as usize;
    report.vocabulary_agreement = [early_vocabulary, vocabulary_agreement(&engine, population)];
    report.children_vocabulary = children_vocabulary(&engine, everyone);
    for index in 0..everyone {
        let Some(mind) = engine.mental_map(AgentId::new(index as u32)) else {
            continue;
        };
        let bitter = mind
            .affordances
            .iter()
            .find(|belief| belief.material == sim_core::Material::Bitterberries);
        let slot = match bitter {
            Some(belief) if belief.feeds > 2 * belief.sickens => 0,
            Some(_) => 1,
            None => 2,
        };
        food.bitter_beliefs[usize::from(mind.child)][slot] += 1;
        let fears = mind
            .fauna
            .iter()
            .any(|belief| belief.species == sim_core::Species::Wolf && belief.danger > 64);
        wildlife.fear_wolves[usize::from(mind.child)] += u64::from(fears);
        wildlife.know_fire[usize::from(mind.child)] += u64::from(mind.knows_hearths);
        wildlife.know_knapping[usize::from(mind.child)] += u64::from(mind.knows_knapping);
    }
    report.food = food;
    wildlife.deer = engine.animal_count(sim_core::Species::Deer) as u64;
    wildlife.wolves = engine.animal_count(sim_core::Species::Wolf) as u64;
    report.wildlife = wildlife;
    Ok(report)
}

/// Builds the engine and chooses where everyone starts.
fn prepare_world(config: StudyConfig) -> Result<Start, ScenarioError> {
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
        return Ok(Start {
            engine,
            area,
            fresh_water,
            spawns,
            founders: None,
            parents: Vec::new(),
        });
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
    let families = sim_core::VALLEY_FAMILIES;
    let no_room = || ScenarioError("valley has no room for the band beside water".into());
    // The standard band has children; other sizes are founders only.
    if config.population == VALLEY_POPULATION {
        let layout = sim_core::band_layout(
            engine.world(),
            valley.bounds,
            families,
            sim_core::VALLEY_BAND / families,
            sim_core::VALLEY_CHILDREN_PER_FAMILY,
            config.seed,
        )
        .ok_or_else(no_room)?;
        return Ok(Start {
            engine,
            area: valley.bounds,
            fresh_water,
            spawns: layout.sites,
            founders: Some(layout.founders as u32),
            parents: layout.parents,
        });
    }
    let spawns = sim_core::family_camps(
        engine.world(),
        valley.bounds,
        families,
        (config.population as usize).div_ceil(families),
        config.seed,
    )
    .map(|mut sites| {
        sites.truncate(config.population as usize);
        sites
    })
    .ok_or_else(no_room)?;
    Ok(Start {
        engine,
        area: valley.bounds,
        fresh_water,
        spawns,
        founders: None,
        parents: Vec::new(),
    })
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
                match diagnostic.reason {
                    sim_core::PolicyReason::ToldPlace => track.hint_decisions += 1,
                    sim_core::PolicyReason::RememberedPlace => track.memory_decisions += 1,
                    _ => {}
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
    let at = engine
        .agent_views(usize::MAX)
        .find(|view| view.id == agent)
        .map(|view| (view.position.x, view.position.y));
    let mut lines = Vec::new();
    for diagnostic in engine.policy_diagnostics() {
        if diagnostic.agent == agent && diagnostic.kind != PolicyDiagnosticKind::RouteScheduled {
            lines.push(format!(
                "t={now} at={at:?} {:?} {:?} {:?} target={:?} failure={:?} needs(h/t/r/e)={needs:?}",
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
    for event in engine.wildlife_events() {
        if let sim_core::WildlifeEvent::Bite {
            agent: bitten,
            species,
            damage,
            position,
            ..
        } = *event
            && bitten == agent
        {
            lines.push(format!(
                "t={now} BITTEN by {species:?} at ({}, {}) damage={damage}",
                position.x, position.y
            ));
        }
    }
    for diagnostic in engine.health_diagnostics() {
        if diagnostic.agent == agent
            && diagnostic.kind == sim_core::HealthDiagnosticKind::Incapacitated
        {
            lines.push(format!("t={now} INCAPACITATED"));
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
        let family = tracks[view.id.get() as usize].family;
        let near = |other: &&AgentView| {
            other.id != view.id
                && other.position.x.abs_diff(view.position.x) <= 8
                && other.position.y.abs_diff(view.position.y) <= 8
        };
        let in_company = living.iter().any(|other| near(&other));
        let mixed = living
            .iter()
            .filter(near)
            .any(|other| tracks[other.id.get() as usize].family != family);
        let track = &mut tracks[view.id.get() as usize];
        track.company_samples += u64::from(in_company);
        track.mixed_samples += u64::from(mixed);
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
    let mut deaths = [0_u32; DeathCause::COUNT];
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
        .map(|(index, track)| {
            let spawn = track.spawn.unwrap_or(spawns[index.min(spawns.len() - 1)]);
            (index, track, spawn)
        })
        .map(|(index, track, spawn)| StudyAgentLine {
            agent: AgentId::new(index as u32),
            spawn,
            spawn_water_distance: nearest_distance(spawn, fresh_water),
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
                    let food = count(sim_core::LandmarkKind::BERRIES);
                    (water, food, map.landmarks.len() - water - food)
                }),
        })
        .collect();
    StudyReport {
        config,
        world: summarize_world(engine, fresh_water),
        survivors: engine.snapshot().living_agent_count,
        collapsed: engine
            .agent_views(usize::MAX)
            .filter(|view| view.activity == AgentActivity::Incapacitated)
            .count() as u32,
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
        memory_decisions: tracks.iter().map(|track| track.memory_decisions).sum(),
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
        children_vocabulary: None,
        food: FoodStats::default(),
        wildlife: WildlifeStats::default(),
        families: FamilyStats::default(),
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
const PLACE_CONCEPTS: [sim_core::Concept; 6] = [
    sim_core::Concept::Water,
    sim_core::Concept::BERRIES,
    sim_core::Concept::WOOD,
    sim_core::Concept::STONE,
    sim_core::Concept::HOME,
    sim_core::Concept::Been,
];

/// The word an agent would say for `concept` (its best-supported form).
fn top_word(
    lexicon: &[sim_core::LexiconEntryView],
    concept: sim_core::Concept,
) -> Option<sim_core::VocalForm> {
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
}

/// How many of `minds` say each word for `concept`.
fn word_counts(
    minds: &[&sim_core::MentalMapView],
    concept: sim_core::Concept,
) -> std::collections::BTreeMap<sim_core::VocalForm, u64> {
    let mut counts = std::collections::BTreeMap::new();
    for mind in minds {
        if let Some(word) = top_word(&mind.lexicon, concept) {
            *counts.entry(word).or_insert(0_u64) += 1;
        }
    }
    counts
}

/// The most common word for each place concept among `minds`.
fn modal_words(minds: &[&sim_core::MentalMapView]) -> [Option<sim_core::VocalForm>; 6] {
    PLACE_CONCEPTS.map(|concept| {
        word_counts(minds, concept)
            .into_iter()
            .max_by_key(|&(word, count)| (count, std::cmp::Reverse(word)))
            .map(|(word, _)| word)
    })
}

/// For each generation after the founders: how many people, and what percent
/// of their place words match the founders' most common word for each place.
fn generation_words(engine: &Engine, tracks: &[AgentTrack]) -> Vec<(u8, u64, u64)> {
    let views: Vec<(u8, sim_core::MentalMapView)> = tracks
        .iter()
        .enumerate()
        .filter_map(|(index, track)| {
            engine
                .mental_map(AgentId::new(index as u32))
                .map(|view| (track.generation, view))
        })
        .collect();
    let founders: Vec<&sim_core::MentalMapView> = views
        .iter()
        .filter(|(generation, _)| *generation == 0)
        .map(|(_, view)| view)
        .collect();
    let modal = modal_words(&founders);
    let last = views
        .iter()
        .map(|(generation, _)| *generation)
        .max()
        .unwrap_or(0);
    (1..=last)
        .filter_map(|generation| {
            let members: Vec<&sim_core::MentalMapView> = views
                .iter()
                .filter(|(of, _)| *of == generation)
                .map(|(_, view)| view)
                .collect();
            if members.is_empty() {
                return None;
            }
            let (mut matching, mut total) = (0_u64, 0_u64);
            for member in &members {
                for (concept, founders_word) in PLACE_CONCEPTS.into_iter().zip(modal) {
                    if let (Some(word), Some(founders_word)) =
                        (top_word(&member.lexicon, concept), founders_word)
                    {
                        total += 1;
                        matching += u64::from(word == founders_word);
                    }
                }
            }
            Some((generation, members.len() as u64, percent(matching, total)))
        })
        .collect()
}

fn vocabulary_agreement(engine: &Engine, population: usize) -> u64 {
    let views: Vec<_> = (0..population)
        .filter_map(|index| engine.mental_map(AgentId::new(index as u32)))
        .collect();
    let minds: Vec<_> = views.iter().collect();
    if minds.is_empty() {
        return 0;
    }
    let total: u64 = PLACE_CONCEPTS
        .into_iter()
        .map(|concept| {
            let modal = word_counts(&minds, concept)
                .into_values()
                .max()
                .unwrap_or(0);
            percent(modal, minds.len() as u64)
        })
        .sum();
    total / PLACE_CONCEPTS.len() as u64
}

/// For children: how many there are, and the percent of place words where a
/// child says what most founders say.
fn children_vocabulary(engine: &Engine, population: usize) -> Option<(u64, u64)> {
    let minds: Vec<_> = (0..population)
        .filter_map(|index| engine.mental_map(AgentId::new(index as u32)))
        .collect();
    let founders: Vec<_> = minds.iter().filter(|mind| !mind.child).collect();
    let children: Vec<_> = minds.iter().filter(|mind| mind.child).collect();
    if children.is_empty() {
        return None;
    }
    let band = modal_words(&founders);
    let matching = children
        .iter()
        .flat_map(|child| {
            PLACE_CONCEPTS.iter().zip(band).filter(|&(&concept, word)| {
                word.is_some() && top_word(&child.lexicon, concept) == word
            })
        })
        .count() as u64;
    Some((
        children.len() as u64,
        percent(matching, (children.len() * PLACE_CONCEPTS.len()) as u64),
    ))
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
            sim_core::Material::Berries => summary.food_features += 1,
            sim_core::Material::Wood => summary.wood_features += 1,
            _ => {}
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
            "  survivors={} (collapsed {})  deaths dehydration/exposure/starvation/exhaustion/injury/old age={}/{}/{}/{}/{}/{}  median_death_tick={}",
            self.survivors,
            self.collapsed,
            self.deaths[DeathCause::Dehydration as usize],
            self.deaths[DeathCause::Exposure as usize],
            self.deaths[DeathCause::Starvation as usize],
            self.deaths[DeathCause::Exhaustion as usize],
            self.deaths[DeathCause::Injury as usize],
            self.deaths[DeathCause::OldAge as usize],
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
            "\n  social: company={}% acquaintances={} trust={} explored-gestures={} memory-decisions={} hint-decisions={}",
            self.company_percent,
            self.mean_acquaintances,
            self.mean_trust,
            self.explored_gestures,
            self.memory_decisions,
            self.hint_decisions
        )?;
        write!(formatter, "\n{}", self.comms.summary())?;
        write!(
            formatter,
            "\n  vocabulary: band agreement on each place word {}% at start -> {}% at end",
            self.vocabulary_agreement[0], self.vocabulary_agreement[1]
        )?;
        let food = &self.food;
        let [
            [founders_eat, founders_avoid, founders_unsure],
            [children_eat, children_avoid, children_unsure],
        ] = food.bitter_beliefs;
        write!(
            formatter,
            "\n  food: meals berries {} bitterberries {} wood {} stone {} meat {}; sick {}, first tastes {}, watched {}; bitterberries thought food/sickening/unknown: founders {founders_eat}/{founders_avoid}/{founders_unsure}, children {children_eat}/{children_avoid}/{children_unsure}",
            food.meals[sim_core::Material::Berries as usize],
            food.meals[sim_core::Material::Bitterberries as usize],
            food.meals[sim_core::Material::Wood as usize],
            food.meals[sim_core::Material::Stone as usize],
            food.meals[sim_core::Material::Meat as usize],
            food.sick,
            food.first_tastes,
            food.watched,
        )?;
        let wild = &self.wildlife;
        if self.config.wildlife && self.config.spawn == StudySpawn::Valley {
            let [founders_fear, children_fear] = wild.fear_wolves;
            write!(
                formatter,
                "\n  wildlife: hunt decisions {}, flee decisions {}, calls {} ({} failed), strikes {}, kills by people {} ({} together), deer killed by wolves {}, bites {}, births {}; left: deer {} wolves {}; fear wolves: founders {founders_fear}, children {children_fear}",
                wild.hunt_decisions,
                wild.flee_decisions,
                wild.call_decisions,
                wild.failed_calls,
                wild.strikes,
                wild.kills,
                wild.group_kills,
                wild.predator_kills,
                wild.bites,
                wild.births,
                wild.deer,
                wild.wolves,
            )?;
        }
        if self.config.spawn == StudySpawn::Valley {
            let families = &self.families;
            write!(
                formatter,
                "\n  families: camps {} cells apart; near the other family {}% of the time; receptions across families {} ({} misread); couples within families {}, across {}",
                families.camp_distance,
                families.mixed_percent,
                families.cross_receptions,
                families.cross_misread,
                families.couples[0],
                families.couples[1]
            )?;
            let [conceived, born, walking, lost] = families.births;
            write!(
                formatter,
                "\n  births: pregnancies {conceived}, babies born {born}, children walking {walking}, lost with their mother {lost}"
            )?;
            if !families.generations.is_empty() {
                let parts: Vec<String> = families
                    .generations
                    .iter()
                    .map(|(generation, people, shared)| {
                        format!("generation {generation} ({people} people) {shared}%")
                    })
                    .collect();
                write!(
                    formatter,
                    "\n  words passed down (share of place words that are the founders' most common): {}",
                    parts.join(", ")
                )?;
            }
            let [coined, caught_on, shifts] = families.new_words;
            write!(
                formatter,
                "\n  new words: coined {coined} ({caught_on} said by 2+ people at the end), sound shifts {shifts}; the two families share {}% of place words",
                families.shared_words
            )?;
            let [known, acquaintances, wrong] = families.names;
            let [heard, misheard] = families.calls;
            write!(
                formatter,
                "\n  names: known for {known} of {acquaintances} acquaintances ({wrong} wrong); names heard called {heard} ({misheard} pinned on the wrong person)"
            )?;
        }
        let [founders_fire, children_fire] = self.wildlife.know_fire;
        write!(
            formatter,
            "\n  fire: hearths built {}, warm-ups {}, fuel added {} ({} relit); know hearths warm: founders {founders_fire}, children {children_fire}",
            self.wildlife.hearths, self.wildlife.warm_ups, self.wildlife.tends, self.wildlife.relit
        )?;
        let [founders_knap, children_knap] = self.wildlife.know_knapping;
        write!(
            formatter,
            "\n  tools: blades made {} (watched {} times); know how to knap: founders {founders_knap}, children {children_knap}",
            self.wildlife.blades, self.wildlife.watched_crafts
        )?;
        if let Some((children, matching)) = self.children_vocabulary {
            write!(
                formatter,
                "\n  children: {children} born with no words; they say the founders' word for {matching}% of place words"
            )?;
        }
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
