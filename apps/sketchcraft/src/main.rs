// No console window in release builds on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui;
use sketchcraft_ui_egui::SketchCraftApp;

fn main() -> eframe::Result {
    env_logger::init();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("SketchCraft")
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([640.0, 400.0])
            .with_maximized(true),
        renderer: eframe::Renderer::Wgpu,
        // Open maximized (as SketchUp does) and centered when restored, so the window never
        // starts partly off a scaled-up screen.
        centered: true,
        ..Default::default()
    };
    // `sketchcraft drawing.scraft` opens that drawing.
    let open = std::env::args_os().nth(1).map(std::path::PathBuf::from);
    eframe::run_native(
        "SketchCraft",
        options,
        Box::new(move |cc| {
            let mut app = SketchCraftApp::new(cc);
            if let Some(path) = open
                && let Err(error) = app.open_path(&path)
            {
                app.show_error(error);
            }
            Ok(Box::new(app))
        }),
    )
}
