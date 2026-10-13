//! The Move tool (M). Moves the selection, or whatever is under the cursor when nothing is
//! selected. Click a base point, then the destination (or press-drag-release).
//!
//! - Inference: endpoints snap; near a red/green/blue axis line through the base point, the
//!   move locks to it. Arrow keys lock an axis: Right = red, Left = green, Up/Down = blue.
//! - Tap Ctrl to toggle copying.
//! - Type a distance (along the current direction) or a vector `x,y,z` / `<x,y,z>`; after a
//!   copy, `3x` (or `*3`, `x3`) makes an array of 3 and `/3` divides the distance into 3.
//!   Typing right after finishing changes the move just made.

use std::collections::BTreeSet;

use glam::{DVec3, Vec2};
use sketchcraft_doc::units::{format_length, parse_length, parse_lengths};
use sketchcraft_doc::{Document, EPS, Entity, Model, VertexId};

use crate::pick::{Screen, Snap, closest_on_line, pick_entity, snap_feature};
use crate::{Array, Axis, Guide, Overlay, Protractor, highlight_entity, parse_array};

/// How close, in points, the cursor must come to an axis line to lock onto it.
const AXIS_RADIUS: f32 = 14.0;

/// How close, in points, the cursor must be to a rotation grip.
const GRIP_RADIUS: f32 = 9.0;

/// SketchUp's rotation grips: red crosses on the faces of the bounding box of what Move would
/// move. Clicking one rotates about that face's center line instead of moving.
pub struct Grips {
    pub entities: Vec<Entity>,
    pub vertices: BTreeSet<VertexId>,
    /// Faces of the bounding box that face the camera.
    pub faces: Vec<GripFace>,
    /// The grip under the cursor: (face index, grip index).
    pub hovered: Option<(usize, usize)>,
}

#[derive(Clone, Copy, Debug)]
pub struct GripFace {
    pub center: DVec3,
    pub normal: DVec3,
    pub grips: [DVec3; 4],
}

#[derive(Default)]
pub struct MoveTool {
    op: Option<Op>,
    /// What the move would pick up when nothing is selected.
    hover: Option<Entity>,
    grips: Option<Grips>,
    snap: Option<Snap>,
    /// Axis locked with the arrow keys.
    lock: Option<Axis>,
    copy: bool,
    last: Option<Done>,
}

struct Op {
    entities: Vec<Entity>,
    vertices: BTreeSet<VertexId>,
    base: DVec3,
    offset: DVec3,
    /// The axis the move currently follows, by inference or lock.
    axis: Option<Axis>,
    dragging: bool,
}

#[derive(Clone)]
struct Done {
    entities: Vec<Entity>,
    vertices: BTreeSet<VertexId>,
    offset: DVec3,
    copy: Option<Array>,
    model_version: u64,
}

impl MoveTool {
    pub fn is_busy(&self) -> bool {
        self.op.is_some()
    }

    pub fn cancel(&mut self) {
        self.op = None;
        self.snap = None;
        self.lock = None;
        self.last = None;
        self.grips = None;
    }

    pub fn toggle_copy(&mut self) {
        self.copy = !self.copy;
    }

    /// Lock moves to `axis`, or unlock if it's already locked.
    pub fn lock_axis(&mut self, axis: Axis) {
        self.lock = if self.lock == Some(axis) { None } else { Some(axis) };
    }

    pub fn hover(&mut self, doc: &Document, screen: Screen<'_>, cursor: Vec2) {
        let model = doc.model();
        if self.op.is_none() {
            // Grips can sit off the geometry; keep them while the cursor is on one.
            if let Some(grips) = &mut self.grips {
                grips.hovered = grip_at(grips, screen, cursor);
                if grips.hovered.is_some() {
                    self.snap = None;
                    return;
                }
            }
            self.hover = if doc.selection().is_empty() { pick_entity(model, screen, cursor) } else { None };
            self.snap = crate::pick::snap(model, screen, cursor, None);
            let target: Vec<Entity> = match self.hover {
                // Loose geometry: grips go around the whole connected piece, like a board.
                Some(entity) => model.connected(entity).into_iter().collect(),
                None => doc.selection().iter().collect(),
            };
            self.grips = grips_for(model, screen, target);
            if let Some(grips) = &mut self.grips {
                grips.hovered = grip_at(grips, screen, cursor);
            }
            return;
        }
        let (target, axis, snap) = self.target(model, screen, cursor);
        self.snap = snap;
        if let Some(op) = &mut self.op {
            op.offset = target - op.base;
            op.axis = axis;
        }
    }

    /// The rotation grip under the cursor, if any: (what to rotate, the vertices, center, axis,
    /// starting direction).
    pub fn grip_under(&mut self, doc: &Document, screen: Screen<'_>, cursor: Vec2) -> Option<GripRotation> {
        if self.op.is_some() {
            return None;
        }
        self.hover(doc, screen, cursor);
        let grips = self.grips.as_ref()?;
        let (f, g) = grips.hovered?;
        let face = grips.faces[f];
        Some(GripRotation {
            entities: grips.entities.clone(),
            vertices: grips.vertices.clone(),
            center: face.center,
            normal: face.normal,
            from: (face.grips[g] - face.center).normalize(),
        })
    }

    pub fn click(&mut self, doc: &mut Document, screen: Screen<'_>, cursor: Vec2) -> Result<(), String> {
        if self.op.is_some() {
            self.hover(doc, screen, cursor);
            return self.finish(doc, None);
        }
        self.start(doc, screen, cursor, false)
    }

    pub fn drag_start(&mut self, doc: &Document, screen: Screen<'_>, origin: Vec2) -> Result<(), String> {
        if self.op.is_none() {
            return self.start(doc, screen, origin, true);
        }
        Ok(())
    }

    pub fn drag_end(&mut self, doc: &mut Document, screen: Screen<'_>, cursor: Vec2) -> Result<(), String> {
        if !self.op.as_ref().is_some_and(|op| op.dragging) {
            return Ok(());
        }
        self.hover(doc, screen, cursor);
        self.finish(doc, None)
    }

    pub fn enter(&mut self, doc: &mut Document, typed: &str) -> Result<(), String> {
        let typed = typed.trim();
        if let Some(array) = parse_array(typed) {
            let Some(last) = self.last.take().filter(|l| l.model_version == doc.model_version()) else {
                return Err("Copy something first (tap Ctrl while moving), then type 3x or /3".into());
            };
            if last.copy.is_none() {
                self.last = Some(last);
                return Err("Arrays need a copy: tap Ctrl while moving".into());
            }
            return self.redo_last(doc, last, |done| done.copy = Some(array));
        }

        if let Some(op) = &self.op {
            // Along an axis, use its exact direction so typed lengths come out exact.
            let direction = match op.axis {
                Some(axis) if op.offset.dot(axis.dir()) < 0.0 => -axis.dir(),
                Some(axis) => axis.dir(),
                None => op.offset,
            };
            let offset = parse_offset(typed, direction, op.base)?;
            return self.finish(doc, Some(offset));
        }
        match self.last.take().filter(|l| l.model_version == doc.model_version()) {
            Some(last) => {
                let offset = parse_offset(typed, last.offset, DVec3::ZERO)?;
                self.redo_last(doc, last, |done| done.offset = offset)
            }
            None => Err("Click a base point, then type the distance".into()),
        }
    }

    pub fn status(&self) -> String {
        let copy = if self.copy { " (copying)" } else { "" };
        if self.op.is_some() {
            format!("Pick the destination, or enter a distance{copy}. Arrow keys lock an axis; tap Ctrl to copy.")
        } else {
            format!("Pick a base point to move{copy}. Tap Ctrl to copy; type 3x or /3 after a copy for an array.")
        }
    }

    pub fn measurement(&self) -> String {
        match (&self.op, &self.last) {
            (Some(op), _) => format_length(op.offset.length()),
            (None, Some(last)) => match last.copy {
                Some(Array { count, divide: false }) if count > 1 => format!("{count}x"),
                Some(Array { count, divide: true }) if count > 1 => format!("/{count}"),
                _ => format_length(last.offset.length()),
            },
            (None, None) => String::new(),
        }
    }

    pub fn overlay(&self, doc: &Document, overlay: &mut Overlay) {
        let model = doc.model();
        overlay.snap = self.snap;
        let Some(op) = &self.op else {
            if let Some(entity) = self.hover {
                highlight_entity(model, entity, overlay);
            }
            if let Some(grips) = &self.grips {
                for (f, face) in grips.faces.iter().enumerate() {
                    for (g, &p) in face.grips.iter().enumerate() {
                        overlay.grips.push((p, grips.hovered == Some((f, g))));
                    }
                }
                // Hovering a grip previews the protractor it would rotate with.
                if let Some((f, g)) = grips.hovered {
                    let face = grips.faces[f];
                    overlay.protractor =
                        Some(Protractor { center: face.center, normal: face.normal, from: Some(face.grips[g]), to: None });
                    overlay.snap = None;
                }
            }
            return;
        };
        overlay.guide = Some(Guide { from: op.base, to: op.base + op.offset, axis: op.axis });
        // Preview: every edge touching a moved vertex, with those ends moved. When copying,
        // only the copied geometry moves.
        for (_, edge) in model.edges() {
            let (a_moves, b_moves) = (op.vertices.contains(&edge.a), op.vertices.contains(&edge.b));
            if !(a_moves || b_moves) || (self.copy && !(a_moves && b_moves)) {
                continue;
            }
            let [a, b] = model.edge_points(edge);
            let shift = |p: DVec3, moves: bool| if moves { p + op.offset } else { p };
            overlay.lines.push([shift(a, a_moves), shift(b, b_moves)]);
        }
    }

    fn start(&mut self, doc: &Document, screen: Screen<'_>, cursor: Vec2, dragging: bool) -> Result<(), String> {
        let model = doc.model();
        let entities: Vec<Entity> = if doc.selection().is_empty() {
            match pick_entity(model, screen, cursor) {
                // A face moves with its edges.
                Some(Entity::Face(id)) => std::iter::once(Entity::Face(id))
                    .chain(model.face_edges(id).into_iter().map(Entity::Edge))
                    .collect(),
                Some(entity) => vec![entity],
                None => return Err("Select something to move, or start on it".into()),
            }
        } else {
            doc.selection().iter().collect()
        };
        let Some(base) = crate::pick::snap(model, screen, cursor, None).map(|s| s.point) else {
            return Ok(());
        };
        let vertices = model.entity_vertices(entities.iter().copied());
        self.last = None;
        self.hover = None;
        self.op = Some(Op { entities, vertices, base, offset: DVec3::ZERO, axis: self.lock, dragging });
        Ok(())
    }

    /// Where the cursor puts the moved base point: locked axis, else a snapped endpoint, else
    /// an inferred axis, else the horizontal plane through the base point.
    fn target(&self, model: &Model, screen: Screen<'_>, cursor: Vec2) -> (DVec3, Option<Axis>, Option<Snap>) {
        let op = self.op.as_ref().expect("only called while moving");
        let base = op.base;
        let ray = screen.ray(cursor);
        // Don't snap to the geometry being moved, unless copying (the original stays put).
        let snap = snap_feature(model, screen, cursor, |v| !self.copy && op.vertices.contains(&v));

        if let Some(axis) = self.lock {
            let dir = axis.dir();
            let t = match &snap {
                Some(s) => Some((s.point - base).dot(dir)),
                None => closest_on_line(base, dir, &ray),
            };
            return (base + dir * t.unwrap_or(op.offset.dot(dir)), Some(axis), snap);
        }
        if let Some(s) = snap {
            return (s.point, None, Some(s));
        }
        let inferred = Axis::ALL
            .into_iter()
            .filter_map(|axis| {
                let point = base + axis.dir() * closest_on_line(base, axis.dir(), &ray)?;
                let distance = screen.project(point)?.distance(cursor);
                (distance < AXIS_RADIUS).then_some((distance, point, axis))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0));
        if let Some((_, point, axis)) = inferred {
            return (point, Some(axis), None);
        }
        let free = ray
            .hit_plane(base, DVec3::Z)
            .or_else(|| ray.hit_plane(base, -screen.camera.forward().as_dvec3()))
            .unwrap_or(base + op.offset);
        (free, None, None)
    }

    fn finish(&mut self, doc: &mut Document, typed: Option<DVec3>) -> Result<(), String> {
        let Some(op) = &self.op else { return Ok(()) };
        let offset = typed.unwrap_or(op.offset);
        if offset.length() < EPS {
            return Err("Move the mouse away from the base point first".into());
        }
        let done = Done {
            entities: op.entities.clone(),
            vertices: op.vertices.clone(),
            offset,
            copy: self.copy.then_some(Array { count: 1, divide: false }),
            model_version: 0,
        };
        self.apply(doc, done)?;
        self.op = None;
        self.snap = None;
        // Copy mode lasts one move, as in SketchUp; `3x` or `/3` can still extend this copy.
        self.copy = false;
        Ok(())
    }

    /// Undo the last move and redo it changed by `change`; restores it if that fails.
    fn redo_last(&mut self, doc: &mut Document, last: Done, change: impl FnOnce(&mut Done)) -> Result<(), String> {
        let mut changed = last.clone();
        change(&mut changed);
        doc.undo();
        match self.apply(doc, changed) {
            Ok(()) => Ok(()),
            Err(e) => {
                doc.redo();
                self.last = Some(Done { model_version: doc.model_version(), ..last });
                Err(e)
            }
        }
    }

    /// Move or copy as one undo step; on failure the document is untouched.
    fn apply(&mut self, doc: &mut Document, mut done: Done) -> Result<(), String> {
        let mut trial = doc.model().clone();
        let copies = match done.copy {
            None => {
                trial.move_vertices(&done.vertices, done.offset).map_err(|e| e.to_string())?;
                None
            }
            Some(Array { count, divide }) => {
                let step = if divide { done.offset / f64::from(count) } else { done.offset };
                let mut all = Vec::new();
                for i in 1..=count {
                    all.extend(trial.copy_entities(&done.entities, step * f64::from(i)));
                }
                Some(all)
            }
        };
        doc.edit(|model| *model = trial);
        // SketchUp selects what was copied.
        if let Some(copies) = copies {
            let selection = doc.selection_mut();
            selection.clear();
            selection.extend(copies);
        }
        done.model_version = doc.model_version();
        self.last = Some(done);
        Ok(())
    }
}

/// What a grip rotates and how.
pub struct GripRotation {
    pub entities: Vec<Entity>,
    pub vertices: BTreeSet<VertexId>,
    pub center: DVec3,
    pub normal: DVec3,
    pub from: DVec3,
}

/// Grips for the bounding box of `entities`: on each box face that faces the camera, four
/// grips halfway between the face's center and its sides. Flat boxes keep only their two big
/// faces (one of them visible).
fn grips_for(model: &Model, screen: Screen<'_>, entities: Vec<Entity>) -> Option<Grips> {
    if entities.is_empty() {
        return None;
    }
    let vertices = model.entity_vertices(entities.iter().copied());
    let (lo, hi) = vertices
        .iter()
        .map(|&v| model.vertex(v))
        .fold((DVec3::INFINITY, DVec3::NEG_INFINITY), |(lo, hi), p| (lo.min(p), hi.max(p)));
    let (center, half) = ((lo + hi) * 0.5, (hi - lo) * 0.5);
    let eye = screen.eye();
    let mut faces = Vec::new();
    for axis in Axis::ALL {
        let n = axis.dir();
        // The two in-plane axes of the box faces perpendicular to `axis`.
        let (u, v) = match axis {
            Axis::X => (DVec3::Y, DVec3::Z),
            Axis::Y => (DVec3::X, DVec3::Z),
            Axis::Z => (DVec3::X, DVec3::Y),
        };
        let (hu, hv) = (half.dot(u), half.dot(v));
        if hu < EPS || hv < EPS {
            continue;
        }
        for side in [1.0, -1.0] {
            let normal = n * side;
            let face_center = center + normal * half.dot(n);
            if (eye - face_center).dot(normal) <= 0.0 {
                continue;
            }
            let grips = [
                face_center + u * hu * 0.5,
                face_center - u * hu * 0.5,
                face_center + v * hv * 0.5,
                face_center - v * hv * 0.5,
            ];
            faces.push(GripFace { center: face_center, normal, grips });
        }
    }
    Some(Grips { entities, vertices, faces, hovered: None })
}

fn grip_at(grips: &Grips, screen: Screen<'_>, cursor: Vec2) -> Option<(usize, usize)> {
    grips
        .faces
        .iter()
        .enumerate()
        .flat_map(|(f, face)| face.grips.iter().enumerate().map(move |(g, &p)| (f, g, p)))
        .filter_map(|(f, g, p)| Some((screen.project(p)?.distance(cursor), f, g)))
        .filter(|(d, ..)| *d <= GRIP_RADIUS)
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, f, g)| (f, g))
}

/// A typed move: a length along `current`'s direction (negative reverses), a relative vector
/// `x,y,z` or `<x,y,z>`, or an absolute position `[x,y,z]` relative to `base`.
fn parse_offset(typed: &str, current: DVec3, base: DVec3) -> Result<DVec3, String> {
    let t = typed.trim();
    let vector = |inner: &str| -> Result<DVec3, String> {
        let values = parse_lengths(inner).map_err(|e| e.to_string())?;
        let [x, y, z] = values[..] else { return Err("Enter a vector as x,y,z".into()) };
        Ok(DVec3::new(x.unwrap_or(0.0), y.unwrap_or(0.0), z.unwrap_or(0.0)))
    };
    if let Some(inner) = t.strip_prefix('[').and_then(|r| r.strip_suffix(']')) {
        return Ok(vector(inner)? - base);
    }
    if let Some(inner) = t.strip_prefix('<').and_then(|r| r.strip_suffix('>')) {
        return vector(inner);
    }
    if t.contains([',', ';']) {
        return vector(t);
    }
    let length = parse_length(t).map_err(|e| e.to_string())?;
    if current.length() < EPS {
        return Err("Move the mouse to show the direction, then type the distance".into());
    }
    Ok(current.normalize() * length)
}

#[cfg(test)]
mod tests {
    use sketchcraft_view::{OrbitCamera, StandardView};

    use super::*;

    const SIZE: Vec2 = Vec2::new(1000.0, 800.0);

    /// An 8' 2x4 standing at the origin, fully selected, seen from the front-left.
    fn setup() -> (Document, OrbitCamera) {
        let mut doc = Document::default();
        doc.edit(|m| {
            let face = m.add_rectangle(DVec3::ZERO, DVec3::X * 1.5, DVec3::Y * 3.5, DVec3::Z).unwrap();
            m.push_pull(face, 96.0).unwrap();
        });
        doc.select_all();
        let mut camera = OrbitCamera { target: glam::Vec3::new(10.0, 2.0, 0.0), distance: 120.0, ..Default::default() };
        camera.set_view(StandardView::Iso);
        (doc, camera)
    }

    fn at(screen: Screen<'_>, p: DVec3) -> Vec2 {
        screen.project(p).unwrap()
    }

    fn bounds(doc: &Document) -> (DVec3, DVec3) {
        doc.model().bounds().unwrap()
    }

    #[test]
    fn move_along_red_by_typing() {
        let (mut doc, camera) = setup();
        let screen = Screen { camera: &camera, size: SIZE };
        let mut tool = MoveTool::default();
        tool.click(&mut doc, screen, at(screen, DVec3::ZERO)).unwrap();
        tool.hover(&doc, screen, at(screen, DVec3::X * 20.0));
        assert_eq!(tool.op.as_ref().unwrap().axis, Some(Axis::X));
        tool.enter(&mut doc, "16").unwrap();
        assert_eq!(bounds(&doc), (DVec3::new(16.0, 0.0, 0.0), DVec3::new(17.5, 3.5, 96.0)));
        assert_eq!(doc.model().faces().count(), 6);
    }

    #[test]
    fn arrow_key_lock_moves_straight_up() {
        let (mut doc, camera) = setup();
        let screen = Screen { camera: &camera, size: SIZE };
        let mut tool = MoveTool::default();
        tool.lock_axis(Axis::Z);
        tool.click(&mut doc, screen, at(screen, DVec3::ZERO)).unwrap();
        // Even with the mouse off to the side, the lock keeps the move vertical.
        tool.hover(&doc, screen, at(screen, DVec3::new(30.0, -20.0, 10.0)));
        tool.enter(&mut doc, "12").unwrap();
        assert_eq!(bounds(&doc).0, DVec3::new(0.0, 0.0, 12.0));
    }

    #[test]
    fn vector_entry_moves_exactly() {
        let (mut doc, camera) = setup();
        let screen = Screen { camera: &camera, size: SIZE };
        let mut tool = MoveTool::default();
        tool.click(&mut doc, screen, at(screen, DVec3::ZERO)).unwrap();
        tool.enter(&mut doc, "<3,4,5>").unwrap();
        assert_eq!(bounds(&doc).0, DVec3::new(3.0, 4.0, 5.0));
        // Retyping changes that same move.
        tool.enter(&mut doc, "1,0,0").unwrap();
        assert_eq!(bounds(&doc).0, DVec3::new(1.0, 0.0, 0.0));
        doc.undo();
        assert_eq!(bounds(&doc).0, DVec3::ZERO);
    }

    #[test]
    fn ctrl_copy_then_array_makes_a_row_of_studs() {
        let (mut doc, camera) = setup();
        let screen = Screen { camera: &camera, size: SIZE };
        let mut tool = MoveTool::default();
        tool.toggle_copy();
        tool.click(&mut doc, screen, at(screen, DVec3::ZERO)).unwrap();
        tool.hover(&doc, screen, at(screen, DVec3::X * 20.0));
        tool.enter(&mut doc, "16").unwrap();
        assert_eq!(doc.model().faces().count(), 12);
        // The copy is selected, like SketchUp.
        assert_eq!(doc.selection().len(), 18);
        tool.enter(&mut doc, "4x").unwrap();
        // Original plus four copies, 16" on center.
        assert_eq!(doc.model().faces().count(), 30);
        assert_eq!(bounds(&doc).1.x, 64.0 + 1.5);
        assert_eq!(tool.measurement(), "4x");
        doc.undo();
        assert_eq!(doc.model().faces().count(), 6);
    }

    #[test]
    fn divide_spreads_copies_evenly() {
        let (mut doc, camera) = setup();
        let screen = Screen { camera: &camera, size: SIZE };
        let mut tool = MoveTool::default();
        tool.toggle_copy();
        tool.click(&mut doc, screen, at(screen, DVec3::ZERO)).unwrap();
        tool.hover(&doc, screen, at(screen, DVec3::X * 20.0));
        tool.enter(&mut doc, "48").unwrap();
        tool.enter(&mut doc, "/3").unwrap();
        // Corners of the four boards' bottom-front edges, in half inches (boards are 1.5" wide).
        let mut half_inches: Vec<i64> = doc
            .model()
            .vertices()
            .filter(|(_, p)| p.y == 0.0 && p.z == 0.0)
            .map(|(_, p)| (p.x * 2.0).round() as i64)
            .collect();
        half_inches.sort();
        half_inches.dedup();
        assert_eq!(half_inches, [0, 3, 32, 35, 64, 67, 96, 99]);
    }

    #[test]
    fn nothing_selected_moves_the_face_under_the_cursor() {
        let (mut doc, camera) = setup();
        doc.selection_mut().clear();
        let screen = Screen { camera: &camera, size: SIZE };
        let mut tool = MoveTool::default();
        // The board's long left face (x = 0), big enough on screen to hit away from its edges.
        let side = DVec3::new(0.0, 1.75, 48.0);
        tool.hover(&doc, screen, at(screen, side));
        assert!(matches!(tool.hover, Some(Entity::Face(_))), "{:?}", tool.hover);
        tool.lock_axis(Axis::X);
        tool.click(&mut doc, screen, at(screen, side)).unwrap();
        tool.hover(&doc, screen, at(screen, side - DVec3::X * 5.0));
        tool.enter(&mut doc, "4").unwrap();
        // Only that face moved: the board is now 5.5" wide.
        assert_eq!(bounds(&doc), (DVec3::new(-4.0, 0.0, 0.0), DVec3::new(1.5, 3.5, 96.0)));
    }

    #[test]
    fn bending_move_is_refused_and_tool_stays_ready() {
        let (mut doc, camera) = setup();
        let screen = Screen { camera: &camera, size: SIZE };
        let corner = doc
            .model()
            .edges()
            .find(|(_, e)| {
                let [a, b] = doc.model().edge_points(e);
                a.z == 96.0 && b.z == 96.0
            })
            .map(|(id, _)| Entity::Edge(id))
            .unwrap();
        let selection = doc.selection_mut();
        selection.clear();
        selection.extend([corner]);
        let version = doc.model_version();
        let mut tool = MoveTool::default();
        // Sliding the top front edge along red twists the x = 0 end face.
        tool.lock_axis(Axis::X);
        tool.click(&mut doc, screen, at(screen, DVec3::new(0.0, 0.0, 96.0))).unwrap();
        tool.hover(&doc, screen, at(screen, DVec3::new(4.0, 0.0, 96.0)));
        assert!(tool.enter(&mut doc, "1").unwrap_err().contains("bend"));
        assert_eq!(doc.model_version(), version);
        assert!(tool.is_busy());
    }

    #[test]
    fn array_parsing() {
        assert_eq!(parse_array("3x"), Some(Array { count: 3, divide: false }));
        assert_eq!(parse_array("x3"), Some(Array { count: 3, divide: false }));
        assert_eq!(parse_array("*3"), Some(Array { count: 3, divide: false }));
        assert_eq!(parse_array("/4"), Some(Array { count: 4, divide: true }));
        assert_eq!(parse_array("16"), None);
        assert_eq!(parse_array("0x"), None);
    }
}
