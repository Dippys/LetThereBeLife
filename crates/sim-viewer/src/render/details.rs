//! The F3 technical readout: raw simulation, cursor-cell, and selected-agent values.

use std::fmt::Write;

use sim_core::{
    AgentActivity, ChunkPresence, ExplorationHeading, FeatureKind, GenerateAreaError,
    LandmarkSource, NeedLevelView, PhysicalPolicyView, PrevailingWind, SurfaceType, World,
};

use super::{AgentInspection, RenderState};

pub(super) fn write_details(output: &mut String, world: &World, state: &RenderState) {
    output.clear();
    let snapshot = &state.snapshot;
    writeln!(
        output,
        "TICK {}  SEED {}  CHUNKS {}  REV {}",
        snapshot.tick,
        snapshot.seed,
        world.loaded_chunk_count(),
        world.revision()
    )
    .unwrap();
    writeln!(
        output,
        "AGENTS {}  LIVING {}  ACTIVE {}  STRUCTURES {}  EVENTS {}",
        snapshot.agent_count,
        snapshot.living_agent_count,
        snapshot.active_agent_count,
        snapshot.structure_count,
        snapshot.scheduled_event_count
    )
    .unwrap();
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
        .unwrap();
    }
    write_cursor(output, world, state);
    if let Some(agent) = &state.selected {
        write_agent_details(output, agent);
    }
}

fn write_cursor(output: &mut String, world: &World, state: &RenderState) {
    let Some(position) = state.cursor_world else {
        writeln!(output, "CURSOR OFF MAP").unwrap();
        return;
    };
    writeln!(output, "CURSOR X {}  Y {}", position.x, position.y).unwrap();
    match world.inspect_chunk_at(position) {
        Ok(inspection) => {
            writeln!(
                output,
                "CHUNK X {} Y {}  LOCAL {},{}  {}",
                inspection.coord.x,
                inspection.coord.y,
                inspection.local.x,
                inspection.local.y,
                coverage_label(inspection.presence)
            )
            .unwrap();
            let Some(cell) = world.cell(position) else {
                writeln!(output, "CELL UNLOADED").unwrap();
                return;
            };
            write!(
                output,
                "SURFACE {}  ELEV {}",
                surface_label(cell.surface()),
                cell.elevation
            )
            .unwrap();
            if let Some(climate) = world.climate_at(position) {
                write!(
                    output,
                    "  TEMP {}  MOIST {}  WIND {}",
                    climate.temperature,
                    climate.moisture,
                    wind_label(climate.wind)
                )
                .unwrap();
            }
            output.push('\n');
            if let Some(feature) = world.feature_at(position) {
                writeln!(
                    output,
                    "FEATURE {}  CAPACITY {}",
                    feature_label(feature.kind),
                    feature.base_resource().capacity
                )
                .unwrap();
            }
        }
        Err(GenerateAreaError::OutsideWorldBounds) => {
            writeln!(output, "OUTSIDE WORLD BOUNDARY").unwrap();
        }
        Err(_) => writeln!(output, "CHUNK COORDINATES UNAVAILABLE").unwrap(),
    }
}

/// Raw values behind the person panel: ticks, thresholds, rates, and traits.
pub(super) fn write_agent_details(output: &mut String, agent: &AgentInspection) {
    writeln!(
        output,
        "AGENT {} AT {},{}  {:?}",
        agent.view.id.get(),
        agent.view.position.x,
        agent.view.position.y,
        agent.view.activity
    )
    .unwrap();
    if let Some(policy) = agent.policy {
        writeln!(output, "GOAL {:?}  WHY {:?}", policy.goal, policy.reason).unwrap();
        write!(
            output,
            "{}  RETRIES {}  HEADING {}",
            policy_status_label(agent.view.activity, policy),
            policy.retry_count,
            exploration_heading_label(policy.exploration_heading)
        )
        .unwrap();
        if let Some(target) = policy.target {
            write!(output, "  TARGET {},{}", target.x, target.y).unwrap();
        }
        output.push('\n');
    }
    if let Some(needs) = agent.needs {
        writeln!(
            output,
            "HUNGER {}  THIRST {}",
            need_text(needs.hunger),
            need_text(needs.thirst)
        )
        .unwrap();
        writeln!(
            output,
            "REST {}  EXPOSURE {}",
            need_text(needs.rest),
            need_text(needs.exposure)
        )
        .unwrap();
        if let Some(next) = needs.next_threshold {
            writeln!(output, "NEXT {:?} AT TICK {}", next.kind, next.due.ticks()).unwrap();
        }
    }
    if let Some(health) = agent.health {
        write!(output, "HEALTH {} {:?}", health.value, health.status).unwrap();
        if let Some(due) = health.next_consequence {
            write!(output, "  NEXT DAMAGE TICK {}", due.ticks()).unwrap();
        }
        output.push('\n');
    }
    if let Some(sleep) = agent.sleep {
        writeln!(
            output,
            "SLEEP {:?}  WAKE TICK {}",
            sleep.quality,
            sleep.planned_wake.ticks()
        )
        .unwrap();
    }
    if let Some(death) = agent.death {
        writeln!(
            output,
            "DIED {:?} AT TICK {}",
            death.cause,
            death.at.ticks()
        )
        .unwrap();
    }
    if let Some(memory) = &agent.memory {
        let hints = memory
            .places()
            .iter()
            .filter(|place| place.source == LandmarkSource::Told)
            .count();
        writeln!(
            output,
            "PLACES {}  HINTS {}  EXPLORED {}  WORDS {}",
            memory.places().len(),
            hints,
            memory.explored_tiles,
            memory.lexicon().len()
        )
        .unwrap();
        let traits = memory.personality;
        writeln!(
            output,
            "CUR {} CAU {} SOC {} DIL {}",
            traits.curiosity, traits.caution, traits.sociability, traits.diligence
        )
        .unwrap();
    }
}

fn need_text(need: NeedLevelView) -> String {
    format!(
        "{}/{} {:+}",
        need.value, need.threshold, need.rate_per_period
    )
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
