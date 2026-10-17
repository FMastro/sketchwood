//! The Rotate tool (Q). Rotates the selection, or whatever is under the cursor when nothing is
//! selected, with SketchUp's protractor:
//!
//! 1. Click the center. The protractor lies on the face under the cursor, or the ground; arrow
//!    keys lock it to an axis (Right = red, Left = green, Up/Down = blue).
//! 2. Click a point to set the starting direction.
//! 3. Click to finish, or type an angle in degrees. Angles snap to 15° steps.
//!
//! Tap Ctrl to copy; after a copy, `3x` makes 3 rotated copies and `/3` spreads 3 evenly.
//! Typing right after finishing changes the rotation just made.

use std::collections::BTreeSet;

use glam::{DAffine3, DVec3, Vec2};
use woodsketch_doc::{Document, EPS, Entity, Model, VertexId, rotation};

use crate::pick::{Screen, Snap, hit_face, snap, snap_feature};
use crate::{Array, Axis, Overlay, Protractor, highlight_entity, parse_array, pick_resolved};

/// Angles snap to multiples of this when within [`ANGLE_SNAP_TOLERANCE`].
const ANGLE_STEP: f64 = 15.0;
const ANGLE_SNAP_TOLERANCE: f64 = 2.5;

#[derive(Default)]
pub struct RotateTool {
    op: Option<Op>,
    hover: Option<Entity>,
    snap: Option<Snap>,
    /// Protractor orientation locked with the arrow keys.
    lock: Option<Axis>,
    /// Where the protractor would go, while idle.
    preview: Option<(DVec3, DVec3)>,
    copy: bool,
    last: Option<Done>,
}

struct Op {
    entities: Vec<Entity>,
    vertices: BTreeSet<VertexId>,
    center: DVec3,
    normal: DVec3,
    stage: Stage,
    /// The axis the direction under the cursor snapped to, and its direction in the plane.
    axis: Option<(Axis, DVec3)>,
    /// How far the starting point was from the center (sizes the axis guide lines).
    reach: f64,
}

enum Stage {
    /// Picking the starting direction; holds the point under the cursor.
    Start(Option<DVec3>),
    /// Rotating from `from` (a unit vector in the plane); `angle` in degrees.
    Rotate { from: DVec3, angle: f64 },
}

#[derive(Clone)]
struct Done {
    entities: Vec<Entity>,
    center: DVec3,
    normal: DVec3,
    angle: f64,
    copy: Option<Array>,
    model_version: u64,
}

impl RotateTool {
    pub fn is_busy(&self) -> bool {
        self.op.is_some()
    }

    pub fn cancel(&mut self) {
        self.op = None;
        self.snap = None;
        self.lock = None;
        self.last = None;
    }

    pub fn toggle_copy(&mut self) {
        self.copy = !self.copy;
    }

    pub fn lock_axis(&mut self, axis: Axis) {
        self.lock = if self.lock == Some(axis) { None } else { Some(axis) };
        // Re-orient a protractor that hasn't started turning yet.
        if let Some(op) = &mut self.op
            && matches!(op.stage, Stage::Start(_))
        {
            op.normal = self.lock.map_or(op.normal, Axis::dir);
        }
    }

    /// Start turning right away about the line through `center` along `normal`, measuring from
    /// the unit direction `from`. Used by Move's rotation grips.
    pub fn begin(
        &mut self,
        entities: Vec<Entity>,
        vertices: BTreeSet<VertexId>,
        center: DVec3,
        normal: DVec3,
        from: DVec3,
    ) {
        self.last = None;
        self.hover = None;
        self.preview = None;
        self.snap = None;
        self.op = Some(Op { entities, vertices, center, normal, stage: Stage::Rotate { from, angle: 0.0 }, axis: None, reach: 0.0 });
    }

    pub fn hover(&mut self, doc: &Document, screen: Screen<'_>, cursor: Vec2) {
        let model = doc.view().model();
        let Some(op) = &mut self.op else {
            self.hover = if doc.selection().is_empty() { pick_resolved(doc, screen, cursor) } else { None };
            self.snap = snap(model, screen, cursor, None);
            self.preview = self.snap.map(|s| (s.point, self.lock.map_or_else(|| surface_normal(model, screen, cursor), Axis::dir)));
            return;
        };
        let exclude = |v: VertexId| !self.copy && op.vertices.contains(&v);
        self.snap = snap_feature(model, screen, cursor, exclude);
        let point = match &self.snap {
            // Features off the protractor's plane count where they project onto it.
            Some(s) => Some(s.point - op.normal * (s.point - op.center).dot(op.normal)),
            None => screen.ray(cursor).hit_plane(op.center, op.normal),
        };
        // Near a red, green or blue direction in the protractor's plane (and not on a feature):
        // snap to it.
        op.axis = None;
        let axis = point.filter(|_| self.snap.is_none()).and_then(|p| near_axis(p - op.center, op.normal));
        match &mut op.stage {
            Stage::Start(start) => {
                *start = match (point, axis) {
                    (Some(p), Some((a, dir))) => {
                        op.axis = Some((a, dir));
                        Some(op.center + dir * (p - op.center).dot(dir))
                    }
                    _ => point,
                };
            }
            Stage::Rotate { from, angle } => {
                if let Some(point) = point
                    && (point - op.center).length() > EPS
                {
                    let to = axis.map_or_else(|| (point - op.center).normalize(), |(_, dir)| dir);
                    let raw = from.cross(to).dot(op.normal).atan2(from.dot(to)).to_degrees();
                    *angle = if axis.is_some() { raw } else { snap_angle(raw) };
                    op.axis = axis;
                }
            }
        }
    }

    pub fn click(&mut self, doc: &mut Document, screen: Screen<'_>, cursor: Vec2) -> Result<(), String> {
        self.hover(doc, screen, cursor);
        let Some(op) = &mut self.op else { return self.start(doc, screen, cursor) };
        match op.stage {
            Stage::Start(start) => {
                let Some(start) = start.filter(|s| (*s - op.center).length() > EPS) else {
                    return Err("Pick a point away from the center".into());
                };
                op.reach = (start - op.center).length();
                op.stage = Stage::Rotate { from: (start - op.center).normalize(), angle: 0.0 };
                Ok(())
            }
            Stage::Rotate { angle, .. } => self.finish(doc, angle),
        }
    }

    /// Press-drag-release sets the center and starting direction in one motion.
    pub fn drag_start(&mut self, doc: &mut Document, screen: Screen<'_>, origin: Vec2) -> Result<(), String> {
        if self.op.is_none() {
            self.hover(doc, screen, origin);
            return self.start(doc, screen, origin);
        }
        Ok(())
    }

    pub fn drag_end(&mut self, doc: &mut Document, screen: Screen<'_>, cursor: Vec2) -> Result<(), String> {
        match self.op.as_ref().map(|op| &op.stage) {
            Some(Stage::Start(_)) => self.click(doc, screen, cursor),
            _ => Ok(()),
        }
    }

    pub fn enter(&mut self, doc: &mut Document, typed: &str) -> Result<(), String> {
        let typed = typed.trim();
        if let Some(array) = parse_array(typed) {
            let Some(last) = self.last.take().filter(|l| l.model_version == doc.model_version()) else {
                return Err("Rotate a copy first (tap Ctrl while rotating), then type 3x or /3".into());
            };
            if last.copy.is_none() {
                self.last = Some(last);
                return Err("Arrays need a copy: tap Ctrl while rotating".into());
            }
            return self.redo_last(doc, last, |done| done.copy = Some(array));
        }
        let degrees = parse_degrees(typed)?;
        // Typed angles turn the way the mouse went; negative values reverse it.
        let directed = |current: f64| if current < 0.0 { -degrees } else { degrees };
        if let Some(op) = &self.op {
            return match op.stage {
                Stage::Rotate { angle, .. } => self.finish(doc, directed(angle)),
                Stage::Start(_) => Err("Pick the starting direction, then type the angle".into()),
            };
        }
        match self.last.take().filter(|l| l.model_version == doc.model_version()) {
            Some(last) => {
                let angle = directed(last.angle);
                self.redo_last(doc, last, |done| done.angle = angle)
            }
            None => Err("Pick a center and a starting direction, then type the angle".into()),
        }
    }

    pub fn status(&self) -> String {
        let copy = if self.copy { " (copying)" } else { "" };
        match self.op.as_ref().map(|op| &op.stage) {
            None => format!(
                "Pick the center of rotation{copy}. Arrow keys lock the protractor (Right red, Left green, \
                 Up blue); tap Ctrl to copy."
            ),
            Some(Stage::Start(_)) => format!("Pick a point to start the rotation{copy}."),
            Some(Stage::Rotate { .. }) => format!("Pick the end of the rotation, or enter an angle{copy}."),
        }
    }

    pub fn measurement(&self) -> String {
        if let Some(Op { stage: Stage::Rotate { angle, .. }, .. }) = &self.op {
            return format_degrees(*angle);
        }
        match &self.last {
            Some(Done { copy: Some(Array { count, divide }), .. }) if *count > 1 => {
                if *divide { format!("/{count}") } else { format!("{count}x") }
            }
            Some(last) => format_degrees(last.angle),
            None => String::new(),
        }
    }

    pub fn overlay(&self, doc: &Document, overlay: &mut Overlay) {
        let model = doc.view().model();
        overlay.snap = self.snap;
        let Some(op) = &self.op else {
            if let Some(entity) = self.hover {
                highlight_entity(doc.view(), entity, overlay);
            }
            overlay.protractor =
                self.preview.map(|(center, normal)| Protractor { center, normal, from: None, to: None });
            return;
        };
        let (from, to) = match op.stage {
            Stage::Start(start) => (start, None),
            Stage::Rotate { from, angle } => {
                let turn = rotation(op.center, op.normal, angle.to_radians());
                let reach = op.center + from;
                (Some(reach), Some(turn(reach)))
            }
        };
        overlay.protractor = Some(Protractor { center: op.center, normal: op.normal, from, to });
        // The axes through the center, and the one the direction snapped to, in their colors.
        let reach = match op.stage {
            Stage::Start(Some(p)) => (p - op.center).length(),
            _ => op.reach,
        };
        if reach > EPS {
            for (axis, dir) in plane_axes(op.normal).into_iter().step_by(2) {
                overlay.axis_lines.push((op.center - dir * reach * 1.5, op.center + dir * reach * 1.5, axis));
            }
            if let Some((axis, dir)) = op.axis {
                overlay.guide = Some(crate::Guide { from: op.center, to: op.center + dir * reach, along: Some(crate::infer::Along::Axis(axis)) });
            }
        }
        if let Stage::Rotate { angle, .. } = op.stage {
            let turn = rotation(op.center, op.normal, angle.to_radians());
            for (_, edge) in model.edges() {
                let (a_moves, b_moves) = (op.vertices.contains(&edge.a), op.vertices.contains(&edge.b));
                if !(a_moves || b_moves) || (self.copy && !(a_moves && b_moves)) {
                    continue;
                }
                let [a, b] = model.edge_points(edge);
                let place = |p: DVec3, moves: bool| if moves { turn(p) } else { p };
                overlay.lines.push([place(a, a_moves), place(b, b_moves)]);
            }
        }
    }

    fn start(&mut self, doc: &Document, screen: Screen<'_>, cursor: Vec2) -> Result<(), String> {
        let model = doc.model();
        let entities: Vec<Entity> = if doc.selection().is_empty() {
            match pick_resolved(doc, screen, cursor) {
                Some(Entity::Face(id)) => std::iter::once(Entity::Face(id))
                    .chain(model.face_edges(id).into_iter().map(Entity::Edge))
                    .collect(),
                Some(entity) => vec![entity],
                None => return Err("Select something to rotate, or start on it".into()),
            }
        } else {
            doc.selection().iter().collect()
        };
        let Some(center) = self.snap.map(|s| s.point) else { return Ok(()) };
        let normal = self.lock.map_or_else(|| surface_normal(doc.view().model(), screen, cursor), Axis::dir);
        let vertices = doc.view().vertices_of(&entities);
        self.last = None;
        self.hover = None;
        self.preview = None;
        self.op = Some(Op { entities, vertices, center, normal, stage: Stage::Start(None), axis: None, reach: 0.0 });
        Ok(())
    }

    fn finish(&mut self, doc: &mut Document, angle: f64) -> Result<(), String> {
        let Some(op) = &self.op else { return Ok(()) };
        if angle.abs() < 1e-9 {
            return Err("Turn away from the starting direction first".into());
        }
        let done = Done {
            entities: op.entities.clone(),
            center: op.center,
            normal: op.normal,
            angle,
            copy: self.copy.then_some(Array { count: 1, divide: false }),
            model_version: 0,
        };
        self.apply(doc, done)?;
        self.op = None;
        self.snap = None;
        self.copy = false;
        Ok(())
    }

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

    /// Rotate or copy as one undo step; on failure the document is untouched.
    fn apply(&mut self, doc: &mut Document, mut done: Done) -> Result<(), String> {
        let radians = done.angle.to_radians();
        let turn = |angle: f64| {
            DAffine3::from_translation(done.center)
                * DAffine3::from_axis_angle(done.normal.normalize(), angle)
                * DAffine3::from_translation(-done.center)
        };
        let transforms: Vec<DAffine3> = match done.copy {
            None => vec![turn(radians)],
            Some(Array { count, divide }) => {
                let step = if divide { radians / f64::from(count) } else { radians };
                (1..=count).map(|i| turn(step * f64::from(i))).collect()
            }
        };
        let copies = doc.transform_entities(&done.entities, &transforms, done.copy.is_some()).map_err(|e| e.to_string())?;
        if done.copy.is_some() {
            let selection = doc.selection_mut();
            selection.clear();
            selection.extend(copies);
        }
        done.model_version = doc.model_version();
        self.last = Some(done);
        Ok(())
    }
}

/// The normal of the face under the cursor, or the ground's.
fn surface_normal(model: &Model, screen: Screen<'_>, cursor: Vec2) -> DVec3 {
    hit_face(model, &screen.ray(cursor)).and_then(|(id, _)| model.face(id)).map_or(DVec3::Z, |f| f.normal)
}

/// Snap to the nearest multiple of [`ANGLE_STEP`] degrees when close to it.
/// Directions within this many degrees of a red, green or blue axis snap to it.
const AXIS_SNAP_DEGREES: f64 = 4.0;

/// The model axes lying in the plane with `normal`, both ways along each.
fn plane_axes(normal: DVec3) -> Vec<(Axis, DVec3)> {
    Axis::ALL
        .into_iter()
        .filter_map(|a| {
            let d = a.dir() - normal * a.dir().dot(normal);
            (d.length() > 0.999).then(|| (a, d.normalize()))
        })
        .flat_map(|(a, d)| [(a, d), (a, -d)])
        .collect()
}

/// The in-plane axis `dir` (from the center) is within [`AXIS_SNAP_DEGREES`] of.
fn near_axis(dir: DVec3, normal: DVec3) -> Option<(Axis, DVec3)> {
    let dir = dir.normalize_or_zero();
    let limit = AXIS_SNAP_DEGREES.to_radians().cos();
    plane_axes(normal).into_iter().filter(|(_, d)| d.dot(dir) >= limit).max_by(|a, b| a.1.dot(dir).total_cmp(&b.1.dot(dir)))
}

fn snap_angle(degrees: f64) -> f64 {
    let nearest = (degrees / ANGLE_STEP).round() * ANGLE_STEP;
    if (degrees - nearest).abs() <= ANGLE_SNAP_TOLERANCE { nearest } else { degrees }
}

fn parse_degrees(typed: &str) -> Result<f64, String> {
    let number = typed.trim().trim_end_matches('°').trim_end_matches("deg").trim();
    match number.parse::<f64>() {
        Ok(d) if d.is_finite() => Ok(d),
        _ => Err(format!("Invalid angle: \"{}\"", typed.trim())),
    }
}

/// SketchUp shows angles with one decimal, like `90.0°`.
fn format_degrees(degrees: f64) -> String {
    format!("{:.1}°", degrees.abs())
}

#[cfg(test)]
mod tests {
    use woodsketch_view::{OrbitCamera, StandardView};

    use super::*;

    const SIZE: Vec2 = Vec2::new(1000.0, 800.0);

    /// A 1.5" x 3.5" profile pulled 10" up at (10, 0), selected, seen from the top.
    fn setup() -> (Document, OrbitCamera) {
        let mut doc = Document::default();
        doc.edit(|m| {
            let face = m.add_rectangle(DVec3::new(10.0, 0.0, 0.0), DVec3::X * 1.5, DVec3::Y * 3.5, DVec3::Z).unwrap();
            m.push_pull(face, 10.0).unwrap();
        });
        doc.select_all();
        let mut camera = OrbitCamera { target: glam::Vec3::ZERO, distance: 80.0, ..Default::default() };
        camera.set_view(StandardView::Top);
        (doc, camera)
    }

    fn at(screen: Screen<'_>, x: f64, y: f64) -> Vec2 {
        screen.project(DVec3::new(x, y, 0.0)).unwrap()
    }

    /// Center at the origin, start along +X, then hover at `(x, y)`.
    fn turn_toward(tool: &mut RotateTool, doc: &mut Document, screen: Screen<'_>, x: f64, y: f64) {
        tool.click(doc, screen, at(screen, 0.0, 0.0)).unwrap();
        tool.click(doc, screen, at(screen, 6.0, 0.0)).unwrap();
        tool.hover(doc, screen, at(screen, x, y));
    }

    fn bounds(doc: &Document) -> (DVec3, DVec3) {
        doc.model().bounds().unwrap()
    }

    #[test]
    fn directions_snap_to_the_axes_with_guide_lines() {
        let (mut doc, camera) = setup();
        let screen = Screen::new(&camera, SIZE);
        let mut tool = RotateTool::default();
        tool.click(&mut doc, screen, at(screen, 0.0, 0.0)).unwrap();
        // Picking the start a hair off the red axis lands on it.
        tool.hover(&doc, screen, at(screen, 6.0, 0.3));
        let mut overlay = Overlay::default();
        tool.overlay(&doc, &mut overlay);
        let from = overlay.protractor.unwrap().from.unwrap();
        assert!(from.y.abs() < 1e-9 && (from.x - 6.0).abs() < 1e-3, "on the red axis: {from}");
        assert_eq!(overlay.axis_lines.len(), 2, "red and green lie in the ground's plane");
        // Start off-axis (about 9.5 degrees), then turn to just past the green axis: the angle
        // snaps so the start lands exactly on green (not to a 15 degree step).
        tool.click(&mut doc, screen, at(screen, 6.0, 1.0)).unwrap();
        tool.hover(&doc, screen, at(screen, -0.3, 6.0));
        let mut overlay = Overlay::default();
        tool.overlay(&doc, &mut overlay);
        assert_eq!(overlay.guide.and_then(|g| g.along), Some(crate::infer::Along::Axis(Axis::Y)));
        let to = overlay.protractor.unwrap().to.unwrap();
        assert!(to.x.abs() < 1e-9 && to.y > 0.0, "the start direction turned onto green: {to}");
    }

    #[test]
    fn quarter_turn_by_clicking_snaps_to_ninety() {
        let (mut doc, camera) = setup();
        let screen = Screen::new(&camera, SIZE);
        let mut tool = RotateTool::default();
        // Slightly off 90°: within the 15° snap tolerance.
        turn_toward(&mut tool, &mut doc, screen, -0.2, 6.0);
        assert_eq!(tool.measurement(), "90.0°");
        tool.click(&mut doc, screen, at(screen, -0.2, 6.0)).unwrap();
        let (lo, hi) = bounds(&doc);
        assert!(lo.abs_diff_eq(DVec3::new(-3.5, 10.0, 0.0), 1e-9), "{lo}");
        assert!(hi.abs_diff_eq(DVec3::new(0.0, 11.5, 10.0), 1e-9), "{hi}");
    }

    #[test]
    fn typed_angle_follows_the_mouse_and_can_be_retyped() {
        let (mut doc, camera) = setup();
        let screen = Screen::new(&camera, SIZE);
        let mut tool = RotateTool::default();
        // Mouse clockwise (toward -Y): typed 90 turns clockwise.
        turn_toward(&mut tool, &mut doc, screen, 4.0, -3.0);
        tool.enter(&mut doc, "90").unwrap();
        let (lo, _) = bounds(&doc);
        assert!(lo.abs_diff_eq(DVec3::new(0.0, -11.5, 0.0), 1e-9), "{lo}");
        tool.enter(&mut doc, "180").unwrap();
        let (lo, hi) = bounds(&doc);
        assert!(lo.abs_diff_eq(DVec3::new(-11.5, -3.5, 0.0), 1e-9), "{lo}");
        assert!(hi.abs_diff_eq(DVec3::new(-10.0, 0.0, 10.0), 1e-9), "{hi}");
        doc.undo();
        assert_eq!(bounds(&doc).0, DVec3::new(10.0, 0.0, 0.0));
    }

    #[test]
    fn rotated_copy_array_goes_around() {
        let (mut doc, camera) = setup();
        let screen = Screen::new(&camera, SIZE);
        let mut tool = RotateTool::default();
        tool.toggle_copy();
        turn_toward(&mut tool, &mut doc, screen, 0.0, 6.0);
        tool.enter(&mut doc, "90").unwrap();
        tool.enter(&mut doc, "3x").unwrap();
        // Original plus copies at 90°, 180° and 270°: four boards around the origin.
        assert_eq!(doc.model().faces().count(), 24);
        let (lo, hi) = bounds(&doc);
        assert!(lo.abs_diff_eq(DVec3::new(-11.5, -11.5, 0.0), 1e-9), "{lo}");
        assert!(hi.abs_diff_eq(DVec3::new(11.5, 11.5, 10.0), 1e-9), "{hi}");
    }

    #[test]
    fn locked_red_axis_lays_a_board_down() {
        let (mut doc, _) = setup();
        let mut camera = OrbitCamera { target: glam::Vec3::new(10.0, 0.0, 5.0), distance: 80.0, ..Default::default() };
        camera.set_view(StandardView::Right);
        let screen = Screen::new(&camera, SIZE);
        let mut tool = RotateTool::default();
        tool.lock_axis(Axis::X);
        let side = |y: f64, z: f64| screen.project(DVec3::new(10.0, y, z)).unwrap();
        // Center at the board's bottom corner (10, 0, 0); start straight up; turn toward -Y.
        tool.click(&mut doc, screen, side(0.0, 0.0)).unwrap();
        assert_eq!(tool.op.as_ref().unwrap().normal, DVec3::X);
        tool.click(&mut doc, screen, side(0.0, 6.0)).unwrap();
        tool.hover(&doc, screen, side(-6.0, 0.2));
        tool.enter(&mut doc, "90").unwrap();
        let (lo, hi) = bounds(&doc);
        assert!(lo.abs_diff_eq(DVec3::new(10.0, -10.0, 0.0), 1e-9), "{lo}");
        assert!(hi.abs_diff_eq(DVec3::new(11.5, 0.0, 3.5), 1e-9), "{hi}");
    }

    #[test]
    fn angle_snapping_and_parsing() {
        assert_eq!(snap_angle(44.0), 45.0);
        assert_eq!(snap_angle(-91.5), -90.0);
        assert_eq!(snap_angle(37.0), 37.0);
        assert_eq!(parse_degrees("45").unwrap(), 45.0);
        assert_eq!(parse_degrees("-22.5°").unwrap(), -22.5);
        assert!(parse_degrees("abc").is_err());
        assert_eq!(format_degrees(-90.0), "90.0°");
    }
}
