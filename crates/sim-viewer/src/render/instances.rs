//! Instance builders for agents, structures, spawned objects, exact world cells, chunk outlines, and the world border.

use sim_core::{
    AgentActivity, AgentView, CHUNK_SIZE, ChunkInspection, ChunkPresence, SpawnKind,
    SpawnedObjectView, StructureView, WORLD_GENERATION_BOUNDS, World, WorldRect,
};

use super::{
    MAX_AGENT_INSTANCES, MAX_CHUNK_OUTLINE_WORLD_WIDTH, MAX_SPAWNED_OBJECT_INSTANCES,
    MAX_STRUCTURE_INSTANCES, MAX_WORLD_BORDER_WIDTH, MIN_CHUNK_OUTLINE_PIXELS,
    MIN_DYNAMIC_INSTANCE_PIXELS,
    colors::{agent_color, feature_color, rgba, spawn_kind_color, structure_color, terrain_color},
    gpu::Instance,
};

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
                structure_color(structure.state),
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
