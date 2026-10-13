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
    let (lo, hi) = harness.state().document().model().bounds().expect("geometry exists");
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

    let model = harness.state().document().model();
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
    assert_eq!(harness.state().document().model().faces().count(), 1);
}

#[test]
fn clicking_a_face_selects_it() {
    let mut harness = app();
    type_text(&mut harness, "r");
    let corner = pos2(600.0, 450.0);
    click(&mut harness, corner);
    let far = pos2(760.0, 360.0);
    click(&mut harness, far);
    assert_eq!(harness.state().document().model().faces().count(), 1);

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
    let (lo, hi) = harness.state().document().model().bounds().unwrap();
    let center = harness.state().to_window((lo + hi) * 0.5).unwrap();
    click(&mut harness, center);
    // Pull upward on screen, then type the length.
    harness.hover_at(center - egui::vec2(0.0, 60.0));
    harness.run();
    type_text(&mut harness, "8'");
    harness.key_press(Key::Enter);
    harness.run();

    let model = harness.state().document().model();
    assert_eq!(model.faces().count(), 6);
    let (lo, hi) = model.bounds().unwrap();
    assert_eq!((hi - lo).to_array(), [1.5, 3.5, 96.0]);
    assert_eq!(harness.state().tools().measurement(), "96\"");

    type_text(&mut harness, "Z");
    snapshot(&mut harness, "board");
}

#[test]
fn unavailable_tool_shortcut_explains() {
    let mut harness = app();
    type_text(&mut harness, "m");
    assert_eq!(harness.state().tools().active(), ToolId::Select);
    assert!(harness.state().tools().status().contains("Move (M) is coming soon"));
}
