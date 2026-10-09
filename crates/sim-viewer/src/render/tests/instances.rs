//! Agent, spawned-object, and structure instance tests.

use sim_core::{
    AgentActivity, AgentView, SpawnKind, SpawnedObjectView, StructureState, WorldPosition,
    WorldRect,
};

use crate::render::colors::{agent_color, spawn_kind_color, structure_color};
use crate::render::instances::{build_agent_instances, build_spawned_object_instances};
use crate::render::summary::{CacheSyncAction, cache_sync_action};
use crate::render::{MAX_AGENT_INSTANCES, MAX_SPAWNED_OBJECT_INSTANCES};

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
