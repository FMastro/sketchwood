//! Exporting the drawing for other programs: OBJ (with an MTL file of material colors, for
//! renderers and other modelers) and STL (for 3D printers, CNC and CAM software).
//!
//! Everything visible is exported in world coordinates: loose geometry and every group and
//! component, with hidden tags left out. Faces become triangles wound counter-clockwise around
//! their front side. Model units are inches; either format can be scaled to millimeters.

use std::collections::HashMap;
use std::fmt::Write;

use glam::{DAffine3, DVec3};

use crate::{MaterialId, Scene};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportUnits {
    Inches,
    Millimeters,
}

impl ExportUnits {
    fn scale(self) -> f64 {
        match self {
            Self::Inches => 1.0,
            Self::Millimeters => 25.4,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Inches => "inches",
            Self::Millimeters => "millimeters",
        }
    }
}

/// The triangles of one group, component or the loose geometry.
struct Mesh {
    name: String,
    triangles: Vec<Triangle>,
}

struct Triangle {
    corners: [DVec3; 3],
    normal: DVec3,
    material: Option<MaterialId>,
}

/// Every visible mesh, named by its place in the drawing ("Table/Leg#2").
fn meshes(scene: &Scene) -> Vec<Mesh> {
    let mut out = Vec::new();
    collect(scene, &scene.root, "Model", &DAffine3::IDENTITY, None, &mut out);
    out.retain(|m| !m.triangles.is_empty());
    out
}

fn collect(
    scene: &Scene,
    container: &crate::Container,
    name: &str,
    transform: &DAffine3,
    paint: Option<MaterialId>,
    out: &mut Vec<Mesh>,
) {
    let model = &container.model;
    let mut mesh = Mesh { name: name.to_owned(), triangles: Vec::new() };
    for (_, face) in model.faces() {
        let normal = transform.transform_vector3(face.normal).normalize();
        for t in model.triangulate(face) {
            let mut corners = t.map(|p| transform.transform_point3(p));
            if (corners[1] - corners[0]).cross(corners[2] - corners[0]).dot(normal) < 0.0 {
                corners.swap(1, 2);
            }
            mesh.triangles.push(Triangle { corners, normal, material: face.material.or(paint) });
        }
    }
    out.push(mesh);
    for (_, inst) in container.instances() {
        if !scene.is_visible(inst) {
            continue;
        }
        let Some(def) = scene.definition(inst.definition) else { continue };
        let child = if name == "Model" { def.name.clone() } else { format!("{name}/{}", def.name) };
        collect(scene, &def.container, &child, &(*transform * inst.transform), inst.material.or(paint), out);
    }
}

/// A name safe for OBJ and MTL statements (no spaces).
fn token(name: &str) -> String {
    name.chars().map(|c| if c.is_whitespace() { '_' } else { c }).collect()
}

/// The drawing as an OBJ file referring to `mtl_file` for its materials, and that MTL file.
pub fn to_obj(scene: &Scene, units: ExportUnits, mtl_file: &str) -> (String, String) {
    let scale = units.scale();
    let material_name = |id: Option<MaterialId>| {
        id.and_then(|m| scene.material(m)).map_or_else(|| "Default".to_owned(), |m| token(&m.name))
    };

    let mut obj = String::new();
    let _ = writeln!(obj, "# Exported from SketchCraft; units: {}", units.name());
    let _ = writeln!(obj, "mtllib {mtl_file}");
    let mut used = std::collections::BTreeSet::new();
    let mut next_vertex = 1usize;
    for mesh in meshes(scene) {
        let _ = writeln!(obj, "o {}", token(&mesh.name));
        // Corners shared by triangles of this mesh are written once.
        let mut index: HashMap<[u64; 3], usize> = HashMap::new();
        let mut faces = String::new();
        let mut current = None;
        for t in &mesh.triangles {
            let name = material_name(t.material);
            if current.as_ref() != Some(&name) {
                let _ = writeln!(faces, "usemtl {name}");
                current = Some(name.clone());
                used.insert(name);
            }
            let ids = t.corners.map(|p| {
                let p = p * scale;
                *index.entry(p.to_array().map(f64::to_bits)).or_insert_with(|| {
                    let _ = writeln!(obj, "v {} {} {}", p.x, p.y, p.z);
                    next_vertex += 1;
                    next_vertex - 1
                })
            });
            let _ = writeln!(faces, "f {} {} {}", ids[0], ids[1], ids[2]);
        }
        obj.push_str(&faces);
    }

    let mut mtl = String::from("# Exported from SketchCraft\n");
    for name in used {
        let color = scene
            .materials()
            .find(|(_, m)| token(&m.name) == name)
            .map_or([250, 250, 246], |(_, m)| m.color);
        let [r, g, b] = color.map(|c| f64::from(c) / 255.0);
        let _ = writeln!(mtl, "newmtl {name}\nKd {r:.4} {g:.4} {b:.4}\n");
    }
    (obj, mtl)
}

/// The drawing as an ASCII STL file: one solid of triangles with their normals.
pub fn to_stl(scene: &Scene, units: ExportUnits) -> String {
    let scale = units.scale();
    let mut stl = String::from("solid sketchcraft\n");
    for mesh in meshes(scene) {
        for t in &mesh.triangles {
            let n = t.normal;
            let _ = writeln!(stl, "  facet normal {} {} {}\n    outer loop", n.x, n.y, n.z);
            for p in t.corners {
                let p = p * scale;
                let _ = writeln!(stl, "      vertex {} {} {}", p.x, p.y, p.z);
            }
            let _ = writeln!(stl, "    endloop\n  endfacet");
        }
    }
    stl.push_str("endsolid sketchcraft\n");
    stl
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Document, MaterialKind};

    /// A 2x4 board as a group, painted, plus a loose square.
    fn drawing() -> Document {
        let mut doc = Document::default();
        let face = doc.edit(|m| m.add_rectangle(DVec3::ZERO, DVec3::X * 1.5, DVec3::Y * 3.5, DVec3::Z)).unwrap();
        let board = doc.push_pull(face, 10.0, true).unwrap().unwrap();
        let pine = doc.add_material(MaterialKind::DimensionalLumber);
        doc.paint(&[crate::Entity::Instance(board)], Some(pine));
        doc.edit(|m| m.add_rectangle(DVec3::new(20.0, 0.0, 0.0), DVec3::X, DVec3::Y, DVec3::Z));
        doc
    }

    #[test]
    fn stl_has_every_triangle_facing_out_in_millimeters() {
        let doc = drawing();
        let stl = to_stl(doc.scene(), ExportUnits::Millimeters);
        assert!(stl.starts_with("solid sketchcraft") && stl.trim_end().ends_with("endsolid sketchcraft"));
        // The board's 6 faces and the square: 7 rectangles, 14 triangles.
        assert_eq!(stl.matches("facet normal").count(), 14);
        // Every triangle winds counter-clockwise around its stated normal.
        let numbers = |line: &str| -> DVec3 {
            let v: Vec<f64> = line.split_whitespace().rev().take(3).map(|s| s.parse().unwrap()).collect();
            DVec3::new(v[2], v[1], v[0])
        };
        let lines: Vec<&str> = stl.lines().collect();
        let far_corner = DVec3::new(38.1, 88.9, 254.0);
        assert!(
            lines.iter().filter(|l| l.trim_start().starts_with("vertex")).any(|l| numbers(l).abs_diff_eq(far_corner, 1e-9)),
            "inches scaled to mm"
        );
        for (i, line) in lines.iter().enumerate().filter(|(_, l)| l.contains("facet normal")) {
            let n = numbers(line);
            let [a, b, c] = [numbers(lines[i + 2]), numbers(lines[i + 3]), numbers(lines[i + 4])];
            assert!((b - a).cross(c - a).dot(n) > 0.0);
        }
    }

    #[test]
    fn obj_names_parts_and_uses_their_materials() {
        let doc = drawing();
        let (obj, mtl) = to_obj(doc.scene(), ExportUnits::Inches, "bench.mtl");
        assert!(obj.contains("mtllib bench.mtl"));
        assert!(obj.contains("o Model") && obj.contains("o Group#1"));
        assert!(obj.contains("usemtl 2x4") || obj.lines().any(|l| l.starts_with("usemtl") && !l.ends_with("Default")));
        // The board's 8 corners and the square's 4, each written once.
        assert_eq!(obj.lines().filter(|l| l.starts_with("v ")).count(), 12);
        assert_eq!(obj.lines().filter(|l| l.starts_with("f ")).count(), 14);
        assert!(mtl.contains("newmtl Default") && mtl.matches("newmtl").count() == 2);
    }

    #[test]
    fn hidden_tags_are_left_out() {
        let mut doc = drawing();
        let (board, _) = doc.container().instances().next().unwrap();
        doc.selection_mut().extend([crate::Entity::Instance(board)]);
        let tag = doc.add_tag();
        doc.tag_selection(Some(tag));
        doc.set_tag_visible(tag, false);
        assert_eq!(to_stl(doc.scene(), ExportUnits::Inches).matches("facet normal").count(), 2);
    }
}
