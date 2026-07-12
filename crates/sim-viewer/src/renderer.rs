use crate::camera::Camera;
use sim_core::{FeatureKind, GroundType, SimulationSnapshot, World, WorldPosition, WorldRect};

pub struct RenderState {
    pub snapshot: SimulationSnapshot,
    pub camera: Camera,
    pub hovered: Option<WorldPosition>,
    pub selection: Option<WorldRect>,
}

pub fn draw(frame: &mut [u32], width: u32, height: u32, world: &World, state: RenderState) {
    let width = width as usize;
    let height = height as usize;
    if width == 0 || height == 0 {
        return;
    }
    let view = state
        .camera
        .view(width as u32, height as u32, world.width(), world.height());

    for (index, pixel) in frame.iter_mut().enumerate() {
        let x = index % width;
        let y = index / width;
        *pixel = world
            .cell(view.screen_to_world_position(x as f64, y as f64))
            .map_or(rgb(0, 0, 0), terrain_color);
    }

    let scale = view.scale();
    let feature_size = scale.ceil().max(2.0) as usize;
    for feature in world.visible_features() {
        let (screen_x, screen_y) = view.world_to_screen(feature.position);
        if screen_x + feature_size as f64 <= 0.0
            || screen_y + feature_size as f64 <= 0.0
            || screen_x >= width as f64
            || screen_y >= height as f64
        {
            continue;
        }
        let color = match feature.kind {
            FeatureKind::Tree => rgb(24, 72, 28),
            FeatureKind::Rock => rgb(118, 116, 108),
            FeatureKind::BerryBush => rgb(112, 42, 74),
        };
        rectangle(
            frame,
            width,
            height,
            Rect::new(
                screen_x.max(0.0) as usize,
                screen_y.max(0.0) as usize,
                feature_size,
                feature_size,
            ),
            color,
        );
    }

    if let Some(position) = state.hovered {
        let (screen_x, screen_y) = view.world_to_screen(position);
        outline(
            frame,
            width,
            height,
            Rect::new(
                screen_x.max(0.0) as usize,
                screen_y.max(0.0) as usize,
                scale.ceil().max(3.0) as usize,
                scale.ceil().max(3.0) as usize,
            ),
            rgb(255, 235, 92),
        );
    }

    if let Some(bounds) = state.selection {
        let (left, top) = view.world_to_screen(bounds.min);
        let (right, bottom) = view.world_to_screen(bounds.max);
        let x = left.min(right).max(0.0) as usize;
        let y = top.min(bottom).max(0.0) as usize;
        let max_x = left.max(right).min(width as f64).max(0.0) as usize;
        let max_y = top.max(bottom).min(height as f64).max(0.0) as usize;
        if max_x > x && max_y > y {
            let rect = Rect::new(x, y, max_x - x, max_y - y);
            blend_rectangle(frame, width, height, rect, rgb(255, 220, 35), 72);
            outline(frame, width, height, rect, rgb(255, 235, 92));
        }
    }

    let pulse = ((state.snapshot.tick / 12) % 80) as usize;
    rectangle(
        frame,
        width,
        height,
        Rect::new(28, 28, 260, 92),
        rgb(9, 16, 20),
    );
    rectangle(
        frame,
        width,
        height,
        Rect::new(44, 48, 12 + pulse, 12),
        if state.snapshot.paused {
            rgb(210, 150, 55)
        } else {
            rgb(75, 205, 125)
        },
    );
    rectangle(
        frame,
        width,
        height,
        Rect::new(44, 72, (state.snapshot.speed * 24.0) as usize, 8),
        rgb(78, 145, 220),
    );

    // Temporary visual marker proving that the presentation reads simulation time.
    let marker_x = (state.snapshot.tick as usize / 2) % width.saturating_sub(32).max(1);
    rectangle(
        frame,
        width,
        height,
        Rect::new(marker_x, height.saturating_sub(28), 16, 16),
        rgb(235, 216, 130),
    );
}

fn outline(frame: &mut [u32], stride: usize, height: usize, rect: Rect, color: u32) {
    rectangle(
        frame,
        stride,
        height,
        Rect::new(rect.x, rect.y, rect.width, 1),
        color,
    );
    rectangle(
        frame,
        stride,
        height,
        Rect::new(
            rect.x,
            rect.y + rect.height.saturating_sub(1),
            rect.width,
            1,
        ),
        color,
    );
    rectangle(
        frame,
        stride,
        height,
        Rect::new(rect.x, rect.y, 1, rect.height),
        color,
    );
    rectangle(
        frame,
        stride,
        height,
        Rect::new(
            rect.x + rect.width.saturating_sub(1),
            rect.y,
            1,
            rect.height,
        ),
        color,
    );
}

fn blend_rectangle(
    frame: &mut [u32],
    stride: usize,
    height: usize,
    rect: Rect,
    color: u32,
    alpha: u32,
) {
    let max_y = (rect.y + rect.height).min(height);
    let max_x = (rect.x + rect.width).min(stride);
    for row in rect.y.min(height)..max_y {
        for column in rect.x.min(stride)..max_x {
            let pixel = &mut frame[row * stride + column];
            *pixel = blend(*pixel, color, alpha);
        }
    }
}

const fn blend(background: u32, foreground: u32, alpha: u32) -> u32 {
    let inverse = 255 - alpha;
    let red = (((background >> 16) & 255) * inverse + ((foreground >> 16) & 255) * alpha) / 255;
    let green = (((background >> 8) & 255) * inverse + ((foreground >> 8) & 255) * alpha) / 255;
    let blue = ((background & 255) * inverse + (foreground & 255) * alpha) / 255;
    rgb(red, green, blue)
}

fn terrain_color(cell: sim_core::TerrainCell) -> u32 {
    let elevation_shade = u32::from(cell.elevation >> 12);
    match cell.ground {
        GroundType::DeepWater => rgb(16, 48 + elevation_shade, 94 + elevation_shade),
        GroundType::ShallowWater => rgb(28, 84 + elevation_shade, 126 + elevation_shade),
        GroundType::Sand => rgb(184 + elevation_shade, 166 + elevation_shade, 105),
        GroundType::Grass => rgb(50 + elevation_shade, 112 + elevation_shade, 51),
        GroundType::ForestFloor => rgb(37, 86 + elevation_shade, 39),
        GroundType::Hill => rgb(100 + elevation_shade, 108 + elevation_shade, 72),
        GroundType::BareRock => rgb(
            125 + elevation_shade,
            124 + elevation_shade,
            119 + elevation_shade,
        ),
    }
}

#[derive(Clone, Copy)]
struct Rect {
    x: usize,
    y: usize,
    width: usize,
    height: usize,
}

impl Rect {
    const fn new(x: usize, y: usize, width: usize, height: usize) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }
}

fn rectangle(frame: &mut [u32], stride: usize, height: usize, rect: Rect, color: u32) {
    let max_y = (rect.y + rect.height).min(height);
    let max_x = (rect.x + rect.width).min(stride);
    for row in rect.y.min(height)..max_y {
        for column in rect.x.min(stride)..max_x {
            frame[row * stride + column] = color;
        }
    }
}

const fn rgb(red: u32, green: u32, blue: u32) -> u32 {
    (red << 16) | (green << 8) | blue
}
