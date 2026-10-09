//! Agent, spawned-object, and structure instance tests.

use sim_core::{
    AgentActivity, AgentView, LANDMARK_SLOTS, LandmarkKind, LandmarkSource, LandmarkView,
    SpawnKind, SpawnedObjectView, StructureState, WorldPosition, WorldRect,
};

use crate::render::colors::{agent_color, landmark_color, spawn_kind_color, structure_color};
use crate::render::instances::{
    build_agent_instances, build_memory_marker_instances, build_spawned_object_instances,
};
use crate::render::summary::{CacheSyncAction, cache_sync_action};
use crate::render::{
    MAX_AGENT_INSTANCES, MAX_MEMORY_MARKER_INSTANCES, MAX_SPAWNED_OBJECT_INSTANCES,
};

#[test]
fn agent_instances_reflect_position_activity_and_bounded_far_zoom_culling() {
    let bounds = WorldRect {
        min: WorldPosition { x: -4, y: -4 },
        max: WorldPosition { x: 4, y: 4 },
    };
    let activities = [
        AgentActivity::Idle,
        AgentActivity::Moving,
        AgentActivity::Gathering,
        AgentActivity::Building,
        AgentActivity::Sleeping,
        AgentActivity::Incapacitated,
        AgentActivity::Dead,
    ];
    let views = activities
        .into_iter()
        .enumerate()
        .map(|(index, activity)| AgentView {
            id: sim_core::AgentId::new(index as u32),
            position: WorldPosition {
                x: index as i64 - 3,
                y: 0,
            },
            activity,
        });
    let mut instances = Vec::new();
    build_agent_instances(views, bounds, 2.0, &mut instances);
    assert_eq!(instances.len(), activities.len());
    assert_eq!(instances[0].color, agent_color(AgentActivity::Idle));
    assert_eq!(
        instances[5].color,
        agent_color(AgentActivity::Incapacitated)
    );
    assert_eq!(instances[6].color, agent_color(AgentActivity::Dead));
    assert_ne!(instances[5].color, instances[6].color);
    assert_eq!(instances[0].position, [-2.86, 0.14]);

    build_agent_instances(
        std::iter::repeat_n(
            AgentView {
                id: sim_core::AgentId::new(0),
                position: WorldPosition { x: 0, y: 0 },
                activity: AgentActivity::Moving,
            },
            MAX_AGENT_INSTANCES + 100,
        ),
        bounds,
        2.0,
        &mut instances,
    );
    assert_eq!(instances.len(), MAX_AGENT_INSTANCES);
    build_agent_instances(std::iter::empty(), bounds, 0.5, &mut instances);
    assert!(instances.is_empty());
}

#[test]
fn spawned_object_instances_use_kind_geometry_culling_and_capacity() {
    let bounds = WorldRect {
        min: WorldPosition { x: -2, y: -2 },
        max: WorldPosition { x: 3, y: 3 },
    };
    let views = SpawnKind::ALL
        .into_iter()
        .enumerate()
        .map(|(index, kind)| SpawnedObjectView {
            position: WorldPosition {
                x: index as i64 - 1,
                y: 0,
            },
            kind,
            remaining: kind.resource().map(|resource| resource.capacity),
        });
    let mut instances = Vec::new();
    build_spawned_object_instances(views, bounds, 2.0, &mut instances);
    assert_eq!(instances.len(), 4);
    assert_eq!(instances[0].color, spawn_kind_color(SpawnKind::Tree));
    assert_eq!(instances[3].size, [1.0, 1.0]);

    let repeated = std::iter::repeat_n(
        SpawnedObjectView {
            position: WorldPosition { x: 0, y: 0 },
            kind: SpawnKind::Rock,
            remaining: Some(80),
        },
        MAX_SPAWNED_OBJECT_INSTANCES + 1,
    );
    build_spawned_object_instances(repeated, bounds, 2.0, &mut instances);
    assert_eq!(instances.len(), MAX_SPAWNED_OBJECT_INSTANCES);
    build_spawned_object_instances(std::iter::empty(), bounds, 0.5, &mut instances);
    assert!(instances.is_empty());
}

#[test]
fn dynamic_agents_do_not_enter_the_immutable_terrain_cache_key() {
    let before = AgentView {
        id: sim_core::AgentId::new(0),
        position: WorldPosition { x: 0, y: 0 },
        activity: AgentActivity::Idle,
    };
    let after = AgentView {
        position: WorldPosition { x: 1, y: 0 },
        activity: AgentActivity::Moving,
        ..before
    };
    assert_ne!(before, after);
    assert_eq!(
        cache_sync_action(true, true, false, false, false),
        CacheSyncAction::Skip
    );
}

#[test]
fn shelter_lifecycle_states_have_distinct_footprint_colors() {
    assert_ne!(
        structure_color(StructureState::UnderConstruction),
        structure_color(StructureState::Complete)
    );
}

#[test]
fn memory_markers_show_seen_places_solid_and_hints_as_search_outlines() {
    let seen = LandmarkView {
        kind: LandmarkKind::Water,
        position: WorldPosition { x: 10, y: -4 },
        source: LandmarkSource::Seen,
        confidence: 255,
        search_radius: 0,
        seen_second: 5,
    };
    let told = LandmarkView {
        kind: LandmarkKind::Food,
        position: WorldPosition { x: -20, y: 6 },
        source: LandmarkSource::Told,
        confidence: 120,
        search_radius: 8,
        seen_second: 9,
    };
    let narrow_hint = LandmarkView {
        kind: LandmarkKind::Stone,
        search_radius: 0,
        ..told
    };
    let mut instances = Vec::new();

    build_memory_marker_instances(&[seen, told, narrow_hint], 4.0, &mut instances);
    assert_eq!(instances.len(), 1 + 4 + 4);
    let marker = instances[0];
    assert_eq!(marker.color, landmark_color(LandmarkKind::Water));
    assert_eq!(marker.size, [1.5, 1.5]);
    assert_eq!(marker.position, [9.75, -4.25]);

    let outline = &instances[1..5];
    assert!(
        outline
            .iter()
            .all(|edge| edge.color == landmark_color(LandmarkKind::Food))
    );
    assert_eq!(outline[0].position, [-28.0, -2.0]);
    assert_eq!(outline[0].size, [17.0, 0.5]);
    assert_eq!(outline[2].size, [0.5, 17.0]);
    assert_eq!(outline[3].position, [-11.5, -2.0]);
    assert_eq!(
        instances[5].size[0], 7.0,
        "hints without a search radius still show a few cells of uncertainty"
    );

    let sightings = [seen; LANDMARK_SLOTS * 2];
    build_memory_marker_instances(&sightings, 4.0, &mut instances);
    assert_eq!(instances.len(), LANDMARK_SLOTS);
    let hints = [told; LANDMARK_SLOTS];
    build_memory_marker_instances(&hints, 4.0, &mut instances);
    assert_eq!(instances.len(), MAX_MEMORY_MARKER_INSTANCES);
    build_memory_marker_instances(&hints, 0.5, &mut instances);
    assert!(instances.is_empty());
}
