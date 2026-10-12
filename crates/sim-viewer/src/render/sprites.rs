//! Tiny pixel pictures drawn inside a cell when zoomed in close enough to see
//! them: people, animals, huts and fires, plants, rocks, and a little texture
//! on the ground. Each is an 8x8 grid of dots; letters pick colors from a
//! palette, '.' is empty. Drawing only: nothing here touches the simulation.

use sim_core::{AgentActivity, FeatureKind, LifeStage, Sex, Species, TerrainCell};

use super::{colors::rgba, gpu::Instance};

/// Dots across a sprite.
const DOTS: usize = 8;
/// Cells must be at least this many screen pixels wide for sprites.
pub(super) const SPRITE_CELL_PIXELS: f32 = 10.0;

type Picture = [&'static str; DOTS];

/// Draws `picture` filling the cell at (`x`, `y`) scaled by `size` (1 fills
/// the cell, smaller is centered at the bottom), with `palette` mapping letters
/// to colors.
fn draw(
    out: &mut Vec<Instance>,
    x: f32,
    y: f32,
    size: f32,
    picture: &Picture,
    palette: &dyn Fn(u8) -> Option<u32>,
) {
    let dot = size / DOTS as f32;
    let left = x + (1.0 - size) / 2.0;
    let top = y + (1.0 - size);
    for (row, line) in picture.iter().enumerate() {
        let bytes = line.as_bytes();
        let mut column = 0;
        while column < bytes.len() {
            let Some(color) = palette(bytes[column]) else {
                column += 1;
                continue;
            };
            // Runs of one color become one rectangle.
            let start = column;
            while column < bytes.len() && bytes[column] == bytes[start] {
                column += 1;
            }
            out.push(Instance::new(
                left + start as f32 * dot,
                top + row as f32 * dot,
                (column - start) as f32 * dot,
                dot,
                color,
            ));
        }
    }
}

const SKIN: u32 = rgba(224, 182, 140, 255);
const HAIR: u32 = rgba(70, 48, 32, 255);
const GREY_HAIR: u32 = rgba(200, 200, 205, 255);
const LEGS: u32 = rgba(60, 50, 44, 255);

/// How a person looks this frame.
pub(super) struct PersonLook {
    /// Their clothes take the color of what they're doing.
    pub(super) body: u32,
    /// Their family line's color, as a band at the waist (if known).
    pub(super) family: Option<u32>,
    pub(super) sex: Option<Sex>,
    pub(super) stage: Option<LifeStage>,
    pub(super) activity: AgentActivity,
    /// Alternates while walking, for a stepping gait.
    pub(super) step: bool,
}

const PERSON_A: Picture = [
    "..hhh...", "..sss...", "..sss...", ".bbbbb..", ".bfffb..", "..bbb...", "..l.l...", "..l.l...",
];
const PERSON_B: Picture = [
    "..hhh...", "..sss...", "..sss...", ".bbbbb..", ".bfffb..", "..bbb...", "..ll....", "...l....",
];
const WOMAN_A: Picture = [
    ".hhhh...", ".hsss...", "..sss...", ".bbbbb..", ".bfffb..", ".bbbbb..", "bbbbbbb.", "..l.l...",
];
const WOMAN_B: Picture = [
    ".hhhh...", ".hsss...", "..sss...", ".bbbbb..", ".bfffb..", ".bbbbb..", "bbbbbbb.", "...ll...",
];
const ASLEEP: Picture = [
    "........", "........", "........", "........", "........", "ss.bbbbb", "hsbbffbl", "........",
];
const DEAD: Picture = [
    "........", "........", "........", "........", "........", "........", "sbbbbbll", "........",
];

pub(super) fn person(out: &mut Vec<Instance>, x: f32, y: f32, look: &PersonLook) {
    let picture = match look.activity {
        AgentActivity::Dead => &DEAD,
        AgentActivity::Sleeping | AgentActivity::Incapacitated => &ASLEEP,
        _ => match (look.sex == Some(Sex::Female), look.step) {
            (true, false) => &WOMAN_A,
            (true, true) => &WOMAN_B,
            (false, false) => &PERSON_A,
            (false, true) => &PERSON_B,
        },
    };
    let size = match look.stage {
        Some(LifeStage::Baby) => 0.45,
        Some(LifeStage::Child) => 0.7,
        _ => 1.0,
    };
    let hair = if look.stage == Some(LifeStage::Elder) {
        GREY_HAIR
    } else {
        HAIR
    };
    let body = look.body;
    let family = look.family.unwrap_or(body);
    draw(out, x, y, size, picture, &|letter| match letter {
        b'h' => Some(hair),
        b's' => Some(SKIN),
        b'b' => Some(body),
        b'f' => Some(family),
        b'l' => Some(LEGS),
        _ => None,
    });
}

const DEER: Picture = [
    "......a.", ".....aa.", ".....tt.", "tttttttt", "tttttttw", ".tttttt.", ".l.l.l.l", ".l.l.l.l",
];
const WOLF: Picture = [
    "......e.", ".....ggg", "g...gggn", ".ggggggg", ".gggggg.", ".gggggg.", ".g.g.g.g", ".g.g.g.g",
];

pub(super) fn animal(out: &mut Vec<Instance>, x: f32, y: f32, species: Species, alert: bool) {
    match species {
        Species::Deer => {
            let coat = if alert {
                rgba(216, 160, 96, 255)
            } else {
                rgba(170, 120, 70, 255)
            };
            draw(out, x, y, 0.95, &DEER, &|letter| match letter {
                b't' => Some(coat),
                b'a' => Some(rgba(120, 90, 60, 255)),
                b'w' => Some(rgba(240, 236, 226, 255)),
                b'l' => Some(rgba(90, 64, 40, 255)),
                _ => None,
            });
        }
        Species::Wolf => {
            let coat = if alert {
                rgba(150, 150, 160, 255)
            } else {
                rgba(110, 110, 118, 255)
            };
            draw(out, x, y, 0.95, &WOLF, &|letter| match letter {
                b'g' => Some(coat),
                b'e' => Some(rgba(80, 80, 88, 255)),
                b'n' => Some(rgba(30, 30, 34, 255)),
                _ => None,
            });
        }
    }
}

const HUT: Picture = [
    "...rr...", "..rrrr..", ".rrrrrr.", "rrrrrrrr", ".wwwwww.", ".wwddww.", ".wwddww.", ".wwddww.",
];
const HUT_FRAME: Picture = [
    "...rr...", "..r..r..", ".r....r.", "r......r", ".w....w.", ".w....w.", ".w....w.", ".wwwwww.",
];
const HEARTH: Picture = [
    "........", "...y....", "..yoy...", "..ooo...", ".oocco..", ".scccs..", "sswwwss.", ".sssss..",
];
const COLD_HEARTH: Picture = [
    "........", "........", "........", "........", "........", ".s...s..", "sswwwss.", ".sssss..",
];

/// A hut (finished or being built), or a hearth, burning or gone out.
/// `flicker` (0-3) shifts a burning fire's colors from frame to frame.
pub(super) fn structure(
    out: &mut Vec<Instance>,
    x: f32,
    y: f32,
    hearth: bool,
    finished: bool,
    burning: bool,
    flicker: u8,
) {
    let (picture, flames) = match (hearth, finished, burning) {
        (false, true, _) => (&HUT, false),
        (false, false, _) => (&HUT_FRAME, false),
        (true, _, true) => (&HEARTH, true),
        (true, _, false) => (&COLD_HEARTH, false),
    };
    let flame = |bright: u8| -> u32 {
        let lift = flicker * 18;
        rgba(255, bright.saturating_add(lift / 2), 40 + lift, 255)
    };
    draw(out, x, y, 1.0, picture, &|letter| match letter {
        b'r' => Some(rgba(150, 110, 60, 255)),
        b'w' => Some(if hearth {
            rgba(60, 40, 30, 255)
        } else {
            rgba(120, 84, 50, 255)
        }),
        b'd' => Some(rgba(50, 34, 24, 255)),
        b's' => Some(rgba(130, 128, 120, 255)),
        b'y' if flames => Some(flame(220)),
        b'o' if flames => Some(flame(150)),
        b'c' if flames => Some(flame(80)),
        _ => None,
    });
}

const TREE: Picture = [
    "..ggg...", ".ggggg..", "gggGggg.", "ggGgggg.", ".ggggg..", "...t....", "...t....", "..ttt...",
];
const SAPLING: Picture = [
    "........", "........", "........", "...g....", "..ggg...", "...t....", "...t....", "........",
];
const BUSH: Picture = [
    "........", "........", "..ggg...", ".gBgBg..", "gggBggg.", ".gBggBg.", "..ggg...", "........",
];
const BARE_BUSH: Picture = [
    "........", "........", "..g.g...", ".g.g.g..", "..ggg...", ".g.g.g..", "...g....", "........",
];
const ROCK: Picture = [
    "........", "........", "...rr...", "..rRrr..", ".rrRrrr.", ".rrrrrr.", "..rrrr..", "........",
];

/// A plant or rock: `bearing` is whether a bush has berries (or a tree is
/// grown), so empty bushes look bare and saplings small.
pub(super) fn feature(
    out: &mut Vec<Instance>,
    x: f32,
    y: f32,
    kind: FeatureKind,
    bearing: bool,
    winter: bool,
) {
    let leaf = if winter {
        rgba(70, 96, 70, 255)
    } else {
        rgba(40, 110, 46, 255)
    };
    let dark = rgba(26, 76, 32, 255);
    let (picture, berry) = match (kind, bearing) {
        (FeatureKind::Tree, true) => (&TREE, leaf),
        (FeatureKind::Tree, false) => (&SAPLING, leaf),
        (FeatureKind::BerryBush, true) => (&BUSH, rgba(200, 40, 70, 255)),
        (FeatureKind::BitterBush, true) => (&BUSH, rgba(120, 60, 170, 255)),
        (FeatureKind::BerryBush | FeatureKind::BitterBush, false) => (&BARE_BUSH, leaf),
        (FeatureKind::Rock, _) => (&ROCK, leaf),
    };
    draw(out, x, y, 1.0, picture, &|letter| match letter {
        b'g' => Some(leaf),
        b'G' => Some(dark),
        b't' => Some(rgba(96, 66, 40, 255)),
        b'B' => Some(berry),
        b'r' => Some(rgba(128, 126, 118, 255)),
        b'R' => Some(rgba(160, 158, 150, 255)),
        _ => None,
    });
}

/// A little texture on the ground: a couple of specks per cell, a shade off
/// the ground's color, placed by the cell's position (so they stay put);
/// water gets a ripple that drifts with `wave`.
pub(super) fn ground(
    out: &mut Vec<Instance>,
    x: i64,
    y: i64,
    cell: TerrainCell,
    base: u32,
    wave: u32,
) {
    let hash = {
        let mut key = (x as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15)
            ^ (y as u64).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        key ^= key >> 29;
        key.wrapping_mul(0x94d0_49bb_1331_11eb)
    };
    let shade = |color: u32, lighter: bool| -> u32 {
        let [r, g, b, a] = color.to_le_bytes();
        let shift = |channel: u8| {
            if lighter {
                channel.saturating_add(14)
            } else {
                channel.saturating_sub(16)
            }
        };
        u32::from_le_bytes([shift(r), shift(g), shift(b), a])
    };
    let (fx, fy) = (x as f32, y as f32);
    if cell.water().is_some() {
        // A short ripple line that slides across the cell over time.
        let phase = ((hash >> 8) as u32).wrapping_add(wave) % 8;
        out.push(Instance::new(
            fx + phase as f32 / 8.0 * 0.6,
            fy + ((hash >> 16) % 6) as f32 / 8.0 + 0.1,
            0.3,
            0.08,
            shade(base, true),
        ));
        return;
    }
    for speck in 0..2 {
        let bits = hash >> (speck * 20);
        out.push(Instance::new(
            fx + (bits % 7) as f32 / 8.0,
            fy + ((bits >> 4) % 7) as f32 / 8.0,
            0.125,
            0.125,
            shade(base, bits & 0x100 != 0),
        ));
    }
}
