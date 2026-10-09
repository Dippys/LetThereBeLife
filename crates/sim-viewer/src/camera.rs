use sim_core::{WORLD_GENERATION_BOUNDS, WORLD_SIDE_CELLS, WorldPosition};

const INITIAL_ZOOM: f64 = 1.0;
const MAX_ZOOM: f64 = 64.0;

#[derive(Debug, Clone, Copy)]
pub struct Camera {
    center_x: f64,
    center_y: f64,
    zoom: f64,
}

#[derive(Debug, Clone, Copy)]
pub struct CameraView {
    center_x: f64,
    center_y: f64,
    scale: f64,
    screen_width: u32,
    screen_height: u32,
}

#[derive(Debug, Clone, Copy)]
pub struct Viewport {
    pub screen_width: u32,
    pub screen_height: u32,
    pub world_width: u32,
    pub world_height: u32,
}

impl Camera {
    pub const fn at_origin() -> Self {
        Self {
            center_x: 0.0,
            center_y: 0.0,
            zoom: INITIAL_ZOOM,
        }
    }

    pub fn show_full_world(&mut self, viewport: Viewport) {
        self.center_x = 0.0;
        self.center_y = 0.0;
        self.zoom = minimum_zoom(viewport);
        self.constrain_to_viewport(viewport);
    }

    pub fn scale(
        self,
        screen_width: u32,
        screen_height: u32,
        world_width: u32,
        world_height: u32,
    ) -> f64 {
        initial_fit_scale(screen_width, screen_height, world_width, world_height) * self.zoom
    }

    pub fn screen_to_world_position(
        self,
        screen_x: f64,
        screen_y: f64,
        viewport: Viewport,
    ) -> WorldPosition {
        self.view(
            viewport.screen_width,
            viewport.screen_height,
            viewport.world_width,
            viewport.world_height,
        )
        .screen_to_world_position(screen_x, screen_y)
    }

    pub fn view(
        self,
        screen_width: u32,
        screen_height: u32,
        world_width: u32,
        world_height: u32,
    ) -> CameraView {
        CameraView {
            center_x: self.center_x,
            center_y: self.center_y,
            scale: self.scale(screen_width, screen_height, world_width, world_height),
            screen_width,
            screen_height,
        }
    }

    pub fn zoom_at(&mut self, steps: f64, cursor: (f64, f64), viewport: Viewport) {
        if viewport.screen_width == 0 || viewport.screen_height == 0 {
            return;
        }
        let before = self.world_point(
            cursor.0,
            cursor.1,
            viewport.screen_width,
            viewport.screen_height,
            viewport.world_width,
            viewport.world_height,
        );
        self.zoom = (self.zoom * 1.25_f64.powf(steps)).clamp(minimum_zoom(viewport), MAX_ZOOM);
        let after = self.world_point(
            cursor.0,
            cursor.1,
            viewport.screen_width,
            viewport.screen_height,
            viewport.world_width,
            viewport.world_height,
        );
        self.center_x += before.0 - after.0;
        self.center_y += before.1 - after.1;
        self.constrain_to_viewport(viewport);
    }

    /// Moves the world with the pointer while keeping its center inside the world boundary.
    pub fn pan_by_screen_delta(&mut self, delta_x: f64, delta_y: f64, viewport: Viewport) {
        if viewport.screen_width == 0 || viewport.screen_height == 0 {
            return;
        }
        let scale = self.scale(
            viewport.screen_width,
            viewport.screen_height,
            viewport.world_width,
            viewport.world_height,
        );
        self.center_x -= delta_x / scale;
        self.center_y -= delta_y / scale;
        self.constrain_to_viewport(viewport);
    }

    fn world_point(
        self,
        screen_x: f64,
        screen_y: f64,
        screen_width: u32,
        screen_height: u32,
        world_width: u32,
        world_height: u32,
    ) -> (f64, f64) {
        let scale = self.scale(screen_width, screen_height, world_width, world_height);
        (
            self.center_x + (screen_x - f64::from(screen_width) / 2.0) / scale,
            self.center_y + (screen_y - f64::from(screen_height) / 2.0) / scale,
        )
    }

    pub fn constrain_to_viewport(&mut self, viewport: Viewport) {
        if viewport.screen_width == 0 || viewport.screen_height == 0 {
            return;
        }
        self.zoom = self.zoom.clamp(minimum_zoom(viewport), MAX_ZOOM);
        let scale = self.scale(
            viewport.screen_width,
            viewport.screen_height,
            viewport.world_width,
            viewport.world_height,
        );
        self.center_x = clamp_axis_to_world(
            self.center_x,
            f64::from(viewport.screen_width) / (2.0 * scale),
            WORLD_GENERATION_BOUNDS.min.x as f64,
            WORLD_GENERATION_BOUNDS.max.x as f64,
        );
        self.center_y = clamp_axis_to_world(
            self.center_y,
            f64::from(viewport.screen_height) / (2.0 * scale),
            WORLD_GENERATION_BOUNDS.min.y as f64,
            WORLD_GENERATION_BOUNDS.max.y as f64,
        );
    }
}

fn initial_fit_scale(
    screen_width: u32,
    screen_height: u32,
    world_width: u32,
    world_height: u32,
) -> f64 {
    (f64::from(screen_width) / f64::from(world_width))
        .min(f64::from(screen_height) / f64::from(world_height))
}

fn minimum_zoom(viewport: Viewport) -> f64 {
    let initial_fit = initial_fit_scale(
        viewport.screen_width,
        viewport.screen_height,
        viewport.world_width,
        viewport.world_height,
    );
    let world_fit = (f64::from(viewport.screen_width) / WORLD_SIDE_CELLS as f64)
        .min(f64::from(viewport.screen_height) / WORLD_SIDE_CELLS as f64);
    (world_fit / initial_fit).min(INITIAL_ZOOM)
}

fn clamp_axis_to_world(center: f64, half_view: f64, min: f64, max: f64) -> f64 {
    if half_view * 2.0 >= max - min {
        (min + max) / 2.0
    } else {
        center.clamp(min + half_view, max - half_view)
    }
}

impl CameraView {
    pub fn center(self) -> [f32; 2] {
        [self.center_x as f32, self.center_y as f32]
    }

    pub const fn scale(self) -> f64 {
        self.scale
    }

    pub fn world_bounds(self) -> sim_core::WorldRect {
        let min = self.world_point(0.0, 0.0);
        let max = self.world_point(f64::from(self.screen_width), f64::from(self.screen_height));
        sim_core::WorldRect {
            min: WorldPosition {
                x: floor_to_i64(min.0),
                y: floor_to_i64(min.1),
            },
            max: WorldPosition {
                x: ceil_to_i64(max.0),
                y: ceil_to_i64(max.1),
            },
        }
    }

    pub fn screen_to_world_position(self, screen_x: f64, screen_y: f64) -> WorldPosition {
        let (x, y) = self.world_point(screen_x, screen_y);
        WorldPosition {
            x: floor_to_i64(x),
            y: floor_to_i64(y),
        }
    }

    fn world_point(self, screen_x: f64, screen_y: f64) -> (f64, f64) {
        (
            self.center_x + (screen_x - f64::from(self.screen_width) / 2.0) / self.scale,
            self.center_y + (screen_y - f64::from(self.screen_height) / 2.0) / self.scale,
        )
    }
}

fn floor_to_i64(value: f64) -> i64 {
    value.floor().clamp(i64::MIN as f64, i64::MAX as f64) as i64
}

fn ceil_to_i64(value: f64) -> i64 {
    value.ceil().clamp(i64::MIN as f64, i64::MAX as f64) as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_camera_fits_and_centers_world() {
        let camera = Camera::at_origin();
        assert_eq!(
            camera.screen_to_world_position(480.0, 270.0, test_viewport()),
            WorldPosition { x: 0, y: 0 }
        );
    }

    #[test]
    fn zoom_keeps_cursor_over_same_world_point() {
        let mut camera = Camera::at_origin();
        let before = camera.world_point(550.0, 300.0, 960, 540, 1_024, 1_024);
        camera.zoom_at(4.0, (550.0, 300.0), test_viewport());
        let after = camera.world_point(550.0, 300.0, 960, 540, 1_024, 1_024);
        assert!((before.0 - after.0).abs() < 0.001);
        assert!((before.1 - after.1).abs() < 0.001);
    }

    #[test]
    fn camera_center_stops_at_world_border() {
        let mut camera = Camera::at_origin();
        camera.pan_by_screen_delta(f64::MAX, 0.0, test_viewport());
        assert_eq!(
            camera.view(960, 540, 1_024, 1_024).world_bounds().min.x,
            WORLD_GENERATION_BOUNDS.min.x
        );
    }

    #[test]
    fn camera_can_zoom_out_to_fit_the_full_world() {
        let mut camera = Camera::at_origin();
        camera.zoom_at(-100.0, (480.0, 270.0), test_viewport());
        assert_eq!(camera.zoom, minimum_zoom(test_viewport()));
        let visible = camera.view(960, 540, 1_024, 1_024).world_bounds();
        assert!(visible.contains_rect(WORLD_GENERATION_BOUNDS));
    }

    #[test]
    fn show_full_world_recenters_and_fits_after_navigation() {
        let viewport = test_viewport();
        let mut camera = Camera::at_origin();
        camera.pan_by_screen_delta(400.0, -200.0, viewport);
        camera.zoom_at(6.0, (120.0, 100.0), viewport);

        camera.show_full_world(viewport);

        assert_eq!(camera.center_x, 0.0);
        assert_eq!(camera.center_y, 0.0);
        assert_eq!(camera.zoom, minimum_zoom(viewport));
        assert!(
            camera
                .view(960, 540, 1_024, 1_024)
                .world_bounds()
                .contains_rect(WORLD_GENERATION_BOUNDS)
        );
    }

    #[test]
    fn configured_world_reaches_full_envelope_at_one_sixteenth_fit() {
        let viewport = Viewport {
            world_width: 4_096,
            world_height: 4_096,
            ..test_viewport()
        };
        assert_eq!(minimum_zoom(viewport), 1.0 / 16.0);
    }

    #[test]
    fn resize_reconstrains_rectangular_bootstrap_to_full_world_fit() {
        let landscape = Viewport {
            screen_width: 960,
            screen_height: 540,
            world_width: 4_096,
            world_height: 1_024,
        };
        let portrait = Viewport {
            screen_width: 540,
            screen_height: 960,
            ..landscape
        };
        let mut camera = Camera::at_origin();
        camera.zoom_at(-100.0, (480.0, 270.0), landscape);
        assert!(camera.zoom < minimum_zoom(portrait));

        camera.constrain_to_viewport(portrait);

        assert_eq!(camera.zoom, minimum_zoom(portrait));
        assert!(
            camera
                .view(540, 960, 4_096, 1_024)
                .world_bounds()
                .contains_rect(WORLD_GENERATION_BOUNDS)
        );
    }

    #[test]
    fn world_bounds_include_fractionally_visible_edge_cells() {
        let bounds = CameraView {
            center_x: 0.5,
            center_y: 0.5,
            scale: 1.0,
            screen_width: 2,
            screen_height: 2,
        }
        .world_bounds();

        assert_eq!(bounds.min, WorldPosition { x: -1, y: -1 });
        assert_eq!(bounds.max, WorldPosition { x: 2, y: 2 });
    }

    #[test]
    fn world_bounds_keep_exact_integer_maximum_exclusive() {
        let bounds = CameraView {
            center_x: 1.0,
            center_y: 1.0,
            scale: 1.0,
            screen_width: 2,
            screen_height: 2,
        }
        .world_bounds();

        assert_eq!(bounds.min, WorldPosition { x: 0, y: 0 });
        assert_eq!(bounds.max, WorldPosition { x: 2, y: 2 });
    }

    const fn test_viewport() -> Viewport {
        Viewport {
            screen_width: 960,
            screen_height: 540,
            world_width: 1_024,
            world_height: 1_024,
        }
    }
}
