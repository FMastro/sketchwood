//! Drawing loose edges (the Line tool), with SketchUp's rules:
//!
//! - A new edge splits the edges it crosses or touches, and is split by them, so drawing
//!   always leaves a connected graph.
//! - An edge across a face splits the face in two; a loop drawn inside a face makes a new face
//!   there (cut into the old one as a hole).
//! - Edges that close a flat loop in free space make a face.
//! - Erasing an edge between two coplanar faces merges them back into one.
//!
//! Faces are rebuilt one plane at a time: the plane's edges form a planar graph whose minimal
//! cycles are the possible faces. Cycles matching an existing face keep it (same id); the rest
//! replace the faces they fall in, keeping their side and paint.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use glam::{DVec2, DVec3};

use crate::model::{Location, locate, same_loop};
use crate::{EPS, EdgeId, Face, FaceId, Model, VertexId};

/// How close, in inches, a point must be to a line or plane to count as on it.
const TOL: f64 = 1e-5;

impl Model {
    /// Draw the edge from `a` to `b`, splitting it and the edges it meets, and splitting or
    /// making faces. Returns the edges the line became, in order from `a`.
    pub fn add_line(&mut self, a: DVec3, b: DVec3) -> Vec<EdgeId> {
        self.add_line_facing(a, b, None)
    }

    /// Like [`Self::add_line`]; a face the line closes in free space faces `facing` (when it
    /// isn't edge-on to it).
    pub(crate) fn add_line_facing(&mut self, a: DVec3, b: DVec3, facing: Option<DVec3>) -> Vec<EdgeId> {
        let length = a.distance(b);
        if length <= EPS {
            return Vec::new();
        }
        let dir = (b - a) / length;

        // Where the new line must be cut (distance along it), and where existing edges must be.
        let mut cuts: Vec<(f64, DVec3)> = vec![(0.0, a), (length, b)];
        let mut splits: Vec<DVec3> = Vec::new();
        for (_, edge) in self.edges() {
            let [c, d] = self.edge_points(edge);
            for p in [c, d] {
                let t = (p - a).dot(dir);
                if t > TOL && t < length - TOL && (a + dir * t).distance(p) <= TOL {
                    cuts.push((t, p));
                }
            }
            for p in [a, b] {
                if strictly_on_segment(p, c, d) {
                    splits.push(p);
                }
            }
            if let Some((t, p)) = crossing(a, dir, length, c, d) {
                cuts.push((t, p));
                splits.push(p);
            }
        }
        for p in splits {
            let on = self.edges().find(|(_, e)| {
                let [c, d] = self.edge_points(e);
                strictly_on_segment(p, c, d)
            });
            if let Some((id, _)) = on {
                self.split_edge(id, p);
            }
        }

        cuts.sort_by(|x, y| x.0.total_cmp(&y.0));
        cuts.dedup_by(|x, y| x.0 - y.0 <= TOL);
        let ids: Vec<VertexId> = cuts.iter().map(|&(_, p)| self.add_vertex(p)).collect();
        let mut pieces = Vec::new();
        let mut new = Vec::new();
        for pair in ids.windows(2) {
            let existed = self.find_edge(pair[0], pair[1]);
            if let Some(id) = self.add_edge(pair[0], pair[1]) {
                pieces.push(id);
                if existed.is_none() {
                    new.push(id);
                }
            }
        }
        self.heal_faces(&new, facing);
        pieces
    }

    /// Draw a closed polygon as lines, the way the Rectangle tool does: it splits faces and edges
    /// it crosses and is split by them. A face it closes in free space faces `facing`. Returns
    /// the face filling exactly the polygon, if one formed (not when it partly overlaps a face).
    pub fn draw_polygon(&mut self, points: &[DVec3], facing: DVec3) -> Option<FaceId> {
        let distinct = points.iter().enumerate().all(|(i, p)| points[i + 1..].iter().all(|q| p.distance(*q) > EPS));
        if points.len() < 3 || !distinct || crate::model::newell_normal(points).length() <= EPS {
            return None;
        }
        let n = points.len();
        for i in 0..n {
            self.add_line_facing(points[i], points[(i + 1) % n], Some(facing));
        }
        let corners: Vec<VertexId> = points.iter().map(|&p| self.vertex_at(p)).collect::<Option<_>>()?;
        let on_outline = |p: DVec3| {
            (0..n).any(|i| {
                let (a, b) = (points[i], points[(i + 1) % n]);
                p.distance(a) <= TOL || p.distance(b) <= TOL || strictly_on_segment(p, a, b)
            })
        };
        self.faces()
            .find(|(_, f)| {
                corners.iter().all(|c| f.outer.contains(c)) && f.outer.iter().all(|&v| on_outline(self.vertex(v)))
            })
            .map(|(id, _)| id)
    }

    /// [`Self::draw_polygon`] for the rectangle `corner`, `corner + u`, `corner + u + v`,
    /// `corner + v`.
    pub fn draw_rectangle(&mut self, corner: DVec3, u: DVec3, v: DVec3, facing: DVec3) -> Option<FaceId> {
        self.draw_polygon(&[corner, corner + u, corner + u + v, corner + v], facing)
    }

    /// Merge the two coplanar faces on either side of `edge` by erasing it. Returns `false`
    /// (changing nothing) unless the edge separates exactly two such faces.
    pub(crate) fn merge_across(&mut self, edge: EdgeId) -> bool {
        let faces = self.edge_faces(edge);
        let [f, g] = faces[..] else { return false };
        let (Some(ff), Some(gf)) = (self.face(f), self.face(g)) else { return false };
        let normal = ff.normal;
        let origin = self.vertex(ff.outer[0]);
        let coplanar = gf.normal.dot(normal) > 1.0 - 1e-9 && (self.vertex(gf.outer[0]) - origin).dot(normal).abs() <= TOL;
        if !coplanar {
            return false;
        }
        self.remove_edge(edge);
        self.rebuild_plane(normal, origin, &[], &BTreeSet::from([f, g]), None);
        true
    }

    /// Rebuild faces in every plane a new edge could split or close a face in.
    fn heal_faces(&mut self, new: &[EdgeId], facing: Option<DVec3>) {
        let mut planes: Vec<(DVec3, DVec3)> = Vec::new();
        for &id in new {
            let Some(&edge) = self.edge(id) else { continue };
            let [p, q] = self.edge_points(&edge);
            let dir = (q - p).normalize();
            let mut add = |n: DVec3| {
                if n.length() < 1e-9 {
                    return;
                }
                let n = n.normalize();
                if !planes.iter().any(|(m, o)| m.dot(n).abs() > 1.0 - 1e-9 && (p - *o).dot(*m).abs() <= TOL) {
                    planes.push((n, p));
                }
            };
            for (_, other) in self.edges() {
                let shares = [other.a, other.b].iter().any(|v| *v == edge.a || *v == edge.b);
                if shares {
                    let [c, d] = self.edge_points(other);
                    add(dir.cross(d - c));
                }
            }
            for (_, face) in self.faces() {
                let o = self.vertex(face.outer[0]);
                if [p, q].iter().all(|x| (*x - o).dot(face.normal).abs() <= TOL) {
                    add(face.normal);
                }
            }
        }
        for (normal, origin) in planes {
            let on_plane = |p: DVec3| (p - origin).dot(normal).abs() <= TOL;
            let new_here: Vec<EdgeId> = new
                .iter()
                .copied()
                .filter(|&e| self.edge(e).is_some_and(|e| self.edge_points(e).iter().all(|&p| on_plane(p))))
                .collect();
            // Faces a new edge runs across get rebuilt.
            let affected: BTreeSet<FaceId> = self
                .faces()
                .filter(|(_, f)| self.in_plane(f, normal, origin))
                .filter(|(_, f)| {
                    new_here.iter().filter_map(|&e| self.edge(e)).any(|e| {
                        let [p, q] = self.edge_points(e);
                        self.face_contains(f, (p + q) * 0.5)
                    })
                })
                .map(|(id, _)| id)
                .collect();
            self.rebuild_plane(normal, origin, &new_here, &affected, facing);
        }
    }

    fn in_plane(&self, face: &Face, normal: DVec3, origin: DVec3) -> bool {
        face.normal.dot(normal).abs() > 1.0 - 1e-9 && (self.vertex(face.outer[0]) - origin).dot(normal).abs() <= TOL
    }

    /// Recompute the faces of one plane. `affected` faces are replaced by the regions inside
    /// them; regions bounded by a `new` edge outside every face become new faces.
    fn rebuild_plane(
        &mut self,
        normal: DVec3,
        origin: DVec3,
        new: &[EdgeId],
        affected: &BTreeSet<FaceId>,
        facing: Option<DVec3>,
    ) {
        let u = normal.any_orthonormal_vector();
        let v = normal.cross(u);
        let flat = |p: DVec3| DVec2::new((p - origin).dot(u), (p - origin).dot(v));
        let lift = |p: DVec2| origin + u * p.x + v * p.y;
        let on_plane = |p: DVec3| (p - origin).dot(normal).abs() <= TOL;

        let edges: Vec<(VertexId, VertexId)> = self
            .edges()
            .filter(|(_, e)| on_plane(self.vertex(e.a)) && on_plane(self.vertex(e.b)))
            .map(|(_, e)| (e.a, e.b))
            .collect();
        let points: HashMap<VertexId, DVec2> =
            edges.iter().flat_map(|&(a, b)| [a, b]).map(|id| (id, flat(self.vertex(id)))).collect();
        let regions = regions(&edges, &points);

        let new_edges: BTreeSet<(VertexId, VertexId)> = new
            .iter()
            .filter_map(|&e| self.edge(e))
            .flat_map(|e| [(e.a, e.b), (e.b, e.a)])
            .collect();
        let old: Vec<FaceId> =
            self.faces().filter(|(_, f)| self.in_plane(f, normal, origin)).map(|(id, _)| id).collect();

        let mut kept = BTreeSet::new();
        let mut created: Vec<Face> = Vec::new();
        for region in regions {
            let Some(sample) = interior_point(&region, &points) else { continue };
            let sample = lift(sample);
            let exact = old.iter().copied().find(|&id| {
                let f = self.face(id).expect("live");
                same_loop(&f.outer, &region.outer)
                    && f.holes.len() == region.holes.len()
                    && region.holes.iter().all(|h| f.holes.iter().any(|fh| same_loop(fh, h)))
            });
            if let Some(id) = exact {
                kept.insert(id);
                continue;
            }
            let container = old.iter().copied().find(|&id| self.face_contains(self.face(id).expect("live"), sample));
            let (side, material) = match container {
                Some(id) if affected.contains(&id) => {
                    let f = self.face(id).expect("live");
                    (f.normal, f.material)
                }
                Some(_) => continue,
                None => {
                    let bounded_by_new = (0..region.outer.len())
                        .any(|i| new_edges.contains(&(region.outer[i], region.outer[(i + 1) % region.outer.len()])));
                    // Inside another face's outline but not on it: an opening, left open.
                    let in_opening = old.iter().any(|&id| {
                        let f = self.face(id).expect("live");
                        let outline: Vec<DVec2> = f.outer.iter().map(|&p| flat(self.vertex(p))).collect();
                        locate(flat(sample), &outline) == Location::Inside
                    });
                    if !bounded_by_new || in_opening {
                        continue;
                    }
                    (preferred_side(normal, facing), None)
                }
            };
            // Regions wind counter-clockwise around `normal`; faces wind around their front.
            let mut face = Face { outer: region.outer, holes: region.holes, normal: side, material };
            if side.dot(normal) < 0.0 {
                face.outer.reverse();
                face.holes.iter_mut().for_each(|h| h.reverse());
            }
            created.push(face);
        }

        // Each replaced face hands its id (and side and paint) on to the region holding its
        // middle, so one half of a split, or a merged face, keeps its identity.
        let polygon = |ids: &[VertexId]| -> Vec<DVec2> { ids.iter().map(|&v| flat(self.vertex(v))).collect() };
        let mut replaced: Vec<(FaceId, f64, DVec2)> = affected
            .iter()
            .filter(|id| !kept.contains(id))
            .filter_map(|&id| {
                let face = self.face(id)?;
                let triangles = self.triangulate(face);
                let area: f64 = triangles.iter().map(|[a, b, c]| (*b - *a).cross(*c - *a).length()).sum();
                let middle = triangles
                    .iter()
                    .max_by(|x, y| tri_area(&x.map(flat)).total_cmp(&tri_area(&y.map(flat))))
                    .map(|[a, b, c]| flat((*a + *b + *c) / 3.0))?;
                Some((id, area, middle))
            })
            .collect();
        replaced.sort_by(|x, y| y.1.total_cmp(&x.1));
        let mut reuse: Vec<Option<FaceId>> = vec![None; created.len()];
        for &(id, _, middle) in &replaced {
            let holder = created.iter().enumerate().position(|(i, f)| {
                reuse[i].is_none()
                    && locate(middle, &polygon(&f.outer)) == Location::Inside
                    && f.holes.iter().all(|h| locate(middle, &polygon(h)) == Location::Outside)
            });
            if let Some(i) = holder {
                reuse[i] = Some(id);
            }
        }
        for &(id, _, _) in &replaced {
            if !reuse.contains(&Some(id)) {
                self.remove_face(id);
            }
        }
        for (face, id) in created.into_iter().zip(reuse) {
            match id {
                Some(id) => {
                    let old = self.face(id).expect("replaced faces stay live until here");
                    let mut face = face;
                    if face.normal.dot(old.normal) < 0.0 {
                        face.outer.reverse();
                        face.holes.iter_mut().for_each(|h| h.reverse());
                    }
                    face.normal = old.normal;
                    face.material = old.material;
                    self.replace_face(id, face);
                }
                None => {
                    self.insert_face(face);
                }
            }
        }
    }
}

/// The front side for a face made from loose edges: toward `facing` if given, else up, else
/// toward the front view (-Y), else toward the right view (+X).
fn preferred_side(normal: DVec3, facing: Option<DVec3>) -> DVec3 {
    if let Some(f) = facing.filter(|f| f.dot(normal).abs() > 1e-9) {
        return normal * f.dot(normal).signum();
    }
    let key = [normal.z, -normal.y, normal.x];
    let sign = key.iter().find(|c| c.abs() > 1e-9).map_or(1.0, |c| c.signum());
    normal * sign
}

/// Whether `p` lies on segment `cd`, away from its ends.
fn strictly_on_segment(p: DVec3, c: DVec3, d: DVec3) -> bool {
    let length = c.distance(d);
    if length <= EPS {
        return false;
    }
    let dir = (d - c) / length;
    let t = (p - c).dot(dir);
    t > TOL && t < length - TOL && (c + dir * t).distance(p) <= TOL
}

/// Where the segment from `a` (unit `dir`, `length`) properly crosses segment `cd`: away from
/// both segments' ends. Returns the distance along the first segment and the point.
fn crossing(a: DVec3, dir: DVec3, length: f64, c: DVec3, d: DVec3) -> Option<(f64, DVec3)> {
    let other = d - c;
    let other_length = other.length();
    if other_length <= EPS {
        return None;
    }
    let e = other / other_length;
    let b = dir.dot(e);
    let denom = 1.0 - b * b;
    if denom < 1e-12 {
        return None;
    }
    let w = a - c;
    let t = (b * e.dot(w) - dir.dot(w)) / denom;
    let s = (e.dot(w) - b * dir.dot(w)) / denom;
    let p = a + dir * t;
    let ok = t > TOL && t < length - TOL && s > TOL && s < other_length - TOL && p.distance(c + e * s) <= TOL;
    ok.then_some((t, p))
}

/// A face-shaped region of a planar graph: an outer loop and holes, all counter-clockwise.
struct Region {
    outer: Vec<VertexId>,
    holes: Vec<Vec<VertexId>>,
}

/// The bounded regions of the planar graph `edges` (vertices at `points`). Edges with the same
/// region on both sides (dangling lines, bridges between loops) don't bound anything and are
/// ignored, as in SketchUp.
fn regions(edges: &[(VertexId, VertexId)], points: &HashMap<VertexId, DVec2>) -> Vec<Region> {
    let mut edges: Vec<(VertexId, VertexId)> = edges.to_vec();
    let cycles = loop {
        let cycles = cycles(&edges, points);
        let mut bridges = BTreeSet::new();
        for cycle in &cycles {
            let used: BTreeSet<(VertexId, VertexId)> = cycle_edges(cycle).collect();
            for (a, b) in cycle_edges(cycle) {
                if used.contains(&(b, a)) {
                    bridges.insert((a.min(b), a.max(b)));
                }
            }
        }
        if bridges.is_empty() {
            break cycles;
        }
        edges.retain(|&(a, b)| !bridges.contains(&(a.min(b), a.max(b))));
    };

    // Connected components, to tell which outlines are holes in which loops.
    let mut component: BTreeMap<VertexId, usize> = BTreeMap::new();
    for (i, &(a, b)) in edges.iter().enumerate() {
        let (ca, cb) = (component.get(&a).copied(), component.get(&b).copied());
        match (ca, cb) {
            (None, None) => {
                component.insert(a, i);
                component.insert(b, i);
            }
            (Some(c), None) => {
                component.insert(b, c);
            }
            (None, Some(c)) => {
                component.insert(a, c);
            }
            (Some(x), Some(y)) if x != y => {
                for c in component.values_mut() {
                    if *c == y {
                        *c = x;
                    }
                }
            }
            _ => {}
        }
    }

    let polygon = |cycle: &[VertexId]| -> Vec<DVec2> { cycle.iter().map(|v| points[v]).collect() };
    let area = |cycle: &[VertexId]| signed_area(&polygon(cycle));
    let (loops, outlines): (Vec<Vec<VertexId>>, Vec<Vec<VertexId>>) =
        cycles.into_iter().filter(|c| area(c).abs() > EPS * EPS).partition(|c| area(c) > 0.0);

    let mut regions: Vec<Region> = loops.iter().map(|l| Region { outer: l.clone(), holes: Vec::new() }).collect();
    for outline in outlines {
        let c = component[&outline[0]];
        let probe = points[&outline[0]];
        let around = loops
            .iter()
            .enumerate()
            .filter(|(_, l)| component[&l[0]] != c && locate(probe, &polygon(l)) == Location::Inside)
            .min_by(|(_, x), (_, y)| area(x).total_cmp(&area(y)));
        if let Some((i, _)) = around {
            regions[i].holes.push(outline.into_iter().rev().collect());
        }
    }
    regions
}

/// Every cycle of the graph, tracing each edge once in each direction with the region on the
/// left. Bounded regions come out counter-clockwise; the outline of each connected piece
/// clockwise.
fn cycles(edges: &[(VertexId, VertexId)], points: &HashMap<VertexId, DVec2>) -> Vec<Vec<VertexId>> {
    let mut around: BTreeMap<VertexId, Vec<(f64, VertexId)>> = BTreeMap::new();
    for &(a, b) in edges {
        for (from, to) in [(a, b), (b, a)] {
            let d = points[&to] - points[&from];
            around.entry(from).or_default().push((d.y.atan2(d.x), to));
        }
    }
    for list in around.values_mut() {
        list.sort_by(|x, y| x.0.total_cmp(&y.0));
    }
    // After arriving at `v` from `u`, leave by the edge just clockwise of the way back.
    let next = |u: VertexId, v: VertexId| -> VertexId {
        let list = &around[&v];
        let back = list.iter().position(|&(_, w)| w == u).expect("edges go both ways");
        list[(back + list.len() - 1) % list.len()].1
    };

    let mut seen: BTreeSet<(VertexId, VertexId)> = BTreeSet::new();
    let mut cycles = Vec::new();
    for &(a, b) in edges {
        for start in [(a, b), (b, a)] {
            if seen.contains(&start) {
                continue;
            }
            let mut cycle = Vec::new();
            let (mut u, mut v) = start;
            while seen.insert((u, v)) {
                cycle.push(u);
                (u, v) = (v, next(u, v));
            }
            cycles.push(cycle);
        }
    }
    cycles
}

fn cycle_edges(cycle: &[VertexId]) -> impl Iterator<Item = (VertexId, VertexId)> + '_ {
    (0..cycle.len()).map(|i| (cycle[i], cycle[(i + 1) % cycle.len()]))
}

fn signed_area(polygon: &[DVec2]) -> f64 {
    (0..polygon.len()).map(|i| polygon[i].perp_dot(polygon[(i + 1) % polygon.len()])).sum::<f64>() * 0.5
}

/// A point well inside the region: the center of its largest triangle.
fn interior_point(region: &Region, points: &HashMap<VertexId, DVec2>) -> Option<DVec2> {
    let mut flat: Vec<DVec2> = region.outer.iter().map(|v| points[v]).collect();
    let mut hole_starts = Vec::new();
    for hole in &region.holes {
        hole_starts.push(flat.len() as u32);
        flat.extend(hole.iter().map(|v| points[v]));
    }
    let mut indices: Vec<u32> = Vec::new();
    earcut::Earcut::new().earcut(flat.iter().map(|p| p.to_array()), &hole_starts, &mut indices);
    indices
        .as_chunks::<3>()
        .0
        .iter()
        .map(|t| t.map(|i| flat[i as usize]))
        .max_by(|x, y| tri_area(x).total_cmp(&tri_area(y)))
        .map(|[a, b, c]| (a + b + c) / 3.0)
}

fn tri_area([a, b, c]: &[DVec2; 3]) -> f64 {
    (b - a).perp_dot(c - a).abs()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Entity;

    fn area(model: &Model, face: FaceId) -> f64 {
        model
            .triangulate(model.face(face).unwrap())
            .iter()
            .map(|[a, b, c]| (*b - *a).cross(*c - *a).length() * 0.5)
            .sum()
    }

    fn p(x: f64, y: f64) -> DVec3 {
        DVec3::new(x, y, 0.0)
    }

    fn square(model: &mut Model, size: f64) -> FaceId {
        model.add_rectangle(DVec3::ZERO, DVec3::X * size, DVec3::Y * size, DVec3::Z).unwrap()
    }

    /// Every face's loops wind counter-clockwise around its normal.
    fn assert_wound(model: &Model) {
        for (_, f) in model.faces() {
            let outer = model.loop_points(&f.outer);
            assert!(crate::model::newell_normal(&outer).dot(f.normal) > 0.0, "outer winds with the normal");
        }
    }

    #[test]
    fn a_line_across_a_face_splits_it() {
        let mut model = Model::default();
        let face = square(&mut model, 10.0);
        let pieces = model.add_line(p(4.0, 0.0), p(4.0, 10.0));
        assert_eq!(pieces.len(), 1);
        let faces: Vec<FaceId> = model.faces().map(|(id, _)| id).collect();
        assert_eq!(faces.len(), 2);
        assert!(model.face(face).is_some(), "one half keeps the old id");
        let mut areas: Vec<f64> = faces.iter().map(|&f| area(&model, f)).collect();
        areas.sort_by(f64::total_cmp);
        assert!((areas[0] - 40.0).abs() < 1e-9 && (areas[1] - 60.0).abs() < 1e-9, "{areas:?}");
        // 4 sides, two of them split, plus the new line.
        assert_eq!(model.edges().count(), 7);
        assert!(model.faces().all(|(_, f)| f.normal.abs_diff_eq(DVec3::Z, 1e-12)));
        assert_wound(&model);
    }

    #[test]
    fn split_halves_keep_paint_and_side() {
        let mut model = Model::default();
        let face = model.add_rectangle(DVec3::ZERO, DVec3::X * 10.0, DVec3::Y * 10.0, -DVec3::Z).unwrap();
        let paint = crate::MaterialId::for_tests(3);
        model.set_face_material(face, Some(paint));
        model.add_line(p(0.0, 5.0), p(10.0, 5.0));
        assert_eq!(model.faces().count(), 2);
        assert!(model.faces().all(|(_, f)| f.material == Some(paint) && f.normal.abs_diff_eq(-DVec3::Z, 1e-12)));
        assert_wound(&model);
    }

    #[test]
    fn a_dangling_line_on_a_face_leaves_it_whole() {
        let mut model = Model::default();
        let face = square(&mut model, 10.0);
        model.add_line(p(0.0, 5.0), p(5.0, 5.0));
        assert_eq!(model.faces().map(|(id, _)| id).collect::<Vec<_>>(), vec![face]);
        assert!((area(&model, face) - 100.0).abs() < 1e-9);
    }

    #[test]
    fn crossing_lines_split_each_other() {
        let mut model = Model::default();
        square(&mut model, 10.0);
        model.add_line(p(5.0, 0.0), p(5.0, 10.0));
        let pieces = model.add_line(p(0.0, 5.0), p(10.0, 5.0));
        assert_eq!(pieces.len(), 2, "cut where it crosses the first line");
        assert_eq!(model.faces().count(), 4);
        assert!(model.faces().all(|(id, _)| (area(&model, id) - 25.0).abs() < 1e-9));
        assert_wound(&model);
    }

    #[test]
    fn a_loop_drawn_on_a_face_becomes_a_face_in_a_hole() {
        let mut model = Model::default();
        let face = square(&mut model, 10.0);
        let corners = [p(2.0, 2.0), p(5.0, 2.0), p(5.0, 6.0), p(2.0, 6.0)];
        for i in 0..4 {
            model.add_line(corners[i], corners[(i + 1) % 4]);
        }
        assert_eq!(model.faces().count(), 2);
        let (outer, inner): (Vec<_>, Vec<_>) = model.faces().partition(|(_, f)| !f.holes.is_empty());
        assert!((area(&model, outer[0].0) - 88.0).abs() < 1e-9);
        assert!((area(&model, inner[0].0) - 12.0).abs() < 1e-9);
        assert_eq!(outer[0].0, face, "the surrounding face keeps its id");
        assert_wound(&model);
    }

    #[test]
    fn closing_a_loop_in_free_space_makes_a_face() {
        let mut model = Model::default();
        let corners = [p(0.0, 0.0), p(4.0, 0.0), p(4.0, 3.0)];
        model.add_line(corners[0], corners[1]);
        model.add_line(corners[1], corners[2]);
        assert_eq!(model.faces().count(), 0);
        model.add_line(corners[2], corners[0]);
        let (id, face) = model.faces().next().unwrap();
        assert!(face.normal.abs_diff_eq(DVec3::Z, 1e-12), "ground faces face up");
        assert!((area(&model, id) - 6.0).abs() < 1e-9);
        assert_wound(&model);
    }

    #[test]
    fn a_vertical_loop_faces_the_front() {
        let mut model = Model::default();
        let corners = [DVec3::ZERO, DVec3::X * 4.0, DVec3::new(4.0, 0.0, 3.0), DVec3::Z * 3.0];
        for i in 0..4 {
            model.add_line(corners[i], corners[(i + 1) % 4]);
        }
        let (_, face) = model.faces().next().unwrap();
        assert!(face.normal.abs_diff_eq(-DVec3::Y, 1e-12));
        assert_wound(&model);
    }

    #[test]
    fn a_triangle_on_a_squares_side_adds_a_face_and_keeps_the_square() {
        let mut model = Model::default();
        let face = square(&mut model, 10.0);
        model.add_line(p(10.0, 0.0), p(15.0, 5.0));
        model.add_line(p(15.0, 5.0), p(10.0, 10.0));
        assert_eq!(model.faces().count(), 2);
        assert!(model.face(face).is_some(), "the square keeps its id");
        assert!((area(&model, face) - 100.0).abs() < 1e-9);
    }

    #[test]
    fn a_line_through_an_opening_leaves_it_open() {
        let mut model = Model::default();
        square(&mut model, 10.0);
        let hole = model.add_rectangle(p(4.0, 4.0), DVec3::X * 2.0, DVec3::Y * 2.0, DVec3::Z).unwrap();
        model.erase([Entity::Face(hole)]);
        model.add_line(p(5.0, 0.0), p(5.0, 10.0));
        // Two halves either side of the line, each notched by the opening; no face in it.
        assert_eq!(model.faces().count(), 2);
        let total: f64 = model.faces().map(|(id, _)| area(&model, id)).sum();
        assert!((total - 96.0).abs() < 1e-9, "{total}");
    }

    #[test]
    fn a_line_on_a_boxs_top_splits_only_the_top() {
        let mut model = Model::default();
        let base = square(&mut model, 10.0);
        model.push_pull(base, 2.0).unwrap();
        assert_eq!(model.faces().count(), 6);
        model.add_line(DVec3::new(3.0, 0.0, 2.0), DVec3::new(3.0, 10.0, 2.0));
        assert_eq!(model.faces().count(), 7);
        assert_wound(&model);
    }

    #[test]
    fn erasing_a_splitting_line_merges_the_halves() {
        let mut model = Model::default();
        let face = square(&mut model, 10.0);
        let paint = crate::MaterialId::for_tests(1);
        model.set_face_material(face, Some(paint));
        let line = model.add_line(p(4.0, 0.0), p(4.0, 10.0))[0];
        assert_eq!(model.faces().count(), 2);
        model.erase([Entity::Edge(line)]);
        let faces: Vec<_> = model.faces().collect();
        assert_eq!(faces.len(), 1);
        assert_eq!(faces[0].1.material, Some(paint));
        assert!((area(&model, faces[0].0) - 100.0).abs() < 1e-9);
    }

    #[test]
    fn erasing_an_edge_between_perpendicular_faces_still_erases_them() {
        let mut model = Model::default();
        let base = square(&mut model, 10.0);
        model.push_pull(base, 2.0).unwrap();
        let edge = model.face_edges(base)[0];
        model.erase([Entity::Edge(edge)]);
        assert_eq!(model.faces().count(), 4);
    }

    #[test]
    fn overlapping_rectangles_split_into_three_faces() {
        let mut model = Model::default();
        let first = model.draw_rectangle(DVec3::ZERO, DVec3::X * 10.0, DVec3::Y * 10.0, DVec3::Z).unwrap();
        let second = model.draw_rectangle(p(6.0, 6.0), DVec3::X * 10.0, DVec3::Y * 10.0, DVec3::Z);
        assert_eq!(second, None, "no single face fills an overlapping rectangle");
        let mut areas: Vec<f64> = model.faces().map(|(id, _)| area(&model, id)).collect();
        areas.sort_by(f64::total_cmp);
        assert_eq!(areas.len(), 3);
        assert!((areas[0] - 16.0).abs() < 1e-9, "{areas:?}");
        assert!((areas[1] - 84.0).abs() < 1e-9 && (areas[2] - 84.0).abs() < 1e-9, "{areas:?}");
        assert!(model.face(first).is_some());
        assert!(model.faces().all(|(_, f)| f.normal.abs_diff_eq(DVec3::Z, 1e-12) && f.holes.is_empty()));
        assert_wound(&model);
    }

    #[test]
    fn a_drawn_rectangle_faces_the_way_asked() {
        let mut model = Model::default();
        let face = model.draw_rectangle(DVec3::ZERO, DVec3::X * 2.0, DVec3::Y * 3.0, -DVec3::Z).unwrap();
        assert!(model.face(face).unwrap().normal.abs_diff_eq(-DVec3::Z, 1e-12));
        assert!((area(&model, face) - 6.0).abs() < 1e-9);
        // Drawn again over itself: the same face.
        assert_eq!(model.draw_rectangle(p(2.0, 3.0), -DVec3::X * 2.0, -DVec3::Y * 3.0, DVec3::Z), Some(face));
        assert_eq!(model.faces().count(), 1);
        assert_wound(&model);
    }

    #[test]
    fn a_line_along_an_edge_splits_it_without_new_faces() {
        let mut model = Model::default();
        square(&mut model, 10.0);
        let pieces = model.add_line(p(2.0, 0.0), p(6.0, 0.0));
        assert_eq!(pieces.len(), 1);
        assert_eq!(model.faces().count(), 1);
        assert_eq!(model.edges().count(), 6);
        let (_, face) = model.faces().next().unwrap();
        assert_eq!(face.outer.len(), 6);
    }
}
