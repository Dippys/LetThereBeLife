//! World-space instance tests: agents, objects, structures, the selected person's
//! markers, gestures, chunk outlines, the world border, and buffer partitioning.

use bytemuck::Zeroable;
use sim_core::{
    ACQUAINTANCE_SLOTS, AcquaintanceView, AgentActivity, AgentView, DEFAULT_TRUST,
    FRIEND_FAMILIARITY, LANDMARK_SLOTS, LandmarkKind, LandmarkSource, LandmarkView, SpawnKind,
    SpawnedObjectView, StructureState, World, WorldConfig, WorldPosition, WorldRect,
};

use crate::gestures::{GestureMark, RECENT_GESTURE_CAPACITY};
use crate::render::colors::{
    GESTURE_COLOR, agent_color, landmark_color, relationship_color, rgba, selection_color,
    spawn_kind_color, structure_color,
};
use crate::render::gpu::{Instance, static_instance_chunks};
use crate::render::instances::{
    build_agent_instances, build_gesture_instances, build_memory_marker_instances,
    build_relationship_marker_instances, build_spawned_object_instances, chunk_outline,
    world_border,
};
use crate::render::summary::{CacheSyncAction, cache_sync_action};
use crate::render::{
    MAX_AGENT_INSTANCES, MAX_GESTURE_DOTS, MAX_GESTURE_MARKER_INSTANCES, MAX_INSTANCES_PER_BUFFER,
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
    build_agent_instances(views.map(|view| (view, None)), bounds, 2.0, &mut instances);
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
            (
                AgentView {
                    id: sim_core::AgentId::new(0),
                    position: WorldPosition { x: 0, y: 0 },
                    activity: AgentActivity::Moving,
                },
                None,
            ),
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
fn women_children_and_elders_are_drawn_distinctly() {
    let bounds = WorldRect {
        min: WorldPosition { x: -4, y: -4 },
        max: WorldPosition { x: 4, y: 4 },
    };
    let person = |id: u32, sex, age| {
        (
            AgentView {
                id: sim_core::AgentId::new(id),
                position: WorldPosition { x: 0, y: 0 },
                activity: AgentActivity::Idle,
            },
            Some(sim_core::LifeView {
                name: sim_core::Name(0),
                sex,
                age,
                stage: sim_core::LifeStage::of(age),
            }),
        )
    };
    let mut instances = Vec::new();
    build_agent_instances(
        [person(0, sim_core::Sex::Male, 30)],
        bounds,
        4.0,
        &mut instances,
    );
    assert_eq!(instances.len(), 1, "a man is one square");
    build_agent_instances(
        [person(1, sim_core::Sex::Female, 30)],
        bounds,
        4.0,
        &mut instances,
    );
    assert_eq!(instances.len(), 2, "a woman's square has notched corners");
    build_agent_instances(
        [person(2, sim_core::Sex::Male, 8)],
        bounds,
        4.0,
        &mut instances,
    );
    assert!(instances[0].size[0] < 0.5, "children are smaller");
    build_agent_instances(
        [person(3, sim_core::Sex::Male, 70)],
        bounds,
        4.0,
        &mut instances,
    );
    assert_eq!(instances.len(), 2, "an elder has a grey edge");
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
        structure_color(
            sim_core::StructureKind::Shelter,
            StructureState::UnderConstruction
        ),
        structure_color(sim_core::StructureKind::Shelter, StructureState::Complete)
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
        kind: LandmarkKind::BERRIES,
        position: WorldPosition { x: -20, y: 6 },
        source: LandmarkSource::Told,
        confidence: 120,
        search_radius: 8,
        seen_second: 9,
    };
    let narrow_hint = LandmarkView {
        kind: LandmarkKind::STONE,
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
            .all(|edge| edge.color == landmark_color(LandmarkKind::BERRIES))
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
        tie: None,
        owed: 0,
        name: None,
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
fn gestures_draw_a_neutral_dotted_line_and_an_end_square() {
    let gesture = GestureMark {
        id: 3,
        sender: sim_core::AgentId::new(1),
        origin: WorldPosition { x: 0, y: 0 },
        inferred_position: WorldPosition { x: 10, y: 0 },
        word: None,
        mime: sim_core::Mime::PickAndChew,
        loud: false,
    };
    let mut instances = Vec::new();

    // At 4 px per cell the line is 40 px: 4 dots at 8 px spacing, then the
    // end square on the inferred cell.
    build_gesture_instances([&gesture], 4.0, &mut instances);
    assert_eq!(instances.len(), 4 + 1);
    let dot_centers: Vec<_> = instances[..4]
        .iter()
        .map(|dot| dot.position[0] + dot.size[0] / 2.0)
        .collect();
    assert_eq!(dot_centers, [2.5, 4.5, 6.5, 8.5]);
    assert!(
        instances
            .iter()
            .all(|instance| instance.color == GESTURE_COLOR)
    );
    assert_eq!(instances[4].position, [9.875, -0.125]);
    assert_eq!(instances[4].size, [1.25, 1.25]);

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
        RECENT_GESTURE_CAPACITY * (MAX_GESTURE_DOTS + 1)
    );

    build_gesture_instances([&gesture], 0.5, &mut instances);
    assert!(instances.is_empty(), "far zoom hides gesture markers");
}

#[test]
fn static_buffers_partition_at_the_device_safe_limit() {
    let instances = vec![Instance::zeroed(); MAX_INSTANCES_PER_BUFFER + 1];
    let lengths: Vec<_> = static_instance_chunks(&instances).map(<[_]>::len).collect();
    assert_eq!(lengths, [MAX_INSTANCES_PER_BUFFER, 1]);
}

#[test]
fn invalid_selection_uses_red_preview() {
    assert_eq!(selection_color(true), rgba(255, 220, 35, 72));
    assert_eq!(selection_color(false), rgba(235, 48, 48, 96));
}

#[test]
fn world_border_marks_the_centered_generation_envelope() {
    let border = world_border(1.0);

    assert_eq!(border[0].position, [-32_768.0, -32_768.0]);
    assert_eq!(border[0].size, [65_536.0, 2.0]);
    assert_eq!(border[1].position, [-32_768.0, 32_766.0]);
    assert_eq!(border[2].size, [2.0, 65_536.0]);
    assert_eq!(border[3].position, [32_766.0, -32_768.0]);
    assert!(
        border
            .iter()
            .all(|instance| instance.color == rgba(245, 40, 40, 230))
    );
}

#[test]
fn chunk_outline_uses_signed_chunk_bounds() {
    let world = World::generate(1, WorldConfig::new(64, 64).unwrap());
    let inspection = world
        .inspect_chunk_at(WorldPosition { x: -1, y: 63 })
        .unwrap();
    let outline = chunk_outline(inspection, 1.0).unwrap();

    assert_eq!(outline[0].position, [-64.0, 0.0]);
    assert_eq!(outline[0].size, [64.0, 1.0]);
    assert_eq!(outline[1].position, [-64.0, 63.0]);
    assert_eq!(outline[2].position, [-64.0, 0.0]);
    assert_eq!(outline[3].position, [-1.0, 0.0]);
}

#[test]
fn subpixel_chunks_do_not_create_inspection_overlays() {
    let world = World::generate(1, WorldConfig::new(64, 64).unwrap());
    let inspection = world
        .inspect_chunk_at(WorldPosition { x: 0, y: 0 })
        .unwrap();
    assert!(chunk_outline(inspection, 0.01).is_none());
    assert!(chunk_outline(inspection, 0.1).is_some());
}
