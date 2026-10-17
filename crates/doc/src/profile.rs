//! Edge profiles, the way a router bit shapes a board's edges: roundover, chamfer, cove and
//! ogee, a given size.
//!
//! Two common cases, on a plain board (a box, before other cuts):
//! - **Around a face**: some or all of the edges around one face (a tabletop's top edges); where
//!   two profiled edges meet, the corner is mitered, as a router leaves it.
//! - **Along the length**: some or all of the four edges running along one axis (a leg's
//!   corners).
//!
//! The board is rebuilt from its outline with the profile in place, so it stays one clean
//! solid, and its cut-list size doesn't change.

use std::fmt;

use glam::DVec3;
use serde::{Deserialize, Serialize};

use crate::units::format_length;
use crate::{DefinitionId, Document, EPS, InstanceId, Model};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Profile {
    Roundover,
    Chamfer,
    Cove,
    Ogee,
}

impl Profile {
    pub const ALL: [Self; 4] = [Self::Roundover, Self::Chamfer, Self::Cove, Self::Ogee];

    pub fn name(self) -> &'static str {
        match self {
            Self::Roundover => "Roundover",
            Self::Chamfer => "Chamfer",
            Self::Cove => "Cove",
            Self::Ogee => "Ogee",
        }
    }

    /// The profile's curve across a corner of size `r`: points `(u, v)` from `(r, 0)` (on the
    /// incoming face, `r` back from the corner) to `(0, r)` (on the outgoing face), where `u`
    /// is the distance back along the incoming side and `v` along the outgoing side.
    fn curve(self, r: f64) -> Vec<(f64, f64)> {
        const STEPS: u32 = 8;
        let t = |i: u32| f64::from(i) / f64::from(STEPS);
        let quarter = std::f64::consts::FRAC_PI_2;
        match self {
            Self::Chamfer => vec![(r, 0.0), (0.0, r)],
            // Convex: a quarter circle around a center `r` in from both faces.
            Self::Roundover => (0..=STEPS).map(|i| {
                let a = t(i) * quarter;
                (r * (1.0 - a.sin()), r * (1.0 - a.cos()))
            })
            .collect(),
            // Concave: a quarter circle around the corner itself.
            Self::Cove => (0..=STEPS).map(|i| {
                let a = t(i) * quarter;
                (r * a.cos(), r * a.sin())
            })
            .collect(),
            // An S: concave near one face, convex near the other.
            Self::Ogee => (0..=STEPS * 2)
                .map(|i| {
                    let s = f64::from(i) / f64::from(STEPS * 2);
                    (r * (1.0 - s), r * (1.0 - (std::f64::consts::PI * s).cos()) / 2.0)
                })
                .collect(),
        }
    }
}

/// Which edges of a board to profile, in its own axes (0, 1, 2 = its red, green, blue).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Edges {
    /// Around the face on `axis` at its high end (`positive`) or low end; `sides` are the face's
    /// edges by the neighboring face: `[u-, u+, v-, v+]`, where `u, v` are the next axes.
    AroundFace { axis: usize, positive: bool, sides: [bool; 4] },
    /// The four edges running along `axis`, by corner of the cross-section:
    /// `[(u-, v-), (u+, v-), (u+, v+), (u-, v+)]`.
    Along { axis: usize, corners: [bool; 4] },
}

/// An edge profile made on a part, for listing.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProfileRecord {
    pub part: DefinitionId,
    pub profile: Profile,
    pub size: f64,
    pub edges: Edges,
}

impl ProfileRecord {
    pub fn describe(&self) -> String {
        let count = match self.edges {
            Edges::AroundFace { sides, .. } => sides.iter().filter(|s| **s).count(),
            Edges::Along { corners, .. } => corners.iter().filter(|s| **s).count(),
        };
        format!("{} {}, {count} edge{}", format_length(self.size), self.profile.name(), if count == 1 { "" } else { "s" })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum ProfileError {
    NotAPart,
    NotABoard,
    NoEdges,
    TooBig,
}

impl fmt::Display for ProfileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NotAPart => "Pick one group or component.",
            Self::NotABoard => "Edge profiles work on plain boards for now: profile before cutting joints or other shapes.",
            Self::NoEdges => "Pick at least one edge.",
            Self::TooBig => "That profile is too big for the board.",
        })
    }
}

impl std::error::Error for ProfileError {}

/// A plain board: six faces, all square to its axes. Its box.
fn board_box(model: &Model) -> Option<(DVec3, DVec3)> {
    let square = model.faces().count() == 6 && model.faces().all(|(_, f)| f.normal.abs().max_element() > 1.0 - 1e-9);
    if !square || model.vertices().count() != 8 {
        return None;
    }
    model.bounds()
}

/// Corner `i` of the box's cross-section across `axis`: `[(u-, v-), (u+, v-), (u+, v+), (u-, v+)]`.
fn corner(lo: DVec3, hi: DVec3, axis: usize, i: usize, at: f64) -> DVec3 {
    let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
    let mut p = DVec3::ZERO;
    p[axis] = at;
    p[u] = if i == 1 || i == 2 { hi[u] } else { lo[u] };
    p[v] = if i >= 2 { hi[v] } else { lo[v] };
    p
}

/// The cross-section across `axis` (at `at`) going around counterclockwise about +axis, each
/// chosen corner replaced by the profile's curve.
fn section(lo: DVec3, hi: DVec3, axis: usize, at: f64, corners: [bool; 4], curve: &[(f64, f64)]) -> Vec<DVec3> {
    let mut points = Vec::new();
    for (i, &chosen) in corners.iter().enumerate() {
        let p = corner(lo, hi, axis, i, at);
        if !chosen {
            points.push(p);
            continue;
        }
        let prev = corner(lo, hi, axis, (i + 3) % 4, at);
        let next = corner(lo, hi, axis, (i + 1) % 4, at);
        let (d_in, d_out) = ((p - prev).normalize(), (next - p).normalize());
        points.extend(curve.iter().map(|&(a, b)| p - d_in * a + d_out * b));
    }
    points
}

/// Drop repeated points (consecutive, and the last matching the first).
fn dedupe(mut points: Vec<DVec3>) -> Vec<DVec3> {
    points.dedup_by(|a, b| a.distance(*b) < EPS);
    while points.len() > 1 && points[0].distance(points[points.len() - 1]) < EPS {
        points.pop();
    }
    points
}

/// Build a closed solid from its faces, each turned to face away from `center`.
fn solid(faces: Vec<Vec<DVec3>>, center: DVec3) -> Model {
    let mut model = Model::default();
    for face in faces {
        let mut face = dedupe(face);
        if face.len() < 3 {
            continue;
        }
        let normal = crate::model::newell_normal(&face);
        let middle = face.iter().copied().sum::<DVec3>() / face.len() as f64;
        if normal.dot(middle - center) < 0.0 {
            face.reverse();
        }
        model.add_face(&face);
    }
    model
}

/// The board from `lo` to `hi` with `edges` shaped by `curve` (size `r`).
fn profiled(lo: DVec3, hi: DVec3, edges: Edges, curve: &[(f64, f64)], r: f64) -> Result<Model, ProfileError> {
    let size = hi - lo;
    let center = (lo + hi) / 2.0;
    match edges {
        Edges::Along { axis, corners } => {
            if !corners.contains(&true) {
                return Err(ProfileError::NoEdges);
            }
            let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
            // Two profiled corners on one side need room for both.
            if 2.0 * r >= size[u].min(size[v]) {
                return Err(ProfileError::TooBig);
            }
            let bottom = section(lo, hi, axis, lo[axis], corners, curve);
            let top: Vec<DVec3> = bottom.iter().map(|p| *p + DVec3::AXES[axis] * size[axis]).collect();
            let n = bottom.len();
            let mut faces = vec![bottom.clone(), top.clone()];
            for i in 0..n {
                let j = (i + 1) % n;
                faces.push(vec![bottom[i], bottom[j], top[j], top[i]]);
            }
            Ok(solid(faces, center))
        }
        Edges::AroundFace { axis, positive, sides } => {
            if !sides.contains(&true) {
                return Err(ProfileError::NoEdges);
            }
            let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
            let both = |a: bool, b: bool| f64::from(u8::from(a)) + f64::from(u8::from(b));
            if r >= size[axis] || both(sides[0], sides[1]) * r >= size[u] - EPS || both(sides[2], sides[3]) * r >= size[v] - EPS {
                return Err(ProfileError::TooBig);
            }
            let dir = if positive { 1.0 } else { -1.0 };
            let face_at = if positive { hi[axis] } else { lo[axis] };
            let far_at = if positive { lo[axis] } else { hi[axis] };
            // Rings from the face down through the profile: (depth below the face, inset).
            // The curve runs from (depth r, inset 0) to (depth 0, inset r): walk it backwards.
            let mut steps: Vec<(f64, f64)> = curve.iter().rev().copied().collect();
            steps.dedup_by(|a, b| (a.0 - b.0).abs() < EPS && (a.1 - b.1).abs() < EPS);
            let ring = |depth: f64, inset: f64| -> [DVec3; 4] {
                let mut a = lo;
                let mut b = hi;
                if sides[0] {
                    a[u] += inset;
                }
                if sides[1] {
                    b[u] -= inset;
                }
                if sides[2] {
                    a[v] += inset;
                }
                if sides[3] {
                    b[v] -= inset;
                }
                let at = face_at - dir * depth;
                std::array::from_fn(|i| corner(a, b, axis, i, at))
            };
            let rings: Vec<[DVec3; 4]> = steps.iter().map(|&(depth, inset)| ring(depth, inset)).collect();
            let bottom: [DVec3; 4] = std::array::from_fn(|i| corner(lo, hi, axis, i, far_at));
            let mut faces = vec![rings[0].to_vec(), bottom.to_vec()];
            // Each side of the face: corners (i, i+1) in ring order; which `sides` entry it is.
            // Ring corners: 0 (u-, v-), 1 (u+, v-), 2 (u+, v+), 3 (u-, v+).
            let side_of = [2, 1, 3, 0]; // edge 0-1 is v-, 1-2 is u+, 2-3 is v+, 3-0 is u-
            let last = rings.len() - 1;
            for (e, &side) in side_of.iter().enumerate() {
                let (i, j) = (e, (e + 1) % 4);
                if sides[side] {
                    // Profiled: a strip between each pair of rings, then flat down to the far face.
                    for k in 0..last {
                        faces.push(vec![rings[k][i], rings[k][j], rings[k + 1][j], rings[k + 1][i]]);
                    }
                    faces.push(vec![rings[last][i], rings[last][j], bottom[j], bottom[i]]);
                } else {
                    // Flat: one face whose corners follow the profiled neighbors.
                    let mut points: Vec<DVec3> = rings.iter().map(|r| r[j]).collect();
                    points.push(bottom[j]);
                    points.push(bottom[i]);
                    points.extend(rings.iter().rev().map(|r| r[i]));
                    faces.push(points);
                }
            }
            Ok(solid(faces, center))
        }
    }
}

impl Document {
    /// Shape `edges` of a board (one group or component in the container being edited) with
    /// `profile` at `size`, as one undo step. Components change in every copy.
    pub fn profile_edges(&mut self, part: InstanceId, profile: Profile, size: f64, edges: Edges) -> Result<(), ProfileError> {
        if size <= EPS {
            return Err(ProfileError::TooBig);
        }
        let path = self.path().to_vec();
        let mut scene = self.scene().clone();
        scene.make_unique_if_group(&path, part);
        let def = scene.container(&path).and_then(|c| c.instance(part)).ok_or(ProfileError::NotAPart)?.definition;
        let definition = scene.definition_mut(def).ok_or(ProfileError::NotAPart)?;
        let (lo, hi) = board_box(&definition.container.model).ok_or(ProfileError::NotABoard)?;
        let mut model = profiled(lo, hi, edges, &profile.curve(size), size)?;
        // Keep the board's painted faces' look: faces take the part's paint anyway.
        let material = definition.container.model.faces().find_map(|(_, f)| f.material);
        if let Some(material) = material {
            let ids: Vec<_> = model.faces().map(|(id, _)| id).collect();
            for id in ids {
                model.set_face_material(id, Some(material));
            }
        }
        definition.container.model = model;
        scene.profiles.push(ProfileRecord { part: def, profile, size, edges });
        self.edit_scene(|s, _| *s = scene);
        Ok(())
    }

    /// The edge profiles on a part.
    pub fn profiles_of(&self, part: InstanceId) -> Vec<ProfileRecord> {
        let Some(def) = self.container().instance(part).map(|i| i.definition) else { return Vec::new() };
        self.scene().profiles.iter().filter(|p| p.part == def).cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::Board;

    fn closed(model: &Model) -> bool {
        model.edges().all(|(id, _)| model.edge_faces(id).len() == 2)
    }

    fn top(doc: &mut Document) -> InstanceId {
        // A 24" x 16" x 3/4" tabletop lying flat: length X, width Y, thickness Z.
        doc.add_board(&Board { origin: DVec3::ZERO, length_dir: DVec3::X, width_dir: DVec3::Y, length: 24.0, width: 16.0, thickness: 0.75 }, None).unwrap()
    }

    fn model(doc: &Document, part: InstanceId) -> &Model {
        let def = doc.container().instance(part).unwrap().definition;
        &doc.scene().definition(def).unwrap().container.model
    }

    #[test]
    fn a_roundover_all_around_a_tabletop_stays_one_closed_board() {
        let mut doc = Document::default();
        let part = top(&mut doc);
        let all = Edges::AroundFace { axis: 2, positive: true, sides: [true; 4] };
        doc.profile_edges(part, Profile::Roundover, 0.25, all).unwrap();
        let m = model(&doc, part);
        assert!(closed(m), "a closed solid");
        let (lo, hi) = m.bounds().unwrap();
        assert!((hi - lo).abs_diff_eq(DVec3::new(24.0, 16.0, 0.75), 1e-9), "same size");
        assert_eq!(doc.part_size(part), Some([24.0, 16.0, 0.75]), "the cut list is unchanged");
        // The top face shrank by the roundover on every side.
        let top_face = m.faces().find(|(_, f)| f.normal.z > 0.999).unwrap().1;
        let ys: Vec<f64> = top_face.outer.iter().map(|&v| m.vertex(v).y).collect();
        assert!(ys.iter().all(|y| (*y - 0.25).abs() < 1e-9 || (*y - 15.75).abs() < 1e-9), "{ys:?}");
        assert_eq!(doc.profiles_of(part)[0].describe(), "1/4\" Roundover, 4 edges");
        doc.undo();
        assert_eq!(model(&doc, part).faces().count(), 6, "one undo step");
    }

    #[test]
    fn each_profile_shape_and_one_edge_work() {
        for profile in Profile::ALL {
            let mut doc = Document::default();
            let part = top(&mut doc);
            // Only the front edge (v- is -Y around +Z: u = X, v = Y).
            let front = Edges::AroundFace { axis: 2, positive: true, sides: [false, false, true, false] };
            doc.profile_edges(part, profile, 0.375, front).unwrap();
            assert!(closed(model(&doc, part)), "{profile:?}");
        }
    }

    #[test]
    fn chamfered_leg_corners_run_its_length() {
        let mut doc = Document::default();
        // A 1 1/2" square leg, 28" tall.
        let leg = doc
            .add_board(&Board { origin: DVec3::ZERO, length_dir: DVec3::Z, width_dir: DVec3::X, length: 28.0, width: 1.5, thickness: 1.5 }, None)
            .unwrap();
        // The leg's own length axis is its X.
        doc.profile_edges(leg, Profile::Chamfer, 0.125, Edges::Along { axis: 0, corners: [true; 4] }).unwrap();
        let m = model(&doc, leg);
        assert!(closed(m));
        assert_eq!(m.faces().count(), 2 + 8, "two octagon ends and eight sides");
    }

    #[test]
    fn too_big_or_on_a_cut_board_is_refused() {
        let mut doc = Document::default();
        let part = top(&mut doc);
        let all = Edges::AroundFace { axis: 2, positive: true, sides: [true; 4] };
        assert_eq!(doc.profile_edges(part, Profile::Roundover, 1.0, all), Err(ProfileError::TooBig));
        doc.profile_edges(part, Profile::Roundover, 0.25, all).unwrap();
        assert_eq!(doc.profile_edges(part, Profile::Chamfer, 0.125, all), Err(ProfileError::NotABoard));
    }
}
