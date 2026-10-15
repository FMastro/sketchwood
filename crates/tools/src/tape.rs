//! The Tape Measure tool (T): measures distances and lays out guides, as in SketchUp.
//!
//! - Start on an edge (or a guide line) and move away: a guide line parallel to it follows at
//!   the offset; click, or type the offset, to place it. This is how you mark a cut line or a
//!   board's position a set distance from an edge.
//! - Start on a point: the tape measures to the next point and leaves a guide point there.
//! - Tap Ctrl to switch guide-making off (just measure) and back on.
//! - Typing right after placing a guide moves it to the typed distance.

use glam::{DVec3, Vec2};
use sketchcraft_doc::units::format_length;
use sketchcraft_doc::{Document, EPS, Guide};

use crate::move_tool::parse_offset;
use crate::pick::{Screen, Snap, closest_on_line, infer_axis, snap, snap_feature};
use crate::{Axis, Overlay, SnapKind};

#[derive(Default)]
pub struct TapeTool {
    start: Option<Start>,
    hover: Option<Snap>,
    /// Where the tape ends, and the axis it follows.
    end: Option<(DVec3, Option<Axis>)>,
    lock: Option<Axis>,
    /// Ctrl switched guide-making off.
    measure_only: bool,
    /// The last measurement or guide, which typing can still change.
    last: Option<Done>,
}

#[derive(Clone, Copy)]
struct Start {
    point: DVec3,
    /// Started on an edge or guide line with this direction: making a parallel guide line.
    line: Option<DVec3>,
}

#[derive(Clone, Copy)]
struct Done {
    start: Start,
    /// Offset from the start point (perpendicular to the line, for a guide line).
    offset: DVec3,
    /// The model version right after placing the guide; `None` if nothing was placed.
    model_version: Option<u64>,
}

impl TapeTool {
    pub fn is_busy(&self) -> bool {
        self.start.is_some()
    }

    pub fn cancel(&mut self) {
        self.start = None;
        self.end = None;
        self.lock = None;
        self.last = None;
    }

    pub fn toggle_guides(&mut self) {
        self.measure_only = !self.measure_only;
    }

    pub fn lock_axis(&mut self, axis: Axis) {
        self.lock = if self.lock == Some(axis) { None } else { Some(axis) };
    }

    pub fn hover(&mut self, doc: &Document, screen: Screen<'_>, cursor: Vec2) {
        let model = doc.view().model();
        let Some(start) = self.start else {
            self.hover = snap(model, screen, cursor, None);
            return;
        };
        let base = start.point;
        let ray = screen.ray(cursor);
        let feature = snap_feature(model, screen, cursor, |_| false);
        let (point, axis, shown) = if let Some(axis) = self.lock {
            let dir = axis.dir();
            let t = match &feature {
                Some(s) => Some((s.point - base).dot(dir)),
                None => closest_on_line(base, dir, &ray),
            };
            let current = self.end.map_or(0.0, |(p, _)| (p - base).dot(dir));
            (base + dir * t.unwrap_or(current), Some(axis), feature)
        } else if let Some(s) = feature {
            (s.point, None, Some(s))
        } else if let Some((point, axis)) = infer_axis(screen, base, cursor).filter(|_| start.line.is_none()) {
            (point, Some(axis), None)
        } else {
            let surface = snap(model, screen, cursor, None).filter(|s| s.kind == SnapKind::OnFace);
            // Off any face: a guide line's offset stays level when its edge is level (laying out
            // on the floor), otherwise the tape runs in the plane facing the camera.
            let level = start.line.is_some_and(|d| d.z.abs() < 1e-9);
            let point = surface
                .map(|s| s.point)
                .or_else(|| level.then(|| ray.hit_plane(base, DVec3::Z)).flatten())
                .or_else(|| ray.hit_plane(base, -screen.forward()))
                .unwrap_or(base);
            (point, None, surface)
        };
        self.hover = shown;
        self.end = Some((point, axis));
    }

    pub fn click(&mut self, doc: &mut Document, screen: Screen<'_>, cursor: Vec2) -> Result<(), String> {
        self.hover(doc, screen, cursor);
        match self.start {
            None => {
                let Some(hover) = self.hover else { return Ok(()) };
                self.last = None;
                self.start = Some(Start {
                    point: hover.point,
                    line: hover.line.filter(|_| matches!(hover.kind, SnapKind::OnEdge | SnapKind::OnGuide)),
                });
                self.end = None;
                Ok(())
            }
            Some(start) => {
                let offset = self.offset();
                self.start = None;
                self.end = None;
                if offset.length() < EPS {
                    return Ok(());
                }
                self.place(doc, start, offset);
                Ok(())
            }
        }
    }

    /// A typed distance: places the guide being measured out, or moves the one just placed.
    pub fn enter(&mut self, doc: &mut Document, typed: &str) -> Result<(), String> {
        if let Some(start) = self.start {
            let offset = parse_offset(typed, self.direction(), start.point)?;
            let offset = Self::project(start, offset);
            if offset.length() < EPS {
                return Err("Move the mouse to show the direction, then type the distance".into());
            }
            self.start = None;
            self.end = None;
            self.place(doc, start, offset);
            return Ok(());
        }
        match self.last {
            Some(last) if last.model_version.is_some_and(|v| v == doc.model_version()) => {
                let offset = Self::project(last.start, parse_offset(typed, last.offset, last.start.point)?);
                if offset.length() < EPS {
                    return Err("Distance must be greater than zero".into());
                }
                doc.undo();
                self.place(doc, last.start, offset);
                Ok(())
            }
            _ => Err("Click where to measure from, then type the distance".into()),
        }
    }

    pub fn status(&self) -> String {
        let mode = if self.measure_only { " Measuring only: tap Ctrl to make guides." } else { "" };
        match self.start {
            None => format!("Click an edge to offset a guide line from it, or a point to measure from.{mode}"),
            Some(Start { line: Some(_), .. }) => format!("Click or type the guide line's offset.{mode}"),
            Some(_) => format!("Click the point to measure to, or type the distance.{mode}"),
        }
    }

    pub fn measurement(&self) -> String {
        let offset = if self.start.is_some() { self.offset() } else { self.last.map_or(DVec3::ZERO, |l| l.offset) };
        if self.start.is_none() && self.last.is_none() {
            return String::new();
        }
        format_length(offset.length())
    }

    pub fn overlay(&self, overlay: &mut Overlay) {
        overlay.snap = self.hover;
        // SketchUp marks the cursor with "+" while the tape makes guides.
        overlay.cursor_note = Some(if self.measure_only { "Measure" } else { "+ Guide" });
        let Some(start) = self.start else { return };
        let offset = self.offset();
        let end = start.point + offset;
        // The tape itself, and the guide it would leave.
        overlay.lines.push([start.point, end]);
        let axis = self.end.and_then(|(_, axis)| axis);
        overlay.guide = Some(crate::Guide { from: start.point, to: end, axis });
        if !self.measure_only && offset.length() >= EPS {
            overlay.guides.push((Self::guide(start, offset), true));
        }
    }

    /// The tape from the start point to the cursor; for a guide line, only the part across it.
    fn offset(&self) -> DVec3 {
        match (self.start, self.end) {
            (Some(start), Some((end, _))) => Self::project(start, end - start.point),
            _ => DVec3::ZERO,
        }
    }

    /// The direction typed distances go in.
    fn direction(&self) -> DVec3 {
        match self.end {
            Some((p, Some(axis))) if self.start.is_some_and(|s| (p - s.point).dot(axis.dir()) < 0.0) => -axis.dir(),
            Some((_, Some(axis))) => axis.dir(),
            _ => self.offset(),
        }
    }

    fn project(start: Start, offset: DVec3) -> DVec3 {
        match start.line {
            Some(dir) => offset - dir * offset.dot(dir),
            None => offset,
        }
    }

    fn guide(start: Start, offset: DVec3) -> Guide {
        match start.line {
            Some(dir) => Guide::Line { point: start.point + offset, dir },
            None => Guide::Point(start.point + offset),
        }
    }

    fn place(&mut self, doc: &mut Document, start: Start, offset: DVec3) {
        let model_version = (!self.measure_only).then(|| {
            doc.edit(|m| m.add_guide(Self::guide(start, offset)));
            doc.model_version()
        });
        self.last = Some(Done { start, offset, model_version });
    }
}

#[cfg(test)]
mod tests {
    use sketchcraft_view::{OrbitCamera, StandardView};

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

    fn board() -> Document {
        let mut doc = Document::default();
        doc.edit(|m| m.add_rectangle(DVec3::ZERO, DVec3::X * 10.0, DVec3::Y * 10.0, DVec3::Z));
        doc
    }

    #[test]
    fn offsetting_from_an_edge_makes_a_parallel_guide_line() {
        let mut doc = board();
        let camera = camera();
        let screen = Screen::new(&camera, SIZE);
        let mut tool = TapeTool::default();
        // On the bottom edge, away from its midpoint.
        tool.click(&mut doc, screen, at(screen, 2.5, 0.0) + Vec2::new(0.0, 2.0)).unwrap();
        tool.hover(&doc, screen, at(screen, 3.0, 4.0));
        tool.enter(&mut doc, "3 1/2").unwrap();
        let guides: Vec<Guide> = doc.model().guides().map(|(_, g)| *g).collect();
        let [Guide::Line { point, dir }] = guides[..] else { panic!("{guides:?}") };
        assert!(dir.cross(DVec3::X).length() < 1e-9, "parallel to the edge");
        assert!((point.y - 3.5).abs() < 1e-9 && point.z.abs() < 1e-9, "{point}");
        assert_eq!(tool.measurement(), "3 1/2\"");
        // Typing again moves it, in one undo step.
        tool.enter(&mut doc, "2").unwrap();
        let (_, &Guide::Line { point, .. }) = doc.model().guides().next().unwrap() else { panic!() };
        assert!((point.y - 2.0).abs() < 1e-9);
        assert_eq!(doc.model().guides().count(), 1);
    }

    #[test]
    fn guide_lines_cross_edges_at_snappable_intersections() {
        let mut doc = board();
        doc.edit(|m| m.add_guide(Guide::Line { point: DVec3::new(0.0, 3.5, 0.0), dir: DVec3::X }));
        let camera = camera();
        let screen = Screen::new(&camera, SIZE);
        // Near where the guide meets the right edge.
        let cursor = at(screen, 10.0, 3.5) + Vec2::new(4.0, 3.0);
        let found = snap(doc.view().model(), screen, cursor, None).unwrap();
        assert_eq!(found.kind, SnapKind::Intersection);
        assert!(found.point.abs_diff_eq(DVec3::new(10.0, 3.5, 0.0), 1e-9), "{}", found.point);
        // Further along it, the guide line itself.
        let along = at(screen, 13.0, 3.5) + Vec2::new(0.0, 3.0);
        assert_eq!(snap(doc.view().model(), screen, along, None).unwrap().kind, SnapKind::OnGuide);
    }

    #[test]
    fn from_a_point_it_leaves_a_guide_point_or_just_measures() {
        let mut doc = board();
        let camera = camera();
        let screen = Screen::new(&camera, SIZE);
        let mut tool = TapeTool::default();
        tool.click(&mut doc, screen, at(screen, 10.0, 10.0)).unwrap();
        tool.lock_axis(Axis::X);
        tool.hover(&doc, screen, at(screen, 14.0, 10.0));
        tool.enter(&mut doc, "24").unwrap();
        let (_, &guide) = doc.model().guides().next().unwrap();
        assert_eq!(guide, Guide::Point(DVec3::new(34.0, 10.0, 0.0)));

        let before = doc.model_version();
        let mut overlay = Overlay::default();
        tool.overlay(&mut overlay);
        assert_eq!(overlay.cursor_note, Some("+ Guide"));
        tool.toggle_guides();
        let mut overlay = Overlay::default();
        tool.overlay(&mut overlay);
        assert_eq!(overlay.cursor_note, Some("Measure"), "Ctrl switched to measuring only");
        tool.click(&mut doc, screen, at(screen, 0.0, 0.0)).unwrap();
        tool.click(&mut doc, screen, at(screen, 10.0, 0.0)).unwrap();
        assert_eq!(tool.measurement(), "10\"");
        assert_eq!(doc.model_version(), before, "measuring only changes nothing");
    }

    #[test]
    fn guides_inside_a_closed_group_still_snap() {
        let mut doc = Document::default();
        let face = doc.edit(|m| m.add_rectangle(DVec3::ZERO, DVec3::X * 4.0, DVec3::Y * 4.0, DVec3::Z)).unwrap();
        let board = doc.push_pull(face, 1.0, true).unwrap().unwrap();
        doc.enter(board);
        doc.edit(|m| m.add_guide(Guide::Line { point: DVec3::new(0.0, 8.0, 0.0), dir: DVec3::X }));
        doc.exit();
        let camera = camera();
        let screen = Screen::new(&camera, SIZE);
        let found = snap(doc.view().model(), screen, at(screen, 9.0, 8.0) + Vec2::new(0.0, 2.0), None).unwrap();
        assert_eq!(found.kind, SnapKind::OnGuide);
    }

    #[test]
    fn rectangles_start_at_guide_intersections() {
        let mut doc = Document::default();
        doc.edit(|m| m.add_guide(Guide::Line { point: DVec3::new(0.0, 2.0, 0.0), dir: DVec3::X }));
        doc.edit(|m| m.add_guide(Guide::Line { point: DVec3::new(3.0, 0.0, 0.0), dir: DVec3::Y }));
        let camera = camera();
        let screen = Screen::new(&camera, SIZE);
        let found = snap(doc.view().model(), screen, at(screen, 3.0, 2.0) + Vec2::new(3.0, 3.0), None).unwrap();
        assert_eq!(found.kind, SnapKind::Intersection);
        assert!(found.point.abs_diff_eq(DVec3::new(3.0, 2.0, 0.0), 1e-9));
    }
}
