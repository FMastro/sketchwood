//! Circles and arcs, as SketchUp draws them: polygons and polylines of short straight edges
//! (24 sides for a circle, 12 for an arc by default). They're drawn as lines, so they split
//! faces like any other edges: a circle on a board's top becomes a disk to push into a hole, an
//! arc across a corner splits off the piece to remove for a rounded corner.

use glam::{DQuat, DVec3};

use crate::{EPS, EdgeId, FaceId, Model};

/// SketchUp's defaults.
pub const CIRCLE_SEGMENTS: u32 = 24;
pub const ARC_SEGMENTS: u32 = 12;

/// Axes in the plane with `normal`: horizontal planes use red and green, others a level axis
/// and the in-plane "up".
pub fn plane_axes(normal: DVec3) -> (DVec3, DVec3) {
    let n = normal.normalize();
    if n.z.abs() > 0.999 {
        (DVec3::X, DVec3::Y * n.z.signum())
    } else {
        let u = DVec3::Z.cross(n).normalize();
        (u, n.cross(u))
    }
}

/// The corners of a circle of `radius` around `center` in the plane with `normal`, starting on
/// the plane's first axis.
pub fn circle_points(center: DVec3, normal: DVec3, radius: f64, segments: u32) -> Vec<DVec3> {
    let (u, v) = plane_axes(normal);
    let segments = segments.max(3);
    (0..segments)
        .map(|i| {
            let angle = std::f64::consts::TAU * f64::from(i) / f64::from(segments);
            center + (u * angle.cos() + v * angle.sin()) * radius
        })
        .collect()
}

/// An arc from `a` to `b` in the plane with `normal` that bulges out by `bulge` (the sagitta:
/// the arc's height above the chord's middle) toward `side`, a direction in the plane across
/// the chord. Returns `segments + 1` points from `a` to `b`, or just the two ends for no bulge.
pub fn arc_points(a: DVec3, b: DVec3, normal: DVec3, bulge: f64, side: DVec3, segments: u32) -> Vec<DVec3> {
    let chord = b - a;
    let half = chord.length() * 0.5;
    if half < EPS || bulge.abs() < EPS {
        return vec![a, b];
    }
    let n = normal.normalize();
    let mut across = n.cross(chord).normalize();
    if across.dot(side) < 0.0 {
        across = -across;
    }
    let s = bulge.abs();
    let radius = (half * half + s * s) / (2.0 * s);
    let middle = a + chord * 0.5;
    // The center sits on the far side of the chord for a minor arc, beyond the bulge for more.
    let center = middle + across * (s - radius);
    let top = middle + across * s;
    let sweep = 2.0 * ((top - center).angle_between(a - center));
    // Turn whichever way passes through the bulge.
    let axis = if (a - center).cross(top - center).dot(n) >= 0.0 { n } else { -n };
    let segments = segments.max(1);
    (0..=segments)
        .map(|i| {
            if i == segments {
                return b;
            }
            let turn = DQuat::from_axis_angle(axis, sweep * f64::from(i) / f64::from(segments));
            center + turn * (a - center)
        })
        .collect()
}

impl Model {
    /// Draw a circle; returns its face (the disk) when one formed.
    pub fn draw_circle(&mut self, center: DVec3, normal: DVec3, radius: f64, segments: u32) -> Option<FaceId> {
        if radius < EPS {
            return None;
        }
        self.draw_polygon(&circle_points(center, normal, radius, segments), normal)
    }

    /// Draw a polyline (an arc's points), splitting what it crosses. Returns its edges.
    pub fn draw_polyline(&mut self, points: &[DVec3], facing: DVec3) -> Vec<EdgeId> {
        points.windows(2).flat_map(|w| self.add_line_facing(w[0], w[1], Some(facing))).collect()
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

    #[test]
    fn a_circle_is_a_24_sided_face() {
        let mut model = Model::default();
        let disk = model.draw_circle(DVec3::new(5.0, 5.0, 0.0), DVec3::Z, 2.0, CIRCLE_SEGMENTS).unwrap();
        assert_eq!(model.edges().count(), 24);
        let polygon = 0.5 * 24.0 * 4.0 * (std::f64::consts::TAU / 24.0).sin();
        assert!((area(&model, disk) - polygon).abs() < 1e-9);
        assert!(model.face(disk).unwrap().normal.abs_diff_eq(DVec3::Z, 1e-12));
    }

    #[test]
    fn a_circle_on_a_board_pushes_through_into_a_hole() {
        let mut model = Model::default();
        let base = model.add_rectangle(DVec3::ZERO, DVec3::X * 10.0, DVec3::Y * 10.0, DVec3::Z).unwrap();
        model.push_pull(base, 0.75).unwrap();
        let disk = model.draw_circle(DVec3::new(5.0, 5.0, 0.75), DVec3::Z, 1.0, 16).unwrap();
        let top = model.faces().find(|(id, f)| *id != disk && f.normal.z > 0.5).map(|(id, _)| id).unwrap();
        assert_eq!(model.face(top).unwrap().holes.len(), 1);
        model.push_pull(disk, -0.75).unwrap();
        // Through: the bottom has a matching hole and nothing covers it.
        let bottom = model.faces().find(|(_, f)| f.normal.z < -0.5).map(|(_, f)| f.clone()).unwrap();
        assert_eq!(bottom.holes.len(), 1, "the hole goes through the bottom");
        assert!(model.faces().all(|(_, f)| f.normal.z.abs() < 0.5 || f.holes.len() == 1));
    }

    #[test]
    fn arc_points_bulge_by_the_sagitta() {
        let a = DVec3::ZERO;
        let b = DVec3::new(4.0, 0.0, 0.0);
        let points = arc_points(a, b, DVec3::Z, 2.0, DVec3::Y, ARC_SEGMENTS);
        assert_eq!(points.len(), 13);
        assert_eq!((points[0], points[12]), (a, b));
        // A semicircle of radius 2 around (2, 0).
        assert!(points.iter().all(|p| (p.distance(DVec3::new(2.0, 0.0, 0.0)) - 2.0).abs() < 1e-9));
        assert!(points[6].abs_diff_eq(DVec3::new(2.0, 2.0, 0.0), 1e-9), "{}", points[6]);
        // The other way, and more than a half circle.
        let below = arc_points(a, b, DVec3::Z, 3.0, -DVec3::Y, 12);
        assert!(below.iter().all(|p| p.y <= 1e-9));
        // Radius 13/6 around (2, -5/6): it reaches x = 2 - 13/6, past the chord's start.
        assert!(below.iter().any(|p| p.x < -0.1), "a major arc reaches past the chord's ends");
        assert!((below[6].y + 3.0).abs() < 1e-9);
    }

    #[test]
    fn pushing_an_arced_corner_through_rounds_the_board() {
        let mut model = Model::default();
        let base = model.add_rectangle(DVec3::ZERO, DVec3::X * 10.0, DVec3::Y * 10.0, DVec3::Z).unwrap();
        model.push_pull(base, 0.75).unwrap();
        let (a, b) = (DVec3::new(8.0, 10.0, 0.75), DVec3::new(10.0, 8.0, 0.75));
        let bulge = 2.0 - 2.0 * std::f64::consts::FRAC_1_SQRT_2;
        model.draw_polyline(&arc_points(a, b, DVec3::Z, bulge, DVec3::new(1.0, 1.0, 0.0), 12), DVec3::Z);
        let corner = model
            .faces()
            .filter(|(_, f)| f.normal.z > 0.5)
            .min_by(|(x, _), (y, _)| area(&model, *x).total_cmp(&area(&model, *y)))
            .map(|(id, _)| id)
            .unwrap();
        model.push_pull(corner, -0.75).unwrap();
        // Top, bottom, two full sides, two shortened sides and 12 rounded facets; no leftovers.
        assert_eq!(model.faces().count(), 18);
        assert!(model.faces().all(|(_, f)| f.holes.is_empty()));
        assert!(model.vertices().all(|(_, p)| p.x < 9.99 || p.y < 9.99), "the square corner is gone");
        let edges_without_faces = model.edges().filter(|(id, _)| model.edge_faces(*id).is_empty()).count();
        assert_eq!(edges_without_faces, 0);
    }

    #[test]
    fn an_arc_across_a_corner_splits_it_off_for_rounding() {
        let mut model = Model::default();
        model.add_rectangle(DVec3::ZERO, DVec3::X * 10.0, DVec3::Y * 10.0, DVec3::Z).unwrap();
        // Radius-2 quarter round at the (10, 10) corner: from (8, 10) to (10, 8).
        let a = DVec3::new(8.0, 10.0, 0.0);
        let b = DVec3::new(10.0, 8.0, 0.0);
        let bulge = 2.0 - 2.0 * std::f64::consts::FRAC_1_SQRT_2;
        let points = arc_points(a, b, DVec3::Z, bulge, DVec3::new(1.0, 1.0, 0.0), ARC_SEGMENTS);
        assert!(points.iter().all(|p| (p.distance(DVec3::new(8.0, 8.0, 0.0)) - 2.0).abs() < 1e-9));
        model.draw_polyline(&points, DVec3::Z);
        assert_eq!(model.faces().count(), 2);
        let corner = model.faces().map(|(id, _)| area(&model, id)).fold(f64::INFINITY, f64::min);
        assert!(corner > 0.8 && corner < 0.87, "{corner}"); // 4 - pi, less the polygon's chords
    }
}
