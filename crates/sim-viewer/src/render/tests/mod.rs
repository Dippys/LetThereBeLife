//! Renderer tests, split by topic, plus the shared render-state fixture.

mod instances;
mod summary;
mod ui;

use sim_core::{SimulationSnapshot, WorldPosition};

use crate::camera::Camera;
use crate::render::{Census, GenerationStatus, PopulationStatus, RenderState};

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
        cursor: None,
        cursor_world,
        inspected: None,
        hovered: None,
        selection: None,
        selection_valid: true,
        generation_status: GenerationStatus::Idle,
        population_status: PopulationStatus::Active,
        hover: None,
        selected: None,
        following: false,
        toast: None,
        help_open: false,
        info_open: false,
        reached_speed: None,
        details_open: false,
        build: None,
        feed: Vec::new(),
        census: Census::default(),
        season: sim_core::Season::Spring,
    }
}
