//! Timing a big shop drawing: how long editing, hovering and redrawing take. Not run by default;
//! run with `cargo test --release -p sketchcraft-ui-egui --test perf -- --ignored --nocapture`.

use std::time::{Duration, Instant};

use glam::{DVec3, Vec2};
use sketchcraft_doc::Document;
use sketchcraft_render::Scene;
use sketchcraft_tools::{PointerEvent, ToolId, Tools};
use sketchcraft_view::{OrbitCamera, StandardView};

/// `boards` boards (2x4s and panels, each its own group), every tenth one drilled with a
/// 24-sided hole.
fn shop(boards: usize) -> Document {
    let mut doc = Document::default();
    for i in 0..boards {
        let (x, y) = ((i % 20) as f64 * 30.0, (i / 20) as f64 * 30.0);
        let face = doc.edit(|m| m.add_rectangle(DVec3::new(x, y, 0.0), DVec3::X * 20.0, DVec3::Y * 10.0, DVec3::Z)).unwrap();
        let board = doc.push_pull(face, 0.75, true).unwrap().unwrap();
        if i % 10 == 0 {
            doc.enter(board);
            let disk = doc.edit(|m| m.draw_circle(DVec3::new(x + 10.0, y + 5.0, 0.75), DVec3::Z, 1.0, 24)).unwrap();
            doc.push_pull(disk, -0.75, false).unwrap();
            doc.exit();
        }
    }
    doc
}

fn time<R>(label: &str, runs: u32, mut f: impl FnMut() -> R) -> Duration {
    // The fastest run: the least disturbed by whatever else the machine is doing.
    let each = (0..runs)
        .map(|_| {
            let start = Instant::now();
            std::hint::black_box(f());
            start.elapsed()
        })
        .min()
        .unwrap_or_default();
    println!("{label:<44} {:>10.3} ms", each.as_secs_f64() * 1000.0);
    each
}

#[test]
#[ignore]
fn big_shop_drawing() {
    let sizes: Vec<usize> = std::env::var("PERF_BOARDS").map_or(vec![100, 400], |s| s.split(',').filter_map(|n| n.parse().ok()).collect());
    for boards in sizes {
        println!("--- {boards} boards ---");
        let start = Instant::now();
        let mut doc = shop(boards);
        println!("{:<44} {:>10.3} ms", "build the drawing", start.elapsed().as_secs_f64() * 1000.0);
        let faces: usize = doc.view().model().faces().count();
        println!("{:<44} {:>10}", "faces in view", faces);

        let mut camera = OrbitCamera::default();
        camera.set_view(StandardView::Iso);
        let size = Vec2::new(1400.0, 900.0);
        let (lo, hi) = doc.bounds().unwrap();
        camera.zoom_extents(lo.as_vec3(), hi.as_vec3(), size);

        time("edit: draw a line (incl. undo + view)", 5, || {
            doc.edit(|m| m.add_line(DVec3::new(-50.0, 0.0, 0.0), DVec3::new(-50.0, 10.0, 0.0)));
            doc.undo();
        });
        time("  clone the scene (an undo snapshot)", 5, || doc.scene().clone());
        time("  flatten the view", 5, || sketchcraft_doc::View::build(doc.scene(), doc.path()));
        time("select something (version bump)", 20, || {
            doc.selection_mut().clear();
        });
        time("rebuild render scene", 5, || {
            let mut scene = Scene::reference(false);
            scene.add_view(doc.view(), doc.selection(), &doc.context_transform(), doc.scene());
            scene.faces.len()
        });

        let (first, _) = doc.container().instances().next().unwrap();
        doc.selection_mut().extend([sketchcraft_doc::Entity::Instance(first)]);
        time("rebuild selection highlight only", 5, || {
            let mut scene = Scene::default();
            scene.add_selection(doc.view(), doc.selection(), &doc.context_transform(), doc.scene());
            scene.selected_faces.len()
        });
        doc.selection_mut().clear();

        let mut tools = Tools::default();
        for tool in [ToolId::Select, ToolId::Line, ToolId::Move, ToolId::PushPull] {
            tools.activate(tool);
            let mut x = 300.0;
            time(&format!("hover with {}", tool.name()), 20, || {
                x += 7.0;
                tools.pointer(PointerEvent::Move(Vec2::new(x % 1300.0, 450.0)), &mut doc, &mut camera, size);
            });
        }
        tools.activate(ToolId::Select);
        time("click to select", 10, || {
            let mods = sketchcraft_tools::Modifiers::default();
            tools.pointer(PointerEvent::Click { pos: Vec2::new(700.0, 450.0), count: 1, mods }, &mut doc, &mut camera, size);
        });
        time("cut list + plans", 3, || {
            let list = sketchcraft_doc::cutlist::cut_list(doc.scene());
            sketchcraft_doc::cutplan::plan_all(doc.scene(), &list).len()
        });
    }
}
