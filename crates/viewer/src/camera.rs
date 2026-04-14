use glam::{Mat4, Vec3};

/// Orbit camera: orbits around a target point with azimuth/elevation/distance.
pub struct Camera {
    /// World position the camera orbits around
    pub target: Vec3,
    /// Distance from target (zoom level)
    pub distance: f32,
    /// Horizontal rotation angle (radians)
    pub azimuth: f32,
    /// Vertical angle from horizontal (radians, clamped 10-85 degrees)
    pub elevation: f32,
    /// Viewport dimensions
    pub width: f32,
    pub height: f32,
    /// Near/far clip planes
    pub near: f32,
    pub far: f32,
}

impl Camera {
    pub fn new(width: f32, height: f32) -> Self {
        Self {
            target: Vec3::new(512.0, 0.0, 512.0),
            distance: 500.0,
            azimuth: std::f32::consts::FRAC_PI_4,
            elevation: 0.7, // ~40 degrees
            width,
            height,
            near: 1.0,
            far: 10000.0,
        }
    }

    pub fn resize(&mut self, width: f32, height: f32) {
        self.width = width;
        self.height = height;
    }

    /// Compute the camera's world-space position on the orbit sphere.
    pub fn eye(&self) -> Vec3 {
        let cos_elev = self.elevation.cos();
        let offset = Vec3::new(
            cos_elev * self.azimuth.cos(),
            self.elevation.sin(),
            cos_elev * self.azimuth.sin(),
        );
        self.target + offset * self.distance
    }

    /// View matrix (world → camera).
    pub fn view_matrix(&self) -> Mat4 {
        Mat4::look_at_rh(self.eye(), self.target, Vec3::Y)
    }

    /// Perspective projection matrix.
    pub fn proj_matrix(&self) -> Mat4 {
        let aspect = self.width / self.height;
        Mat4::perspective_rh(std::f32::consts::FRAC_PI_4, aspect, self.near, self.far)
    }

    /// Combined view-projection matrix.
    pub fn view_proj(&self) -> Mat4 {
        self.proj_matrix() * self.view_matrix()
    }

    /// Orbit: change azimuth and elevation.
    pub fn orbit(&mut self, delta_azimuth: f32, delta_elevation: f32) {
        self.azimuth += delta_azimuth;
        self.elevation =
            (self.elevation + delta_elevation).clamp(10.0_f32.to_radians(), 85.0_f32.to_radians());
    }

    /// Zoom: change distance.
    pub fn zoom(&mut self, factor: f32) {
        self.distance = (self.distance * factor).clamp(10.0, 5000.0);
    }

    /// Pan: move target in the camera's local XZ plane.
    pub fn pan(&mut self, dx: f32, dz: f32) {
        // Camera's right vector (horizontal only)
        let right = Vec3::new(-self.azimuth.sin(), 0.0, self.azimuth.cos());
        // Camera's forward vector (horizontal only)
        let forward = Vec3::new(-self.azimuth.cos(), 0.0, -self.azimuth.sin());

        let speed = self.distance * 0.002;
        self.target += right * dx * speed + forward * dz * speed;
    }
}

/// GPU-uploadable camera uniforms.
#[repr(C)]
#[derive(Debug, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct CameraUniforms {
    pub view_proj: [[f32; 4]; 4],
    pub camera_pos: [f32; 3],
    pub _pad0: f32,
}

impl Camera {
    pub fn uniforms(&self) -> CameraUniforms {
        CameraUniforms {
            view_proj: self.view_proj().to_cols_array_2d(),
            camera_pos: self.eye().to_array(),
            _pad0: 0.0,
        }
    }
}
