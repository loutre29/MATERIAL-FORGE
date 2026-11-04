use glam::{Mat4, Vec3};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct OrbitCamera {
    pub target: [f32; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub distance: f32,
    pub fov_y: f32,
    /// Orthographic projection (same framing as the perspective one).
    #[serde(default)]
    pub ortho: bool,
    /// Lens shift (normalized device units): the target shows off the
    /// view's center (the packshot studio centers it between the panels).
    #[serde(skip)]
    pub shift: [f32; 2],
}

impl Default for OrbitCamera {
    fn default() -> Self {
        Self { target: [0.0; 3], yaw: 0.6, pitch: 0.25, distance: 3.0, fov_y: 40f32.to_radians(), ortho: false, shift: [0.0; 2] }
    }
}

impl OrbitCamera {
    pub fn eye(&self) -> Vec3 {
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        Vec3::from(self.target) + Vec3::new(cp * sy, sp, cp * cy) * self.distance
    }

    pub fn near_far(&self) -> (f32, f32) {
        (self.distance * 0.01, self.distance * 100.0)
    }

    pub fn view(&self) -> Mat4 {
        Mat4::look_at_rh(self.eye(), Vec3::from(self.target), Vec3::Y)
    }

    pub fn proj(&self, aspect: f32) -> Mat4 {
        let (n, f) = self.near_far();
        let shift = Mat4::from_translation(Vec3::new(self.shift[0], self.shift[1], 0.0));
        if self.ortho {
            let h = self.distance * (self.fov_y * 0.5).tan();
            let w = h * aspect.max(1e-3);
            return shift * Mat4::orthographic_rh(-w, w, -h, h, n, f);
        }
        shift * Mat4::perspective_rh(self.fov_y, aspect.max(1e-3), n, f)
    }

    /// World units per pixel at the target distance (ortho: everywhere).
    pub fn ortho_world_per_px(&self, viewport_h: f32) -> f32 {
        2.0 * self.distance * (self.fov_y * 0.5).tan() / viewport_h.max(1.0)
    }

    /// Preset view: yaw / pitch in radians.
    pub fn look_from(&mut self, yaw: f32, pitch: f32) {
        self.yaw = yaw;
        self.pitch = pitch.clamp(-1.5699, 1.5699);
    }

    pub fn view_proj(&self, aspect: f32) -> Mat4 {
        self.proj(aspect) * self.view()
    }

    fn basis(&self) -> (Vec3, Vec3) {
        let fwd = (Vec3::from(self.target) - self.eye()).normalize_or(Vec3::NEG_Z);
        let right = fwd.cross(Vec3::Y).normalize_or(Vec3::X);
        (right, right.cross(fwd))
    }

    pub fn orbit(&mut self, dx: f32, dy: f32) {
        self.yaw -= dx * 0.0105;
        self.pitch = (self.pitch + dy * 0.0105).clamp(-1.55, 1.55);
    }

    /// dx, dy in pixels; viewport_h in pixels.
    pub fn pan(&mut self, dx: f32, dy: f32, viewport_h: f32) {
        let scale = 2.0 * self.distance * (self.fov_y * 0.5).tan() / viewport_h.max(1.0);
        let (right, up) = self.basis();
        let t = Vec3::from(self.target) + (-right * dx + up * dy) * scale;
        self.target = t.into();
    }

    /// Orbit around `pivot` (the point under the cursor) instead of the
    /// target: the pivot keeps its place on screen.
    pub fn orbit_around(&mut self, dx: f32, dy: f32, pivot: Vec3) {
        let d0 = (self.eye() - Vec3::from(self.target)).normalize_or_zero();
        self.orbit(dx, dy);
        let d1 = (self.eye() - Vec3::from(self.target)).normalize_or_zero();
        let r = glam::Quat::from_rotation_arc(d0, d1);
        let t = pivot + r * (Vec3::from(self.target) - pivot);
        self.target = t.into();
    }

    /// Zoom by `factor` toward `point` (homothety centered on it).
    pub fn zoom_at(&mut self, factor: f32, point: Vec3) {
        let f = factor.clamp(0.2, 5.0);
        let t = point + (Vec3::from(self.target) - point) * f;
        self.target = t.into();
        self.distance = (self.distance * f).clamp(1e-4, 1e7);
    }

    pub fn zoom(&mut self, factor: f32) {
        self.distance = (self.distance * factor).clamp(1e-4, 1e7);
    }

    pub fn frame(&mut self, min: Vec3, max: Vec3) {
        let center = (min + max) * 0.5;
        let radius = ((max - min).length() * 0.5).max(1e-4);
        self.target = center.into();
        self.distance = radius / (self.fov_y * 0.5).sin() * 1.05;
    }
}
