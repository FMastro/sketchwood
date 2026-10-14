//! CPU-side geometry the renderer draws: the reference grid and axes, plus the model.

use glam::{DAffine3, DVec3, Vec3};
use sketchcraft_doc::{Entity, Owner, Scene as SceneData, Selection, View};

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
    /// SketchUp-colored axes, and optionally a ground grid (12" spacing).
    pub fn reference(grid: bool) -> Self {
        let mut scene = Self::default();

        let grid_color = srgb(222, 222, 222);
        let (spacing, half_count) = (12.0, if grid { 20 } else { 0 });
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
                scene.background_lines.extend([line_vertex(a, grid_color), line_vertex(b, grid_color)]);
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

    /// Add the drawing as seen while editing: `view` holds everything in the edited
    /// container's coordinates, which `to_world` maps to the world. The selection is
    /// highlighted in blue (whole groups when they're selected); while editing inside a group,
    /// everything outside it is faded, as in SketchUp.
    pub fn add_view(&mut self, view: &View, selection: &Selection, to_world: &DAffine3, scene: &SceneData) {
        let model = view.model();
        let edge = srgb(0, 0, 0);
        let selected_edge = srgb(0, 64, 255);
        let faded_edge = srgb(190, 190, 190);
        // SketchUp's default material: warm white front, blue-grey back.
        let front = srgb(250, 250, 246);
        let back = srgb(164, 180, 204);
        let tint = |c: Color| mix(c, srgb(80, 120, 255), 0.45);
        let fade = |c: Color| mix(c, srgb(250, 250, 250), 0.7);
        let is_selected = |entity: Entity| match view.owner(entity) {
            Owner::Active => selection.contains(entity),
            Owner::Instance(id) => selection.contains(Entity::Instance(id)),
            Owner::Outside => false,
        };
        let world = |p: DVec3| to_world.transform_point3(p).as_vec3();

        for (id, face) in model.faces() {
            let entity = Entity::Face(id);
            // A painted face (or one in a painted group) shows the material on both sides.
            let painted = view.face_material(id).and_then(|m| scene.material(m)).map(|m| srgb(m.color[0], m.color[1], m.color[2]));
            let (front, back) = painted.map_or((front, back), |c| (c, c));
            let (front, back) = match view.owner(entity) {
                Owner::Outside => (fade(front), fade(back)),
                _ if is_selected(entity) => (tint(front), tint(back)),
                _ => (front, back),
            };
            let n = to_world.transform_vector3(face.normal).normalize().as_vec3();
            for triangle in model.triangulate(face) {
                for p in triangle.map(world) {
                    self.faces.push([
                        p.x, p.y, p.z, n.x, n.y, n.z, front[0], front[1], front[2], front[3], back[0],
                        back[1], back[2], back[3],
                    ]);
                }
            }
        }
        for (id, e) in model.edges() {
            let entity = Entity::Edge(id);
            let color = match view.owner(entity) {
                Owner::Outside => faded_edge,
                _ if is_selected(entity) => selected_edge,
                _ => edge,
            };
            let [a, b] = model.edge_points(e);
            self.add_line(world(a), world(b), color);
        }
    }
}

#[cfg(test)]
mod tests {
    use sketchcraft_doc::Document;

    use super::*;

    fn two_boards() -> (Document, sketchcraft_doc::InstanceId) {
        let mut doc = Document::default();
        let face = doc.edit(|m| m.add_rectangle(DVec3::ZERO, DVec3::X * 1.5, DVec3::Y * 3.5, DVec3::Z)).unwrap();
        let group = doc.push_pull(face, 10.0, true).unwrap().unwrap();
        let loose = doc.edit(|m| m.add_rectangle(DVec3::X * 20.0, DVec3::X * 1.5, DVec3::Y * 3.5, DVec3::Z)).unwrap();
        doc.push_pull(loose, 10.0, false).unwrap();
        (doc, group)
    }

    #[test]
    fn groups_and_loose_geometry_both_draw() {
        let (doc, _) = two_boards();
        let mut scene = Scene::default();
        scene.add_view(doc.view(), doc.selection(), &doc.context_transform(), doc.scene());
        assert_eq!(scene.faces.len(), 2 * 6 * 6); // two boards x 6 faces x 2 triangles x 3
        assert_eq!(scene.lines.len(), 2 * 12 * 2);
    }

    #[test]
    fn a_selected_group_is_tinted_as_a_whole() {
        let (mut doc, group) = two_boards();
        let plain = |doc: &Document| {
            let mut scene = Scene::default();
            scene.add_view(doc.view(), doc.selection(), &doc.context_transform(), doc.scene());
            scene
        };
        let before = plain(&doc);
        doc.selection_mut().extend([Entity::Instance(group)]);
        let after = plain(&doc);
        let changed = before.faces.iter().zip(&after.faces).filter(|(a, b)| a[6..10] != b[6..10]).count();
        assert_eq!(changed, 36, "exactly the group's 6 faces (36 vertices) are tinted");
    }

    #[test]
    fn editing_inside_fades_the_outside_and_keeps_world_positions() {
        let (mut doc, group) = two_boards();
        let world_before = {
            let mut scene = Scene::default();
            scene.add_view(doc.view(), doc.selection(), &doc.context_transform(), doc.scene());
            scene.faces.iter().map(|v| [v[0], v[1], v[2]]).collect::<Vec<_>>()
        };
        doc.enter(group);
        let mut scene = Scene::default();
        scene.add_view(doc.view(), doc.selection(), &doc.context_transform(), doc.scene());
        let mut world_after: Vec<_> = scene.faces.iter().map(|v| [v[0], v[1], v[2]]).collect();
        let mut sorted_before = world_before.clone();
        let key = |p: &[f32; 3]| (p[0].to_bits(), p[1].to_bits(), p[2].to_bits());
        sorted_before.sort_by_key(key);
        world_after.sort_by_key(key);
        assert_eq!(sorted_before, world_after, "nothing moves on screen when opening a group");
        let faded = scene.lines.iter().filter(|l| l[3..6] == srgb(190, 190, 190)[..3]).count();
        assert_eq!(faded, 24, "the loose board outside is faded");
    }
}
