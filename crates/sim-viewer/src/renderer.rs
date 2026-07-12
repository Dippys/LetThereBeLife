use sim_core::SimulationSnapshot;

pub fn draw(frame: &mut [u32], width: u32, height: u32, snapshot: SimulationSnapshot) {
    let width = width as usize;
    let height = height as usize;
    if width == 0 || height == 0 {
        return;
    }

    for (index, pixel) in frame.iter_mut().enumerate() {
        let x = index % width;
        let y = index / width;
        let horizon = height * 58 / 100;
        *pixel = if y < horizon {
            let shade = 22 + (y * 24 / horizon.max(1)) as u32;
            rgb(shade / 2, shade, shade + 12)
        } else {
            let checker = ((x / 32) + (y / 32)) % 2;
            if checker == 0 {
                rgb(24, 49, 34)
            } else {
                rgb(27, 55, 38)
            }
        };
    }

    let pulse = ((snapshot.tick / 12) % 80) as usize;
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
        if snapshot.paused {
            rgb(210, 150, 55)
        } else {
            rgb(75, 205, 125)
        },
    );
    rectangle(
        frame,
        width,
        height,
        Rect::new(44, 72, (snapshot.speed * 24.0) as usize, 8),
        rgb(78, 145, 220),
    );

    // Temporary visual marker proving that the presentation reads simulation time.
    let marker_x = (snapshot.tick as usize / 2) % width.saturating_sub(32).max(1);
    rectangle(
        frame,
        width,
        height,
        Rect::new(marker_x, height * 58 / 100 - 12, 16, 16),
        rgb(235, 216, 130),
    );
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
