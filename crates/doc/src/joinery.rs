//! Joinery between two parts (groups or components side by side in the same container), the
//! way woodworkers model it in SketchUp:
//!
//! - **Cut where they overlap** (dado, groove, rabbet, notch): model the shelf reaching into the
//!   side by the joint's depth; the overlap is cut out of the side, and the shelf keeps its full
//!   length (which is what the cut list needs).
//! - **Half-lap**: two crossing boards overlapping through their thickness; each loses half.
//! - **Mortise and tenon**: a rail whose end touches a stile gets a tenon (so its cut-list
//!   length includes it) and the stile a matching mortise.
//!
//! The parts must be square to each other (each one's axes along the other's). Each joint is
//! one undo step and is recorded on the drawing ([`Joint`]) so it can be listed.

use std::fmt;

use glam::{DAffine3, DVec3};
use serde::{Deserialize, Serialize};

use crate::units::format_length;
use crate::{DefinitionId, Document, EPS, InstanceId, Model};

/// Distances closer than this count as touching (1/1000").
const TOUCH: f64 = 1e-3;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum JointKind {
    Dado,
    Groove,
    Rabbet,
    Notch,
    Mortise,
    HalfLap,
    MortiseAndTenon,
}

impl JointKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Dado => "Dado",
            Self::Groove => "Groove",
            Self::Rabbet => "Rabbet",
            Self::Notch => "Notch",
            Self::Mortise => "Mortise",
            Self::HalfLap => "Half-lap",
            Self::MortiseAndTenon => "Mortise and tenon",
        }
    }
}

/// A joint made between two parts, for listing: `cut` was shaped to fit `into` (for mortise and
/// tenon, `cut` has the tenon and `into` the mortise).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Joint {
    pub kind: JointKind,
    pub cut: DefinitionId,
    pub into: DefinitionId,
    /// Sizes, like `1/4" deep`.
    pub detail: String,
}

#[derive(Clone, Debug, PartialEq)]
pub enum JoineryError {
    NotParts,
    NotSquare,
    NoOverlap,
    NotTouching,
    NoSurface,
    Geometry(String),
}

impl fmt::Display for JoineryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotParts => f.write_str("Pick two groups or components in the same place."),
            Self::NotSquare => f.write_str("The parts need to be square to each other."),
            Self::NoOverlap => f.write_str(
                "The parts don't overlap. Move one into the other by the joint's depth (a shelf 1/4\" into the side for a 1/4\" dado).",
            ),
            Self::NotTouching => f.write_str("The rail's end needs to touch the other part for a mortise and tenon."),
            Self::NoSurface => f.write_str("There's no surface to cut from where they overlap."),
            Self::Geometry(e) => write!(f, "That cut didn't work: {e}"),
        }
    }
}

impl std::error::Error for JoineryError {}

/// Mortise and tenon sizes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tenon {
    pub length: f64,
    pub thickness: f64,
    /// How far the tenon is set in from the rail's edges (across its width).
    pub shoulder: f64,
}

/// What two parts allow, from where they sit.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Fit {
    /// They overlap: one can be cut into the other (or a half-lap if they cross).
    Overlap { half_lap: bool },
    /// `rail`'s end touches the other: a mortise and tenon, with suggested sizes.
    EndToFace { rail: InstanceId, tenon: Tenon },
    Apart,
}

/// An axis-aligned box, `lo` to `hi`.
type Box3 = (DVec3, DVec3);

fn intersect((alo, ahi): Box3, (blo, bhi): Box3) -> Option<Box3> {
    let lo = alo.max(blo);
    let hi = ahi.min(bhi);
    ((hi - lo).min_element() > TOUCH).then_some((lo, hi))
}

/// Round to the nearest 1/16".
fn sixteenth(x: f64) -> f64 {
    (x * 16.0).round() / 16.0
}

impl Document {
    /// How `a` and `b` (instances in the container being edited) can be joined.
    pub fn joint_fit(&self, a: InstanceId, b: InstanceId) -> Result<Fit, JoineryError> {
        let (a_box, b_box) = (self.part_box(a, b)?, self.part_box(b, b)?);
        if let Some(overlap) = intersect(a_box, b_box) {
            // Crossing through each other's full thickness (both parts' thinnest side, on the same
            // axis): a half-lap.
            let thinnest = |(lo, hi): Box3| { let s = hi - lo; (0..3).min_by(|&i, &j| s[i].total_cmp(&s[j])).unwrap_or(0) };
            let k = thinnest(a_box);
            let spans = |(lo, hi): Box3| (lo[k] - overlap.0[k]).abs() < TOUCH && (hi[k] - overlap.1[k]).abs() < TOUCH;
            let half_lap = thinnest(b_box) == k && spans(a_box) && spans(b_box);
            return Ok(Fit::Overlap { half_lap });
        }
        // An end touching a face: whichever part's smallest touching face is the end.
        for (rail, other) in [(a, b), (b, a)] {
            if let Some((_, _, end)) = self.touching_end(rail, other)? {
                let rail_size = end.1 - end.0;
                let mut across: Vec<f64> = (0..3).map(|i| rail_size[i]).filter(|s| *s > TOUCH).collect();
                across.sort_by(f64::total_cmp);
                let [thick, wide] = across[..] else { continue };
                let other_depth = self.part_depth_behind(rail, other)?;
                let tenon = Tenon {
                    length: sixteenth((other_depth * 2.0 / 3.0).min(1.5)).max(0.25),
                    thickness: sixteenth(thick / 3.0).max(0.125),
                    shoulder: if wide > 1.5 { 0.25 } else { 0.0 },
                };
                return Ok(Fit::EndToFace { rail, tenon });
            }
        }
        Ok(Fit::Apart)
    }

    /// Cut the overlap of `cut` out of `into` (a dado, groove, rabbet or notch), one undo step.
    pub fn cut_overlap(&mut self, cut: InstanceId, into: InstanceId) -> Result<JointKind, JoineryError> {
        let overlap = intersect(self.part_box(cut, into)?, self.part_box(into, into)?).ok_or(JoineryError::NoOverlap)?;
        let path = self.path().to_vec();
        let mut done = None;
        let result = self.try_edit_scene(|scene| {
            scene.make_unique_if_group(&path, into);
            let def = scene.container(&path).and_then(|c| c.instance(into)).ok_or(JoineryError::NotParts)?.definition;
            let cut_def = scene.container(&path).and_then(|c| c.instance(cut)).ok_or(JoineryError::NotParts)?.definition;
            let grain = scene.definition(def).map(|d| (d.part.grain, crate::part::extents(d, glam::DMat3::IDENTITY)));
            let model = &mut scene.definition_mut(def).ok_or(JoineryError::NotParts)?.container.model;
            let (kind, depth) = cut_box(model, overlap, grain)?;
            scene.joints.push(Joint { kind, cut: cut_def, into: def, detail: format!("{} deep", format_length(depth)) });
            done = Some(kind);
            Ok(())
        });
        result.map(|()| done.expect("set on success"))
    }

    /// A half-lap: each of two crossing parts loses half the overlap, `top` from its upper half
    /// (along the axis they cross through), the other from its lower half. One undo step.
    pub fn half_lap(&mut self, top: InstanceId, bottom: InstanceId) -> Result<(), JoineryError> {
        let (top_box, bottom_box) = (self.part_box(top, bottom)?, self.part_box(bottom, bottom)?);
        let overlap = intersect(top_box, bottom_box).ok_or(JoineryError::NoOverlap)?;
        let size = overlap.1 - overlap.0;
        let k = (0..3)
            .filter(|&k| {
                let spans = |(lo, hi): Box3| (lo[k] - overlap.0[k]).abs() < TOUCH && (hi[k] - overlap.1[k]).abs() < TOUCH;
                spans(top_box) && spans(bottom_box)
            })
            .min_by(|&a, &b| size[a].total_cmp(&size[b]))
            .ok_or(JoineryError::NoOverlap)?;
        let middle = (overlap.0[k] + overlap.1[k]) / 2.0;
        let (mut upper, mut lower) = (overlap, overlap);
        upper.0[k] = middle;
        lower.1[k] = middle;
        // In `bottom`'s coordinates; `top` gets its half mapped into its own.
        let to_top = self.relative(bottom, top)?;
        let upper_in_top = transform_box(&to_top, upper);
        let path = self.path().to_vec();
        self.try_edit_scene(|scene| {
            scene.make_unique_if_group(&path, top);
            scene.make_unique_if_group(&path, bottom);
            let container = scene.container(&path).ok_or(JoineryError::NotParts)?;
            let top_def = container.instance(top).ok_or(JoineryError::NotParts)?.definition;
            let bottom_def = container.instance(bottom).ok_or(JoineryError::NotParts)?.definition;
            if top_def == bottom_def {
                return Err(JoineryError::NotParts);
            }
            cut_box(&mut scene.definition_mut(top_def).ok_or(JoineryError::NotParts)?.container.model, upper_in_top, None)?;
            cut_box(&mut scene.definition_mut(bottom_def).ok_or(JoineryError::NotParts)?.container.model, lower, None)?;
            let detail = format!("{} deep", format_length(size[k] / 2.0));
            scene.joints.push(Joint { kind: JointKind::HalfLap, cut: top_def, into: bottom_def, detail });
            Ok(())
        })
    }

    /// A mortise and tenon: `rail`'s end (touching `stile`) grows a tenon, and `stile` gets a
    /// mortise to take it. One undo step.
    pub fn mortise_and_tenon(&mut self, rail: InstanceId, stile: InstanceId, tenon: Tenon) -> Result<(), JoineryError> {
        if tenon.length <= EPS || tenon.thickness <= EPS || tenon.shoulder < 0.0 {
            return Err(JoineryError::Geometry("the tenon needs a length and thickness".into()));
        }
        let (_, _, end) = self.touching_end(rail, stile)?.ok_or(JoineryError::NotTouching)?;
        // The end face in the rail's own coordinates, and the tenon on it.
        let rail_box = self.part_box(rail, rail)?;
        let to_rail = self.relative(stile, rail)?;
        let end_in_rail = transform_box(&to_rail, end);
        let size = end_in_rail.1 - end_in_rail.0;
        let k_rail = (0..3).min_by(|&a, &b| size[a].total_cmp(&size[b])).expect("three axes");
        let across: Vec<usize> = (0..3).filter(|&i| i != k_rail).collect();
        let (thin, wide) = if size[across[0]] <= size[across[1]] { (across[0], across[1]) } else { (across[1], across[0]) };
        if tenon.thickness >= size[thin] - EPS || 2.0 * tenon.shoulder >= size[wide] - EPS {
            return Err(JoineryError::Geometry("the tenon must be thinner and narrower than the rail".into()));
        }
        let outward = if (end_in_rail.0[k_rail] - rail_box.1[k_rail]).abs() < TOUCH { 1.0 } else { -1.0 };
        let face_at = if outward > 0.0 { rail_box.1[k_rail] } else { rail_box.0[k_rail] };
        let mid = (end_in_rail.0[thin] + end_in_rail.1[thin]) / 2.0;
        let mut t_lo = DVec3::ZERO;
        let mut t_hi = DVec3::ZERO;
        t_lo[thin] = mid - tenon.thickness / 2.0;
        t_hi[thin] = mid + tenon.thickness / 2.0;
        t_lo[wide] = end_in_rail.0[wide] + tenon.shoulder;
        t_hi[wide] = end_in_rail.1[wide] - tenon.shoulder;
        t_lo[k_rail] = face_at.min(face_at + outward * tenon.length);
        t_hi[k_rail] = face_at.max(face_at + outward * tenon.length);
        let tenon_in_rail = (t_lo, t_hi);
        let tenon_in_stile = transform_box(&self.relative(rail, stile)?, tenon_in_rail);
        let path = self.path().to_vec();
        self.try_edit_scene(|scene| {
            scene.make_unique_if_group(&path, rail);
            scene.make_unique_if_group(&path, stile);
            let container = scene.container(&path).ok_or(JoineryError::NotParts)?;
            let rail_def = container.instance(rail).ok_or(JoineryError::NotParts)?.definition;
            let stile_def = container.instance(stile).ok_or(JoineryError::NotParts)?.definition;
            add_box(&mut scene.definition_mut(rail_def).ok_or(JoineryError::NotParts)?.container.model, tenon_in_rail, k_rail, outward)?;
            cut_box(&mut scene.definition_mut(stile_def).ok_or(JoineryError::NotParts)?.container.model, tenon_in_stile, None)?;
            let detail = format!(
                "{} long, {} x {} tenon",
                format_length(tenon.length),
                format_length(tenon.thickness),
                format_length(size[wide] - 2.0 * tenon.shoulder)
            );
            scene.joints.push(Joint { kind: JointKind::MortiseAndTenon, cut: rail_def, into: stile_def, detail });
            Ok(())
        })
    }

    /// The joints a part has, as `(joint, the other part's name)`.
    pub fn joints_of(&self, instance: InstanceId) -> Vec<(Joint, String)> {
        let Some(def) = self.container().instance(instance).map(|i| i.definition) else { return Vec::new() };
        let scene = self.scene();
        let name = |d: DefinitionId| scene.definition(d).map_or_else(|| "a deleted part".to_owned(), |d| d.name.clone());
        scene
            .joints
            .iter()
            .filter(|j| j.cut == def || j.into == def)
            .map(|j| (j.clone(), name(if j.cut == def { j.into } else { j.cut })))
            .collect()
    }

    /// Edit the scene as one undo step, keeping nothing if `change` fails.
    fn try_edit_scene(&mut self, change: impl FnOnce(&mut crate::Scene) -> Result<(), JoineryError>) -> Result<(), JoineryError> {
        let mut trial = self.scene().clone();
        change(&mut trial)?;
        self.edit_scene(|scene, _| *scene = trial);
        Ok(())
    }

    /// Maps `from`'s own coordinates into `to`'s; both instances of the container being edited.
    /// Errors unless their axes line up (square to each other).
    fn relative(&self, from: InstanceId, to: InstanceId) -> Result<DAffine3, JoineryError> {
        let container = self.container();
        let (f, t) = (container.instance(from).ok_or(JoineryError::NotParts)?, container.instance(to).ok_or(JoineryError::NotParts)?);
        let m = t.transform.inverse() * f.transform;
        for col in [m.matrix3.x_axis, m.matrix3.y_axis, m.matrix3.z_axis] {
            let len = col.length();
            if len <= EPS || (col.abs().max_element() - len).abs() > 1e-6 * len.max(1.0) {
                return Err(JoineryError::NotSquare);
            }
        }
        Ok(m)
    }

    /// `part`'s bounding box in `frame`'s coordinates.
    fn part_box(&self, part: InstanceId, frame: InstanceId) -> Result<Box3, JoineryError> {
        let def = self.container().instance(part).ok_or(JoineryError::NotParts)?.definition;
        let local = self.scene().definition_bounds(def).ok_or(JoineryError::NotParts)?;
        Ok(transform_box(&self.relative(part, frame)?, local))
    }

    /// Where `rail`'s end touches `other`: the axis and side (in `other`'s coordinates), and
    /// the touching rectangle (a flat box). The end is the smallest face of the rail's box.
    fn touching_end(&self, rail: InstanceId, other: InstanceId) -> Result<Option<(usize, f64, Box3)>, JoineryError> {
        let (r, o) = (self.part_box(rail, other)?, self.part_box(other, other)?);
        let size = r.1 - r.0;
        let mut ends: Vec<usize> = (0..3).collect();
        ends.sort_by(|&a, &b| (size[(a + 1) % 3] * size[(a + 2) % 3]).total_cmp(&(size[(b + 1) % 3] * size[(b + 2) % 3])));
        for k in ends.into_iter().take(1) {
            for (rail_at, other_at, side) in [(r.1[k], o.0[k], 1.0), (r.0[k], o.1[k], -1.0)] {
                if (rail_at - other_at).abs() > TOUCH {
                    continue;
                }
                let mut lo = r.0.max(o.0);
                let mut hi = r.1.min(o.1);
                lo[k] = rail_at;
                hi[k] = rail_at;
                let inside = (0..3).filter(|&i| i != k).all(|i| hi[i] - lo[i] > TOUCH && (hi[i] - lo[i] - size[i]).abs() < TOUCH);
                if inside {
                    return Ok(Some((k, side, (lo, hi))));
                }
            }
        }
        Ok(None)
    }

    /// How deep `other` is behind where `rail`'s end touches it.
    fn part_depth_behind(&self, rail: InstanceId, other: InstanceId) -> Result<f64, JoineryError> {
        let (k, _, _) = self.touching_end(rail, other)?.ok_or(JoineryError::NotTouching)?;
        let o = self.part_box(other, other)?;
        Ok(o.1[k] - o.0[k])
    }
}

fn transform_box(t: &DAffine3, (lo, hi): Box3) -> Box3 {
    let corners = (0..8).map(|i| {
        t.transform_point3(DVec3::new(
            if i & 1 == 0 { lo.x } else { hi.x },
            if i & 2 == 0 { lo.y } else { hi.y },
            if i & 4 == 0 { lo.z } else { hi.z },
        ))
    });
    let (mut a, mut b) = (DVec3::splat(f64::INFINITY), DVec3::splat(f64::NEG_INFINITY));
    for c in corners {
        a = a.min(c);
        b = b.max(c);
    }
    // Snap away floating-point noise from the transforms.
    let snap = |v: DVec3| (v * 1e9).round() / 1e9;
    (snap(a), snap(b))
}

/// The rectangle of `bx` on the plane `axis = at`, as a polygon.
fn face_rect(bx: Box3, axis: usize, at: f64) -> [DVec3; 4] {
    let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
    let corner = |a: f64, b: f64| {
        let mut p = DVec3::ZERO;
        p[axis] = at;
        p[u] = a;
        p[v] = b;
        p
    };
    let (lo, hi) = bx;
    [corner(lo[u], lo[v]), corner(hi[u], lo[v]), corner(hi[u], hi[v]), corner(lo[u], hi[v])]
}

/// The model's face on the plane `axis = at` (facing `normal`) under `point`.
fn surface_at(model: &Model, axis: usize, at: f64, normal: DVec3, point: DVec3) -> bool {
    model.faces().any(|(_, f)| {
        f.normal.dot(normal) > 1.0 - 1e-9 && (model.vertex(f.outer[0])[axis] - at).abs() < TOUCH && model.face_contains(f, point)
    })
}

/// Cut the box `bx` (inside the model's bounds) out of the model: draw its face on the
/// surface it opens onto and push it in. Returns what kind of cut that is and its depth.
/// `grain` (the part's grain axis and extents) tells a groove (along the grain) from a dado.
fn cut_box(
    model: &mut Model,
    bx: Box3,
    grain: Option<(Option<crate::part::PartAxis>, Option<DVec3>)>,
) -> Result<(JointKind, f64), JoineryError> {
    let (plo, phi) = model.bounds().ok_or(JoineryError::NoSurface)?;
    let size = bx.1 - bx.0;
    // Faces of the box lying on the part's outside, largest first: the one to cut from.
    let mut entries: Vec<(usize, f64, f64)> = Vec::new(); // (axis, sign, area)
    for k in 0..3 {
        let area = size[(k + 1) % 3] * size[(k + 2) % 3];
        if (bx.1[k] - phi[k]).abs() < TOUCH {
            entries.push((k, 1.0, area));
        }
        if (bx.0[k] - plo[k]).abs() < TOUCH {
            entries.push((k, -1.0, area));
        }
    }
    entries.sort_by(|a, b| b.2.total_cmp(&a.2));
    let &(k, sign, _) = entries
        .iter()
        .find(|&&(k, sign, _)| {
            let at = if sign > 0.0 { phi[k] } else { plo[k] };
            let normal = DVec3::AXES[k] * sign;
            let mut center = (bx.0 + bx.1) / 2.0;
            center[k] = at;
            surface_at(model, k, at, normal, center)
        })
        .ok_or(JoineryError::NoSurface)?;
    let at = if sign > 0.0 { phi[k] } else { plo[k] };
    let normal = DVec3::AXES[k] * sign;
    let depth = size[k];
    let mut trial = model.clone();
    let face = trial.draw_polygon(&face_rect(bx, k, at), normal).ok_or(JoineryError::NoSurface)?;
    trial.push_pull(face, -depth).map_err(|e| JoineryError::Geometry(e.to_string()))?;
    *model = trial;

    // What kind of cut: which of the part's other faces the box reaches.
    let (u, v) = ((k + 1) % 3, (k + 2) % 3);
    let reaches = |i: usize| ((bx.0[i] - plo[i]).abs() < TOUCH) as u8 + ((bx.1[i] - phi[i]).abs() < TOUCH) as u8;
    let through = (reaches(k) == 2) as u8;
    let kind = match (reaches(u), reaches(v)) {
        // Across the part from side to side.
        (2, 0) | (0, 2) => {
            let along = if reaches(u) == 2 { u } else { v };
            let grain_axis = grain.and_then(|(g, ext)| g.map(|a| a.index()).or_else(|| ext.map(|e| (0..3).max_by(|&a, &b| e[a].total_cmp(&e[b])).unwrap_or(0))));
            if grain_axis == Some(along) { JointKind::Groove } else { JointKind::Dado }
        }
        // Along one edge: a rabbet.
        (2, 1) | (1, 2) => JointKind::Rabbet,
        (0, 0) if through == 0 => JointKind::Mortise,
        _ => JointKind::Notch,
    };
    Ok((kind, depth))
}

/// Add the box `bx` to the model, growing out of its face at the box's `axis` side facing
/// `outward` (a tenon from a rail's end): draw its footprint there and pull it out.
fn add_box(model: &mut Model, bx: Box3, axis: usize, outward: f64) -> Result<(), JoineryError> {
    let at = if outward > 0.0 { bx.0[axis] } else { bx.1[axis] };
    let normal = DVec3::AXES[axis] * outward;
    let mut center = (bx.0 + bx.1) / 2.0;
    center[axis] = at;
    if !surface_at(model, axis, at, normal, center) {
        return Err(JoineryError::NoSurface);
    }
    let mut trial = model.clone();
    let face = trial.draw_polygon(&face_rect(bx, axis, at), normal).ok_or(JoineryError::NoSurface)?;
    trial.push_pull(face, bx.1[axis] - bx.0[axis]).map_err(|e| JoineryError::Geometry(e.to_string()))?;
    *model = trial;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::Board;
    use crate::cutlist::cut_list;
    use crate::library::LIBRARY;

    fn board(doc: &mut Document, origin: DVec3, length_dir: DVec3, width_dir: DVec3, [l, w, t]: [f64; 3]) -> InstanceId {
        let oak = doc.add_preset(LIBRARY.iter().find(|p| p.name == "Red Oak").unwrap());
        doc.add_board(&Board { origin, length_dir, width_dir, length: l, width: w, thickness: t }, Some(oak)).unwrap()
    }

    fn closed(doc: &Document, part: InstanceId) -> bool {
        let def = doc.container().instance(part).unwrap().definition;
        let model = &doc.scene().definition(def).unwrap().container.model;
        model.edges().all(|(id, _)| model.edge_faces(id).len() == 2)
    }

    /// A side standing up (3/4" thick along X, 11 1/4" deep along Y, 30" tall) and a shelf
    /// reaching 1/4" into it.
    fn side_and_shelf(doc: &mut Document) -> (InstanceId, InstanceId) {
        let side = board(doc, DVec3::ZERO, DVec3::Z, DVec3::Y, [30.0, 11.25, 0.75]);
        // The side's thickness runs along -X (Z x Y): it spans x in [-0.75, 0].
        let shelf = board(doc, DVec3::new(-0.25, 0.0, 12.0), DVec3::X, DVec3::Y, [20.0, 11.25, 0.75]);
        (side, shelf)
    }

    #[test]
    fn a_shelf_reaching_into_a_side_cuts_a_dado() {
        let mut doc = Document::default();
        let (side, shelf) = side_and_shelf(&mut doc);
        assert_eq!(doc.joint_fit(shelf, side), Ok(Fit::Overlap { half_lap: false }));
        let kind = doc.cut_overlap(shelf, side).unwrap();
        assert_eq!(kind, JointKind::Dado);
        assert!(closed(&doc, side), "still a closed solid");
        // The side's faces: 6 + the dado's 3 walls, its top split in two.
        let def = doc.container().instance(side).unwrap().definition;
        let model = &doc.scene().definition(def).unwrap().container.model;
        assert_eq!(model.faces().count(), 10, "inner face in two, the dado's bottom and two walls");
        let (lo, hi) = model.bounds().unwrap();
        assert!((hi - lo).abs_diff_eq(DVec3::new(30.0, 11.25, 0.75), 1e-9), "same size");
        // The cut list keeps the shelf's full length, and the side's size.
        let list = cut_list(doc.scene());
        let lengths: Vec<f64> = list.materials[0].parts.iter().map(|p| p.length).collect();
        assert!(lengths.contains(&20.0) && lengths.contains(&30.0), "{lengths:?}");
        let joints = doc.joints_of(side);
        assert_eq!(joints.len(), 1);
        assert_eq!(joints[0].0.detail, "1/4\" deep");
        doc.undo();
        assert!(doc.joints_of(side).is_empty(), "one undo step");
    }

    #[test]
    fn a_second_shelf_cuts_a_second_dado() {
        let mut doc = Document::default();
        let (side, shelf) = side_and_shelf(&mut doc);
        let lower = board(&mut doc, DVec3::new(-0.25, 0.0, 4.0), DVec3::X, DVec3::Y, [20.0, 11.25, 0.75]);
        doc.cut_overlap(shelf, side).unwrap();
        assert_eq!(doc.cut_overlap(lower, side), Ok(JointKind::Dado));
        assert!(closed(&doc, side));
        assert_eq!(doc.joints_of(side).len(), 2);
    }

    #[test]
    fn a_panel_in_a_rail_cuts_a_groove_along_its_grain() {
        let mut doc = Document::default();
        // A rail lying flat (24" along X, 2 1/2" wide along Y, 3/4" thick), and a 1/4" panel
        // standing in its edge, 3/8" deep, centered on its thickness.
        let rail = board(&mut doc, DVec3::ZERO, DVec3::X, DVec3::Y, [24.0, 2.5, 0.75]);
        let panel = board(&mut doc, DVec3::new(0.0, 2.125, 0.25), DVec3::X, DVec3::Z, [24.0, 0.25, 10.0]);
        let _ = panel;
        assert_eq!(doc.cut_overlap(panel, rail), Ok(JointKind::Groove));
        assert!(closed(&doc, rail));
    }

    #[test]
    fn a_top_sitting_in_the_side_cuts_a_rabbet() {
        let mut doc = Document::default();
        let side = board(&mut doc, DVec3::ZERO, DVec3::Z, DVec3::Y, [30.0, 11.25, 0.75]);
        // The top's ends sit 3/8" into the side, flush with its top.
        let top = board(&mut doc, DVec3::new(-0.375, 0.0, 29.25), DVec3::X, DVec3::Y, [20.0, 11.25, 0.75]);
        assert_eq!(doc.cut_overlap(top, side), Ok(JointKind::Rabbet));
        assert!(closed(&doc, side));
    }

    #[test]
    fn apart_parts_say_so() {
        let mut doc = Document::default();
        let a = board(&mut doc, DVec3::ZERO, DVec3::X, DVec3::Y, [10.0, 3.0, 0.75]);
        let b = board(&mut doc, DVec3::new(0.0, 10.0, 0.0), DVec3::X, DVec3::Y, [10.0, 3.0, 0.75]);
        assert_eq!(doc.joint_fit(a, b), Ok(Fit::Apart));
        assert_eq!(doc.cut_overlap(a, b), Err(JoineryError::NoOverlap));
    }

    #[test]
    fn crossing_boards_half_lap() {
        let mut doc = Document::default();
        let a = board(&mut doc, DVec3::ZERO, DVec3::X, DVec3::Y, [20.0, 3.0, 0.75]);
        let b = board(&mut doc, DVec3::new(8.5, -8.0, 0.0), DVec3::Y, -DVec3::X, [20.0, 3.0, 0.75]);
        assert_eq!(doc.joint_fit(a, b), Ok(Fit::Overlap { half_lap: true }));
        doc.half_lap(a, b).unwrap();
        assert!(closed(&doc, a) && closed(&doc, b));
        assert_eq!(doc.joints_of(a)[0].0.kind, JointKind::HalfLap);
    }

    #[test]
    fn a_rail_touching_a_stile_gets_a_tenon_and_the_stile_a_mortise() {
        let mut doc = Document::default();
        // A stile standing up (1 1/2" x 3" x 30") and a rail (3/4"... 1 1/2" thick, 2 1/2" wide)
        // butting into its side.
        let stile = board(&mut doc, DVec3::ZERO, DVec3::Z, DVec3::X, [30.0, 3.0, 1.5]);
        // Stile spans x 0..3, y 0..1.5 (Z x X = Y), z 0..30. Rail runs +X from x = 3, z 10..12 1/2, y 0..1 1/2 (X x -Z = Y).
        let rail = board(&mut doc, DVec3::new(3.0, 0.0, 12.5), DVec3::X, -DVec3::Z, [20.0, 2.5, 1.5]);
        let Fit::EndToFace { rail: found, tenon } = doc.joint_fit(rail, stile).unwrap() else { panic!("end to face") };
        assert_eq!(found, rail);
        assert_eq!(tenon.thickness, 0.5, "a third of the rail");
        doc.mortise_and_tenon(rail, stile, Tenon { length: 1.0, ..tenon }).unwrap();
        assert!(closed(&doc, rail), "the rail with its tenon is closed");
        assert!(closed(&doc, stile), "the stile with its mortise is closed");
        // The cut list counts the tenon in the rail's length.
        assert_eq!(doc.part_size(rail).unwrap()[0], 21.0);
        let joints = doc.joints_of(stile);
        assert_eq!(joints[0].0.kind, JointKind::MortiseAndTenon);
    }
}
