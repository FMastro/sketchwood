//! The Scale tool (S). Hover something (or select it first) to see its scale box: grips at the
//! corners scale it uniformly, grips on the box's edges scale two directions, and grips in the
//! middle of its faces stretch one. Click a grip, move, and click again, or type the scale:
//! a factor (`2`, `0.5`, `1.5,2` for two directions) or a new size with units (`36"`). The
//! opposite grip stays put; tap Ctrl to scale about the center instead. Typing right after
//! changes the scale just made.
//!
//! A single group or component scales along its own axes (its box turns with it); anything
//! else along the red, green and blue axes.

use glam::{DVec3, Vec2};
use sketchcraft_doc::units::parse_lengths;
use sketchcraft_doc::{Document, EPS, Entity, scaling};

use crate::pick::{Screen, Snap, closest_on_line, snap_feature};
use crate::{Overlay, pick_resolved};

/// How close, in points, the cursor must be to a grip.
const GRIP_RADIUS: f32 = 9.0;

#[derive(Default)]
pub struct ScaleTool {
    /// The scale box of what would be scaled, while hovering.
    target: Option<Target>,
    hovered: Option<[usize; 3]>,
    op: Option<Op>,
    about_center: bool,
    snap: Option<Snap>,
    last: Option<Done>,
}

/// What to scale and its box: `lo..hi` measured along `axes` from `origin`.
#[derive(Clone)]
struct Target {
    entities: Vec<Entity>,
    origin: DVec3,
    axes: [DVec3; 3],
    lo: DVec3,
    hi: DVec3,
}

impl Target {
    /// The box point at grip `k` (each coordinate 0 = low side, 1 = middle, 2 = high side).
    fn point(&self, k: [usize; 3]) -> DVec3 {
        (0..3).fold(self.origin, |p, i| {
            let t = k[i] as f64 * 0.5;
            p + self.axes[i] * (self.lo[i] + (self.hi[i] - self.lo[i]) * t)
        })
    }

    /// Every grip: corners, edge middles and face middles, skipping directions the box is flat in.
    fn grips(&self) -> Vec<[usize; 3]> {
        let mut grips = Vec::new();
        for a in 0..3 {
            for b in 0..3 {
                for c in 0..3 {
                    let k = [a, b, c];
                    let active = (0..3).filter(|&i| k[i] != 1).collect::<Vec<_>>();
                    let flat = active.iter().any(|&i| self.hi[i] - self.lo[i] < EPS);
                    if !active.is_empty() && !flat {
                        grips.push(k);
                    }
                }
            }
        }
        grips
    }

    fn edges(&self, factors: DVec3, anchor: DVec3) -> Vec<[DVec3; 2]> {
        let t = scaling(self.axes, anchor, factors);
        let corner = |i: usize| t.transform_point3(self.point([(i & 1) * 2, ((i >> 1) & 1) * 2, ((i >> 2) & 1) * 2]));
        let pairs = [(0, 1), (2, 3), (4, 5), (6, 7), (0, 2), (1, 3), (4, 6), (5, 7), (0, 4), (1, 5), (2, 6), (3, 7)];
        pairs.iter().map(|&(a, b)| [corner(a), corner(b)]).collect()
    }
}

struct Op {
    target: Target,
    grip: [usize; 3],
    anchor: DVec3,
    factors: DVec3,
    dragging: bool,
}

impl Op {
    fn active(&self) -> Vec<usize> {
        (0..3).filter(|&i| self.grip[i] != 1).collect()
    }
}

#[derive(Clone)]
struct Done {
    target: Target,
    grip: [usize; 3],
    anchor: DVec3,
    factors: DVec3,
    model_version: u64,
}

impl ScaleTool {
    pub fn is_busy(&self) -> bool {
        self.op.is_some()
    }

    pub fn cancel(&mut self) {
        self.op = None;
        self.last = None;
        self.target = None;
    }

    pub fn toggle_center(&mut self) {
        self.about_center = !self.about_center;
        if let Some(op) = &mut self.op {
            op.anchor = anchor(&op.target, op.grip, self.about_center);
        }
    }

    pub fn hover(&mut self, doc: &Document, screen: Screen<'_>, cursor: Vec2) {
        let Some(op) = &mut self.op else {
            // Keep the box while the cursor is on one of its grips (they can sit off the model).
            if let Some(target) = &self.target
                && let Some(k) = grip_at(target, screen, cursor)
            {
                self.hovered = Some(k);
                return;
            }
            self.target = target(doc, screen, cursor);
            self.hovered = self.target.as_ref().and_then(|t| grip_at(t, screen, cursor));
            return;
        };
        let model = doc.view().model();
        self.snap = snap_feature(model, screen, cursor, |_| false);
        let grip = op.target.point(op.grip);
        let ray = screen.ray(cursor);
        let active = op.active();
        let along = |i: usize| (grip - op.anchor).dot(op.target.axes[i]);
        let factors = match (active.len(), self.snap) {
            // A snapped point sets the size: the box reaches it.
            (3, Some(s)) => {
                let d = grip - op.anchor;
                DVec3::splat((s.point - op.anchor).dot(d) / d.length_squared())
            }
            (_, Some(s)) => factor_each(&active, |i| (s.point - op.anchor).dot(op.target.axes[i]) / along(i)),
            (3, None) => {
                let d = grip - op.anchor;
                let Some(t) = closest_on_line(op.anchor, d.normalize(), &ray) else { return };
                DVec3::splat(t / d.length())
            }
            (2, None) => {
                let normal = op.target.axes[(0..3).find(|i| !active.contains(i)).expect("one inactive")];
                let Some(hit) = ray.hit_plane(grip, normal) else { return };
                factor_each(&active, |i| (hit - op.anchor).dot(op.target.axes[i]) / along(i))
            }
            _ => {
                let i = active[0];
                let Some(t) = closest_on_line(op.anchor, op.target.axes[i], &ray) else { return };
                factor_each(&active, |_| t / along(i))
            }
        };
        op.factors = factors;
    }

    pub fn click(&mut self, doc: &mut Document, screen: Screen<'_>, cursor: Vec2) -> Result<(), String> {
        if let Some(op) = &self.op {
            let factors = op.factors;
            self.hover(doc, screen, cursor);
            let factors = self.op.as_ref().map_or(factors, |op| op.factors);
            return self.apply(doc, factors);
        }
        self.start(doc, screen, cursor, false)
    }

    pub fn drag_start(&mut self, doc: &mut Document, screen: Screen<'_>, origin: Vec2) -> Result<(), String> {
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
        let factors = self.op.as_ref().map_or(DVec3::ONE, |op| op.factors);
        self.apply(doc, factors)
    }

    pub fn enter(&mut self, doc: &mut Document, typed: &str) -> Result<(), String> {
        if let Some(op) = &self.op {
            let factors = typed_factors(typed, &op.target, &op.active())?;
            return self.apply(doc, factors);
        }
        match self.last.clone() {
            Some(last) if last.model_version == doc.model_version() => {
                let active: Vec<usize> = (0..3).filter(|&i| last.grip[i] != 1).collect();
                let factors = typed_factors(typed, &last.target, &active)?;
                doc.undo();
                self.op = Some(Op { target: last.target, grip: last.grip, anchor: last.anchor, factors, dragging: false });
                self.apply(doc, factors)
            }
            _ => Err("Click a scale grip, then type the scale".into()),
        }
    }

    pub fn status(&self) -> String {
        let center = if self.about_center { " About the center (tap Ctrl to anchor the opposite side)." } else { " Tap Ctrl to scale about the center." };
        if self.op.is_some() {
            format!("Move to scale, or type a factor (2, 0.5) or a size (36\").{center}")
        } else {
            "Click a grip: corners scale evenly, edges two ways, faces one way.".into()
        }
    }

    pub fn measurement(&self) -> String {
        let (factors, active) = match (&self.op, &self.last) {
            (Some(op), _) => (op.factors, op.active()),
            (None, Some(last)) => (last.factors, (0..3).filter(|&i| last.grip[i] != 1).collect()),
            _ => return String::new(),
        };
        if active.len() == 3 {
            return format!("{:.2}", factors.x);
        }
        active.iter().map(|&i| format!("{:.2}", factors[i])).collect::<Vec<_>>().join(", ")
    }

    pub fn overlay(&self, overlay: &mut Overlay) {
        if let Some(op) = &self.op {
            overlay.snap = self.snap;
            overlay.lines.extend(op.target.edges(op.factors, op.anchor));
            return;
        }
        let Some(target) = &self.target else { return };
        overlay.highlight.extend(target.edges(DVec3::ONE, target.origin));
        for k in target.grips() {
            overlay.grips.push((target.point(k), self.hovered == Some(k)));
        }
    }

    fn start(&mut self, doc: &Document, screen: Screen<'_>, cursor: Vec2, dragging: bool) -> Result<(), String> {
        self.hover(doc, screen, cursor);
        let (Some(target), Some(grip)) = (self.target.clone(), self.hovered) else {
            return Err("Click one of the scale grips (hover something to see them)".into());
        };
        self.last = None;
        let anchor = anchor(&target, grip, self.about_center);
        self.op = Some(Op { target, grip, anchor, factors: DVec3::ONE, dragging });
        Ok(())
    }

    fn apply(&mut self, doc: &mut Document, factors: DVec3) -> Result<(), String> {
        let Some(op) = &self.op else { return Ok(()) };
        if factors.min_element() < 1e-6 {
            return Err("Scale must be greater than zero (mirroring isn't supported yet)".into());
        }
        let transform = scaling(op.target.axes, op.anchor, factors);
        doc.transform_entities(&op.target.entities, &[transform], false).map_err(|e| e.to_string())?;
        let op = self.op.take().expect("checked above");
        self.last = Some(Done { target: op.target, grip: op.grip, anchor: op.anchor, factors, model_version: doc.model_version() });
        self.target = None;
        Ok(())
    }
}

/// Factors of 1, except along the `active` axes.
fn factor_each(active: &[usize], f: impl Fn(usize) -> f64) -> DVec3 {
    let mut factors = DVec3::ONE;
    for &i in active {
        factors[i] = f(i);
    }
    factors
}

/// The fixed point: the opposite grip, or the box's center.
fn anchor(target: &Target, grip: [usize; 3], about_center: bool) -> DVec3 {
    let k = if about_center { [1, 1, 1] } else { grip.map(|c| 2 - c) };
    target.point(k)
}

/// Typed factors (`2`, `1.5,2`) or sizes with units (`36"`, `30",18"`), one per active axis or
/// one for all of them.
fn typed_factors(typed: &str, target: &Target, active: &[usize]) -> Result<DVec3, String> {
    let lower = typed.to_ascii_lowercase();
    let sizes = lower.contains(['"', '\'']) || lower.contains("mm") || lower.contains("cm") || lower.ends_with('m');
    let values: Vec<f64> = if sizes {
        parse_lengths(typed).map_err(|e| e.to_string())?.into_iter().map(|v| v.unwrap_or(f64::NAN)).collect()
    } else {
        typed.split([',', ';']).map(|s| s.trim().parse::<f64>().map_err(|_| format!("\"{}\" isn't a scale factor", s.trim()))).collect::<Result<_, _>>()?
    };
    let typed_count = values.len();
    let per_axis: Vec<f64> = match values.len() {
        1 => vec![values[0]; active.len()],
        n if n == active.len() => values,
        _ => return Err(format!("Enter one value, or {} separated by commas", active.len())),
    };
    let mut factors = DVec3::ONE;
    for (&i, &v) in active.iter().zip(&per_axis) {
        if v.is_nan() {
            continue;
        }
        factors[i] = if sizes { v / (target.hi[i] - target.lo[i]) } else { v };
    }
    // A single size on a corner grip scales evenly: by the first direction's factor.
    if sizes && active.len() == 3 && typed_count == 1 {
        factors = DVec3::splat(factors[active[0]]);
    }
    Ok(factors)
}

/// What the tool would scale: the selection, else what's under the cursor (a whole piece of
/// loose geometry, like a board that isn't grouped).
fn target(doc: &Document, screen: Screen<'_>, cursor: Vec2) -> Option<Target> {
    let entities: Vec<Entity> = if doc.selection().is_empty() {
        match pick_resolved(doc, screen, cursor)? {
            Entity::Instance(id) => vec![Entity::Instance(id)],
            Entity::Guide(_) => return None,
            entity => doc.model().connected(entity).into_iter().collect(),
        }
    } else {
        doc.selection().iter().filter(|e| !matches!(e, Entity::Guide(_))).collect()
    };
    // One group or component: its own axes, so its box turns with it.
    let (origin, axes) = match entities[..] {
        [Entity::Instance(id)] => {
            let t = doc.container().instance(id)?.transform;
            (t.translation, [t.matrix3.x_axis, t.matrix3.y_axis, t.matrix3.z_axis].map(DVec3::normalize))
        }
        _ => (DVec3::ZERO, [DVec3::X, DVec3::Y, DVec3::Z]),
    };
    let view = doc.view();
    let model = view.model();
    let along = |p: DVec3| DVec3::new((p - origin).dot(axes[0]), (p - origin).dot(axes[1]), (p - origin).dot(axes[2]));
    let points: Vec<DVec3> = view.vertices_of(&entities).into_iter().map(|v| along(model.vertex(v))).collect();
    let lo = points.iter().copied().reduce(DVec3::min)?;
    let hi = points.iter().copied().reduce(DVec3::max)?;
    Some(Target { entities, origin, axes, lo, hi })
}

fn grip_at(target: &Target, screen: Screen<'_>, cursor: Vec2) -> Option<[usize; 3]> {
    target
        .grips()
        .into_iter()
        .filter_map(|k| Some((k, screen.project(target.point(k))?.distance(cursor))))
        .filter(|(_, d)| *d <= GRIP_RADIUS)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(k, _)| k)
}

#[cfg(test)]
mod tests {
    use sketchcraft_view::{OrbitCamera, StandardView};

    use super::*;

    const SIZE: Vec2 = Vec2::new(1000.0, 800.0);

    /// A 30" x 2 x 4 board standing up, as a group, seen from the front-left.
    fn setup() -> (Document, OrbitCamera, sketchcraft_doc::InstanceId) {
        let mut doc = Document::default();
        let face = doc.edit(|m| m.add_rectangle(DVec3::ZERO, DVec3::X * 3.5, DVec3::Y * 1.5, DVec3::Z)).unwrap();
        let board = doc.push_pull(face, 30.0, true).unwrap().unwrap();
        let mut camera = OrbitCamera { target: glam::Vec3::new(1.75, 0.75, 15.0), distance: 80.0, ..Default::default() };
        camera.set_view(StandardView::Iso);
        (doc, camera, board)
    }

    fn at(screen: Screen<'_>, p: DVec3) -> Vec2 {
        screen.project(p).unwrap()
    }

    #[test]
    fn the_top_grip_stretches_a_board_to_a_typed_length() {
        let (mut doc, camera, board) = setup();
        let screen = Screen::new(&camera, SIZE);
        let mut tool = ScaleTool::default();
        let middle = at(screen, DVec3::new(1.75, 0.0, 15.0));
        tool.hover(&doc, screen, middle);
        assert_eq!(tool.target.as_ref().unwrap().grips().len(), 26);
        // The grip in the middle of the top face.
        let top = at(screen, DVec3::new(1.75, 0.75, 30.0));
        tool.hover(&doc, screen, top);
        tool.click(&mut doc, screen, top).unwrap();
        assert_eq!(tool.op.as_ref().unwrap().active(), vec![2]);
        tool.enter(&mut doc, "36\"").unwrap();
        let (lo, hi) = doc.bounds().unwrap();
        assert!((hi - lo).abs_diff_eq(DVec3::new(3.5, 1.5, 36.0), 1e-9), "{}", hi - lo);
        assert!(lo.abs_diff_eq(DVec3::ZERO, 1e-9), "the bottom stays put");
        // The cut list reads the stretched size.
        let list = sketchcraft_doc::cutlist::cut_list(doc.scene());
        let part = &list.materials[0].parts[0];
        assert!(((part.length, part.width, part.thickness) == (36.0, 3.5, 1.5)), "{part:?}");
        // Typing again changes it, as one undo step.
        tool.enter(&mut doc, "0.5").unwrap();
        assert!((doc.bounds().unwrap().1.z - 15.0).abs() < 1e-9);
        doc.undo();
        assert!((doc.bounds().unwrap().1.z - 30.0).abs() < 1e-9);
        assert!(doc.container().instance(board).is_some());
    }

    #[test]
    fn a_corner_grip_about_the_center_scales_evenly() {
        let (mut doc, camera, _) = setup();
        let screen = Screen::new(&camera, SIZE);
        let mut tool = ScaleTool::default();
        tool.hover(&doc, screen, at(screen, DVec3::new(1.75, 0.0, 15.0)));
        let corner = at(screen, DVec3::new(0.0, 0.0, 30.0));
        tool.hover(&doc, screen, corner);
        tool.toggle_center();
        tool.click(&mut doc, screen, corner).unwrap();
        tool.enter(&mut doc, "2").unwrap();
        let (lo, hi) = doc.bounds().unwrap();
        assert!((hi - lo).abs_diff_eq(DVec3::new(7.0, 3.0, 60.0), 1e-9));
        assert!(((lo + hi) * 0.5).abs_diff_eq(DVec3::new(1.75, 0.75, 15.0), 1e-9), "about the center");
    }

    #[test]
    fn zero_and_bad_input_are_refused() {
        let (mut doc, camera, _) = setup();
        let screen = Screen::new(&camera, SIZE);
        let mut tool = ScaleTool::default();
        tool.hover(&doc, screen, at(screen, DVec3::new(1.75, 0.0, 15.0)));
        let top = at(screen, DVec3::new(1.75, 0.75, 30.0));
        tool.hover(&doc, screen, top);
        tool.click(&mut doc, screen, top).unwrap();
        assert!(tool.enter(&mut doc, "0").is_err());
        assert!(tool.enter(&mut doc, "big").is_err());
        assert!(tool.enter(&mut doc, "1,2").is_err());
        assert!(tool.is_busy());
    }
}
