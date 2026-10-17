//! Writes a sample drawing with furniture built from WoodSketch's own operations, for trying
//! the cut list, build sheets and wood movement checks:
//!
//! - a cherry side table: a top with a roundover, chamfered legs, aprons tenoned into the legs;
//! - a red oak bookcase: shelves let into the sides (dados, rabbets at the top), a plywood back.
//!
//! `cargo run -p woodsketch-doc --example sample_furniture -- "Sample Furniture.woodsketch"`

use glam::DVec3;
use woodsketch_doc::board::Board;
use woodsketch_doc::joinery::Tenon;
use woodsketch_doc::library::LIBRARY;
use woodsketch_doc::profile::{Edges, Profile};
use woodsketch_doc::{Document, InstanceId, MaterialId};

fn wood(doc: &mut Document, name: &str) -> MaterialId {
    doc.add_preset(LIBRARY.iter().find(|p| p.name == name).expect("a library wood"))
}

fn board(doc: &mut Document, material: MaterialId, name: &str, origin: DVec3, length_dir: DVec3, width_dir: DVec3, [length, width, thickness]: [f64; 3]) -> InstanceId {
    let id = doc.add_board(&Board { origin, length_dir, width_dir, length, width, thickness }, Some(material)).expect("a board");
    doc.rename_instance(id, name);
    id
}

fn side_table(doc: &mut Document) {
    let cherry = wood(doc, "Cherry");
    // Legs: 1 3/4" square, 29 1/4" tall, set in 1" from the top's edges. Chamfer their corners
    // first (profiles go on plain boards).
    let mut legs = Vec::new();
    for (x, y, name) in [(1.0, 1.0, "Front left leg"), (33.25, 1.0, "Front right leg"), (1.0, 17.25, "Back left leg"), (33.25, 17.25, "Back right leg")] {
        let leg = board(doc, cherry, name, DVec3::new(x, y, 0.0), DVec3::Z, DVec3::X, [29.25, 1.75, 1.75]);
        doc.profile_edges(leg, Profile::Chamfer, 0.125, Edges::Along { axis: 0, corners: [true; 4] }).expect("chamfer");
        legs.push(leg);
    }
    // Aprons: 4" wide, 3/4" thick, set back 1/4" from the legs' outside faces, between the legs.
    let front = board(doc, cherry, "Front apron", DVec3::new(2.75, 1.25, 29.25), DVec3::X, -DVec3::Z, [30.5, 4.0, 0.75]);
    let back = board(doc, cherry, "Back apron", DVec3::new(2.75, 18.0, 29.25), DVec3::X, -DVec3::Z, [30.5, 4.0, 0.75]);
    let left = board(doc, cherry, "Left apron", DVec3::new(2.0, 2.75, 29.25), DVec3::Y, -DVec3::Z, [14.5, 4.0, 0.75]);
    let right = board(doc, cherry, "Right apron", DVec3::new(34.75, 2.75, 29.25), DVec3::Y, -DVec3::Z, [14.5, 4.0, 0.75]);
    // Tenons 3/4" long, so the two mortises in each leg don't meet.
    let tenon = Tenon { length: 0.75, thickness: 0.25, shoulder: 0.25 };
    for (apron, leg) in [(front, 0), (front, 1), (back, 2), (back, 3), (left, 0), (left, 2), (right, 1), (right, 3)] {
        doc.mortise_and_tenon(apron, legs[leg], tenon).expect("mortise and tenon");
    }
    // The top, resting on the legs and aprons, with a roundover all around its top edges.
    let top = board(doc, cherry, "Table top", DVec3::new(0.0, 0.0, 29.25), DVec3::X, DVec3::Y, [36.0, 20.0, 0.75]);
    doc.profile_edges(top, Profile::Roundover, 0.25, Edges::AroundFace { axis: 2, positive: true, sides: [true; 4] }).expect("roundover");
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

fn main() {
    let path = std::env::args().nth(1).unwrap_or_else(|| "Sample Furniture.woodsketch".into());
    let mut doc = Document::default();
    side_table(&mut doc);
    bookcase(&mut doc);
    std::fs::write(&path, woodsketch_doc::file::to_string(doc.scene())).expect("write the drawing");
    let list = woodsketch_doc::cutlist::cut_list(doc.scene());
    let report = woodsketch_doc::movement::check(doc.scene(), doc.scene().moisture_swing());
    println!("Wrote {path}: {} materials, {} joints, {} profiles, {} movement warnings", list.materials.len(), doc.scene().joints.len(), doc.scene().profiles.len(), report.conflicts.len());
}
