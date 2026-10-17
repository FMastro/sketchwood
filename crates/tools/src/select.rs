//! The Select tool (Space). Click, double-click (face + edges, or edge + faces), triple-click
//! (everything connected), and window (drag right) or crossing (drag left) selection.
//! Ctrl adds, Shift toggles, Ctrl+Shift removes.

use std::collections::{BTreeMap, BTreeSet};

use glam::{DVec3, Vec2};
use woodsketch_doc::{Document, Entity, Model, Owner, View};

use crate::pick::{Screen, pick_entity, segment_distance};
use crate::{Modifiers, Overlay, SelectionWindow};

#[derive(Default)]
pub struct SelectTool {
    window: Option<(Vec2, Vec2)>,
}

impl SelectTool {
    pub fn is_busy(&self) -> bool {
        self.window.is_some()
    }

    pub fn cancel(&mut self) {
        self.window = None;
    }

    pub fn click(&mut self, doc: &mut Document, screen: Screen<'_>, cursor: Vec2, count: u8, mods: Modifiers) {
        let view = doc.view();
        let hit = pick_entity(view.model(), screen, cursor);
        let Some(entity) = hit.and_then(|e| view.resolve(e)) else {
            // Clicking empty space, or geometry outside the group being edited, closes it
            // (SketchUp). At the top level it just clears the selection.
            if !doc.path().is_empty() && mods == Modifiers::default() {
                doc.exit();
                doc.selection_mut().clear();
            } else {
                apply(doc, Vec::new(), mods);
            }
            return;
        };
        let model = doc.model();
        let picked: Vec<Entity> = match entity {
            // Double-clicking a group or component opens it for editing.
            Entity::Instance(id) if count >= 2 => {
                doc.enter(id);
                return;
            }
            Entity::Instance(_) | Entity::Guide(_) => vec![entity],
            _ if count >= 3 => model.connected(entity).into_iter().collect(),
            Entity::Face(id) if count == 2 => {
                std::iter::once(Entity::Face(id)).chain(model.face_edges(id).into_iter().map(Entity::Edge)).collect()
            }
            Entity::Edge(id) if count == 2 => {
                std::iter::once(Entity::Edge(id)).chain(model.edge_faces(id).into_iter().map(Entity::Face)).collect()
            }
            _ => vec![entity],
        };
        apply(doc, picked, mods);
    }

    pub fn drag_start(&mut self, origin: Vec2) {
        self.window = Some((origin, origin));
    }

    pub fn drag(&mut self, cursor: Vec2) {
        if let Some((_, end)) = &mut self.window {
            *end = cursor;
        }
    }

    pub fn drag_end(&mut self, doc: &mut Document, screen: Screen<'_>, cursor: Vec2, mods: Modifiers) {
        let Some((start, _)) = self.window.take() else { return };
        let crossing = cursor.x < start.x;
        let picked = in_window(doc.view(), screen, start.min(cursor), start.max(cursor), crossing);
        apply(doc, picked, mods);
    }

    pub fn overlay(&self, overlay: &mut Overlay) {
        if let Some((start, end)) = self.window {
            overlay.window = Some(SelectionWindow { start, end, crossing: end.x < start.x });
        }
    }
}

fn apply(doc: &mut Document, picked: Vec<Entity>, mods: Modifiers) {
    let selection = doc.selection_mut();
    match (mods.ctrl, mods.shift) {
        (false, false) => {
            selection.clear();
            selection.extend(picked);
        }
        (true, false) => selection.extend(picked),
        (false, true) => selection.toggle(picked),
        (true, true) => selection.remove(picked),
    }
}

/// Window selection takes entities entirely inside; crossing takes anything touching. A group
/// or component counts as one entity: inside if all of it is, touched if any of it is.
fn in_window(view: &View, screen: Screen<'_>, min: Vec2, max: Vec2, crossing: bool) -> Vec<Entity> {
    let mut picked = BTreeSet::new();
    // Per instance: (any part hit, every part hit).
    let mut instances: BTreeMap<Entity, (bool, bool)> = BTreeMap::new();
    for (entity, hit) in hits(view.model(), screen, min, max, crossing) {
        match view.owner(entity) {
            Owner::Active => {
                if hit {
                    picked.insert(entity);
                }
            }
            Owner::Instance(id) => {
                let state = instances.entry(Entity::Instance(id)).or_insert((false, true));
                state.0 |= hit;
                state.1 &= hit;
            }
            Owner::Outside => {}
        }
    }
    picked.extend(instances.into_iter().filter(|(_, (any, all))| if crossing { *any } else { *all }).map(|(e, _)| e));
    picked.into_iter().collect()
}

/// Every edge and face of `model`, and whether the window (or crossing) rectangle takes it.
fn hits(model: &Model, screen: Screen<'_>, min: Vec2, max: Vec2, crossing: bool) -> Vec<(Entity, bool)> {
    let inside = |p: Vec2| p.cmpge(min).all() && p.cmple(max).all();
    let project_all = |points: &[DVec3]| -> Option<Vec<Vec2>> { points.iter().map(|&p| screen.project(p)).collect() };
    let touches = |polygon: &[Vec2], closed: bool| {
        let segments = if closed { polygon.len() } else { polygon.len() - 1 };
        polygon.iter().any(|&p| inside(p))
            || (0..segments).any(|i| segment_hits_rect(polygon[i], polygon[(i + 1) % polygon.len()], min, max))
            || (closed && contains((min + max) * 0.5, polygon))
    };

    // Geometry behind the camera can't be in the window.
    let edges = model.edges().map(|(id, edge)| {
        let hit = project_all(&model.edge_points(edge)).is_some_and(|points| {
            if crossing { touches(&points, false) } else { points.iter().all(|&p| inside(p)) }
        });
        (Entity::Edge(id), hit)
    });
    let faces = model.faces().map(|(id, face)| {
        let hit = project_all(&model.loop_points(&face.outer)).is_some_and(|points| {
            if crossing { touches(&points, true) } else { points.iter().all(|&p| inside(p)) }
        });
        (Entity::Face(id), hit)
    });
    edges.chain(faces).collect()
}

fn segment_hits_rect(a: Vec2, b: Vec2, min: Vec2, max: Vec2) -> bool {
    let corners = [min, Vec2::new(max.x, min.y), max, Vec2::new(min.x, max.y)];
    (0..4).any(|i| segments_intersect(a, b, corners[i], corners[(i + 1) % 4]))
}

fn segments_intersect(a: Vec2, b: Vec2, c: Vec2, d: Vec2) -> bool {
    let cross = |o: Vec2, p: Vec2, q: Vec2| (p - o).perp_dot(q - o);
    let (d1, d2) = (cross(c, d, a), cross(c, d, b));
    let (d3, d4) = (cross(a, b, c), cross(a, b, d));
    if ((d1 > 0.0) != (d2 > 0.0)) && ((d3 > 0.0) != (d4 > 0.0)) {
        return true;
    }
    // Touching counts as crossing.
    [(c, d, a), (c, d, b), (a, b, c), (a, b, d)].iter().any(|&(p, q, r)| segment_distance(r, p, q).0 < 1e-4)
}

fn contains(p: Vec2, polygon: &[Vec2]) -> bool {
    let mut inside = false;
    for (i, &a) in polygon.iter().enumerate() {
        let b = polygon[(i + 1) % polygon.len()];
        if (a.y > p.y) != (b.y > p.y) && p.x < a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x) {
            inside = !inside;
        }
    }
    inside
}

#[cfg(test)]
mod tests {
    use woodsketch_view::{OrbitCamera, StandardView};

    use super::*;

    const SIZE: Vec2 = Vec2::new(1000.0, 800.0);

    fn setup() -> (Document, OrbitCamera) {
        let mut doc = Document::default();
        doc.edit(|m| {
            m.add_rectangle(DVec3::ZERO, DVec3::X * 1.5, DVec3::Y * 3.5, DVec3::Z);
            m.add_rectangle(DVec3::new(10.0, 0.0, 0.0), DVec3::X * 1.5, DVec3::Y * 3.5, DVec3::Z);
        });
        let mut camera = OrbitCamera { target: glam::Vec3::new(6.0, 2.0, 0.0), distance: 40.0, ..Default::default() };
        camera.set_view(StandardView::Top);
        (doc, camera)
    }

    fn at(screen: Screen<'_>, x: f64, y: f64) -> Vec2 {
        screen.project(DVec3::new(x, y, 0.0)).unwrap()
    }

    #[test]
    fn click_selects_one_and_double_click_adds_bounding_edges() {
        let (mut doc, camera) = setup();
        let screen = Screen::new(&camera, SIZE);
        let mut tool = SelectTool::default();
        tool.click(&mut doc, screen, at(screen, 0.75, 1.75), 1, Modifiers::default());
        assert_eq!(doc.selection().len(), 1);
        tool.click(&mut doc, screen, at(screen, 0.75, 1.75), 2, Modifiers::default());
        assert_eq!(doc.selection().len(), 5);
    }

    #[test]
    fn clicking_empty_space_clears() {
        let (mut doc, camera) = setup();
        let screen = Screen::new(&camera, SIZE);
        let mut tool = SelectTool::default();
        doc.select_all();
        tool.click(&mut doc, screen, at(screen, 5.0, 20.0), 1, Modifiers::default());
        assert!(doc.selection().is_empty());
    }

    #[test]
    fn modifiers_add_toggle_and_remove() {
        let (mut doc, camera) = setup();
        let screen = Screen::new(&camera, SIZE);
        let mut tool = SelectTool::default();
        let (a, b) = (at(screen, 0.75, 1.75), at(screen, 10.75, 1.75));
        tool.click(&mut doc, screen, a, 1, Modifiers::default());
        tool.click(&mut doc, screen, b, 1, Modifiers { ctrl: true, ..Default::default() });
        assert_eq!(doc.selection().len(), 2);
        tool.click(&mut doc, screen, a, 1, Modifiers { shift: true, ..Default::default() });
        assert_eq!(doc.selection().len(), 1);
        tool.click(&mut doc, screen, b, 1, Modifiers { ctrl: true, shift: true, ..Default::default() });
        assert!(doc.selection().is_empty());
    }

    #[test]
    fn window_takes_only_whats_fully_inside() {
        let (mut doc, camera) = setup();
        let screen = Screen::new(&camera, SIZE);
        let mut tool = SelectTool::default();
        // Left to right around the first board only.
        tool.drag_start(at(screen, -1.0, -1.0));
        tool.drag_end(&mut doc, screen, at(screen, 2.0, 5.0), Modifiers::default());
        assert_eq!(doc.selection().len(), 5);
    }

    #[test]
    fn crossing_takes_anything_touched() {
        let (mut doc, camera) = setup();
        let screen = Screen::new(&camera, SIZE);
        let mut tool = SelectTool::default();
        // Right to left over just the middle of the first board.
        tool.drag_start(at(screen, 1.0, 2.0));
        tool.drag_end(&mut doc, screen, at(screen, 0.5, 1.5), Modifiers::default());
        // Only the face: the rectangle doesn't reach any edge.
        assert_eq!(doc.selection().len(), 1);
        // Right to left from x=11 to x=0.5: crosses each board's face and one long edge of
        // each (x=1.5 and x=10).
        tool.drag_start(at(screen, 11.0, 2.0));
        tool.drag_end(&mut doc, screen, at(screen, 0.5, 1.5), Modifiers::default());
        assert_eq!(doc.selection().len(), 4);
    }
}
