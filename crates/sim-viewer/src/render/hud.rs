//! HUD and hovered-agent text formatting, including every enum label shown on screen.

use std::{cmp::Reverse, fmt::Write};

use sim_core::{
    ACQUAINTANCE_SLOTS, AcquaintanceView, AgentActivity, BiomeType, ChunkPresence, Concept,
    DeathCause, ExplorationHeading, FRIEND_FAMILIARITY, FeatureKind, GenerateAreaError,
    HealthStatus, LandmarkKind, LandmarkSource, Material, Mime, NeedKind, Personality,
    PhysicalGoal, PhysicalPolicyView, PolicyReason, PrevailingWind, SleepQuality, SurfaceType,
    VocalForm, World,
};

use super::{
    AGENT_TEXT_CAPACITY, AgentInspection, GenerationStatus, MemoryInspection, PopulationStatus,
    RenderState, overlay::spawn_kind_label,
};

pub(super) fn write_hud_text(output: &mut String, world: &World, state: &RenderState) {
    output.clear();
    let activity = if state.snapshot.paused {
        "PAUSED"
    } else {
        "RUNNING"
    };
    let speed = state.snapshot.speed;
    let total_tenths = (state.snapshot.simulated_seconds.max(0.0) * 10.0) as u64;
    let hours = total_tenths / 36_000;
    let minutes = total_tenths / 600 % 60;
    let seconds = total_tenths / 10 % 60;
    let tenths = total_tenths % 10;

    writeln!(output, "LET THERE BE LIFE").expect("writing to String cannot fail");
    if speed.fract() == 0.0 {
        writeln!(output, "{activity}  SPEED {speed:.0}X").expect("writing to String cannot fail");
    } else {
        writeln!(output, "{activity}  SPEED {speed:.1}X").expect("writing to String cannot fail");
    }
    writeln!(
        output,
        "SIM {hours:04}:{minutes:02}:{seconds:02}.{tenths}  TICK {}",
        state.snapshot.tick
    )
    .expect("writing to String cannot fail");
    if let Some(message) = &state.spawn_message {
        writeln!(output, "{message}").expect("writing to String cannot fail");
    }
    writeln!(
        output,
        "SEED {}  LOADED {}  REV {}",
        state.snapshot.seed,
        world.loaded_chunk_count(),
        world.revision()
    )
    .expect("writing to String cannot fail");
    writeln!(output, "GEN {}", generation_label(state.generation_status))
        .expect("writing to String cannot fail");
    writeln!(
        output,
        "AGENTS {}  TOTAL {}  LIVING {}  ACTIVE {}  DEAD {}",
        population_label(state.population_status),
        state.snapshot.agent_count,
        state.snapshot.living_agent_count,
        state.snapshot.active_agent_count,
        state.snapshot.death_count,
    )
    .expect("writing to String cannot fail");
    write!(output, "GESTURES {}", state.gestures.total).expect("writing to String cannot fail");
    if let Some(last) = state.gestures.last {
        write!(output, "  LAST #{} ", last.id).expect("writing to String cannot fail");
        match last.word {
            Some(word) => {
                output.push('"');
                write_form(output, word);
                output.push('"');
            }
            None => output.push('-'),
        }
        write!(
            output,
            " {} -> {} WATCHERS",
            mime_label(last.mime),
            last.watchers
        )
        .expect("writing to String cannot fail");
    }
    output.push('\n');
    if let Some(selection) = state.selection {
        writeln!(
            output,
            "SELECT {} X {}  {}",
            selection.max.x - selection.min.x,
            selection.max.y - selection.min.y,
            if state.selection_valid {
                "VALID"
            } else {
                "INVALID"
            }
        )
        .expect("writing to String cannot fail");
    }

    let Some(position) = state.cursor_world else {
        writeln!(output, "CURSOR  MOVE OVER MAP TO INSPECT")
            .expect("writing to String cannot fail");
        writeln!(output, "L-DRAG PAN  R-DRAG GENERATE").expect("writing to String cannot fail");
        writeln!(output, "T AGENT  NUM5 OBJECT MENU").expect("writing to String cannot fail");
        write!(output, "SPACE PAUSE  1-9 SPEED  C CANCEL").expect("writing to String cannot fail");
        return;
    };

    writeln!(output, "CURSOR X {}  Y {}", position.x, position.y)
        .expect("writing to String cannot fail");
    match world.inspect_chunk_at(position) {
        Ok(inspection) => {
            writeln!(
                output,
                "CHUNK X {} Y {}  LOCAL {},{}",
                inspection.coord.x, inspection.coord.y, inspection.local.x, inspection.local.y
            )
            .expect("writing to String cannot fail");
            writeln!(output, "COVERAGE {}", coverage_label(inspection.presence))
                .expect("writing to String cannot fail");
            if let Some(cell) = world.cell(position) {
                writeln!(
                    output,
                    "SURFACE {}  BIOME {}",
                    surface_label(cell.surface()),
                    biome_label(cell.biome())
                )
                .expect("writing to String cannot fail");
                if let Some(climate) = world.climate_at(position) {
                    writeln!(
                        output,
                        "ELEV {}  TEMP {}  MOIST {}",
                        cell.elevation, climate.temperature, climate.moisture
                    )
                    .expect("writing to String cannot fail");
                    write!(output, "WIND {}  FEATURE ", wind_label(climate.wind))
                        .expect("writing to String cannot fail");
                    if let Some(object) = state.cursor_spawned_object {
                        match object.remaining {
                            Some(remaining) => write!(
                                output,
                                "SPAWNED {}  {} CAP {}",
                                spawn_kind_label(object.kind),
                                resource_label(object.kind.resource().expect("resource kind").kind),
                                remaining
                            )
                            .expect("writing to String cannot fail"),
                            None => write!(
                                output,
                                "SPAWNED {}  DRINKABLE",
                                spawn_kind_label(object.kind)
                            )
                            .expect("writing to String cannot fail"),
                        }
                    } else if let Some(feature) = world.feature_at(position) {
                        let resource = feature.base_resource();
                        write!(
                            output,
                            "{}  {} CAP {}",
                            feature_label(feature.kind),
                            resource_label(resource.kind),
                            resource.capacity
                        )
                        .expect("writing to String cannot fail");
                    } else {
                        output.push_str("NONE");
                    }
                }
            } else {
                write!(output, "CELL UNLOADED").expect("writing to String cannot fail");
            }
        }
        Err(GenerateAreaError::OutsideWorldBounds) => {
            write!(output, "OUTSIDE WORLD BOUNDARY").expect("writing to String cannot fail");
        }
        Err(_) => {
            write!(output, "CHUNK COORDINATES UNAVAILABLE").expect("writing to String cannot fail");
        }
    }
}

pub(super) fn write_agent_text(output: &mut String, inspection: Option<AgentInspection>) {
    output.clear();
    let Some(agent) = inspection else {
        return;
    };
    writeln!(output, "AGENT {}", agent.view.id.get()).unwrap();
    writeln!(
        output,
        "POSITION X {}  Y {}",
        agent.view.position.x, agent.view.position.y
    )
    .unwrap();
    writeln!(output, "ACTIVITY {}", activity_label(agent.view.activity)).unwrap();
    if let Some(policy) = agent.policy {
        writeln!(output, "GOAL {}", goal_label(policy.goal)).unwrap();
        writeln!(output, "WHY {}", policy_reason_label(policy.reason)).unwrap();
        if let Some(target) = policy.target {
            writeln!(output, "TARGET X {}  Y {}", target.x, target.y).unwrap();
        } else {
            writeln!(output, "TARGET NONE").unwrap();
        }
        writeln!(
            output,
            "STATUS {}  RETRIES {}",
            policy_status_label(agent.view.activity, policy),
            policy.retry_count
        )
        .unwrap();
        writeln!(
            output,
            "SEARCH HEADING {}",
            exploration_heading_label(policy.exploration_heading)
        )
        .unwrap();
    } else {
        writeln!(output, "POLICY NONE").unwrap();
    }
    if let Some(needs) = agent.needs {
        write_need(output, "HUNGER", needs.hunger);
        write_need(output, "THIRST", needs.thirst);
        write_need(output, "REST", needs.rest);
        write_need(output, "EXPOSURE", needs.exposure);
        if let Some(next) = needs.next_threshold {
            writeln!(
                output,
                "NEXT {} AT TICK {}",
                need_label(next.kind),
                next.due.ticks()
            )
            .unwrap();
        } else {
            writeln!(output, "NEXT NEED NONE").unwrap();
        }
    } else {
        writeln!(output, "NEEDS UNAVAILABLE").unwrap();
    }
    if let Some(inventory) = agent.inventory {
        writeln!(
            output,
            "INVENTORY F {}  W {}  S {}",
            inventory.amount(Material::Berries),
            inventory.amount(Material::Wood),
            inventory.amount(Material::Stone)
        )
        .unwrap();
    }
    if let Some(health) = agent.health {
        writeln!(
            output,
            "HEALTH {}  {}",
            health.value,
            health_label(health.status)
        )
        .unwrap();
        if let Some(due) = health.next_consequence {
            writeln!(output, "NEXT DAMAGE TICK {}", due.ticks()).unwrap();
        }
    }
    if let Some(death) = agent.death {
        writeln!(output, "DEATH CAUSE {}", death_cause_label(death.cause)).unwrap();
        writeln!(output, "DIED AT TICK {}", death.at.ticks()).unwrap();
    }
    if let Some(sleep) = agent.sleep {
        writeln!(
            output,
            "SLEEP {}  WAKE {}",
            sleep_quality_label(sleep.quality),
            sleep.planned_wake.ticks()
        )
        .unwrap();
    } else {
        writeln!(output, "SLEEP NONE").unwrap();
    }
    if let Some(memory) = agent.memory {
        write_memory(output, &memory);
    } else {
        writeln!(output, "MEMORY NONE").unwrap();
    }
    debug_assert!(output.len() <= AGENT_TEXT_CAPACITY);
}

fn write_memory(output: &mut String, memory: &MemoryInspection) {
    let mut counts = [0_u8; LandmarkKind::ALL.len()];
    let mut hints = 0_u8;
    for place in memory.places() {
        counts[place.kind as usize] += 1;
        hints += u8::from(place.source == LandmarkSource::Told);
    }
    let count = |kind: LandmarkKind| counts[kind as usize];
    writeln!(
        output,
        "MEMORY WATER {}  FOOD {}  WOOD {}  STONE {}",
        count(LandmarkKind::Water),
        count(LandmarkKind::Berries),
        count(LandmarkKind::Wood),
        count(LandmarkKind::Stone),
    )
    .unwrap();
    writeln!(
        output,
        "SHELTER {}  HINTS {}  EXPLORED {} TILES",
        count(LandmarkKind::Shelter),
        hints,
        memory.explored_tiles
    )
    .unwrap();
    if memory.child {
        writeln!(output, "CHILD, BORN WITH NO WORDS").unwrap();
    }
    write_personality(output, memory.personality);
    write_friends(output, memory.acquaintances());
    write_words(output, memory);
}

/// Place concepts whose words the card lists, in display order.
const PLACE_CONCEPTS: [(Concept, &str); 5] = [
    (Concept::Water, "WATER"),
    (Concept::Berries, "FOOD"),
    (Concept::Wood, "WOOD"),
    (Concept::Stone, "STONE"),
    (Concept::Home, "HOME"),
];

/// `WORDS <CONCEPT> <FORM>  ...` for each place concept the agent has a word
/// for, wrapping onto an indented continuation line rather than exceeding
/// `AGENT_CARD_LINE_WIDTH`. Writes nothing when the agent has no place words.
fn write_words(output: &mut String, memory: &MemoryInspection) {
    const PREFIX: &str = "WORDS";
    let mut line_start = None;
    for (concept, label) in PLACE_CONCEPTS {
        let Some(form) = memory.word_for(concept) else {
            continue;
        };
        let entry_len = 2 + label.len() + 1 + FORM_NAME_LEN;
        match line_start {
            None => {
                line_start = Some(output.len());
                output.push_str(PREFIX);
                output.push(' ');
            }
            Some(start) if output.len() + entry_len - start > AGENT_CARD_LINE_WIDTH => {
                output.push('\n');
                line_start = Some(output.len());
                output.extend([' '; PREFIX.len() + 1]);
            }
            Some(_) => output.push_str("  "),
        }
        output.push_str(label);
        output.push(' ');
        write_form(output, form);
    }
    if line_start.is_some() {
        output.push('\n');
    }
}

/// Characters in every `VocalForm::name`.
const FORM_NAME_LEN: usize = 4;

/// A vocal form's readable name, uppercased for the bitmap font.
fn write_form(output: &mut String, form: VocalForm) {
    let name = form.name();
    debug_assert_eq!(name.len(), FORM_NAME_LEN);
    output.extend(name.chars().map(|letter| letter.to_ascii_uppercase()));
}

/// Width cap for the personality, friend, and word lines, so they never widen the card
/// past its existing worst-case lines (`NEXT EXPOSURE AT TICK <u64::MAX>` is 42
/// characters, `SLEEP OPEN GROUND  WAKE <u64::MAX>` is 44).
pub(super) const AGENT_CARD_LINE_WIDTH: usize = 42;

/// A trait this far from the average (128) is pronounced enough to name.
const NOTABLE_TRAIT_DEVIATION: u8 = 48;

/// One line: the most pronounced trait as a one-word nature, then all four values.
fn write_personality(output: &mut String, personality: Personality) {
    writeln!(
        output,
        "{}  CUR {} CAU {} SOC {} DIL {}",
        personality_summary(personality),
        personality.curiosity,
        personality.caution,
        personality.sociability,
        personality.diligence,
    )
    .unwrap();
}

/// Names the trait that deviates most from average (ties go to the earlier
/// trait in curiosity, caution, sociability, diligence order), or `BALANCED`
/// when no trait is notable.
pub(super) fn personality_summary(personality: Personality) -> &'static str {
    let traits = [
        (personality.curiosity, "EXPLORER", "HOMEBODY"),
        (personality.caution, "CAREFUL", "DARING"),
        (personality.sociability, "SOCIABLE", "LONER"),
        (personality.diligence, "DILIGENT", "EASYGOING"),
    ];
    let mut summary = "BALANCED";
    let mut strongest = NOTABLE_TRAIT_DEVIATION - 1;
    for (value, high, low) in traits {
        let deviation = value.abs_diff(128);
        if deviation > strongest {
            strongest = deviation;
            summary = if value > 128 { high } else { low };
        }
    }
    summary
}

/// `FRIENDS <friends> OF <known> KNOWN`, then, when there are friends, a `TOP`
/// line listing the most familiar ones (familiarity, then trust, then lowest id)
/// with their trust, as many as fit in `AGENT_CARD_LINE_WIDTH`.
fn write_friends(output: &mut String, acquaintances: &[AcquaintanceView]) {
    let known = &acquaintances[..acquaintances.len().min(ACQUAINTANCE_SLOTS)];
    let is_friend =
        |acquaintance: &&AcquaintanceView| acquaintance.familiarity >= FRIEND_FAMILIARITY;
    let friend_count = known.iter().filter(is_friend).count();
    writeln!(output, "FRIENDS {friend_count} OF {} KNOWN", known.len()).unwrap();
    let Some(first) = known.iter().find(is_friend) else {
        return;
    };
    let mut friends = [first; ACQUAINTANCE_SLOTS];
    for (slot, friend) in friends.iter_mut().zip(known.iter().filter(is_friend)) {
        *slot = friend;
    }
    let friends = &mut friends[..friend_count];
    friends.sort_unstable_by_key(|friend| {
        (
            Reverse(friend.familiarity),
            Reverse(friend.trust),
            friend.agent.get(),
        )
    });
    let line_start = output.len();
    output.push_str("TOP");
    for friend in friends.iter() {
        let before = output.len();
        write!(output, "  #{} T{}", friend.agent.get(), friend.trust).unwrap();
        if output.len() - line_start > AGENT_CARD_LINE_WIDTH {
            output.truncate(before);
            break;
        }
    }
    output.push('\n');
}

fn write_need(output: &mut String, label: &str, need: sim_core::NeedLevelView) {
    writeln!(
        output,
        "{label} {} OF {}  RATE {:+}",
        need.value, need.threshold, need.rate_per_period
    )
    .unwrap();
}

const fn mime_label(mime: Mime) -> &'static str {
    match mime {
        Mime::Scoop => "SCOOP",
        Mime::PickAndChew => "PICK-AND-CHEW",
        Mime::Chop => "CHOP",
        Mime::Strike => "STRIKE",
        Mime::RestHead => "REST-HEAD",
        Mime::Sweep => "SWEEP",
        Mime::Retch => "RETCH",
    }
}

const fn generation_label(status: GenerationStatus) -> &'static str {
    match status {
        GenerationStatus::Idle => "READY",
        GenerationStatus::Bootstrap => "LOADING WORLD",
        GenerationStatus::Manual => "GENERATING SELECTION",
        GenerationStatus::Cancelling => "CANCELLING",
        GenerationStatus::WorkerUnavailable => "WORKER OFFLINE",
    }
}

const fn population_label(status: PopulationStatus) -> &'static str {
    match status {
        PopulationStatus::Waiting => "WAITING FOR WORLD",
        PopulationStatus::Ready => "READY - PRESS T",
        PopulationStatus::Active => "ACTIVE",
    }
}

const fn activity_label(activity: AgentActivity) -> &'static str {
    match activity {
        AgentActivity::Idle => "IDLE",
        AgentActivity::Moving => "MOVING",
        AgentActivity::Gathering => "GATHERING",
        AgentActivity::Building => "BUILDING",
        AgentActivity::Sleeping => "SLEEPING",
        AgentActivity::Incapacitated => "INCAPACITATED",
        AgentActivity::Dead => "DEAD",
    }
}

const fn goal_label(goal: PhysicalGoal) -> &'static str {
    match goal {
        PhysicalGoal::SeekWater => "SEEK WATER",
        PhysicalGoal::SeekFood => "SEEK FOOD",
        PhysicalGoal::GatherMaterial => "GATHER MATERIAL",
        PhysicalGoal::Eat => "EAT",
        PhysicalGoal::Drink => "DRINK",
        PhysicalGoal::Sleep => "SLEEP",
        PhysicalGoal::SeekShelter => "SEEK SHELTER",
        PhysicalGoal::BuildShelter => "BUILD SHELTER",
        PhysicalGoal::Wait => "WAIT",
        PhysicalGoal::Incapacitated => "INCAPACITATED",
        PhysicalGoal::Explore => "EXPLORE",
        PhysicalGoal::Signal => "POINT OUT PLACE",
    }
}

const fn policy_reason_label(reason: PolicyReason) -> &'static str {
    match reason {
        PolicyReason::InitialDecision => "INITIAL DECISION",
        PolicyReason::ThirstThreshold => "THIRST THRESHOLD",
        PolicyReason::HungerThreshold => "HUNGER THRESHOLD",
        PolicyReason::RestThreshold => "REST THRESHOLD",
        PolicyReason::ExposureThreshold => "EXPOSURE THRESHOLD",
        PolicyReason::NoUrgentNeed => "NO URGENT NEED",
        PolicyReason::RouteArrived => "ROUTE ARRIVED",
        PolicyReason::ActionCompleted => "ACTION COMPLETED",
        PolicyReason::ShelterMaterials => "SHELTER MATERIALS",
        PolicyReason::Retry => "RETRY",
        PolicyReason::RememberedPlace => "REMEMBERED PLACE",
        PolicyReason::ToldPlace => "PLACE SOMEONE POINTED OUT",
        PolicyReason::PrepareTrip => "PREPARING FOR TRIP",
        PolicyReason::Sharing => "SHARING A PLACE",
        PolicyReason::Returning => "RETURNING TO WATER",
        PolicyReason::Visiting => "VISITING A FRIEND",
        PolicyReason::Following => "FOLLOWING A PARENT",
        PolicyReason::Begging => "ASKING FOR FOOD",
    }
}

const fn death_cause_label(cause: DeathCause) -> &'static str {
    match cause {
        DeathCause::Dehydration => "DEHYDRATION",
        DeathCause::Exposure => "EXPOSURE",
        DeathCause::Starvation => "STARVATION",
        DeathCause::Exhaustion => "EXHAUSTION",
    }
}

const fn need_label(need: NeedKind) -> &'static str {
    match need {
        NeedKind::Hunger => "HUNGER",
        NeedKind::Thirst => "THIRST",
        NeedKind::Rest => "REST",
        NeedKind::Exposure => "EXPOSURE",
    }
}

const fn health_label(status: HealthStatus) -> &'static str {
    match status {
        HealthStatus::Healthy => "HEALTHY",
        HealthStatus::Incapacitated => "INCAPACITATED",
        HealthStatus::Dead => "DEAD",
    }
}

const fn sleep_quality_label(quality: SleepQuality) -> &'static str {
    match quality {
        SleepQuality::OpenGround => "OPEN GROUND",
        SleepQuality::Sheltered => "SHELTERED",
    }
}

const fn policy_status_label(activity: AgentActivity, policy: PhysicalPolicyView) -> &'static str {
    if matches!(activity, AgentActivity::Incapacitated | AgentActivity::Dead) {
        "INACTIVE"
    } else if policy.committed {
        "COMMITTED"
    } else if policy.retry_count > 0 {
        "BACKOFF"
    } else {
        "DECIDING"
    }
}

const fn exploration_heading_label(heading: ExplorationHeading) -> &'static str {
    match heading {
        ExplorationHeading::North => "N",
        ExplorationHeading::NorthEast => "NE",
        ExplorationHeading::East => "E",
        ExplorationHeading::SouthEast => "SE",
        ExplorationHeading::South => "S",
        ExplorationHeading::SouthWest => "SW",
        ExplorationHeading::West => "W",
        ExplorationHeading::NorthWest => "NW",
    }
}

const fn coverage_label(presence: ChunkPresence) -> &'static str {
    match presence {
        ChunkPresence::Missing => "MISSING",
        ChunkPresence::InitialUnloaded => "INITIAL UNLOADED",
        ChunkPresence::PartialInitialUnloaded => "PARTIAL INITIAL UNLOADED",
        ChunkPresence::PartialInitial => "PARTIAL INITIAL",
        ChunkPresence::Initial => "INITIAL",
        ChunkPresence::Retained => "RETAINED",
        ChunkPresence::RetainedPartialInitial => "PARTIAL INITIAL RETAINED",
    }
}

const fn surface_label(surface: SurfaceType) -> &'static str {
    match surface {
        SurfaceType::DeepWater => "DEEP WATER",
        SurfaceType::ShallowWater => "SHALLOW WATER",
        SurfaceType::Sand => "SAND",
        SurfaceType::Soil => "SOIL",
        SurfaceType::Hill => "HILL",
        SurfaceType::Rock => "ROCK",
        SurfaceType::SnowIce => "SNOW/ICE",
    }
}

const fn biome_label(biome: BiomeType) -> &'static str {
    match biome {
        BiomeType::Ocean => "OCEAN",
        BiomeType::Lake => "LAKE",
        BiomeType::River => "RIVER",
        BiomeType::Beach => "BEACH",
        BiomeType::Desert => "DESERT",
        BiomeType::Grassland => "GRASSLAND",
        BiomeType::Savanna => "SAVANNA",
        BiomeType::Forest => "FOREST",
        BiomeType::Wetland => "WETLAND",
        BiomeType::Tundra => "TUNDRA",
        BiomeType::Alpine => "ALPINE",
    }
}

const fn wind_label(wind: PrevailingWind) -> &'static str {
    match wind {
        PrevailingWind::Southeast => "SE",
        PrevailingWind::Northwest => "NW",
    }
}

const fn feature_label(feature: FeatureKind) -> &'static str {
    match feature {
        FeatureKind::Tree => "TREE",
        FeatureKind::Rock => "ROCK",
        FeatureKind::BerryBush => "BERRY BUSH",
        FeatureKind::BitterBush => "BITTER BUSH",
    }
}

const fn resource_label(resource: Material) -> &'static str {
    match resource {
        Material::Berries => "BERRIES",
        Material::Bitterberries => "BITTERBERRIES",
        Material::Wood => "WOOD",
        Material::Stone => "STONE",
    }
}
