//! CPU-side geometry the renderer draws: the reference grid and axes, plus the model.

use glam::{DVec3, Vec3};
use sketchcraft_doc::{Entity, Model, Selection};

/// Linear-space RGBA. The render target is sRGB, so colors are converted from sRGB on creation.
pub type Color = [f32; 4];

pub fn srgb(r: u8, g: u8, b: u8) -> Color {
    let lin = |c: u8| (f32::from(c) / 255.0).powf(2.2);
    [lin(r), lin(g), lin(b), 1.0]
}

fn mix(a: Color, b: Color, t: f32) -> Color {
    std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t)
}

/// Position + color, 7 floats.
pub type LineVertex = [f32; 7];
/// Position + normal + front color + back color, 14 floats.
pub type FaceVertex = [f32; 14];

#[derive(Default)]
pub struct Scene {
    /// Drawn first without writing depth, so faces always cover them (the ground grid).
    pub background_lines: Vec<LineVertex>,
    /// Pairs of vertices, one pair per segment.
    pub lines: Vec<LineVertex>,
    /// Triples of vertices, one triple per triangle.
    pub faces: Vec<FaceVertex>,
}

fn line_vertex(p: Vec3, color: Color) -> LineVertex {
    [p.x, p.y, p.z, color[0], color[1], color[2], color[3]]
}

impl Scene {
    /// The ground grid (12" spacing) and SketchUp-colored axes.
    pub fn reference() -> Self {
        let mut scene = Self::default();

        let grid = srgb(222, 222, 222);
        let (spacing, half_count) = (12.0, 20);
        let extent = spacing * half_count as f32;
        for i in -half_count..=half_count {
            if i == 0 {
                continue; // The axes are drawn there.
            }
            let o = i as f32 * spacing;
            for (a, b) in [
                (Vec3::new(o, -extent, 0.0), Vec3::new(o, extent, 0.0)),
                (Vec3::new(-extent, o, 0.0), Vec3::new(extent, o, 0.0)),
            ] {
                scene.background_lines.extend([line_vertex(a, grid), line_vertex(b, grid)]);
            }
        }

        // Solid on the positive side, faded on the negative side, as in SketchUp.
        let axis_len = 1.0e4;
        for (dir, color) in [
            (Vec3::X, srgb(220, 0, 0)),
            (Vec3::Y, srgb(0, 160, 0)),
            (Vec3::Z, srgb(0, 0, 220)),
        ] {
            let faded = mix(color, srgb(255, 255, 255), 0.6);
            scene.add_line(Vec3::ZERO, dir * axis_len, color);
            scene.add_line(Vec3::ZERO, -dir * axis_len, faded);
        }
        scene
    }

    pub fn add_line(&mut self, a: Vec3, b: Vec3, color: Color) {
        self.lines.extend([line_vertex(a, color), line_vertex(b, color)]);
    }

    /// Add the model's faces and edges, highlighting the selection in blue.
    pub fn add_model(&mut self, model: &Model, selection: &Selection) {
        let edge = srgb(0, 0, 0);
        let selected = srgb(0, 64, 255);
        // SketchUp's default material: warm white front, blue-grey back.
        let front = srgb(250, 250, 246);
        let back = srgb(164, 180, 204);
        let tint = |c: Color| mix(c, srgb(80, 120, 255), 0.45);

        for (id, face) in model.faces() {
            let is_selected = selection.contains(Entity::Face(id));
            let (front, back) = if is_selected { (tint(front), tint(back)) } else { (front, back) };
            let n = face.normal.as_vec3();
            for triangle in model.triangulate(face) {
                for p in triangle.map(DVec3::as_vec3) {
                    self.faces.push([
                        p.x, p.y, p.z, n.x, n.y, n.z, front[0], front[1], front[2], front[3], back[0],
                        back[1], back[2], back[3],
                    ]);
                }
            }
        }
        for (id, e) in model.edges() {
            let color = if selection.contains(Entity::Edge(id)) { selected } else { edge };
            let [a, b] = model.edge_points(e);
            self.add_line(a.as_vec3(), b.as_vec3(), color);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_geometry_lands_in_buffers() {
        let mut model = Model::default();
        let face = model.add_rectangle(DVec3::ZERO, DVec3::X * 1.5, DVec3::Y * 3.5, DVec3::Z).unwrap();
        let mut selection = Selection::default();
        selection.extend([Entity::Face(face)]);

        let mut scene = Scene::default();
        scene.add_model(&model, &selection);
        assert_eq!(scene.faces.len(), 6); // two triangles
        assert_eq!(scene.lines.len(), 8); // four edges

        let mut unselected = Scene::default();
        unselected.add_model(&model, &Selection::default());
        assert_ne!(scene.faces[0][6..10], unselected.faces[0][6..10]);
    }
}
