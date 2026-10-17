//! Writes a sample drawing with furniture built from WoodSketch's own operations, for trying
//! the cut list, build sheets and wood movement checks:
//!
//! - a cherry side table: a glued-up top with a roundover; one chamfered leg component turned
//!   to each corner; front/back and side apron components tenoned into the legs;
//! - a red oak bookcase: shelves let into the sides (dados, rabbets at the top), a plywood back.
//!
//! `cargo run -p woodsketch-doc --example sample_furniture -- "Sample Furniture.woodsketch"`

use glam::{DAffine3, DVec3};
use woodsketch_doc::board::Board;
use woodsketch_doc::joinery::Tenon;
use woodsketch_doc::library::LIBRARY;
use woodsketch_doc::profile::{Edges, Profile};
use woodsketch_doc::{DefinitionKind, Document, Entity, InstanceId, MaterialId};

fn wood(doc: &mut Document, name: &str) -> MaterialId {
    doc.add_preset(LIBRARY.iter().find(|p| p.name == name).expect("a library wood"))
}

fn board(doc: &mut Document, material: MaterialId, name: &str, origin: DVec3, length_dir: DVec3, width_dir: DVec3, [length, width, thickness]: [f64; 3]) -> InstanceId {
    let id = doc.add_board(&Board { origin, length_dir, width_dir, length, width, thickness }, Some(material)).expect("a board");
    doc.rename_instance(id, name);
    id
}

/// Make `part` a component named `name`.
fn component(doc: &mut Document, part: InstanceId, name: &str) -> InstanceId {
    doc.selection_mut().clear();
    doc.selection_mut().extend([Entity::Instance(part)]);
    let id = doc.group_selection(DefinitionKind::Component).expect("a component");
    doc.rename_instance(id, name);
    doc.selection_mut().clear();
    id
}

/// Copies of `part` turned `degrees` about the vertical through `center`, then moved to `to`.
fn copies(doc: &mut Document, part: InstanceId, center: DVec3, spots: &[(DVec3, f64)]) -> Vec<InstanceId> {
    let transforms: Vec<DAffine3> = spots
        .iter()
        .map(|&(to, degrees)| DAffine3::from_translation(to) * DAffine3::from_rotation_z(degrees.to_radians()) * DAffine3::from_translation(-center))
        .collect();
    doc.transform_entities(&[Entity::Instance(part)], &transforms, true)
        .expect("copies")
        .into_iter()
        .filter_map(|e| if let Entity::Instance(i) = e { Some(i) } else { None })
        .collect()
}

fn side_table(doc: &mut Document) {
    let cherry = wood(doc, "Cherry");
    // One leg, 1 3/4" square and 29 1/4" tall, chamfered, made a component and turned to each
    // corner so its two inside faces (where the mortises go) always face in.
    let leg = board(doc, cherry, "Leg", DVec3::new(1.0, 1.0, 0.0), DVec3::Z, DVec3::X, [29.25, 1.75, 1.75]);
    doc.profile_edges(leg, Profile::Chamfer, 0.125, Edges::Along { axis: 0, corners: [true; 4] }).expect("chamfer");
    let front_left = component(doc, leg, "Leg");
    let corner = DVec3::new(1.875, 1.875, 0.0);
    let others = copies(doc, front_left, corner, &[(DVec3::new(34.125, 1.875, 0.0), 90.0), (DVec3::new(34.125, 18.125, 0.0), 180.0), (DVec3::new(1.875, 18.125, 0.0), 270.0)]);
    let (front_right, back_right, back_left) = (others[0], others[1], others[2]);

    // Aprons: 4" wide, 3/4" thick, set back 1/4" from the legs' outside faces. The back is a
    // copy of the front turned around, the right a copy of the left.
    let middle = DVec3::new(18.0, 10.0, 0.0);
    let front = board(doc, cherry, "Front apron", DVec3::new(2.75, 1.25, 29.25), DVec3::X, -DVec3::Z, [30.5, 4.0, 0.75]);
    let front = component(doc, front, "Front and back apron");
    let back = copies(doc, front, middle, &[(middle, 180.0)])[0];
    let left = board(doc, cherry, "Side apron", DVec3::new(2.0, 2.75, 29.25), DVec3::Y, -DVec3::Z, [14.5, 4.0, 0.75]);
    let left = component(doc, left, "Side apron");
    let right = copies(doc, left, middle, &[(middle, 180.0)])[0];

    // Tenons 3/4" long, so the two mortises in each leg don't meet. Joints already made in the
    // shared leg and apron designs are just recorded.
    let tenon = Tenon { length: 0.75, thickness: 0.25, shoulder: 0.25 };
    for (apron, leg) in [
        (front, front_left),
        (front, front_right),
        (back, back_right),
        (back, back_left),
        (left, front_left),
        (left, back_left),
        (right, front_right),
        (right, back_right),
    ] {
        doc.mortise_and_tenon(apron, leg, tenon).expect("mortise and tenon");
    }
    // The top, resting on the legs and aprons, with a roundover all around its top edges. It's
    // wider than any cherry board, so the cut list glues it up from three, with biscuits.
    let top = board(doc, cherry, "Table top", DVec3::new(0.0, 0.0, 29.25), DVec3::X, DVec3::Y, [36.0, 20.0, 0.75]);
    doc.profile_edges(top, Profile::Roundover, 0.25, Edges::AroundFace { axis: 2, positive: true, sides: [true; 4] }).expect("roundover");
    // Biscuits along the glue-up's seams keep the boards aligned while it's clamped.
    doc.edit_part(top, |p| p.edge_joint = woodsketch_doc::part::EdgeJoint::Biscuits);
}

fn bookcase(doc: &mut Document) {
    let oak = wood(doc, "Red Oak");
    let plywood = wood(doc, "Plywood");
    let x = 50.0;
    // Sides: 3/4" thick, 11 1/4" deep, 48" tall; 30" apart inside.
    let left = board(doc, oak, "Left side", DVec3::new(x + 0.75, 0.0, 0.0), DVec3::Z, DVec3::Y, [48.0, 11.25, 0.75]);
    let right = board(doc, oak, "Right side", DVec3::new(x + 31.5, 0.0, 0.0), DVec3::Z, DVec3::Y, [48.0, 11.25, 0.75]);
    // Shelves butting the sides, each let in 1/4": dados, and rabbets for the top one (flush
    // with the sides' tops).
    for (z, name) in [(3.0, "Bottom shelf"), (24.0, "Middle shelf"), (47.25, "Top shelf")] {
        let shelf = board(doc, oak, name, DVec3::new(x + 0.75, 0.0, z), DVec3::X, DVec3::Y, [30.0, 11.25, 0.75]);
        doc.let_in(shelf, left, 0.25).expect("let into the left side");
        doc.let_in(shelf, right, 0.25).expect("let into the right side");
    }
    // A 1/4" plywood back across the whole case.
    board(doc, plywood, "Back", DVec3::new(x, 11.25, 48.0), DVec3::X, -DVec3::Z, [31.5, 48.0, 0.25]);
}

/// Group everything at the top level (except `keep`) into a group named `name`.
fn group_all(doc: &mut Document, name: &str, keep: &[InstanceId]) -> InstanceId {
    let parts: Vec<Entity> = doc.container().instances().map(|(id, _)| id).filter(|id| !keep.contains(id)).map(Entity::Instance).collect();
    doc.selection_mut().clear();
    doc.selection_mut().extend(parts);
    let group = doc.group_selection(DefinitionKind::Group).expect("a group");
    doc.rename_instance(group, name);
    doc.selection_mut().clear();
    group
}

fn main() {
    let path = std::env::args().nth(1).unwrap_or_else(|| "Sample Furniture.woodsketch".into());
    let mut doc = Document::default();
    side_table(&mut doc);
    let table = group_all(&mut doc, "Table", &[]);
    bookcase(&mut doc);
    group_all(&mut doc, "Bookcase", &[table]);
    std::fs::write(&path, woodsketch_doc::file::to_string(doc.scene())).expect("write the drawing");
    let list = woodsketch_doc::cutlist::cut_list(doc.scene());
    let report = woodsketch_doc::movement::check(doc.scene(), doc.scene().moisture_swing());
    println!("Wrote {path}: {} materials, {} joints, {} profiles, {} movement warnings", list.materials.len(), doc.scene().joints.len(), doc.scene().profiles.len(), report.conflicts.len());
}
