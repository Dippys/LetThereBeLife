//! Instance builders for agents, structures, spawned objects, remembered-place, relationship, and gesture markers, exact world cells, chunk outlines, and the world border.

use sim_core::{
    ACQUAINTANCE_SLOTS, AcquaintanceView, AgentActivity, AgentView, AnimalMode, AnimalView,
    CHUNK_SIZE, ChunkInspection, ChunkPresence, FRIEND_FAMILIARITY, LANDMARK_SLOTS, LandmarkSource,
    LandmarkView, SpawnKind, SpawnedObjectView, Species, StructureView, WORLD_GENERATION_BOUNDS,
    World, WorldPosition, WorldRect,
};

use super::{
    MAX_AGENT_INSTANCES, MAX_CHUNK_OUTLINE_WORLD_WIDTH, MAX_GESTURE_DOTS, MAX_RELATIONSHIP_DOTS,
    MAX_SPAWNED_OBJECT_INSTANCES, MAX_STRUCTURE_INSTANCES, MAX_WORLD_BORDER_WIDTH,
    MIN_CHUNK_OUTLINE_PIXELS, MIN_DYNAMIC_INSTANCE_PIXELS,
    colors::{
        CARCASS, DEER, DEER_ALERT, GESTURE_COLOR, WOLF, WOLF_ALERT, agent_color, feature_color,
        landmark_color, relationship_color, rgba, spawn_kind_color, structure_color, terrain_color,
    },
    gpu::Instance,
};
use crate::gestures::{GestureMark, RECENT_GESTURE_CAPACITY};

pub(super) fn build_spawned_object_instances(
    views: impl IntoIterator<Item = SpawnedObjectView>,
    visible: WorldRect,
    scale: f32,
    output: &mut Vec<Instance>,
) {
    output.clear();
    if scale < MIN_DYNAMIC_INSTANCE_PIXELS {
        return;
    }
    for object in views
        .into_iter()
        .filter(|object| visible.contains(object.position))
        .take(MAX_SPAWNED_OBJECT_INSTANCES)
    {
        let inset = match object.kind {
            SpawnKind::Water => 0.0,
            SpawnKind::Tree | SpawnKind::Rock => 0.08,
            SpawnKind::BerryBush => 0.18,
        };
        output.push(Instance::new(
            object.position.x as f32 + inset,
            object.position.y as f32 + inset,
            1.0 - inset * 2.0,
            1.0 - inset * 2.0,
            spawn_kind_color(object.kind),
        ));
    }
}

/// Animals (deer tan, wolves grey, brighter while fleeing or hunting) and
/// carcasses (dark red), appended to `output`.
pub(super) fn append_wildlife_instances(
    animals: impl IntoIterator<Item = AnimalView>,
    carcasses: impl IntoIterator<Item = (WorldPosition, u8)>,
    visible: WorldRect,
    scale: f32,
    output: &mut Vec<Instance>,
) {
    if scale < MIN_DYNAMIC_INSTANCE_PIXELS {
        return;
    }
    for (position, _) in carcasses
        .into_iter()
        .filter(|(position, _)| visible.contains(*position))
    {
        output.push(Instance::new(
            position.x as f32 + 0.15,
            position.y as f32 + 0.3,
            0.7,
            0.4,
            CARCASS,
        ));
    }
    for animal in animals
        .into_iter()
        .filter(|animal| visible.contains(animal.position))
    {
        let alert = matches!(animal.mode, AnimalMode::Fleeing | AnimalMode::Hunting);
        let (inset, color) = match (animal.species, alert) {
            (Species::Deer, false) => (0.2, DEER),
            (Species::Deer, true) => (0.2, DEER_ALERT),
            (Species::Wolf, false) => (0.1, WOLF),
            (Species::Wolf, true) => (0.1, WOLF_ALERT),
        };
        output.push(Instance::new(
            animal.position.x as f32 + inset,
            animal.position.y as f32 + inset,
            1.0 - inset * 2.0,
            1.0 - inset * 2.0,
            color,
        ));
    }
}

pub(super) fn build_agent_instances(
    views: impl IntoIterator<Item = AgentView>,
    visible: WorldRect,
    scale: f32,
    output: &mut Vec<Instance>,
) {
    output.clear();
    if scale < MIN_DYNAMIC_INSTANCE_PIXELS {
        return;
    }
    for agent in views.into_iter().take(MAX_AGENT_INSTANCES) {
        if !visible.contains(agent.position) {
            continue;
        }
        let inset = if matches!(agent.activity, AgentActivity::Dead) {
            0.08
        } else {
            0.14
        };
        output.push(Instance::new(
            agent.position.x as f32 + inset,
            agent.position.y as f32 + inset,
            1.0 - inset * 2.0,
            1.0 - inset * 2.0,
            agent_color(agent.activity),
        ));
    }
}

/// Marks a hovered agent's remembered places: first-hand sightings as small solid
/// squares, hints from gestures as hollow squares spanning the expected search area.
pub(super) fn build_memory_marker_instances(
    places: &[LandmarkView],
    scale: f32,
    output: &mut Vec<Instance>,
) {
    output.clear();
    if scale < MIN_DYNAMIC_INSTANCE_PIXELS {
        return;
    }
    let scale = scale.max(f32::EPSILON);
    for place in places.iter().take(LANDMARK_SLOTS) {
        let color = landmark_color(place.kind);
        let center_x = place.position.x as f32 + 0.5;
        let center_y = place.position.y as f32 + 0.5;
        match place.source {
            LandmarkSource::Seen => {
                let size = (MEMORY_MARKER_PIXELS / scale).max(MIN_MEMORY_MARKER_CELLS);
                output.push(Instance::new(
                    center_x - size / 2.0,
                    center_y - size / 2.0,
                    size,
                    size,
                    color,
                ));
            }
            LandmarkSource::Told => {
                let half = f32::from(place.search_radius.max(MIN_HINT_RADIUS_CELLS)) + 0.5;
                let side = half * 2.0;
                let line = (MEMORY_OUTLINE_PIXELS / scale).clamp(0.1, half);
                let x = center_x - half;
                let y = center_y - half;
                output.extend_from_slice(&[
                    Instance::new(x, y, side, line, color),
                    Instance::new(x, y + side - line, side, line, color),
                    Instance::new(x, y, line, side, color),
                    Instance::new(x + side - line, y, line, side, color),
                ]);
            }
        }
    }
}

const MEMORY_MARKER_PIXELS: f32 = 6.0;
const MIN_MEMORY_MARKER_CELLS: f32 = 0.4;
const MEMORY_OUTLINE_PIXELS: f32 = 2.0;
const MIN_HINT_RADIUS_CELLS: u16 = 3;

/// Links a hovered agent to where it last saw each acquaintance: a dotted line
/// from the agent plus a square at the remembered spot, faint for acquaintances
/// and bright (and larger) for friends. Dots keep a minimum screen spacing, so
/// short or far-zoomed lines get fewer of them, never more than
/// `MAX_RELATIONSHIP_DOTS`. Acquaintances with no remembered position are skipped.
pub(super) fn build_relationship_marker_instances(
    origin: WorldPosition,
    acquaintances: &[AcquaintanceView],
    scale: f32,
    output: &mut Vec<Instance>,
) {
    output.clear();
    if scale < MIN_DYNAMIC_INSTANCE_PIXELS {
        return;
    }
    let scale = scale.max(f32::EPSILON);
    let from_x = origin.x as f32 + 0.5;
    let from_y = origin.y as f32 + 0.5;
    let dot = (RELATIONSHIP_DOT_PIXELS / scale).max(MIN_RELATIONSHIP_DOT_CELLS);
    for acquaintance in acquaintances.iter().take(ACQUAINTANCE_SLOTS) {
        let Some(target) = acquaintance.last_seen_position else {
            continue;
        };
        let friend = acquaintance.familiarity >= FRIEND_FAMILIARITY;
        let color = relationship_color(friend);
        let to_x = target.x as f32 + 0.5;
        let to_y = target.y as f32 + 0.5;
        let (dx, dy) = (to_x - from_x, to_y - from_y);
        let screen_length = dx.hypot(dy) * scale;
        let dots = ((screen_length / RELATIONSHIP_DOT_SPACING_PIXELS) as usize)
            .saturating_sub(1)
            .min(MAX_RELATIONSHIP_DOTS);
        for step in 1..=dots {
            let t = step as f32 / (dots + 1) as f32;
            output.push(Instance::new(
                from_x + dx * t - dot / 2.0,
                from_y + dy * t - dot / 2.0,
                dot,
                dot,
                color,
            ));
        }
        let end_pixels = if friend {
            FRIEND_MARKER_PIXELS
        } else {
            ACQUAINTANCE_MARKER_PIXELS
        };
        let end = (end_pixels / scale).max(MIN_MEMORY_MARKER_CELLS);
        output.push(Instance::new(
            to_x - end / 2.0,
            to_y - end / 2.0,
            end,
            end,
            color,
        ));
    }
}

const RELATIONSHIP_DOT_PIXELS: f32 = 2.0;
const MIN_RELATIONSHIP_DOT_CELLS: f32 = 0.2;
const RELATIONSHIP_DOT_SPACING_PIXELS: f32 = 10.0;
const ACQUAINTANCE_MARKER_PIXELS: f32 = 4.0;
const FRIEND_MARKER_PIXELS: f32 = 7.0;

/// Draws recently completed gestures: a dotted line in one neutral color from the
/// sender to where watchers concluded the place is (the public pointing), ending
/// in a small square there.
pub(super) fn build_gesture_instances<'a>(
    gestures: impl IntoIterator<Item = &'a GestureMark>,
    scale: f32,
    output: &mut Vec<Instance>,
) {
    output.clear();
    if scale < MIN_DYNAMIC_INSTANCE_PIXELS {
        return;
    }
    let scale = scale.max(f32::EPSILON);
    let dot = (GESTURE_DOT_PIXELS / scale).max(MIN_RELATIONSHIP_DOT_CELLS);
    let end = (GESTURE_END_PIXELS / scale).max(MIN_MEMORY_MARKER_CELLS);
    for gesture in gestures.into_iter().take(RECENT_GESTURE_CAPACITY) {
        let from_x = gesture.origin.x as f32 + 0.5;
        let from_y = gesture.origin.y as f32 + 0.5;
        let to_x = gesture.inferred_position.x as f32 + 0.5;
        let to_y = gesture.inferred_position.y as f32 + 0.5;
        let (dx, dy) = (to_x - from_x, to_y - from_y);
        let dots = ((dx.hypot(dy) * scale / GESTURE_DOT_SPACING_PIXELS) as usize)
            .saturating_sub(1)
            .min(MAX_GESTURE_DOTS);
        for step in 1..=dots {
            let t = step as f32 / (dots + 1) as f32;
            output.push(Instance::new(
                from_x + dx * t - dot / 2.0,
                from_y + dy * t - dot / 2.0,
                dot,
                dot,
                GESTURE_COLOR,
            ));
        }
        output.push(Instance::new(
            to_x - end / 2.0,
            to_y - end / 2.0,
            end,
            end,
            GESTURE_COLOR,
        ));
    }
}

const GESTURE_DOT_PIXELS: f32 = 3.0;
const GESTURE_DOT_SPACING_PIXELS: f32 = 8.0;
const GESTURE_END_PIXELS: f32 = 5.0;

pub(super) fn build_structure_instances(
    views: impl IntoIterator<Item = StructureView>,
    visible: WorldRect,
    scale: f32,
    output: &mut Vec<Instance>,
) {
    output.clear();
    if scale < MIN_DYNAMIC_INSTANCE_PIXELS {
        return;
    }
    for structure in views.into_iter().take(MAX_STRUCTURE_INSTANCES) {
        if visible.contains(structure.position) {
            output.push(Instance::new(
                structure.position.x as f32 + 0.05,
                structure.position.y as f32 + 0.05,
                0.9,
                0.9,
                structure_color(structure.kind, structure.state),
            ));
        }
    }
}

pub(super) fn build_exact_world_instances(
    world: &World,
    bounds: WorldRect,
) -> (Vec<Instance>, Vec<Instance>) {
    let mut terrain = Vec::new();
    world.visit_cells_in(bounds, |position, cell| {
        terrain.push(Instance::new(
            position.x as f32,
            position.y as f32,
            1.0,
            1.0,
            terrain_color(cell),
        ));
    });
    let mut features = Vec::new();
    world.visit_features_in(bounds, |feature| {
        features.push(Instance::new(
            feature.position.x as f32,
            feature.position.y as f32,
            1.0,
            1.0,
            feature_color(feature.kind),
        ));
    });
    (terrain, features)
}

pub(super) fn rect_instance(bounds: WorldRect, color: u32) -> Instance {
    Instance::new(
        bounds.min.x as f32,
        bounds.min.y as f32,
        (bounds.max.x - bounds.min.x) as f32,
        (bounds.max.y - bounds.min.y) as f32,
        color,
    )
}

pub(super) fn chunk_outline(inspection: ChunkInspection, scale: f32) -> Option<[Instance; 4]> {
    if scale * (CHUNK_SIZE as f32) < MIN_CHUNK_OUTLINE_PIXELS {
        return None;
    }
    let bounds = inspection.bounds;
    let x = bounds.min.x as f32;
    let y = bounds.min.y as f32;
    let width = (bounds.max.x - bounds.min.x) as f32;
    let height = (bounds.max.y - bounds.min.y) as f32;
    let line = (1.0 / scale.max(f32::EPSILON)).clamp(1.0, MAX_CHUNK_OUTLINE_WORLD_WIDTH);
    let color = match inspection.presence {
        ChunkPresence::Missing => rgba(235, 70, 70, 190),
        ChunkPresence::InitialUnloaded | ChunkPresence::PartialInitialUnloaded => {
            rgba(145, 145, 145, 190)
        }
        ChunkPresence::PartialInitial => rgba(255, 205, 55, 190),
        ChunkPresence::Initial => rgba(80, 180, 255, 180),
        ChunkPresence::Retained => rgba(85, 225, 135, 190),
        ChunkPresence::RetainedPartialInitial => rgba(85, 225, 135, 190),
    };
    Some([
        Instance::new(x, y, width, line, color),
        Instance::new(x, y + height - line, width, line, color),
        Instance::new(x, y, line, height, color),
        Instance::new(x + width - line, y, line, height, color),
    ])
}

pub(super) fn world_border(scale: f32) -> [Instance; 4] {
    let bounds = WORLD_GENERATION_BOUNDS;
    let x = bounds.min.x as f32;
    let y = bounds.min.y as f32;
    let width = (bounds.max.x - bounds.min.x) as f32;
    let height = (bounds.max.y - bounds.min.y) as f32;
    let line = (2.0 / scale.max(f32::EPSILON)).clamp(1.0, MAX_WORLD_BORDER_WIDTH);
    let color = rgba(245, 40, 40, 230);
    [
        Instance::new(x, y, width, line, color),
        Instance::new(x, y + height - line, width, line, color),
        Instance::new(x, y, line, height, color),
        Instance::new(x + width - line, y, line, height, color),
    ]
}
