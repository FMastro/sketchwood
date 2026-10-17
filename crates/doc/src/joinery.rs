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
    Biscuits,
    LooseTenons,
    PocketScrews,
    Dovetails,
    Splines,
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
            Self::Biscuits => "Biscuits",
            Self::LooseTenons => "Loose tenons",
            Self::PocketScrews => "Pocket screws",
            Self::Dovetails => "Dovetails",
            Self::Splines => "Spline",
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
    /// What it takes besides glue: dowels, biscuits, screws.
    #[serde(default)]
    pub hardware: Option<Hardware>,
    /// The cuts and additions it made to each part, to take it out again.
    #[serde(default)]
    pub ops: Vec<JointOp>,
}

/// Pieces a joint uses, for the shopping list: `3 x 3/8" x 2" dowel`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hardware {
    pub name: String,
    pub count: u32,
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
    /// A haunch: a short stub of tenon filling the shoulder at the stile's end, as deep as this
    /// (0 for none), to fill a panel groove there.
    pub haunch: f64,
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
#[derive(Clone, Debug, PartialEq)]
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
    /// How deep the other part is behind a butt joint (a through tenon's length).
    pub depth: f64,
    /// Where they butt: the butting part's thickness and width there.
    pub contact: (f64, f64),
    /// The joints these two designs already have (made on these parts, or on other copies of
    /// the same components), as `(kind, detail)`.
    pub joined: Vec<(JointKind, String)>,
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
            tenon: Tenon { length: 1.0, thickness: 0.25, shoulder: 0.25, haunch: 0.0 },
            dowels: Dowels { count: 2, diameter: 0.375, depth: 1.0 },
            finger: 0.5,
            depth: 0.0,
            contact: (0.0, 0.0),
            joined: Vec::new(),
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
            fit.depth = depth;
            fit.contact = (rail_thick, rail_wide);
            fit.let_in_depth = sixteenth((depth / 3.0).min(0.5)).max(0.125);
            fit.tenon = Tenon {
                haunch: 0.0,
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
        let (da, db) = (p.definition(a)?, p.definition(b)?);
        fit.joined = self
            .scene()
            .joints
            .iter()
            .filter(|j| (j.cut, j.into) == (da, db) || (j.cut, j.into) == (db, da))
            .map(|j| (j.kind, j.detail.clone()))
            .collect();
        fit.joined.dedup();
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
            let Some(butt) = p.butt(part, into)? else {
                return repeat(scene, path, part, into, |k| !matches!(k, JointKind::MortiseAndTenon | JointKind::Dowels | JointKind::BoxJoint | JointKind::HalfLap));
            };
            let o = p.part_box(into, into)?;
            if depth <= EPS || depth > o.1[butt.k] - o.0[butt.k] + TOUCH {
                return Err(JoineryError::Size("The depth must be more than zero and no more than the other part's thickness."));
            }
            let region = extrude(butt, depth);
            grow_into(scene, path, part, into, butt, region)?;
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
            let Some(butt) = p.butt(rail, stile)? else {
                return repeat(scene, path, rail, stile, |k| k == JointKind::MortiseAndTenon).map(|_| ());
            };
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
            // A haunch fills the shoulder toward the stile's nearer end (a panel groove runs out there):
            // the tenon and haunch come out together as deep as the haunch, then the tenon goes on.
            let haunched = tenon.haunch > EPS && tenon.shoulder > EPS && tenon.haunch < tenon.length;
            if haunched {
                let k = butt.k;
                let mut both = region;
                if butt.contact.0[wide] - o.0[wide] < o.1[wide] - butt.contact.1[wide] {
                    both.0[wide] = butt.contact.0[wide];
                } else {
                    both.1[wide] = butt.contact.1[wide];
                }
                let c = butt.contact.0[k];
                let step = c + butt.side * tenon.haunch;
                let toward = DVec3::AXES[k] * butt.side;
                push_polygon_in(scene, path, rail, stile, &face_rect(both, k, c), toward, -tenon.haunch)?;
                push_polygon_in(scene, path, rail, stile, &face_rect(region, k, step), toward, -(tenon.length - tenon.haunch))?;
                push_polygon_in(scene, path, stile, stile, &face_rect(both, k, c), -toward, tenon.haunch)?;
                push_polygon_in(scene, path, stile, stile, &face_rect(region, k, step), -toward, tenon.length - tenon.haunch)?;
            } else {
                grow_into(scene, path, rail, stile, butt, region)?;
                cut_box_in(scene, path, stile, region)?;
            }
            let detail = format!(
                "{} long, {} x {} tenon{}",
                format_length(tenon.length),
                format_length(tenon.thickness),
                format_length(size[wide] - 2.0 * tenon.shoulder),
                if haunched { format!(", {} haunch", format_length(tenon.haunch)) } else { String::new() }
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
            let Some(butt) = p.butt(part, into)? else {
                return repeat(scene, path, part, into, |k| k == JointKind::Dowels).map(|_| ());
            };
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
                drill_in(scene, path, into, c, face_normal, radius, dowels.depth)?;
                drill_in(scene, path, part, to_part.transform_point3(c), end_normal, radius, dowels.depth)?;
            }
            let detail = format!("{} x {} dowels, {} deep each side", dowels.count, format_length(dowels.diameter), format_length(dowels.depth));
            let hardware = Hardware { name: format!("{} x {} dowel", format_length(dowels.diameter), format_length(2.0 * dowels.depth)), count: dowels.count };
            record_with(scene, path, JointKind::Dowels, part, into, detail, Some(hardware))
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
                grow_into(scene, path, rail, other, butt, extrude(butt, o.1[butt.k] - o.0[butt.k]))?;
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

/// The parts don't touch, but these two designs already have this joint: another copy of a shared
/// component made it (the back apron, a copy of the front, already has its tenons). Record it
/// again, changing nothing; otherwise they don't touch.
fn repeat(scene: &mut Scene, path: &[InstanceId], part: InstanceId, into: InstanceId, kind: impl Fn(JointKind) -> bool) -> Result<JointKind, JoineryError> {
    let p = Parts { scene, path };
    let (cut, into_def) = (p.definition(part)?, p.definition(into)?);
    let earlier = scene.joints.iter().find(|j| j.cut == cut && j.into == into_def && kind(j.kind)).cloned().ok_or(JoineryError::NotTouching)?;
    scene.joints.push(earlier.clone());
    Ok(earlier.kind)
}

/// Note the joint on the drawing.
fn record(scene: &mut Scene, path: &[InstanceId], kind: JointKind, cut: InstanceId, into: InstanceId, detail: String) -> Result<(), JoineryError> {
    record_with(scene, path, kind, cut, into, detail, None)
}

/// Note the joint and the pieces it uses.
fn record_with(
    scene: &mut Scene,
    path: &[InstanceId],
    kind: JointKind,
    cut: InstanceId,
    into: InstanceId,
    detail: String,
    hardware: Option<Hardware>,
) -> Result<(), JoineryError> {
    let p = Parts { scene, path };
    let (cut, into) = (p.definition(cut)?, p.definition(into)?);
    let ops = std::mem::take(&mut scene.pending_ops);
    scene.joints.push(Joint { kind, cut, into, detail, hardware, ops });
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

/// The definition holding a part's own faces, and the map from its coordinates to the part's:
/// through wrappers (a component made from a group holds just that group).
fn geometry(scene: &Scene, path: &[InstanceId], instance: InstanceId) -> Result<(DefinitionId, DAffine3), JoineryError> {
    let mut def = Parts { scene, path }.definition(instance)?;
    let mut to_part = DAffine3::IDENTITY;
    for _ in 0..8 {
        let container = &scene.definition(def).ok_or(JoineryError::NotParts)?.container;
        if container.model.faces().next().is_some() {
            break;
        }
        let mut inner = container.instances();
        match (inner.next(), inner.next()) {
            (Some((_, only)), None) => {
                to_part *= only.transform;
                def = only.definition;
            }
            _ => break,
        }
    }
    Ok((def, to_part))
}

/// One cut or addition a joint made to a part's geometry, in that geometry's coordinates: a
/// polygon drawn on the surface facing `normal`, pushed (negative) or pulled by `distance`.
/// Joints keep theirs, so one can be taken out by rebuilding its parts without it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct JointOp {
    pub part: DefinitionId,
    pub points: Vec<DVec3>,
    pub normal: DVec3,
    pub distance: f64,
}

/// A planned push or pull, before it's made.
struct Plan {
    points: Vec<DVec3>,
    normal: DVec3,
    distance: f64,
}

/// Make a planned cut or addition on `instance`'s geometry (a group shared with copies gets its
/// own first), and log it for the joint being made. `plan` gets the geometry and the map from
/// the part's coordinates into it.
fn apply<R>(
    scene: &mut Scene,
    path: &[InstanceId],
    instance: InstanceId,
    plan: impl FnOnce(&Model, DAffine3) -> Result<(Plan, R), JoineryError>,
) -> Result<R, JoineryError> {
    scene.make_unique_if_group(path, instance);
    let (def, to_part) = geometry(scene, path, instance)?;
    let model = &scene.definition(def).ok_or(JoineryError::NotParts)?.container.model;
    let (op, result) = plan(model, to_part.inverse())?;
    // How the part looked before its first joint, to rebuild it without one later.
    if !scene.joint_bases.iter().any(|(d, _)| *d == def) {
        let base = model.clone();
        scene.joint_bases.push((def, base));
    }
    let model = &mut scene.definition_mut(def).ok_or(JoineryError::NotParts)?.container.model;
    draw_and_push(model, &op.points, op.normal, op.distance)?;
    scene.pending_ops.push(JointOp { part: def, points: op.points, normal: op.normal, distance: op.distance });
    Ok(result)
}

/// Cut `region` (in `instance`'s coordinates) out of it.
fn cut_box_in(scene: &mut Scene, path: &[InstanceId], instance: InstanceId, region: Box3) -> Result<(JointKind, f64), JoineryError> {
    let (def, _) = geometry(scene, path, instance)?;
    let grain = scene.definition(def).map(|d| (d.part.grain, crate::part::extents(d, glam::DMat3::IDENTITY)));
    apply(scene, path, instance, |model, from_part| {
        let (plan, kind, depth) = plan_cut(model, transform_box(&from_part, region), grain)?;
        Ok((plan, (kind, depth)))
    })
}

/// Grow `part` by `region` (in `frame`'s coordinates), which sits against its end: a let-in
/// shelf's extra length, a tenon.
fn grow_into(scene: &mut Scene, path: &[InstanceId], part: InstanceId, frame: InstanceId, butt: Butt, region: Box3) -> Result<(), JoineryError> {
    let to_part = Parts { scene, path }.relative(frame, part)?;
    let in_part = transform_box(&to_part, region);
    apply(scene, path, part, |_, from_part| {
        let in_model = transform_box(&from_part, in_part);
        // It grows the way its end faces: toward the other part.
        let toward = from_part.transform_vector3(to_part.transform_vector3(DVec3::AXES[butt.k] * butt.side));
        let axis = (0..3).max_by(|&a, &b| toward[a].abs().total_cmp(&toward[b].abs())).unwrap_or(0);
        let outward = toward[axis].signum();
        let at = if outward > 0.0 { in_model.0[axis] } else { in_model.1[axis] };
        let plan = Plan {
            points: face_rect(in_model, axis, at).to_vec(),
            normal: DVec3::AXES[axis] * outward,
            distance: in_model.1[axis] - in_model.0[axis],
        };
        Ok((plan, ()))
    })
}

/// A round hole into `instance` (`center` and `normal` in its coordinates).
fn drill_in(scene: &mut Scene, path: &[InstanceId], instance: InstanceId, center: DVec3, normal: DVec3, radius: f64, depth: f64) -> Result<(), JoineryError> {
    apply(scene, path, instance, |_, from_part| {
        let normal = from_part.transform_vector3(normal).normalize();
        let points = circle_points(from_part.transform_point3(center), normal, radius, 24);
        Ok((Plan { points, normal, distance: -depth }, ()))
    })
}

/// Push a polygon (in `frame`'s coordinates, on the surface facing `normal`) into `instance` by
/// `depth`: a dovetail socket, the waste between tails.
fn push_polygon_in(
    scene: &mut Scene,
    path: &[InstanceId],
    instance: InstanceId,
    frame: InstanceId,
    points: &[DVec3],
    normal: DVec3,
    depth: f64,
) -> Result<(), JoineryError> {
    let to_part = Parts { scene, path }.relative(frame, instance)?;
    apply(scene, path, instance, |_, from_part| {
        let t = from_part * to_part;
        let points = points.iter().map(|&p| t.transform_point3(p)).collect();
        Ok((Plan { points, normal: t.transform_vector3(normal).normalize(), distance: -depth }, ()))
    })
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
    // Already done here: another copy of a shared component got this same cut or tenon.
    if face_under(model, center, normal).is_none() && already_done(model, center, normal, distance) {
        return Ok(());
    }
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

/// Whether pushing or pulling the surface at `center` (facing `normal`) by `distance` was already
/// done: there's a face where it would end, facing the same way (a pocket's floor, a tenon's end).
fn already_done(model: &Model, center: DVec3, normal: DVec3, distance: f64) -> bool {
    face_under(model, center + normal * distance, normal).is_some()
}

/// Plan cutting the box `bx` out of the model: its face on the surface it opens onto (its
/// largest face lying on the part's outside), pushed in. Also what kind of cut that is and its
/// depth. `grain` (the part's grain axis and extents) tells a groove (along the grain) from a
/// dado.
fn plan_cut(model: &Model, bx: Box3, grain: Option<(Option<crate::part::PartAxis>, Option<DVec3>)>) -> Result<(Plan, JointKind, f64), JoineryError> {
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
            face_under(model, center, DVec3::AXES[k] * sign).is_some() || already_done(model, center, DVec3::AXES[k] * sign, -size[k])
        })
        .ok_or(JoineryError::NoSurface)?;
    let plan = Plan { points: face_rect(bx, k, at(k, sign)).to_vec(), normal: DVec3::AXES[k] * sign, distance: -size[k] };

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
    Ok((plan, kind, size[k]))
}

/// The geometry of each part the joint at `index` changed, rebuilt from how it looked before
/// its first joint with every other joint's cuts. Errors if a part was changed by hand since
/// (rebuilding would lose that), or the joint was made before joints kept their cuts.
fn rebuilt_without(scene: &Scene, index: usize) -> Result<Vec<(DefinitionId, Model)>, JoineryError> {
    let joint = scene.joints.get(index).ok_or(JoineryError::NotParts)?;
    if joint.ops.is_empty() && joint.kind != JointKind::PocketScrews {
        // Shared components: a copy's record has no cuts of its own when another copy made them.
        let shared = scene.joints.iter().enumerate().any(|(i, j)| i != index && j.cut == joint.cut && j.into == joint.into && j.kind == joint.kind);
        if !shared {
            return Err(JoineryError::Size("That joint was made in an older version of WoodSketch, so it can't be taken out on its own. Use Undo, or rebuild the parts."));
        }
    }
    let parts: std::collections::BTreeSet<DefinitionId> = joint.ops.iter().map(|o| o.part).collect();
    let replay = |base: &Model, def: DefinitionId, skip: Option<usize>| -> Result<Model, JoineryError> {
        let mut model = base.clone();
        for (i, j) in scene.joints.iter().enumerate() {
            if Some(i) == skip {
                continue;
            }
            for op in j.ops.iter().filter(|o| o.part == def) {
                draw_and_push(&mut model, &op.points, op.normal, op.distance)?;
            }
        }
        Ok(model)
    };
    let same = |a: &Model, b: &Model| serde_json::to_string(a).ok() == serde_json::to_string(b).ok();
    let mut rebuilt = Vec::new();
    for def in parts {
        let (Some((_, base)), Some(current)) = (scene.joint_bases.iter().find(|(d, _)| *d == def), scene.definition(def)) else {
            return Err(JoineryError::Size("That joint can't be taken out on its own. Use Undo, or rebuild the parts."));
        };
        if !same(&replay(base, def, None)?, &current.container.model) {
            return Err(JoineryError::Size(
                "A part in that joint was changed by hand after its joints were made, and taking the joint out would lose that. Use Undo instead.",
            ));
        }
        rebuilt.push((def, replay(base, def, Some(index))?));
    }
    Ok(rebuilt)
}

impl Document {
    /// Take out the joint at `index` in [`Scene::joints`]: its parts are rebuilt with every
    /// other joint but this one (a tenon goes, its mortise fills in). One undo step.
    pub fn remove_joint(&mut self, index: usize) -> Result<(), JoineryError> {
        let rebuilt = rebuilt_without(self.scene(), index)?;
        self.edit_scene(|scene, _| {
            for (def, model) in rebuilt {
                if let Some(d) = scene.definition_mut(def) {
                    d.container.model = model;
                }
            }
            scene.joints.remove(index);
        });
        Ok(())
    }

    /// The joints a part has, each with its index in [`Scene::joints`] (for
    /// [`Document::remove_joint`]) and the other part's name.
    pub fn joint_list(&self, instance: InstanceId) -> Vec<(usize, Joint, String)> {
        let Some(def) = self.container().instance(instance).map(|i| i.definition) else { return Vec::new() };
        let scene = self.scene();
        let name = |d: DefinitionId| scene.definition(d).map_or_else(|| "a deleted part".to_owned(), |d| d.name.clone());
        scene
            .joints
            .iter()
            .enumerate()
            .filter(|(_, j)| j.cut == def || j.into == def)
            .map(|(i, j)| (i, j.clone(), name(if j.cut == def { j.into } else { j.cut })))
            .collect()
    }
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
    fn components_wrapping_a_group_still_half_lap() {
        let mut doc = Document::default();
        let a = board(&mut doc, DVec3::ZERO, DVec3::X, DVec3::Y, [20.0, 3.0, 0.75]);
        let b = board(&mut doc, DVec3::new(8.5, -8.0, 0.0), DVec3::Y, -DVec3::X, [20.0, 3.0, 0.75]);
        // Components made the old way: each wraps the board's group.
        let wrap = |doc: &mut Document, part: InstanceId| {
            doc.edit_scene(|scene, path| scene.group(path, &[crate::Entity::Instance(part)], crate::DefinitionKind::Component)).unwrap()
        };
        let (a, b) = (wrap(&mut doc, a), wrap(&mut doc, b));
        doc.half_lap(a, b).unwrap();
        let inner = |doc: &Document, part: InstanceId| geometry(doc.scene(), &[], part).unwrap().0;
        for part in [a, b] {
            let model = &doc.scene().definition(inner(&doc, part)).unwrap().container.model;
            assert!(model.faces().count() > 6, "the board inside was cut");
            assert!(model.edges().all(|(id, _)| model.edge_faces(id).len() == 2));
        }
    }

    #[test]
    fn making_one_group_a_component_converts_it() {
        let mut doc = Document::default();
        let part = board(&mut doc, DVec3::ZERO, DVec3::X, DVec3::Y, [20.0, 3.0, 0.75]);
        doc.selection_mut().extend([crate::Entity::Instance(part)]);
        assert_eq!(doc.group_selection(crate::DefinitionKind::Component), Some(part), "the same part, not a wrapper");
        let def = doc.scene().definition(doc.container().instance(part).unwrap().definition).unwrap();
        assert_eq!(def.kind, crate::DefinitionKind::Component);
        assert_eq!(def.name, "Component#1");
        assert!(def.container.model.faces().count() == 6, "its own faces, no group inside");
        doc.undo();
        assert_eq!(doc.scene().definition(doc.container().instance(part).unwrap().definition).unwrap().kind, crate::DefinitionKind::Group);
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

#[cfg(test)]
mod components {
    use glam::{DAffine3, DVec3};

    use super::*;
    use crate::board::Board;
    use crate::{DefinitionKind, Entity};

    fn component(doc: &mut Document, board: Board) -> InstanceId {
        let id = doc.add_board(&board, None).unwrap();
        doc.selection_mut().clear();
        doc.selection_mut().extend([Entity::Instance(id)]);
        doc.group_selection(DefinitionKind::Component).unwrap()
    }

    /// Copies of `part` turned `degrees` about the vertical through `center` and moved to `to`.
    fn place(doc: &mut Document, part: InstanceId, center: DVec3, spots: &[(DVec3, f64)]) -> Vec<InstanceId> {
        let transforms: Vec<DAffine3> = spots
            .iter()
            .map(|&(to, degrees)| DAffine3::from_translation(to) * DAffine3::from_rotation_z(degrees.to_radians()) * DAffine3::from_translation(-center))
            .collect();
        doc.transform_entities(&[Entity::Instance(part)], &transforms, true)
            .unwrap()
            .into_iter()
            .map(|e| if let Entity::Instance(i) = e { i } else { panic!("an instance") })
            .collect()
    }

    fn faces(doc: &Document, part: InstanceId) -> usize {
        let def = doc.container().instance(part).unwrap().definition;
        doc.scene().definition(def).unwrap().container.model.faces().count()
    }

    fn closed(doc: &Document, part: InstanceId) -> bool {
        let def = doc.container().instance(part).unwrap().definition;
        let model = &doc.scene().definition(def).unwrap().container.model;
        model.edges().all(|(id, _)| model.edge_faces(id).len() == 2)
    }

    #[test]
    fn a_table_with_one_leg_component_turned_to_each_corner_gets_two_mortises_per_leg() {
        let mut doc = Document::default();
        // A leg at the front left corner, its inside faces toward +X and +Y.
        let fl = component(&mut doc, Board { origin: DVec3::new(1.0, 1.0, 0.0), length_dir: DVec3::Z, width_dir: DVec3::X, length: 29.25, width: 1.75, thickness: 1.75 });
        let c = DVec3::new(1.875, 1.875, 0.0);
        let legs = place(&mut doc, fl, c, &[(DVec3::new(34.125, 1.875, 0.0), 90.0), (DVec3::new(34.125, 18.125, 0.0), 180.0), (DVec3::new(1.875, 18.125, 0.0), 270.0)]);
        let (fr, br, bl) = (legs[0], legs[1], legs[2]);
        // Front apron and its copy at the back (turned about the table's center); side aprons too.
        let front = component(&mut doc, Board { origin: DVec3::new(2.75, 1.25, 29.25), length_dir: DVec3::X, width_dir: -DVec3::Z, length: 30.5, width: 4.0, thickness: 0.75 });
        let middle = DVec3::new(18.0, 10.0, 0.0);
        let back = place(&mut doc, front, middle, &[(middle, 180.0)])[0];
        let left = component(&mut doc, Board { origin: DVec3::new(2.0, 2.75, 29.25), length_dir: DVec3::Y, width_dir: -DVec3::Z, length: 14.5, width: 4.0, thickness: 0.75 });
        let right = place(&mut doc, left, middle, &[(middle, 180.0)])[0];

        let tenon = Tenon { length: 0.75, thickness: 0.25, shoulder: 0.25, haunch: 0.0 };
        for (apron, leg) in [(front, fl), (front, fr), (back, br), (back, bl), (left, fl), (left, bl), (right, fr), (right, br)] {
            doc.mortise_and_tenon(apron, leg, tenon).unwrap();
        }
        // One leg design: six sides plus two mortises (four walls and a floor each).
        assert_eq!(faces(&doc, fl), 6 + 2 * 5, "two mortises, not eight");
        assert!(legs.iter().chain([&fl]).all(|&l| faces(&doc, l) == faces(&doc, fl)), "all four legs are the same component");
        // Each apron design: a tenon at each end (four sides and an end each), the rest unchanged.
        assert_eq!(faces(&doc, front), 6 + 2 * 5);
        assert_eq!(faces(&doc, left), 6 + 2 * 5);
        assert!([fl, front, left].iter().all(|&p| closed(&doc, p)));
        assert_eq!(doc.part_size(front).unwrap()[0], 32.0, "30 1/2\" between the legs plus two 3/4\" tenons");
        assert_eq!(doc.scene().joints.len(), 8, "every joint is listed");
        // The back apron and a back leg (copies) read as already joined, not as a cut to make.
        let fit = doc.joint_fit(back, br).unwrap();
        assert_eq!(fit.joined.first().map(|j| j.0), Some(JointKind::MortiseAndTenon));
        assert_eq!(crate::cutlist::cut_list(doc.scene()).materials[0].parts.iter().map(|p| p.count).sum::<usize>(), 8);
    }
}

/// Slots cut into both parts across a butt joint, for a biscuit or a loose tenon each.
#[derive(Clone, Debug, PartialEq)]
pub struct Slots {
    pub kind: JointKind,
    /// What goes in each slot, for the shopping list: `#20 biscuit`.
    pub piece: String,
    pub count: u32,
    /// Along the joint.
    pub length: f64,
    /// Across the part's thickness.
    pub thickness: f64,
    /// Into each part.
    pub depth: f64,
}

/// Biscuit sizes: the name and its slot (length, thickness, depth into each part).
pub const BISCUITS: [(&str, f64, f64, f64); 3] = [("#0", 2.125, 0.15625, 0.3125), ("#10", 2.375, 0.15625, 0.375), ("#20", 2.625, 0.15625, 0.5)];

/// Domino-style loose tenons: thickness x width x length in millimeters.
pub const LOOSE_TENONS: [(u32, u32, u32); 4] = [(5, 19, 30), (6, 20, 40), (8, 22, 40), (10, 24, 50)];

const MM: f64 = 1.0 / 25.4;

impl Slots {
    /// Biscuits of a size (`BISCUITS` index) across a joint `wide` along, about one per 6".
    pub fn biscuits(size: usize, wide: f64) -> Self {
        let (name, length, thickness, depth) = BISCUITS[size.min(BISCUITS.len() - 1)];
        let count = ((wide / 6.0).round() as u32).max(1);
        Slots { kind: JointKind::Biscuits, piece: format!("{name} biscuit"), count, length, thickness, depth }
    }

    /// Loose tenons of a size (`LOOSE_TENONS` index) across a joint `wide` along.
    pub fn loose_tenons(size: usize, wide: f64) -> Self {
        let (t, w, l) = LOOSE_TENONS[size.min(LOOSE_TENONS.len() - 1)];
        let count = ((wide / 6.0).round() as u32).max(1);
        Slots {
            kind: JointKind::LooseTenons,
            piece: format!("{t} x {w} x {l} mm loose tenon"),
            count,
            length: f64::from(w) * MM,
            thickness: f64::from(t) * MM,
            depth: f64::from(l) * MM / 2.0,
        }
    }

    /// The biscuit size (`BISCUITS` index) for stock `thick` thick and a joint `wide` wide.
    pub fn suggested_biscuit(thick: f64, wide: f64) -> usize {
        if wide >= 3.0 && thick >= 0.625 {
            2
        } else if wide >= 2.5 {
            1
        } else {
            0
        }
    }

    /// The loose tenon size (`LOOSE_TENONS` index) for stock `thick` thick.
    pub fn suggested_loose_tenon(thick: f64) -> usize {
        if thick >= 1.25 {
            3
        } else if thick >= 0.875 {
            2
        } else if thick >= 0.625 {
            1
        } else {
            0
        }
    }
}

/// The contact rectangle of `butt` pushed `depth` back into the butting part.
fn extrude_back(butt: Butt, depth: f64) -> Box3 {
    let (mut lo, mut hi) = butt.contact;
    if butt.side > 0.0 {
        lo[butt.k] = hi[butt.k] - depth;
    } else {
        hi[butt.k] = lo[butt.k] + depth;
    }
    (lo, hi)
}

impl Document {
    /// Biscuits or loose tenons across a butt joint: matching slots in `part`'s end and
    /// `into`'s face, spread evenly across the part's width.
    pub fn slots(&mut self, part: InstanceId, into: InstanceId, slots: &Slots) -> Result<(), JoineryError> {
        let sized = slots.length > EPS || slots.kind == JointKind::Splines;
        if slots.count == 0 || !sized || slots.thickness <= EPS || slots.depth <= EPS {
            return Err(JoineryError::Size("The slots need a count and a size."));
        }
        self.joint_edit(|scene, path| {
            let p = Parts { scene, path };
            let Some(butt) = p.butt(part, into)? else {
                return repeat(scene, path, part, into, |k| k == slots.kind).map(|_| ());
            };
            let (thin, wide) = across(butt);
            let size = butt.contact.1 - butt.contact.0;
            let o = p.part_box(into, into)?;
            let r = p.part_box(part, into)?;
            if slots.thickness >= size[thin] - 0.125 {
                return Err(JoineryError::Size("The part is too thin for that size: pick a smaller one."));
            }
            // Into a face, at most 2/3 of the way through; a loose tenon's other end takes up the rest
            // of its length in the butting part's end (a biscuit goes in equally either way).
            let face_depth = slots.depth.min((o.1[butt.k] - o.0[butt.k]) * 2.0 / 3.0);
            let end_depth = if slots.kind == JointKind::LooseTenons { 2.0 * slots.depth - face_depth } else { slots.depth };
            if end_depth >= r.1[butt.k] - r.0[butt.k] - EPS {
                return Err(JoineryError::Size("The slots would go right through a part."));
            }
            // A spline runs the whole joint; the others are spaced across it.
            let spline = slots.kind == JointKind::Splines;
            let length = if spline { size[wide] } else { slots.length };
            let spacing = size[wide] / f64::from(slots.count);
            if !spline && spacing < length + 0.25 {
                return Err(JoineryError::Size("Too many for the width: use fewer, or a smaller size."));
            }
            let to_part = p.relative(into, part)?;
            let mid = (butt.contact.0[thin] + butt.contact.1[thin]) / 2.0;
            for i in 0..slots.count {
                let center = butt.contact.0[wide] + spacing * (f64::from(i) + 0.5);
                let narrow = |(mut lo, mut hi): Box3| {
                    lo[thin] = mid - slots.thickness / 2.0;
                    hi[thin] = mid + slots.thickness / 2.0;
                    lo[wide] = center - length / 2.0;
                    hi[wide] = center + length / 2.0;
                    (lo, hi)
                };
                cut_box_in(scene, path, into, narrow(extrude(butt, face_depth)))?;
                cut_box_in(scene, path, part, transform_box(&to_part, narrow(extrude_back(butt, end_depth))))?;
            }
            let detail = if (face_depth - end_depth).abs() < 1e-9 {
                format!("{} x {}, {} deep each side", slots.count, slots.piece, format_length(face_depth))
            } else {
                format!("{} x {}, {} deep in the face, {} in the end", slots.count, slots.piece, format_length(face_depth), format_length(end_depth))
            };
            let name = if spline {
                format!("{} x {} x {} spline", format_length(slots.thickness), format_length(face_depth + end_depth), format_length(length))
            } else {
                slots.piece.clone()
            };
            let hardware = Hardware { name, count: slots.count };
            record_with(scene, path, slots.kind, part, into, detail, Some(hardware))
        })
    }

    /// Pocket screws across a butt joint, driven from the hidden face of `part` into `into`.
    /// Recorded for the build sheet and shopping list; the holes aren't drawn.
    pub fn pocket_screws(&mut self, part: InstanceId, into: InstanceId, count: u32, screw: f64) -> Result<(), JoineryError> {
        if count == 0 || screw <= EPS {
            return Err(JoineryError::Size("Pocket screws need a count and a length."));
        }
        self.joint_edit(|scene, path| {
            let p = Parts { scene, path };
            let Some(butt) = p.butt(part, into)? else {
                return repeat(scene, path, part, into, |k| k == JointKind::PocketScrews).map(|_| ());
            };
            // The pockets go on the part's face that's hidden: the one facing down, or else back.
            let (thin, wide) = across(butt);
            let world = scene.transform(path) * scene.container(path).and_then(|c| c.instance(into)).ok_or(JoineryError::NotParts)?.transform;
            let facing = |sign: f64| world.transform_vector3(DVec3::AXES[thin] * sign);
            let hidden = |d: DVec3| if d.z.abs() > 0.5 { -d.z } else { d.y };
            let sign = if hidden(facing(1.0)) >= hidden(facing(-1.0)) { 1.0 } else { -1.0 };
            let r = p.part_box(part, into)?;
            let thick = butt.contact.1[thin] - butt.contact.0[thin];
            let long = r.1[butt.k] - r.0[butt.k];
            let to_part = p.relative(into, part)?;
            if long > 2.0 {
                let spacing = (butt.contact.1[wide] - butt.contact.0[wide]) / f64::from(count);
                let c = butt.contact.0[butt.k];
                let (k0, k1) = (c - butt.side * 0.25, c - butt.side * 1.5);
                for i in 0..count {
                    let mid = butt.contact.0[wide] + spacing * (f64::from(i) + 0.5);
                    let (mut lo, mut hi) = butt.contact;
                    lo[butt.k] = k0.min(k1);
                    hi[butt.k] = k0.max(k1);
                    lo[wide] = mid - 0.1875;
                    hi[wide] = mid + 0.1875;
                    if sign > 0.0 {
                        lo[thin] = hi[thin] - thick / 2.0;
                    } else {
                        hi[thin] = lo[thin] + thick / 2.0;
                    }
                    cut_box_in(scene, path, part, transform_box(&to_part, (lo, hi)))?;
                }
            }
            let detail = format!("{count} pocket holes in the hidden face, {} screws", format_length(screw));
            let hardware = Hardware { name: format!("{} pocket screw", format_length(screw)), count };
            record_with(scene, path, JointKind::PocketScrews, part, into, detail, Some(hardware))
        })
    }
}

/// The pocket screw length for stock `thick` thick: 1" for 1/2", 1 1/4" for 3/4", 1 1/2" for
/// 1", 2 1/2" for 1 1/2".
pub fn pocket_screw_for(thick: f64) -> f64 {
    if thick >= 1.375 {
        2.5
    } else if thick >= 1.0 {
        1.5
    } else if thick >= 0.625 {
        1.25
    } else {
        1.0
    }
}

#[cfg(test)]
mod more_joints {
    use glam::DVec3;

    use super::*;
    use crate::board::Board;

    fn board(doc: &mut Document, origin: DVec3, length_dir: DVec3, width_dir: DVec3, [l, w, t]: [f64; 3]) -> InstanceId {
        doc.add_board(&Board { origin, length_dir, width_dir, length: l, width: w, thickness: t }, None).unwrap()
    }

    fn model(doc: &Document, part: InstanceId) -> &Model {
        let def = doc.container().instance(part).unwrap().definition;
        &doc.scene().definition(def).unwrap().container.model
    }

    fn closed(doc: &Document, part: InstanceId) -> bool {
        let m = model(doc, part);
        m.edges().all(|(id, _)| m.edge_faces(id).len() == 2)
    }

    /// A side standing up and a shelf butting it (as in the joinery tests).
    fn side_and_shelf(doc: &mut Document) -> (InstanceId, InstanceId) {
        let side = board(doc, DVec3::ZERO, DVec3::Z, DVec3::Y, [30.0, 11.25, 0.75]);
        let shelf = board(doc, DVec3::new(0.0, 0.0, 12.0), DVec3::X, DVec3::Y, [20.0, 11.25, 0.75]);
        (side, shelf)
    }

    #[test]
    fn biscuits_cut_matching_slots_and_count_for_the_shopping_list() {
        let mut doc = Document::default();
        let (side, shelf) = side_and_shelf(&mut doc);
        let slots = Slots::biscuits(Slots::suggested_biscuit(0.75, 11.25), 11.25);
        assert_eq!((slots.piece.as_str(), slots.count), ("#20 biscuit", 2));
        doc.slots(shelf, side, &slots).unwrap();
        assert!(closed(&doc, side) && closed(&doc, shelf));
        // Each slot: four walls and a floor.
        assert_eq!(model(&doc, side).faces().count(), 6 + 2 * 5);
        assert_eq!(model(&doc, shelf).faces().count(), 6 + 2 * 5);
        assert_eq!(doc.part_size(shelf).unwrap()[0], 20.0, "the shelf keeps its length");
        let joint = &doc.joints_of(side)[0].0;
        assert_eq!(joint.hardware, Some(Hardware { name: "#20 biscuit".into(), count: 2 }));
    }

    #[test]
    fn loose_tenons_cut_mortises_in_both_parts() {
        let mut doc = Document::default();
        let (side, shelf) = side_and_shelf(&mut doc);
        let slots = Slots::loose_tenons(Slots::suggested_loose_tenon(0.75), 11.25);
        assert_eq!(slots.piece, "6 x 20 x 40 mm loose tenon");
        doc.slots(shelf, side, &slots).unwrap();
        assert!(closed(&doc, side) && closed(&doc, shelf));
        assert_eq!(doc.joints_of(shelf)[0].0.kind, JointKind::LooseTenons);
    }

    #[test]
    fn pocket_screws_are_listed_without_cutting() {
        let mut doc = Document::default();
        let (side, shelf) = side_and_shelf(&mut doc);
        doc.pocket_screws(shelf, side, 3, pocket_screw_for(0.75)).unwrap();
        assert_eq!(model(&doc, side).faces().count(), 6);
        let joint = &doc.joints_of(side)[0].0;
        assert_eq!(joint.hardware, Some(Hardware { name: "1 1/4\" pocket screw".into(), count: 3 }));
    }

    #[test]
    fn a_through_tenon_shows_on_the_far_side() {
        let mut doc = Document::default();
        // A stile 3" deep where the rail meets it: a 3" tenon goes right through.
        let stile = board(&mut doc, DVec3::ZERO, DVec3::Z, DVec3::X, [30.0, 3.0, 1.5]);
        let rail = board(&mut doc, DVec3::new(3.0, 0.0, 12.5), DVec3::X, -DVec3::Z, [20.0, 2.5, 1.5]);
        let fit = doc.joint_fit(rail, stile).unwrap();
        assert_eq!(fit.depth, 3.0);
        doc.mortise_and_tenon(rail, stile, Tenon { length: fit.depth, thickness: 0.5, shoulder: 0.25, haunch: 0.0 }).unwrap();
        assert!(closed(&doc, rail) && closed(&doc, stile));
        assert_eq!(doc.part_size(rail).unwrap()[0], 23.0);
    }
}

#[cfg(test)]
mod removing {
    use glam::DVec3;

    use super::*;
    use crate::board::Board;

    fn board(doc: &mut Document, origin: DVec3, length_dir: DVec3, width_dir: DVec3, [l, w, t]: [f64; 3]) -> InstanceId {
        doc.add_board(&Board { origin, length_dir, width_dir, length: l, width: w, thickness: t }, None).unwrap()
    }

    fn faces(doc: &Document, part: InstanceId) -> usize {
        let def = doc.container().instance(part).unwrap().definition;
        doc.scene().definition(def).unwrap().container.model.faces().count()
    }

    #[test]
    fn removing_a_joint_restores_both_parts_and_keeps_the_others() {
        let mut doc = Document::default();
        let side = board(&mut doc, DVec3::ZERO, DVec3::Z, DVec3::Y, [30.0, 11.25, 0.75]);
        let low = board(&mut doc, DVec3::new(0.0, 0.0, 4.0), DVec3::X, DVec3::Y, [20.0, 11.25, 0.75]);
        let high = board(&mut doc, DVec3::new(0.0, 0.0, 20.0), DVec3::X, DVec3::Y, [20.0, 11.25, 0.75]);
        doc.let_in(low, side, 0.25).unwrap();
        doc.let_in(high, side, 0.25).unwrap();
        assert_eq!(faces(&doc, side), 14);
        let (index, joint, other) = doc.joint_list(low)[0].clone();
        assert_eq!((joint.kind, other.as_str()), (JointKind::Dado, "Group#1"));
        doc.remove_joint(index).unwrap();
        assert_eq!(faces(&doc, side), 10, "one dado left");
        assert_eq!(doc.part_size(low).unwrap()[0], 20.0, "the shelf is back to its length");
        assert_eq!(doc.part_size(high).unwrap()[0], 20.25, "the other shelf still reaches into its dado");
        assert_eq!(doc.scene().joints.len(), 1);
        doc.undo();
        assert_eq!(faces(&doc, side), 14, "one undo step");
    }

    #[test]
    fn a_part_changed_by_hand_since_says_to_use_undo() {
        let mut doc = Document::default();
        let side = board(&mut doc, DVec3::ZERO, DVec3::Z, DVec3::Y, [30.0, 11.25, 0.75]);
        let shelf = board(&mut doc, DVec3::new(0.0, 0.0, 12.0), DVec3::X, DVec3::Y, [20.0, 11.25, 0.75]);
        doc.let_in(shelf, side, 0.25).unwrap();
        // Lengthen the side by hand.
        doc.enter(side);
        let top = doc.model().faces().find(|(_, f)| f.normal.x > 0.999).map(|(id, _)| id).unwrap();
        doc.push_pull(top, 2.0, false).unwrap();
        doc.exit();
        assert!(matches!(doc.remove_joint(0), Err(JoineryError::Size(m)) if m.contains("Undo")));
    }

    #[test]
    fn pocket_screws_come_out_cleanly() {
        let mut doc = Document::default();
        let side = board(&mut doc, DVec3::ZERO, DVec3::Z, DVec3::Y, [30.0, 11.25, 0.75]);
        let shelf = board(&mut doc, DVec3::new(0.0, 0.0, 12.0), DVec3::X, DVec3::Y, [20.0, 11.25, 0.75]);
        doc.pocket_screws(shelf, side, 3, 1.25).unwrap();
        doc.remove_joint(0).unwrap();
        assert!(doc.scene().joints.is_empty());
    }
}

/// Dovetail sizes: how many tails, and whether the pins board hides them (half-blind, as on a
/// drawer front) or they show through.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Dovetails {
    pub tails: u32,
    pub half_blind: bool,
}

/// Dovetail slope: 1 in 8, usual for hardwood.
const DOVETAIL_SLOPE: f64 = 1.0 / 8.0;
/// Pins are narrow where they show: 1/4" (half pins at the edges 3/8").
const DOVETAIL_PIN: f64 = 0.25;
const DOVETAIL_HALF_PIN: f64 = 0.375;

impl Dovetails {
    /// About one tail per 2 1/2" of width.
    pub fn suggested(wide: f64) -> u32 {
        ((wide / 2.5).round() as u32).max(1)
    }
}

impl Spline {
    /// A spline for stock `thick` thick: 1/4" thick (1/8" in thin stock), 3/8" into each part.
    pub fn for_stock(thick: f64) -> Slots {
        let t = if thick >= 0.6 { 0.25 } else { 0.125 };
        Slots { kind: JointKind::Splines, piece: format!("{} spline", format_length(t)), count: 1, length: 0.0, thickness: t, depth: 0.375 }
    }
}

/// Splines: one slot along the whole joint, a thin strip glued into both parts.
pub struct Spline;

impl Document {
    /// Dovetails at a corner: `tails` (a drawer side) butts the inside of `pins` (the front) at
    /// its end. The tails board grows into the pins board (all the way through, or short of its
    /// face for half-blind) and both are cut: sockets in the pins board, waste between the tails.
    pub fn dovetails(&mut self, tails: InstanceId, pins: InstanceId, style: Dovetails) -> Result<(), JoineryError> {
        if style.tails == 0 {
            return Err(JoineryError::Size("Dovetails need at least one tail."));
        }
        self.joint_edit(|scene, path| {
            let p = Parts { scene, path };
            let Some(butt) = p.butt(tails, pins)? else {
                return repeat(scene, path, tails, pins, |k| k == JointKind::Dovetails).map(|_| ());
            };
            if !p.flush_at_end(tails, pins, &butt)? {
                return Err(JoineryError::Size(
                    "Butt the tail board's end against the inside of the other board at its end (a drawer side against its front).",
                ));
            }
            let o = p.part_box(pins, pins)?;
            let k = butt.k;
            let (u, w) = across(butt);
            let pins_thick = o.1[k] - o.0[k];
            let lap = if style.half_blind { (pins_thick / 3.0).clamp(0.125, 0.25) } else { 0.0 };
            let depth = pins_thick - lap;
            // Which of the tail board's faces is flush with the pins board's end.
            let (u_end, out) = if (butt.contact.1[u] - o.1[u]).abs() < TOUCH {
                (butt.contact.1[u], 1.0)
            } else if (butt.contact.0[u] - o.0[u]).abs() < TOUCH {
                (butt.contact.0[u], -1.0)
            } else {
                return Err(JoineryError::NotCorner);
            };
            let tail_thick = butt.contact.1[u] - butt.contact.0[u];
            let (lo_w, hi_w) = (butt.contact.0[w], butt.contact.1[w]);
            let width = hi_w - lo_w;
            let n = style.tails;
            let off = depth * DOVETAIL_SLOPE;
            let tail = (width - 2.0 * DOVETAIL_HALF_PIN - f64::from(n - 1) * DOVETAIL_PIN) / f64::from(n);
            if tail < 2.0 * off + 0.25 {
                return Err(JoineryError::Size("Too many tails for the width: use fewer."));
            }
            let v_in = butt.contact.0[k];
            let v_out = v_in + butt.side * depth;
            let point = |wv: f64, vv: f64| {
                let mut p = DVec3::ZERO;
                p[k] = vv;
                p[w] = wv;
                p[u] = u_end;
                p
            };
            // Each tail at the board's end (wide) and at the baseline (narrower by the slope).
            let tails_at: Vec<(f64, f64)> = (0..n).map(|i| {
                let x0 = lo_w + DOVETAIL_HALF_PIN + f64::from(i) * (tail + DOVETAIL_PIN);
                (x0, x0 + tail)
            })
            .collect();
            grow_into(scene, path, tails, pins, butt, extrude(butt, depth))?;
            let normal = DVec3::AXES[u] * out;
            // Sockets in the pins board, from its end, as deep as the tail board is thick.
            for &(x0, x1) in &tails_at {
                let socket = [point(x0 + off, v_in), point(x1 - off, v_in), point(x1, v_out), point(x0, v_out)];
                push_polygon_in(scene, path, pins, pins, &socket, normal, tail_thick)?;
            }
            // The waste between the tails (the pins' places), through the tail board.
            let mut waste = vec![[point(lo_w, v_in), point(tails_at[0].0 + off, v_in), point(tails_at[0].0, v_out), point(lo_w, v_out)]];
            for pair in tails_at.windows(2) {
                let (a, b) = (pair[0].1, pair[1].0);
                waste.push([point(a - off, v_in), point(b + off, v_in), point(b, v_out), point(a, v_out)]);
            }
            let last = tails_at[tails_at.len() - 1].1;
            waste.push([point(last - off, v_in), point(hi_w, v_in), point(hi_w, v_out), point(last, v_out)]);
            for polygon in &waste {
                push_polygon_in(scene, path, tails, pins, polygon, normal, tail_thick)?;
            }
            let detail = format!("{n} tails, {}", if style.half_blind { "half-blind" } else { "through" });
            record(scene, path, JointKind::Dovetails, tails, pins, detail)
        })
    }
}

#[cfg(test)]
mod dovetail_tests {
    use glam::DVec3;

    use super::*;
    use crate::board::Board;

    fn board(doc: &mut Document, origin: DVec3, length_dir: DVec3, width_dir: DVec3, [l, w, t]: [f64; 3]) -> InstanceId {
        doc.add_board(&Board { origin, length_dir, width_dir, length: l, width: w, thickness: t }, None).unwrap()
    }

    fn closed(doc: &Document, part: InstanceId) -> bool {
        let def = doc.container().instance(part).unwrap().definition;
        let m = &doc.scene().definition(def).unwrap().container.model;
        m.edges().all(|(id, _)| m.edge_faces(id).len() == 2)
    }

    /// A drawer front along X (y in [-3/4, 0]) and a side butting its back at its left end.
    fn drawer_corner(doc: &mut Document) -> (InstanceId, InstanceId) {
        let front = board(doc, DVec3::ZERO, DVec3::X, DVec3::Z, [16.0, 6.0, 0.75]);
        let side = board(doc, DVec3::new(0.0, -0.75, 6.0), -DVec3::Y, -DVec3::Z, [14.0, 6.0, 0.5]);
        (front, side)
    }

    #[test]
    fn through_dovetails_cut_tails_and_pins() {
        let mut doc = Document::default();
        let (front, side) = drawer_corner(&mut doc);
        let tails = Dovetails::suggested(6.0);
        assert_eq!(tails, 2);
        doc.dovetails(side, front, Dovetails { tails: 3, half_blind: false }).unwrap();
        assert!(closed(&doc, side) && closed(&doc, front), "both stay closed solids");
        assert_eq!(doc.part_size(side).unwrap()[0], 14.75, "the side reaches through the front");
        assert_eq!(doc.joint_list(front)[0].1.detail, "3 tails, through");
        // And it comes out again.
        doc.remove_joint(0).unwrap();
        assert_eq!(doc.part_size(side).unwrap()[0], 14.0);
    }

    #[test]
    fn half_blind_dovetails_stop_short_of_the_front_face() {
        let mut doc = Document::default();
        let (front, side) = drawer_corner(&mut doc);
        doc.dovetails(side, front, Dovetails { tails: 2, half_blind: true }).unwrap();
        assert!(closed(&doc, side) && closed(&doc, front));
        assert_eq!(doc.part_size(side).unwrap()[0], 14.5, "into the front, short of its face by 1/4\"");
    }

    #[test]
    fn a_spline_runs_the_whole_joint() {
        let mut doc = Document::default();
        let side = board(&mut doc, DVec3::ZERO, DVec3::Z, DVec3::Y, [30.0, 11.25, 0.75]);
        let shelf = board(&mut doc, DVec3::new(0.0, 0.0, 12.0), DVec3::X, DVec3::Y, [20.0, 11.25, 0.75]);
        doc.slots(shelf, side, &Spline::for_stock(0.75)).unwrap();
        assert!(closed(&doc, side) && closed(&doc, shelf));
        let joint = &doc.joint_list(side)[0].1;
        assert_eq!(joint.kind, JointKind::Splines);
        assert_eq!(joint.hardware.as_ref().map(|h| h.name.as_str()), Some("1/4\" x 3/4\" x 11 1/4\" spline"));
    }

    #[test]
    fn a_haunched_tenon_fills_the_groove_at_the_top() {
        let mut doc = Document::default();
        let stile = board(&mut doc, DVec3::ZERO, DVec3::Z, DVec3::X, [30.0, 3.0, 1.5]);
        // A rail at the top of the stile.
        let rail = board(&mut doc, DVec3::new(3.0, 0.0, 30.0), DVec3::X, -DVec3::Z, [20.0, 2.5, 1.5]);
        doc.mortise_and_tenon(rail, stile, Tenon { length: 1.0, thickness: 0.5, shoulder: 0.5, haunch: 0.375 }).unwrap();
        assert!(closed(&doc, rail) && closed(&doc, stile));
        assert!(doc.joint_list(rail)[0].1.detail.contains("haunch"));
    }
}
