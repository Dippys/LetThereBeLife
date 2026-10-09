//! Agent, spawned-object, structure, and hovered-agent marker instance tests.

use sim_core::{
    ACQUAINTANCE_SLOTS, AcquaintanceView, AgentActivity, AgentView, DEFAULT_TRUST,
    FRIEND_FAMILIARITY, LANDMARK_SLOTS, LandmarkKind, LandmarkSource, LandmarkView, SpawnKind,
    SpawnedObjectView, StructureState, WorldPosition, WorldRect,
};

use crate::gestures::{GestureMark, RECENT_GESTURE_CAPACITY};
use crate::render::colors::{
    GESTURE_COLOR, agent_color, gesture_topic_color, landmark_color, relationship_color,
    spawn_kind_color, structure_color,
};
use crate::render::instances::{
    build_agent_instances, build_gesture_instances, build_memory_marker_instances,
    build_relationship_marker_instances, build_spawned_object_instances,
};
use crate::render::summary::{CacheSyncAction, cache_sync_action};
use crate::render::{
    MAX_AGENT_INSTANCES, MAX_GESTURE_DOTS, MAX_GESTURE_MARKER_INSTANCES,
    MAX_MEMORY_MARKER_INSTANCES, MAX_RELATIONSHIP_DOTS, MAX_RELATIONSHIP_MARKER_INSTANCES,
    MAX_SPAWNED_OBJECT_INSTANCES,
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

#[test]
fn relationship_markers_dot_a_line_to_each_last_seen_position() {
    let origin = WorldPosition { x: 0, y: 0 };
    let friend = AcquaintanceView {
        agent: sim_core::AgentId::new(3),
        familiarity: FRIEND_FAMILIARITY,
        trust: DEFAULT_TRUST,
        last_seen_position: Some(WorldPosition { x: 10, y: 0 }),
        last_seen_second: 40,
    };
    let acquaintance = AcquaintanceView {
        agent: sim_core::AgentId::new(4),
        familiarity: FRIEND_FAMILIARITY - 1,
        last_seen_position: Some(WorldPosition { x: 0, y: -1 }),
        ..friend
    };
    let lost = AcquaintanceView {
        agent: sim_core::AgentId::new(5),
        last_seen_position: None,
        ..friend
    };
    let mut instances = Vec::new();

    // At 4 px per cell the friend is 40 px away: 3 dots at 10 px spacing, then
    // its end marker. The adjacent acquaintance is too close for dots.
    build_relationship_marker_instances(origin, &[friend, acquaintance, lost], 4.0, &mut instances);
    assert_eq!(instances.len(), 3 + 1 + 1);
    let dot_centers: Vec<_> = instances[..3]
        .iter()
        .map(|dot| dot.position[0] + dot.size[0] / 2.0)
        .collect();
    assert_eq!(dot_centers, [3.0, 5.5, 8.0]);
    assert!(instances[..3].iter().all(|dot| dot.size == [0.5, 0.5]
        && dot.position[1] == 0.25
        && dot.color == relationship_color(true)));
    assert_eq!(instances[3].position, [9.625, -0.375]);
    assert_eq!(instances[3].size, [1.75, 1.75]);
    assert_eq!(instances[4].position, [0.0, -1.0]);
    assert_eq!(instances[4].size, [1.0, 1.0]);
    assert_eq!(instances[4].color, relationship_color(false));
    assert_ne!(relationship_color(true), relationship_color(false));

    // Long lines cap their dots, and slots beyond the acquaintance limit are ignored.
    let far = AcquaintanceView {
        last_seen_position: Some(WorldPosition {
            x: 10_000,
            y: -7_000,
        }),
        ..friend
    };
    build_relationship_marker_instances(
        origin,
        &[far; ACQUAINTANCE_SLOTS + 2],
        4.0,
        &mut instances,
    );
    assert_eq!(instances.len(), MAX_RELATIONSHIP_MARKER_INSTANCES);
    assert_eq!(
        MAX_RELATIONSHIP_MARKER_INSTANCES,
        ACQUAINTANCE_SLOTS * (MAX_RELATIONSHIP_DOTS + 1)
    );

    let here = AcquaintanceView {
        last_seen_position: Some(origin),
        ..friend
    };
    build_relationship_marker_instances(origin, &[here], 4.0, &mut instances);
    assert_eq!(
        instances.len(),
        1,
        "an acquaintance at the agent's cell is just a marker"
    );
    build_relationship_marker_instances(origin, &[far], 0.5, &mut instances);
    assert!(instances.is_empty());
}

#[test]
fn gestures_draw_a_neutral_dotted_line_search_square_and_topic_dot() {
    let gesture = GestureMark {
        id: 3,
        origin: WorldPosition { x: 0, y: 0 },
        inferred_position: WorldPosition { x: 10, y: 0 },
        search_radius: 2,
        watchers: 2,
        word: None,
        mime: sim_core::Mime::PickAndChew,
        topic: sim_core::GestureTopic::Place(LandmarkKind::Food),
    };
    let mut instances = Vec::new();

    // At 4 px per cell the line is 40 px: 4 dots at 8 px spacing, then the
    // 4-sided search square around the inferred cell, then the topic dot.
    build_gesture_instances([&gesture], 4.0, &mut instances);
    assert_eq!(instances.len(), 4 + 4 + 1);
    let dot_centers: Vec<_> = instances[..4]
        .iter()
        .map(|dot| dot.position[0] + dot.size[0] / 2.0)
        .collect();
    assert_eq!(dot_centers, [2.5, 4.5, 6.5, 8.5]);
    assert!(
        instances[..8]
            .iter()
            .all(|instance| instance.color == GESTURE_COLOR)
    );
    assert_eq!(instances[4].position, [8.0, -2.0]);
    assert_eq!(instances[4].size, [5.0, 0.5]);
    assert_eq!(instances[7].position, [12.5, -2.0]);
    assert_eq!(instances[7].size, [0.5, 5.0]);
    assert_eq!(instances[8].position, [-0.125, -0.125]);
    assert_eq!(instances[8].size, [1.25, 1.25]);
    assert_eq!(instances[8].color, landmark_color(LandmarkKind::Food));
    assert_eq!(
        gesture_topic_color(sim_core::GestureTopic::Place(LandmarkKind::Food)),
        landmark_color(LandmarkKind::Food)
    );
    assert_ne!(
        gesture_topic_color(sim_core::GestureTopic::Explored),
        GESTURE_COLOR
    );

    // Long lines cap their dots, and more gestures than the ring holds are ignored.
    let far = GestureMark {
        inferred_position: WorldPosition {
            x: 9_000,
            y: -4_000,
        },
        ..gesture
    };
    let many = [far; RECENT_GESTURE_CAPACITY + 3];
    build_gesture_instances(&many, 4.0, &mut instances);
    assert_eq!(instances.len(), MAX_GESTURE_MARKER_INSTANCES);
    assert_eq!(
        MAX_GESTURE_MARKER_INSTANCES,
        RECENT_GESTURE_CAPACITY * (MAX_GESTURE_DOTS + 5)
    );

    build_gesture_instances([&gesture], 0.5, &mut instances);
    assert!(instances.is_empty(), "far zoom hides gesture markers");
}
