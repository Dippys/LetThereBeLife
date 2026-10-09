//! Renderer tests, split by topic, plus the shared render-state fixture.

mod hud;
mod instances;
mod summary;

use sim_core::{SimulationSnapshot, WorldPosition};

use crate::camera::Camera;
use crate::gestures::GestureSummary;
use crate::render::{GenerationStatus, PopulationStatus, RenderState};

pub(super) fn test_render_state(cursor_world: Option<WorldPosition>) -> RenderState {
    RenderState {
        snapshot: SimulationSnapshot {
            tick: 3_721,
            simulated_seconds: 62.0,
            paused: false,
            speed: 4.0,
            seed: 7,
            agent_count: 0,
            living_agent_count: 0,
            active_agent_count: 0,
            death_count: 0,
            scheduled_event_count: 0,
            structure_count: 0,
        },
        camera: Camera::at_origin(),
        ui_scale: 1.0,
        cursor_world,
        cursor_spawned_object: None,
        inspected: None,
        hovered: None,
        selection: None,
        selection_valid: true,
        generation_status: GenerationStatus::Idle,
        population_status: PopulationStatus::Active,
        hovered_agent: None,
        spawn_message: None,
        spawn_menu: None,
        gestures: GestureSummary::default(),
    }
}
