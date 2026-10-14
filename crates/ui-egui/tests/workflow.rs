//! End-to-end tests: drive the real app headlessly with simulated input, as a user would.
//!
//! Set `SKETCHCRAFT_SNAPSHOT_DIR` to also save rendered PNGs of each scenario.

use eframe::egui::{self, Event, Key, Modifiers, PointerButton, pos2};
use egui_kittest::Harness;
use sketchcraft_tools::ToolId;
use sketchcraft_ui_egui::SketchCraftApp;

/// Tests take turns on the GPU. Two full-suite runs died without an assertion failure, which
/// looked like a driver crash while several tests created wgpu devices at once.
static GPU: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// The app under test, holding the GPU turn until dropped (fields drop in order: harness first).
struct App {
    harness: Harness<'static, SketchCraftApp>,
    _gpu: std::sync::MutexGuard<'static, ()>,
}

impl std::ops::Deref for App {
    type Target = Harness<'static, SketchCraftApp>;
    fn deref(&self) -> &Self::Target {
        &self.harness
    }
}

impl std::ops::DerefMut for App {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.harness
    }
}

fn app() -> App {
    // A panicking test poisons the lock; the others should still run.
    let gpu = GPU.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut harness = Harness::builder()
        .with_size([1200.0, 800.0])
        // Real frame timing, so quick successive clicks count as double-clicks.
        .with_step_dt(1.0 / 60.0)
        .wgpu()
        .build_eframe(|cc| SketchCraftApp::new(cc));
    harness.run();
    App { harness, _gpu: gpu }
}

fn click(harness: &mut Harness<'_, SketchCraftApp>, pos: egui::Pos2) {
    harness.hover_at(pos);
    harness.run();
    for pressed in [true, false] {
        harness.event(Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
        harness.run();
    }
}

fn type_text(harness: &mut Harness<'_, SketchCraftApp>, text: &str) {
    harness.event(Event::Text(text.into()));
    harness.run();
}

fn snapshot(harness: &mut Harness<'_, SketchCraftApp>, name: &str) {
    if let Ok(dir) = std::env::var("SKETCHCRAFT_SNAPSHOT_DIR") {
        let image = harness.render().expect("render the frame");
        image.save(std::path::Path::new(&dir).join(format!("{name}.png"))).expect("save snapshot");
    }
}

fn size(harness: &Harness<'_, SketchCraftApp>) -> (f64, f64) {
    let (lo, hi) = harness.state().document().bounds().expect("geometry exists");
    ((hi - lo).x, (hi - lo).y)
}

#[test]
fn draw_a_two_by_four_by_typing_its_size() {
    let mut harness = app();
    type_text(&mut harness, "r");
    assert_eq!(harness.state().tools().active(), ToolId::Rectangle);

    click(&mut harness, pos2(600.0, 450.0));
    harness.hover_at(pos2(680.0, 400.0));
    harness.run();
    type_text(&mut harness, "1.5\",3.5\"");
    assert_eq!(harness.state().tools().measurement(), "1.5\",3.5\"");
    harness.key_press(Key::Enter);
    harness.run();

    let model = harness.state().document().view().model();
    assert_eq!(model.faces().count(), 1);
    assert_eq!(model.edges().count(), 4);
    assert_eq!(size(&harness), (1.5, 3.5));
    assert_eq!(harness.state().tools().measurement(), "1 1/2\", 3 1/2\"");

    // Shift+Z frames it, like SketchUp's Zoom Extents.
    type_text(&mut harness, "Z");
    snapshot(&mut harness, "two_by_four");
}

#[test]
fn space_selects_and_ctrl_a_delete_undo_work() {
    let mut harness = app();
    type_text(&mut harness, "r");
    click(&mut harness, pos2(600.0, 450.0));
    harness.hover_at(pos2(700.0, 380.0));
    harness.run();
    type_text(&mut harness, "3.5,1.5");
    harness.key_press(Key::Enter);
    harness.run();

    type_text(&mut harness, " ");
    assert_eq!(harness.state().tools().active(), ToolId::Select);

    harness.key_press_modifiers(Modifiers::COMMAND, Key::A);
    harness.run();
    assert_eq!(harness.state().document().selection().len(), 5);
    snapshot(&mut harness, "selected");

    harness.key_press(Key::Delete);
    harness.run();
    assert!(harness.state().document().model().is_empty());

    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
    assert_eq!(harness.state().document().view().model().faces().count(), 1);
}

#[test]
fn clicking_a_face_selects_it() {
    let mut harness = app();
    type_text(&mut harness, "r");
    let corner = pos2(600.0, 450.0);
    click(&mut harness, corner);
    let far = pos2(760.0, 360.0);
    click(&mut harness, far);
    assert_eq!(harness.state().document().view().model().faces().count(), 1);

    type_text(&mut harness, " ");
    // Halfway between the corners is inside the rectangle on screen.
    click(&mut harness, corner.lerp(far, 0.5));
    let selection = harness.state().document().selection();
    assert_eq!(selection.len(), 1);
    assert!(matches!(selection.iter().next(), Some(sketchcraft_doc::Entity::Face(_))));
}

#[test]
fn letters_go_to_measurements_once_typing_starts() {
    let mut harness = app();
    type_text(&mut harness, "r");
    click(&mut harness, pos2(600.0, 450.0));
    // "m" would be Move, but after digits it's the start of "mm".
    type_text(&mut harness, "38mm,89mm");
    assert_eq!(harness.state().tools().active(), ToolId::Rectangle);
    harness.key_press(Key::Enter);
    harness.run();
    let (w, h) = size(&harness);
    assert!((w - 38.0 / 25.4).abs() < 1e-9 && (h - 89.0 / 25.4).abs() < 1e-9, "{w} x {h}");
}

#[test]
fn push_pull_a_profile_into_an_eight_foot_two_by_four() {
    let mut harness = app();
    type_text(&mut harness, "r");
    click(&mut harness, pos2(600.0, 450.0));
    harness.hover_at(pos2(680.0, 400.0));
    harness.run();
    type_text(&mut harness, "1.5,3.5");
    harness.key_press(Key::Enter);
    harness.run();
    type_text(&mut harness, "Z");

    type_text(&mut harness, "p");
    assert_eq!(harness.state().tools().active(), ToolId::PushPull);
    let (lo, hi) = harness.state().document().bounds().unwrap();
    let center = harness.state().to_window((lo + hi) * 0.5).unwrap();
    click(&mut harness, center);
    // Pull upward on screen, then type the length.
    harness.hover_at(center - egui::vec2(0.0, 60.0));
    harness.run();
    type_text(&mut harness, "8'");
    harness.key_press(Key::Enter);
    harness.run();

    let model = harness.state().document().view().model();
    assert_eq!(model.faces().count(), 6);
    let (lo, hi) = model.bounds().unwrap();
    assert_eq!((hi - lo).to_array(), [1.5, 3.5, 96.0]);
    assert_eq!(harness.state().tools().measurement(), "96\"");

    type_text(&mut harness, "Z");
    snapshot(&mut harness, "board");
}

/// An 8' 2x4 standing at the origin, framed.
fn board(harness: &mut App) {
    type_text(harness, "r");
    click(harness, pos2(600.0, 450.0));
    harness.hover_at(pos2(680.0, 400.0));
    harness.run();
    type_text(harness, "1.5,3.5");
    harness.key_press(Key::Enter);
    harness.run();
    type_text(harness, "Z");
    type_text(harness, "p");
    let (lo, hi) = harness.state().document().bounds().unwrap();
    let center = harness.state().to_window((lo + hi) * 0.5).unwrap();
    click(harness, center);
    harness.hover_at(center - egui::vec2(0.0, 60.0));
    harness.run();
    type_text(harness, "96");
    harness.key_press(Key::Enter);
    harness.run();
    type_text(harness, "Z");
}

#[test]
fn copy_a_stud_into_a_wall_with_move_ctrl_and_array() {
    let mut harness = app();
    board(&mut harness);
    type_text(&mut harness, " ");
    harness.key_press_modifiers(Modifiers::COMMAND, Key::A);
    harness.run();

    type_text(&mut harness, "m");
    assert_eq!(harness.state().tools().active(), ToolId::Move);
    // Tap Ctrl on its own to switch to copying.
    harness.event(Event::ModifiersChanged(Modifiers::COMMAND));
    harness.event(Event::ModifiersChanged(Modifiers::NONE));
    harness.run();
    assert!(harness.state().tools().status().contains("copying"), "{}", harness.state().tools().status());

    let (start, _) = harness.state().document().bounds().unwrap();
    let base = harness.state().to_window(start).unwrap();
    click(&mut harness, base);
    harness.key_press(Key::ArrowRight);
    harness.hover_at(base + egui::vec2(120.0, 0.0));
    harness.run();
    snapshot(&mut harness, "move_on_red");
    type_text(&mut harness, "16");
    harness.key_press(Key::Enter);
    harness.run();
    type_text(&mut harness, "4x");
    harness.key_press(Key::Enter);
    harness.run();

    let model = harness.state().document().view().model();
    assert_eq!(model.faces().count(), 30, "five boards");
    let (lo, hi) = model.bounds().unwrap();
    // Original plus four copies, 16" on center, along red only.
    assert_eq!(lo, start);
    assert!((hi.x - lo.x - 65.5).abs() < 1e-9, "{}", hi.x - lo.x);
    assert_eq!(hi.y - lo.y, 3.5);
    type_text(&mut harness, "Z");
    snapshot(&mut harness, "stud_wall");
}

#[test]
fn rotate_a_board_with_a_move_grip() {
    let mut harness = app();
    board(&mut harness);
    type_text(&mut harness, "m");
    let (lo, hi) = harness.state().document().bounds().unwrap();
    let center = (lo + hi) * 0.5;

    // Hover the board's side: grips appear on the visible faces of its bounding box.
    let side = harness.state().to_window(glam::DVec3::new(center.x, lo.y, center.z)).unwrap();
    harness.hover_at(side);
    harness.run();
    let state = harness.state();
    let grips = state.tools().overlay(state.document()).grips;
    assert_eq!(grips.len(), 12);
    snapshot(&mut harness, "grips");

    // Grab a grip on the top and turn it.
    let top = grips.iter().map(|(p, _)| *p).find(|p| (p.z - hi.z).abs() < 1e-9).unwrap();
    let grip = harness.state().to_window(top).unwrap();
    click(&mut harness, grip);
    assert_eq!(harness.state().tools().measurement_label(), "Angle");
    let pivot = glam::DVec3::new(center.x, center.y, hi.z);
    let turned = pivot + (top - pivot).cross(glam::DVec3::Z);
    harness.hover_at(harness.state().to_window(turned).unwrap());
    harness.run();
    snapshot(&mut harness, "grip_rotate");
    type_text(&mut harness, "90");
    harness.key_press(Key::Enter);
    harness.run();

    let (new_lo, new_hi) = harness.state().document().bounds().unwrap();
    let size = new_hi - new_lo;
    assert!((size.x - 3.5).abs() < 1e-9 && (size.y - 1.5).abs() < 1e-9, "{size}");
    assert!(((new_lo + new_hi) * 0.5).abs_diff_eq(center, 1e-9));
    assert_eq!(harness.state().tools().active(), ToolId::Move);
}

#[test]
fn new_boards_are_groups_you_open_by_double_clicking() {
    let mut harness = app();
    board(&mut harness);
    let doc = harness.state().document();
    assert!(doc.model().is_empty(), "the board went into a group");
    assert_eq!(doc.container().instances().count(), 1);
    let (lo, hi) = doc.bounds().unwrap();

    // Clicking the board with Select picks the whole group.
    type_text(&mut harness, " ");
    let side = harness.state().to_window(glam::DVec3::new((lo.x + hi.x) * 0.5, lo.y, (lo.z + hi.z) * 0.5)).unwrap();
    click(&mut harness, side);
    let selection = harness.state().document().selection();
    assert!(matches!(selection.iter().next(), Some(sketchcraft_doc::Entity::Instance(_))));
    snapshot(&mut harness, "group_selected");

    // Double-click opens it: now its faces are editable. One button event per frame, well
    // inside the double-click interval.
    for pressed in [true, false, true, false] {
        harness.event(Event::PointerButton { pos: side, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
        harness.step();
    }
    harness.run();
    assert_eq!(harness.state().document().path().len(), 1, "double-click opened the group");
    assert_eq!(harness.state().document().model().faces().count(), 6);
    snapshot(&mut harness, "group_open");

    // Esc closes it again (twice: first clears any selection).
    harness.key_press(Key::Escape);
    harness.key_press(Key::Escape);
    harness.run();
    assert!(harness.state().document().path().is_empty());
}

#[test]
fn g_makes_a_component_from_loose_geometry() {
    let mut harness = app();
    type_text(&mut harness, "r");
    click(&mut harness, pos2(600.0, 450.0));
    harness.hover_at(pos2(680.0, 400.0));
    harness.run();
    type_text(&mut harness, "1.5,3.5");
    harness.key_press(Key::Enter);
    harness.run();
    type_text(&mut harness, " ");
    harness.key_press_modifiers(Modifiers::COMMAND, Key::A);
    harness.run();
    type_text(&mut harness, "g");
    let doc = harness.state().document();
    assert!(doc.model().is_empty());
    let (id, _) = doc.container().instances().next().expect("a component");
    assert_eq!(doc.instance_name(id), Some("Component#1"));
}

#[test]
fn tag_a_board_and_hide_it_from_the_tags_panel() {
    use egui_kittest::kittest::Queryable;
    use egui_phosphor::regular as ph;

    let mut harness = app();
    board(&mut harness);

    // "+" adds a tag and starts renaming it.
    // The last "+" is the Tags panel's (the Materials panel above it has one too).
    harness.get_all_by_label(ph::PLUS).last().expect("Tags + button").click();
    harness.run();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::A);
    type_text(&mut harness, "Legs");
    harness.key_press(Key::Enter);
    harness.run();
    let legs = {
        let scene = harness.state().document().scene();
        let (id, tag) = scene.tags().next().expect("a tag was added");
        assert_eq!(tag.name, "Legs");
        id
    };

    // Select the board and tag it from Entity Info.
    type_text(&mut harness, " ");
    harness.key_press_modifiers(Modifiers::COMMAND, Key::A);
    harness.run();
    // Entity Info has a material dropdown, then the tag dropdown.
    harness.get_all_by_role(egui::accesskit::Role::ComboBox).last().expect("tag dropdown").click();
    harness.run();
    snapshot(&mut harness, "tag_dropdown");
    harness.run();
    harness.get_all_by_label("Legs").last().expect("Legs in the tag list").click();
    harness.run();
    assert_eq!(harness.state().document().selection_tag(), Some(Some(legs)));
    snapshot(&mut harness, "tagged");

    // The eye next to Legs hides the board.
    harness.get_all_by_label(ph::EYE).last().expect("Legs's eye button").click();
    harness.run();
    let doc = harness.state().document();
    assert!(!doc.scene().tag(legs).unwrap().visible);
    assert_eq!(doc.view().model().faces().count(), 0);
    assert!(doc.selection().is_empty());
    snapshot(&mut harness, "tag_hidden");
}

#[test]
fn paint_a_board_with_a_new_plywood_material() {
    use egui_kittest::kittest::Queryable;
    use egui_phosphor::regular as ph;

    let mut harness = app();
    board(&mut harness);
    let (lo, hi) = harness.state().document().bounds().unwrap();

    // Materials' "+" (above the Tags panel's) opens a menu of material types.
    harness.get_all_by_label(ph::PLUS).next().expect("Materials + button").click();
    harness.run();
    harness.get_by_label("Sheet good").click();
    harness.run();
    let state = harness.state();
    assert_eq!(state.tools().active(), ToolId::PaintBucket, "picking a material switches to the Paint Bucket");
    let plywood = state.tools().active_material.expect("the new material is active");
    let material = state.document().scene().material(plywood).unwrap();
    assert_eq!((material.name.as_str(), material.thickness), ("Plywood", Some(0.75)));

    // Click the board: the whole group gets the material.
    let side = harness.state().to_window(glam::DVec3::new((lo.x + hi.x) * 0.5, lo.y, (lo.z + hi.z) * 0.5)).unwrap();
    click(&mut harness, side);
    let doc = harness.state().document();
    let (board, _) = doc.container().instances().next().unwrap();
    assert_eq!(doc.material_of(sketchcraft_doc::Entity::Instance(board)), Some(plywood));
    type_text(&mut harness, "Z");
    snapshot(&mut harness, "painted");
}

#[test]
fn save_new_and_reopen_a_drawing() {
    let dir = std::env::temp_dir().join(format!("sketchcraft-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("bench.scraft");

    let mut harness = app();
    board(&mut harness);
    assert!(harness.state().is_dirty());
    let bounds = harness.state().document().bounds();
    harness.state_mut().save_to(&path).unwrap();
    harness.run();
    assert!(!harness.state().is_dirty());
    assert_eq!(harness.state().file_path(), Some(path.as_path()));

    harness.state_mut().new_document();
    harness.run();
    assert_eq!(harness.state().document().bounds(), None);

    harness.state_mut().open_path(&path).unwrap();
    harness.run();
    assert_eq!(harness.state().document().bounds(), bounds);
    assert_eq!(harness.state().document().container().instances().count(), 1, "the board is still a group");
    assert!(!harness.state().is_dirty());
    snapshot(&mut harness, "reopened");

    // Editing marks it dirty again.
    type_text(&mut harness, " ");
    harness.key_press_modifiers(Modifiers::COMMAND, Key::A);
    harness.key_press(Key::Delete);
    harness.run();
    assert!(harness.state().is_dirty());

    // Opening something that isn't a drawing fails cleanly and keeps the current one.
    let junk = dir.join("junk.scraft");
    std::fs::write(&junk, "hello").unwrap();
    let error = harness.state_mut().open_path(&junk).unwrap_err();
    assert!(error.contains("isn't a SketchCraft drawing"), "{error}");
    assert_eq!(harness.state().file_path(), Some(path.as_path()));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn new_asks_before_discarding_unsaved_work() {
    use egui_kittest::kittest::Queryable;

    let mut harness = app();
    board(&mut harness);
    harness.get_by_label("File").click();
    harness.run();
    harness.get_by_label_contains("New").click();
    harness.run();
    // The drawing is still there while the question is up.
    assert!(harness.state().document().bounds().is_some());
    snapshot(&mut harness, "save_changes");
    harness.get_by_label("Cancel").click();
    harness.run();
    assert!(harness.state().document().bounds().is_some());

    harness.key_press_modifiers(Modifiers::COMMAND, Key::N);
    harness.run();
    harness.get_by_label("Don't Save").click();
    harness.run();
    assert_eq!(harness.state().document().bounds(), None);
    assert!(!harness.state().is_dirty());
}

/// A board `l` long, `w` wide, `t` thick standing at `at`, auto-grouped.
fn add_board(doc: &mut sketchcraft_doc::Document, at: glam::DVec3, l: f64, w: f64, t: f64) -> sketchcraft_doc::Entity {
    use glam::DVec3;
    let face = doc.edit(|m| m.add_rectangle(at, DVec3::X * w, DVec3::Y * t, DVec3::Z)).unwrap();
    sketchcraft_doc::Entity::Instance(doc.push_pull(face, l, true).unwrap().unwrap())
}

#[test]
fn cut_list_shows_parts_and_cutting_diagrams() {
    use egui_kittest::kittest::Queryable;
    use egui_phosphor::regular as ph;
    use glam::DVec3;
    use sketchcraft_doc::MaterialKind;

    let mut harness = app();
    {
        let doc = harness.state_mut().document_mut();
        // A plywood cabinet carcass: 2 sides, top and bottom, a back.
        let ply = doc.add_material(MaterialKind::SheetGood);
        let mut carcass = Vec::new();
        for i in 0..2 {
            carcass.push(add_board(doc, DVec3::X * (25.0 * f64::from(i)), 30.0, 23.25, 0.75));
            carcass.push(add_board(doc, DVec3::new(60.0 + 36.0 * f64::from(i), 0.0, 0.0), 34.5, 23.25, 0.75));
        }
        carcass.push(add_board(doc, DVec3::new(0.0, 40.0, 0.0), 34.5, 30.0, 0.75));
        doc.paint(&carcass, Some(ply));
        // A 2x4 frame: two studs and three short pieces.
        let lumber = doc.add_material(MaterialKind::DimensionalLumber);
        let mut frame = Vec::new();
        for i in 0..2 {
            frame.push(add_board(doc, DVec3::new(-20.0 - 6.0 * f64::from(i), 0.0, 0.0), 92.625, 3.5, 1.5));
        }
        for i in 0..3 {
            frame.push(add_board(doc, DVec3::new(-40.0 - 6.0 * f64::from(i), 0.0, 0.0), 46.0, 3.5, 1.5));
        }
        doc.paint(&frame, Some(lumber));
    }
    harness.run();
    type_text(&mut harness, "Z");

    harness.get_by_label(ph::TABLE).click();
    harness.run();
    for label in ["Plywood", "Lumber", "A", "B", "C", "D", "E"] {
        assert!(harness.query_all_by_label(label).next().is_some(), "{label} is listed");
    }
    snapshot(&mut harness, "cut_list_parts");

    harness.get_by_label("Cutting diagrams").click();
    harness.run();
    assert!(harness.query_all_by_label_contains("× Sheet 96\" x 48\"").next().is_some(), "plywood shopping list");
    assert!(harness.query_all_by_label_contains("× 2x4 x").next().is_some(), "lumber shopping list");
    snapshot(&mut harness, "cut_list_diagrams");
}

#[test]
fn unavailable_tool_shortcut_explains() {
    let mut harness = app();
    type_text(&mut harness, "e");
    assert_eq!(harness.state().tools().active(), ToolId::Select);
    assert!(harness.state().tools().status().contains("Eraser (E) is coming soon"));
}

#[test]
fn draw_a_line_across_a_boards_top_and_push_down_a_rabbet() {
    use glam::DVec3;
    let mut harness = app();
    {
        let doc = harness.state_mut().document_mut();
        let face = doc.edit(|m| m.add_rectangle(DVec3::ZERO, DVec3::X * 10.0, DVec3::Y * 4.0, DVec3::Z)).unwrap();
        doc.push_pull(face, 2.0, false).unwrap();
    }
    harness.run();
    type_text(&mut harness, "Z");

    type_text(&mut harness, "l");
    assert_eq!(harness.state().tools().active(), ToolId::Line);
    let front = harness.state().to_window(DVec3::new(8.0, 0.0, 2.0)).unwrap();
    let back = harness.state().to_window(DVec3::new(8.0, 4.0, 2.0)).unwrap();
    click(&mut harness, front);
    harness.hover_at(back);
    harness.run();
    assert_eq!(harness.state().tools().measurement_label(), "Length");
    click(&mut harness, back);
    assert_eq!(harness.state().document().model().faces().count(), 7, "the top is split in two");
    snapshot(&mut harness, "line_split");

    type_text(&mut harness, "p");
    let end = harness.state().to_window(DVec3::new(9.0, 2.0, 2.0)).unwrap();
    click(&mut harness, end);
    harness.hover_at(end + egui::vec2(0.0, 40.0));
    harness.run();
    type_text(&mut harness, "1");
    harness.key_press(Key::Enter);
    harness.run();

    let model = harness.state().document().model();
    assert_eq!(model.faces().count(), 8, "a clean rabbet: no patched holes");
    assert!(model.faces().all(|(_, f)| f.holes.is_empty()));
    let (lo, hi) = model.bounds().unwrap();
    assert_eq!((hi - lo).to_array(), [10.0, 4.0, 2.0]);
    snapshot(&mut harness, "rabbet");
}

#[test]
fn tape_a_guide_off_an_edge_then_draw_from_where_it_crosses() {
    use egui_kittest::kittest::Queryable;
    use glam::DVec3;
    use sketchcraft_doc::Guide;
    let mut harness = app();
    harness
        .state_mut()
        .document_mut()
        .edit(|m| m.add_rectangle(DVec3::ZERO, DVec3::X * 24.0, DVec3::Y * 12.0, DVec3::Z));
    harness.run();
    type_text(&mut harness, "Z");

    type_text(&mut harness, "t");
    assert_eq!(harness.state().tools().active(), ToolId::TapeMeasure);
    let on_edge = harness.state().to_window(DVec3::new(6.0, 0.0, 0.0)).unwrap();
    click(&mut harness, on_edge);
    harness.hover_at(harness.state().to_window(DVec3::new(6.0, 4.0, 0.0)).unwrap());
    harness.run();
    type_text(&mut harness, "3 1/2");
    harness.key_press(Key::Enter);
    harness.run();
    let guides: Vec<Guide> = harness.state().document().model().guides().map(|(_, g)| *g).collect();
    let [Guide::Line { point, dir }] = guides[..] else { panic!("{guides:?}") };
    assert!((point.y - 3.5).abs() < 1e-9 && dir.cross(DVec3::X).length() < 1e-9, "{point} {dir}");
    snapshot(&mut harness, "tape_guide");

    // A line from where the guide crosses the left edge splits off a 3 1/2" strip.
    type_text(&mut harness, "l");
    let left = harness.state().to_window(DVec3::new(0.0, 3.5, 0.0)).unwrap();
    click(&mut harness, left + egui::vec2(3.0, 2.0));
    let right = harness.state().to_window(DVec3::new(24.0, 3.5, 0.0)).unwrap();
    click(&mut harness, right + egui::vec2(-3.0, 2.0));
    let model = harness.state().document().model();
    assert_eq!(model.faces().count(), 2);
    assert!(model.vertices().any(|(_, p)| p.abs_diff_eq(DVec3::new(0.0, 3.5, 0.0), 1e-9)));

    harness.get_by_label("Edit").click();
    harness.run();
    harness.get_by_label("Delete Guides").click();
    harness.run();
    assert_eq!(harness.state().document().model().guides().count(), 0);
}

#[test]
fn the_window_is_laid_out_like_sketchup() {
    use egui_kittest::kittest::Queryable;
    let mut harness = app();
    for menu in ["File", "Edit", "View", "Camera", "Draw", "Tools", "Window", "Help"] {
        assert!(harness.query_by_label(menu).is_some(), "{menu} menu");
    }
    for panel in ["Default Tray", "Entity Info", "Materials", "Tags", "Outliner", "Measurements"] {
        assert!(harness.query_by_label_contains(panel).is_some(), "{panel}");
    }
    // Draw > Line activates the Line tool, like its L shortcut.
    harness.get_by_label("Draw").click();
    harness.run();
    harness.get_by_label_contains("Line").click();
    harness.run();
    assert_eq!(harness.state().tools().active(), ToolId::Line);

    // The front view shows the sky above the horizon.
    harness.get_by_label("Front").click();
    harness.run();
    snapshot(&mut harness, "front_view");
}

#[test]
fn offset_a_door_into_a_frame_and_recess_the_panel() {
    use glam::DVec3;
    let mut harness = app();
    {
        let doc = harness.state_mut().document_mut();
        let face = doc.edit(|m| m.add_rectangle(DVec3::ZERO, DVec3::X * 24.0, DVec3::Y * 36.0, DVec3::Z)).unwrap();
        doc.push_pull(face, 0.75, false).unwrap();
    }
    harness.run();
    type_text(&mut harness, "Z");

    type_text(&mut harness, "f");
    assert_eq!(harness.state().tools().active(), ToolId::Offset);
    let center = harness.state().to_window(DVec3::new(12.0, 18.0, 0.75)).unwrap();
    click(&mut harness, center);
    harness.hover_at(harness.state().to_window(DVec3::new(4.0, 18.0, 0.75)).unwrap());
    harness.run();
    type_text(&mut harness, "2 1/2");
    harness.key_press(Key::Enter);
    harness.run();
    assert_eq!(harness.state().tools().measurement(), "2 1/2\"");

    // Push the panel down 1/4" inside its frame.
    type_text(&mut harness, "p");
    click(&mut harness, center);
    harness.hover_at(center + egui::vec2(0.0, 30.0));
    harness.run();
    type_text(&mut harness, "1/4");
    harness.key_press(Key::Enter);
    harness.run();
    let model = harness.state().document().model();
    let floor = model
        .faces()
        .find(|(_, f)| f.normal.z > 0.5 && (model.vertex(f.outer[0]).z - 0.5).abs() < 1e-9)
        .map(|(id, _)| id)
        .expect("a recessed panel at 1/2\"");
    let (lo, hi) = model.loop_points(&model.face(floor).unwrap().outer).iter().fold(
        (DVec3::splat(f64::MAX), DVec3::splat(f64::MIN)),
        |(lo, hi), p| (lo.min(*p), hi.max(*p)),
    );
    assert!(lo.abs_diff_eq(DVec3::new(2.5, 2.5, 0.5), 1e-9) && hi.abs_diff_eq(DVec3::new(21.5, 33.5, 0.5), 1e-9));
    type_text(&mut harness, "Z");
    snapshot(&mut harness, "door_panel");
}

#[test]
fn drill_a_dowel_hole_with_circle_and_push_pull() {
    use glam::DVec3;
    let mut harness = app();
    {
        let doc = harness.state_mut().document_mut();
        let face = doc.edit(|m| m.add_rectangle(DVec3::ZERO, DVec3::X * 6.0, DVec3::Y * 4.0, DVec3::Z)).unwrap();
        doc.push_pull(face, 0.75, false).unwrap();
    }
    harness.run();
    type_text(&mut harness, "Z");

    type_text(&mut harness, "c");
    assert_eq!(harness.state().tools().active(), ToolId::Circle);
    let center = harness.state().to_window(DVec3::new(3.0, 2.0, 0.75)).unwrap();
    click(&mut harness, center);
    harness.hover_at(center + egui::vec2(30.0, 0.0));
    harness.run();
    assert_eq!(harness.state().tools().measurement_label(), "Radius");
    type_text(&mut harness, "3/16");
    harness.key_press(Key::Enter);
    harness.run();

    type_text(&mut harness, "p");
    click(&mut harness, center);
    harness.hover_at(center + egui::vec2(0.0, 30.0));
    harness.run();
    type_text(&mut harness, "3/4");
    harness.key_press(Key::Enter);
    harness.run();

    let model = harness.state().document().model();
    let top = model.faces().find(|(_, f)| f.normal.z > 0.5).unwrap().1;
    let bottom = model.faces().find(|(_, f)| f.normal.z < -0.5).unwrap().1;
    assert_eq!((top.holes.len(), bottom.holes.len()), (1, 1), "a hole right through");
    assert_eq!(model.faces().count(), 6 + 24, "the board and the hole's 24 walls");
    snapshot(&mut harness, "dowel_hole");
}

#[test]
fn export_obj_with_materials_and_stl() {
    use egui_kittest::kittest::Queryable;
    use sketchcraft_ui_egui::files::ExportFormat;
    let dir = std::env::temp_dir().join(format!("sketchcraft-export-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut harness = app();
    board(&mut harness);
    harness.get_by_label("File").click();
    harness.run();
    harness.get_by_label_contains("Export").hover();
    harness.run();
    harness.get_by_label("Millimeters").click();
    harness.run();

    harness.state().export_to(&dir.join("bench.obj"), ExportFormat::Obj).unwrap();
    harness.state().export_to(&dir.join("bench.stl"), ExportFormat::Stl).unwrap();
    let obj = std::fs::read_to_string(dir.join("bench.obj")).unwrap();
    assert!(obj.contains("mtllib bench.mtl") && obj.contains("units: millimeters"));
    assert!(std::fs::read_to_string(dir.join("bench.mtl")).unwrap().contains("newmtl"));
    let stl = std::fs::read_to_string(dir.join("bench.stl")).unwrap();
    assert_eq!(stl.matches("facet normal").count(), 12, "a board: 6 faces, 12 triangles");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn rename_a_board_in_entity_info_for_the_cut_list() {
    use egui_kittest::kittest::Queryable;
    let mut harness = app();
    board(&mut harness);
    type_text(&mut harness, " ");
    harness.key_press_modifiers(Modifiers::COMMAND, Key::A);
    harness.run();
    harness.get_by_role(egui::accesskit::Role::TextInput).click();
    harness.run();
    // Replace the default "Group#1" while the field has focus; shortcuts don't fire.
    harness.key_press_modifiers(Modifiers::COMMAND, Key::A);
    type_text(&mut harness, "Left side");
    assert_eq!(harness.state().tools().active(), ToolId::Select, "typing a name isn't tool shortcuts");
    let doc = harness.state().document();
    let (board, _) = doc.container().instances().next().unwrap();
    assert_eq!(doc.instance_name(board), Some("Left side"));
    let list = sketchcraft_doc::cutlist::cut_list(doc.scene());
    assert_eq!(list.materials[0].parts[0].names, vec!["Left side".to_owned()]);
    // One undo step undoes the whole name.
    harness.key_press(Key::Escape);
    harness.run();
    harness.state_mut().document_mut().undo();
    let doc = harness.state().document();
    assert_eq!(doc.instance_name(board), Some("Group#1"));
}
