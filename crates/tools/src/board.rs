//! The Board tool (W): draw a board straight from stock, as a woodworker would pick one off
//! the rack.
//!
//! - Pick the stock in the Board panel (a 2x4 of Pine Lumber, a 3/4" Baltic Birch sheet).
//! - Lumber: click where the board starts and where it ends; it lies flat on the surface you
//!   start on (Ctrl stands it on edge). Inference works as for the Line tool (axes, edges,
//!   arrow-key locks, Shift).
//! - Sheets and solid wood: click two opposite corners, like the Rectangle tool; the board
//!   rises from the surface by the stock's thickness.
//! - Type a length (`96`, `8'`), or for sheets `length,width`; or a whole board: `2x4x96`,
//!   `3/4x11 1/4x30`. Typing again right after changes the board just drawn.
//!
//! Each board is its own group, painted with the stock's material, with its grain along its
//! length.

use glam::{DVec3, Vec2};
use woodsketch_doc::board::{Board, Stock, TypedBoard, default_stock, parse_board, stock_options};
use woodsketch_doc::units::{format_length, parse_length, parse_lengths};
use woodsketch_doc::{Document, EPS, MaterialId};

use crate::infer::{self, Direction, Hints, Query, Target};
use crate::pick::{Screen, Snap, snap};
use crate::rectangle::plane_axes;
use crate::{Axis, Guide, Overlay};

/// The stock the Board tool draws with.
#[derive(Clone, Debug, PartialEq)]
pub struct Picked {
    pub material: MaterialId,
    pub stock: Stock,
    /// Like `2x4` or `3/4" sheet`.
    pub name: String,
}

#[derive(Default)]
pub struct BoardTool {
    pub picked: Option<Picked>,
    /// Lumber stands on its edge instead of lying flat (Ctrl toggles).
    pub on_edge: bool,
    start: Option<Start>,
    hover: Option<Snap>,
    /// Where the board under the cursor ends (lumber), or its opposite corner (sheets).
    end: Option<Target>,
    lock: Option<Direction>,
    shift_lock: Option<Direction>,
    shift: bool,
    hints: Hints,
    /// The board just drawn, which typing can still change.
    last: Option<Last>,
}

#[derive(Clone, Copy)]
struct Start {
    point: DVec3,
    /// The surface it starts on: lumber lies flat on it, sheets rise from it.
    normal: DVec3,
}

struct Last {
    start: Start,
    /// Direction of the length (lumber), or the rectangle's signed extents (sheets).
    dir: DVec3,
    extents: (f64, f64),
    stock: Stock,
    model_version: u64,
}

impl BoardTool {
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

    pub fn set_shift(&mut self, shift: bool) {
        self.shift = shift;
        if !shift {
            self.shift_lock = None;
        }
    }

    pub fn toggle_on_edge(&mut self) {
        self.on_edge = !self.on_edge;
    }

    /// Keep the picked stock valid: if nothing is picked or its material is gone, pick the
    /// first stock of the first material that has some.
    pub fn ensure_stock(&mut self, doc: &Document) {
        if self.picked.as_ref().is_some_and(|p| doc.scene().material(p.material).is_some()) {
            return;
        }
        self.picked = doc.scene().materials().find_map(|(id, m)| {
            let (name, stock) = default_stock(m)?;
            Some(Picked { material: id, stock, name })
        });
    }

    fn is_lumber(&self) -> bool {
        matches!(self.picked.as_ref().map(|p| p.stock), Some(Stock::Section { .. }))
    }

    pub fn hover(&mut self, doc: &Document, screen: Screen<'_>, cursor: Vec2) {
        let model = doc.view().model();
        let Some(start) = self.start else {
            self.hover = snap(model, screen, cursor, None);
            self.hints.note(self.hover.as_ref());
            self.end = None;
            return;
        };
        if !self.is_lumber() {
            // A rectangle on the surface it started on, as with the Rectangle tool.
            self.hover = snap(model, screen, cursor, Some((start.point, start.normal)));
            self.end = self.hover.map(|s| Target { point: s.point, direction: None, snap: Some(s), from: None });
            return;
        }
        let base = start.point;
        let query = Query {
            base,
            hints: &self.hints,
            lock: self.lock.or(self.shift_lock),
            plane_normal: start.normal,
            current: self.end.map_or(base, |t| t.point),
        };
        let target = infer::target(model, screen, cursor, &query, |_| false, |ray| {
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
        self.ensure_stock(doc);
        if self.picked.is_none() {
            return Err("Add a wood material first (Materials panel, +), then pick its stock in the Board panel.".into());
        }
        self.hover(doc, screen, cursor);
        match self.start {
            None => {
                let Some(hover) = self.hover else { return Ok(()) };
                self.last = None;
                self.start = Some(Start { point: hover.point, normal: hover.normal.normalize() });
                Ok(())
            }
            Some(start) => {
                let end = self.end.map_or(start.point, |t| t.point);
                self.finish(doc, start, end - start.point)
            }
        }
    }

    /// Draw the board from `start` reaching `reach` (lumber: along its length; sheets: to the
    /// opposite corner).
    fn finish(&mut self, doc: &mut Document, start: Start, reach: DVec3) -> Result<(), String> {
        let Some(picked) = self.picked.clone() else { return Ok(()) };
        let (dir, extents) = match picked.stock {
            Stock::Section { .. } => (reach.normalize_or_zero(), (reach.length(), 0.0)),
            Stock::Thickness(_) => {
                let (u, v) = plane_axes(start.normal);
                (DVec3::ZERO, (reach.dot(u), reach.dot(v)))
            }
        };
        let board = self.board(start, dir, extents, picked.stock).ok_or("Pick a length first: move away from the start.")?;
        self.start = None;
        self.end = None;
        doc.add_board(&board, Some(picked.material)).ok_or("That board has no size.")?;
        self.last = Some(Last {
            start,
            dir,
            extents,
            stock: picked.stock,
            model_version: doc.model_version(),
        });
        Ok(())
    }

    /// The board drawn from `start`: lumber along `dir` for `extents.0`; sheets over the signed
    /// rectangle `extents` on the start surface, its longer side as the length.
    fn board(&self, start: Start, dir: DVec3, extents: (f64, f64), stock: Stock) -> Option<Board> {
        let n = start.normal;
        match stock {
            Stock::Section { thickness, width } => {
                let length = extents.0;
                if length < EPS || dir.length() < 0.5 {
                    return None;
                }
                // Flat on the surface: the width across it. Drawn straight off the surface (a
                // stud from the floor), the width follows the surface's first axis.
                let across = n.cross(dir);
                let flat_width = if across.length() > 1e-6 { across.normalize() } else { plane_axes(n).0 };
                let width_dir = if self.on_edge { dir.cross(flat_width) } else { flat_width };
                Some(Board { origin: start.point, length_dir: dir, width_dir, length, width, thickness })
            }
            Stock::Thickness(thickness) => {
                let (fu, fv) = plane_axes(n);
                let (eu, ev) = extents;
                if eu.abs() < EPS || ev.abs() < EPS {
                    return None;
                }
                let (a, b) = (fu * eu.signum(), fv * ev.signum());
                let (mut length_dir, mut width_dir, length, width) =
                    if eu.abs() >= ev.abs() { (a, b, eu.abs(), ev.abs()) } else { (b, a, ev.abs(), eu.abs()) };
                let mut origin = start.point;
                // Rise off the surface (the thickness along its normal).
                if length_dir.cross(width_dir).dot(n) < 0.0 {
                    origin += width_dir * width;
                    width_dir = -width_dir;
                }
                length_dir = length_dir.normalize();
                Some(Board { origin, length_dir, width_dir, length, width, thickness })
            }
        }
    }

    /// The board under the cursor, for the preview.
    fn preview(&self) -> Option<Board> {
        let start = self.start?;
        let stock = self.picked.as_ref()?.stock;
        let reach = self.end?.point - start.point;
        match stock {
            Stock::Section { .. } => self.board(start, reach.normalize_or_zero(), (reach.length(), 0.0), stock),
            Stock::Thickness(_) => {
                let (u, v) = plane_axes(start.normal);
                self.board(start, DVec3::ZERO, (reach.dot(u), reach.dot(v)), stock)
            }
        }
    }

    /// Typed values: a length (lumber) or `length,width` (sheets) for the board being drawn or
    /// the one just drawn; or a whole board like `2x4x96`, which also picks that stock.
    pub fn enter(&mut self, doc: &mut Document, typed: &str) -> Result<(), String> {
        if let Some(whole) = parse_board(typed) {
            return self.enter_whole(doc, whole);
        }
        // With lumber, `thickness,width` picks that section (added to the material if it's new).
        if self.is_lumber()
            && let Ok(values) = parse_lengths(typed)
            && let [Some(thickness), Some(width)] = values[..]
        {
            return self.pick_matching(doc, Stock::Section { thickness, width });
        }
        let stock = self.picked.as_ref().map(|p| p.stock).ok_or("Pick stock in the Board panel first.")?;
        // The board being drawn, else the one just drawn (redrawn in place, one undo step).
        let (start, dir, extents, redo) = match (self.start, self.last.as_ref()) {
            (Some(start), _) => {
                let reach = self.end.map_or(DVec3::ZERO, |t| t.point - start.point);
                let (u, v) = plane_axes(start.normal);
                (start, reach.normalize_or_zero(), (reach.dot(u), reach.dot(v)), false)
            }
            (None, Some(last)) if last.model_version == doc.model_version() && last.stock == stock => {
                (last.start, last.dir, last.extents, true)
            }
            _ => return Err("Click where the board starts, then type its length (or type 2x4x96).".into()),
        };
        let reach = match stock {
            Stock::Section { .. } => {
                let length = parse_length(typed).map_err(|e| e.to_string())?;
                if length <= EPS {
                    return Err("The length must be greater than zero.".into());
                }
                if dir.length() < 0.5 {
                    return Err("Move the mouse the way the board should run, then type its length.".into());
                }
                dir * length
            }
            Stock::Thickness(_) => {
                let values = parse_lengths(typed).map_err(|e| e.to_string())?;
                let [a, b] = values[..] else { return Err("Type length and width, like 30,11 1/4".into()) };
                // Each keeps the mouse's direction; the longer one is the length either way.
                let signed = |typed: Option<f64>, current: f64| typed.map_or(current, |t| if current < 0.0 { -t } else { t });
                let (u, v) = plane_axes(start.normal);
                u * signed(a, extents.0) + v * signed(b, extents.1)
            }
        };
        if redo {
            doc.undo();
        }
        self.finish(doc, start, reach)
    }

    /// `2x4x96`: pick that stock (from a material that has it, adding Pine Lumber for a lumber
    /// section if none does) and draw it from the start point, or place it at the start of the
    /// next click.
    fn enter_whole(&mut self, doc: &mut Document, typed: TypedBoard) -> Result<(), String> {
        // Not a nominal size, with lumber picked (or nothing yet): a custom lumber section.
        let lumber = self.picked.as_ref().is_none_or(|p| matches!(p.stock, Stock::Section { .. }));
        let stock = match typed.stock {
            Stock::Thickness(thickness) if lumber => Stock::Section { thickness, width: typed.width },
            stock => stock,
        };
        self.pick_matching(doc, stock)?;
        let Some(picked) = self.picked.clone() else { return Ok(()) };
        let start = match (self.start, self.last.as_ref()) {
            (Some(start), _) => start,
            (None, Some(last)) if last.model_version == doc.model_version() => {
                doc.undo();
                last.start
            }
            _ => return Ok(()),
        };
        let mouse = self.end.map_or(DVec3::ZERO, |t| t.point - start.point);
        let reach = match picked.stock {
            Stock::Section { .. } => {
                let dir = if mouse.length() > EPS { mouse.normalize() } else { self.last.as_ref().map_or(DVec3::X, |l| l.dir) };
                dir * typed.length
            }
            Stock::Thickness(_) => {
                let (u, v) = plane_axes(start.normal);
                let sign = |x: f64| if x < 0.0 { -1.0 } else { 1.0 };
                u * typed.length * sign(mouse.dot(u)) + v * typed.width * sign(mouse.dot(v))
            }
        };
        self.finish(doc, start, reach)
    }

    /// Pick `stock`: the current material's, else any material's that has it. A lumber section
    /// no material has comes from Pine Lumber (added from the library); a thickness no
    /// material lists is drawn from the current sheet or solid-wood material anyway.
    fn pick_matching(&mut self, doc: &mut Document, stock: Stock) -> Result<(), String> {
        let close = |a: Stock, b: Stock| match (a, b) {
            (Stock::Section { thickness: t1, width: w1 }, Stock::Section { thickness: t2, width: w2 }) => {
                (t1 - t2).abs() < 1e-6 && (w1 - w2).abs() < 1e-6
            }
            (Stock::Thickness(a), Stock::Thickness(b)) => (a - b).abs() < 1e-6,
            _ => false,
        };
        let current = self.picked.as_ref().map(|p| p.material);
        let mut candidates: Vec<MaterialId> = current.into_iter().collect();
        candidates.extend(doc.scene().materials().map(|(id, _)| id).filter(|id| Some(*id) != current));
        for id in candidates {
            let Some(material) = doc.scene().material(id) else { continue };
            if let Some((name, found)) = stock_options(material).into_iter().find(|(_, s)| close(*s, stock)) {
                self.picked = Some(Picked { material: id, stock: found, name });
                return Ok(());
            }
        }
        match stock {
            Stock::Section { .. } => {
                // The current lumber, else Pine Lumber from the library; a size it doesn't list yet
                // is added to it, so the cut list knows it.
                let id = match self.picked.as_ref().filter(|p| matches!(p.stock, Stock::Section { .. })) {
                    Some(p) => p.material,
                    None => {
                        let pine = woodsketch_doc::library::LIBRARY.iter().find(|p| p.name == "Pine Lumber").expect("in the library");
                        doc.add_preset(pine)
                    }
                };
                let material = doc.scene().material(id).expect("a material in the drawing");
                let found = stock_options(material).into_iter().find(|(_, s)| close(*s, stock));
                let (name, found) = match found {
                    Some(found) => found,
                    None => doc.add_stock(id, stock).ok_or("That size has no thickness or width.")?,
                };
                self.picked = Some(Picked { material: id, stock: found, name });
                Ok(())
            }
            Stock::Thickness(t) => {
                let picked = self.picked.as_mut().filter(|p| matches!(p.stock, Stock::Thickness(_)));
                let Some(picked) = picked else {
                    return Err("Pick a sheet or solid-wood material in the Board panel first.".into());
                };
                picked.stock = stock;
                picked.name = format!("{} (not a standard thickness)", format_length(t));
                Ok(())
            }
        }
    }

    pub fn status(&self) -> String {
        let Some(picked) = &self.picked else {
            return "Board: add a wood material (Materials panel, +) to draw boards from its stock.".into();
        };
        match (self.start, picked.stock) {
            (None, Stock::Section { .. }) => {
                format!("{}: click where the board starts. Type 2x4x96, or 3,6 1/2 for a custom size. Ctrl stands it on edge.", picked.name)
            }
            (None, Stock::Thickness(_)) => format!("{}: click a corner of the board.", picked.name),
            (Some(_), Stock::Section { .. }) => "Click where it ends, or type the length. Arrows lock an axis.".into(),
            (Some(_), Stock::Thickness(_)) => "Click the opposite corner, or type length,width.".into(),
        }
    }

    pub fn measurement_label(&self) -> &'static str {
        if self.is_lumber() { "Length" } else { "Dimensions" }
    }

    pub fn measurement(&self) -> String {
        if let Some(board) = self.preview() {
            return match self.picked.as_ref().map(|p| p.stock) {
                Some(Stock::Section { .. }) => format_length(board.length),
                _ => format!("{}, {}", format_length(board.length), format_length(board.width)),
            };
        }
        String::new()
    }

    pub fn overlay(&self, overlay: &mut Overlay) {
        overlay.snap = self.hover;
        if let Some(board) = self.preview() {
            overlay.lines.extend(board.edges());
        }
        if let (Some(start), Some(end)) = (self.start, self.end)
            && self.is_lumber()
        {
            overlay.guide = Some(Guide { from: start.point, to: end.point, along: end.direction.map(|d| d.along) });
            overlay.alignment = end.from.map(|(from, axis)| (from, end.point, axis));
        }
    }
}

#[cfg(test)]
mod tests {
    use woodsketch_doc::library::LIBRARY;
    use woodsketch_view::{OrbitCamera, StandardView};

    use super::*;

    const SIZE: Vec2 = Vec2::new(1000.0, 800.0);

    fn camera() -> OrbitCamera {
        let mut camera = OrbitCamera { target: glam::Vec3::new(20.0, 20.0, 0.0), distance: 200.0, ..Default::default() };
        camera.set_view(StandardView::Top);
        camera
    }

    fn at(screen: Screen<'_>, x: f64, y: f64) -> Vec2 {
        screen.project(DVec3::new(x, y, 0.0)).unwrap()
    }

    fn with(doc: &mut Document, preset: &str) -> BoardTool {
        doc.add_preset(LIBRARY.iter().find(|p| p.name == preset).unwrap());
        let mut tool = BoardTool::default();
        tool.ensure_stock(doc);
        tool
    }

    #[test]
    fn a_two_by_four_lies_flat_along_the_drag_with_a_typed_length() {
        let mut doc = Document::default();
        let mut tool = with(&mut doc, "Pine Lumber");
        tool.pick_matching(&mut doc, Stock::Section { thickness: 1.5, width: 3.5 }).unwrap();
        assert_eq!(tool.picked.as_ref().unwrap().name, "2x4");
        let camera = camera();
        let screen = Screen::new(&camera, SIZE);
        tool.click(&mut doc, screen, at(screen, 0.0, 0.0)).unwrap();
        tool.hover(&doc, screen, at(screen, 40.0, 0.0));
        tool.enter(&mut doc, "8'").unwrap();
        assert!(!tool.is_busy());
        let (lo, hi) = doc.bounds().unwrap();
        assert!((hi - lo).abs_diff_eq(DVec3::new(96.0, 3.5, 1.5), 1e-9), "{:?}", hi - lo);
        let part = doc.container().instances().next().unwrap().0;
        assert_eq!(doc.part_size(part), Some([96.0, 3.5, 1.5]));
        // Typing again changes it, in one undo step.
        tool.enter(&mut doc, "84").unwrap();
        let (lo, hi) = doc.bounds().unwrap();
        assert!(((hi - lo).x - 84.0).abs() < 1e-9);
        doc.undo();
        assert!(doc.bounds().is_none());
    }

    #[test]
    fn ctrl_stands_lumber_on_edge() {
        let mut doc = Document::default();
        let mut tool = with(&mut doc, "Pine Lumber");
        tool.pick_matching(&mut doc, Stock::Section { thickness: 1.5, width: 3.5 }).unwrap();
        tool.toggle_on_edge();
        let camera = camera();
        let screen = Screen::new(&camera, SIZE);
        tool.click(&mut doc, screen, at(screen, 0.0, 0.0)).unwrap();
        tool.hover(&doc, screen, at(screen, 0.0, 40.0));
        tool.enter(&mut doc, "30").unwrap();
        let (lo, hi) = doc.bounds().unwrap();
        let size = hi - lo;
        assert!((size.y - 30.0).abs() < 1e-9 && (size.z - 3.5).abs() < 1e-9 && (size.x - 1.5).abs() < 1e-9, "{size}");
        assert!(lo.z.abs() < 1e-9, "stands on the ground");
    }

    #[test]
    fn a_sheet_is_drawn_as_a_rectangle_at_its_thickness() {
        let mut doc = Document::default();
        let mut tool = with(&mut doc, "Baltic Birch Plywood");
        tool.pick_matching(&mut doc, Stock::Thickness(0.75)).unwrap();
        let camera = camera();
        let screen = Screen::new(&camera, SIZE);
        tool.click(&mut doc, screen, at(screen, 0.0, 0.0)).unwrap();
        tool.hover(&doc, screen, at(screen, 10.0, 30.0));
        tool.enter(&mut doc, "11 1/4,30").unwrap();
        let (lo, hi) = doc.bounds().unwrap();
        assert!(lo.abs_diff_eq(DVec3::ZERO, 1e-9), "{lo}");
        assert!(hi.abs_diff_eq(DVec3::new(11.25, 30.0, 0.75), 1e-9), "{hi}");
        let part = doc.container().instances().next().unwrap().0;
        assert_eq!(doc.part_size(part), Some([30.0, 11.25, 0.75]), "length along the longer side");
    }

    #[test]
    fn typing_a_custom_lumber_size_adds_it_to_the_material() {
        let mut doc = Document::default();
        let mut tool = with(&mut doc, "Pine Lumber");
        let camera = camera();
        let screen = Screen::new(&camera, SIZE);
        tool.enter(&mut doc, "3,6 1/2").unwrap();
        let custom = Stock::Section { thickness: 3.0, width: 6.5 };
        let picked = tool.picked.clone().unwrap();
        assert_eq!(picked.stock, custom);
        assert!(stock_options(doc.scene().material(picked.material).unwrap()).iter().any(|(_, s)| *s == custom), "Pine Lumber lists it now");
        assert_eq!(doc.scene().materials().count(), 1, "no new material");
        // A whole custom board, while drawing.
        tool.click(&mut doc, screen, at(screen, 0.0, 0.0)).unwrap();
        tool.hover(&doc, screen, at(screen, 40.0, 0.0));
        tool.enter(&mut doc, "1 1/4x2 1/2x30").unwrap();
        let (lo, hi) = doc.bounds().unwrap();
        assert!((hi - lo).abs_diff_eq(DVec3::new(30.0, 2.5, 1.25), 1e-9), "{:?}", hi - lo);
    }

    #[test]
    fn typing_a_whole_board_picks_its_stock_adding_pine_if_needed() {
        let mut doc = Document::default();
        let mut tool = BoardTool::default();
        let camera = camera();
        let screen = Screen::new(&camera, SIZE);
        tool.enter(&mut doc, "2x6x96").unwrap();
        assert_eq!(tool.picked.as_ref().unwrap().name, "2x6");
        assert!(doc.scene().materials().any(|(_, m)| m.name == "Pine Lumber"));
        assert!(doc.bounds().is_none(), "nothing drawn until a start point");
        tool.click(&mut doc, screen, at(screen, 0.0, 0.0)).unwrap();
        tool.hover(&doc, screen, at(screen, 0.0, 30.0));
        tool.enter(&mut doc, "2x4x36").unwrap();
        let (lo, hi) = doc.bounds().unwrap();
        assert!((hi - lo).abs_diff_eq(DVec3::new(3.5, 36.0, 1.5), 1e-9), "{:?}", hi - lo);
    }
}
