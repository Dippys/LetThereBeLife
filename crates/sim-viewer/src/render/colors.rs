//! Color palette for terrain, features, spawned objects, agents, structures, remembered places, relationships, gestures, and selection previews.

use sim_core::{
    AgentActivity, BiomeType, FeatureKind, LandmarkKind, Material, SpawnKind, StructureKind,
    StructureState, SurfaceType,
};

pub(super) const HUT: u32 = rgba(116, 72, 38, 255);
pub(super) const HEARTH: u32 = rgba(255, 120, 24, 255);
pub(super) const COLD_HEARTH: u32 = rgba(90, 84, 80, 255);
pub(super) const UNDER_CONSTRUCTION: u32 = rgba(224, 170, 72, 230);
pub(super) const DEER: u32 = rgba(176, 128, 72, 255);
pub(super) const DEER_ALERT: u32 = rgba(222, 170, 96, 255);
pub(super) const WOLF: u32 = rgba(132, 132, 140, 255);
pub(super) const WOLF_ALERT: u32 = rgba(200, 200, 214, 255);
pub(super) const CARCASS: u32 = rgba(110, 24, 24, 255);
pub(super) const BABY: u32 = rgba(255, 214, 230, 255);
pub(super) const ELDER_EDGE: u32 = rgba(170, 170, 176, 255);
pub(super) const PARTNER: u32 = rgba(255, 120, 170, 235);
pub(super) const WINTER_TINT: u32 = rgba(225, 238, 255, 70);
pub(super) const AUTUMN_TINT: u32 = rgba(255, 160, 60, 26);

/// Interface palette: dark translucent panels, light text, a few signal colors.
pub(super) const UI_BAR: u32 = rgba(16, 20, 26, 236);
pub(super) const UI_PANEL: u32 = rgba(18, 23, 30, 250);
pub(super) const UI_TOOLTIP: u32 = rgba(28, 34, 44, 245);
pub(super) const UI_TOAST: u32 = rgba(52, 44, 22, 240);
pub(super) const UI_SCRIM: u32 = rgba(0, 0, 0, 150);
pub(super) const UI_BORDER: u32 = rgba(255, 255, 255, 30);
pub(super) const UI_BUTTON: u32 = rgba(44, 52, 64, 255);
pub(super) const UI_BUTTON_HOVER: u32 = rgba(62, 74, 90, 255);
pub(super) const UI_BUTTON_ACTIVE: u32 = rgba(40, 104, 150, 255);
pub(super) const UI_TRACK: u32 = rgba(48, 56, 68, 255);
/// The year strip: each season's months, gone by and still to come.
pub(super) const SEASON_PAST: [u32; 4] = [
    rgba(110, 190, 90, 255),
    rgba(220, 196, 70, 255),
    rgba(214, 120, 50, 255),
    rgba(170, 200, 230, 255),
];
pub(super) const SEASON_AHEAD: [u32; 4] = [
    rgba(52, 76, 56, 255),
    rgba(80, 76, 50, 255),
    rgba(80, 60, 48, 255),
    rgba(62, 72, 86, 255),
];
pub(super) const UI_TITLE: u32 = rgba(255, 255, 255, 255);
pub(super) const UI_TEXT: u32 = rgba(226, 230, 236, 255);
pub(super) const UI_DIM: u32 = rgba(140, 150, 164, 255);
pub(super) const UI_ACCENT: u32 = rgba(110, 190, 250, 255);
pub(super) const UI_NEED: u32 = rgba(120, 170, 220, 255);
pub(super) const UI_GOOD: u32 = rgba(110, 200, 120, 255);
pub(super) const UI_WARN: u32 = rgba(240, 180, 70, 255);
pub(super) const UI_BAD: u32 = rgba(236, 92, 80, 255);
pub(super) const UI_TALK: u32 = rgba(190, 140, 250, 255);
pub(super) const UI_BUBBLE: u32 = rgba(250, 248, 236, 240);
pub(super) const UI_BUBBLE_LOUD: u32 = rgba(255, 196, 120, 245);

pub(super) const fn structure_color(kind: StructureKind, state: StructureState) -> u32 {
    match (kind, state) {
        (_, StructureState::UnderConstruction) => UNDER_CONSTRUCTION,
        (StructureKind::Shelter, StructureState::Complete) => HUT,
        (StructureKind::Hearth, StructureState::Complete) => HEARTH,
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
        LandmarkKind::Material(material) => material_color(material),
        LandmarkKind::Structure(StructureKind::Shelter) => rgba(255, 150, 40, 235),
        LandmarkKind::Structure(StructureKind::Hearth) => rgba(255, 90, 30, 235),
    }
}

/// A material's marker color.
pub(super) const fn material_color(material: Material) -> u32 {
    match material {
        Material::Berries => rgba(240, 84, 136, 235),
        Material::Bitterberries => rgba(150, 90, 230, 235),
        Material::Wood => rgba(164, 104, 52, 235),
        Material::Stone => rgba(176, 176, 170, 235),
        Material::Meat => rgba(150, 40, 40, 235),
        Material::Blade => rgba(200, 210, 220, 235),
        Material::Hide => rgba(140, 100, 60, 235),
    }
}

/// The public part of a gesture (pointing line and search square): one neutral color.
pub(super) const GESTURE_COLOR: u32 = rgba(255, 252, 236, 220);

/// Relationship lines and last-seen markers: faint for acquaintances, strong for friends.
pub(super) const fn relationship_color(friend: bool) -> u32 {
    if friend {
        rgba(214, 150, 255, 235)
    } else {
        rgba(214, 206, 236, 110)
    }
}

pub(super) const fn feature_color(kind: FeatureKind) -> u32 {
    match kind {
        FeatureKind::Tree => rgba(24, 72, 28, 255),
        FeatureKind::Rock => rgba(118, 116, 108, 255),
        FeatureKind::BerryBush => rgba(112, 42, 74, 255),
        FeatureKind::BitterBush => rgba(78, 48, 104, 255),
    }
}

pub(super) const fn spawn_kind_color(kind: SpawnKind) -> u32 {
    match kind {
        SpawnKind::Tree => feature_color(FeatureKind::Tree),
        SpawnKind::BerryBush => feature_color(FeatureKind::BerryBush),
        SpawnKind::BitterBush => feature_color(FeatureKind::BitterBush),
        SpawnKind::Rock => feature_color(FeatureKind::Rock),
        SpawnKind::Water => rgba(45, 132, 202, 235),
    }
}

pub(super) const fn summary_feature_color(kind: FeatureKind) -> u32 {
    match kind {
        FeatureKind::Tree => rgba(24, 72, 28, 230),
        FeatureKind::Rock => rgba(118, 116, 108, 230),
        FeatureKind::BerryBush => rgba(112, 42, 74, 230),
        FeatureKind::BitterBush => rgba(78, 48, 104, 230),
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

/// A floating window's title bar.
pub(super) const UI_WINDOW_BAR: u32 = rgba(44, 52, 66, 255);
/// A watcher's reading of a gesture: as meant, and mistaken.
pub(super) const UI_READING: u32 = rgba(200, 228, 196, 230);
pub(super) const UI_READING_WRONG: u32 = rgba(200, 60, 60, 235);
/// Behind a sign over someone's head, so it reads on any ground.
pub(super) const UI_SIGN_BACK: u32 = rgba(16, 18, 22, 190);
/// Signs over people: asleep, building, by a fire, at water, eating,
/// carrying or gathering, hunting, running from danger.
pub(super) const SIGN_SLEEP: u32 = rgba(170, 196, 255, 255);
pub(super) const SIGN_BUILD: u32 = rgba(222, 196, 140, 255);
pub(super) const SIGN_FIRE: u32 = rgba(255, 150, 50, 255);
pub(super) const SIGN_WATER: u32 = rgba(90, 170, 255, 255);
pub(super) const SIGN_FOOD: u32 = rgba(235, 80, 120, 255);
pub(super) const SIGN_WORK: u32 = rgba(196, 150, 100, 255);
pub(super) const SIGN_HUNT: u32 = rgba(240, 210, 90, 255);
pub(super) const SIGN_DANGER: u32 = rgba(255, 80, 70, 255);

/// Ground worn by walking: darker the more it's trodden.
pub(super) fn worn(wear: u16) -> u32 {
    let alpha = (60 + u32::from(wear.min(300)) / 2).min(170) as u8;
    rgba(58, 40, 22, alpha)
}

/// The season's light laid over the land (none in spring).
pub(super) const fn season_tint(season: sim_core::Season) -> Option<u32> {
    match season {
        sim_core::Season::Spring => None,
        sim_core::Season::Summer => Some(rgba(255, 236, 160, 22)),
        sim_core::Season::Autumn => Some(rgba(230, 130, 50, 30)),
        sim_core::Season::Winter => Some(rgba(205, 225, 255, 52)),
    }
}

/// One ring of a fire's glow (rings stack toward the middle); brighter in the dark.
pub(super) fn firelight(darkness: f32) -> u32 {
    rgba(255, 160, 60, (34.0 + 36.0 * darkness.clamp(0.0, 1.0)) as u8)
}

/// Night over the land, as dark as `darkness` (0-1).
pub(super) fn night(darkness: f32) -> u32 {
    rgba(6, 10, 34, (160.0 * darkness.clamp(0.0, 1.0)) as u8)
}

/// Family lines: the two founding families, then newcomer couples.
pub(super) const fn family_line(line: u8) -> u32 {
    match line % 6 {
        0 => rgba(80, 150, 255, 255),
        1 => rgba(255, 196, 60, 255),
        2 => rgba(120, 220, 120, 255),
        3 => rgba(240, 110, 200, 255),
        4 => rgba(90, 220, 220, 255),
        _ => rgba(240, 120, 80, 255),
    }
}

/// Falling snow, and autumn leaves.
pub(super) const SNOW: u32 = rgba(245, 248, 255, 220);
pub(super) const LEAVES: [u32; 3] = [
    rgba(214, 120, 40, 230),
    rgba(190, 70, 40, 230),
    rgba(220, 180, 60, 230),
];

/// Land on the small map in winter: lightened toward snow.
pub(super) fn snowy(color: u32) -> u32 {
    let lift = |shift: u32| {
        let channel = (color >> shift) & 0xff;
        (channel + (235 - channel.min(235)) / 2) << shift
    };
    lift(0) | lift(8) | lift(16) | (color & 0xff00_0000)
}
/// A burning fire on the small map.
pub(super) const MINIMAP_FIRE: u32 = rgba(255, 150, 40, 255);
/// Behind the start screen.
pub(super) const UI_DIM_SCREEN: u32 = rgba(8, 10, 14, 200);
