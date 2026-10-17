//! Toolbar icons from the Phosphor icon font (MIT), drawn as text glyphs.

use eframe::egui;
use egui_phosphor::regular as ph;
use woodsketch_tools::ToolId;

/// Register the icon font as a fallback for egui's default fonts.
pub fn install(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
    ctx.set_fonts(fonts);
}

pub fn tool(tool: ToolId) -> &'static str {
    match tool {
        ToolId::Select => ph::CURSOR,
        ToolId::Eraser => ph::ERASER,
        ToolId::PaintBucket => ph::PAINT_BUCKET,
        ToolId::Line => ph::LINE_SEGMENT,
        ToolId::Arc => ph::BEZIER_CURVE,
        ToolId::Rectangle => ph::RECTANGLE,
        ToolId::Circle => ph::CIRCLE,
        ToolId::Board => ph::LOG,
        ToolId::PushPull => ph::ARROW_FAT_LINES_UP,
        ToolId::Move => ph::ARROWS_OUT_CARDINAL,
        ToolId::Rotate => ph::ARROW_CLOCKWISE,
        ToolId::Scale => ph::RESIZE,
        ToolId::Offset => ph::FRAME_CORNERS,
        ToolId::TapeMeasure => ph::RULER,
        ToolId::Orbit => ph::ARROWS_CLOCKWISE,
        ToolId::Pan => ph::HAND,
        ToolId::Zoom => ph::MAGNIFYING_GLASS,
    }
}

pub const ZOOM_EXTENTS: &str = ph::CORNERS_OUT;
pub const ISO: &str = ph::CUBE;
pub const UNDO: &str = ph::ARROW_U_UP_LEFT;
pub const REDO: &str = ph::ARROW_U_UP_RIGHT;
pub const CUT_LIST: &str = ph::TABLE;
pub const NEW: &str = ph::FILE;
pub const OPEN: &str = ph::FOLDER_OPEN;
pub const SAVE: &str = ph::FLOPPY_DISK;
pub const CLOSE: &str = ph::X;
pub const HELP: &str = ph::QUESTION;
