// No console window in release builds on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui;
use woodsketch_ui_egui::WoodSketchApp;

mod update;

fn main() -> eframe::Result {
    // First: the installer and updater run their setup steps through the app, and exit there.
    velopack::VelopackApp::build().run();
    env_logger::init();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("WoodSketch")
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([640.0, 400.0])
            .with_maximized(true),
        renderer: eframe::Renderer::Wgpu,
        // Open maximized (as SketchUp does) and centered when restored, so the window never
        // starts partly off a scaled-up screen.
        centered: true,
        // Settings (recent files) are kept between runs, but not the window: it always opens
        // maximized and centered.
        persist_window: false,
        ..Default::default()
    };
    // `woodsketch drawing.woodsketch` opens that drawing.
    let open = std::env::args_os().nth(1).map(std::path::PathBuf::from);
    eframe::run_native(
        "WoodSketch",
        options,
        Box::new(move |cc| {
            let mut app = WoodSketchApp::new(cc);
            app.set_updater(update::updater(cc.egui_ctx.clone()));
            if let Some(path) = open
                && let Err(error) = app.open_path(&path)
            {
                app.show_error(error);
            }
            Ok(Box::new(app))
        }),
    )
}
