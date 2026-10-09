//! Screen-space overlay layout: HUD panels, spawn menu, agent panel, and the bitmap font.

use sim_core::SpawnKind;

use super::{
    GenerationStatus, RenderState, SpawnMenuView,
    colors::{rgba, spawn_kind_color},
    gpu::Instance,
};

pub(super) fn build_screen_overlay(
    instances: &mut Vec<Instance>,
    text: &str,
    agent_text: Option<&str>,
    state: &RenderState,
    width: u32,
    height: u32,
) {
    instances.clear();
    let scale = state.ui_scale.clamp(1.0, 3.0);
    let pixel = 2.0 * scale;
    let advance = 6.0 * pixel;
    let line_height = 9.0 * pixel;
    let margin = 14.0 * scale;
    let text_x = margin + 14.0 * scale;
    let text_y = margin + 10.0 * scale;
    let line_count = text.lines().count().max(1);
    let longest_line = text.lines().map(str::len).max().unwrap_or(1) as f32;
    let panel_width = longest_line * advance + 28.0 * scale;
    let panel_height = line_count as f32 * line_height + 20.0 * scale;

    instances.push(Instance::new(
        margin + 3.0 * scale,
        margin + 3.0 * scale,
        panel_width,
        panel_height,
        rgba(0, 0, 0, 105),
    ));
    instances.push(Instance::new(
        margin,
        margin,
        panel_width,
        panel_height,
        rgba(8, 15, 20, 232),
    ));
    instances.push(Instance::new(
        margin,
        margin,
        4.0 * scale,
        panel_height,
        rgba(71, 190, 194, 255),
    ));
    instances.push(Instance::new(
        text_x,
        text_y + line_height - 3.0 * scale,
        panel_width - 28.0 * scale,
        scale,
        rgba(71, 190, 194, 100),
    ));
    instances.push(Instance::new(
        text_x,
        text_y + 5.0 * line_height - 3.0 * scale,
        panel_width - 28.0 * scale,
        scale,
        rgba(120, 145, 150, 75),
    ));

    for (line_index, line) in text.lines().enumerate() {
        let color = match line_index {
            0 => rgba(151, 232, 229, 255),
            1 if state.snapshot.paused => rgba(240, 183, 78, 255),
            1 => rgba(100, 220, 145, 255),
            4 if state.generation_status == GenerationStatus::WorkerUnavailable => {
                rgba(245, 96, 86, 255)
            }
            4 if state.generation_status != GenerationStatus::Idle => rgba(236, 196, 84, 255),
            _ => rgba(218, 229, 226, 255),
        };
        push_bitmap_text(
            instances,
            line,
            text_x,
            text_y + line_index as f32 * line_height,
            pixel,
            color,
        );
    }

    if let Some(agent_text) = agent_text {
        push_agent_panel(instances, agent_text, width as f32, margin, scale);
    }
    if let Some(menu) = state.spawn_menu {
        push_spawn_menu(instances, menu, height as f32, margin, scale);
    }

    let rail_margin = 28.0 * scale;
    let square = 14.0 * scale;
    let rail_width = (width as f32 - rail_margin * 2.0).max(square);
    let rail_y = height as f32 - 19.0 * scale;
    let cycle = (state.snapshot.simulated_seconds.max(0.0) % 60.0) as f32 / 60.0;
    let travel = (rail_width - square).max(0.0);
    let marker_x = rail_margin + travel * cycle;
    instances.push(Instance::new(
        rail_margin,
        rail_y - scale,
        rail_width,
        2.0 * scale,
        rgba(224, 220, 191, 70),
    ));
    instances.push(Instance::new(
        rail_margin,
        rail_y - scale,
        travel * cycle + square * 0.5,
        2.0 * scale,
        rgba(235, 216, 130, 155),
    ));
    instances.push(Instance::new(
        marker_x,
        rail_y - square * 0.5,
        square,
        square,
        rgba(235, 216, 130, 255),
    ));
}

fn push_spawn_menu(
    instances: &mut Vec<Instance>,
    menu: SpawnMenuView,
    screen_height: f32,
    margin: f32,
    scale: f32,
) {
    let pixel = 2.0 * scale;
    let line_height = 9.0 * pixel;
    let panel_width = 180.0 * scale;
    let panel_height = 7.0 * line_height + 18.0 * scale;
    let x = margin;
    let y = (screen_height - panel_height - 38.0 * scale).max(margin);
    instances.push(Instance::new(
        x + 3.0 * scale,
        y + 3.0 * scale,
        panel_width,
        panel_height,
        rgba(0, 0, 0, 105),
    ));
    instances.push(Instance::new(
        x,
        y,
        panel_width,
        panel_height,
        rgba(8, 15, 20, 240),
    ));
    instances.push(Instance::new(
        x,
        y,
        4.0 * scale,
        panel_height,
        rgba(235, 216, 130, 255),
    ));
    push_bitmap_text(
        instances,
        if menu.placing {
            "PLACE MODE"
        } else {
            "SPAWN MENU"
        },
        x + 14.0 * scale,
        y + 9.0 * scale,
        pixel,
        rgba(245, 226, 145, 255),
    );
    for (index, kind) in SpawnKind::ALL.into_iter().enumerate() {
        let row_y = y + 9.0 * scale + (index as f32 + 1.5) * line_height;
        let selected = kind == menu.selected;
        if selected {
            instances.push(Instance::new(
                x + 9.0 * scale,
                row_y - 2.0 * scale,
                panel_width - 18.0 * scale,
                line_height,
                spawn_kind_color(kind) & 0x7fff_ffff,
            ));
        }
        push_bitmap_text(
            instances,
            spawn_kind_label(kind),
            x + 18.0 * scale,
            row_y,
            pixel,
            if selected {
                rgba(255, 255, 255, 255)
            } else {
                rgba(185, 199, 198, 255)
            },
        );
    }
    push_bitmap_text(
        instances,
        if menu.placing {
            "L CLICK PLACE  5 MENU  0 END"
        } else {
            "2/8 SELECT  5 PLACE  0 CLOSE"
        },
        x + 14.0 * scale,
        y + panel_height - line_height - 5.0 * scale,
        pixel,
        rgba(218, 229, 226, 255),
    );
}

pub(super) const fn spawn_kind_label(kind: SpawnKind) -> &'static str {
    match kind {
        SpawnKind::Tree => "TREE",
        SpawnKind::BerryBush => "BERRIES",
        SpawnKind::Rock => "ROCK",
        SpawnKind::Water => "WATER",
    }
}

fn push_agent_panel(
    instances: &mut Vec<Instance>,
    text: &str,
    screen_width: f32,
    margin: f32,
    scale: f32,
) {
    let pixel = 2.0 * scale;
    let advance = 6.0 * pixel;
    let line_height = 9.0 * pixel;
    let line_count = text.lines().count().max(1);
    let longest_line = text.lines().map(str::len).max().unwrap_or(1) as f32;
    let panel_width = longest_line * advance + 28.0 * scale;
    let panel_height = line_count as f32 * line_height + 20.0 * scale;
    let panel_x = (screen_width - margin - panel_width).max(margin);
    let text_x = panel_x + 14.0 * scale;
    let text_y = margin + 10.0 * scale;
    instances.push(Instance::new(
        panel_x + 3.0 * scale,
        margin + 3.0 * scale,
        panel_width,
        panel_height,
        rgba(0, 0, 0, 105),
    ));
    instances.push(Instance::new(
        panel_x,
        margin,
        panel_width,
        panel_height,
        rgba(8, 15, 20, 232),
    ));
    instances.push(Instance::new(
        panel_x + panel_width - 4.0 * scale,
        margin,
        4.0 * scale,
        panel_height,
        rgba(71, 190, 194, 255),
    ));
    instances.push(Instance::new(
        text_x,
        text_y + line_height - 3.0 * scale,
        panel_width - 28.0 * scale,
        scale,
        rgba(71, 190, 194, 100),
    ));
    for (line_index, line) in text.lines().enumerate() {
        push_bitmap_text(
            instances,
            line,
            text_x,
            text_y + line_index as f32 * line_height,
            pixel,
            if line_index == 0 {
                rgba(151, 232, 229, 255)
            } else {
                rgba(218, 229, 226, 255)
            },
        );
    }
}

fn push_bitmap_text(
    instances: &mut Vec<Instance>,
    text: &str,
    x: f32,
    y: f32,
    pixel: f32,
    color: u32,
) {
    for (character_index, character) in text.chars().enumerate() {
        let glyph = glyph_rows(character);
        let glyph_x = x + character_index as f32 * pixel * 6.0;
        for (row_index, row) in glyph.into_iter().enumerate() {
            let mut column = 0;
            while column < 5 {
                if row & (1 << (4 - column)) == 0 {
                    column += 1;
                    continue;
                }
                let start = column;
                while column < 5 && row & (1 << (4 - column)) != 0 {
                    column += 1;
                }
                instances.push(Instance::new(
                    glyph_x + start as f32 * pixel,
                    y + row_index as f32 * pixel,
                    (column - start) as f32 * pixel,
                    pixel,
                    color,
                ));
            }
        }
    }
}

const fn glyph_rows(character: char) -> [u8; 7] {
    match character {
        'A' => [14, 17, 17, 31, 17, 17, 17],
        'B' => [30, 17, 17, 30, 17, 17, 30],
        'C' => [14, 17, 16, 16, 16, 17, 14],
        'D' => [30, 17, 17, 17, 17, 17, 30],
        'E' => [31, 16, 16, 30, 16, 16, 31],
        'F' => [31, 16, 16, 30, 16, 16, 16],
        'G' => [14, 17, 16, 23, 17, 17, 15],
        'H' => [17, 17, 17, 31, 17, 17, 17],
        'I' => [31, 4, 4, 4, 4, 4, 31],
        'J' => [7, 2, 2, 2, 18, 18, 12],
        'K' => [17, 18, 20, 24, 20, 18, 17],
        'L' => [16, 16, 16, 16, 16, 16, 31],
        'M' => [17, 27, 21, 21, 17, 17, 17],
        'N' => [17, 25, 21, 19, 17, 17, 17],
        'O' => [14, 17, 17, 17, 17, 17, 14],
        'P' => [30, 17, 17, 30, 16, 16, 16],
        'Q' => [14, 17, 17, 17, 21, 18, 13],
        'R' => [30, 17, 17, 30, 20, 18, 17],
        'S' => [15, 16, 16, 14, 1, 1, 30],
        'T' => [31, 4, 4, 4, 4, 4, 4],
        'U' => [17, 17, 17, 17, 17, 17, 14],
        'V' => [17, 17, 17, 17, 17, 10, 4],
        'W' => [17, 17, 17, 21, 21, 21, 10],
        'X' => [17, 17, 10, 4, 10, 17, 17],
        'Y' => [17, 17, 10, 4, 4, 4, 4],
        'Z' => [31, 1, 2, 4, 8, 16, 31],
        '0' => [14, 17, 19, 21, 25, 17, 14],
        '1' => [4, 12, 4, 4, 4, 4, 14],
        '2' => [14, 17, 1, 2, 4, 8, 31],
        '3' => [30, 1, 1, 14, 1, 1, 30],
        '4' => [2, 6, 10, 18, 31, 2, 2],
        '5' => [31, 16, 16, 30, 1, 1, 30],
        '6' => [14, 16, 16, 30, 17, 17, 14],
        '7' => [31, 1, 2, 4, 8, 8, 8],
        '8' => [14, 17, 17, 14, 17, 17, 14],
        '9' => [14, 17, 17, 15, 1, 1, 14],
        ':' => [0, 4, 4, 0, 4, 4, 0],
        ',' => [0, 0, 0, 0, 4, 4, 8],
        '.' => [0, 0, 0, 0, 0, 4, 4],
        '-' => [0, 0, 0, 31, 0, 0, 0],
        '+' => [0, 4, 4, 31, 4, 4, 0],
        '/' => [1, 1, 2, 4, 8, 16, 16],
        ' ' => [0; 7],
        _ => [31, 1, 2, 4, 0, 4, 0],
    }
}
