use sim_core::WorldPosition;

const MIN_ZOOM: f64 = 1.0 / 16.0;
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
    pub fn centered(world_width: u32, world_height: u32) -> Self {
        Self {
            center_x: f64::from(world_width) / 2.0,
            center_y: f64::from(world_height) / 2.0,
            zoom: INITIAL_ZOOM,
        }
    }

    pub fn scale(
        self,
        screen_width: u32,
        screen_height: u32,
        world_width: u32,
        world_height: u32,
    ) -> f64 {
        let fit = (f64::from(screen_width) / f64::from(world_width))
            .min(f64::from(screen_height) / f64::from(world_height));
        fit * self.zoom
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
        let before = self.world_point(
            cursor.0,
            cursor.1,
            viewport.screen_width,
            viewport.screen_height,
            viewport.world_width,
            viewport.world_height,
        );
        self.zoom = (self.zoom * 1.25_f64.powf(steps)).clamp(MIN_ZOOM, MAX_ZOOM);
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
    }

    /// Moves the world with the pointer; camera coordinates are intentionally unbounded.
    pub fn pan_by_screen_delta(&mut self, delta_x: f64, delta_y: f64, viewport: Viewport) {
        let scale = self.scale(
            viewport.screen_width,
            viewport.screen_height,
            viewport.world_width,
            viewport.world_height,
        );
        self.center_x -= delta_x / scale;
        self.center_y -= delta_y / scale;
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
}

impl CameraView {
    pub fn center(self) -> [f32; 2] {
        [self.center_x as f32, self.center_y as f32]
    }

    pub const fn scale(self) -> f64 {
        self.scale
    }

    pub fn world_bounds(self) -> sim_core::WorldRect {
        sim_core::WorldRect {
            min: self.screen_to_world_position(0.0, 0.0),
            max: self.screen_to_world_position(
                f64::from(self.screen_width),
                f64::from(self.screen_height),
            ),
        }
    }

    pub fn screen_to_world_position(self, screen_x: f64, screen_y: f64) -> WorldPosition {
        let x = self.center_x + (screen_x - f64::from(self.screen_width) / 2.0) / self.scale;
        let y = self.center_y + (screen_y - f64::from(self.screen_height) / 2.0) / self.scale;
        WorldPosition {
            x: floor_to_i64(x),
            y: floor_to_i64(y),
        }
    }
}

fn floor_to_i64(value: f64) -> i64 {
    value.floor().clamp(i64::MIN as f64, i64::MAX as f64) as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_camera_fits_and_centers_world() {
        let camera = Camera::centered(1_024, 1_024);
        assert_eq!(
            camera.screen_to_world_position(480.0, 270.0, test_viewport()),
            WorldPosition { x: 512, y: 512 }
        );
    }

    #[test]
    fn zoom_keeps_cursor_over_same_world_point() {
        let mut camera = Camera::centered(1_024, 1_024);
        let before = camera.world_point(550.0, 300.0, 960, 540, 1_024, 1_024);
        camera.zoom_at(4.0, (550.0, 300.0), test_viewport());
        let after = camera.world_point(550.0, 300.0, 960, 540, 1_024, 1_024);
        assert!((before.0 - after.0).abs() < 0.001);
        assert!((before.1 - after.1).abs() < 0.001);
    }

    #[test]
    fn drag_can_move_camera_beyond_generated_world() {
        let mut camera = Camera::centered(1_024, 1_024);
        camera.pan_by_screen_delta(2_000.0, 0.0, test_viewport());
        assert!(camera.center_x < 0.0);
        assert!(
            camera
                .screen_to_world_position(480.0, 270.0, test_viewport())
                .x
                < 0
        );
    }

    #[test]
    fn camera_can_zoom_out_beyond_initial_fit() {
        let mut camera = Camera::centered(1_024, 1_024);
        camera.zoom_at(-100.0, (480.0, 270.0), test_viewport());
        assert_eq!(camera.zoom, MIN_ZOOM);
        assert!(camera.scale(960, 540, 1_024, 1_024) < 1.0);
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
