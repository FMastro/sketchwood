//! Turning cursor positions into model entities and snapped points.

use glam::{DVec3, Vec2};
use sketchcraft_doc::{EdgeId, Entity, FaceId, Model, VertexId};
use sketchcraft_view::OrbitCamera;

/// How close, in points, the cursor must be to snap to an endpoint.
pub const SNAP_RADIUS: f32 = 12.0;
/// How close, in points, the cursor must be to pick an edge.
pub const EDGE_RADIUS: f32 = 6.0;

/// The camera and viewport size: everything needed to map between screen and model.
#[derive(Clone, Copy)]
pub struct Screen<'a> {
    pub camera: &'a OrbitCamera,
    pub size: Vec2,
}

impl Screen<'_> {
    pub fn project(&self, point: DVec3) -> Option<Vec2> {
        self.camera.project(point.as_vec3(), self.size)
    }

    pub fn ray(&self, cursor: Vec2) -> Ray {
        let (origin, dir) = self.camera.ray(cursor, self.size);
        Ray { origin: origin.as_dvec3(), dir: dir.as_dvec3().normalize() }
    }

    pub fn eye(&self) -> DVec3 {
        self.camera.eye().as_dvec3()
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Ray {
    pub origin: DVec3,
    pub dir: DVec3,
}

impl Ray {
    /// Where the ray crosses the plane through `point` with `normal`, if in front of it.
    pub fn hit_plane(&self, point: DVec3, normal: DVec3) -> Option<DVec3> {
        let denom = self.dir.dot(normal);
        if denom.abs() < 1e-9 {
            return None;
        }
        let t = (point - self.origin).dot(normal) / denom;
        (t > 0.0).then(|| self.origin + self.dir * t)
    }
}

/// The nearest face the ray hits, and where.
pub fn hit_face(model: &Model, ray: &Ray) -> Option<(FaceId, DVec3)> {
    model
        .faces()
        .filter_map(|(id, face)| {
            let hit = ray.hit_plane(model.vertex(face.outer[0]), face.normal)?;
            model.face_contains(face, hit).then_some((id, hit))
        })
        .min_by(|(_, a), (_, b)| {
            a.distance_squared(ray.origin).total_cmp(&b.distance_squared(ray.origin))
        })
}

/// Whether a face (other than those `ignore` accepts) hides `point` from `eye`.
fn occluded(model: &Model, eye: DVec3, point: DVec3, ignore: impl Fn(FaceId) -> bool) -> bool {
    let to_point = point - eye;
    let distance = to_point.length();
    let ray = Ray { origin: eye, dir: to_point / distance };
    model.faces().any(|(id, face)| {
        !ignore(id)
            && ray.hit_plane(model.vertex(face.outer[0]), face.normal).is_some_and(|hit| {
                hit.distance(eye) < distance * (1.0 - 1e-6) && model.face_contains(face, hit)
            })
    })
}

/// The visible edge nearest the cursor, within [`EDGE_RADIUS`].
pub fn hit_edge(model: &Model, screen: Screen<'_>, cursor: Vec2) -> Option<EdgeId> {
    let eye = screen.eye();
    model
        .edges()
        .filter_map(|(id, edge)| {
            let [a, b] = model.edge_points(edge);
            let (sa, sb) = (screen.project(a)?, screen.project(b)?);
            let (distance, t) = segment_distance(cursor, sa, sb);
            if distance > EDGE_RADIUS {
                return None;
            }
            let point = a.lerp(b, f64::from(t));
            let hidden = occluded(model, eye, point, |f| {
                model.face(f).is_some_and(|face| face.uses_edge(edge))
            });
            (!hidden).then_some((id, distance))
        })
        .min_by(|(_, a), (_, b)| a.total_cmp(b))
        .map(|(id, _)| id)
}

/// What's under the cursor: edges take priority over the faces they border, as in SketchUp.
pub fn pick_entity(model: &Model, screen: Screen<'_>, cursor: Vec2) -> Option<Entity> {
    hit_edge(model, screen, cursor)
        .map(Entity::Edge)
        .or_else(|| hit_face(model, &screen.ray(cursor)).map(|(id, _)| Entity::Face(id)))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapKind {
    Endpoint,
    Origin,
    OnFace,
    /// On the plane a tool is drawing in.
    OnPlane,
    OnGround,
}

impl SnapKind {
    /// SketchUp-style inference tooltip.
    pub fn label(self) -> Option<&'static str> {
        match self {
            Self::Endpoint => Some("Endpoint"),
            Self::Origin => Some("Origin"),
            Self::OnFace => Some("On Face"),
            Self::OnPlane | Self::OnGround => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Snap {
    pub point: DVec3,
    pub kind: SnapKind,
    /// Normal of the surface the point is on; tools draw in this plane.
    pub normal: DVec3,
}

/// The point the cursor indicates. With a `plane` (point, normal), the result is always on
/// that plane. Endpoints within [`SNAP_RADIUS`] win, then faces, then the ground.
pub fn snap(
    model: &Model,
    screen: Screen<'_>,
    cursor: Vec2,
    plane: Option<(DVec3, DVec3)>,
) -> Option<Snap> {
    let ray = screen.ray(cursor);
    let surface = match plane {
        Some((point, normal)) => ray.hit_plane(point, normal).map(|p| (p, normal, SnapKind::OnPlane)),
        None => hit_face(model, &ray)
            .map(|(id, p)| (p, model.face(id).expect("hit a live face").normal, SnapKind::OnFace))
            .or_else(|| ray.hit_plane(DVec3::ZERO, DVec3::Z).map(|p| (p, DVec3::Z, SnapKind::OnGround))),
    };

    match (snap_endpoint(model, screen, cursor, |_| false), plane) {
        // Snapped points off the drawing plane are projected onto it.
        (Some(s), Some((origin, normal))) => {
            Some(Snap { point: s.point - normal * (s.point - origin).dot(normal), kind: s.kind, normal })
        }
        (Some(s), None) => Some(Snap { normal: surface.map_or(DVec3::Z, |s| s.1), ..s }),
        (None, _) => surface.map(|(point, normal, kind)| Snap { point, kind, normal }),
    }
}

/// The visible endpoint (or the origin) nearest the cursor within [`SNAP_RADIUS`], skipping
/// vertices `exclude` accepts. The snap's normal is +Z.
pub fn snap_endpoint(
    model: &Model,
    screen: Screen<'_>,
    cursor: Vec2,
    exclude: impl Fn(VertexId) -> bool,
) -> Option<Snap> {
    let eye = screen.eye();
    model
        .vertices()
        .filter(|(id, _)| !exclude(*id))
        .map(|(id, p)| (p, SnapKind::Endpoint, Some(id)))
        .chain(std::iter::once((DVec3::ZERO, SnapKind::Origin, None)))
        .filter_map(|(point, kind, id)| {
            let distance = screen.project(point)?.distance(cursor);
            if distance > SNAP_RADIUS {
                return None;
            }
            if let Some(id) = id
                && occluded(model, eye, point, |f| model.face(f).is_some_and(|face| face.uses_vertex(id)))
            {
                return None;
            }
            Some((distance, Snap { point, kind, normal: DVec3::Z }))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, snap)| snap)
}

/// The parameter `t` of the point on the line `anchor + t * dir` (unit `dir`) closest to `ray`,
/// or `None` when looking straight down the line.
pub fn closest_on_line(anchor: DVec3, dir: DVec3, ray: &Ray) -> Option<f64> {
    let b = dir.dot(ray.dir);
    let denom = 1.0 - b * b;
    if denom < 1e-6 {
        return None;
    }
    let w = anchor - ray.origin;
    Some((b * ray.dir.dot(w) - dir.dot(w)) / denom)
}

/// Distance from `p` to segment `ab`, and the parameter of the closest point.
pub fn segment_distance(p: Vec2, a: Vec2, b: Vec2) -> (f32, f32) {
    let ab = b - a;
    let t = if ab.length_squared() > 0.0 { ((p - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0) } else { 0.0 };
    ((a + ab * t).distance(p), t)
}

#[cfg(test)]
mod tests {
    use sketchcraft_doc::Model;
    use sketchcraft_view::StandardView;

    use super::*;

    const SIZE: Vec2 = Vec2::new(1000.0, 800.0);

    fn top_camera() -> OrbitCamera {
        let mut camera = OrbitCamera { target: glam::Vec3::new(5.0, 5.0, 0.0), distance: 40.0, ..Default::default() };
        camera.set_view(StandardView::Top);
        camera
    }

    fn square() -> Model {
        let mut model = Model::default();
        model.add_rectangle(DVec3::ZERO, DVec3::X * 10.0, DVec3::Y * 10.0, DVec3::Z);
        model
    }

    #[test]
    fn picks_face_in_the_middle_and_edge_near_the_border() {
        let model = square();
        let camera = top_camera();
        let screen = Screen { camera: &camera, size: SIZE };
        let center = screen.project(DVec3::new(5.0, 5.0, 0.0)).unwrap();
        assert!(matches!(pick_entity(&model, screen, center), Some(Entity::Face(_))));
        let on_edge = screen.project(DVec3::new(5.0, 0.0, 0.0)).unwrap() + Vec2::new(0.0, 3.0);
        assert!(matches!(pick_entity(&model, screen, on_edge), Some(Entity::Edge(_))));
        let outside = screen.project(DVec3::new(15.0, 15.0, 0.0)).unwrap();
        assert_eq!(pick_entity(&model, screen, outside), None);
    }

    #[test]
    fn snaps_to_nearby_endpoint() {
        let model = square();
        let camera = top_camera();
        let screen = Screen { camera: &camera, size: SIZE };
        let near_corner = screen.project(DVec3::new(10.0, 10.0, 0.0)).unwrap() + Vec2::new(4.0, -4.0);
        let snap = snap(&model, screen, near_corner, None).unwrap();
        assert_eq!(snap.kind, SnapKind::Endpoint);
        assert!(snap.point.abs_diff_eq(DVec3::new(10.0, 10.0, 0.0), 1e-9));
    }

    #[test]
    fn falls_back_to_face_then_ground() {
        let model = square();
        let camera = top_camera();
        let screen = Screen { camera: &camera, size: SIZE };
        let on_face = screen.project(DVec3::new(4.0, 6.0, 0.0)).unwrap();
        assert_eq!(snap(&model, screen, on_face, None).unwrap().kind, SnapKind::OnFace);
        let off_face = screen.project(DVec3::new(-5.0, 6.0, 0.0)).unwrap();
        let ground = snap(&model, screen, off_face, None).unwrap();
        assert_eq!(ground.kind, SnapKind::OnGround);
        assert!(ground.point.abs_diff_eq(DVec3::new(-5.0, 6.0, 0.0), 1e-3));
    }
}
