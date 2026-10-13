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
fn unavailable_tool_shortcut_explains() {
    let mut harness = app();
    type_text(&mut harness, "l");
    assert_eq!(harness.state().tools().active(), ToolId::Select);
    assert!(harness.state().tools().status().contains("Line (L) is coming soon"));
}
