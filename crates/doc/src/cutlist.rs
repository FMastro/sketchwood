//! The cut list: every part in the drawing, grouped by material and size.
//!
//! A part is a visible group or component instance whose definition has faces. Groups that only
//! hold other groups are assemblies and are looked into. A part's size is its bounding box along
//! its own axes, sorted so thickness ≤ width ≤ length; the length is the grain direction (the
//! default OpenCutList uses too). Loose geometry isn't a part.

use std::collections::BTreeMap;


use crate::{Container, MaterialId, MaterialKind, Scene};

/// Sizes are compared to this precision (inches) when grouping identical parts.
const SIZE_PRECISION: f64 = 1.0 / 64.0;

/// One row of the cut list: identical parts.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Part {
    /// "A", "B", ... in list order.
    pub label: String,
    /// The definitions' names, e.g. "Leg" (several when different groups share a size).
    pub names: Vec<String>,
    pub count: usize,
    pub length: f64,
    pub width: f64,
    pub thickness: f64,
    /// Solid wood: whether it's glued up from narrower boards ([`crate::cutplan::glue_up`]).
    pub glue_up: crate::part::GlueUp,
    /// What reinforces a glue-up's seams.
    pub edge_joint: crate::part::EdgeJoint,
}

/// The parts made of one material.
#[derive(Clone, Debug, PartialEq)]
pub struct MaterialParts {
    pub material: Option<MaterialId>,
    /// The material's name, or "No material".
    pub name: String,
    pub kind: Option<MaterialKind>,
    pub grained: bool,
    /// The standard stock the material comes in.
    pub standard: crate::cutplan::Standard,
    pub parts: Vec<Part>,
}

impl MaterialParts {
    pub fn part_count(&self) -> usize {
        self.parts.iter().map(|p| p.count).sum()
    }

    /// Board feet (12" × 12" × 1"), the usual measure for solid wood.
    pub fn board_feet(&self) -> f64 {
        self.parts.iter().map(|p| p.count as f64 * p.length * p.width * p.thickness).sum::<f64>() / 144.0
    }

    /// Square feet of face area, the usual measure for sheet goods.
    pub fn square_feet(&self) -> f64 {
        self.parts.iter().map(|p| p.count as f64 * p.length * p.width).sum::<f64>() / 144.0
    }

    /// Linear feet, the usual measure for dimensional lumber.
    pub fn linear_feet(&self) -> f64 {
        self.parts.iter().map(|p| p.count as f64 * p.length).sum::<f64>() / 12.0
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CutList {
    pub materials: Vec<MaterialParts>,
    /// Faces of loose geometry at the top level, which aren't parts until grouped.
    pub loose_faces: usize,
}

impl CutList {
    /// Everything besides wood, for the shopping list: `(piece, how many)`, from the joints made
    /// (dowels, biscuits, loose tenons, screws) and the seams of reinforced glue-ups.
    pub fn hardware(&self, scene: &Scene) -> Vec<(String, u32)> {
        let mut total: BTreeMap<String, u32> = BTreeMap::new();
        for hardware in scene.joints.iter().filter_map(|j| j.hardware.as_ref()) {
            *total.entry(hardware.name.clone()).or_default() += hardware.count;
        }
        for material in self.materials.iter().filter(|m| m.kind == Some(MaterialKind::SolidWood)) {
            for part in &material.parts {
                let (Some(boards), Some((piece, spacing))) = (crate::cutplan::glue_up(part, &material.standard), part.edge_joint.piece(part.thickness)) else { continue };
                let seams = (boards - 1) * crate::part::EdgeJoint::per_seam(spacing, part.length);
                *total.entry(piece).or_default() += seams * part.count as u32;
            }
        }
        total.into_iter().collect()
    }

    /// How many parts (counting copies) have no material yet.
    pub fn needs_material(&self) -> usize {
        self.materials.iter().filter(|m| m.material.is_none()).map(MaterialParts::part_count).sum()
    }
}

/// Identical parts while grouping: their names, how many, and their exact size.
type Group = (Vec<String>, usize, [f64; 3]);
type GroupKey = (Option<MaterialId>, [i64; 3], crate::part::GlueUp, crate::part::EdgeJoint);

/// One part found in the drawing, before grouping.
struct Found {
    name: String,
    material: Option<MaterialId>,
    size: [f64; 3],
    glue_up: crate::part::GlueUp,
    edge_joint: crate::part::EdgeJoint,
}

pub fn cut_list(scene: &Scene) -> CutList {
    let mut found = Vec::new();
    collect(scene, &scene.root, None, glam::DMat3::IDENTITY, &mut found);

    // Group identical parts: same material, same rounded size.
    let key = |v: f64| (v / SIZE_PRECISION).round() as i64;
    // (material, rounded size) -> (names, count, exact size)
    let mut groups: BTreeMap<GroupKey, Group> = BTreeMap::new();
    for part in found {
        let size_key = part.size.map(key);
        let entry = groups.entry((part.material, size_key, part.glue_up, part.edge_joint)).or_insert_with(|| (Vec::new(), 0, part.size));
        if !entry.0.contains(&part.name) {
            entry.0.push(part.name);
        }
        entry.1 += 1;
    }

    let mut by_material: BTreeMap<Option<MaterialId>, Vec<Part>> = BTreeMap::new();
    for ((material, _, glue_up, edge_joint), (names, count, [thickness, width, length])) in groups {
        by_material.entry(material).or_default().push(Part { label: String::new(), names, count, length, width, thickness, glue_up, edge_joint });
    }

    let mut materials: Vec<MaterialParts> = by_material
        .into_iter()
        .map(|(material, mut parts)| {
            // Thickest, then longest, then widest first, like a lumber list.
            parts.sort_by(|a, b| {
                b.thickness.total_cmp(&a.thickness).then(b.length.total_cmp(&a.length)).then(b.width.total_cmp(&a.width))
            });
            let info = material.and_then(|m| scene.material(m));
            MaterialParts {
                material,
                name: info.map_or_else(|| "No material".to_owned(), |m| m.name.clone()),
                kind: info.map(|m| m.kind),
                grained: info.is_some_and(|m| m.grained),
                standard: info.map(|m| m.standard.clone()).unwrap_or_default(),
                parts,
            }
        })
        .collect();
    // Named materials alphabetically, "No material" last.
    materials.sort_by_key(|m| (m.material.is_none(), m.name.to_lowercase()));

    for (index, part) in materials.iter_mut().flat_map(|m| m.parts.iter_mut()).enumerate() {
        part.label = label(index);
    }
    CutList { materials, loose_faces: scene.root.model.faces().count() }
}
/// Walk instances, inheriting materials from enclosing groups.
/// Walk instances, inheriting materials from enclosing groups. `linear` is the enclosing
/// groups' combined rotation and scale: a scaled group's part is measured at its scaled size.
fn collect(scene: &Scene, container: &Container, inherited: Option<MaterialId>, linear: glam::DMat3, found: &mut Vec<Found>) {
    for (_, instance) in container.instances() {
        if !scene.is_visible(instance) {
            continue;
        }
        let Some(definition) = scene.definition(instance.definition) else { continue };
        let material = instance.material.or(inherited);
        let linear = linear * instance.transform.matrix3;
        let model = &definition.container.model;
        // A part: its own faces, measured along its grain (the longest side unless set).
        if let Some(extents) = crate::part::extents(definition, linear) {
            let [length, width, thickness] = crate::part::size(extents, &definition.part);
            found.push(Found {
                name: definition.name.clone(),
                // Unpainted parts can still have painted faces: use the most common one.
                material: material.or_else(|| most_common_face_material(model)),
                size: [thickness, width, length],
                glue_up: definition.part.glue_up,
                edge_joint: definition.part.edge_joint,
            });
        }
        collect(scene, &definition.container, material, linear, found);
    }
}

fn most_common_face_material(model: &crate::Model) -> Option<MaterialId> {
    let mut counts: BTreeMap<MaterialId, usize> = BTreeMap::new();
    for (_, face) in model.faces() {
        if let Some(m) = face.material {
            *counts.entry(m).or_default() += 1;
        }
    }
    counts.into_iter().max_by_key(|(_, n)| *n).map(|(m, _)| m)
}

/// Spreadsheet-style labels: A..Z, AA..AZ, BA...
fn label(mut index: usize) -> String {
    let mut label = String::new();
    loop {
        label.insert(0, (b'A' + (index % 26) as u8) as char);
        if index < 26 {
            break;
        }
        index = index / 26 - 1;
    }
    label
}

/// The cut list as CSV (one row per part group), lengths in decimal inches.
pub fn to_csv(list: &CutList) -> String {
    let mut csv = String::from("Material,Label,Name,Quantity,Length (in),Width (in),Thickness (in)\n");
    let quote = |s: &str| format!("\"{}\"", s.replace('"', "\"\""));
    for material in &list.materials {
        for part in &material.parts {
            csv.push_str(&format!(
                "{},{},{},{},{:.4},{:.4},{:.4}\n",
                quote(&material.name),
                part.label,
                quote(&part.names.join(", ")),
                part.count,
                part.length,
                part.width,
                part.thickness
            ));
        }
    }
    csv
}

#[cfg(test)]
mod tests {
    use glam::DVec3;
    use super::*;
    use crate::{DefinitionKind, Document, Entity};

    /// A board of `l` × `w` × `t` made with Push/Pull (auto-grouped) at `at`.
    pub(crate) fn board(doc: &mut Document, at: DVec3, l: f64, w: f64, t: f64) -> crate::InstanceId {
        let face = doc.edit(|m| m.add_rectangle(at, DVec3::X * w, DVec3::Y * t, DVec3::Z)).unwrap();
        doc.push_pull(face, l, true).unwrap().unwrap()
    }

    #[test]
    fn assemblies_list_their_parts_and_unpainted_ones_need_a_material() {
        let mut doc = Document::default();
        let oak = doc.add_material(MaterialKind::SolidWood);
        // A drawer: four boards grouped into an assembly (no faces of its own).
        let parts: Vec<Entity> = (0..4).map(|i| Entity::Instance(board(&mut doc, DVec3::X * (30.0 * f64::from(i)), 20.0, 6.0, 0.75))).collect();
        doc.paint(&parts[..1], Some(oak));
        doc.selection_mut().extend(parts.iter().copied());
        let drawer = doc.group_selection(DefinitionKind::Group).unwrap();
        doc.rename_instance(drawer, "Drawer");
        // A shelf made a component, used twice, unpainted.
        let shelf = board(&mut doc, DVec3::Y * 50.0, 30.0, 10.0, 0.75);
        doc.selection_mut().clear();
        doc.selection_mut().extend([Entity::Instance(shelf)]);
        let shelf = doc.group_selection(DefinitionKind::Component).unwrap();
        doc.transform_entities(&[Entity::Instance(shelf)], &[glam::DAffine3::from_translation(DVec3::Z * 12.0)], true).unwrap();

        let list = cut_list(doc.scene());
        let count = |name: &str| list.materials.iter().find(|m| m.name == name).map_or(0, MaterialParts::part_count);
        assert_eq!(count("Wood"), 1, "the painted drawer board");
        assert_eq!(count("No material"), 3 + 2, "three drawer boards and both shelves need a material");
        assert_eq!(list.needs_material(), 5);
        // Painting the assembly gives its unpainted boards its material.
        doc.selection_mut().clear();
        doc.paint(&[Entity::Instance(drawer)], Some(oak));
        assert_eq!(cut_list(doc.scene()).needs_material(), 2);
    }

    #[test]
    fn identical_boards_are_one_row_with_a_quantity() {
        let mut doc = Document::default();
        let ply = doc.add_material(MaterialKind::SheetGood);
        let mut boards = Vec::new();
        for i in 0..4 {
            boards.push(board(&mut doc, DVec3::X * (30.0 * f64::from(i)), 30.0, 18.0, 0.75));
        }
        // One odd one out.
        boards.push(board(&mut doc, DVec3::Y * 40.0, 24.0, 18.0, 0.75));
        let all: Vec<Entity> = boards.iter().map(|&b| Entity::Instance(b)).collect();
        doc.paint(&all, Some(ply));

        let list = cut_list(doc.scene());
        assert_eq!(list.materials.len(), 1);
        let plywood = &list.materials[0];
        assert_eq!(plywood.name, "Plywood");
        assert_eq!(plywood.parts.len(), 2);
        let sides = &plywood.parts[0];
        assert_eq!((sides.label.as_str(), sides.count), ("A", 4));
        assert_eq!((sides.length, sides.width, sides.thickness), (30.0, 18.0, 0.75));
        assert_eq!(plywood.parts[1].label, "B");
        assert_eq!(plywood.part_count(), 5);
        assert!((plywood.square_feet() - (4.0 * 30.0 * 18.0 + 24.0 * 18.0) / 144.0).abs() < 1e-9);
    }

    #[test]
    fn dimensions_follow_the_part_not_the_world() {
        let mut doc = Document::default();
        // A board lying flat and turned: 48" long along Y, 3.5" wide, 1.5" thick.
        let face = doc.edit(|m| m.add_rectangle(DVec3::ZERO, DVec3::Y * 48.0, DVec3::X * 3.5, -DVec3::Z)).unwrap();
        doc.push_pull(face, 1.5, true).unwrap();
        let part = &cut_list(doc.scene()).materials[0].parts[0];
        assert_eq!((part.length, part.width, part.thickness), (48.0, 3.5, 1.5));
    }

    #[test]
    fn assemblies_are_looked_into_and_materials_inherited() {
        let mut doc = Document::default();
        let oak = doc.add_material(MaterialKind::SolidWood);
        let a = board(&mut doc, DVec3::ZERO, 30.0, 1.75, 1.75);
        let b = board(&mut doc, DVec3::X * 10.0, 30.0, 1.75, 1.75);
        doc.selection_mut().extend([Entity::Instance(a), Entity::Instance(b)]);
        let legs = doc.group_selection(DefinitionKind::Group).unwrap();
        doc.paint(&[Entity::Instance(legs)], Some(oak));
        let list = cut_list(doc.scene());
        assert_eq!(list.materials.len(), 1);
        assert_eq!(list.materials[0].name, "Wood");
        assert_eq!(list.materials[0].parts[0].count, 2);
        assert!((list.materials[0].board_feet() - 2.0 * 30.0 * 1.75 * 1.75 / 144.0).abs() < 1e-9);
    }

    #[test]
    fn hidden_parts_and_loose_geometry_are_left_out() {
        let mut doc = Document::default();
        let a = board(&mut doc, DVec3::ZERO, 30.0, 3.5, 1.5);
        board(&mut doc, DVec3::X * 10.0, 20.0, 3.5, 1.5);
        doc.edit(|m| m.add_rectangle(DVec3::Y * 50.0, DVec3::X, DVec3::Y, DVec3::Z));
        let tag = doc.add_tag();
        doc.selection_mut().extend([Entity::Instance(a)]);
        doc.tag_selection(Some(tag));
        doc.set_tag_visible(tag, false);
        let list = cut_list(doc.scene());
        assert_eq!(list.materials[0].part_count(), 1);
        assert_eq!(list.materials[0].name, "No material");
        assert_eq!(list.loose_faces, 1);
    }

    #[test]
    fn labels_go_past_z() {
        assert_eq!(label(0), "A");
        assert_eq!(label(25), "Z");
        assert_eq!(label(26), "AA");
        assert_eq!(label(27), "AB");
        assert_eq!(label(52), "BA");
    }

    #[test]
    fn csv_has_a_row_per_part_group() {
        let mut doc = Document::default();
        board(&mut doc, DVec3::ZERO, 30.0, 3.5, 1.5);
        board(&mut doc, DVec3::X * 10.0, 30.0, 3.5, 1.5);
        let csv = to_csv(&cut_list(doc.scene()));
        let lines: Vec<&str> = csv.lines().collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[1], "\"No material\",A,\"Group#1, Group#2\",2,30.0000,3.5000,1.5000");
    }
}

#[cfg(test)]
mod hardware_tests {
    use glam::DVec3;

    use crate::board::Board;
    use crate::library::LIBRARY;
    use crate::part::{EdgeJoint, GlueUp};
    use crate::{Document, InstanceId};

    fn board(doc: &mut Document, origin: DVec3, length_dir: DVec3, width_dir: DVec3, [l, w, t]: [f64; 3]) -> InstanceId {
        let cherry = doc.add_preset(LIBRARY.iter().find(|p| p.name == "Cherry").unwrap());
        doc.add_board(&Board { origin, length_dir, width_dir, length: l, width: w, thickness: t }, Some(cherry)).unwrap()
    }

    #[test]
    fn joints_and_reinforced_glue_ups_add_up_to_a_hardware_list() {
        let mut doc = Document::default();
        // A 36" x 20" top glued up from 3 boards with biscuits: 2 seams x 4 biscuits.
        let top = board(&mut doc, DVec3::ZERO, DVec3::X, DVec3::Y, [36.0, 20.0, 0.75]);
        doc.edit_part(top, |p| p.edge_joint = EdgeJoint::Biscuits);
        // A rail doweled into a stile.
        let stile = board(&mut doc, DVec3::new(50.0, 0.0, 0.0), DVec3::Z, DVec3::X, [30.0, 3.0, 1.5]);
        let rail = board(&mut doc, DVec3::new(53.0, 0.0, 12.5), DVec3::X, -DVec3::Z, [20.0, 2.5, 1.5]);
        doc.dowels(rail, stile, crate::joinery::Dowels { count: 2, diameter: 0.375, depth: 1.0 }).unwrap();
        let list = super::cut_list(doc.scene());
        assert_eq!(list.hardware(doc.scene()), [("#20 biscuit".to_owned(), 8), ("3/8\" x 2\" dowel".to_owned(), 2)]);
        // Glued only: no biscuits; one board: no seams.
        doc.edit_part(top, |p| p.edge_joint = EdgeJoint::Glue);
        assert_eq!(super::cut_list(doc.scene()).hardware(doc.scene()).len(), 1);
        doc.edit_part(top, |p| {
            p.edge_joint = EdgeJoint::Dowels;
            p.glue_up = GlueUp::Boards(2);
        });
        assert!(super::cut_list(doc.scene()).hardware(doc.scene()).contains(&("3/8\" x 2\" dowel".to_owned(), 2 + 6)));
    }
}
