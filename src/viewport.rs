use crate::layers::Orientation;

fn m44(a: f64, b: f64, p: f64, q: f64) -> nalgebra::Matrix4<f64> {
    nalgebra::matrix![
        a, 0.0, 0.0, p;
        0.0, b, 0.0, q;
        0.0, 0.0, 1.0, 0.0;
        0.0, 0.0, 0.0, 1.0
    ]
}

fn m_orient(a: f64, b: f64, c: f64, d: f64) -> nalgebra::Matrix4<f64> {
    nalgebra::matrix![
        a, b, 0.0, 0.0;
        c, d, 0.0, 0.0;
        0.0, 0.0, 1.0, 0.0;
        0.0, 0.0, 0.0, 1.0
    ]
}

fn proj_xy(m: nalgebra::Matrix4<f64>, (x, y): (f64, f64)) -> (f64, f64) {
    let v = m * nalgebra::vector![x, y, 0.0, 1.0];
    (v.x, v.y)
}

pub struct Viewport {
    pub cursor: (f64, f64),
    pub zoom: f64,
    pub pan: (f64, f64),
}

impl Viewport {
    pub fn new() -> Self {
        Self {
            cursor: (0.0, 0.0),
            zoom: 1.0,
            pan: (0.0, 0.0),
        }
    }

    fn scale(image_size: (f64, f64), screen_size: (f64, f64)) -> (f64, f64) {
        let texture_aspect_ratio = (image_size.0 / image_size.1).abs();
        let window_aspect_ratio = (screen_size.0 / screen_size.1).abs();

        if window_aspect_ratio > texture_aspect_ratio {
            // Window is wider than texture
            let scale_y = 1.0;
            let scale_x = texture_aspect_ratio / window_aspect_ratio;
            (scale_x, scale_y)
        } else {
            // Window is taller than texture
            let scale_x = 1.0;
            let scale_y = window_aspect_ratio / texture_aspect_ratio;
            (scale_x, scale_y)
        }
    }

    pub fn zoom(&mut self, delta: f64, screen_size: (f64, f64)) {
        let cursor = proj_xy((2.0 / self.zoom) * self.mscreen(screen_size), self.cursor);
        let zoom = (self.zoom * (1.0 + delta * 0.2)).max(1.0).min(1000.0);
        let delta = zoom - self.zoom;
        self.zoom = zoom;
        self.pan.0 -= delta * cursor.0;
        self.pan.1 += delta * cursor.1;
    }

    pub fn pan(&mut self, delta: (f64, f64)) {
        self.pan.0 += delta.0;
        self.pan.1 += delta.1;
    }

    fn projection(&self, scale: (f64, f64)) -> nalgebra::Matrix4<f64> {
        m44(
            scale.0 * self.zoom,
            scale.1 * self.zoom,
            self.pan.0,
            self.pan.1,
        )
    }

    fn mscreen(&self, screen_size: (f64, f64)) -> nalgebra::Matrix4<f64> {
        // => [0, screen_width] -> [-0.5, 0.5]
        m44(
            1.0 / screen_size.0,
            1.0 / screen_size.1,
            (-self.pan.0 - 1.0) / 2.0,
            (self.pan.1 - 1.0) / 2.0,
        )
    }

    fn mscale(&self, scale: (f64, f64)) -> nalgebra::Matrix4<f64> {
        m44(
            1.0 / (scale.0 * self.zoom),
            1.0 / (scale.1 * self.zoom),
            0.5,
            0.5,
        )
    }

    pub fn to_uniforms(
        &self,
        image_size: (f64, f64),
        screen_size: (f64, f64),
        orientation: Orientation,
        alpha: f64,
    ) -> Uniforms {
        let m_orientation = match orientation {
            Orientation::Normal | Orientation::Unspecified => m_orient(1.0, 0.0, 0.0, 1.0),
            Orientation::Rotate90 => m_orient(0.0, 1.0, -1.0, 0.0),
            Orientation::Rotate90HorizontalFlip => m_orient(0.0, -1.0, -1.0, 0.0),
            Orientation::Rotate90VerticalFlip => m_orient(0.0, 1.0, 1.0, 0.0),
            Orientation::HorizontalFlip => m_orient(-1.0, 0.0, 0.0, 1.0),
            Orientation::VerticalFlip => m_orient(1.0, 0.0, 0.0, -1.0),
            Orientation::Rotate180 => m_orient(-1.0, 0.0, 0.0, -1.0),
            Orientation::Rotate270 => m_orient(0.0, -1.0, 1.0, 0.0),
        };
        let scale = Self::scale(proj_xy(m_orientation, image_size), screen_size);
        let projection = self.projection(scale) * m_orientation;
        let cursor = proj_xy(self.mscale(scale) * self.mscreen(screen_size), self.cursor);
        Uniforms {
            projection: projection.map(|x| x as f32).into(),
            image_size: [image_size.0 as f32, image_size.1 as f32],
            cursor: [cursor.0 as f32, cursor.1 as f32],
            alpha: alpha as f32,
            padding: [0; 3],
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct Uniforms {
    projection: [[f32; 4]; 4],
    image_size: [f32; 2],
    cursor: [f32; 2],
    alpha: f32,
    padding: [u32; 3],
}

impl Uniforms {
    pub fn min_binding_size() -> usize {
        std::mem::size_of::<Self>()
    }
}

// Ensure your struct implements bytemuck traits
unsafe impl bytemuck::Pod for Uniforms {}
unsafe impl bytemuck::Zeroable for Uniforms {}
