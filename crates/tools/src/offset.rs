//! The Offset tool (F). Click a face (or select one first, then click anywhere), move the
//! mouse in or out, and click again or type the distance: the face's outline is copied at that
//! distance on the face. Typing right after changes the offset just made.

use glam::{DVec3, Vec2};
use woodsketch_doc::units::{format_length, parse_length};
use woodsketch_doc::{Document, EPS, Entity, FaceId};

use crate::pick::Screen;
use crate::{Overlay, highlight_entity, pick_resolved};

#[derive(Default)]
pub struct OffsetTool {
    op: Option<Op>,
    /// The face a click would offset.
    hover: Option<FaceId>,
    last: Option<Done>,
}

struct Op {
    face: FaceId,
    /// Positive inward.
    distance: f64,
    preview: Vec<DVec3>,
}

#[derive(Clone, Copy)]
struct Done {
    face: FaceId,
    distance: f64,
    model_version: u64,
}

impl OffsetTool {
    pub fn is_busy(&self) -> bool {
        self.op.is_some()
    }

    pub fn cancel(&mut self) {
        self.op = None;
        self.last = None;
    }

    pub fn hover(&mut self, doc: &Document, screen: Screen<'_>, cursor: Vec2) {
        let Some(op) = &mut self.op else {
            // A face inside a closed group highlights too: clicking it opens the group first.
            let view = doc.view();
            self.hover = target(doc, screen, cursor).or_else(|| {
                crate::pick::hit_face(view.model(), &screen.ray(cursor))
                    .map(|(id, _)| id)
                    .filter(|&id| view.owner(Entity::Face(id)) != woodsketch_doc::Owner::Outside)
            });
            return;
        };
        let model = doc.model();
        let Some(face) = model.face(op.face) else { return };
        let origin = model.vertex(face.outer[0]);
        let Some(point) = screen.ray(cursor).hit_plane(origin, face.normal) else { return };
        // How far the cursor is from the outline: inside is an inward offset.
        let outline = model.loop_points(&face.outer);
        let distance = (0..outline.len())
            .map(|i| {
                let (a, b) = (outline[i], outline[(i + 1) % outline.len()]);
                let t = ((point - a).dot(b - a) / (b - a).length_squared()).clamp(0.0, 1.0);
                point.distance(a + (b - a) * t)
            })
            .fold(f64::INFINITY, f64::min);
        op.distance = if model.face_contains(face, point) { distance } else { -distance };
        op.preview = model.offset_outline(op.face, op.distance).unwrap_or_default();
    }

    pub fn click(&mut self, doc: &mut Document, screen: Screen<'_>, cursor: Vec2) -> Result<(), String> {
        if self.op.is_none() {
            let Some(face) = target(doc, screen, cursor) else {
                return Err("Click a face to offset its outline".into());
            };
            self.last = None;
            self.op = Some(Op { face, distance: 0.0, preview: Vec::new() });
            return Ok(());
        }
        self.hover(doc, screen, cursor);
        let op = self.op.as_ref().expect("checked above");
        let (face, distance) = (op.face, op.distance);
        self.apply(doc, face, distance)
    }

    pub fn enter(&mut self, doc: &mut Document, typed: &str) -> Result<(), String> {
        let length = parse_length(typed).map_err(|e| e.to_string())?;
        // Typed distances go the way the mouse is (in or out); negative reverses.
        if let Some(op) = &self.op {
            let distance = if op.distance < 0.0 { -length } else { length };
            return self.apply(doc, op.face, distance);
        }
        match self.last {
            Some(last) if last.model_version == doc.model_version() => {
                let distance = if last.distance < 0.0 { -length } else { length };
                doc.model().offset_outline(last.face, distance).map_err(|e| e.to_string())?;
                doc.undo();
                self.apply(doc, last.face, distance)
            }
            _ => Err("Click a face, then type the offset".into()),
        }
    }

    pub fn status(&self) -> &'static str {
        if self.op.is_some() {
            "Move in or out and click, or enter the offset distance."
        } else {
            "Click a face to offset its outline (or select a face first)."
        }
    }

    pub fn measurement(&self) -> String {
        match (&self.op, &self.last) {
            (Some(op), _) => format_length(op.distance.abs()),
            (None, Some(last)) => format_length(last.distance.abs()),
            (None, None) => String::new(),
        }
    }

    pub fn overlay(&self, doc: &Document, overlay: &mut Overlay) {
        let face = self.op.as_ref().map(|op| op.face).or(self.hover);
        if let Some(face) = face {
            highlight_entity(doc.view(), Entity::Face(face), overlay);
        }
        if let Some(op) = &self.op {
            let p = &op.preview;
            for i in 0..p.len() {
                overlay.lines.push([p[i], p[(i + 1) % p.len()]]);
            }
        }
    }

    fn apply(&mut self, doc: &mut Document, face: FaceId, distance: f64) -> Result<(), String> {
        if distance.abs() < EPS {
            return Err("Move the mouse in or out of the face first".into());
        }
        // Check before editing, so a refused offset leaves no empty undo step.
        doc.model().offset_outline(face, distance).map_err(|e| e.to_string())?;
        doc.edit(|m| m.offset_face(face, distance)).map_err(|e| e.to_string())?;
        self.op = None;
        self.last = Some(Done { face, distance, model_version: doc.model_version() });
        Ok(())
    }
}

/// The face to offset: the selected face, if exactly one is selected, else the face under the
/// cursor.
fn target(doc: &Document, screen: Screen<'_>, cursor: Vec2) -> Option<FaceId> {
    let selection: Vec<Entity> = doc.selection().iter().collect();
    if let [Entity::Face(face)] = selection[..] {
        return Some(face);
    }
    match pick_resolved(doc, screen, cursor)? {
        Entity::Face(face) => Some(face),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use woodsketch_view::{OrbitCamera, StandardView};

    use super::*;

    const SIZE: Vec2 = Vec2::new(1000.0, 800.0);

    fn setup() -> (Document, OrbitCamera) {
        let mut doc = Document::default();
        doc.edit(|m| m.add_rectangle(DVec3::ZERO, DVec3::X * 24.0, DVec3::Y * 36.0, DVec3::Z));
        let mut camera = OrbitCamera { target: glam::Vec3::new(12.0, 18.0, 0.0), distance: 80.0, ..Default::default() };
        camera.set_view(StandardView::Top);
        (doc, camera)
    }

    fn at(screen: Screen<'_>, x: f64, y: f64) -> Vec2 {
        screen.project(DVec3::new(x, y, 0.0)).unwrap()
    }

    #[test]
    fn click_a_face_move_inward_and_type_the_frame_width() {
        let (mut doc, camera) = setup();
        let screen = Screen::new(&camera, SIZE);
        let mut tool = OffsetTool::default();
        tool.click(&mut doc, screen, at(screen, 12.0, 18.0)).unwrap();
        tool.hover(&doc, screen, at(screen, 3.0, 18.0));
        assert!((tool.op.as_ref().unwrap().distance - 3.0).abs() < 0.1, "about 3\" in");
        tool.enter(&mut doc, "2 1/2").unwrap();
        assert_eq!(doc.model().faces().count(), 2);
        assert_eq!(tool.measurement(), "2 1/2\"");
        // Retyping changes it, as one undo step.
        tool.enter(&mut doc, "3").unwrap();
        doc.undo();
        assert_eq!(doc.model().faces().count(), 1);
    }

    #[test]
    fn moving_outside_offsets_outward() {
        let (mut doc, camera) = setup();
        let screen = Screen::new(&camera, SIZE);
        let mut tool = OffsetTool::default();
        tool.click(&mut doc, screen, at(screen, 12.0, 18.0)).unwrap();
        tool.hover(&doc, screen, at(screen, -2.0, 18.0));
        tool.enter(&mut doc, "1").unwrap();
        let (lo, hi) = doc.model().bounds().unwrap();
        assert!(lo.abs_diff_eq(DVec3::new(-1.0, -1.0, 0.0), 1e-9) && hi.abs_diff_eq(DVec3::new(25.0, 37.0, 0.0), 1e-9));
    }

    #[test]
    fn too_far_is_refused_without_an_undo_step() {
        let (mut doc, camera) = setup();
        let screen = Screen::new(&camera, SIZE);
        let mut tool = OffsetTool::default();
        tool.click(&mut doc, screen, at(screen, 12.0, 18.0)).unwrap();
        tool.hover(&doc, screen, at(screen, 10.0, 18.0));
        let version = doc.model_version();
        assert!(tool.enter(&mut doc, "13").is_err());
        assert_eq!(doc.model_version(), version);
        assert!(tool.is_busy(), "still offsetting; try another distance");
    }
}
