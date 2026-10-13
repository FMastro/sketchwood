//! The Rectangle tool (R). Click a corner, then click the opposite corner, or press-drag-release.
//! While sizing, or right after finishing, type `width,height` and Enter, as in SketchUp.

use glam::{DVec3, Vec2};
use sketchcraft_doc::units::{format_length, parse_lengths};
use sketchcraft_doc::{Document, EPS};

use crate::Overlay;
use crate::pick::{Screen, Snap, snap};

#[derive(Default)]
pub struct RectangleTool {
    sizing: Option<Sizing>,
    /// The rectangle just drawn, which typed dimensions can still resize.
    last: Option<Drawn>,
    hover: Option<Snap>,
}

struct Sizing {
    frame: Frame,
    /// Signed extents along `u` and `v` under the cursor.
    extents: (f64, f64),
    /// Started by press-and-drag; finishes on release.
    dragging: bool,
}

struct Drawn {
    frame: Frame,
    extents: (f64, f64),
    /// The document's model version right after drawing; any other edit ends resizing.
    model_version: u64,
}

/// The corner the rectangle grows from and the plane it lies in.
#[derive(Clone, Copy)]
struct Frame {
    corner: DVec3,
    normal: DVec3,
    u: DVec3,
    v: DVec3,
}

impl RectangleTool {
    pub fn is_busy(&self) -> bool {
        self.sizing.is_some()
    }

    pub fn cancel(&mut self) {
        self.sizing = None;
        self.last = None;
    }

    pub fn hover(&mut self, doc: &Document, screen: Screen<'_>, cursor: Vec2) {
        let plane = self.sizing.as_ref().map(|s| (s.frame.corner, s.frame.normal));
        // Snap to anything in the drawing, including other groups.
        self.hover = snap(doc.view().model(), screen, cursor, plane);
        if let (Some(sizing), Some(hover)) = (self.sizing.as_mut(), self.hover) {
            let d = hover.point - sizing.frame.corner;
            sizing.extents = (d.dot(sizing.frame.u), d.dot(sizing.frame.v));
        }
    }

    pub fn click(&mut self, doc: &mut Document, screen: Screen<'_>, cursor: Vec2) -> Result<(), String> {
        self.hover(doc, screen, cursor);
        match &self.sizing {
            None => {
                self.start();
                Ok(())
            }
            Some(sizing) => self.finish(doc, sizing.extents),
        }
    }

    pub fn drag_start(&mut self, doc: &Document, screen: Screen<'_>, origin: Vec2) {
        if self.sizing.is_none() {
            self.hover(doc, screen, origin);
            self.start();
            if let Some(sizing) = &mut self.sizing {
                sizing.dragging = true;
            }
        }
    }

    pub fn drag_end(&mut self, doc: &mut Document, screen: Screen<'_>, cursor: Vec2) -> Result<(), String> {
        // Enter may already have finished the rectangle while the button was held.
        if !self.sizing.as_ref().is_some_and(|s| s.dragging) {
            return Ok(());
        }
        self.hover(doc, screen, cursor);
        let extents = self.sizing.as_ref().map_or((0.0, 0.0), |s| s.extents);
        self.finish(doc, extents)
    }

    /// Apply typed dimensions: to the rectangle being sized, or to the one just drawn.
    pub fn enter(&mut self, doc: &mut Document, typed: &str) -> Result<(), String> {
        let values = parse_lengths(typed).map_err(|e| e.to_string())?;
        let [width, height] = values[..] else {
            return Err("Enter width and height, like 1.5\",3.5\"".into());
        };
        // Typed values take their direction from the mouse; negative values flip it.
        let apply = |typed: Option<f64>, current: f64| match typed {
            Some(t) if current < 0.0 => -t,
            Some(t) => t,
            None => current,
        };
        if let Some(sizing) = &self.sizing {
            let extents = (apply(width, sizing.extents.0), apply(height, sizing.extents.1));
            return self.finish(doc, extents);
        }
        match self.last.take() {
            Some(last) if last.model_version == doc.model_version() => {
                let extents = (apply(width, last.extents.0), apply(height, last.extents.1));
                if extents.0.abs() < EPS || extents.1.abs() < EPS {
                    self.last = Some(last);
                    return Err("Dimensions must be greater than zero".into());
                }
                doc.undo();
                self.draw(doc, last.frame, extents);
                Ok(())
            }
            _ => Err("Click the first corner, then type the size".into()),
        }
    }

    pub fn status(&self) -> &'static str {
        if self.sizing.is_some() {
            "Select opposite corner or enter value."
        } else {
            "Select first corner."
        }
    }

    /// Live dimensions for the Measurements box.
    pub fn measurement(&self) -> String {
        let extents = match (&self.sizing, &self.last) {
            (Some(sizing), _) => sizing.extents,
            (None, Some(last)) => last.extents,
            (None, None) => return String::new(),
        };
        format!("{}, {}", format_length(extents.0.abs()), format_length(extents.1.abs()))
    }

    pub fn overlay(&self, overlay: &mut Overlay) {
        overlay.snap = self.hover;
        if let Some(sizing) = &self.sizing {
            let Frame { corner, u, v, .. } = sizing.frame;
            let (w, h) = sizing.extents;
            let corners = [corner, corner + u * w, corner + u * w + v * h, corner + v * h];
            for i in 0..4 {
                overlay.lines.push([corners[i], corners[(i + 1) % 4]]);
            }
        }
    }

    fn start(&mut self) {
        let Some(hover) = self.hover else { return };
        let (u, v) = plane_axes(hover.normal);
        self.last = None;
        self.sizing = Some(Sizing {
            frame: Frame { corner: hover.point, normal: hover.normal, u, v },
            extents: (0.0, 0.0),
            dragging: false,
        });
    }

    fn finish(&mut self, doc: &mut Document, extents: (f64, f64)) -> Result<(), String> {
        let Some(sizing) = &self.sizing else { return Ok(()) };
        if extents.0.abs() < EPS || extents.1.abs() < EPS {
            return Err("Dimensions must be greater than zero".into());
        }
        let frame = sizing.frame;
        self.sizing = None;
        self.draw(doc, frame, extents);
        Ok(())
    }

    fn draw(&mut self, doc: &mut Document, frame: Frame, extents: (f64, f64)) {
        doc.edit(|model| {
            model.add_rectangle(frame.corner, frame.u * extents.0, frame.v * extents.1, frame.normal)
        });
        self.last = Some(Drawn { frame, extents, model_version: doc.model_version() });
    }
}

/// In-plane axes for drawing on a surface with `normal`: red/green on horizontal surfaces,
/// otherwise a horizontal axis and the in-plane "up".
pub fn plane_axes(normal: DVec3) -> (DVec3, DVec3) {
    let n = normal.normalize();
    if n.z.abs() > 0.999 {
        (DVec3::X, DVec3::Y)
    } else {
        let u = DVec3::Z.cross(n).normalize();
        (u, n.cross(u))
    }
}

#[cfg(test)]
mod tests {
    use sketchcraft_doc::Entity;
    use sketchcraft_view::{OrbitCamera, StandardView};

    use super::*;

    const SIZE: Vec2 = Vec2::new(1000.0, 800.0);

    fn camera() -> OrbitCamera {
        let mut camera = OrbitCamera { target: glam::Vec3::new(2.0, 2.0, 0.0), distance: 30.0, ..Default::default() };
        camera.set_view(StandardView::Top);
        camera
    }

    fn at(screen: Screen<'_>, x: f64, y: f64) -> Vec2 {
        screen.project(DVec3::new(x, y, 0.0)).unwrap()
    }

    fn face_size(doc: &Document) -> (f64, f64) {
        let (lo, hi) = doc.model().bounds().unwrap();
        ((hi - lo).x, (hi - lo).y)
    }

    #[test]
    fn click_click_draws_on_the_ground() {
        let mut doc = Document::default();
        let camera = camera();
        let screen = Screen::new(&camera, SIZE);
        let mut tool = RectangleTool::default();
        tool.click(&mut doc, screen, at(screen, 1.0, 1.0)).unwrap();
        tool.hover(&doc, screen, at(screen, 3.0, 4.0));
        tool.click(&mut doc, screen, at(screen, 3.0, 4.0)).unwrap();
        assert_eq!(doc.model().faces().count(), 1);
        let (w, h) = face_size(&doc);
        assert!((w - 2.0).abs() < 0.05 && (h - 3.0).abs() < 0.05, "{w} x {h}");
    }

    #[test]
    fn typed_size_while_sizing_makes_an_exact_two_by_four() {
        let mut doc = Document::default();
        let camera = camera();
        let screen = Screen::new(&camera, SIZE);
        let mut tool = RectangleTool::default();
        tool.click(&mut doc, screen, at(screen, 0.0, 0.0)).unwrap();
        // Mouse toward -X, +Y: the width should extend toward -X.
        tool.hover(&doc, screen, at(screen, -2.0, 2.0));
        tool.enter(&mut doc, "1.5\",3.5\"").unwrap();
        assert!(!tool.is_busy());
        let (lo, hi) = doc.model().bounds().unwrap();
        assert!(lo.abs_diff_eq(DVec3::new(-1.5, 0.0, 0.0), 1e-9), "{lo}");
        assert!(hi.abs_diff_eq(DVec3::new(0.0, 3.5, 0.0), 1e-9), "{hi}");
        assert_eq!(tool.measurement(), "1 1/2\", 3 1/2\"");
    }

    #[test]
    fn typing_after_drawing_resizes_the_last_rectangle_in_one_undo_step() {
        let mut doc = Document::default();
        let camera = camera();
        let screen = Screen::new(&camera, SIZE);
        let mut tool = RectangleTool::default();
        tool.click(&mut doc, screen, at(screen, 0.0, 0.0)).unwrap();
        tool.click(&mut doc, screen, at(screen, 2.0, 3.0)).unwrap();
        tool.enter(&mut doc, "1 1/2, 3 1/2").unwrap();
        assert_eq!(face_size(&doc), (1.5, 3.5));
        assert_eq!(doc.model().faces().count(), 1);
        doc.undo();
        assert!(doc.model().is_empty());
    }

    #[test]
    fn partial_entry_keeps_the_other_dimension() {
        let mut doc = Document::default();
        let camera = camera();
        let screen = Screen::new(&camera, SIZE);
        let mut tool = RectangleTool::default();
        tool.click(&mut doc, screen, at(screen, 0.0, 0.0)).unwrap();
        tool.click(&mut doc, screen, at(screen, 2.0, 3.0)).unwrap();
        tool.enter(&mut doc, "1.5,3.5").unwrap();
        tool.enter(&mut doc, ",5.5").unwrap();
        assert_eq!(face_size(&doc), (1.5, 5.5));
    }

    #[test]
    fn press_drag_release_draws() {
        let mut doc = Document::default();
        let camera = camera();
        let screen = Screen::new(&camera, SIZE);
        let mut tool = RectangleTool::default();
        tool.drag_start(&doc, screen, at(screen, 0.0, 0.0));
        tool.hover(&doc, screen, at(screen, 2.0, 2.0));
        tool.drag_end(&mut doc, screen, at(screen, 2.0, 2.0)).unwrap();
        assert_eq!(doc.model().faces().count(), 1);
    }

    #[test]
    fn enter_during_drag_then_release_draws_once() {
        let mut doc = Document::default();
        let camera = camera();
        let screen = Screen::new(&camera, SIZE);
        let mut tool = RectangleTool::default();
        tool.drag_start(&doc, screen, at(screen, 0.0, 0.0));
        tool.hover(&doc, screen, at(screen, 2.0, 2.0));
        tool.enter(&mut doc, "1.5,3.5").unwrap();
        tool.drag_end(&mut doc, screen, at(screen, 2.0, 2.0)).unwrap();
        assert_eq!(doc.model().faces().count(), 1);
        assert_eq!(face_size(&doc), (1.5, 3.5));
    }

    #[test]
    fn first_corner_snaps_to_existing_endpoint() {
        let mut doc = Document::default();
        doc.edit(|m| m.add_rectangle(DVec3::ZERO, DVec3::X * 1.5, DVec3::Y * 3.5, DVec3::Z));
        let camera = camera();
        let screen = Screen::new(&camera, SIZE);
        let mut tool = RectangleTool::default();
        // A second board alongside: snap to the first board's corner, then type its size.
        tool.click(&mut doc, screen, at(screen, 1.5, 0.0) + Vec2::new(3.0, 2.0)).unwrap();
        tool.hover(&doc, screen, at(screen, 3.0, 3.5));
        tool.enter(&mut doc, "1.5,3.5").unwrap();
        assert_eq!(doc.model().faces().count(), 2);
        // Shared edge: 4 + 3 edges.
        assert_eq!(doc.model().edges().count(), 7);
        let shared = doc
            .model()
            .entities()
            .filter(|e| matches!(e, Entity::Edge(id) if doc.model().edge_faces(*id).len() == 2))
            .count();
        assert_eq!(shared, 1);
    }

    #[test]
    fn bad_input_reports_an_error_and_keeps_sizing() {
        let mut doc = Document::default();
        let camera = camera();
        let screen = Screen::new(&camera, SIZE);
        let mut tool = RectangleTool::default();
        tool.click(&mut doc, screen, at(screen, 0.0, 0.0)).unwrap();
        assert!(tool.enter(&mut doc, "abc").is_err());
        assert!(tool.enter(&mut doc, "1.5").is_err());
        assert!(tool.is_busy());
    }

    #[test]
    fn vertical_plane_axes() {
        let (u, v) = plane_axes(-DVec3::Y);
        assert!(u.abs_diff_eq(DVec3::X, 1e-12) && v.abs_diff_eq(DVec3::Z, 1e-12));
    }
}
