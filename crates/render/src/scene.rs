//! CPU-side geometry the renderer draws: the reference grid and axes, plus the model.

use std::collections::HashMap;

use glam::{DAffine3, DVec3, Mat4, Vec3};
use sketchcraft_doc::{Entity, Guide, Owner, Scene as SceneData, Selection, VertexId, View};

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
    /// Guides, in world coordinates; turned into dashed lines each frame ([`guide_vertices`]).
    pub guides: Vec<SceneGuide>,
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
        // The normals of the faces on each edge, to hide the edges between a curve's facets.
        let mut sides: HashMap<(VertexId, VertexId), Vec<DVec3>> = HashMap::new();
        for (_, face) in model.faces() {
            for l in face.loops() {
                for i in 0..l.len() {
                    let (a, b) = (l[i], l[(i + 1) % l.len()]);
                    sides.entry((a.min(b), a.max(b))).or_default().push(face.normal);
                }
            }
        }
        for (id, e) in model.edges() {
            let entity = Entity::Edge(id);
            if let Some([n, m]) = sides.get(&(e.a.min(e.b), e.a.max(e.b))).map(Vec::as_slice)
                && is_soft(*n, *m)
                && !is_selected(entity)
            {
                continue;
            }
            let color = match view.owner(entity) {
                Owner::Outside => faded_edge,
                _ if is_selected(entity) => selected_edge,
                _ => edge,
            };
            let [a, b] = model.edge_points(e);
            self.add_line(world(a), world(b), color);
        }
        for (id, guide) in model.guides() {
            let entity = Entity::Guide(id);
            let color = match view.owner(entity) {
                Owner::Outside => srgb(170, 170, 170),
                _ if is_selected(entity) => selected_edge,
                _ => srgb(70, 70, 70),
            };
            let guide = match *guide {
                Guide::Line { point, dir } => {
                    Guide::Line { point: to_world.transform_point3(point), dir: to_world.transform_vector3(dir).normalize() }
                }
                Guide::Point(p) => Guide::Point(to_world.transform_point3(p)),
            };
            self.guides.push(SceneGuide { guide, color });
        }
    }
}


/// A guide to draw, in world coordinates.
#[derive(Clone, Copy, Debug)]
pub struct SceneGuide {
    pub guide: Guide,
    pub color: Color,
}

/// Position + color + the segment's start (for measuring dashes) + 1 for dashed, 0 for solid.
pub type DashedVertex = [f32; 11];

/// How far guide lines reach either side of the point nearest the eye, in inches: far beyond
/// any furniture, yet small enough for f32 precision.
const GUIDE_REACH: f64 = 20_000.0;

/// The guides as line segments for this camera: guide lines cut where they pass behind the
/// eye (so the GPU can measure dashes from a visible start), guide points as small crosses
/// facing the eye.
pub fn guide_vertices(guides: &[SceneGuide], view_proj: Mat4, eye: Vec3) -> Vec<DashedVertex> {
    let eye = eye.as_dvec3();
    let view_proj = view_proj.as_dmat4();
    // Depth in front of the eye (the projection's w).
    let w = |p: DVec3| (view_proj * p.extend(1.0)).w;
    let min_w = 1e-3;
    let vertex = |p: DVec3, color: Color, start: DVec3, dash: f32| -> DashedVertex {
        let (p, s) = (p.as_vec3(), start.as_vec3());
        [p.x, p.y, p.z, color[0], color[1], color[2], color[3], s.x, s.y, s.z, dash]
    };
    let mut out = Vec::new();
    for g in guides {
        match g.guide {
            Guide::Line { point, dir } => {
                let middle = point + dir * (eye - point).dot(dir);
                let (mut a, mut b) = (middle - dir * GUIDE_REACH, middle + dir * GUIDE_REACH);
                let (wa, wb) = (w(a), w(b));
                if wa < min_w && wb < min_w {
                    continue;
                }
                if wa < min_w {
                    a += (b - a) * ((min_w - wa) / (wb - wa));
                } else if wb < min_w {
                    b = a + (b - a) * ((wa - min_w) / (wa - wb));
                }
                // Dashes are measured from a point that projects near the view, not from a far
                // end that lands way off screen (where f32 pixel distances lose precision): the
                // point nearest the line of sight through the middle of the view.
                let ahead = view_proj.inverse().project_point3(DVec3::new(0.0, 0.0, 1.0)) - eye;
                let start = closest_to_ray(a, b, eye, ahead.normalize_or_zero());
                out.extend([vertex(a, g.color, start, 1.0), vertex(b, g.color, start, 1.0)]);
            }
            Guide::Point(p) => {
                let to = (p - eye).normalize_or_zero();
                let right = to.cross(DVec3::Z).try_normalize().unwrap_or(DVec3::X);
                let up = right.cross(to);
                let size = p.distance(eye) * 0.008;
                for d in [right + up, right - up] {
                    let d = d * size;
                    out.extend([vertex(p - d, g.color, p - d, 0.0), vertex(p + d, g.color, p - d, 0.0)]);
                }
            }
        }
    }
    out
}


/// The point of segment `a`–`b` nearest the ray from `origin` along unit `dir`.
fn closest_to_ray(a: DVec3, b: DVec3, origin: DVec3, dir: DVec3) -> DVec3 {
    let along = b - a;
    let length = along.length();
    if length < 1e-9 {
        return a;
    }
    let d = along / length;
    let w = a - origin;
    let k = d.dot(dir);
    let denom = 1.0 - k * k;
    let t = if denom < 1e-12 { 0.0 } else { (k * dir.dot(w) - d.dot(w)) / denom };
    a + d * t.clamp(0.0, length)
}

/// Whether the edge between faces with normals `n` and `m` is soft, as SketchUp draws the edges
/// between a circle's or arc's facets: hidden, because the faces meet at a shallow angle (under
/// 30°). Edges between faces on one plane stay visible: they're real seams.
fn is_soft(n: DVec3, m: DVec3) -> bool {
    let cos = n.dot(m);
    cos > 30f64.to_radians().cos() && cos < 1.0 - 1e-6
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
    fn edges_between_a_holes_facets_are_hidden() {
        let mut doc = Document::default();
        doc.edit(|m| {
            let base = m.add_rectangle(DVec3::ZERO, DVec3::X * 6.0, DVec3::Y * 4.0, DVec3::Z).unwrap();
            m.push_pull(base, 0.75).unwrap();
            let disk = m.draw_circle(DVec3::new(3.0, 2.0, 0.75), DVec3::Z, 0.5, 24).unwrap();
            m.push_pull(disk, -0.75).unwrap();
        });
        let mut scene = Scene::default();
        scene.add_view(doc.view(), doc.selection(), &doc.context_transform(), doc.scene());
        // The board's 12 edges and the hole's two rims; not the 24 seams down its wall.
        assert_eq!(scene.lines.len(), (12 + 24 + 24) * 2);
    }

    #[test]
    fn guide_lines_start_in_front_of_the_eye() {
        let guides = [SceneGuide { guide: Guide::Line { point: DVec3::ZERO, dir: DVec3::X }, color: srgb(0, 0, 0) }];
        let mut camera = sketchcraft_view::OrbitCamera { target: glam::Vec3::ZERO, distance: 50.0, ..Default::default() };
        camera.set_view(sketchcraft_view::StandardView::Iso);
        let size = glam::Vec2::new(1000.0, 800.0);
        let vp = camera.view_proj(size);
        let vertices = guide_vertices(&guides, vp, camera.eye());
        assert_eq!(vertices.len(), 2);
        for v in &vertices {
            let w = (vp * glam::Vec4::new(v[0], v[1], v[2], 1.0)).w;
            assert!(w > 0.0, "every end is in front of the camera");
        }
    }

    #[test]
    fn a_closed_groups_guides_are_drawn_and_faded_outside_the_open_group() {
        let (mut doc, group) = two_boards();
        doc.enter(group);
        doc.edit(|m| m.add_guide(Guide::Line { point: DVec3::ZERO, dir: DVec3::Z }));
        doc.exit();
        doc.selection_mut().clear();
        let mut scene = Scene::default();
        scene.add_view(doc.view(), doc.selection(), &doc.context_transform(), doc.scene());
        assert_eq!(scene.guides.len(), 1, "a closed group's guide still shows");
        assert_eq!(scene.guides[0].color, srgb(70, 70, 70));
        // Editing the other (loose) geometry's group puts this one outside: faded.
        let loose_group = doc.edit(|m| m.add_rectangle(DVec3::new(40.0, 0.0, 0.0), DVec3::X, DVec3::Y, DVec3::Z)).unwrap();
        let other = doc.push_pull(loose_group, 1.0, true).unwrap().unwrap();
        doc.enter(other);
        let mut scene = Scene::default();
        scene.add_view(doc.view(), doc.selection(), &doc.context_transform(), doc.scene());
        assert_eq!(scene.guides[0].color, srgb(170, 170, 170));
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
