//! Offset (SketchUp's F): a copy of a face's outline, moved in or out by a distance, drawn on
//! the face's plane. Offsetting inward splits the face into a frame and an inner panel (the
//! classic way to start a door frame or a raised panel); outward adds a frame around it.

use std::fmt;

use glam::{DVec2, DVec3};

use crate::{EPS, FaceId, Model};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OffsetError {
    MissingFace,
    ZeroDistance,
    /// The outline would fold over itself (an inward offset wider than the face allows).
    TooFar,
}

impl fmt::Display for OffsetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::MissingFace => "That face no longer exists",
            Self::ZeroDistance => "Distance must not be zero",
            Self::TooFar => "That offset doesn't fit inside the face",
        })
    }
}

impl std::error::Error for OffsetError {}

impl Model {
    /// The face's outer outline moved `distance` inches inward (negative: outward), corner to
    /// corner, each side staying parallel to the original.
    pub fn offset_outline(&self, face: FaceId, distance: f64) -> Result<Vec<DVec3>, OffsetError> {
        if distance.abs() < EPS {
            return Err(OffsetError::ZeroDistance);
        }
        let face = self.face(face).ok_or(OffsetError::MissingFace)?;
        let n = face.normal;
        let u = n.any_orthonormal_vector();
        let v = n.cross(u);
        let origin = self.vertex(face.outer[0]);
        let flat = |p: DVec3| DVec2::new((p - origin).dot(u), (p - origin).dot(v));
        let lift = |p: DVec2| origin + u * p.x + v * p.y;

        // Counter-clockwise around the normal, so inward is to the left of each side. Points
        // where the outline runs straight on are dropped; they'd only make degenerate corners.
        let all: Vec<DVec2> = face.outer.iter().map(|&id| flat(self.vertex(id))).collect();
        let count = all.len();
        let points: Vec<DVec2> = (0..count)
            .filter(|&i| {
                let (prev, here, next) = (all[(i + count - 1) % count], all[i], all[(i + 1) % count]);
                (here - prev).perp_dot(next - here).abs() > EPS * (here - prev).length().max(1.0)
            })
            .map(|i| all[i])
            .collect();
        let count = points.len();
        if count < 3 {
            return Err(OffsetError::MissingFace);
        }
        let side = |i: usize| (points[(i + 1) % count] - points[i]).normalize();
        let result: Vec<DVec2> = points
            .iter()
            .enumerate()
            .map(|(i, &corner)| {
                // Where the two shifted sides meet: along the corner's bisector.
                let bisector = side((i + count - 1) % count).perp() + side(i).perp();
                corner + bisector * (distance * 2.0 / bisector.length_squared())
            })
            .collect();
        // Every side must keep its direction, or the outline turned inside out.
        let valid = (0..count).all(|i| {
            let shifted = result[(i + 1) % count] - result[i];
            shifted.dot(side(i)) > EPS
        });
        if !valid {
            return Err(OffsetError::TooFar);
        }
        Ok(result.into_iter().map(lift).collect())
    }

    /// Draw the face's outline offset by `distance` (positive inward) onto its plane. Returns
    /// the face the new outline bounds: the inner panel, or for an outward offset the frame.
    pub fn offset_face(&mut self, face: FaceId, distance: f64) -> Result<Option<FaceId>, OffsetError> {
        let outline = self.offset_outline(face, distance)?;
        let normal = self.face(face).ok_or(OffsetError::MissingFace)?.normal;
        Ok(self.draw_polygon(&outline, normal))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area(model: &Model, face: FaceId) -> f64 {
        model
            .triangulate(model.face(face).unwrap())
            .iter()
            .map(|[a, b, c]| (*b - *a).cross(*c - *a).length() * 0.5)
            .sum()
    }

    fn door(model: &mut Model) -> FaceId {
        // A 24" x 36" door seen from the front.
        model.add_rectangle(DVec3::ZERO, DVec3::X * 24.0, DVec3::Z * 36.0, -DVec3::Y).unwrap()
    }

    #[test]
    fn inward_offset_makes_a_frame_and_a_panel() {
        let mut model = Model::default();
        let face = door(&mut model);
        let panel = model.offset_face(face, 2.5).unwrap().unwrap();
        assert!((area(&model, panel) - 19.0 * 31.0).abs() < 1e-9);
        assert_eq!(model.faces().count(), 2);
        let frame = model.faces().map(|(id, _)| id).find(|&id| id != panel).unwrap();
        assert!((area(&model, frame) - (24.0 * 36.0 - 19.0 * 31.0)).abs() < 1e-9);
        assert!(model.faces().all(|(_, f)| f.normal.abs_diff_eq(-DVec3::Y, 1e-12)));
    }

    #[test]
    fn outward_offset_adds_a_frame_around() {
        let mut model = Model::default();
        let face = door(&mut model);
        let frame = model.offset_face(face, -1.0).unwrap().unwrap();
        assert_eq!(model.face(frame).unwrap().holes.len(), 1, "a frame around the original");
        assert_eq!(model.faces().count(), 2);
        assert!(model.face(face).is_some());
        let total: f64 = model.faces().map(|(id, _)| area(&model, id)).sum();
        assert!((total - 26.0 * 38.0).abs() < 1e-9, "{total}");
    }

    #[test]
    fn an_l_shape_offsets_with_mitered_corners() {
        let mut model = Model::default();
        let l = model
            .add_face(&[
                DVec3::ZERO,
                DVec3::new(10.0, 0.0, 0.0),
                DVec3::new(10.0, 4.0, 0.0),
                DVec3::new(4.0, 4.0, 0.0),
                DVec3::new(4.0, 10.0, 0.0),
                DVec3::new(0.0, 10.0, 0.0),
            ])
            .unwrap();
        let outline = model.offset_outline(l, 1.0).unwrap();
        assert_eq!(outline.len(), 6);
        assert!(outline.iter().any(|p| p.abs_diff_eq(DVec3::new(3.0, 3.0, 0.0), 1e-9)), "{outline:?}");
        assert!(outline.iter().any(|p| p.abs_diff_eq(DVec3::new(1.0, 1.0, 0.0), 1e-9)));
    }

    #[test]
    fn too_wide_an_inward_offset_is_refused() {
        let mut model = Model::default();
        let face = door(&mut model);
        assert_eq!(model.offset_face(face, 12.0), Err(OffsetError::TooFar));
        assert_eq!(model.offset_face(face, 0.0), Err(OffsetError::ZeroDistance));
        assert_eq!(model.faces().count(), 1);
    }
}
