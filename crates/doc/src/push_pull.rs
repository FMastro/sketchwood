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
        // Pushed past the far side of the board: stop there and cut through (SketchUp stops at
        // the face behind too), rather than poking out the back.
        let distance = match self.depth_to_far_side(&original) {
            Some(depth) if attached && distance < -depth => -depth,
            _ => distance,
        };
        let offset = n * distance;
        // Pushing an attached face back cuts a pocket: the new faces look into the pocket.
        let removing = attached && distance < 0.0;
        let loops: Vec<Vec<DVec3>> = original.loops().map(|l| self.loop_points(l)).collect();
        if attached {
            // The base ends up inside the solid (or open to the pocket). Remove it first, so
            // walls flush with a side face can merge with it across the base's edge.
            self.remove_face(face);
        }

        // Pushed exactly to the far side of the board: cut the outline out of the face there, a
        // hole right through, instead of capping the pocket. Done before the walls, so their
        // bottom edges don't already exist when the outline is drawn.
        let through = removing && {
            let outline: Vec<DVec3> = loops[0].iter().map(|&p| p + offset).collect();
            let center = outline.iter().sum::<DVec3>() / outline.len() as f64;
            let far = self.faces().find(|(_, f)| {
                f.normal.dot(n) < -1.0 + 1e-9
                    && (self.vertex(f.outer[0]) - center).dot(n).abs() <= EPS
                    && self.face_contains(f, center)
            });
            far.is_some() && {
                if let Some(opening) = self.draw_polygon(&outline, -n) {
                    self.remove_face(opening);
                }
                true
            }
        };

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
                self.add_wall(quad, a, b, removing);
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
        if !through {
            // The moved end keeps the original face's paint; new walls start unpainted (SketchUp).
            if let Some(cap) = self.add_face_with_holes(&cap_outer, &cap_holes) {
                self.set_face_material(cap, original.material);
            }
        }

        if !attached && distance > 0.0 {
            // The base becomes the solid's bottom, which must face away from the solid.
            self.reverse_face(face);
        }
        if removing {
            // Cutting a rabbet removes part of a side face; the base's edges and wall edges
            // that no longer border anything go with it.
            let stray: Vec<crate::Entity> = loops
                .iter()
                .flat_map(|points| {
                    (0..points.len()).flat_map(move |j| {
                        let (a, b) = (points[j], points[(j + 1) % points.len()]);
                        [(a, b), (a, a + offset), (a + offset, b + offset)]
                    })
                })
                .filter_map(|(a, b)| self.find_edge(self.vertex_at(a)?, self.vertex_at(b)?))
                .filter(|&e| self.edge_faces(e).is_empty())
                .map(crate::Entity::Edge)
                .collect();
            self.erase(stray);
        }
    }


    /// How far behind `face` the board's far side is: the nearest face facing the other way
    /// that lies straight behind the middle of `face`.
    fn depth_to_far_side(&self, face: &crate::Face) -> Option<f64> {
        let n = face.normal;
        let outline = self.loop_points(&face.outer);
        let center = outline.iter().sum::<DVec3>() / outline.len() as f64;
        self.faces()
            .filter(|(_, f)| f.normal.dot(n) < -1.0 + 1e-9)
            .filter_map(|(_, f)| {
                let depth = (center - self.vertex(f.outer[0])).dot(n);
                (depth > EPS && self.face_contains(f, center - n * depth)).then_some(depth)
            })
            .min_by(f64::total_cmp)
    }
    /// Add one side wall of an extrusion, swept from the edge `a`–`b`. Where a face already
    /// lies in the wall's plane on the far side of that edge (a board's side, when pushing or
    /// pulling part of its top flush with it), the wall belongs to that face: a push cuts the
    /// wall's area out of it (a rabbet or notch), a pull extends it.
    fn add_wall(&mut self, quad: [DVec3; 4], a: DVec3, b: DVec3, removing: bool) {
        let normal = newell_normal(&quad).normalize();
        let edge_ab = |m: &Self| m.find_edge(m.vertex_at(a)?, m.vertex_at(b)?);
        let flush = edge_ab(self).and_then(|edge| {
            self.edge_faces(edge).into_iter().find(|&f| {
                self.face(f).is_some_and(|f| {
                    f.normal.dot(normal).abs() > 1.0 - 1e-9 && (self.vertex(f.outer[0]) - a).dot(normal).abs() <= EPS
                })
            })
        });
        let Some(side) = flush else {
            self.add_face(&quad);
            return;
        };
        // The "side" may be the wall itself: neighbouring walls' edges can close its outline, which
        // fills it in. Keep it as the wall, facing the right way.
        let is_wall = self.face(side).is_some_and(|f| {
            f.outer.len() == 4 && f.holes.is_empty() && f.outer.iter().all(|&v| quad.iter().any(|q| q.distance(self.vertex(v)) <= EPS))
        });
        if is_wall {
            if self.face(side).is_some_and(|f| f.normal.dot(normal) < 0.0) {
                self.reverse_face(side);
            }
            return;
        }
        // In the side's plane but beyond it (a deeper pocket below a shallower one): new surface,
        // not part of the side to cut away.
        let center = quad.iter().sum::<DVec3>() / 4.0;
        if removing && self.face(side).is_some_and(|f| !self.face_contains(f, center)) {
            for i in 0..4 {
                self.add_line(quad[i], quad[(i + 1) % 4]);
            }
            let in_plane = |f: &crate::Face| f.normal.dot(normal).abs() > 1.0 - 1e-9 && (self.vertex(f.outer[0]) - center).dot(normal).abs() <= EPS;
            let found = self.faces().find(|(_, f)| in_plane(f) && self.face_contains(f, center)).map(|(id, _)| id);
            let wall = match found {
                Some(id) => Some(id),
                None => self.add_face(&quad),
            };
            if let Some(wall) = wall
                && self.face(wall).is_some_and(|f| f.normal.dot(normal) < 0.0)
            {
                self.reverse_face(wall);
            }
            return;
        }
        let side_normal = self.face(side).expect("found above").normal;
        // Draw the wall's edges onto the side's plane (the one along the base already exists, and
        // the quad may wind either way); that splits the side (push)
        // or closes a new face beside it (pull).
        for i in 0..4 {
            self.add_line(quad[i], quad[(i + 1) % 4]);
        }
        // Its edges may all exist already (a neighbouring wall drew them), in which case no line
        // was new and nothing split; split the side by them explicitly.
        if let Some(f) = self.face(side) {
            let origin = self.vertex(f.outer[0]);
            self.rebuild_plane(side_normal, origin, &[], &BTreeSet::from([side]), None);
        }
        let center = quad.iter().sum::<DVec3>() / 4.0;
        let in_plane = |f: &crate::Face| {
            f.normal.dot(normal).abs() > 1.0 - 1e-9 && (self.vertex(f.outer[0]) - center).dot(normal).abs() <= EPS
        };
        let Some((wall, _)) = self.faces().find(|(_, f)| in_plane(f) && self.face_contains(f, center)) else {
            return;
        };
        if removing {
            self.remove_face(wall);
        } else {
            if self.face(wall).is_some_and(|f| f.normal.dot(side_normal) < 0.0) {
                self.reverse_face(wall);
            }
            if let Some(edge) = edge_ab(self) {
                self.merge_across(edge);
            }
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

    /// A 10 x 4 x 2 board with a line across its top at x = 8; returns the top's end piece.
    fn board_with_split_top(model: &mut Model) -> FaceId {
        let base = model.add_rectangle(DVec3::ZERO, DVec3::X * 10.0, DVec3::Y * 4.0, DVec3::Z).unwrap();
        model.push_pull(base, 2.0).unwrap();
        model.add_line(DVec3::new(8.0, 0.0, 2.0), DVec3::new(8.0, 4.0, 2.0));
        model
            .faces()
            .find(|(_, f)| f.normal.z > 0.5 && model.loop_points(&f.outer).iter().all(|p| p.x >= 8.0 - 1e-9))
            .map(|(id, _)| id)
            .unwrap()
    }

    #[test]
    fn pushing_part_of_a_top_down_cuts_a_rabbet() {
        let mut model = Model::default();
        let end = board_with_split_top(&mut model);
        model.push_pull(end, -1.0).unwrap();
        assert!((volume(&model) - (80.0 - 8.0)).abs() < 1e-6, "{}", volume(&model));
        // Bottom, notched front and back, two ends (one now shorter), the top's rest, the
        // rabbet's shoulder and floor; no holes patched over.
        assert_eq!(model.faces().count(), 8);
        assert!(model.faces().all(|(_, f)| f.holes.is_empty()));
        // An L-shaped prism: two 6-sided ends and 6 edges between them, nothing left over.
        assert_eq!(model.edges().count(), 18);
    }

    #[test]
    fn pulling_part_of_a_top_up_extends_the_sides() {
        let mut model = Model::default();
        let end = board_with_split_top(&mut model);
        model.push_pull(end, 1.0).unwrap();
        assert!((volume(&model) - (80.0 + 8.0)).abs() < 1e-6, "{}", volume(&model));
        // Front, back and end each grew into one face; plus bottom, both tops and the step.
        assert_eq!(model.faces().count(), 8);
        assert!(model.faces().all(|(_, f)| f.holes.is_empty()));
    }

    #[test]
    fn pushing_past_the_far_side_stops_there_and_cuts_through() {
        let mut model = Model::default();
        let base = model.add_rectangle(DVec3::ZERO, DVec3::X * 10.0, DVec3::Y * 10.0, DVec3::Z).unwrap();
        model.push_pull(base, 0.75).unwrap();
        let inner = model.add_rectangle(DVec3::new(4.0, 4.0, 0.75), DVec3::X * 2.0, DVec3::Y * 2.0, DVec3::Z).unwrap();
        // Dragged too far by hand.
        model.push_pull(inner, -1.3).unwrap();
        let (lo, _) = model.bounds().unwrap();
        assert_eq!(lo.z, 0.0, "nothing pokes out of the back");
        let holes: Vec<usize> = model.faces().filter(|(_, f)| f.normal.z.abs() > 0.5).map(|(_, f)| f.holes.len()).collect();
        assert_eq!(holes, vec![1, 1]);
        assert!((volume(&model) - (75.0 - 3.0)).abs() < 1e-6, "{}", volume(&model));
    }

    #[test]
    fn zero_distance_is_rejected() {
        let mut model = Model::default();
        let face = two_by_four_profile(&mut model);
        assert_eq!(model.push_pull(face, 0.0), Err(PushPullError::ZeroDistance));
    }
}

#[cfg(test)]
mod notches_through_an_end {
    use glam::DVec3;

    use crate::Model;

    fn open(m: &Model) -> Vec<[DVec3; 2]> {
        m.edges().filter(|(id, _)| m.edge_faces(*id).len() != 2).map(|(_, e)| m.edge_points(e)).collect()
    }

    fn board() -> Model {
        let mut m = Model::default();
        let f = m.add_rectangle(DVec3::ZERO, DVec3::X * 10.0, DVec3::Y * 6.0, DVec3::Z).unwrap();
        m.push_pull(f, 0.5).unwrap();
        m
    }

    #[test]
    fn a_notch_cut_through_at_the_end_keeps_all_its_walls() {
        // Notches through the board at its end (x 9..10), straight or slanted (dovetail waste),
        // in the middle or at the corner. The last wall's outline gets closed by its neighbours'
        // edges before it's added; it must stay a wall, not be taken for the board's side.
        for (name, pts) in [
            ("corner trapezoid", vec![DVec3::new(9.0, 6.0, 0.5), DVec3::new(9.0, 5.5, 0.5), DVec3::new(10.0, 5.4, 0.5), DVec3::new(10.0, 6.0, 0.5)]),
            ("middle trapezoid", vec![DVec3::new(9.0, 3.0, 0.5), DVec3::new(9.0, 2.0, 0.5), DVec3::new(10.0, 1.9, 0.5), DVec3::new(10.0, 3.1, 0.5)]),
            ("middle rectangle", vec![DVec3::new(9.0, 3.0, 0.5), DVec3::new(9.0, 2.0, 0.5), DVec3::new(10.0, 2.0, 0.5), DVec3::new(10.0, 3.0, 0.5)]),
        ] {
            let mut m = board();
            let face = m.draw_polygon(&pts, DVec3::Z).expect("a face");
            m.push_pull(face, -0.5).unwrap();
            assert!(open(&m).is_empty(), "{name}: open edges {:?}", open(&m));
        }
    }
}
