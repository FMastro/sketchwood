//! Moving and copying geometry.
//!
//! Moving translates the vertices of the moved entities. Faces connected to them but not moved
//! stretch, as in SketchUp, as long as they stay flat; a move that would bend or collapse a
//! face is refused.

use std::collections::BTreeSet;
use std::fmt;

use glam::{DQuat, DVec3};

use crate::model::newell_normal;
use crate::{EPS, Entity, FaceId, Model, VertexId};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MoveError {
    /// A connected face would stop being flat.
    WouldBendFace,
    /// A face or edge would shrink to nothing.
    WouldCollapse,
}

impl fmt::Display for MoveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::WouldBendFace => {
                "That would bend a connected face. Select the whole piece (triple-click) to move it."
            }
            Self::WouldCollapse => "That would collapse a face or edge",
        })
    }
}

impl std::error::Error for MoveError {}

impl Model {
    /// The vertices that moving `entities` moves.
    pub fn entity_vertices(&self, entities: impl IntoIterator<Item = Entity>) -> BTreeSet<VertexId> {
        let mut vertices = BTreeSet::new();
        for entity in entities {
            match entity {
                Entity::Edge(id) => vertices.extend(self.edge(id).into_iter().flat_map(|e| [e.a, e.b])),
                Entity::Face(id) => {
                    vertices.extend(self.face(id).into_iter().flat_map(|f| f.loops().flatten().copied()))
                }
            }
        }
        vertices
    }

    /// Translate `vertices` by `delta`, stretching connected faces. On error the model is
    /// unchanged.
    pub fn move_vertices(&mut self, vertices: &BTreeSet<VertexId>, delta: DVec3) -> Result<(), MoveError> {
        self.transform_vertices(vertices, |p| p + delta)
    }

    /// Rotate `vertices` by `angle` radians around the line through `center` along `axis`
    /// (counter-clockwise looking down the axis), stretching connected faces.
    pub fn rotate_vertices(
        &mut self,
        vertices: &BTreeSet<VertexId>,
        center: DVec3,
        axis: DVec3,
        angle: f64,
    ) -> Result<(), MoveError> {
        self.transform_vertices(vertices, rotation(center, axis, angle))
    }

    /// Map `vertices` through `transform`. On error the model is unchanged.
    pub fn transform_vertices(
        &mut self,
        vertices: &BTreeSet<VertexId>,
        transform: impl Fn(DVec3) -> DVec3,
    ) -> Result<(), MoveError> {
        let original: Vec<(VertexId, DVec3)> = vertices.iter().map(|&v| (v, self.vertex(v))).collect();
        for &(v, p) in &original {
            self.set_vertex(v, transform(p));
        }
        let result = self.check_after_move(vertices);
        if result.is_err() {
            for (v, p) in original {
                self.set_vertex(v, p);
            }
        }
        result
    }

    fn check_after_move(&mut self, moved: &BTreeSet<VertexId>) -> Result<(), MoveError> {
        if self.edges().any(|(_, e)| {
            (moved.contains(&e.a) || moved.contains(&e.b)) && self.vertex(e.a).distance(self.vertex(e.b)) <= EPS
        }) {
            return Err(MoveError::WouldCollapse);
        }
        let touched: Vec<FaceId> =
            self.faces().filter(|(_, f)| f.loops().flatten().any(|v| moved.contains(v))).map(|(id, _)| id).collect();
        let mut normals = Vec::with_capacity(touched.len());
        for &id in &touched {
            let face = self.face(id).expect("collected from live faces");
            let outer = self.loop_points(&face.outer);
            let normal = newell_normal(&outer);
            if normal.length() <= EPS {
                return Err(MoveError::WouldCollapse);
            }
            let normal = normal.normalize();
            let origin = outer[0];
            let scale = outer.iter().map(|p| p.distance(origin)).fold(1.0, f64::max);
            let flat = face.loops().flatten().all(|&v| (self.vertex(v) - origin).dot(normal).abs() <= 1e-9 * scale);
            if !flat {
                return Err(MoveError::WouldBendFace);
            }
            normals.push(normal);
        }
        // Stretched faces may have tilted.
        for (id, normal) in touched.into_iter().zip(normals) {
            self.set_face_normal(id, normal);
        }
        Ok(())
    }

    /// Copy `entities` offset by `delta`. Returns the copies (faces first, then loose edges).
    pub fn copy_entities(&mut self, entities: &[Entity], delta: DVec3) -> Vec<Entity> {
        self.copy_entities_with(entities, |p| p + delta)
    }

    /// Copy `entities` mapped through `transform`, which must preserve orientation (a move or
    /// rotation, not a mirror). Returns the copies.
    pub fn copy_entities_with(&mut self, entities: &[Entity], transform: impl Fn(DVec3) -> DVec3) -> Vec<Entity> {
        let faces: Vec<FaceId> =
            entities.iter().filter_map(|e| if let Entity::Face(id) = e { Some(*id) } else { None }).collect();
        let face_edges: BTreeSet<_> = faces.iter().flat_map(|&f| self.face_edges(f)).collect();
        let mut copies = Vec::new();
        for face in faces {
            let Some(face) = self.face(face).cloned() else { continue };
            let moved = |ids: &[VertexId]| -> Vec<DVec3> { ids.iter().map(|&v| transform(self.vertex(v))).collect() };
            let outer = moved(&face.outer);
            let holes: Vec<Vec<DVec3>> = face.holes.iter().map(|h| moved(h)).collect();
            if let Some(copy) = self.add_face_with_holes(&outer, &holes) {
                copies.push(Entity::Face(copy));
            }
        }
        // Face copies bring their edges; copy the remaining selected edges.
        for entity in entities {
            if let Entity::Edge(id) = *entity
                && !face_edges.contains(&id)
                && let Some(edge) = self.edge(id).copied()
            {
                let [a, b] = self.edge_points(&edge);
                let (a, b) = (self.add_vertex(transform(a)), self.add_vertex(transform(b)));
                if let Some(copy) = self.add_edge(a, b) {
                    copies.push(Entity::Edge(copy));
                }
            }
        }
        // Include the copied faces' edges, so the copy is selected like the original.
        // Each edge borders two faces of a solid; list it once.
        let copied_face_edges: BTreeSet<Entity> = copies
            .iter()
            .filter_map(|e| if let Entity::Face(id) = e { Some(*id) } else { None })
            .flat_map(|f| self.face_edges(f))
            .map(Entity::Edge)
            .collect();
        let wanted_edges = entities.iter().any(|e| matches!(e, Entity::Edge(id) if face_edges.contains(id)));
        if wanted_edges {
            copies.extend(copied_face_edges);
        }
        copies
    }
}

/// The rotation by `angle` radians around the line through `center` along `axis`.
pub fn rotation(center: DVec3, axis: DVec3, angle: f64) -> impl Fn(DVec3) -> DVec3 {
    let q = DQuat::from_axis_angle(axis.normalize(), angle);
    move |p| center + q * (p - center)
}

#[cfg(test)]
mod tests {
    use std::f64::consts::FRAC_PI_2;

    use super::*;

    #[test]
    fn rotating_a_board_a_quarter_turn_about_its_corner() {
        let mut model = Model::default();
        let entities = board(&mut model, DVec3::ZERO);
        let vertices = model.entity_vertices(entities);
        model.rotate_vertices(&vertices, DVec3::ZERO, DVec3::Z, FRAC_PI_2).unwrap();
        let (lo, hi) = model.bounds().unwrap();
        assert!(lo.abs_diff_eq(DVec3::new(-3.5, 0.0, 0.0), 1e-9), "{lo}");
        assert!(hi.abs_diff_eq(DVec3::new(0.0, 1.5, 96.0), 1e-9), "{hi}");
        // Faces turned with it: no face points along +-X any more except the former +-Y ones.
        assert_eq!(model.faces().filter(|(_, f)| f.normal.x.abs() > 0.999).count(), 2);
    }

    #[test]
    fn laying_a_board_down_about_red() {
        let mut model = Model::default();
        let entities = board(&mut model, DVec3::ZERO);
        let vertices = model.entity_vertices(entities);
        // A quarter turn about red tips the top toward -Y.
        model.rotate_vertices(&vertices, DVec3::ZERO, DVec3::X, FRAC_PI_2).unwrap();
        let (lo, hi) = model.bounds().unwrap();
        assert!(lo.abs_diff_eq(DVec3::new(0.0, -96.0, 0.0), 1e-9), "{lo}");
        assert!(hi.abs_diff_eq(DVec3::new(1.5, 0.0, 3.5), 1e-9), "{hi}");
    }

    #[test]
    fn rotated_copies_keep_volume_and_orientation() {
        let mut model = Model::default();
        let entities = board(&mut model, DVec3::new(10.0, 0.0, 0.0));
        let copies = model.copy_entities_with(&entities, rotation(DVec3::ZERO, DVec3::Z, FRAC_PI_2));
        assert_eq!(copies.iter().filter(|e| matches!(e, Entity::Face(_))).count(), 6);
        // Every copied face still points away from the copy's center.
        let center = DVec3::new(-1.75, 10.75, 48.0);
        for e in copies {
            if let Entity::Face(id) = e {
                let f = model.face(id).unwrap();
                assert!(f.normal.dot(model.vertex(f.outer[0]) - center) > 0.0);
            }
        }
    }

    fn board(model: &mut Model, at: DVec3) -> Vec<Entity> {
        let face = model.add_rectangle(at, DVec3::X * 1.5, DVec3::Y * 3.5, DVec3::Z).unwrap();
        model.push_pull(face, 96.0).unwrap();
        model.connected(Entity::Face(face)).into_iter().collect()
    }

    #[test]
    fn moving_a_whole_board_translates_it() {
        let mut model = Model::default();
        let entities = board(&mut model, DVec3::ZERO);
        let vertices = model.entity_vertices(entities.iter().copied());
        assert_eq!(vertices.len(), 8);
        model.move_vertices(&vertices, DVec3::new(10.0, 0.0, 5.0)).unwrap();
        let (lo, hi) = model.bounds().unwrap();
        assert_eq!(lo, DVec3::new(10.0, 0.0, 5.0));
        assert_eq!(hi, DVec3::new(11.5, 3.5, 101.0));
    }

    #[test]
    fn moving_a_top_face_sideways_stretches_into_a_parallelogram() {
        let mut model = Model::default();
        board(&mut model, DVec3::ZERO);
        let top = model.faces().find(|(_, f)| f.normal.z > 0.5).map(|(id, _)| id).unwrap();
        let vertices = model.entity_vertices([Entity::Face(top)]);
        model.move_vertices(&vertices, DVec3::X * 4.0).unwrap();
        // The left and right walls tilted; their normals follow.
        let tilted = model.faces().filter(|(_, f)| f.normal.z.abs() > 1e-3 && f.normal.z.abs() < 0.999).count();
        assert_eq!(tilted, 2);
    }

    #[test]
    fn moving_one_corner_up_is_refused() {
        let mut model = Model::default();
        board(&mut model, DVec3::ZERO);
        let corner: BTreeSet<VertexId> =
            model.vertices().filter(|(_, p)| *p == DVec3::new(0.0, 0.0, 96.0)).map(|(id, _)| id).collect();
        let before = model.bounds();
        assert_eq!(model.move_vertices(&corner, DVec3::Z * 3.0), Err(MoveError::WouldBendFace));
        assert_eq!(model.bounds(), before);
    }

    #[test]
    fn collapsing_is_refused() {
        let mut model = Model::default();
        board(&mut model, DVec3::ZERO);
        let top = model.faces().find(|(_, f)| f.normal.z > 0.5).map(|(id, _)| id).unwrap();
        let vertices = model.entity_vertices([Entity::Face(top)]);
        assert_eq!(model.move_vertices(&vertices, -DVec3::Z * 96.0), Err(MoveError::WouldCollapse));
    }

    #[test]
    fn copying_a_board_makes_an_independent_second_board() {
        let mut model = Model::default();
        let entities = board(&mut model, DVec3::ZERO);
        let copies = model.copy_entities(&entities, DVec3::X * 16.0);
        assert_eq!(model.faces().count(), 12);
        assert_eq!(model.edges().count(), 24);
        assert_eq!(copies.len(), 6 + 12);
        let (_, hi) = model.bounds().unwrap();
        assert_eq!(hi.x, 17.5);
        // Moving the copy leaves the original alone.
        let vertices = model.entity_vertices(copies);
        model.move_vertices(&vertices, DVec3::Y * 5.0).unwrap();
        // All 6 faces of the copy now start at y >= 5; the original (y <= 3.5) stayed put.
        assert_eq!(model.faces().filter(|(_, f)| model.vertex(f.outer[0]).y >= 5.0 - 1e-9).count(), 6);
        assert_eq!(model.faces().filter(|(_, f)| model.vertex(f.outer[0]).y <= 3.5 + 1e-9).count(), 6);
    }

    #[test]
    fn copied_faces_keep_their_orientation() {
        let mut model = Model::default();
        let entities = board(&mut model, DVec3::ZERO);
        let copies = model.copy_entities(&entities, DVec3::X * 16.0);
        let normals = |ids: &[Entity]| -> Vec<[i64; 3]> {
            let mut n: Vec<[i64; 3]> = ids
                .iter()
                .filter_map(|e| if let Entity::Face(id) = e { model.face(*id) } else { None })
                .map(|f| (f.normal * 1000.0).round().as_i64vec3().to_array())
                .collect();
            n.sort();
            n
        };
        assert_eq!(normals(&entities), normals(&copies));
    }
}
