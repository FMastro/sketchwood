//! The SketchCraft window: toolbar, 3D viewport, status bar and Measurements box.

mod icons;
mod keys;
mod viewport;

use eframe::egui;
use sketchcraft_doc::Document;
use sketchcraft_tools::{ToolId, Tools};
use sketchcraft_view::StandardView;
use viewport::Viewport;

pub struct SketchCraftApp {
    doc: Document,
    tools: Tools,
    viewport: Viewport,
}

impl SketchCraftApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let render_state = cc
            .wgpu_render_state
            .as_ref()
            .expect("SketchCraft needs eframe's wgpu renderer");
        icons::install(&cc.egui_ctx);
        Self { doc: Document::default(), tools: Tools::default(), viewport: Viewport::new(render_state) }
    }

    pub fn document(&self) -> &Document {
        &self.doc
    }

    pub fn tools(&self) -> &Tools {
        &self.tools
    }

    /// Where a model point (inches) appears in the window. For tests that click on geometry.
    pub fn to_window(&self, point: glam::DVec3) -> Option<egui::Pos2> {
        self.viewport.to_window(point)
    }
}

impl eframe::App for SketchCraftApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        keys::handle(ui.ctx(), &mut self.doc, &mut self.tools, &mut self.viewport);

        egui::Panel::top("toolbar").show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                for (i, group) in TOOL_GROUPS.iter().enumerate() {
                    if i > 0 {
                        ui.separator();
                    }
                    for &tool in *group {
                        let hint = format!("{} ({})", tool.name(), tool.shortcut_label());
                        let hint = if tool.is_available() { hint } else { format!("{hint}: coming soon") };
                        let selected = self.tools.active() == tool;
                        if icon_button(ui, icons::tool(tool), selected, tool.is_available(), &hint) {
                            self.tools.activate(tool);
                        }
                    }
                }
                if icon_button(ui, icons::ZOOM_EXTENTS, false, true, "Zoom Extents (Shift+Z)") {
                    self.viewport.request_zoom_extents();
                }
                ui.separator();
                for (label, view) in [
                    (format!("{} Iso", icons::ISO), StandardView::Iso),
                    ("Top".into(), StandardView::Top),
                    ("Front".into(), StandardView::Front),
                    ("Right".into(), StandardView::Right),
                ] {
                    let response = ui.add(egui::Button::new(label).min_size(egui::vec2(0.0, BUTTON_SIZE)));
                    if response.clicked() {
                        response.surrender_focus();
                        self.viewport.camera.set_view(view);
                    }
                }
                ui.separator();
                if icon_button(ui, icons::UNDO, false, true, "Undo (Ctrl+Z)") {
                    self.tools.cancel();
                    self.doc.undo();
                }
                if icon_button(ui, icons::REDO, false, true, "Redo (Ctrl+Y)") {
                    self.tools.cancel();
                    self.doc.redo();
                }
            });
        });

        egui::Panel::bottom("status").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    measurements_box(ui, &self.tools);
                    ui.label(self.tools.measurement_label());
                    ui.separator();
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.add(egui::Label::new(self.tools.status()).truncate());
                    });
                });
            });
        });

        egui::CentralPanel::no_frame()
            .show(ui, |ui| self.viewport.show(ui, frame, &mut self.doc, &mut self.tools));
    }
}

const BUTTON_SIZE: f32 = 32.0;

/// Toolbar groups, ordered like SketchUp's Large Tool Set.
const TOOL_GROUPS: &[&[ToolId]] = &[
    &[ToolId::Select, ToolId::Eraser, ToolId::PaintBucket],
    &[ToolId::Line, ToolId::Arc, ToolId::Rectangle, ToolId::Circle],
    &[ToolId::PushPull, ToolId::Move, ToolId::Rotate, ToolId::Scale, ToolId::Offset],
    &[ToolId::TapeMeasure],
    &[ToolId::Orbit, ToolId::Pan, ToolId::Zoom],
];

/// A square icon button with a tooltip. Returns whether it was clicked.
fn icon_button(ui: &mut egui::Ui, icon: &str, selected: bool, enabled: bool, hint: &str) -> bool {
    let button = egui::Button::selectable(selected, egui::RichText::new(icon).size(20.0))
        .min_size(egui::vec2(BUTTON_SIZE, BUTTON_SIZE));
    let response = ui.add_enabled(enabled, button);
    let response = if enabled { response.on_hover_text(hint) } else { response.on_disabled_hover_text(hint) };
    if response.clicked() {
        // Don't keep keyboard focus, or Space would press the button again.
        response.surrender_focus();
        return true;
    }
    false
}

/// SketchUp's Measurements box: shows live values, and whatever is being typed.
fn measurements_box(ui: &mut egui::Ui, tools: &Tools) {
    let text = tools.measurement();
    let color = if tools.is_typing() { ui.visuals().strong_text_color() } else { ui.visuals().text_color() };
    egui::Frame::new()
        .fill(ui.visuals().extreme_bg_color)
        .stroke(ui.visuals().widgets.inactive.bg_stroke)
        .corner_radius(3.0)
        .inner_margin(egui::Margin::symmetric(6, 2))
        .show(ui, |ui| {
            ui.set_min_width(150.0);
            ui.label(egui::RichText::new(text).monospace().color(color));
        });
}
