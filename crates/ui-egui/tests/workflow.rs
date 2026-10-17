//! End-to-end tests: drive the real app headlessly with simulated input, as a user would.
//!
//! Set `WOODSKETCH_SNAPSHOT_DIR` to also save rendered PNGs of each scenario.

use eframe::egui::{self, Event, Key, Modifiers, PointerButton, pos2};
use egui_kittest::Harness;
use woodsketch_tools::ToolId;
use woodsketch_ui_egui::WoodSketchApp;

/// Tests take turns on the GPU. Two full-suite runs died without an assertion failure, which
/// looked like a driver crash while several tests created wgpu devices at once.
static GPU: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// The app under test, holding the GPU turn until dropped (fields drop in order: harness first).
struct App {
    harness: Harness<'static, WoodSketchApp>,
    _gpu: std::sync::MutexGuard<'static, ()>,
}

impl std::ops::Deref for App {
    type Target = Harness<'static, WoodSketchApp>;
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
        .build_eframe(|cc| WoodSketchApp::new(cc));
    harness.run();
    App { harness, _gpu: gpu }
}

fn click(harness: &mut Harness<'_, WoodSketchApp>, pos: egui::Pos2) {
    harness.hover_at(pos);
    harness.run();
    for pressed in [true, false] {
        harness.event(Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
        harness.run();
    }
}

fn type_text(harness: &mut Harness<'_, WoodSketchApp>, text: &str) {
    harness.event(Event::Text(text.into()));
    harness.run();
}

fn snapshot(harness: &mut Harness<'_, WoodSketchApp>, name: &str) {
    if let Ok(dir) = std::env::var("WOODSKETCH_SNAPSHOT_DIR") {
        let image = harness.render().expect("render the frame");
        image.save(std::path::Path::new(&dir).join(format!("{name}.png"))).expect("save snapshot");
    }
}

fn size(harness: &Harness<'_, WoodSketchApp>) -> (f64, f64) {
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
    assert!(matches!(selection.iter().next(), Some(woodsketch_doc::Entity::Face(_))));
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
    assert!(matches!(selection.iter().next(), Some(woodsketch_doc::Entity::Instance(_))));
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
    // The tag dropdown, showing "Untagged".
    harness.get(egui_kittest::kittest::by().role(egui::accesskit::Role::ComboBox).value("Untagged")).click();
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

    // A new drawing has no materials; the panel says how to add one.
    assert!(harness.query_by_label_contains("No materials yet").is_some());
    // Materials' "+" (above the Tags panel's) opens the library.
    harness.get_all_by_label(ph::PLUS).next().expect("Materials + button").click();
    harness.run();
    harness.get_by_label("Plywood").click();
    harness.run();
    let state = harness.state();
    assert_eq!(state.tools().active(), ToolId::PaintBucket, "picking a material switches to the Paint Bucket");
    let plywood = state.tools().active_material.expect("the new material is active");
    let material = state.document().scene().material(plywood).unwrap();
    assert_eq!(material.name, "Plywood");
    assert_eq!(material.standard.thicknesses, vec![0.25, 0.5, 0.75], "one material, every standard thickness");
    // Its editor shows the stock it comes in.
    assert!(harness.query_by_label("Thicknesses").is_some() && harness.query_by_label("Sheet sizes").is_some());

    // Click the board: the whole group gets the material.
    let side = harness.state().to_window(glam::DVec3::new((lo.x + hi.x) * 0.5, lo.y, (lo.z + hi.z) * 0.5)).unwrap();
    click(&mut harness, side);
    let doc = harness.state().document();
    let (board, _) = doc.container().instances().next().unwrap();
    assert_eq!(doc.material_of(woodsketch_doc::Entity::Instance(board)), Some(plywood));
    type_text(&mut harness, "Z");
    snapshot(&mut harness, "painted");
}

#[test]
fn save_new_and_reopen_a_drawing() {
    let dir = std::env::temp_dir().join(format!("woodsketch-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("bench.woodsketch");

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
    let junk = dir.join("junk.woodsketch");
    std::fs::write(&junk, "hello").unwrap();
    let error = harness.state_mut().open_path(&junk).unwrap_err();
    assert!(error.contains("isn't a WoodSketch drawing"), "{error}");
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
fn add_board(doc: &mut woodsketch_doc::Document, at: glam::DVec3, l: f64, w: f64, t: f64) -> woodsketch_doc::Entity {
    use glam::DVec3;
    let face = doc.edit(|m| m.add_rectangle(at, DVec3::X * w, DVec3::Y * t, DVec3::Z)).unwrap();
    woodsketch_doc::Entity::Instance(doc.push_pull(face, l, true).unwrap().unwrap())
}

#[test]
fn cut_list_shows_parts_and_cutting_diagrams() {
    use egui_kittest::kittest::Queryable;
    use egui_phosphor::regular as ph;
    use glam::DVec3;
    use woodsketch_doc::MaterialKind;

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
    use woodsketch_doc::Guide;
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
    // Off to the side: the tape still runs straight across from the edge, at the cursor.
    harness.hover_at(harness.state().to_window(DVec3::new(14.0, 4.0, 0.0)).unwrap());
    harness.run();
    snapshot(&mut harness, "tape_measuring");
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
    use woodsketch_ui_egui::files::ExportFormat;
    let dir = std::env::temp_dir().join(format!("woodsketch-export-{}", std::process::id()));
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
    // The Name field: the first text field (the Edge profile panel below has its own).
    harness.get_all_by_role(egui::accesskit::Role::TextInput).next().expect("name field").click();
    harness.run();
    // Replace the default "Group#1" while the field has focus; shortcuts don't fire.
    harness.key_press_modifiers(Modifiers::COMMAND, Key::A);
    type_text(&mut harness, "Left side");
    assert_eq!(harness.state().tools().active(), ToolId::Select, "typing a name isn't tool shortcuts");
    let doc = harness.state().document();
    let (board, _) = doc.container().instances().next().unwrap();
    assert_eq!(doc.instance_name(board), Some("Left side"));
    let list = woodsketch_doc::cutlist::cut_list(doc.scene());
    assert_eq!(list.materials[0].parts[0].names, vec!["Left side".to_owned()]);
    // One undo step undoes the whole name.
    harness.key_press(Key::Escape);
    harness.run();
    harness.state_mut().document_mut().undo();
    let doc = harness.state().document();
    assert_eq!(doc.instance_name(board), Some("Group#1"));
}

#[test]
fn erase_a_splitting_line_with_the_eraser() {
    use glam::DVec3;
    let mut harness = app();
    harness.state_mut().document_mut().edit(|m| {
        m.add_rectangle(DVec3::ZERO, DVec3::X * 24.0, DVec3::Y * 12.0, DVec3::Z);
        m.add_line(DVec3::new(8.0, 0.0, 0.0), DVec3::new(8.0, 12.0, 0.0));
    });
    harness.run();
    type_text(&mut harness, "Z");
    assert_eq!(harness.state().document().model().faces().count(), 2);

    type_text(&mut harness, "e");
    assert_eq!(harness.state().tools().active(), ToolId::Eraser);
    let on_line = harness.state().to_window(DVec3::new(8.0, 7.0, 0.0)).unwrap();
    click(&mut harness, on_line);
    let model = harness.state().document().model();
    assert_eq!(model.faces().count(), 1, "the two halves merged back into one face");
    assert!(model.edges().all(|(id, _)| !model.edge_faces(id).is_empty()), "the line is gone");
}

#[test]
fn drill_through_a_grouped_board() {
    use glam::DVec3;
    let mut harness = app();
    let board = {
        let doc = harness.state_mut().document_mut();
        let face = doc.edit(|m| m.add_rectangle(DVec3::ZERO, DVec3::X * 6.0, DVec3::Y * 4.0, DVec3::Z)).unwrap();
        doc.push_pull(face, 0.75, true).unwrap().unwrap()
    };
    harness.run();
    type_text(&mut harness, "Z");

    type_text(&mut harness, "c");
    let center = harness.state().to_window(DVec3::new(3.0, 2.0, 0.75)).unwrap();
    click(&mut harness, center);
    harness.hover_at(center + egui::vec2(30.0, 0.0));
    harness.run();
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

    let doc = harness.state().document();
    let def = doc.scene().definition(doc.scene().root.instance(board).unwrap().definition).unwrap();
    let model = &def.container.model;
    let holes: Vec<usize> = model.faces().filter(|(_, f)| f.normal.z.abs() > 0.5).map(|(_, f)| f.holes.len()).collect();
    assert_eq!(holes, vec![1, 1], "the board's top and bottom both have the hole");
    assert_eq!(doc.scene().root.model.faces().count(), 0, "nothing loose left on top of the board");
}

#[test]
fn square_push_pulled_then_a_hole_dragged_through_it() {
    use egui_kittest::kittest::Queryable;
    use glam::DVec3;
    let mut harness = app();
    // A 10" square, pulled up 3/4" into a board (auto-grouped).
    type_text(&mut harness, "r");
    click(&mut harness, pos2(600.0, 450.0));
    harness.hover_at(pos2(700.0, 380.0));
    harness.run();
    type_text(&mut harness, "10,10");
    harness.key_press(Key::Enter);
    harness.run();
    type_text(&mut harness, "Z");
    let (lo, hi) = harness.state().document().bounds().unwrap();
    let middle = (lo + hi) * 0.5;
    type_text(&mut harness, "p");
    let center = harness.state().to_window(middle).unwrap();
    click(&mut harness, center);
    harness.hover_at(center - egui::vec2(0.0, 40.0));
    harness.run();
    type_text(&mut harness, "3/4");
    harness.key_press(Key::Enter);
    harness.run();
    type_text(&mut harness, "Z");

    // A circle in the middle of the top.
    let top_center = harness.state().to_window(middle + DVec3::Z * 0.75).unwrap();
    type_text(&mut harness, "c");
    click(&mut harness, top_center);
    harness.hover_at(top_center + egui::vec2(40.0, 0.0));
    harness.run();
    type_text(&mut harness, "2");
    harness.key_press(Key::Enter);
    harness.run();

    // Push it down by mouse, well past the bottom.
    type_text(&mut harness, "p");
    click(&mut harness, top_center);
    click(&mut harness, top_center + egui::vec2(0.0, 250.0));

    let doc = harness.state().document();
    let all_faces: Vec<_> = doc
        .scene()
        .definitions()
        .flat_map(|(_, d)| d.container.model.faces().map(|(_, f)| f.clone()).collect::<Vec<_>>())
        .chain(doc.scene().root.model.faces().map(|(_, f)| f.clone()))
        .collect();
    let holes: Vec<usize> = all_faces.iter().filter(|f| f.normal.z.abs() > 0.5).map(|f| f.holes.len()).collect();
    assert_eq!(holes, vec![1, 1], "top and bottom both open: you can see through");
    assert_eq!(doc.scene().bounds().unwrap().0.z, 0.0, "nothing pokes out below");

    harness.key_press(Key::Escape);
    harness.run();
    harness.get_by_label("Top").click();
    harness.run();
    snapshot(&mut harness, "hole_top");
}

#[test]
fn an_open_group_is_boxed_and_labeled() {
    use glam::DVec3;
    let mut harness = app();
    let board = {
        let doc = harness.state_mut().document_mut();
        let face = doc.edit(|m| m.add_rectangle(DVec3::ZERO, DVec3::X * 6.0, DVec3::Y * 4.0, DVec3::Z)).unwrap();
        let board = doc.push_pull(face, 0.75, true).unwrap().unwrap();
        doc.rename_instance(board, "Shelf");
        board
    };
    harness.run();
    type_text(&mut harness, "Z");
    assert!(harness.state().tools().overlay(harness.state().document()).dashed.is_empty(), "no box at the top level");

    harness.state_mut().document_mut().enter(board);
    harness.run();
    let overlay = harness.state().tools().overlay(harness.state().document());
    assert_eq!(overlay.dashed.len(), 12, "a dashed box around the open group");
    assert_eq!(harness.state().document().path_names(), vec!["Shelf".to_owned()]);
    snapshot(&mut harness, "editing_group");
}

#[test]
fn ctrl_switches_the_tape_between_guides_and_measuring() {
    let mut harness = app();
    type_text(&mut harness, "t");
    harness.hover_at(pos2(600.0, 450.0));
    harness.run();
    assert!(harness.state().tools().status().starts_with("Create Guide Lines"));
    // Taps of Ctrl on its own cycle the modes, as in SketchUp.
    for expected in ["Create Guide Points", "Measure", "Create Guide Lines", "Create Guide Points", "Measure"] {
        harness.event(Event::ModifiersChanged(Modifiers::COMMAND));
        harness.run();
        harness.event(Event::ModifiersChanged(Modifiers::NONE));
        harness.run();
        let status = harness.state().tools().status();
        assert!(status.starts_with(expected), "{status}");
    }
    snapshot(&mut harness, "tape_measure_only");
}

#[test]
fn boards_hide_the_guides_behind_them() {
    use glam::DVec3;
    let mut harness = app();
    {
        let doc = harness.state_mut().document_mut();
        let face = doc.edit(|m| m.add_rectangle(DVec3::ZERO, DVec3::X * 12.0, DVec3::Y * 4.0, DVec3::Z)).unwrap();
        doc.push_pull(face, 12.0, true).unwrap();
        // A guide running behind the block (it's in front of the camera only past the block).
        doc.edit(|m| m.add_guide(woodsketch_doc::Guide::Line { point: DVec3::new(0.0, 7.0, 3.0), dir: DVec3::X }));
        doc.edit(|m| m.add_guide(woodsketch_doc::Guide::Line { point: DVec3::new(6.0, 0.0, 0.0), dir: DVec3::Z }));
    }
    harness.run();
    type_text(&mut harness, "Z");
    snapshot(&mut harness, "guides_behind_board");
}

#[test]
fn line_infers_parallel_to_a_hovered_edge() {
    use glam::DVec3;
    let mut harness = app();
    harness.state_mut().document_mut().edit(|m| {
        m.add_line(DVec3::new(0.0, 0.0, 0.0), DVec3::new(24.0, 12.0, 0.0));
        m.add_line(DVec3::new(0.0, 18.0, 0.0), DVec3::new(2.0, 18.0, 0.0));
    });
    harness.run();
    type_text(&mut harness, "Z");
    type_text(&mut harness, "l");
    // Pass over the slanted edge, then start from the short line's end.
    let on_edge = harness.state().to_window(DVec3::new(12.0, 6.0, 0.0)).unwrap();
    harness.hover_at(on_edge);
    harness.run();
    let start = harness.state().to_window(DVec3::new(0.0, 18.0, 0.0)).unwrap();
    click(&mut harness, start);
    let along = DVec3::new(24.0, 12.0, 0.0).normalize();
    let near = harness.state().to_window(DVec3::new(0.0, 18.0, 0.0) + along * 16.0).unwrap() + egui::vec2(2.0, 1.0);
    harness.hover_at(near);
    harness.run();
    let overlay = harness.state().tools().overlay(harness.state().document());
    assert_eq!(overlay.guide.and_then(|g| g.along), Some(woodsketch_tools::infer::Along::Parallel));
    snapshot(&mut harness, "parallel_inference");
}

#[test]
fn push_pull_and_offset_open_a_closed_board() {
    use glam::DVec3;
    let mut harness = app();
    let board = {
        let doc = harness.state_mut().document_mut();
        let face = doc.edit(|m| m.add_rectangle(DVec3::ZERO, DVec3::X * 12.0, DVec3::Y * 8.0, DVec3::Z)).unwrap();
        doc.push_pull(face, 0.75, true).unwrap().unwrap()
    };
    harness.run();
    type_text(&mut harness, "Z");

    // Pull the closed board's top up to 1 1/2" thick, straight from outside the group.
    type_text(&mut harness, "p");
    let top = harness.state().to_window(DVec3::new(6.0, 4.0, 0.75)).unwrap();
    harness.hover_at(top);
    harness.run();
    assert!(!harness.state().tools().overlay(harness.state().document()).highlight.is_empty(), "the face highlights");
    click(&mut harness, top);
    assert_eq!(harness.state().document().path(), &[board], "the board opened");
    harness.hover_at(top - egui::vec2(0.0, 30.0));
    harness.run();
    type_text(&mut harness, "3/4");
    harness.key_press(Key::Enter);
    harness.run();
    let (lo, hi) = harness.state().document().bounds().unwrap();
    assert!(((hi - lo).z - 1.5).abs() < 1e-9, "{}", hi - lo);

    // Close it, then Offset its top from outside: a 1" frame.
    harness.key_press(Key::Escape);
    harness.run();
    harness.key_press(Key::Escape);
    harness.run();
    assert!(harness.state().document().path().is_empty());
    type_text(&mut harness, "f");
    let top = harness.state().to_window(DVec3::new(6.0, 4.0, 1.5)).unwrap();
    click(&mut harness, top);
    assert_eq!(harness.state().document().path(), &[board]);
    harness.hover_at(harness.state().to_window(DVec3::new(2.0, 4.0, 1.5)).unwrap());
    harness.run();
    type_text(&mut harness, "1");
    harness.key_press(Key::Enter);
    harness.run();
    let model = harness.state().document().model();
    assert_eq!(model.faces().filter(|(_, f)| f.normal.z > 0.5).count(), 2, "frame and panel on the top");
}

#[test]
fn a_pine_stud_lists_as_a_2x4_and_buys_an_8_foot_board() {
    use egui_kittest::kittest::Queryable;
    use egui_phosphor::regular as ph;
    use glam::DVec3;
    let mut harness = app();
    {
        let doc = harness.state_mut().document_mut();
        let face = doc.edit(|m| m.add_rectangle(DVec3::ZERO, DVec3::X * 1.5, DVec3::Y * 3.5, DVec3::Z)).unwrap();
        doc.push_pull(face, 84.0, true).unwrap();
    }
    harness.run();
    type_text(&mut harness, "Z");
    harness.get_all_by_label(ph::PLUS).next().expect("Materials + button").click();
    harness.run();
    harness.get_by_label("Pine Lumber").click();
    harness.run();
    snapshot(&mut harness, "pine_lumber_editor");
    // Paint the stud: any face paints the whole board.
    let side = harness.state().to_window(DVec3::new(0.75, 0.0, 42.0)).unwrap();
    click(&mut harness, side);

    harness.get_by_label(ph::TABLE).click();
    harness.run();
    assert!(harness.query_by_label("2x4").is_some(), "the stud's stock is a 2x4");
    harness.get_by_label("Cutting diagrams").click();
    harness.run();
    assert!(harness.query_all_by_label_contains("1 × 2x4 x 96\"").next().is_some(), "buy one 8' 2x4");
    snapshot(&mut harness, "pine_stud_cut_list");
}

#[test]
fn the_cut_list_calls_out_parts_needing_a_material() {
    use egui_kittest::kittest::Queryable;
    use egui_phosphor::regular as ph;
    use glam::DVec3;
    let mut harness = app();
    {
        let doc = harness.state_mut().document_mut();
        let oak = doc.add_preset(woodsketch_doc::library::LIBRARY.iter().find(|p| p.name == "Red Oak").unwrap());
        let top = add_board(doc, DVec3::ZERO, 30.0, 12.0, 0.75);
        doc.paint(&[top], Some(oak));
        add_board(doc, DVec3::X * 20.0, 28.0, 3.5, 0.75);
    }
    harness.run();
    harness.get_by_label(ph::TABLE).click();
    harness.run();
    assert!(harness.query_all_by_label_contains("1 part needs a material").next().is_some());
    assert!(harness.query_by_label("Needs a material").is_some());
    snapshot(&mut harness, "cut_list_needs_material");
    // "Edit material" opens Red Oak's editor in the tray, without the Paint Bucket.
    harness.get_by_label_contains("Edit material").click();
    harness.run();
    assert!(harness.query_by_label("Editing Red Oak").is_some());
    assert_ne!(harness.state().tools().active(), ToolId::PaintBucket);
}

#[test]
fn entity_info_shows_a_parts_grain_and_lets_you_turn_it() {
    use egui_kittest::kittest::Queryable;
    use glam::DVec3;
    let mut harness = app();
    let door = {
        let doc = harness.state_mut().document_mut();
        let oak = doc.add_preset(woodsketch_doc::library::LIBRARY.iter().find(|p| p.name == "Red Oak").unwrap());
        // A 30" wide, 12" tall door standing up.
        let face = doc.edit(|m| m.add_rectangle(DVec3::ZERO, DVec3::X * 30.0, DVec3::Y * 0.75, DVec3::Z)).unwrap();
        let door = doc.push_pull(face, 12.0, true).unwrap().unwrap();
        doc.paint(&[woodsketch_doc::Entity::Instance(door)], Some(oak));
        doc.selection_mut().extend([woodsketch_doc::Entity::Instance(door)]);
        door
    };
    harness.run();
    type_text(&mut harness, "Z");
    assert!(harness.query_by_label("30\" x 12\" x 3/4\"").is_some(), "size, longest side first");
    assert!(harness.query_by_label("Red Oak, 3/4\" board").is_some());
    assert!(harness.state().tools().overlay(harness.state().document()).grain.is_some(), "a grain arrow");
    snapshot(&mut harness, "part_grain_along_length");
    // Turn the grain to run up the 12" side.
    harness.get(egui_kittest::kittest::by().role(egui::accesskit::Role::ComboBox).value("Along its length")).click();
    harness.run();
    harness.get_by_label("Along the 12\" side").click();
    harness.run();
    assert_eq!(harness.state().document().part_size(door), Some([12.0, 30.0, 0.75]));
    assert!(harness.query_by_label("12\" x 30\" x 3/4\"").is_some());
    snapshot(&mut harness, "part_grain_turned");
}

#[test]
fn board_tool_draws_a_stud_and_a_plywood_shelf_from_stock() {
    use egui_kittest::kittest::Queryable;
    let mut harness = app();
    type_text(&mut harness, "w");
    assert_eq!(harness.state().tools().active(), ToolId::Board);
    assert!(harness.query_by_label("Board").is_some(), "the Board panel shows");

    // A whole board typed: picks a 2x4 (adding Pine Lumber), then click, aim and type the length.
    type_text(&mut harness, "2x4x96");
    harness.key_press(Key::Enter);
    harness.run();
    assert_eq!(harness.state().tools().board().picked.as_ref().unwrap().name, "2x4");
    click(&mut harness, pos2(600.0, 450.0));
    harness.hover_at(pos2(760.0, 470.0));
    harness.run();
    type_text(&mut harness, "8'");
    harness.key_press(Key::Enter);
    harness.run();
    let doc = harness.state().document();
    let stud = doc.container().instances().next().expect("a board").0;
    assert_eq!(doc.part_size(stud), Some([96.0, 3.5, 1.5]));

    // Plywood from the library, at 3/4", drawn as a rectangle.
    harness.get(egui_kittest::kittest::by().role(egui::accesskit::Role::ComboBox).value("Pine Lumber")).click();
    harness.run();
    harness.get_by_label("Baltic Birch Plywood").click();
    harness.run();
    assert_eq!(harness.state().tools().board().picked.as_ref().unwrap().name, "3/4\" sheet", "sheets start at 3/4\"");
    click(&mut harness, pos2(500.0, 550.0));
    harness.hover_at(pos2(420.0, 600.0));
    harness.run();
    type_text(&mut harness, "30,11 1/4");
    harness.key_press(Key::Enter);
    harness.run();

    let list = woodsketch_doc::cutlist::cut_list(harness.state().document().scene());
    let names: Vec<&str> = list.materials.iter().map(|m| m.name.as_str()).collect();
    assert!(names.contains(&"Pine Lumber") && names.contains(&"Baltic Birch Plywood"), "{names:?}");
    let shelf = list.materials.iter().find(|m| m.name == "Baltic Birch Plywood").unwrap();
    let part = &shelf.parts[0];
    assert_eq!((part.length, part.width, part.thickness), (30.0, 11.25, 0.75));
    type_text(&mut harness, "Z");
    snapshot(&mut harness, "board_tool");
}

#[test]
fn board_panel_switches_material() {
    use egui_kittest::kittest::Queryable;
    let mut harness = app();
    type_text(&mut harness, "w");
    harness.get(egui_kittest::kittest::by().role(egui::accesskit::Role::ComboBox).value("Pick a material")).click();
    harness.run();
    harness.get_by_label("Pine Lumber").click();
    harness.run();
    harness.get(egui_kittest::kittest::by().role(egui::accesskit::Role::ComboBox).value("Pine Lumber")).click();
    harness.run();
    snapshot(&mut harness, "board_material_open");
    let walnut = harness.query_by_label("Walnut");
    assert!(walnut.is_some(), "other woods are offered");
    walnut.unwrap().click();
    harness.run();
    let picked = harness.state().tools().board().picked.clone().unwrap();
    let name = harness.state().document().scene().material(picked.material).unwrap().name.clone();
    assert_eq!(name, "Walnut");
}

#[test]
fn board_panel_adds_a_custom_lumber_size() {
    use egui_kittest::kittest::Queryable;
    let mut harness = app();
    type_text(&mut harness, "w");
    harness.get(egui_kittest::kittest::by().role(egui::accesskit::Role::ComboBox).value("Pick a material")).click();
    harness.run();
    harness.get_by_label("Pine Lumber").click();
    harness.run();
    harness.get(egui_kittest::kittest::by().role(egui::accesskit::Role::ComboBox).value("2x4 (1 1/2\" x 3 1/2\")")).click();
    harness.run();
    harness.get_by_label("Custom size...").click();
    harness.run();
    let count = harness.get_all_by_role(egui::accesskit::Role::TextInput).count();
    assert!(count >= 2, "thickness and width fields");
    for (index, text) in [(count - 2, "1 1/4"), (count - 1, "2 1/2")] {
        harness.get_all_by_role(egui::accesskit::Role::TextInput).nth(index).unwrap().click();
        harness.run();
        type_text(&mut harness, text);
    }
    snapshot(&mut harness, "board_custom_size");
    harness.get_by_label("Add to Pine Lumber").click();
    harness.run();
    let picked = harness.state().tools().board().picked.clone().unwrap();
    assert_eq!(picked.stock, woodsketch_doc::board::Stock::Section { thickness: 1.25, width: 2.5 });
    assert!(harness.query_by_label("Add to Pine Lumber").is_none(), "the form closes");
}

#[test]
fn joinery_panel_cuts_a_dado_and_makes_a_mortise_and_tenon() {
    use egui_kittest::kittest::Queryable;
    use glam::DVec3;
    use woodsketch_doc::board::Board;
    let mut harness = app();
    let (side, shelf, stile, rail) = {
        let doc = harness.state_mut().document_mut();
        let oak = doc.add_preset(woodsketch_doc::library::LIBRARY.iter().find(|p| p.name == "Red Oak").unwrap());
        let mut add = |origin: DVec3, length_dir: DVec3, width_dir: DVec3, [length, width, thickness]: [f64; 3]| {
            doc.add_board(&Board { origin, length_dir, width_dir, length, width, thickness }, Some(oak)).unwrap()
        };
        // A side standing up and a shelf reaching 1/4" into it.
        let side = add(DVec3::ZERO, DVec3::Z, DVec3::Y, [30.0, 11.25, 0.75]);
        let shelf = add(DVec3::new(-0.25, 0.0, 12.0), DVec3::X, DVec3::Y, [20.0, 11.25, 0.75]);
        // A stile and a rail butting into it, off to the side.
        let stile = add(DVec3::new(40.0, 0.0, 0.0), DVec3::Z, DVec3::X, [30.0, 3.0, 1.5]);
        let rail = add(DVec3::new(43.0, 0.0, 12.5), DVec3::X, -DVec3::Z, [20.0, 2.5, 1.5]);
        (side, shelf, stile, rail)
    };
    let select = |harness: &mut Harness<'_, WoodSketchApp>, parts: [woodsketch_doc::InstanceId; 2]| {
        let doc = harness.state_mut().document_mut();
        doc.selection_mut().clear();
        doc.selection_mut().extend(parts.map(woodsketch_doc::Entity::Instance));
        harness.run();
    };
    select(&mut harness, [shelf, side]);
    type_text(&mut harness, "Z");
    assert!(harness.query_by_label("Joinery").is_some(), "the Joinery panel shows for two parts");
    harness.get_by_label("Cut Group#2 into Group#1").click();
    harness.run();
    assert_eq!(harness.state().document().joints_of(side).len(), 1);
    assert!(harness.query_by_label("Dado cut in Group#1.").is_some());
    snapshot(&mut harness, "joinery_dado");

    select(&mut harness, [rail, stile]);
    // Butted: a dado or rabbet is offered first; pick mortise and tenon instead.
    assert!(harness.query_by_label("Let Group#4 into Group#3").is_some());
    harness.get(egui_kittest::kittest::by().role(egui::accesskit::Role::ComboBox).value("Dado, groove or rabbet")).click();
    harness.run();
    harness.get_by_label("Mortise and tenon").click();
    harness.run();
    harness.get_by_label("Tenon on Group#4, mortise in Group#3").click();
    harness.run();
    let doc = harness.state().document();
    assert_eq!(doc.joints_of(stile).len(), 1);
    assert!(doc.part_size(rail).unwrap()[0] > 20.0, "the rail is longer by its tenon");
    // Entity Info lists a part's joints.
    harness.state_mut().document_mut().selection_mut().clear();
    harness.state_mut().document_mut().selection_mut().extend([woodsketch_doc::Entity::Instance(side)]);
    harness.run();
    assert!(harness.query_by_label_contains("Dado with Group#2").is_some());
    // Pull the shelf and rail out to see the dado, tenon and mortise.
    let doc = harness.state_mut().document_mut();
    doc.transform_entities(&[woodsketch_doc::Entity::Instance(shelf)], &[glam::DAffine3::from_translation(DVec3::new(6.0, 0.0, 6.0))], false).unwrap();
    doc.transform_entities(&[woodsketch_doc::Entity::Instance(rail)], &[glam::DAffine3::from_translation(DVec3::new(6.0, 0.0, 0.0))], false).unwrap();
    doc.selection_mut().clear();
    harness.run();
    type_text(&mut harness, "Z");
    snapshot(&mut harness, "joinery_apart");
}

#[test]
fn joinery_panel_lets_a_butted_top_into_a_rabbet() {
    use egui_kittest::kittest::Queryable;
    use glam::DVec3;
    use woodsketch_doc::board::Board;
    let mut harness = app();
    let (side, top) = {
        let doc = harness.state_mut().document_mut();
        let pine = doc.add_preset(woodsketch_doc::library::LIBRARY.iter().find(|p| p.name == "Pine").unwrap());
        let mut add = |origin: DVec3, length_dir: DVec3, width_dir: DVec3, [length, width, thickness]: [f64; 3]| {
            doc.add_board(&Board { origin, length_dir, width_dir, length, width, thickness }, Some(pine)).unwrap()
        };
        let side = add(DVec3::ZERO, DVec3::Z, DVec3::Y, [30.0, 11.25, 0.75]);
        // The top butts the side's inner face, flush with its top.
        let top = add(DVec3::new(0.0, 0.0, 29.25), DVec3::X, DVec3::Y, [20.0, 11.25, 0.75]);
        let doc = harness.state_mut().document_mut();
        doc.selection_mut().extend([woodsketch_doc::Entity::Instance(top), woodsketch_doc::Entity::Instance(side)]);
        (side, top)
    };
    harness.run();
    type_text(&mut harness, "Z");
    // Every joint is listed; the ones that need the parts placed differently say so.
    harness.get(egui_kittest::kittest::by().role(egui::accesskit::Role::ComboBox).value("Dado, groove or rabbet")).click();
    harness.run();
    assert!(harness.query_by_label("Half-lap (place the parts first)").is_some());
    assert!(harness.query_by_label("Box joint").is_some(), "a top flush with the side's end is a corner");
    snapshot(&mut harness, "joinery_picker");
    harness.get_by_label("Dado, groove or rabbet").click();
    harness.run();
    harness.get_by_label("Let Group#2 into Group#1").click();
    harness.run();
    let doc = harness.state().document();
    assert_eq!(doc.joints_of(side)[0].0.kind, woodsketch_doc::joinery::JointKind::Rabbet);
    assert_eq!(doc.part_size(top).unwrap()[0], 20.25, "the top grew into the rabbet");
    assert!(harness.query_by_label_contains("Rabbet cut in Group#1").is_some());
}

#[test]
fn a_crash_leaves_unsaved_work_to_recover() {
    use egui_kittest::kittest::Queryable;
    let dir = std::env::temp_dir().join(format!("woodsketch-recover-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // What a crashed session left: a drawing with a board, and the file it belonged to.
    let mut doc = woodsketch_doc::Document::default();
    doc.edit(|m| m.add_rectangle(glam::DVec3::ZERO, glam::DVec3::X * 3.0, glam::DVec3::Y * 2.0, glam::DVec3::Z));
    std::fs::write(dir.join("autosave-1.woodsketch"), woodsketch_doc::file::to_string(doc.scene())).unwrap();
    std::fs::write(dir.join("autosave-1.path"), "C:/Shop/Nightstand.woodsketch").unwrap();

    let mut harness = app();
    harness.state_mut().use_autosave_folder(dir.clone());
    harness.run();
    assert!(harness.query_by_label("Recover unsaved work?").is_some());
    assert!(harness.query_by_label_contains("Nightstand").is_some());
    snapshot(&mut harness, "recover");
    harness.get_by_label("Recover").click();
    harness.run();
    assert_eq!(harness.state().document().model().faces().count(), 1, "the work is back");
    assert!(harness.state().is_dirty(), "and not saved yet");
    assert_eq!(harness.state().file_path(), Some(std::path::Path::new("C:/Shop/Nightstand.woodsketch")));
    assert!(harness.query_by_label("Recover unsaved work?").is_none());
    assert!(!dir.join("autosave-1.woodsketch").exists(), "the recovered copy is cleaned up");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn outliner_shows_the_whole_tree_and_picks_nested_parts() {
    use egui_kittest::kittest::Queryable;
    use glam::DVec3;
    use woodsketch_doc::board::Board;
    let mut harness = app();
    let (assembly, legs) = {
        let doc = harness.state_mut().document_mut();
        let oak = doc.add_preset(woodsketch_doc::library::LIBRARY.iter().find(|p| p.name == "Red Oak").unwrap());
        let painted = doc.add_board(&Board { origin: DVec3::ZERO, length_dir: DVec3::Z, width_dir: DVec3::X, length: 24.0, width: 1.5, thickness: 1.5 }, Some(oak)).unwrap();
        let bare = doc.add_board(&Board { origin: DVec3::new(10.0, 0.0, 0.0), length_dir: DVec3::Z, width_dir: DVec3::X, length: 24.0, width: 1.5, thickness: 1.5 }, None).unwrap();
        doc.rename_instance(painted, "Front leg");
        doc.rename_instance(bare, "Back leg");
        doc.selection_mut().extend([woodsketch_doc::Entity::Instance(painted), woodsketch_doc::Entity::Instance(bare)]);
        let assembly = doc.group_selection(woodsketch_doc::DefinitionKind::Group).unwrap();
        doc.rename_instance(assembly, "Base");
        doc.selection_mut().clear();
        // Open the base once, so its fold in the Outliner is open.
        doc.enter(assembly);
        (assembly, [painted, bare])
    };
    let _ = legs;
    harness.run();
    assert!(harness.query_by_label_contains("Front leg").is_some() && harness.query_by_label_contains("Back leg").is_some(), "nested parts are listed");
    assert_eq!(harness.get_all_by_label(egui_phosphor::regular::WARNING).count(), 1, "only the unpainted leg needs a material");
    harness.state_mut().document_mut().exit();
    harness.run();
    snapshot(&mut harness, "outliner_tree");
    // From the top level, clicking a leg inside the base opens the base and selects the leg.
    harness.get_by_label_contains("Back leg").click();
    harness.run();
    let doc = harness.state().document();
    assert_eq!(doc.path(), &[assembly]);
    assert_eq!(doc.selection().len(), 1);
    let picked = doc.selection().iter().next().unwrap();
    let woodsketch_doc::Entity::Instance(picked) = picked else { panic!("a part") };
    assert_eq!(doc.instance_name(picked), Some("Back leg"));
}

#[test]
fn edge_profile_panel_rounds_over_a_tabletop_and_chamfers_a_leg() {
    use egui_kittest::kittest::Queryable;
    use glam::DVec3;
    use woodsketch_doc::board::Board;
    let mut harness = app();
    let (top, leg) = {
        let doc = harness.state_mut().document_mut();
        let cherry = doc.add_preset(woodsketch_doc::library::LIBRARY.iter().find(|p| p.name == "Cherry").unwrap());
        let top = doc.add_board(&Board { origin: DVec3::new(0.0, 0.0, 28.0), length_dir: DVec3::X, width_dir: DVec3::Y, length: 24.0, width: 16.0, thickness: 0.75 }, Some(cherry)).unwrap();
        let leg = doc.add_board(&Board { origin: DVec3::new(30.0, 0.0, 0.0), length_dir: DVec3::Z, width_dir: DVec3::X, length: 28.0, width: 1.5, thickness: 1.5 }, Some(cherry)).unwrap();
        doc.selection_mut().extend([woodsketch_doc::Entity::Instance(top)]);
        (top, leg)
    };
    harness.run();
    assert!(harness.query_by_label("Edge profile").is_some());
    assert!(harness.query(egui_kittest::kittest::by().role(egui::accesskit::Role::ComboBox).value("Around the top")).is_some(), "a tabletop starts with its top edges");
    harness.get_by_label("Rout the roundover").click();
    harness.run();
    assert_eq!(harness.state().document().profiles_of(top).len(), 1);
    assert!(harness.query_by_label_contains("Roundover, 4 edges").is_some(), "listed in Entity Info");

    let doc = harness.state_mut().document_mut();
    doc.selection_mut().clear();
    doc.selection_mut().extend([woodsketch_doc::Entity::Instance(leg)]);
    harness.run();
    assert!(harness.query(egui_kittest::kittest::by().role(egui::accesskit::Role::ComboBox).value("Around the top")).is_none(), "a leg starts along its length");
    harness.get(egui_kittest::kittest::by().role(egui::accesskit::Role::ComboBox).value("Roundover")).click();
    harness.run();
    harness.get_by_label("Chamfer").click();
    harness.run();
    harness.get_by_label("Rout the chamfer").click();
    harness.run();
    let doc = harness.state().document();
    assert_eq!(doc.profiles_of(leg)[0].describe(), "1/4\" Chamfer, 4 edges", "a standing leg starts with its four long corners");
    harness.state_mut().document_mut().selection_mut().clear();
    type_text(&mut harness, "Z");
    snapshot(&mut harness, "edge_profiles");
}

#[test]
fn build_sheets_for_a_small_table() {
    use egui_kittest::kittest::Queryable;
    use glam::DVec3;
    use woodsketch_doc::board::Board;
    let mut harness = app();
    {
        let doc = harness.state_mut().document_mut();
        let cherry = doc.add_preset(woodsketch_doc::library::LIBRARY.iter().find(|p| p.name == "Cherry").unwrap());
        let mut add = |name: &str, origin: DVec3, length_dir: DVec3, width_dir: DVec3, [length, width, thickness]: [f64; 3]| {
            let id = doc.add_board(&Board { origin, length_dir, width_dir, length, width, thickness }, Some(cherry)).unwrap();
            doc.rename_instance(id, name);
            id
        };
        // A leg and an apron rail butting it, and a top.
        let leg = add("Leg", DVec3::ZERO, DVec3::Z, DVec3::X, [28.0, 1.75, 1.75]);
        let apron = add("Apron", DVec3::new(1.75, 0.25, 27.0), DVec3::X, -DVec3::Z, [18.0, 4.0, 0.75]);
        let top = add("Top", DVec3::new(-1.0, -1.0, 28.0), DVec3::X, DVec3::Y, [24.0, 16.0, 0.75]);
        doc.profile_edges(leg, woodsketch_doc::profile::Profile::Chamfer, 0.125, woodsketch_doc::profile::Edges::Along { axis: 0, corners: [true; 4] }).unwrap();
        doc.profile_edges(top, woodsketch_doc::profile::Profile::Roundover, 0.25, woodsketch_doc::profile::Edges::AroundFace { axis: 2, positive: true, sides: [true; 4] }).unwrap();
        let fit = doc.joint_fit(apron, leg).unwrap();
        doc.mortise_and_tenon(apron, leg, fit.tenon).unwrap();
    }
    harness.run();
    let html = harness.state_mut().build_sheets_html();
    assert_eq!(html.matches("<section class=\"sheet\">").count(), 3);
    assert!(html.contains("Cut the mortise for Apron") && html.contains("Cut the tenon that goes into Leg"));
    assert!(html.contains("Rout the edges: 1/4\" Roundover, 4 edges."));
    if let Ok(dir) = std::env::var("WOODSKETCH_SNAPSHOT_DIR") {
        std::fs::write(std::path::Path::new(&dir).join("build_sheets.html"), &html).unwrap();
    }
    // The Cut List window offers it.
    harness.get_by_label(egui_phosphor::regular::TABLE).click();
    harness.run();
    assert!(harness.query_by_label_contains("Print build sheets").is_some());
}

#[test]
fn wood_movement_tab_warns_about_a_top_fixed_to_its_aprons() {
    use egui_kittest::kittest::Queryable;
    use glam::DVec3;
    use woodsketch_doc::board::Board;
    let mut harness = app();
    {
        let doc = harness.state_mut().document_mut();
        let cherry = doc.add_preset(woodsketch_doc::library::LIBRARY.iter().find(|p| p.name == "Cherry").unwrap());
        let mut add = |name: &str, origin: DVec3, length_dir: DVec3, width_dir: DVec3, [length, width, thickness]: [f64; 3]| {
            let id = doc.add_board(&Board { origin, length_dir, width_dir, length, width, thickness }, Some(cherry)).unwrap();
            doc.rename_instance(id, name);
        };
        // A 30" x 18" top (grain along X) resting on two side aprons running front to back (Y).
        add("Top", DVec3::new(0.0, 0.0, 28.0), DVec3::X, DVec3::Y, [30.0, 18.0, 0.75]);
        add("Left apron", DVec3::new(1.0, 1.0, 28.0), DVec3::Y, -DVec3::Z, [16.0, 4.0, 0.75]);
        add("Right apron", DVec3::new(28.25, 1.0, 28.0), DVec3::Y, -DVec3::Z, [16.0, 4.0, 0.75]);
    }
    harness.run();
    harness.get_by_label(egui_phosphor::regular::TABLE).click();
    harness.run();
    harness.get_by_label("Wood movement").click();
    harness.run();
    assert_eq!(harness.query_all_by_label_contains("Top moves about").count(), 2, "the top's width runs along both side aprons' grain");
    assert!(harness.query_all_by_label_contains("Left apron").count() >= 1);
    snapshot(&mut harness, "wood_movement");
}
