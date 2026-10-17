//! Joinery between two parts (groups or components side by side in the same container, square
//! to each other). What can be made depends on how they sit ([`Fit`]):
//!
//! - **Butted** (one part's end against the other): let it in (a dado, or a rabbet at an edge),
//!   mortise and tenon, or dowels; at a corner, a box joint.
//! - **Overlapping** (modeled the way many SketchUp woodworkers do, a shelf reaching 1/4" into
//!   the side): cut the overlap out of one; boards crossing through each other's thickness can
//!   be half-lapped; overlapping at a corner, box-jointed.
//!
//! Parts that grow (a let-in shelf, a tenon) grow in their cut-list size too. Every joint is a
//! rectangle or circle drawn on a face and pushed or pulled, so parts stay closed solids; each
//! is one undo step and is recorded in [`crate::Scene::joints`] for listing.

use std::fmt;

use glam::{DAffine3, DVec3};
use serde::{Deserialize, Serialize};

use crate::curves::circle_points;
use crate::units::format_length;
use crate::{DefinitionId, Document, EPS, InstanceId, Model, Scene};

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
    Dowels,
    BoxJoint,
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
            Self::Dowels => "Dowels",
            Self::BoxJoint => "Box joint",
        }
    }
}

/// A joint made between two parts, for listing: `cut` was shaped to fit `into` (the shelf and
/// the side; the tenoned rail and the mortised stile).
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
    NotCorner,
    NoSurface,
    Size(&'static str),
    Geometry(String),
}

impl fmt::Display for JoineryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotParts => f.write_str("Pick two different groups or components in the same place."),
            Self::NotSquare => f.write_str("The parts need to be square to each other."),
            Self::NoOverlap => f.write_str("The parts don't overlap."),
            Self::NotTouching => f.write_str("Butt one part's end against the other (no gap, no overlap)."),
            Self::NotCorner => f.write_str("Put the two parts' ends together at a corner, butted or overlapping."),
            Self::NoSurface => f.write_str("There's no surface to cut from there."),
            Self::Size(why) => f.write_str(why),
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

/// Dowel joint sizes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Dowels {
    pub count: u32,
    pub diameter: f64,
    /// Hole depth in each part.
    pub depth: f64,
}

/// How two parts sit, and suggested sizes for the joints that fit.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fit {
    /// They overlap: one's overlap can be cut out of the other.
    pub overlap: bool,
    /// They cross through each other's thickness: a half-lap.
    pub half_lap: bool,
    /// One's end touches the other: `(that part, the other)`.
    pub butt: Option<(InstanceId, InstanceId)>,
    /// Their ends meet at a corner (butted or overlapping): a box joint.
    pub corner: bool,
    pub let_in_depth: f64,
    pub tenon: Tenon,
    pub dowels: Dowels,
    pub finger: f64,
}

/// An axis-aligned box, `lo` to `hi`.
type Box3 = (DVec3, DVec3);

fn intersect((alo, ahi): Box3, (blo, bhi): Box3) -> Option<Box3> {
    let lo = alo.max(blo);
    let hi = ahi.min(bhi);
    ((hi - lo).min_element() > TOUCH).then_some((lo, hi))
}

fn longest((lo, hi): Box3) -> usize {
    let s = hi - lo;
    (0..3).max_by(|&a, &b| s[a].total_cmp(&s[b])).unwrap_or(0)
}

fn thinnest((lo, hi): Box3) -> usize {
    let s = hi - lo;
    (0..3).min_by(|&a, &b| s[a].total_cmp(&s[b])).unwrap_or(0)
}

/// Round to the nearest 1/16".
fn sixteenth(x: f64) -> f64 {
    (x * 16.0).round() / 16.0
}

/// Where a part's end touches another, in the other's coordinates.
#[derive(Clone, Copy)]
struct Butt {
    /// The axis they meet along.
    k: usize,
    /// +1 when the other part lies toward +k from the contact.
    side: f64,
    /// The touching rectangle (flat along `k`).
    contact: Box3,
}

/// Two parts' places, read from a scene.
struct Parts<'a> {
    scene: &'a Scene,
    path: &'a [InstanceId],
}

impl Parts<'_> {
    fn definition(&self, id: InstanceId) -> Result<DefinitionId, JoineryError> {
        Ok(self.scene.container(self.path).and_then(|c| c.instance(id)).ok_or(JoineryError::NotParts)?.definition)
    }

    /// Maps `from`'s own coordinates into `to`'s. Errors unless their axes line up.
    fn relative(&self, from: InstanceId, to: InstanceId) -> Result<DAffine3, JoineryError> {
        let container = self.scene.container(self.path).ok_or(JoineryError::NotParts)?;
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
        let local = self.scene.definition_bounds(self.definition(part)?).ok_or(JoineryError::NotParts)?;
        Ok(transform_box(&self.relative(part, frame)?, local))
    }

    /// Where `rail`'s end or edge touches `other`, wholly on it: a rail's end against a stile, or
    /// a panel's edge against a post. Its smallest touching face counts (not its broad face).
    fn butt(&self, rail: InstanceId, other: InstanceId) -> Result<Option<Butt>, JoineryError> {
        let (r, o) = (self.part_box(rail, other)?, self.part_box(other, other)?);
        let size = r.1 - r.0;
        let area = |k: usize| size[(k + 1) % 3] * size[(k + 2) % 3];
        let mut axes: Vec<usize> = (0..3).collect();
        axes.sort_by(|&a, &b| area(a).total_cmp(&area(b)));
        for &k in &axes[..2] {
            if let Some(butt) = self.butt_along(r, o, k) {
                return Ok(Some(butt));
            }
        }
        Ok(None)
    }

    fn butt_along(&self, r: Box3, o: Box3, k: usize) -> Option<Butt> {
        let size = r.1 - r.0;
        for (rail_at, other_at, side) in [(r.1[k], o.0[k], 1.0), (r.0[k], o.1[k], -1.0)] {
            if (rail_at - other_at).abs() > TOUCH {
                continue;
            }
            let (mut lo, mut hi) = (r.0.max(o.0), r.1.min(o.1));
            lo[k] = rail_at;
            hi[k] = rail_at;
            let whole = (0..3).filter(|&i| i != k).all(|i| (hi[i] - lo[i] - size[i]).abs() < TOUCH);
            if whole {
                return Some(Butt { k, side, contact: (lo, hi) });
            }
        }
        None
    }

    /// `rail` (butting `other`) is flush with `other`'s end: they meet at a corner.
    fn flush_at_end(&self, rail: InstanceId, other: InstanceId, butt: &Butt) -> Result<bool, JoineryError> {
        let (r, o) = (self.part_box(rail, other)?, self.part_box(other, other)?);
        let l = longest(o);
        Ok(l != butt.k && ((r.0[l] - o.0[l]).abs() < TOUCH || (r.1[l] - o.1[l]).abs() < TOUCH))
    }
}

impl Document {
    fn parts(&self) -> Parts<'_> {
        Parts { scene: self.scene(), path: self.path() }
    }

    /// How `a` and `b` (instances in the container being edited) sit, and suggested sizes.
    pub fn joint_fit(&self, a: InstanceId, b: InstanceId) -> Result<Fit, JoineryError> {
        let p = self.parts();
        if a == b || p.definition(a)? == p.definition(b)? && p.relative(a, b)?.translation.length() < TOUCH {
            return Err(JoineryError::NotParts);
        }
        let (ab, bb) = (p.part_box(a, b)?, p.part_box(b, b)?);
        let mut fit = Fit {
            overlap: false,
            half_lap: false,
            butt: None,
            corner: false,
            let_in_depth: 0.25,
            tenon: Tenon { length: 1.0, thickness: 0.25, shoulder: 0.25 },
            dowels: Dowels { count: 2, diameter: 0.375, depth: 1.0 },
            finger: 0.5,
        };
        // The thinner part's thickness sets the finger size.
        let mut thick = (ab.1 - ab.0)[thinnest(ab)].min((bb.1 - bb.0)[thinnest(bb)]);
        if let Some(o) = intersect(ab, bb) {
            fit.overlap = true;
            let k = thinnest(ab);
            let spans = |(lo, hi): Box3| (lo[k] - o.0[k]).abs() < TOUCH && (hi[k] - o.1[k]).abs() < TOUCH;
            fit.half_lap = thinnest(bb) == k && spans(ab) && spans(bb);
            let at_end = |part: Box3| {
                let l = longest(part);
                (o.0[l] - part.0[l]).abs() < TOUCH || (o.1[l] - part.1[l]).abs() < TOUCH
            };
            fit.corner = at_end(ab) && at_end(bb) && !fit.half_lap;
        }
        for (rail, other) in [(a, b), (b, a)] {
            let Some(butt) = p.butt(rail, other)? else { continue };
            fit.butt = Some((rail, other));
            fit.corner |= p.flush_at_end(rail, other, &butt)?;
            let o = p.part_box(other, other)?;
            let (thin, wide) = across(butt);
            let size = butt.contact.1 - butt.contact.0;
            let (rail_thick, rail_wide) = (size[thin], size[wide]);
            let depth = o.1[butt.k] - o.0[butt.k];
            thick = rail_thick.min(depth);
            fit.let_in_depth = sixteenth((depth / 3.0).min(0.5)).max(0.125);
            fit.tenon = Tenon {
                length: sixteenth((depth * 2.0 / 3.0).min(1.5)).max(0.25),
                thickness: sixteenth(rail_thick / 3.0).max(0.125),
                shoulder: if rail_wide > 1.5 { 0.25 } else { 0.0 },
            };
            let diameter = if rail_thick >= 1.25 { 0.5 } else if rail_thick >= 0.6 { 0.375 } else { 0.25 };
            fit.dowels = Dowels {
                count: ((rail_wide / 3.0).round() as u32).max(1),
                diameter,
                depth: sixteenth((depth * 2.0 / 3.0).min(1.0)).max(0.25),
            };
            break;
        }
        fit.finger = sixteenth(thick.min(0.75)).max(0.125);
        Ok(fit)
    }

    /// Cut the overlap of `cut` out of `into` (a dado, groove, rabbet, notch or mortise).
    pub fn cut_overlap(&mut self, cut: InstanceId, into: InstanceId) -> Result<JointKind, JoineryError> {
        self.joint_edit(|scene, path| {
            let p = Parts { scene, path };
            let region = intersect(p.part_box(cut, into)?, p.part_box(into, into)?).ok_or(JoineryError::NoOverlap)?;
            let (kind, depth) = cut_box_in(scene, path, into, region)?;
            record(scene, path, kind, cut, into, format!("{} deep", format_length(depth)))?;
            Ok(kind)
        })
    }

    /// Let `part`'s end (butted against `into`) into it by `depth`: the part grows by `depth`
    /// and `into` gets a dado, or a rabbet at its edge, to take it.
    pub fn let_in(&mut self, part: InstanceId, into: InstanceId, depth: f64) -> Result<JointKind, JoineryError> {
        self.joint_edit(|scene, path| {
            let p = Parts { scene, path };
            let butt = p.butt(part, into)?.ok_or(JoineryError::NotTouching)?;
            let o = p.part_box(into, into)?;
            if depth <= EPS || depth > o.1[butt.k] - o.0[butt.k] + TOUCH {
                return Err(JoineryError::Size("The depth must be more than zero and no more than the other part's thickness."));
            }
            let region = extrude(butt, depth);
            grow_into(scene, path, part, into, region)?;
            let (kind, depth) = cut_box_in(scene, path, into, region)?;
            record(scene, path, kind, part, into, format!("{} deep", format_length(depth)))?;
            Ok(kind)
        })
    }

    /// A half-lap: each of two crossing parts loses half the overlap, `top` its upper half.
    pub fn half_lap(&mut self, top: InstanceId, bottom: InstanceId) -> Result<(), JoineryError> {
        self.joint_edit(|scene, path| {
            let p = Parts { scene, path };
            let (top_box, bottom_box) = (p.part_box(top, bottom)?, p.part_box(bottom, bottom)?);
            let overlap = intersect(top_box, bottom_box).ok_or(JoineryError::NoOverlap)?;
            if p.definition(top)? == p.definition(bottom)? {
                return Err(JoineryError::Size("Half-lapping two copies of one component would cut both halves from each."));
            }
            let k = thinnest(bottom_box);
            let middle = (overlap.0[k] + overlap.1[k]) / 2.0;
            let (mut upper, mut lower) = (overlap, overlap);
            upper.0[k] = middle;
            lower.1[k] = middle;
            let upper_in_top = transform_box(&p.relative(bottom, top)?, upper);
            cut_box_in(scene, path, top, upper_in_top)?;
            cut_box_in(scene, path, bottom, lower)?;
            let detail = format!("{} deep", format_length((overlap.1 - overlap.0)[k] / 2.0));
            record(scene, path, JointKind::HalfLap, top, bottom, detail)
        })
    }

    /// A mortise and tenon: `rail`'s end (butted against `stile`) grows a tenon, and `stile`
    /// gets a mortise to take it.
    pub fn mortise_and_tenon(&mut self, rail: InstanceId, stile: InstanceId, tenon: Tenon) -> Result<(), JoineryError> {
        if tenon.length <= EPS || tenon.thickness <= EPS || tenon.shoulder < 0.0 {
            return Err(JoineryError::Size("The tenon needs a length and a thickness."));
        }
        self.joint_edit(|scene, path| {
            let p = Parts { scene, path };
            let butt = p.butt(rail, stile)?.ok_or(JoineryError::NotTouching)?;
            let o = p.part_box(stile, stile)?;
            if tenon.length > o.1[butt.k] - o.0[butt.k] + TOUCH {
                return Err(JoineryError::Size("The tenon is longer than the other part is deep."));
            }
            let (thin, wide) = across(butt);
            let size = butt.contact.1 - butt.contact.0;
            if tenon.thickness >= size[thin] - EPS || 2.0 * tenon.shoulder >= size[wide] - EPS {
                return Err(JoineryError::Size("The tenon must be thinner and narrower than the rail."));
            }
            let mut region = extrude(butt, tenon.length);
            let mid = (region.0[thin] + region.1[thin]) / 2.0;
            region.0[thin] = mid - tenon.thickness / 2.0;
            region.1[thin] = mid + tenon.thickness / 2.0;
            region.0[wide] += tenon.shoulder;
            region.1[wide] -= tenon.shoulder;
            grow_into(scene, path, rail, stile, region)?;
            cut_box_in(scene, path, stile, region)?;
            let detail = format!(
                "{} long, {} x {} tenon",
                format_length(tenon.length),
                format_length(tenon.thickness),
                format_length(size[wide] - 2.0 * tenon.shoulder)
            );
            record(scene, path, JointKind::MortiseAndTenon, rail, stile, detail)
        })
    }

    /// Dowels across a butt joint: matching holes in `part`'s end and `into`'s face, spread
    /// evenly across the part's width.
    pub fn dowels(&mut self, part: InstanceId, into: InstanceId, dowels: Dowels) -> Result<(), JoineryError> {
        if dowels.count == 0 || dowels.diameter <= EPS || dowels.depth <= EPS {
            return Err(JoineryError::Size("Dowels need a count, a diameter and a depth."));
        }
        self.joint_edit(|scene, path| {
            let p = Parts { scene, path };
            let butt = p.butt(part, into)?.ok_or(JoineryError::NotTouching)?;
            let (thin, wide) = across(butt);
            let size = butt.contact.1 - butt.contact.0;
            let o = p.part_box(into, into)?;
            if dowels.diameter >= size[thin] - EPS {
                return Err(JoineryError::Size("The dowels must be thinner than the part."));
            }
            if dowels.depth >= o.1[butt.k] - o.0[butt.k] - EPS {
                return Err(JoineryError::Size("The holes would go right through the other part."));
            }
            let spacing = size[wide] / f64::from(dowels.count);
            if spacing < dowels.diameter + EPS {
                return Err(JoineryError::Size("Too many dowels for the width."));
            }
            let to_part = p.relative(into, part)?;
            // `into`'s face, toward the part; the part's end faces the other way.
            let face_normal = DVec3::AXES[butt.k] * -butt.side;
            let end_normal = to_part.transform_vector3(-face_normal).normalize();
            let radius = dowels.diameter / 2.0;
            for i in 0..dowels.count {
                let mut c = butt.contact.0;
                c[thin] = (butt.contact.0[thin] + butt.contact.1[thin]) / 2.0;
                c[wide] = butt.contact.0[wide] + spacing * (f64::from(i) + 0.5);
                drill(model_mut(scene, path, into)?, c, face_normal, radius, dowels.depth)?;
                drill(model_mut(scene, path, part)?, to_part.transform_point3(c), end_normal, radius, dowels.depth)?;
            }
            let detail = format!("{} x {} dowels, {} deep each side", dowels.count, format_length(dowels.diameter), format_length(dowels.depth));
            record(scene, path, JointKind::Dowels, part, into, detail)
        })
    }

    /// A box joint at the corner where `a` and `b` meet (butted: the butting part first grows
    /// through the other's thickness): an odd number of alternating fingers about `finger`
    /// wide across the width.
    pub fn box_joint(&mut self, a: InstanceId, b: InstanceId, finger: f64) -> Result<(), JoineryError> {
        if finger <= EPS {
            return Err(JoineryError::Size("The fingers need a width."));
        }
        let fit = self.joint_fit(a, b)?;
        if !fit.corner {
            return Err(JoineryError::NotCorner);
        }
        self.joint_edit(|scene, path| {
            if !fit.overlap
                && let Some((rail, other)) = fit.butt
            {
                // Grow the butting part through the other's thickness, flush with its outside.
                let p = Parts { scene, path };
                let butt = p.butt(rail, other)?.ok_or(JoineryError::NotCorner)?;
                let o = p.part_box(other, other)?;
                grow_into(scene, path, rail, other, extrude(butt, o.1[butt.k] - o.0[butt.k]))?;
            }
            let p = Parts { scene, path };
            let (ab, bb) = (p.part_box(a, b)?, p.part_box(b, b)?);
            let o = intersect(ab, bb).ok_or(JoineryError::NotCorner)?;
            // Fingers run across the width both parts share.
            let size = o.1 - o.0;
            let spans = |bx: Box3, i: usize| (bx.0[i] - o.0[i]).abs() < TOUCH && (bx.1[i] - o.1[i]).abs() < TOUCH;
            let w = (0..3)
                .filter(|&i| spans(ab, i) && spans(bb, i))
                .max_by(|&x, &y| size[x].total_cmp(&size[y]))
                .ok_or(JoineryError::NotCorner)?;
            let mut n = ((size[w] / finger).round() as u32).max(3);
            if n.is_multiple_of(2) {
                n += 1;
            }
            let f = size[w] / f64::from(n);
            let to_a = p.relative(b, a)?;
            for i in 0..n {
                let mut region = o;
                region.0[w] = o.0[w] + f * f64::from(i);
                region.1[w] = o.0[w] + f * f64::from(i + 1);
                // `a` keeps the odd fingers, `b` the even ones.
                if i % 2 == 0 {
                    cut_box_in(scene, path, a, transform_box(&to_a, region))?;
                } else {
                    cut_box_in(scene, path, b, region)?;
                }
            }
            record(scene, path, JointKind::BoxJoint, a, b, format!("{n} fingers, {} wide", format_length(f)))
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

    /// Make a joint on a copy of the scene, then keep it as one undo step; nothing changes if
    /// it fails.
    fn joint_edit<R>(&mut self, change: impl FnOnce(&mut Scene, &[InstanceId]) -> Result<R, JoineryError>) -> Result<R, JoineryError> {
        let path = self.path().to_vec();
        let mut trial = self.scene().clone();
        let result = change(&mut trial, &path)?;
        self.edit_scene(|scene, _| *scene = trial);
        Ok(result)
    }
}

/// Note the joint on the drawing.
fn record(scene: &mut Scene, path: &[InstanceId], kind: JointKind, cut: InstanceId, into: InstanceId, detail: String) -> Result<(), JoineryError> {
    let p = Parts { scene, path };
    let (cut, into) = (p.definition(cut)?, p.definition(into)?);
    scene.joints.push(Joint { kind, cut, into, detail });
    Ok(())
}

/// The contact rectangle of `butt` pushed `depth` into the other part.
fn extrude(butt: Butt, depth: f64) -> Box3 {
    let (mut lo, mut hi) = butt.contact;
    if butt.side > 0.0 {
        hi[butt.k] = lo[butt.k] + depth;
    } else {
        lo[butt.k] = hi[butt.k] - depth;
    }
    (lo, hi)
}

/// The contact's two in-plane axes: (thinner, wider).
fn across(butt: Butt) -> (usize, usize) {
    let size = butt.contact.1 - butt.contact.0;
    let (u, v) = ((butt.k + 1) % 3, (butt.k + 2) % 3);
    if size[u] <= size[v] { (u, v) } else { (v, u) }
}

/// The geometry of `instance` to change (a group shared with copies gets its own first).
fn model_mut<'s>(scene: &'s mut Scene, path: &[InstanceId], instance: InstanceId) -> Result<&'s mut Model, JoineryError> {
    scene.make_unique_if_group(path, instance);
    let def = Parts { scene, path }.definition(instance)?;
    Ok(&mut scene.definition_mut(def).ok_or(JoineryError::NotParts)?.container.model)
}

/// Cut `region` (in `instance`'s coordinates) out of it.
fn cut_box_in(scene: &mut Scene, path: &[InstanceId], instance: InstanceId, region: Box3) -> Result<(JointKind, f64), JoineryError> {
    let def = Parts { scene, path }.definition(instance)?;
    let grain = scene.definition(def).map(|d| (d.part.grain, crate::part::extents(d, glam::DMat3::IDENTITY)));
    cut_box(model_mut(scene, path, instance)?, region, grain)
}

/// Grow `part` by `region` (in `frame`'s coordinates), which sits against its end: a let-in
/// shelf's extra length, a tenon.
fn grow_into(scene: &mut Scene, path: &[InstanceId], part: InstanceId, frame: InstanceId, region: Box3) -> Result<(), JoineryError> {
    let p = Parts { scene, path };
    let in_part = transform_box(&p.relative(frame, part)?, region);
    let pb = p.part_box(part, part)?;
    let (axis, outward) = (0..3)
        .find_map(|k| {
            if in_part.0[k] >= pb.1[k] - TOUCH {
                Some((k, 1.0))
            } else if in_part.1[k] <= pb.0[k] + TOUCH {
                Some((k, -1.0))
            } else {
                None
            }
        })
        .ok_or(JoineryError::NoSurface)?;
    add_box(model_mut(scene, path, part)?, in_part, axis, outward)
}

fn transform_box(t: &DAffine3, (lo, hi): Box3) -> Box3 {
    let (mut a, mut b) = (DVec3::splat(f64::INFINITY), DVec3::splat(f64::NEG_INFINITY));
    for i in 0..8 {
        let c = t.transform_point3(DVec3::new(
            if i & 1 == 0 { lo.x } else { hi.x },
            if i & 2 == 0 { lo.y } else { hi.y },
            if i & 4 == 0 { lo.z } else { hi.z },
        ));
        a = a.min(c);
        b = b.max(c);
    }
    // Snap away floating-point noise from the transforms.
    let snap = |v: DVec3| (v * 1e9).round() / 1e9;
    (snap(a), snap(b))
}

/// The rectangle of `bx` on the plane `axis = at`.
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

/// The model's face through `point`, facing `normal`.
fn face_under(model: &Model, point: DVec3, normal: DVec3) -> Option<crate::FaceId> {
    model.faces().find_map(|(id, f)| {
        let on_plane = (point - model.vertex(f.outer[0])).dot(f.normal).abs() < TOUCH;
        (f.normal.dot(normal) > 1.0 - 1e-6 && on_plane && model.face_contains(f, point)).then_some(id)
    })
}

/// Draw `points` on the surface facing `normal` and push or pull the face it makes by
/// `distance`. When the polygon is a whole existing face, that face is used.
fn draw_and_push(model: &mut Model, points: &[DVec3], normal: DVec3, distance: f64) -> Result<(), JoineryError> {
    let center = points.iter().copied().sum::<DVec3>() / points.len() as f64;
    let mut trial = model.clone();
    let face = match trial.draw_polygon(points, normal) {
        Some(face) => face,
        // The polygon is a whole face already (a shelf's entire end).
        None => face_under(&trial, center, normal).ok_or(JoineryError::NoSurface)?,
    };
    if face_under(&trial, center, normal) != Some(face) {
        return Err(JoineryError::NoSurface);
    }
    trial.push_pull(face, distance).map_err(|e| JoineryError::Geometry(e.to_string()))?;
    *model = trial;
    Ok(())
}

/// Cut the box `bx` out of the model: draw its face on the surface it opens onto (its largest
/// face lying on the part's outside) and push it in. Returns what kind of cut that is and its
/// depth. `grain` (the part's grain axis and extents) tells a groove (along the grain) from a
/// dado.
fn cut_box(
    model: &mut Model,
    bx: Box3,
    grain: Option<(Option<crate::part::PartAxis>, Option<DVec3>)>,
) -> Result<(JointKind, f64), JoineryError> {
    let (plo, phi) = model.bounds().ok_or(JoineryError::NoSurface)?;
    let size = bx.1 - bx.0;
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
    let at = |k: usize, sign: f64| if sign > 0.0 { phi[k] } else { plo[k] };
    let &(k, sign, _) = entries
        .iter()
        .find(|&&(k, sign, _)| {
            let mut center = (bx.0 + bx.1) / 2.0;
            center[k] = at(k, sign);
            face_under(model, center, DVec3::AXES[k] * sign).is_some()
        })
        .ok_or(JoineryError::NoSurface)?;
    draw_and_push(model, &face_rect(bx, k, at(k, sign)), DVec3::AXES[k] * sign, -size[k])?;

    // What kind of cut: which of the part's other faces the box reaches.
    let (u, v) = ((k + 1) % 3, (k + 2) % 3);
    let reaches = |i: usize| u8::from((bx.0[i] - plo[i]).abs() < TOUCH) + u8::from((bx.1[i] - phi[i]).abs() < TOUCH);
    let kind = match (reaches(u), reaches(v)) {
        (2, 0) | (0, 2) => {
            let along = if reaches(u) == 2 { u } else { v };
            let grain_axis = grain.and_then(|(g, ext)| {
                g.map(crate::part::PartAxis::index).or_else(|| ext.map(|e| (0..3).max_by(|&a, &b| e[a].total_cmp(&e[b])).unwrap_or(0)))
            });
            if grain_axis == Some(along) { JointKind::Groove } else { JointKind::Dado }
        }
        (2, 1) | (1, 2) => JointKind::Rabbet,
        // A pocket: long and narrow is a stopped groove (a panel's edge), else a mortise.
        (0, 0) if reaches(k) < 2 && size[u].max(size[v]) > 3.0 * size[u].min(size[v]) => JointKind::Groove,
        (0, 0) if reaches(k) < 2 => JointKind::Mortise,
        _ => JointKind::Notch,
    };
    Ok((kind, size[k]))
}

/// Add the box `bx` to the model, growing out of its face at the `axis` side facing `outward`.
fn add_box(model: &mut Model, bx: Box3, axis: usize, outward: f64) -> Result<(), JoineryError> {
    let at = if outward > 0.0 { bx.0[axis] } else { bx.1[axis] };
    draw_and_push(model, &face_rect(bx, axis, at), DVec3::AXES[axis] * outward, bx.1[axis] - bx.0[axis])
}

/// A round hole `depth` deep into the surface facing `normal` at `center`.
fn drill(model: &mut Model, center: DVec3, normal: DVec3, radius: f64, depth: f64) -> Result<(), JoineryError> {
    draw_and_push(model, &circle_points(center, normal, radius, 24), normal, -depth)
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

    fn model(doc: &Document, part: InstanceId) -> &Model {
        let def = doc.container().instance(part).unwrap().definition;
        &doc.scene().definition(def).unwrap().container.model
    }

    fn closed(doc: &Document, part: InstanceId) -> bool {
        let model = model(doc, part);
        model.edges().all(|(id, _)| model.edge_faces(id).len() == 2)
    }

    /// A side standing up (3/4" thick, x in [-3/4, 0]; 11 1/4" deep along Y; 30" tall).
    fn side(doc: &mut Document) -> InstanceId {
        board(doc, DVec3::ZERO, DVec3::Z, DVec3::Y, [30.0, 11.25, 0.75])
    }

    #[test]
    fn an_overlapping_shelf_cuts_a_dado() {
        let mut doc = Document::default();
        let side = side(&mut doc);
        let shelf = board(&mut doc, DVec3::new(-0.25, 0.0, 12.0), DVec3::X, DVec3::Y, [20.0, 11.25, 0.75]);
        let fit = doc.joint_fit(shelf, side).unwrap();
        assert!(fit.overlap && !fit.half_lap && !fit.corner, "{fit:?}");
        assert_eq!(doc.cut_overlap(shelf, side), Ok(JointKind::Dado));
        assert!(closed(&doc, side));
        assert_eq!(model(&doc, side).faces().count(), 10, "inner face in two, the dado's bottom and two walls");
        let lengths: Vec<f64> = cut_list(doc.scene()).materials[0].parts.iter().map(|p| p.length).collect();
        assert!(lengths.contains(&20.0) && lengths.contains(&30.0), "{lengths:?}");
        assert_eq!(doc.joints_of(side)[0].0.detail, "1/4\" deep");
        doc.undo();
        assert!(doc.joints_of(side).is_empty(), "one undo step");
    }

    #[test]
    fn a_butted_shelf_is_let_in_with_a_dado_and_grows() {
        let mut doc = Document::default();
        let side = side(&mut doc);
        let shelf = board(&mut doc, DVec3::new(0.0, 0.0, 12.0), DVec3::X, DVec3::Y, [20.0, 11.25, 0.75]);
        let fit = doc.joint_fit(shelf, side).unwrap();
        assert_eq!(fit.butt, Some((shelf, side)));
        assert_eq!(fit.let_in_depth, 0.25);
        assert_eq!(doc.let_in(shelf, side, 0.25), Ok(JointKind::Dado));
        assert!(closed(&doc, side) && closed(&doc, shelf));
        assert_eq!(doc.part_size(shelf).unwrap()[0], 20.25, "the shelf grew into the dado");
        let (lo, _) = doc.scene().bounds().unwrap();
        assert!((lo.x + 0.75).abs() < 1e-9, "nothing pokes out of the side");
    }

    #[test]
    fn a_butted_top_at_the_sides_end_is_let_in_with_a_rabbet() {
        let mut doc = Document::default();
        let side = side(&mut doc);
        let top = board(&mut doc, DVec3::new(0.0, 0.0, 29.25), DVec3::X, DVec3::Y, [20.0, 11.25, 0.75]);
        assert_eq!(doc.let_in(top, side, 0.375), Ok(JointKind::Rabbet));
        assert!(closed(&doc, side) && closed(&doc, top));
    }

    #[test]
    fn an_overlapping_top_cuts_a_rabbet() {
        let mut doc = Document::default();
        let side = side(&mut doc);
        let top = board(&mut doc, DVec3::new(-0.375, 0.0, 29.25), DVec3::X, DVec3::Y, [20.0, 11.25, 0.75]);
        assert_eq!(doc.cut_overlap(top, side), Ok(JointKind::Rabbet));
        assert!(closed(&doc, side));
    }

    #[test]
    fn two_shelves_cut_two_dados() {
        let mut doc = Document::default();
        let side = side(&mut doc);
        let a = board(&mut doc, DVec3::new(0.0, 0.0, 12.0), DVec3::X, DVec3::Y, [20.0, 11.25, 0.75]);
        let b = board(&mut doc, DVec3::new(0.0, 0.0, 4.0), DVec3::X, DVec3::Y, [20.0, 11.25, 0.75]);
        doc.let_in(a, side, 0.25).unwrap();
        doc.let_in(b, side, 0.25).unwrap();
        assert!(closed(&doc, side));
        assert_eq!(doc.joints_of(side).len(), 2);
    }

    #[test]
    fn a_panel_in_a_rail_cuts_a_groove_along_its_grain() {
        let mut doc = Document::default();
        let rail = board(&mut doc, DVec3::ZERO, DVec3::X, DVec3::Y, [24.0, 2.5, 0.75]);
        let panel = board(&mut doc, DVec3::new(0.0, 2.125, 0.25), DVec3::X, DVec3::Z, [24.0, 0.25, 10.0]);
        assert_eq!(doc.cut_overlap(panel, rail), Ok(JointKind::Groove));
        assert!(closed(&doc, rail));
    }

    #[test]
    fn a_panels_edge_against_a_post_is_let_into_a_groove() {
        let mut doc = Document::default();
        // A post standing up (x 0..3 1/2, y 0..1 1/2) and a 3/4" panel whose long edge (24")
        // butts its face at x = 3 1/2, centered on its thickness.
        let post = board(&mut doc, DVec3::ZERO, DVec3::Z, DVec3::X, [84.0, 3.5, 1.5]);
        let panel = board(&mut doc, DVec3::new(3.5, 0.375, 30.0), DVec3::Z, DVec3::X, [24.0, 12.0, 0.75]);
        let fit = doc.joint_fit(panel, post).unwrap();
        assert_eq!(fit.butt, Some((panel, post)), "an edge counts, not just an end");
        assert_eq!(fit.let_in_depth, 0.5, "suggested depth tops out at 1/2\"");
        assert_eq!(doc.let_in(panel, post, 0.5), Ok(JointKind::Groove));
        assert!(closed(&doc, panel) && closed(&doc, post));
        assert_eq!(doc.part_size(panel), Some([24.0, 12.5, 0.75]), "the panel grew into the groove");
    }

    #[test]
    fn apart_parts_offer_nothing() {
        let mut doc = Document::default();
        let a = board(&mut doc, DVec3::ZERO, DVec3::X, DVec3::Y, [10.0, 3.0, 0.75]);
        let b = board(&mut doc, DVec3::new(0.0, 10.0, 0.0), DVec3::X, DVec3::Y, [10.0, 3.0, 0.75]);
        let fit = doc.joint_fit(a, b).unwrap();
        assert!(!fit.overlap && fit.butt.is_none() && !fit.corner);
        assert_eq!(doc.cut_overlap(a, b), Err(JoineryError::NoOverlap));
        assert_eq!(doc.let_in(a, b, 0.25), Err(JoineryError::NotTouching));
    }

    #[test]
    fn crossing_boards_half_lap() {
        let mut doc = Document::default();
        let a = board(&mut doc, DVec3::ZERO, DVec3::X, DVec3::Y, [20.0, 3.0, 0.75]);
        let b = board(&mut doc, DVec3::new(8.5, -8.0, 0.0), DVec3::Y, -DVec3::X, [20.0, 3.0, 0.75]);
        assert!(doc.joint_fit(a, b).unwrap().half_lap);
        doc.half_lap(a, b).unwrap();
        assert!(closed(&doc, a) && closed(&doc, b));
        assert_eq!(doc.joints_of(a)[0].0.kind, JointKind::HalfLap);
    }

    /// A stile standing up (x 0..3, y 0..1 1/2, z 0..30) and a rail butting its side.
    fn stile_and_rail(doc: &mut Document) -> (InstanceId, InstanceId) {
        let stile = board(doc, DVec3::ZERO, DVec3::Z, DVec3::X, [30.0, 3.0, 1.5]);
        let rail = board(doc, DVec3::new(3.0, 0.0, 12.5), DVec3::X, -DVec3::Z, [20.0, 2.5, 1.5]);
        (stile, rail)
    }

    #[test]
    fn a_rail_gets_a_tenon_and_the_stile_a_mortise() {
        let mut doc = Document::default();
        let (stile, rail) = stile_and_rail(&mut doc);
        let fit = doc.joint_fit(rail, stile).unwrap();
        assert_eq!(fit.butt, Some((rail, stile)));
        assert_eq!(fit.tenon.thickness, 0.5, "a third of the rail");
        doc.mortise_and_tenon(rail, stile, Tenon { length: 1.0, ..fit.tenon }).unwrap();
        assert!(closed(&doc, rail) && closed(&doc, stile));
        assert_eq!(doc.part_size(rail).unwrap()[0], 21.0, "the cut list counts the tenon");
        assert_eq!(doc.joints_of(stile)[0].0.kind, JointKind::MortiseAndTenon);
    }

    #[test]
    fn dowels_drill_both_parts() {
        let mut doc = Document::default();
        let (stile, rail) = stile_and_rail(&mut doc);
        let fit = doc.joint_fit(rail, stile).unwrap();
        assert_eq!(fit.dowels.diameter, 0.5);
        doc.dowels(rail, stile, Dowels { count: 2, diameter: 0.375, depth: 1.0 }).unwrap();
        assert!(closed(&doc, rail) && closed(&doc, stile));
        assert_eq!(doc.part_size(rail).unwrap()[0], 20.0, "the rail keeps its length");
        // Each hole: 24 walls and a bottom.
        assert_eq!(model(&doc, rail).faces().count(), 6 + 2 * 25);
        assert_eq!(model(&doc, stile).faces().count(), 6 + 2 * 25);
    }

    #[test]
    fn a_butted_corner_gets_a_box_joint() {
        let mut doc = Document::default();
        // A drawer front along X, 6" tall, thickness along -Y (X x Z = -Y): y in [-3/4, 0].
        // A side runs back along -Y from behind it... it butts the front's back face at y = -3/4,
        // flush with the front's end at x = 0.
        let front = board(&mut doc, DVec3::ZERO, DVec3::X, DVec3::Z, [16.0, 6.0, 0.75]);
        // (-Y) x (-Z) = X: the side spans x 0..3/4, flush with the front's end.
        let side = board(&mut doc, DVec3::new(0.0, -0.75, 6.0), -DVec3::Y, -DVec3::Z, [14.0, 6.0, 0.75]);
        let fit = doc.joint_fit(side, front).unwrap();
        assert!(fit.corner && fit.butt == Some((side, front)), "{fit:?}");
        doc.box_joint(side, front, 0.75).unwrap();
        assert!(closed(&doc, side) && closed(&doc, front));
        assert_eq!(doc.part_size(side).unwrap()[0], 14.75, "the side grew through the front");
        assert_eq!(doc.joints_of(front)[0].0.detail, "9 fingers, ~ 11/16\" wide");
    }
}
