//! Loose geometry, as in SketchUp: vertices, edges, and planar faces bounded by edges.
//!
//! Units are inches. Edges are shared between the faces they bound. A face drawn inside an
//! existing coplanar face is cut into it as a hole, the way SketchUp does. Not handled yet:
//! faces that partially overlap, and edges that split faces (both arrive with the Line tool).
use serde::{Deserialize, Serialize};

use std::collections::BTreeSet;

use glam::{DAffine3, DVec2, DVec3};

use crate::arena::Arena;
use crate::scene::{InstanceId, MaterialId};

/// Points closer than this, in inches, are the same point.
pub const EPS: f64 = 1e-6;

/// A vertex in a [`Model`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct VertexId(u32);

/// An edge in a [`Model`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct EdgeId(u32);

/// A face in a [`Model`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct FaceId(u32);

/// Anything the user can select. Edges and faces belong to a [`Model`]; instances (groups and
/// components) to the [`crate::Container`] around it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Entity {
    Edge(EdgeId),
    Face(FaceId),
    Instance(InstanceId),
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Edge {
    pub a: VertexId,
    pub b: VertexId,
}

impl Edge {
    pub fn connects(&self, a: VertexId, b: VertexId) -> bool {
        (self.a == a && self.b == b) || (self.a == b && self.b == a)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Face {
    /// Boundary vertices, counter-clockwise around `normal`.
    pub outer: Vec<VertexId>,
    pub holes: Vec<Vec<VertexId>>,
    /// Unit normal; the front side faces this way.
    pub normal: DVec3,
    /// Painted material; `None` shows the enclosing group's material, or the default.
    pub material: Option<MaterialId>,
}

impl Face {
    /// The outer loop followed by the holes.
    pub fn loops(&self) -> impl Iterator<Item = &[VertexId]> {
        std::iter::once(self.outer.as_slice()).chain(self.holes.iter().map(Vec::as_slice))
    }

    pub fn uses_vertex(&self, id: VertexId) -> bool {
        self.loops().any(|l| l.contains(&id))
    }

    pub fn uses_edge(&self, edge: &Edge) -> bool {
        self.loops().any(|l| loop_has_edge(l, edge))
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Model {
    vertices: Arena<DVec3>,
    edges: Arena<Edge>,
    faces: Arena<Face>,
}

impl Model {
    pub fn is_empty(&self) -> bool {
        self.edges.iter().next().is_none() && self.faces.iter().next().is_none()
    }

    pub fn vertex(&self, id: VertexId) -> DVec3 {
        *self.vertices.get(id.0).expect("vertex ids held by edges and faces stay live")
    }

    pub fn edge(&self, id: EdgeId) -> Option<&Edge> {
        self.edges.get(id.0)
    }

    pub fn face(&self, id: FaceId) -> Option<&Face> {
        self.faces.get(id.0)
    }

    pub fn contains(&self, entity: Entity) -> bool {
        match entity {
            Entity::Edge(id) => self.edge(id).is_some(),
            Entity::Face(id) => self.face(id).is_some(),
            Entity::Instance(_) => false,
        }
    }

    pub fn vertices(&self) -> impl Iterator<Item = (VertexId, DVec3)> + '_ {
        self.vertices.iter().map(|(i, p)| (VertexId(i), *p))
    }

    pub fn edges(&self) -> impl Iterator<Item = (EdgeId, &Edge)> {
        self.edges.iter().map(|(i, e)| (EdgeId(i), e))
    }

    pub fn faces(&self) -> impl Iterator<Item = (FaceId, &Face)> {
        self.faces.iter().map(|(i, f)| (FaceId(i), f))
    }

    pub fn entities(&self) -> impl Iterator<Item = Entity> + '_ {
        self.edges()
            .map(|(id, _)| Entity::Edge(id))
            .chain(self.faces().map(|(id, _)| Entity::Face(id)))
    }

    pub fn edge_points(&self, edge: &Edge) -> [DVec3; 2] {
        [self.vertex(edge.a), self.vertex(edge.b)]
    }

    pub fn loop_points(&self, ids: &[VertexId]) -> Vec<DVec3> {
        ids.iter().map(|&id| self.vertex(id)).collect()
    }

    pub fn bounds(&self) -> Option<(DVec3, DVec3)> {
        self.vertices().map(|(_, p)| (p, p)).reduce(|(lo, hi), (p, _)| (lo.min(p), hi.max(p)))
    }

    /// The ids the next vertex, edge and face will get. Everything added later has ids at or
    /// above these.
    pub(crate) fn next_ids(&self) -> (VertexId, EdgeId, FaceId) {
        (VertexId(self.vertices.next_id()), EdgeId(self.edges.next_id()), FaceId(self.faces.next_id()))
    }

    /// Copy all of `other`'s geometry in, mapped through `transform` (a rigid motion), without
    /// merging it with anything already here.
    pub(crate) fn append(&mut self, other: &Model, transform: &DAffine3) {
        let mut vertex_map = std::collections::HashMap::new();
        for (id, p) in other.vertices() {
            vertex_map.insert(id, VertexId(self.vertices.insert(transform.transform_point3(p))));
        }
        let map = |ids: &[VertexId]| -> Vec<VertexId> { ids.iter().map(|v| vertex_map[v]).collect() };
        for (_, edge) in other.edges() {
            self.edges.insert(Edge { a: vertex_map[&edge.a], b: vertex_map[&edge.b] });
        }
        for (_, face) in other.faces() {
            self.faces.insert(Face {
                outer: map(&face.outer),
                holes: face.holes.iter().map(|h| map(h)).collect(),
                normal: transform.transform_vector3(face.normal).normalize(),
                material: face.material,
            });
        }
    }

    /// The vertex at `point`, creating it if none is within [`EPS`].
    pub fn add_vertex(&mut self, point: DVec3) -> VertexId {
        let existing = self.vertices().find(|(_, q)| q.distance_squared(point) <= EPS * EPS);
        match existing {
            Some((id, _)) => id,
            None => VertexId(self.vertices.insert(point)),
        }
    }

    pub fn find_edge(&self, a: VertexId, b: VertexId) -> Option<EdgeId> {
        self.edges().find(|(_, e)| e.connects(a, b)).map(|(id, _)| id)
    }

    /// The edge between `a` and `b`, creating it if needed. `None` if `a == b`.
    pub fn add_edge(&mut self, a: VertexId, b: VertexId) -> Option<EdgeId> {
        if a == b {
            return None;
        }
        Some(self.find_edge(a, b).unwrap_or_else(|| EdgeId(self.edges.insert(Edge { a, b }))))
    }

    /// Add a face bounded by `points`, a simple planar polygon, with its edges. The front side
    /// faces the direction `points` wind counter-clockwise around.
    ///
    /// Returns `None` for degenerate input (fewer than 3 distinct points, or zero area).
    pub fn add_face(&mut self, points: &[DVec3]) -> Option<FaceId> {
        let distinct = points.iter().enumerate().all(|(i, p)| {
            points[i + 1..].iter().all(|q| p.distance_squared(*q) > EPS * EPS)
        });
        let normal = newell_normal(points);
        if points.len() < 3 || !distinct || normal.length() <= EPS {
            return None;
        }
        let normal = normal.normalize();

        let ids: Vec<VertexId> = points.iter().map(|&p| self.add_vertex(p)).collect();
        for i in 0..ids.len() {
            self.add_edge(ids[i], ids[(i + 1) % ids.len()]);
        }
        if let Some((existing, _)) = self.faces().find(|(_, f)| same_loop(&f.outer, &ids)) {
            return Some(existing);
        }

        let mut face = Face { outer: ids.clone(), holes: Vec::new(), normal, material: None };
        let coplanar: Vec<FaceId> = self
            .faces()
            .filter(|(_, f)| {
                f.normal.dot(normal).abs() > 1.0 - 1e-9
                    && (self.vertex(f.outer[0]) - points[0]).dot(normal).abs() <= EPS
            })
            .map(|(id, _)| id)
            .collect();
        for other_id in coplanar {
            let other = self.face(other_id).expect("collected from live faces").clone();
            let other_outer = self.loop_points(&other.outer);
            let in_other = polygon_within(points, &other_outer, normal)
                && !other
                    .holes
                    .iter()
                    .any(|h| polygon_within(points, &self.loop_points(h), normal));
            if in_other {
                self.faces.get_mut(other_id.0).expect("live").holes.push(ids.clone());
            } else if polygon_within(&other_outer, points, normal) {
                face.holes.push(other.outer.clone());
            }
        }
        Some(FaceId(self.faces.insert(face)))
    }

    /// Like [`Self::add_face`], with holes: each a simple polygon inside `outer`.
    pub fn add_face_with_holes(&mut self, outer: &[DVec3], holes: &[Vec<DVec3>]) -> Option<FaceId> {
        let id = self.add_face(outer)?;
        for hole in holes {
            let ids: Vec<VertexId> = hole.iter().map(|&p| self.add_vertex(p)).collect();
            for i in 0..ids.len() {
                self.add_edge(ids[i], ids[(i + 1) % ids.len()]);
            }
            let face = self.faces.get_mut(id.0).expect("just added");
            if !face.holes.iter().any(|h| same_loop(h, &ids)) {
                face.holes.push(ids);
            }
        }
        Some(id)
    }

    /// Remove a face, keeping its edges.
    pub fn remove_face(&mut self, id: FaceId) -> Option<Face> {
        self.faces.remove(id.0)
    }

    /// Flip a face so its back becomes its front.
    pub fn reverse_face(&mut self, id: FaceId) {
        if let Some(face) = self.faces.get_mut(id.0) {
            face.outer.reverse();
            face.holes.iter_mut().for_each(|h| h.reverse());
            face.normal = -face.normal;
        }
    }

    pub(crate) fn set_vertex(&mut self, id: VertexId, point: DVec3) {
        if let Some(p) = self.vertices.get_mut(id.0) {
            *p = point;
        }
    }

    /// Paint a face (`None` goes back to the default / inherited material).
    pub fn set_face_material(&mut self, id: FaceId, material: Option<MaterialId>) {
        if let Some(face) = self.faces.get_mut(id.0) {
            face.material = material;
        }
    }

    /// Clear every face painted with `material`.
    pub(crate) fn forget_material(&mut self, material: MaterialId) {
        for face in self.faces.iter_mut() {
            if face.material == Some(material) {
                face.material = None;
            }
        }
    }

    pub(crate) fn set_face_normal(&mut self, id: FaceId, normal: DVec3) {
        if let Some(face) = self.faces.get_mut(id.0) {
            face.normal = normal;
        }
    }

    /// Move vertices by `delta`. Faces using them must stay planar; callers ensure that.
    pub fn translate_vertices(&mut self, ids: impl IntoIterator<Item = VertexId>, delta: DVec3) {
        for id in ids {
            if let Some(p) = self.vertices.get_mut(id.0) {
                *p += delta;
            }
        }
    }

    /// Add the rectangle `corner`, `corner + u`, `corner + u + v`, `corner + v`, with its
    /// front side facing `normal`.
    pub fn add_rectangle(
        &mut self,
        corner: DVec3,
        u: DVec3,
        v: DVec3,
        normal: DVec3,
    ) -> Option<FaceId> {
        let mut points = [corner, corner + u, corner + u + v, corner + v];
        if u.cross(v).dot(normal) < 0.0 {
            points.reverse();
        }
        self.add_face(&points)
    }

    /// Erase entities. Erasing an edge also erases the faces it bounds, and heals any hole it
    /// cut (SketchUp behavior). Vertices left without edges are removed.
    pub fn erase(&mut self, entities: impl IntoIterator<Item = Entity>) {
        for entity in entities {
            match entity {
                // Instances live in the container, not the model.
                Entity::Instance(_) => {}
                Entity::Face(id) => {
                    self.faces.remove(id.0);
                }
                Entity::Edge(id) => {
                    let Some(edge) = self.edges.remove(id.0) else { continue };
                    let bounded: Vec<FaceId> = self
                        .faces()
                        .filter(|(_, f)| loop_has_edge(&f.outer, &edge))
                        .map(|(id, _)| id)
                        .collect();
                    for face in bounded {
                        self.faces.remove(face.0);
                    }
                    for face in self.faces.iter_mut() {
                        face.holes.retain(|h| !loop_has_edge(h, &edge));
                    }
                }
            }
        }
        let used: BTreeSet<VertexId> = self.edges().flat_map(|(_, e)| [e.a, e.b]).collect();
        let orphans: Vec<u32> =
            self.vertices.iter().map(|(i, _)| i).filter(|&i| !used.contains(&VertexId(i))).collect();
        for i in orphans {
            self.vertices.remove(i);
        }
    }

    /// Every edge of every loop of `face`.
    pub fn face_edges(&self, face: FaceId) -> Vec<EdgeId> {
        let Some(face) = self.face(face) else { return Vec::new() };
        face.loops()
            .flat_map(|l| (0..l.len()).filter_map(|i| self.find_edge(l[i], l[(i + 1) % l.len()])))
            .collect()
    }

    /// Faces that `edge` bounds, including faces it cuts a hole in.
    pub fn edge_faces(&self, edge: EdgeId) -> Vec<FaceId> {
        let Some(edge) = self.edge(edge) else { return Vec::new() };
        self.faces().filter(|(_, f)| f.uses_edge(edge)).map(|(id, _)| id).collect()
    }

    /// Everything connected to `start` through shared vertices (SketchUp's triple-click).
    pub fn connected(&self, start: Entity) -> BTreeSet<Entity> {
        let mut reached: BTreeSet<VertexId> = match start {
            Entity::Edge(id) => self.edge(id).map(|e| [e.a, e.b].into()).unwrap_or_default(),
            Entity::Face(id) => self.face(id).map(|f| f.outer[..1].iter().copied().collect()).unwrap_or_default(),
            Entity::Instance(_) => BTreeSet::new(),
        };
        loop {
            let before = reached.len();
            for (_, e) in self.edges() {
                if reached.contains(&e.a) || reached.contains(&e.b) {
                    reached.extend([e.a, e.b]);
                }
            }
            if reached.len() == before {
                break;
            }
        }
        self.edges()
            .filter(|(_, e)| reached.contains(&e.a))
            .map(|(id, _)| Entity::Edge(id))
            .chain(
                self.faces()
                    .filter(|(_, f)| reached.contains(&f.outer[0]))
                    .map(|(id, _)| Entity::Face(id)),
            )
            .collect()
    }

    /// Whether `point`, assumed on the face's plane, is on the face (boundary included, holes
    /// excluded).
    pub fn face_contains(&self, face: &Face, point: DVec3) -> bool {
        let project = projector(face.normal);
        let p = project(point);
        let polygon = |ids: &[VertexId]| -> Vec<DVec2> {
            ids.iter().map(|&id| project(self.vertex(id))).collect()
        };
        locate(p, &polygon(&face.outer)) != Location::Outside
            && face.holes.iter().all(|h| locate(p, &polygon(h)) != Location::Inside)
    }

    /// The face as triangles, holes cut out.
    pub fn triangulate(&self, face: &Face) -> Vec<[DVec3; 3]> {
        let project = projector(face.normal);
        let mut points = Vec::new();
        let mut hole_starts: Vec<u32> = Vec::new();
        for (i, l) in face.loops().enumerate() {
            if i > 0 {
                hole_starts.push(points.len() as u32);
            }
            points.extend(self.loop_points(l));
        }
        let mut indices: Vec<u32> = Vec::new();
        earcut::Earcut::new().earcut(
            points.iter().map(|&p| project(p).to_array()),
            &hole_starts,
            &mut indices,
        );
        indices.as_chunks::<3>().0.iter().map(|t| t.map(|i| points[i as usize])).collect()
    }
}

/// Whether two loops use the same vertices (in any order or direction).
fn same_loop(a: &[VertexId], b: &[VertexId]) -> bool {
    a.len() == b.len() && a.iter().collect::<BTreeSet<_>>() == b.iter().collect::<BTreeSet<_>>()
}

fn loop_has_edge(ids: &[VertexId], edge: &Edge) -> bool {
    (0..ids.len()).any(|i| edge.connects(ids[i], ids[(i + 1) % ids.len()]))
}

/// Polygon normal by Newell's method; its length is twice the polygon's area.
pub(crate) fn newell_normal(points: &[DVec3]) -> DVec3 {
    let mut n = DVec3::ZERO;
    for (i, a) in points.iter().enumerate() {
        let b = points[(i + 1) % points.len()];
        n.x += (a.y - b.y) * (a.z + b.z);
        n.y += (a.z - b.z) * (a.x + b.x);
        n.z += (a.x - b.x) * (a.y + b.y);
    }
    n
}

/// Maps points on a plane with `normal` to 2D by dropping the dominant axis.
fn projector(normal: DVec3) -> impl Fn(DVec3) -> DVec2 {
    let a = normal.abs();
    move |p: DVec3| {
        if a.z >= a.x && a.z >= a.y {
            p.truncate()
        } else if a.x >= a.y {
            DVec2::new(p.y, p.z)
        } else {
            DVec2::new(p.x, p.z)
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Location {
    Inside,
    OnBoundary,
    Outside,
}

fn locate(p: DVec2, polygon: &[DVec2]) -> Location {
    let mut inside = false;
    for (i, &a) in polygon.iter().enumerate() {
        let b = polygon[(i + 1) % polygon.len()];
        let ab = b - a;
        let t = ((p - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0);
        if (a + ab * t).distance_squared(p) <= EPS * EPS {
            return Location::OnBoundary;
        }
        if (a.y > p.y) != (b.y > p.y) && p.x < a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x) {
            inside = !inside;
        }
    }
    if inside { Location::Inside } else { Location::Outside }
}

/// Whether the polygon `inner` lies within `outer` (touching the boundary is allowed). Both
/// are on a plane with `normal`.
fn polygon_within(inner: &[DVec3], outer: &[DVec3], normal: DVec3) -> bool {
    let project = projector(normal);
    let outer: Vec<DVec2> = outer.iter().map(|&p| project(p)).collect();
    (0..inner.len()).all(|i| {
        let a = project(inner[i]);
        let mid = (a + project(inner[(i + 1) % inner.len()])) * 0.5;
        locate(a, &outer) != Location::Outside && locate(mid, &outer) != Location::Outside
    })
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

    fn ground_rect(model: &mut Model, x: f64, y: f64, w: f64, h: f64) -> Option<FaceId> {
        model.add_rectangle(DVec3::new(x, y, 0.0), DVec3::X * w, DVec3::Y * h, DVec3::Z)
    }

    #[test]
    fn rectangle_faces_the_requested_normal() {
        let mut model = Model::default();
        let up = ground_rect(&mut model, 0.0, 0.0, 1.5, 3.5).unwrap();
        assert!(model.face(up).unwrap().normal.abs_diff_eq(DVec3::Z, 1e-12));
        // Negative extents flip the winding; the normal must still follow the request.
        let down = model
            .add_rectangle(DVec3::new(10.0, 0.0, 0.0), -DVec3::X * 2.0, DVec3::Y, -DVec3::Z)
            .unwrap();
        assert!(model.face(down).unwrap().normal.abs_diff_eq(-DVec3::Z, 1e-12));
    }

    #[test]
    fn two_by_four_profile_has_four_edges_and_area() {
        let mut model = Model::default();
        let face = ground_rect(&mut model, 0.0, 0.0, 1.5, 3.5).unwrap();
        assert_eq!(model.edges().count(), 4);
        assert_eq!(model.vertices().count(), 4);
        assert!((area(&model, face) - 5.25).abs() < 1e-9);
    }

    #[test]
    fn degenerate_rectangles_are_rejected() {
        let mut model = Model::default();
        assert!(ground_rect(&mut model, 0.0, 0.0, 0.0, 3.5).is_none());
        assert!(model.is_empty());
    }

    #[test]
    fn adjacent_rectangles_share_an_edge() {
        let mut model = Model::default();
        let a = ground_rect(&mut model, 0.0, 0.0, 1.5, 3.5).unwrap();
        let b = ground_rect(&mut model, 1.5, 0.0, 1.5, 3.5).unwrap();
        assert_eq!(model.edges().count(), 7);
        let shared = model.face_edges(a).into_iter().find(|e| model.face_edges(b).contains(e));
        assert_eq!(model.edge_faces(shared.unwrap()).len(), 2);
    }

    #[test]
    fn redrawing_a_face_returns_the_existing_one() {
        let mut model = Model::default();
        let a = ground_rect(&mut model, 0.0, 0.0, 4.0, 4.0).unwrap();
        let b = ground_rect(&mut model, 4.0, 4.0, -4.0, -4.0).unwrap();
        assert_eq!(a, b);
        assert_eq!(model.faces().count(), 1);
    }

    #[test]
    fn rectangle_inside_a_face_cuts_a_hole() {
        let mut model = Model::default();
        let outer = ground_rect(&mut model, 0.0, 0.0, 10.0, 10.0).unwrap();
        let inner = ground_rect(&mut model, 2.0, 2.0, 3.0, 3.0).unwrap();
        assert_eq!(model.face(outer).unwrap().holes.len(), 1);
        assert!((area(&model, outer) - 91.0).abs() < 1e-9);
        assert!((area(&model, inner) - 9.0).abs() < 1e-9);
        let outer_face = model.face(outer).unwrap();
        assert!(!model.face_contains(outer_face, DVec3::new(3.0, 3.0, 0.0)));
        assert!(model.face_contains(outer_face, DVec3::new(1.0, 1.0, 0.0)));
    }

    #[test]
    fn rectangle_around_a_face_holes_around_it() {
        let mut model = Model::default();
        let inner = ground_rect(&mut model, 2.0, 2.0, 3.0, 3.0).unwrap();
        let outer = ground_rect(&mut model, 0.0, 0.0, 10.0, 10.0).unwrap();
        assert_eq!(model.face(outer).unwrap().holes.len(), 1);
        assert!(model.face(inner).unwrap().holes.is_empty());
    }

    #[test]
    fn erasing_a_hole_edge_heals_the_outer_face() {
        let mut model = Model::default();
        let outer = ground_rect(&mut model, 0.0, 0.0, 10.0, 10.0).unwrap();
        let inner = ground_rect(&mut model, 2.0, 2.0, 3.0, 3.0).unwrap();
        let edge = model.face_edges(inner)[0];
        model.erase([Entity::Edge(edge)]);
        assert!(model.face(inner).is_none());
        assert!(model.face(outer).unwrap().holes.is_empty());
        assert_eq!(model.edges().count(), 7);
        assert_eq!(model.vertices().count(), 8);
    }

    #[test]
    fn erasing_a_face_keeps_its_edges() {
        let mut model = Model::default();
        let face = ground_rect(&mut model, 0.0, 0.0, 1.5, 3.5).unwrap();
        model.erase([Entity::Face(face)]);
        assert_eq!(model.faces().count(), 0);
        assert_eq!(model.edges().count(), 4);
    }

    #[test]
    fn erasing_all_edges_removes_vertices() {
        let mut model = Model::default();
        ground_rect(&mut model, 0.0, 0.0, 1.5, 3.5).unwrap();
        let all: Vec<Entity> = model.entities().collect();
        model.erase(all);
        assert!(model.is_empty());
        assert_eq!(model.vertices().count(), 0);
    }

    #[test]
    fn connected_stops_at_separate_geometry() {
        let mut model = Model::default();
        let a = ground_rect(&mut model, 0.0, 0.0, 1.0, 1.0).unwrap();
        ground_rect(&mut model, 1.0, 0.0, 1.0, 1.0).unwrap();
        ground_rect(&mut model, 5.0, 5.0, 1.0, 1.0).unwrap();
        // Two touching squares: 7 edges + 2 faces.
        assert_eq!(model.connected(Entity::Face(a)).len(), 9);
    }

    #[test]
    fn vertical_rectangle_triangulates() {
        let mut model = Model::default();
        let face = model
            .add_rectangle(DVec3::ZERO, DVec3::X * 3.5, DVec3::Z * 96.0, -DVec3::Y)
            .unwrap();
        assert!((area(&model, face) - 336.0).abs() < 1e-9);
    }
}
