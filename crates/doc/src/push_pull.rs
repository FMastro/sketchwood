//! Push/Pull: extrude a face along its normal, SketchUp style.
//!
//! - A free-standing face becomes a closed solid; the original face stays as the base, flipped
//!   to face outward.
//! - A face capping a prism (every neighbor is perpendicular to it, like the end of a board)
//!   moves instead, lengthening or shortening the solid without leaving a seam.
//! - A face set into a surface (e.g. a rectangle drawn on a box's top) extrudes a raised block
//!   when pulled out, or a pocket when pushed in; the original face is removed, leaving the
//!   surrounding face's hole open to the new walls.

use std::collections::BTreeSet;
use std::fmt;

use glam::DVec3;

use crate::model::newell_normal;
use crate::{EPS, FaceId, Model, VertexId};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PushPullError {
    ZeroDistance,
    MissingFace,
    /// Moving the cap would collapse or invert the solid.
    ThroughGeometry,
}

impl fmt::Display for PushPullError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::ZeroDistance => "Distance must not be zero",
            Self::MissingFace => "That face no longer exists",
            Self::ThroughGeometry => "Can't push past the opposite side",
        })
    }
}

impl std::error::Error for PushPullError {}

impl Model {
    /// Push or pull `face` by `distance` inches along its normal (negative pushes back).
    pub fn push_pull(&mut self, face: FaceId, distance: f64) -> Result<(), PushPullError> {
        if distance.abs() < EPS {
            return Err(PushPullError::ZeroDistance);
        }
        let Some(original) = self.face(face).cloned() else { return Err(PushPullError::MissingFace) };
        let n = original.normal;

        // Faces sharing an edge with this one.
        let neighbors: BTreeSet<FaceId> = self
            .face_edges(face)
            .into_iter()
            .flat_map(|edge| self.edge_faces(edge))
            .filter(|&other| other != face)
            .collect();
        let edge_count = self.face_edges(face).len();
        let every_edge_shared = self.face_edges(face).into_iter().all(|e| self.edge_faces(e).len() > 1);
        let is_cap = edge_count > 0
            && every_edge_shared
            && neighbors.iter().all(|&f| self.face(f).is_some_and(|f| f.normal.dot(n).abs() < 1e-9));

        if is_cap {
            self.move_cap(face, &neighbors, distance)
        } else {
            self.extrude(face, distance, !neighbors.is_empty());
            Ok(())
        }
    }

    fn move_cap(&mut self, face: FaceId, sides: &BTreeSet<FaceId>, distance: f64) -> Result<(), PushPullError> {
        let cap = self.face(face).expect("checked by caller").clone();
        let moving: BTreeSet<VertexId> = cap.loops().flatten().copied().collect();
        let origin = self.vertex(cap.outer[0]);
        // The cap may not reach or pass any fixed vertex of its side faces.
        let blocked = sides.iter().filter_map(|&f| self.face(f)).flat_map(|f| f.loops().flatten().copied()).any(|v| {
            let height = (self.vertex(v) - origin).dot(cap.normal);
            !moving.contains(&v)
                && height.abs() > EPS
                && height.signum() == distance.signum()
                && height.abs() <= distance.abs() + EPS
        });
        if blocked {
            return Err(PushPullError::ThroughGeometry);
        }
        self.translate_vertices(moving, cap.normal * distance);
        Ok(())
    }

    fn extrude(&mut self, face: FaceId, distance: f64, attached: bool) {
        let original = self.face(face).expect("checked by caller").clone();
        let n = original.normal;
        let offset = n * distance;
        // Pushing an attached face back cuts a pocket: the new faces look into the pocket.
        let removing = attached && distance < 0.0;
        let loops: Vec<Vec<DVec3>> = original.loops().map(|l| self.loop_points(l)).collect();

        for (i, points) in loops.iter().enumerate() {
            // Outward (away from the solid) is to the right of a loop winding counter-clockwise
            // around `n` for the outer loop, and to the left for holes.
            let winding = newell_normal(points).dot(n).signum();
            let side = if i == 0 { 1.0 } else { -1.0 };
            for (j, &a) in points.iter().enumerate() {
                let b = points[(j + 1) % points.len()];
                let mut outward = (b - a).cross(n) * winding * side;
                if removing {
                    outward = -outward;
                }
                let mut quad = [a, b, b + offset, a + offset];
                if newell_normal(&quad).dot(outward) < 0.0 {
                    quad.reverse();
                }
                self.add_face(&quad);
            }
        }

        let cap_normal = if attached { n } else { n * distance.signum() };
        let orient = |mut points: Vec<DVec3>, toward: DVec3| {
            if newell_normal(&points).dot(toward) < 0.0 {
                points.reverse();
            }
            points
        };
        let cap_outer = orient(loops[0].iter().map(|&p| p + offset).collect(), cap_normal);
        let cap_holes: Vec<Vec<DVec3>> =
            loops[1..].iter().map(|h| orient(h.iter().map(|&p| p + offset).collect(), -cap_normal)).collect();
        // The moved end keeps the original face's paint; new walls start unpainted (SketchUp).
        if let Some(cap) = self.add_face_with_holes(&cap_outer, &cap_holes) {
            self.set_face_material(cap, original.material);
        }

        if attached {
            // The base is now inside the solid (or open to the pocket).
            self.remove_face(face);
        } else if distance > 0.0 {
            // The base becomes the solid's bottom, which must face away from the solid.
            self.reverse_face(face);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Entity;

    /// Signed volume enclosed by all faces; positive when every normal points outward.
    fn volume(model: &Model) -> f64 {
        model
            .faces()
            .flat_map(|(_, face)| {
                model.triangulate(face).into_iter().map(move |[a, b, c]| {
                    let (b, c) = if (b - a).cross(c - a).dot(face.normal) < 0.0 { (c, b) } else { (b, c) };
                    a.dot(b.cross(c)) / 6.0
                })
            })
            .sum()
    }

    fn two_by_four_profile(model: &mut Model) -> FaceId {
        model.add_rectangle(DVec3::ZERO, DVec3::X * 1.5, DVec3::Y * 3.5, DVec3::Z).unwrap()
    }

    fn top_face(model: &Model) -> FaceId {
        model
            .faces()
            .max_by(|(_, a), (_, b)| {
                let za = model.vertex(a.outer[0]).z * a.normal.z;
                let zb = model.vertex(b.outer[0]).z * b.normal.z;
                za.total_cmp(&zb)
            })
            .map(|(id, _)| id)
            .unwrap()
    }

    fn height(model: &Model) -> f64 {
        let (lo, hi) = model.bounds().unwrap();
        hi.z - lo.z
    }

    #[test]
    fn free_profile_pulls_into_a_closed_board() {
        let mut model = Model::default();
        let face = two_by_four_profile(&mut model);
        model.push_pull(face, 96.0).unwrap();
        assert_eq!(model.faces().count(), 6);
        assert_eq!(model.edges().count(), 12);
        assert_eq!(model.vertices().count(), 8);
        assert!((volume(&model) - 1.5 * 3.5 * 96.0).abs() < 1e-6, "{}", volume(&model));
        // The base now faces down.
        assert!(model.face(face).unwrap().normal.abs_diff_eq(-DVec3::Z, 1e-12));
    }

    #[test]
    fn negative_distance_extrudes_below() {
        let mut model = Model::default();
        let face = two_by_four_profile(&mut model);
        model.push_pull(face, -10.0).unwrap();
        let (lo, hi) = model.bounds().unwrap();
        assert_eq!((lo.z, hi.z), (-10.0, 0.0));
        assert!((volume(&model) - 1.5 * 3.5 * 10.0).abs() < 1e-6);
    }

    #[test]
    fn pulling_a_boards_end_lengthens_it_without_a_seam() {
        let mut model = Model::default();
        let face = two_by_four_profile(&mut model);
        model.push_pull(face, 96.0).unwrap();
        model.push_pull(top_face(&model), 24.0).unwrap();
        assert_eq!(model.faces().count(), 6);
        assert_eq!(model.edges().count(), 12);
        assert_eq!(height(&model), 120.0);
        assert!((volume(&model) - 1.5 * 3.5 * 120.0).abs() < 1e-6);
    }

    #[test]
    fn pushing_a_boards_end_shortens_it_but_not_through() {
        let mut model = Model::default();
        let face = two_by_four_profile(&mut model);
        model.push_pull(face, 96.0).unwrap();
        model.push_pull(top_face(&model), -50.0).unwrap();
        assert_eq!(height(&model), 46.0);
        assert_eq!(model.push_pull(top_face(&model), -46.0), Err(PushPullError::ThroughGeometry));
        assert_eq!(model.push_pull(top_face(&model), -100.0), Err(PushPullError::ThroughGeometry));
        assert_eq!(height(&model), 46.0);
    }

    #[test]
    fn rectangle_on_a_top_pulls_up_a_block() {
        let mut model = Model::default();
        let base = model.add_rectangle(DVec3::ZERO, DVec3::X * 10.0, DVec3::Y * 10.0, DVec3::Z).unwrap();
        model.push_pull(base, 2.0).unwrap();
        let top = top_face(&model);
        let inner = model
            .add_rectangle(DVec3::new(3.0, 3.0, 2.0), DVec3::X * 4.0, DVec3::Y * 4.0, DVec3::Z)
            .unwrap();
        assert_eq!(model.face(top).unwrap().holes.len(), 1);
        model.push_pull(inner, 1.0).unwrap();
        // Box (6) + block walls (4) + block top (1); the drawn face was absorbed.
        assert_eq!(model.faces().count(), 11);
        assert!(model.face(inner).is_none());
        assert!((volume(&model) - (200.0 + 16.0)).abs() < 1e-6, "{}", volume(&model));
    }

    #[test]
    fn rectangle_on_a_top_pushes_down_a_pocket() {
        let mut model = Model::default();
        let base = model.add_rectangle(DVec3::ZERO, DVec3::X * 10.0, DVec3::Y * 10.0, DVec3::Z).unwrap();
        model.push_pull(base, 2.0).unwrap();
        let inner = model
            .add_rectangle(DVec3::new(3.0, 3.0, 2.0), DVec3::X * 4.0, DVec3::Y * 4.0, DVec3::Z)
            .unwrap();
        model.push_pull(inner, -1.0).unwrap();
        assert_eq!(model.faces().count(), 11);
        assert!((volume(&model) - (200.0 - 16.0)).abs() < 1e-6, "{}", volume(&model));
        // The pocket floor faces up, out of the pocket.
        let floor = model
            .faces()
            .find(|(_, f)| (model.vertex(f.outer[0]).z - 1.0).abs() < 1e-9)
            .map(|(_, f)| f.normal)
            .unwrap();
        assert!(floor.abs_diff_eq(DVec3::Z, 1e-12));
    }

    #[test]
    fn face_with_a_hole_extrudes_a_frame() {
        let mut model = Model::default();
        let outer = model.add_rectangle(DVec3::ZERO, DVec3::X * 10.0, DVec3::Y * 10.0, DVec3::Z).unwrap();
        let inner = model
            .add_rectangle(DVec3::new(4.0, 4.0, 0.0), DVec3::X * 2.0, DVec3::Y * 2.0, DVec3::Z)
            .unwrap();
        model.erase([Entity::Face(inner)]);
        model.push_pull(outer, 3.0).unwrap();
        // Base + cap + 4 outer walls + 4 hole walls.
        assert_eq!(model.faces().count(), 10);
        assert!((volume(&model) - 3.0 * 96.0).abs() < 1e-6, "{}", volume(&model));
    }

    #[test]
    fn zero_distance_is_rejected() {
        let mut model = Model::default();
        let face = two_by_four_profile(&mut model);
        assert_eq!(model.push_pull(face, 0.0), Err(PushPullError::ZeroDistance));
    }
}
