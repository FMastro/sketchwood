//! The Line tool (L). Click a start point, then click each next point; the lines chain until a
//! face forms, you click the last point again (or double-click), or press Esc. Press-drag-release
//! draws a single line.
//!
//! - Inference ([`crate::infer`]): endpoints, midpoints and edges snap; near a red/green/blue line,
//!   or one parallel or perpendicular to the last edge hovered, the line follows it; it lines up
//!   with the last point hovered ("From Point"). Hold Shift to lock the direction showing.
//!   Arrow keys lock an axis: Right = red, Left = green, Up/Down = blue.
//! - Type a length (along the current direction), a relative `x,y,z` / `<x,y,z>` or an absolute
//!   `[x,y,z]` to place the next point exactly.

use glam::{DVec3, Vec2};
use woodsketch_doc::units::format_length;
use woodsketch_doc::{Document, EPS};

use crate::infer::{self, Direction, Hints, Query, Target};
use crate::move_tool::parse_offset;
use crate::pick::{Screen, Snap, snap};
use crate::{Axis, Guide, Overlay};

#[derive(Default)]
pub struct LineTool {
    /// The point the next line starts from, while drawing.
    start: Option<Start>,
    hover: Option<Snap>,
    /// Where the line under the cursor ends, and why.
    end: Option<Target>,
    /// Locked with an arrow key.
    lock: Option<Direction>,
    /// Locked by holding Shift while a direction showed.
    shift_lock: Option<Direction>,
    shift: bool,
    /// The edge and point the cursor last passed over.
    hints: Hints,
}

#[derive(Clone, Copy)]
struct Start {
    point: DVec3,
    /// Normal of the surface the chain started on; the cursor falls back to its plane.
    normal: DVec3,
    /// Started by press-and-drag; one line, finished on release.
    dragging: bool,
}

impl LineTool {
    pub fn is_busy(&self) -> bool {
        self.start.is_some()
    }

    pub fn cancel(&mut self) {
        self.start = None;
        self.end = None;
        self.lock = None;
        self.shift_lock = None;
    }

    pub fn lock_axis(&mut self, axis: Axis) {
        let direction = Direction::axis(axis);
        self.lock = if self.lock == Some(direction) { None } else { Some(direction) };
    }

    /// Holding Shift locks the direction showing (an axis, or parallel or perpendicular to an
    /// edge); letting go unlocks it.
    pub fn set_shift(&mut self, shift: bool) {
        self.shift = shift;
        if !shift {
            self.shift_lock = None;
        }
    }

    pub fn hover(&mut self, doc: &Document, screen: Screen<'_>, cursor: Vec2) {
        let model = doc.view().model();
        let Some(start) = self.start else {
            self.hover = snap(model, screen, cursor, None);
            self.hints.note(self.hover.as_ref());
            self.end = None;
            return;
        };
        let base = start.point;
        let query = Query {
            base,
            hints: &self.hints,
            lock: self.lock.or(self.shift_lock),
            plane_normal: start.normal,
            current: self.end.map_or(base, |t| t.point),
        };
        let target = infer::target(model, screen, cursor, &query, |_| false, |ray| {
            // On a face under the cursor, else in the plane the chain started on, else facing
            // the camera.
            snap(model, screen, cursor, None)
                .filter(|s| s.kind == crate::SnapKind::OnFace)
                .map(|s| (s.point, Some(s)))
                .or_else(|| ray.hit_plane(base, start.normal).map(|p| (p, None)))
                .or_else(|| ray.hit_plane(base, -screen.forward()).map(|p| (p, None)))
        });
        if self.shift && self.shift_lock.is_none() {
            self.shift_lock = target.direction;
        }
        self.hints.note(target.snap.as_ref());
        self.hover = target.snap;
        self.end = Some(target);
    }

    pub fn click(&mut self, doc: &mut Document, screen: Screen<'_>, cursor: Vec2) -> Result<(), String> {
        self.hover(doc, screen, cursor);
        match self.start {
            None => {
                self.begin(false);
                Ok(())
            }
            Some(start) => {
                let end = self.end.map_or(start.point, |t| t.point);
                // Clicking the last point again (or double-clicking) ends the chain.
                if end.distance(start.point) < EPS {
                    self.cancel();
                    return Ok(());
                }
                self.draw(doc, end);
                Ok(())
            }
        }
    }

    pub fn drag_start(&mut self, doc: &Document, screen: Screen<'_>, origin: Vec2) {
        if self.start.is_none() {
            self.hover(doc, screen, origin);
            self.begin(true);
        }
    }

    pub fn drag_end(&mut self, doc: &mut Document, screen: Screen<'_>, cursor: Vec2) -> Result<(), String> {
        if !self.start.is_some_and(|s| s.dragging) {
            return Ok(());
        }
        self.hover(doc, screen, cursor);
        let Some(end) = self.end.map(|t| t.point) else { return Ok(()) };
        if self.start.is_some_and(|s| end.distance(s.point) < EPS) {
            self.cancel();
            return Ok(());
        }
        self.draw(doc, end);
        self.cancel();
        Ok(())
    }

    /// A typed length or point for the next line's end.
    pub fn enter(&mut self, doc: &mut Document, typed: &str) -> Result<(), String> {
        let Some(start) = self.start else {
            return Err("Click the start point, then type the length".into());
        };
        // Along an inferred direction, use it exactly so typed lengths come out exact.
        let direction = match self.end {
            Some(Target { point, direction: Some(d), .. }) if (point - start.point).dot(d.dir) < 0.0 => -d.dir,
            Some(Target { direction: Some(d), .. }) => d.dir,
            Some(t) => t.point - start.point,
            None => DVec3::ZERO,
        };
        let offset = parse_offset(typed, direction, start.point)?;
        if offset.length() < EPS {
            return Err("Length must be greater than zero".into());
        }
        self.draw(doc, start.point + offset);
        Ok(())
    }

    pub fn status(&self) -> &'static str {
        if self.start.is_some() {
            "Select end point or enter value. Hold Shift or press an arrow key to lock a direction; Esc ends the line."
        } else {
            "Select start point."
        }
    }

    pub fn measurement(&self) -> String {
        match (self.start, self.end) {
            (Some(start), Some(end)) => format_length(start.point.distance(end.point)),
            _ => String::new(),
        }
    }

    pub fn overlay(&self, overlay: &mut Overlay) {
        overlay.snap = self.hover;
        if let (Some(start), Some(end)) = (self.start, self.end) {
            overlay.lines.push([start.point, end.point]);
            overlay.guide = Some(Guide { from: start.point, to: end.point, along: end.direction.map(|d| d.along) });
            overlay.alignment = end.from.map(|(anchor, axis)| (anchor, end.point, axis));
        }
    }

    fn begin(&mut self, dragging: bool) {
        let Some(hover) = self.hover else { return };
        self.start = Some(Start { point: hover.point, normal: hover.normal, dragging });
        self.end = None;
    }

    /// Draw from the start point to `end` and continue from there, unless the line closed a
    /// face (SketchUp stops chaining then).
    fn draw(&mut self, doc: &mut Document, end: DVec3) {
        let Some(start) = self.start else { return };
        let faces_before = doc.model().faces().count();
        doc.edit(|model| model.add_line(start.point, end));
        if doc.model().faces().count() != faces_before {
            self.cancel();
        } else {
            self.start = Some(Start { point: end, ..start });
            self.end = Some(Target { point: end, ..Target::default() });
        }
    }
}

#[cfg(test)]
mod tests {
    use woodsketch_view::{OrbitCamera, StandardView};

    use super::*;

    const SIZE: Vec2 = Vec2::new(1000.0, 800.0);

    fn camera() -> OrbitCamera {
        let mut camera = OrbitCamera { target: glam::Vec3::new(5.0, 5.0, 0.0), distance: 40.0, ..Default::default() };
        camera.set_view(StandardView::Top);
        camera
    }

    fn at(screen: Screen<'_>, x: f64, y: f64) -> Vec2 {
        screen.project(DVec3::new(x, y, 0.0)).unwrap()
    }

    #[test]
    fn clicking_a_triangle_closes_a_face_and_ends_the_chain() {
        let mut doc = Document::default();
        let camera = camera();
        let screen = Screen::new(&camera, SIZE);
        let mut tool = LineTool::default();
        for (x, y) in [(1.0, 1.0), (7.0, 1.3), (4.0, 6.0), (1.0, 1.0)] {
            tool.click(&mut doc, screen, at(screen, x, y)).unwrap();
        }
        assert_eq!(doc.model().faces().count(), 1);
        assert_eq!(doc.model().edges().count(), 3);
        assert!(!tool.is_busy(), "closing a face ends the chain");
        // One undo step per line.
        doc.undo();
        assert_eq!(doc.model().edges().count(), 2);
    }

    #[test]
    fn typed_lengths_follow_the_inferred_axis() {
        let mut doc = Document::default();
        let camera = camera();
        let screen = Screen::new(&camera, SIZE);
        let mut tool = LineTool::default();
        tool.click(&mut doc, screen, at(screen, 0.0, 0.0)).unwrap();
        // Close to the red axis, a little off it.
        tool.hover(&doc, screen, at(screen, 3.0, 0.1));
        assert_eq!(tool.end.unwrap().direction.map(|d| d.along), Some(infer::Along::Axis(Axis::X)));
        tool.enter(&mut doc, "36").unwrap();
        tool.lock_axis(Axis::Y);
        tool.hover(&doc, screen, at(screen, 20.0, 5.0));
        tool.enter(&mut doc, "1 1/2").unwrap();
        let (lo, hi) = doc.model().bounds().unwrap();
        assert!(lo.abs_diff_eq(DVec3::ZERO, 1e-12) && hi.abs_diff_eq(DVec3::new(36.0, 1.5, 0.0), 1e-12), "{lo} {hi}");
        assert!(tool.is_busy(), "no face yet, so the chain continues");
        assert_eq!(tool.measurement(), "0\"");
    }

    #[test]
    fn a_line_across_a_rectangle_splits_it() {
        let mut doc = Document::default();
        doc.edit(|m| m.add_rectangle(DVec3::ZERO, DVec3::X * 10.0, DVec3::Y * 10.0, DVec3::Z));
        let camera = camera();
        let screen = Screen::new(&camera, SIZE);
        let mut tool = LineTool::default();
        // From the bottom edge's midpoint straight up to the top edge.
        tool.click(&mut doc, screen, at(screen, 5.0, 0.0)).unwrap();
        tool.click(&mut doc, screen, at(screen, 5.0, 10.0)).unwrap();
        assert_eq!(doc.model().faces().count(), 2);
        assert!(!tool.is_busy());
    }

    #[test]
    fn clicking_the_same_point_twice_ends_the_chain() {
        let mut doc = Document::default();
        let camera = camera();
        let screen = Screen::new(&camera, SIZE);
        let mut tool = LineTool::default();
        tool.click(&mut doc, screen, at(screen, 0.0, 0.0)).unwrap();
        tool.click(&mut doc, screen, at(screen, 4.0, 3.0)).unwrap();
        tool.click(&mut doc, screen, at(screen, 4.0, 3.0)).unwrap();
        assert!(!tool.is_busy());
        assert_eq!(doc.model().edges().count(), 1);
    }

    #[test]
    fn press_drag_release_draws_one_line() {
        let mut doc = Document::default();
        let camera = camera();
        let screen = Screen::new(&camera, SIZE);
        let mut tool = LineTool::default();
        tool.drag_start(&doc, screen, at(screen, 0.0, 0.0));
        tool.hover(&doc, screen, at(screen, 4.0, 3.0));
        tool.drag_end(&mut doc, screen, at(screen, 4.0, 3.0)).unwrap();
        assert_eq!(doc.model().edges().count(), 1);
        assert!(!tool.is_busy());
    }

    #[test]
    fn holding_shift_keeps_the_direction_showing() {
        let mut doc = Document::default();
        let camera = camera();
        let screen = Screen::new(&camera, SIZE);
        let mut tool = LineTool::default();
        tool.click(&mut doc, screen, at(screen, 0.0, 0.0)).unwrap();
        tool.hover(&doc, screen, at(screen, 4.0, 0.1));
        assert_eq!(tool.end.unwrap().direction, Some(Direction::axis(Axis::X)));
        tool.set_shift(true);
        tool.hover(&doc, screen, at(screen, 4.0, 0.1));
        // Far off the axis, it still follows red while Shift is down.
        tool.hover(&doc, screen, at(screen, 6.0, 5.0));
        let end = tool.end.unwrap();
        assert!(end.point.y.abs() < 1e-9 && end.point.x > 5.0, "{}", end.point);
        tool.set_shift(false);
        tool.hover(&doc, screen, at(screen, 6.0, 5.0));
        assert!(tool.end.unwrap().point.y > 4.0, "free again");
    }

    #[test]
    fn a_line_parallel_to_a_slanted_edge() {
        let mut doc = Document::default();
        // A slanted edge, hovered on the way to the start point.
        doc.edit(|m| m.add_line(DVec3::new(0.0, 0.0, 0.0), DVec3::new(6.0, 3.0, 0.0)));
        let camera = camera();
        let screen = Screen::new(&camera, SIZE);
        let mut tool = LineTool::default();
        tool.hover(&doc, screen, at(screen, 4.0, 2.0) + Vec2::new(0.0, 1.0));
        tool.click(&mut doc, screen, at(screen, 0.0, 6.0)).unwrap();
        let along = DVec3::new(6.0, 3.0, 0.0).normalize();
        let near = DVec3::new(0.0, 6.0, 0.0) + along * 5.0;
        tool.hover(&doc, screen, at(screen, near.x + 0.05, near.y - 0.05));
        assert_eq!(tool.end.unwrap().direction.map(|d| d.along), Some(infer::Along::Parallel));
        tool.enter(&mut doc, "4").unwrap();
        let (_, edge) = doc.model().edges().last().unwrap();
        let [a, b] = doc.model().edge_points(edge);
        assert!((b - a).cross(along).length() < 1e-9 && ((b - a).length() - 4.0).abs() < 1e-9);
    }

    #[test]
    fn typing_before_starting_explains() {
        let mut doc = Document::default();
        let mut tool = LineTool::default();
        assert!(tool.enter(&mut doc, "12").is_err());
    }
}
