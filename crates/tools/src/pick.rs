//! Turning cursor positions into model entities and snapped points.

use glam::{DAffine3, DVec3, Vec2};
use sketchcraft_doc::{EdgeId, Entity, FaceId, Guide, GuideId, Model, VertexId};
use sketchcraft_view::OrbitCamera;

use crate::Axis;

/// How close, in points, the cursor must be to snap to an endpoint.
pub const SNAP_RADIUS: f32 = 12.0;
/// How close, in points, the cursor must be to pick an edge.
pub const EDGE_RADIUS: f32 = 6.0;

/// The camera and viewport size: everything needed to map between screen and model. Points are
/// in the coordinates of the container being edited (a group's own axes when inside one).
#[derive(Clone, Copy)]
pub struct Screen<'a> {
    pub camera: &'a OrbitCamera,
    pub size: Vec2,
    to_local: DAffine3,
    /// Model coordinates straight to clip space, worked out once: picking projects every
    /// vertex and edge on each mouse move.
    clip: glam::DMat4,
}

impl<'a> Screen<'a> {
    /// At the top level, where model coordinates are world coordinates.
    pub fn new(camera: &'a OrbitCamera, size: Vec2) -> Self {
        Self::in_context(camera, size, DAffine3::IDENTITY)
    }

    /// Inside a group whose coordinates map to the world through `to_world`.
    pub fn in_context(camera: &'a OrbitCamera, size: Vec2, to_world: DAffine3) -> Self {
        let clip = camera.view_proj(size).as_dmat4() * glam::DMat4::from(to_world);
        Self { camera, size, to_local: to_world.inverse(), clip }
    }

    pub fn project(&self, point: DVec3) -> Option<Vec2> {
        let clip = self.clip * point.extend(1.0);
        if clip.w <= 0.0 {
            return None;
        }
        let (x, y) = (clip.x / clip.w, clip.y / clip.w);
        Some(Vec2::new(((x + 1.0) * 0.5) as f32 * self.size.x, ((1.0 - y) * 0.5) as f32 * self.size.y))
    }

    pub fn ray(&self, cursor: Vec2) -> Ray {
        let (origin, dir) = self.camera.ray(cursor, self.size);
        Ray {
            origin: self.to_local.transform_point3(origin.as_dvec3()),
            dir: self.to_local.transform_vector3(dir.as_dvec3()).normalize(),
        }
    }

    pub fn eye(&self) -> DVec3 {
        self.to_local.transform_point3(self.camera.eye().as_dvec3())
    }

    /// The camera's viewing direction.
    pub fn forward(&self) -> DVec3 {
        self.to_local.transform_vector3(self.camera.forward().as_dvec3()).normalize()
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
    // Where the ray crosses each face's plane (cheap), nearest first; then the slower
    // inside-the-outline test only until one passes.
    let mut crossings: Vec<(f64, FaceId, DVec3)> = model
        .face_index()
        .iter()
        .filter_map(|e| {
            let hit = ray.hit_plane(e.origin, e.normal)?;
            e.box_contains(hit).then(|| (hit.distance_squared(ray.origin), e.id, hit))
        })
        .collect();
    crossings.sort_unstable_by(|a, b| a.0.total_cmp(&b.0));
    crossings
        .into_iter()
        .find(|&(_, id, hit)| model.face(id).is_some_and(|face| model.face_contains(face, hit)))
        .map(|(_, id, hit)| (id, hit))
}

/// Whether a face (other than those `ignore` accepts) hides `point` from `eye`.
fn occluded(model: &Model, eye: DVec3, point: DVec3, ignore: impl Fn(FaceId) -> bool) -> bool {
    let to_point = point - eye;
    let distance = to_point.length();
    let ray = Ray { origin: eye, dir: to_point / distance };
    // Cheap tests first (plane, distance, bounding box), the outline test last.
    model.face_index().iter().any(|e| {
        ray.hit_plane(e.origin, e.normal).is_some_and(|hit| {
            hit.distance(eye) < distance * (1.0 - 1e-6)
                && e.box_contains(hit)
                && !ignore(e.id)
                && model.face(e.id).is_some_and(|face| model.face_contains(face, hit))
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

/// What's under the cursor: edges, then guides, then faces, as in SketchUp.
pub fn pick_entity(model: &Model, screen: Screen<'_>, cursor: Vec2) -> Option<Entity> {
    hit_edge(model, screen, cursor)
        .map(Entity::Edge)
        .or_else(|| hit_guide(model, screen, cursor).map(Entity::Guide))
        .or_else(|| hit_face(model, &screen.ray(cursor)).map(|(id, _)| Entity::Face(id)))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapKind {
    Endpoint,
    Origin,
    GuidePoint,
    /// Where a guide line crosses an edge or another guide line.
    Intersection,
    Midpoint,
    OnEdge,
    OnGuide,
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
            Self::GuidePoint => Some("Guide Point"),
            Self::Intersection => Some("Intersection"),
            Self::Midpoint => Some("Midpoint"),
            Self::OnEdge => Some("On Edge"),
            Self::OnGuide => Some("On Guide"),
            Self::OnFace => Some("On Face"),
            Self::OnPlane | Self::OnGround => None,
        }
    }

    /// A point on existing geometry (as opposed to a surface or the drawing plane).
    pub fn is_feature(self) -> bool {
        matches!(
            self,
            Self::Endpoint
                | Self::Origin
                | Self::GuidePoint
                | Self::Intersection
                | Self::Midpoint
                | Self::OnEdge
                | Self::OnGuide
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Snap {
    pub point: DVec3,
    pub kind: SnapKind,
    /// Normal of the surface the point is on; tools draw in this plane.
    pub normal: DVec3,
    /// For a point on an edge or guide line, the line's direction.
    pub line: Option<DVec3>,
}

/// The point the cursor indicates. With a `plane` (point, normal), the result is always on
/// that plane. Features ([`snap_feature`]) win, then faces, then the ground.
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

    match (snap_feature(model, screen, cursor, |_| false), plane) {
        // Snapped points off the drawing plane are projected onto it.
        (Some(s), Some((origin, normal))) => {
            Some(Snap { point: s.point - normal * (s.point - origin).dot(normal), normal, ..s })
        }
        (Some(s), None) => Some(Snap { normal: surface.map_or(DVec3::Z, |s| s.1), ..s }),
        (None, _) => surface.map(|(point, normal, kind)| Snap { point, kind, normal, line: None }),
    }
}

/// The geometry feature the cursor is on, in SketchUp's priority: an endpoint, guide point or
/// the origin within [`SNAP_RADIUS`], else where a guide line crosses an edge or another guide
/// within [`SNAP_RADIUS`], else an edge midpoint within [`SNAP_RADIUS`], else the nearest point
/// on an edge or guide line within [`EDGE_RADIUS`]. Hidden features, and those of edges
/// touching vertices `exclude` accepts, are skipped. The snap's normal is +Z.
pub fn snap_feature(
    model: &Model,
    screen: Screen<'_>,
    cursor: Vec2,
    exclude: impl Fn(VertexId) -> bool,
) -> Option<Snap> {
    let eye = screen.eye();
    let nearest = |candidates: Vec<(f32, DVec3, SnapKind, Option<DVec3>)>| {
        candidates
            .into_iter()
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, point, kind, line)| Snap { point, kind, normal: DVec3::Z, line })
    };
    let near = |point: DVec3, radius: f32| screen.project(point).map(|s| s.distance(cursor)).filter(|&d| d <= radius);
    let guide_lines: Vec<(DVec3, DVec3)> = model
        .guides()
        .filter_map(|(_, g)| match *g {
            Guide::Line { point, dir } => Some((point, dir)),
            Guide::Point(_) => None,
        })
        .collect();

    let endpoints = model
        .vertices()
        .filter(|(id, _)| !exclude(*id))
        .filter_map(|(id, point)| {
            let distance = near(point, SNAP_RADIUS)?;
            let hidden = occluded(model, eye, point, |f| model.face(f).is_some_and(|face| face.uses_vertex(id)));
            (!hidden).then_some((distance, point, SnapKind::Endpoint, None))
        })
        .chain(model.guides().filter_map(|(_, g)| match *g {
            Guide::Point(p) => near(p, SNAP_RADIUS).map(|d| (d, p, SnapKind::GuidePoint, None)),
            Guide::Line { .. } => None,
        }))
        .chain(near(DVec3::ZERO, SNAP_RADIUS).map(|d| (d, DVec3::ZERO, SnapKind::Origin, None)))
        .collect();
    if let Some(snap) = nearest(endpoints) {
        return Some(snap);
    }

    let edges: Vec<_> = model.edges().filter(|(_, e)| !exclude(e.a) && !exclude(e.b)).map(|(_, e)| e).collect();
    let visible = |point: DVec3, edge: &sketchcraft_doc::Edge| {
        !occluded(model, eye, point, |f| model.face(f).is_some_and(|face| face.uses_edge(edge)))
    };

    // Guide lines crossing edges and each other.
    let mut crossings = Vec::new();
    for (i, &(p, d)) in guide_lines.iter().enumerate() {
        for edge in &edges {
            let [a, b] = model.edge_points(edge);
            let length = a.distance(b);
            if let Some((s, point)) = line_crossing(a, (b - a) / length, p, d)
                && (-1e-9..=length + 1e-9).contains(&s)
                && let Some(distance) = near(point, SNAP_RADIUS)
                && visible(point, edge)
            {
                crossings.push((distance, point, SnapKind::Intersection, None));
            }
        }
        for &(q, e) in &guide_lines[i + 1..] {
            if let Some((_, point)) = line_crossing(q, e, p, d)
                && let Some(distance) = near(point, SNAP_RADIUS)
            {
                crossings.push((distance, point, SnapKind::Intersection, None));
            }
        }
    }
    if let Some(snap) = nearest(crossings) {
        return Some(snap);
    }

    let midpoints = edges
        .iter()
        .filter_map(|edge| {
            let [a, b] = model.edge_points(edge);
            let mid = (a + b) * 0.5;
            let distance = near(mid, SNAP_RADIUS)?;
            visible(mid, edge).then_some((distance, mid, SnapKind::Midpoint, None))
        })
        .collect();
    if let Some(snap) = nearest(midpoints) {
        return Some(snap);
    }

    let ray = screen.ray(cursor);
    let on_edge = edges
        .iter()
        .filter_map(|edge| {
            let [a, b] = model.edge_points(edge);
            let (distance, _) = segment_distance(cursor, screen.project(a)?, screen.project(b)?);
            if distance > EDGE_RADIUS {
                return None;
            }
            // The exact 3D point: where the edge passes closest to the cursor ray.
            let length = a.distance(b);
            let dir = (b - a) / length;
            let t = closest_on_line(a, dir, &ray)?.clamp(0.0, length);
            let point = a + dir * t;
            visible(point, edge).then_some((distance, point, SnapKind::OnEdge, Some(dir)))
        })
        .chain(guide_lines.iter().filter_map(|&(p, d)| {
            let point = p + d * closest_on_line(p, d, &ray)?;
            let distance = near(point, EDGE_RADIUS)?;
            Some((distance, point, SnapKind::OnGuide, Some(d)))
        }))
        .collect();
    nearest(on_edge)
}

/// Where the line `p + s * d` (unit `d`) crosses the line through `q` along unit `e`: the
/// parameter `s` and the point. `None` for parallel or skew lines.
fn line_crossing(p: DVec3, d: DVec3, q: DVec3, e: DVec3) -> Option<(f64, DVec3)> {
    let b = d.dot(e);
    let denom = 1.0 - b * b;
    if denom < 1e-9 {
        return None;
    }
    let w = p - q;
    let s = (b * e.dot(w) - d.dot(w)) / denom;
    let t = (e.dot(w) - b * d.dot(w)) / denom;
    let (x, y) = (p + d * s, q + e * t);
    (x.distance(y) <= 1e-6).then_some((s, x))
}

/// The guide nearest the cursor: a guide point within [`SNAP_RADIUS`] or a guide line within
/// [`EDGE_RADIUS`].
pub fn hit_guide(model: &Model, screen: Screen<'_>, cursor: Vec2) -> Option<GuideId> {
    let ray = screen.ray(cursor);
    model
        .guides()
        .filter_map(|(id, g)| {
            let (point, radius) = match *g {
                Guide::Point(p) => (p, SNAP_RADIUS),
                Guide::Line { point, dir } => (point + dir * closest_on_line(point, dir, &ray)?, EDGE_RADIUS),
            };
            let distance = screen.project(point)?.distance(cursor);
            (distance <= radius).then_some((id, distance))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(id, _)| id)
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

/// How close, in points, the cursor must come to an axis line to lock onto it.
pub const AXIS_RADIUS: f32 = 14.0;

/// SketchUp's axis inference: the point on the red, green or blue line through `base` nearest
/// the cursor, when the cursor is within [`AXIS_RADIUS`] of one on screen.
pub fn infer_axis(screen: Screen<'_>, base: DVec3, cursor: Vec2) -> Option<(DVec3, Axis)> {
    let ray = screen.ray(cursor);
    Axis::ALL
        .into_iter()
        .filter_map(|axis| {
            let point = base + axis.dir() * closest_on_line(base, axis.dir(), &ray)?;
            let distance = screen.project(point)?.distance(cursor);
            (distance < AXIS_RADIUS).then_some((distance, point, axis))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, point, axis)| (point, axis))
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
        let screen = Screen::new(&camera, SIZE);
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
        let screen = Screen::new(&camera, SIZE);
        let near_corner = screen.project(DVec3::new(10.0, 10.0, 0.0)).unwrap() + Vec2::new(4.0, -4.0);
        let snap = snap(&model, screen, near_corner, None).unwrap();
        assert_eq!(snap.kind, SnapKind::Endpoint);
        assert!(snap.point.abs_diff_eq(DVec3::new(10.0, 10.0, 0.0), 1e-9));
    }

    #[test]
    fn snaps_to_midpoint_then_on_edge() {
        let model = square();
        let camera = top_camera();
        let screen = Screen::new(&camera, SIZE);
        // Near the bottom edge's midpoint (5, 0).
        let near_mid = screen.project(DVec3::new(5.0, 0.0, 0.0)).unwrap() + Vec2::new(5.0, 3.0);
        let mid = snap(&model, screen, near_mid, None).unwrap();
        assert_eq!(mid.kind, SnapKind::Midpoint);
        assert!(mid.point.abs_diff_eq(DVec3::new(5.0, 0.0, 0.0), 1e-9));
        // Further along the edge: on-edge, at the exact point on the edge.
        let along = screen.project(DVec3::new(7.5, 0.0, 0.0)).unwrap() + Vec2::new(0.0, 3.0);
        let on_edge = snap(&model, screen, along, None).unwrap();
        assert_eq!(on_edge.kind, SnapKind::OnEdge);
        assert!((on_edge.point.x - 7.5).abs() < 0.01 && on_edge.point.y.abs() < 1e-9, "{}", on_edge.point);
    }

    #[test]
    fn endpoints_beat_midpoints_on_short_edges() {
        let mut model = Model::default();
        model.add_rectangle(DVec3::ZERO, DVec3::X * 0.25, DVec3::Y * 10.0, DVec3::Z);
        let camera = top_camera();
        let screen = Screen::new(&camera, SIZE);
        // The short edge's midpoint and endpoints are all within the snap radius.
        let cursor = screen.project(DVec3::new(0.05, 0.0, 0.0)).unwrap();
        assert_eq!(snap(&model, screen, cursor, None).unwrap().kind, SnapKind::Endpoint);
    }

    #[test]
    fn excluded_geometry_is_not_snapped() {
        let model = square();
        let camera = top_camera();
        let screen = Screen::new(&camera, SIZE);
        let near_mid = screen.project(DVec3::new(5.0, 0.0, 0.0)).unwrap();
        assert_eq!(snap_feature(&model, screen, near_mid, |_| true), None);
    }

    #[test]
    fn falls_back_to_face_then_ground() {
        let model = square();
        let camera = top_camera();
        let screen = Screen::new(&camera, SIZE);
        let on_face = screen.project(DVec3::new(4.0, 6.0, 0.0)).unwrap();
        assert_eq!(snap(&model, screen, on_face, None).unwrap().kind, SnapKind::OnFace);
        let off_face = screen.project(DVec3::new(-5.0, 6.0, 0.0)).unwrap();
        let ground = snap(&model, screen, off_face, None).unwrap();
        assert_eq!(ground.kind, SnapKind::OnGround);
        assert!(ground.point.abs_diff_eq(DVec3::new(-5.0, 6.0, 0.0), 1e-3));
    }
}
