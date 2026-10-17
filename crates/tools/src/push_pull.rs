//! The Push/Pull tool (P). Click a face, move to set the distance, then click again (or
//! press-drag-release). Type a distance and Enter at any point; typing right after finishing
//! changes the last push/pull. Double-click a face to repeat the last distance.

use glam::{DVec3, Vec2};
use woodsketch_doc::units::{format_length, parse_length};
use woodsketch_doc::{Document, Entity, FaceId, Model, Owner};

use crate::Overlay;
use crate::pick::{Screen, Snap, closest_on_line, hit_face, snap_feature};

pub struct PushPullTool {
    /// Make a board made from a free-standing face into a group (on by default).
    pub auto_group: bool,
    op: Option<Op>,
    /// The face under the cursor while idle.
    hover: Option<FaceId>,
    /// An endpoint the distance is snapped to.
    snap: Option<Snap>,
    /// The push/pull just finished, which a typed distance can still change.
    last: Option<Done>,
    /// The last distance used, for double-click repeat.
    repeat: Option<f64>,
}

impl Default for PushPullTool {
    fn default() -> Self {
        Self { auto_group: true, op: None, hover: None, snap: None, last: None, repeat: None }
    }
}

struct Op {
    face: FaceId,
    /// Where the face was picked; the distance is measured from here along `normal`.
    anchor: DVec3,
    normal: DVec3,
    distance: f64,
    /// Started by press-and-drag; finishes on release.
    dragging: bool,
}

struct Done {
    face: FaceId,
    distance: f64,
    model_version: u64,
}

impl PushPullTool {
    pub fn is_busy(&self) -> bool {
        self.op.is_some()
    }

    pub fn cancel(&mut self) {
        self.op = None;
        self.last = None;
        self.snap = None;
    }

    pub fn hover(&mut self, doc: &Document, screen: Screen<'_>, cursor: Vec2) {
        let Some(op) = &mut self.op else {
            // Faces inside closed groups highlight too: clicking one opens its group first.
            let view = doc.view();
            self.hover = hit_face(view.model(), &screen.ray(cursor))
                .map(|(id, _)| id)
                .filter(|&id| view.owner(Entity::Face(id)) != Owner::Outside);
            return;
        };
        let (model, view) = (doc.model(), doc.view().model());
        // Corners, midpoints and edges elsewhere set the distance to their height, so faces can
        // be matched to other geometry. The face's own vertices don't count.
        let face = model.face(op.face);
        self.snap = snap_feature(view, screen, cursor, |v| face.is_some_and(|f| f.uses_vertex(v)));
        op.distance = match &self.snap {
            Some(s) => (s.point - op.anchor).dot(op.normal),
            None => closest_on_line(op.anchor, op.normal, &screen.ray(cursor)).unwrap_or(op.distance),
        };
    }

    pub fn click(&mut self, doc: &mut Document, screen: Screen<'_>, cursor: Vec2, count: u8) -> Result<(), String> {
        // The first click of a double-click started an operation; the second repeats the last
        // distance on that face instead.
        if count >= 2
            && let Some(repeat) = self.repeat
        {
            let face = match self.op.take() {
                Some(op) => Some(op.face),
                None => active_face(doc, screen, cursor)?.map(|(id, _)| id),
            };
            return match face {
                Some(face) => self.apply(doc, face, repeat),
                None => Ok(()),
            };
        }
        if self.op.is_some() {
            self.hover(doc, screen, cursor);
            self.finish(doc, None)
        } else {
            self.start(doc, screen, cursor, false)
        }
    }

    pub fn drag_start(&mut self, doc: &Document, screen: Screen<'_>, origin: Vec2) -> Result<(), String> {
        if self.op.is_none() {
            return self.start(doc, screen, origin, true);
        }
        Ok(())
    }

    pub fn drag_end(&mut self, doc: &mut Document, screen: Screen<'_>, cursor: Vec2) -> Result<(), String> {
        // Enter may already have finished it while the button was held.
        if !self.op.as_ref().is_some_and(|op| op.dragging) {
            return Ok(());
        }
        self.hover(doc, screen, cursor);
        self.finish(doc, None)
    }

    /// Apply a typed distance: to the push/pull in progress, or to the one just finished.
    pub fn enter(&mut self, doc: &mut Document, typed: &str) -> Result<(), String> {
        let typed = parse_length(typed).map_err(|e| e.to_string())?;
        // Typed distances follow the mouse's direction; negative values reverse it.
        let directed = |current: f64| if current < 0.0 { -typed } else { typed };
        if let Some(op) = &self.op {
            let distance = directed(op.distance);
            return self.finish(doc, Some(distance));
        }
        match self.last.take() {
            Some(last) if last.model_version == doc.model_version() => {
                let distance = directed(last.distance);
                doc.undo();
                let result = self.apply(doc, last.face, distance);
                if result.is_err() {
                    // Put the previous result back.
                    doc.redo();
                    self.last = Some(Done { model_version: doc.model_version(), ..last });
                }
                result
            }
            _ => Err("Click a face, then type the distance".into()),
        }
    }

    pub fn status(&self) -> &'static str {
        if self.op.is_some() {
            "Move to set the distance, then click, or enter a value."
        } else {
            "Pick a face to push or pull. Double-click a face to repeat the last distance."
        }
    }

    pub fn measurement(&self) -> String {
        match (&self.op, &self.last) {
            (Some(op), _) => format_length(op.distance.abs()),
            (None, Some(last)) => format_length(last.distance.abs()),
            (None, None) => String::new(),
        }
    }

    pub fn overlay(&self, model: &Model, overlay: &mut Overlay) {
        overlay.snap = self.snap;
        let face_id = self.op.as_ref().map(|op| op.face).or(self.hover);
        let Some(face) = face_id.and_then(|id| model.face(id)) else { return };
        let loops: Vec<Vec<DVec3>> = face.loops().map(|l| model.loop_points(l)).collect();
        for points in &loops {
            for (i, &a) in points.iter().enumerate() {
                overlay.highlight.push([a, points[(i + 1) % points.len()]]);
            }
        }
        // Preview the extrusion: the moved cap and the walls' edges.
        if let Some(op) = &self.op {
            let offset = op.normal * op.distance;
            for points in &loops {
                for (i, &a) in points.iter().enumerate() {
                    let b = points[(i + 1) % points.len()];
                    overlay.lines.push([a + offset, b + offset]);
                    overlay.lines.push([a, a + offset]);
                }
            }
        }
    }

    fn start(&mut self, doc: &Document, screen: Screen<'_>, cursor: Vec2, dragging: bool) -> Result<(), String> {
        let Some((face, anchor)) = active_face(doc, screen, cursor)? else { return Ok(()) };
        let normal = doc.model().face(face).expect("hit a live face").normal;
        self.last = None;
        self.op = Some(Op { face, anchor, normal, distance: 0.0, dragging });
        Ok(())
    }

    fn finish(&mut self, doc: &mut Document, typed: Option<f64>) -> Result<(), String> {
        let Some(op) = &self.op else { return Ok(()) };
        let (face, distance) = (op.face, typed.unwrap_or(op.distance));
        self.apply(doc, face, distance)?;
        self.op = None;
        self.snap = None;
        Ok(())
    }

    /// Push/pull as one undo step (auto-grouping a new board if enabled), leaving the document
    /// untouched on failure.
    fn apply(&mut self, doc: &mut Document, face: FaceId, distance: f64) -> Result<(), String> {
        doc.push_pull(face, distance, self.auto_group).map_err(|e| e.to_string())?;
        self.last = Some(Done { face, distance, model_version: doc.model_version() });
        self.repeat = Some(distance);
        self.hover = None;
        Ok(())
    }
}

/// The face of the container being edited under the cursor, and where it was hit. (Clicking a
/// closed group's face opens the group first; see `Tools::open_group_to_draw_on`.)
fn active_face(doc: &Document, screen: Screen<'_>, cursor: Vec2) -> Result<Option<(FaceId, DVec3)>, String> {
    let view = doc.view();
    match hit_face(view.model(), &screen.ray(cursor)) {
        None => Ok(None),
        Some((face, point)) => match view.owner(Entity::Face(face)) {
            Owner::Active => Ok(Some((face, point))),
            _ => Err("That face is part of a group. Double-click the group to edit inside it, then push/pull.".into()),
        },
    }
}

#[cfg(test)]
mod tests {
    use woodsketch_view::{OrbitCamera, StandardView};

    use super::*;

    const SIZE: Vec2 = Vec2::new(1000.0, 800.0);

    /// A 2x4 profile on the ground, seen from the front-left so vertical motion is visible.
    fn setup() -> (Document, OrbitCamera, FaceId) {
        let mut doc = Document::default();
        let face = doc
            .edit(|m| m.add_rectangle(DVec3::ZERO, DVec3::X * 1.5, DVec3::Y * 3.5, DVec3::Z))
            .unwrap();
        let mut camera =
            OrbitCamera { target: glam::Vec3::new(0.75, 1.75, 0.0), distance: 40.0, ..Default::default() };
        camera.set_view(StandardView::Iso);
        (doc, camera, face)
    }

    fn at(screen: Screen<'_>, p: DVec3) -> Vec2 {
        screen.project(p).unwrap()
    }

    fn height(doc: &Document) -> f64 {
        let (lo, hi) = doc.bounds().unwrap();
        hi.z - lo.z
    }

    #[test]
    fn click_type_enter_makes_a_board() {
        let (mut doc, camera, _) = setup();
        let screen = Screen::new(&camera, SIZE);
        let mut tool = PushPullTool::default();
        let center = DVec3::new(0.75, 1.75, 0.0);
        tool.click(&mut doc, screen, at(screen, center), 1).unwrap();
        assert!(tool.is_busy());
        tool.hover(&doc, screen, at(screen, center + DVec3::Z * 5.0));
        tool.enter(&mut doc, "96").unwrap();
        assert!(!tool.is_busy());
        assert_eq!(height(&doc), 96.0);
        // The new board is its own group (auto-grouping is on by default).
        assert!(doc.model().is_empty());
        assert_eq!(doc.container().instances().count(), 1);
        assert_eq!(doc.view().model().faces().count(), 6);
        assert_eq!(tool.measurement(), "96\"");
    }

    #[test]
    fn auto_group_off_leaves_loose_geometry() {
        let (mut doc, camera, _) = setup();
        let screen = Screen::new(&camera, SIZE);
        let mut tool = PushPullTool { auto_group: false, ..Default::default() };
        let center = DVec3::new(0.75, 1.75, 0.0);
        tool.click(&mut doc, screen, at(screen, center), 1).unwrap();
        tool.hover(&doc, screen, at(screen, center + DVec3::Z * 5.0));
        tool.enter(&mut doc, "96").unwrap();
        assert_eq!(doc.model().faces().count(), 6);
        assert_eq!(doc.container().instances().count(), 0);
    }

    #[test]
    fn mouse_distance_follows_the_cursor_up_the_normal() {
        let (mut doc, camera, _) = setup();
        let screen = Screen::new(&camera, SIZE);
        let mut tool = PushPullTool::default();
        let center = DVec3::new(0.75, 1.75, 0.0);
        tool.click(&mut doc, screen, at(screen, center), 1).unwrap();
        tool.hover(&doc, screen, at(screen, center + DVec3::Z * 6.0));
        tool.click(&mut doc, screen, at(screen, center + DVec3::Z * 6.0), 1).unwrap();
        assert!((height(&doc) - 6.0).abs() < 0.01, "{}", height(&doc));
    }

    #[test]
    fn retyping_after_finishing_changes_the_length() {
        let (mut doc, camera, _) = setup();
        let screen = Screen::new(&camera, SIZE);
        let mut tool = PushPullTool::default();
        let center = DVec3::new(0.75, 1.75, 0.0);
        tool.click(&mut doc, screen, at(screen, center), 1).unwrap();
        tool.hover(&doc, screen, at(screen, center + DVec3::Z * 5.0));
        tool.enter(&mut doc, "96").unwrap();
        tool.enter(&mut doc, "8'").unwrap();
        assert_eq!(height(&doc), 96.0);
        tool.enter(&mut doc, "92 5/8").unwrap();
        assert_eq!(height(&doc), 92.625);
        // Still a single undo step back to the flat profile.
        doc.undo();
        assert_eq!(height(&doc), 0.0);
    }

    #[test]
    fn typing_pushes_the_way_the_mouse_went() {
        let (mut doc, camera, _) = setup();
        let screen = Screen::new(&camera, SIZE);
        let mut tool = PushPullTool::default();
        let center = DVec3::new(0.75, 1.75, 0.0);
        tool.click(&mut doc, screen, at(screen, center), 1).unwrap();
        tool.hover(&doc, screen, at(screen, center - DVec3::Z * 5.0));
        tool.enter(&mut doc, "10").unwrap();
        let (lo, _) = doc.bounds().unwrap();
        assert_eq!(lo.z, -10.0);
    }

    #[test]
    fn double_click_repeats_the_last_distance() {
        let mut doc = Document::default();
        let (a, b) = doc.edit(|m| {
            let a = m.add_rectangle(DVec3::ZERO, DVec3::X * 1.5, DVec3::Y * 3.5, DVec3::Z).unwrap();
            let b = m.add_rectangle(DVec3::new(5.0, 0.0, 0.0), DVec3::X * 1.5, DVec3::Y * 3.5, DVec3::Z).unwrap();
            (a, b)
        });
        let mut camera = OrbitCamera { target: glam::Vec3::new(3.0, 1.75, 0.0), distance: 40.0, ..Default::default() };
        camera.set_view(StandardView::Iso);
        let screen = Screen::new(&camera, SIZE);
        let mut tool = PushPullTool::default();
        let first = DVec3::new(0.75, 1.75, 0.0);
        tool.click(&mut doc, screen, at(screen, first), 1).unwrap();
        tool.hover(&doc, screen, at(screen, first + DVec3::Z));
        tool.enter(&mut doc, "12").unwrap();
        // Double-click the second profile: its first click starts, the second repeats 12".
        let second = at(screen, DVec3::new(5.75, 1.75, 0.0));
        tool.click(&mut doc, screen, second, 1).unwrap();
        tool.click(&mut doc, screen, second, 2).unwrap();
        assert!(!tool.is_busy());
        // Both profiles became 12" boards, each auto-grouped.
        let view = doc.view().model();
        let tops = view.faces().filter(|(_, f)| (view.vertex(f.outer[0]).z - 12.0).abs() < 1e-9).count();
        assert_eq!(tops, 2);
        assert_eq!(doc.container().instances().count(), 2);
        assert!(doc.model().face(a).is_none() && doc.model().face(b).is_none());
    }

    #[test]
    fn faces_inside_groups_need_the_group_opened_first() {
        let (mut doc, camera, face) = setup();
        let group = doc.push_pull(face, 10.0, true).unwrap().unwrap();
        let screen = Screen::new(&camera, SIZE);
        let mut tool = PushPullTool::default();
        let top = DVec3::new(0.75, 1.75, 10.0);
        let error = tool.click(&mut doc, screen, at(screen, top), 1).unwrap_err();
        assert!(error.contains("Double-click the group"), "{error}");
        // Inside the group, the top face can be pulled; the board stays one group.
        doc.enter(group);
        let screen = Screen::in_context(&camera, SIZE, doc.context_transform());
        tool.click(&mut doc, screen, at(screen, top), 1).unwrap();
        tool.hover(&doc, screen, at(screen, top + DVec3::Z * 3.0));
        tool.enter(&mut doc, "2").unwrap();
        assert_eq!(doc.bounds().unwrap().1.z, 12.0);
        assert_eq!(doc.scene().root.instances().count(), 1);
    }

    #[test]
    fn failed_push_leaves_the_model_and_tool_ready() {
        let (mut doc, camera, face) = setup();
        doc.edit(|m| m.push_pull(face, 10.0)).unwrap();
        let version = doc.model_version();
        let screen = Screen::new(&camera, SIZE);
        let mut tool = PushPullTool::default();
        // Click the top face, then try to push it through the bottom.
        let top = DVec3::new(0.75, 1.75, 10.0);
        tool.click(&mut doc, screen, at(screen, top), 1).unwrap();
        tool.hover(&doc, screen, at(screen, top - DVec3::Z * 3.0));
        assert!(tool.enter(&mut doc, "20").is_err());
        assert_eq!(doc.model_version(), version);
        assert!(tool.is_busy());
        tool.enter(&mut doc, "4").unwrap();
        assert_eq!(height(&doc), 6.0);
    }
}
