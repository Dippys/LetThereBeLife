//! Color palette for terrain, features, spawned objects, agents, structures, remembered places, and selection previews.

use sim_core::{
    AgentActivity, BiomeType, FeatureKind, LandmarkKind, SpawnKind, StructureState, SurfaceType,
};

pub(super) const fn structure_color(state: StructureState) -> u32 {
    match state {
        StructureState::UnderConstruction => rgba(224, 170, 72, 230),
        StructureState::Complete => rgba(116, 72, 38, 255),
    }
}

pub(super) const fn agent_color(activity: AgentActivity) -> u32 {
    match activity {
        AgentActivity::Idle => rgba(244, 238, 210, 255),
        AgentActivity::Moving => rgba(72, 232, 126, 255),
        AgentActivity::Gathering => rgba(250, 206, 74, 255),
        AgentActivity::Building => rgba(240, 142, 62, 255),
        AgentActivity::Sleeping => rgba(92, 164, 246, 255),
        AgentActivity::Incapacitated => rgba(180, 72, 214, 255),
        AgentActivity::Dead => rgba(118, 28, 32, 255),
    }
}

/// Bright marker colors for a hovered agent's remembered places; they must read over terrain.
pub(super) const fn landmark_color(kind: LandmarkKind) -> u32 {
    match kind {
        LandmarkKind::Water => rgba(70, 176, 255, 235),
        LandmarkKind::Food => rgba(240, 84, 136, 235),
        LandmarkKind::Wood => rgba(164, 104, 52, 235),
        LandmarkKind::Stone => rgba(176, 176, 170, 235),
        LandmarkKind::Shelter => rgba(255, 150, 40, 235),
    }
}

pub(super) const fn feature_color(kind: FeatureKind) -> u32 {
    match kind {
        FeatureKind::Tree => rgba(24, 72, 28, 255),
        FeatureKind::Rock => rgba(118, 116, 108, 255),
        FeatureKind::BerryBush => rgba(112, 42, 74, 255),
    }
}

pub(super) const fn spawn_kind_color(kind: SpawnKind) -> u32 {
    match kind {
        SpawnKind::Tree => feature_color(FeatureKind::Tree),
        SpawnKind::BerryBush => feature_color(FeatureKind::BerryBush),
        SpawnKind::Rock => feature_color(FeatureKind::Rock),
        SpawnKind::Water => rgba(45, 132, 202, 235),
    }
}

pub(super) const fn summary_feature_color(kind: FeatureKind) -> u32 {
    match kind {
        FeatureKind::Tree => rgba(24, 72, 28, 230),
        FeatureKind::Rock => rgba(118, 116, 108, 230),
        FeatureKind::BerryBush => rgba(112, 42, 74, 230),
    }
}

pub(super) fn terrain_color(cell: sim_core::TerrainCell) -> u32 {
    let shade = (cell.elevation >> 12) as u8;
    match (cell.surface(), cell.biome()) {
        (SurfaceType::DeepWater, BiomeType::Ocean) => rgba(16, 48 + shade, 94 + shade, 255),
        (SurfaceType::ShallowWater, BiomeType::Ocean) => rgba(28, 84 + shade, 126 + shade, 255),
        (SurfaceType::DeepWater, BiomeType::Lake) => rgba(24, 66 + shade, 112 + shade, 255),
        (SurfaceType::ShallowWater, BiomeType::Lake) => rgba(40, 102 + shade, 142 + shade, 255),
        (SurfaceType::DeepWater, BiomeType::River) => rgba(20, 74 + shade, 128 + shade, 255),
        (SurfaceType::ShallowWater, BiomeType::River) => rgba(38, 116 + shade, 154 + shade, 255),
        (SurfaceType::Sand, BiomeType::Beach) => rgba(210 + shade, 190 + shade, 126, 255),
        (SurfaceType::Sand, BiomeType::Desert) => rgba(184 + shade, 150 + shade, 75, 255),
        (SurfaceType::Soil, BiomeType::Grassland) => rgba(50 + shade, 112 + shade, 51, 255),
        (SurfaceType::Soil, BiomeType::Savanna) => rgba(118 + shade, 126 + shade, 55, 255),
        (SurfaceType::Soil, BiomeType::Forest) => rgba(37, 86 + shade, 39, 255),
        (SurfaceType::Soil, BiomeType::Wetland) => rgba(48, 94 + shade, 74 + shade, 255),
        (SurfaceType::Soil, BiomeType::Tundra) => rgba(105 + shade, 119 + shade, 105 + shade, 255),
        (SurfaceType::Hill, _) => rgba(100 + shade, 108 + shade, 72, 255),
        (SurfaceType::Rock, _) => rgba(125 + shade, 124 + shade, 119 + shade, 255),
        (SurfaceType::SnowIce, _) => rgba(220 + shade, 229 + shade, 234 + shade, 255),
        _ => rgba(255, 0, 255, 255),
    }
}

pub(super) const fn selection_color(valid: bool) -> u32 {
    if valid {
        rgba(255, 220, 35, 72)
    } else {
        rgba(235, 48, 48, 96)
    }
}

pub(super) const fn rgba(red: u8, green: u8, blue: u8, alpha: u8) -> u32 {
    red as u32 | (green as u32) << 8 | (blue as u32) << 16 | (alpha as u32) << 24
}
