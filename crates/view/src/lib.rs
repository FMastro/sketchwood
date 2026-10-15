//! Camera and view math. Pure functions over `glam` types, with no GPU or UI dependencies,
//! so everything here is unit-testable.
//!
//! Conventions match SketchUp: Z is up, red = X, green = Y, blue = Z. Screen coordinates are
//! in egui points with the origin at the viewport's top-left and Y pointing down.

use glam::camera::rh::{proj::directx, view::look_at_mat4};
use glam::{Mat4, Vec2, Vec3};

const MIN_PITCH: f32 = -89.9_f32.to_radians();
const MAX_PITCH: f32 = 89.9_f32.to_radians();
const MIN_DISTANCE: f32 = 0.05;
const MAX_DISTANCE: f32 = 1.0e6;
const ORBIT_RADIANS_PER_POINT: f32 = 0.008;
/// Distance multiplier per zoom step; a mouse-wheel notch is roughly one step.
const ZOOM_FACTOR_PER_STEP: f32 = 0.85;

/// A perspective camera orbiting a target point.
#[derive(Clone, Debug, PartialEq)]
pub struct OrbitCamera {
    pub target: Vec3,
    pub distance: f32,
    /// Azimuth around +Z, in radians, measured from +X.
    pub yaw: f32,
    /// Elevation of the eye above the target's XY plane, in radians.
    pub pitch: f32,
    /// Vertical field of view, in radians.
    pub fov_y: f32,
}

impl Default for OrbitCamera {
    /// SketchUp's default view: looking from the front-left, slightly above, 35° field of view.
    fn default() -> Self {
        let mut camera = Self {
            target: Vec3::ZERO,
            distance: 200.0,
            yaw: 0.0,
            pitch: 0.0,
            fov_y: 35_f32.to_radians(),
        };
        camera.set_view(StandardView::Iso);
        camera
    }
}

/// Preset view directions, like SketchUp's Views toolbar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StandardView {
    Iso,
    Top,
    Front,
    Right,
}

impl OrbitCamera {
    /// Eye position in world space.
    pub fn eye(&self) -> Vec3 {
        self.target + self.offset_dir() * self.distance
    }

    /// Unit vector from the eye toward the target.
    pub fn forward(&self) -> Vec3 {
        -self.offset_dir()
    }

    pub fn view(&self) -> Mat4 {
        look_at_mat4(self.eye(), self.target, Vec3::Z)
    }

    pub fn projection(&self, aspect: f32) -> Mat4 {
        // Scale the clip planes with distance so depth precision follows the zoom level.
        let near = (self.distance * 0.01).max(0.001);
        let far = self.distance * 100.0 + 1000.0;
        // wgpu clip space: depth in [0, 1], Y up (glam calls this the DirectX convention).
        directx::perspective(self.fov_y, aspect.max(1e-3), near, far)
    }

    pub fn view_proj(&self, viewport: Vec2) -> Mat4 {
        self.projection(aspect(viewport)) * self.view()
    }

    pub fn set_view(&mut self, view: StandardView) {
        let (yaw, pitch) = match view {
            StandardView::Iso => (-120.0, 30.0),
            StandardView::Top => (-90.0, 89.9),
            StandardView::Front => (-90.0, 0.0),
            StandardView::Right => (0.0, 0.0),
        };
        self.yaw = f32::to_radians(yaw);
        self.pitch = f32::to_radians(pitch);
    }

    /// Orbit by a pointer drag. Dragging moves the model with the pointer.
    pub fn orbit(&mut self, drag: Vec2) {
        self.yaw -= drag.x * ORBIT_RADIANS_PER_POINT;
        self.pitch = (self.pitch + drag.y * ORBIT_RADIANS_PER_POINT).clamp(MIN_PITCH, MAX_PITCH);
    }

    /// Pan by a pointer drag so the point under the target stays under the pointer.
    pub fn pan(&mut self, drag: Vec2, viewport: Vec2) {
        let world_per_point = 2.0 * self.distance * (self.fov_y * 0.5).tan() / viewport.y.max(1.0);
        let (right, up) = self.screen_axes();
        self.target += (-right * drag.x + up * drag.y) * world_per_point;
    }

    /// Zoom by `steps` (positive zooms in) toward the point under `cursor`.
    /// Zoom toward what's under the cursor at `depth` (its distance along the view direction),
    /// as SketchUp does: the orbit center first slides along the line of sight to that depth
    /// (the view doesn't change), so zooming closes in on the surface instead of slowing to a
    /// stop at an orbit center floating in front of it.
    pub fn zoom_toward(&mut self, steps: f32, cursor: Vec2, viewport: Vec2, depth: Option<f32>) {
        if let Some(depth) = depth.filter(|d| d.is_finite() && *d > MIN_DISTANCE) {
            let eye = self.eye();
            self.target = eye + self.forward() * depth;
            self.distance = depth;
        }
        self.zoom_at(steps, cursor, viewport);
    }

    pub fn zoom_at(&mut self, steps: f32, cursor: Vec2, viewport: Vec2) {
        let new_distance =
            (self.distance * ZOOM_FACTOR_PER_STEP.powf(steps)).clamp(MIN_DISTANCE, MAX_DISTANCE);
        let factor = new_distance / self.distance;
        // Pivot on where the cursor ray meets the plane through the target facing the camera,
        // so that point stays fixed on screen.
        let (origin, dir) = self.ray(cursor, viewport);
        let normal = self.forward();
        let denom = dir.dot(normal);
        if denom.abs() > 1e-6 {
            let t = (self.target - origin).dot(normal) / denom;
            let pivot = origin + dir * t;
            self.target = pivot + (self.target - pivot) * factor;
        }
        self.distance = new_distance;
    }

    /// Frame an axis-aligned box so it fills the viewport.
    pub fn zoom_extents(&mut self, min: Vec3, max: Vec3, viewport: Vec2) {
        let radius = ((max - min).length() * 0.5).max(1.0);
        let fov_x = 2.0 * ((self.fov_y * 0.5).tan() * aspect(viewport)).atan();
        let half_fov = self.fov_y.min(fov_x) * 0.5;
        self.target = (min + max) * 0.5;
        self.distance = (radius / half_fov.sin() * 1.05).clamp(MIN_DISTANCE, MAX_DISTANCE);
    }

    /// World-space ray through a screen point: (origin on the near plane, unit direction).
    pub fn ray(&self, cursor: Vec2, viewport: Vec2) -> (Vec3, Vec3) {
        let ndc = Vec2::new(
            2.0 * cursor.x / viewport.x.max(1.0) - 1.0,
            1.0 - 2.0 * cursor.y / viewport.y.max(1.0),
        );
        let inverse = self.view_proj(viewport).inverse();
        let near = inverse.project_point3(ndc.extend(0.0));
        let far = inverse.project_point3(ndc.extend(1.0));
        (near, (far - near).normalize())
    }

    /// Screen position of a world point, or `None` if it is behind the camera.
    pub fn project(&self, point: Vec3, viewport: Vec2) -> Option<Vec2> {
        let clip = self.view_proj(viewport) * point.extend(1.0);
        if clip.w <= 0.0 {
            return None;
        }
        let ndc = clip.truncate() / clip.w;
        Some(Vec2::new(
            (ndc.x + 1.0) * 0.5 * viewport.x,
            (1.0 - ndc.y) * 0.5 * viewport.y,
        ))
    }

    fn offset_dir(&self) -> Vec3 {
        let (sin_pitch, cos_pitch) = self.pitch.sin_cos();
        let (sin_yaw, cos_yaw) = self.yaw.sin_cos();
        Vec3::new(cos_pitch * cos_yaw, cos_pitch * sin_yaw, sin_pitch)
    }

    /// Camera right and up vectors in world space.
    fn screen_axes(&self) -> (Vec3, Vec3) {
        let forward = self.forward();
        let right = forward.cross(Vec3::Z).normalize();
        (right, right.cross(forward))
    }
}

fn aspect(viewport: Vec2) -> f32 {
    viewport.x / viewport.y.max(1.0)
}

#[cfg(test)]
mod tests {
    #[test]
    fn zooming_toward_a_surface_gets_right_up_to_it() {
        // The orbit center floats 40" in front of a wall 100" away: zooming at the old center
        // creeps toward it and never reaches the wall; zooming toward the wall's depth does.
        let size = Vec2::new(1000.0, 800.0);
        let mut camera = OrbitCamera { target: Vec3::new(0.0, 0.0, 0.0), distance: 40.0, ..Default::default() };
        camera.set_view(StandardView::Front);
        let eye = camera.eye();
        let wall = 100.0;
        let center = size * 0.5;
        for _ in 0..60 {
            camera.zoom_toward(1.0, center, size, Some(wall - (camera.eye() - eye).length()));
        }
        let travelled = (camera.eye() - eye).length();
        assert!(travelled > wall - 1.0, "within an inch of the wall: moved {travelled}");
    }

    use super::*;

    const VIEWPORT: Vec2 = Vec2::new(1200.0, 800.0);

    fn assert_close(a: Vec2, b: Vec2) {
        assert!((a - b).length() < 0.01, "{a} != {b}");
    }

    #[test]
    fn target_projects_to_viewport_center() {
        let camera = OrbitCamera::default();
        assert_close(camera.project(camera.target, VIEWPORT).unwrap(), VIEWPORT * 0.5);
    }

    #[test]
    fn default_view_looks_from_front_left_above() {
        let eye = OrbitCamera::default().eye();
        assert!(eye.x < 0.0 && eye.y < 0.0 && eye.z > 0.0, "{eye}");
    }

    #[test]
    fn ray_through_center_points_at_target() {
        let camera = OrbitCamera::default();
        let (_, dir) = camera.ray(VIEWPORT * 0.5, VIEWPORT);
        assert!(dir.dot(camera.forward()) > 0.9999);
    }

    #[test]
    fn pitch_is_clamped_short_of_the_poles() {
        let mut camera = OrbitCamera::default();
        camera.orbit(Vec2::new(0.0, 1.0e6));
        assert!(camera.pitch <= MAX_PITCH);
        // look_at must stay well-defined at the clamp.
        assert!(camera.view().is_finite());
    }

    #[test]
    fn pan_keeps_dragged_point_under_pointer() {
        let mut camera = OrbitCamera::default();
        let point = camera.target;
        let before = camera.project(point, VIEWPORT).unwrap();
        let drag = Vec2::new(37.0, -21.0);
        camera.pan(drag, VIEWPORT);
        assert_close(camera.project(point, VIEWPORT).unwrap(), before + drag);
    }

    #[test]
    fn zoom_keeps_point_under_cursor_fixed() {
        let mut camera = OrbitCamera::default();
        let cursor = Vec2::new(900.0, 250.0);
        let (origin, dir) = camera.ray(cursor, VIEWPORT);
        let t = (camera.target - origin).dot(camera.forward()) / dir.dot(camera.forward());
        let point = origin + dir * t;

        camera.zoom_at(3.0, cursor, VIEWPORT);

        assert!(camera.distance < 200.0);
        assert_close(camera.project(point, VIEWPORT).unwrap(), cursor);
    }

    #[test]
    fn zoom_extents_fits_box_on_screen() {
        let mut camera = OrbitCamera::default();
        let (min, max) = (Vec3::ZERO, Vec3::new(48.0, 24.0, 30.0));
        camera.zoom_extents(min, max, VIEWPORT);
        for corner in [min, max, Vec3::new(48.0, 0.0, 0.0), Vec3::new(0.0, 24.0, 30.0)] {
            let p = camera.project(corner, VIEWPORT).unwrap();
            assert!(p.x >= 0.0 && p.x <= VIEWPORT.x && p.y >= 0.0 && p.y <= VIEWPORT.y, "{p}");
        }
    }
}
