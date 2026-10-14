//! The Arc tool (A), SketchUp's 2-point arc. Click the start, click the end (or type the chord
//! length), then move to bulge it out and click, or type the bulge. `5r` sets the radius
//! instead, `24s` the number of sides (12 by default). Typing right after drawing changes the
//! arc just drawn. An arc across a face's corner splits off the corner, ready to push away.

use glam::{DVec3, Vec2};
use sketchcraft_doc::curves::{ARC_SEGMENTS, arc_points};
use sketchcraft_doc::units::{format_length, parse_length};
use sketchcraft_doc::{Document, EPS};

use crate::circle::parse_segments;
use crate::move_tool::parse_offset;
use crate::pick::{Screen, Snap, snap};
use crate::{Guide, Overlay};

pub struct ArcTool {
    stage: Stage,
    hover: Option<Snap>,
    segments: u32,
    last: Option<Drawn>,
}

#[derive(Clone, Copy)]
enum Stage {
    Start,
    /// The first end is placed; `end` follows the cursor.
    End { a: DVec3, normal: DVec3, end: DVec3 },
    /// Both ends placed; `bulge` (signed across the chord) follows the cursor.
    Bulge { a: DVec3, b: DVec3, normal: DVec3, bulge: f64 },
}

#[derive(Clone, Copy)]
struct Drawn {
    a: DVec3,
    b: DVec3,
    normal: DVec3,
    bulge: f64,
    model_version: u64,
}

impl Default for ArcTool {
    fn default() -> Self {
        Self { stage: Stage::Start, hover: None, segments: ARC_SEGMENTS, last: None }
    }
}

/// The direction across the chord from `a` to `b` that a positive bulge goes.
fn across(a: DVec3, b: DVec3, normal: DVec3) -> DVec3 {
    normal.cross(b - a).normalize_or_zero()
}

fn points(a: DVec3, b: DVec3, normal: DVec3, bulge: f64, segments: u32) -> Vec<DVec3> {
    let side = across(a, b, normal) * bulge.signum();
    arc_points(a, b, normal, bulge.abs(), side, segments)
}

impl ArcTool {
    pub fn is_busy(&self) -> bool {
        !matches!(self.stage, Stage::Start)
    }

    pub fn cancel(&mut self) {
        self.stage = Stage::Start;
        self.last = None;
    }

    pub fn hover(&mut self, doc: &Document, screen: Screen<'_>, cursor: Vec2) {
        let model = doc.view().model();
        match &mut self.stage {
            Stage::Start => self.hover = snap(model, screen, cursor, None),
            Stage::End { a, normal, end } => {
                self.hover = snap(model, screen, cursor, Some((*a, *normal)));
                if let Some(h) = self.hover {
                    *end = h.point;
                }
            }
            Stage::Bulge { a, b, normal, bulge } => {
                self.hover = snap(model, screen, cursor, Some((*a, *normal)));
                if let Some(h) = self.hover {
                    *bulge = (h.point - (*a + *b) * 0.5).dot(across(*a, *b, *normal));
                }
            }
        }
    }

    pub fn click(&mut self, doc: &mut Document, screen: Screen<'_>, cursor: Vec2) -> Result<(), String> {
        self.hover(doc, screen, cursor);
        match self.stage {
            Stage::Start => {
                if let Some(h) = self.hover {
                    self.last = None;
                    self.stage = Stage::End { a: h.point, normal: h.normal, end: h.point };
                }
                Ok(())
            }
            Stage::End { a, normal, end } => self.set_end(a, normal, end),
            Stage::Bulge { a, b, normal, bulge } => self.draw(doc, a, b, normal, bulge),
        }
    }

    pub fn enter(&mut self, doc: &mut Document, typed: &str) -> Result<(), String> {
        if let Some(segments) = parse_segments(typed) {
            self.segments = segments?;
            return match self.last {
                Some(l) if l.model_version == doc.model_version() => {
                    doc.undo();
                    self.draw(doc, l.a, l.b, l.normal, l.bulge)
                }
                _ => Ok(()),
            };
        }
        match self.stage {
            Stage::Start => match self.last {
                Some(l) if l.model_version == doc.model_version() => {
                    let bulge = Self::typed_bulge(typed, l.a, l.b, l.bulge)?;
                    doc.undo();
                    self.draw(doc, l.a, l.b, l.normal, bulge)
                }
                _ => Err("Click the arc's start, then its end".into()),
            },
            Stage::End { a, normal, end } => {
                let offset = parse_offset(typed, end - a, a)?;
                self.set_end(a, normal, a + offset)
            }
            Stage::Bulge { a, b, normal, bulge } => {
                let bulge = Self::typed_bulge(typed, a, b, bulge)?;
                self.draw(doc, a, b, normal, bulge)
            }
        }
    }

    /// A typed bulge, or with `r` a radius; on the side the arc currently bulges to.
    fn typed_bulge(typed: &str, a: DVec3, b: DVec3, current: f64) -> Result<f64, String> {
        let typed = typed.trim();
        let sign = if current < 0.0 { -1.0 } else { 1.0 };
        if let Some(radius) = typed.strip_suffix(['r', 'R']) {
            let radius = parse_length(radius).map_err(|e| e.to_string())?.abs();
            let half = a.distance(b) * 0.5;
            if radius < half - EPS {
                return Err(format!("The radius must be at least half the chord ({})", format_length(half)));
            }
            return Ok(sign * (radius - (radius * radius - half * half).max(0.0).sqrt()));
        }
        Ok(sign * parse_length(typed).map_err(|e| e.to_string())?.abs())
    }

    pub fn status(&self) -> String {
        let sides = self.segments;
        match self.stage {
            Stage::Start => format!("Select start point. Sides: {sides} (type 24s to change)."),
            Stage::End { .. } => "Select end point or enter the chord length.".into(),
            Stage::Bulge { .. } => "Select bulge, or enter it (5r for a radius).".into(),
        }
    }

    pub fn measurement_label(&self) -> &'static str {
        match self.stage {
            Stage::End { .. } => "Length",
            _ => "Bulge",
        }
    }

    pub fn measurement(&self) -> String {
        match (self.stage, self.last) {
            (Stage::End { a, end, .. }, _) => format_length(a.distance(end)),
            (Stage::Bulge { bulge, .. }, _) => format_length(bulge.abs()),
            (Stage::Start, Some(last)) => format_length(last.bulge.abs()),
            _ => String::new(),
        }
    }

    pub fn overlay(&self, overlay: &mut Overlay) {
        overlay.snap = self.hover;
        match self.stage {
            Stage::Start => {}
            Stage::End { a, end, .. } => {
                overlay.guide = Some(Guide { from: a, to: end, axis: None });
            }
            Stage::Bulge { a, b, normal, bulge } => {
                for w in points(a, b, normal, bulge, self.segments).windows(2) {
                    overlay.lines.push([w[0], w[1]]);
                }
            }
        }
    }

    fn set_end(&mut self, a: DVec3, normal: DVec3, b: DVec3) -> Result<(), String> {
        if a.distance(b) < EPS {
            return Err("Pick an end point away from the start".into());
        }
        self.stage = Stage::Bulge { a, b, normal, bulge: 0.0 };
        Ok(())
    }

    fn draw(&mut self, doc: &mut Document, a: DVec3, b: DVec3, normal: DVec3, bulge: f64) -> Result<(), String> {
        let points = points(a, b, normal, bulge, self.segments);
        doc.edit(|m| m.draw_polyline(&points, normal));
        self.stage = Stage::Start;
        self.last = Some(Drawn { a, b, normal, bulge, model_version: doc.model_version() });
        Ok(())
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

    #[test]
    fn round_a_corner_with_a_typed_radius() {
        let mut doc = Document::default();
        doc.edit(|m| m.add_rectangle(DVec3::ZERO, DVec3::X * 10.0, DVec3::Y * 10.0, DVec3::Z));
        let camera = camera();
        let screen = Screen::new(&camera, SIZE);
        let mut tool = ArcTool::default();
        // Points 2" from the (10, 10) corner, on its two edges.
        tool.click(&mut doc, screen, at(screen, 8.0, 10.0)).unwrap();
        tool.click(&mut doc, screen, at(screen, 10.0, 8.0)).unwrap();
        tool.hover(&doc, screen, at(screen, 9.8, 9.8));
        tool.enter(&mut doc, "2r").unwrap();
        assert_eq!(doc.model().faces().count(), 2, "the corner is split off");
        let corner = DVec3::new(8.0, 8.0, 0.0);
        let on_arc = doc.model().vertices().filter(|(_, p)| (p.distance(corner) - 2.0).abs() < 1e-6).count();
        assert_eq!(on_arc, 13, "a quarter circle of radius 2 in 12 pieces");
        // 6 sides now, as one undo step.
        tool.enter(&mut doc, "6s").unwrap();
        let on_arc = doc.model().vertices().filter(|(_, p)| (p.distance(corner) - 2.0).abs() < 1e-6).count();
        assert_eq!(on_arc, 7);
        doc.undo();
        assert_eq!(doc.model().faces().count(), 1);
    }

    #[test]
    fn bulge_follows_the_mouse_side() {
        let mut doc = Document::default();
        let camera = camera();
        let screen = Screen::new(&camera, SIZE);
        let mut tool = ArcTool::default();
        tool.click(&mut doc, screen, at(screen, 0.0, 0.0)).unwrap();
        tool.click(&mut doc, screen, at(screen, 4.0, 0.0)).unwrap();
        tool.hover(&doc, screen, at(screen, 2.0, -1.0));
        tool.enter(&mut doc, "2").unwrap();
        let (lo, hi) = doc.model().bounds().unwrap();
        // The clicked end is only as exact as the mouse.
        assert!((lo.y + 2.0).abs() < 1e-3 && hi.y.abs() < 1e-3, "a half circle below: {lo} {hi}");
    }
}
