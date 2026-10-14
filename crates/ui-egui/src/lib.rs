//! The SketchCraft window: toolbar, 3D viewport, status bar and Measurements box.

mod files;
mod icons;
mod keys;
mod materials;
mod outliner;
mod tags;
mod viewport;

use std::path::{Path, PathBuf};

use eframe::egui;
use sketchcraft_doc::Document;
use sketchcraft_tools::{ToolId, Tools};
use sketchcraft_view::StandardView;
use viewport::Viewport;

pub struct SketchCraftApp {
    doc: Document,
    tools: Tools,
    keyboard: keys::Keyboard,
    tags: tags::TagsPanel,
    materials: materials::MaterialsPanel,
    viewport: Viewport,
    files: files::Files,
}

impl SketchCraftApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let render_state = cc
            .wgpu_render_state
            .as_ref()
            .expect("SketchCraft needs eframe's wgpu renderer");
        icons::install(&cc.egui_ctx);
        Self {
            doc: Document::default(),
            tools: Tools::default(),
            keyboard: keys::Keyboard::default(),
            tags: tags::TagsPanel::default(),
            materials: materials::MaterialsPanel::default(),
            viewport: Viewport::new(render_state),
            files: files::Files::default(),
        }
    }

    /// Replace the drawing (New or Open), resetting everything that pointed into the old one.
    fn replace_document(&mut self, doc: Document, path: Option<PathBuf>) {
        self.doc = doc;
        self.tools.cancel();
        self.tools.active_material = None;
        self.tags = tags::TagsPanel::default();
        self.materials = materials::MaterialsPanel::default();
        self.viewport.document_replaced(&self.doc);
        self.files.path = path;
        self.files.mark_saved(&self.doc);
    }

    /// Start a new, empty drawing (without asking about unsaved changes).
    pub fn new_document(&mut self) {
        self.replace_document(Document::default(), None);
    }

    /// Open the drawing at `path` (without asking about unsaved changes).
    pub fn open_path(&mut self, path: &Path) -> Result<(), String> {
        let doc = files::Files::read(path)?;
        self.replace_document(doc, Some(path.to_owned()));
        Ok(())
    }

    /// Save the drawing to `path`, which becomes its file.
    pub fn save_to(&mut self, path: &Path) -> Result<(), String> {
        self.files.save_to(&self.doc, path)
    }

    /// Whether there are changes since the drawing was opened or last saved.
    pub fn is_dirty(&self) -> bool {
        self.files.is_dirty(&self.doc)
    }

    pub fn file_path(&self) -> Option<&Path> {
        self.files.path.as_deref()
    }

    pub fn show_error(&mut self, error: String) {
        self.files.show_error(error);
    }

    /// Save to the drawing's file, asking where if it has none. Returns whether it saved.
    fn save(&mut self, choose: bool) -> bool {
        let path = match (&self.files.path, choose) {
            (Some(path), false) => path.clone(),
            _ => match files::save_dialog(self.files.path.as_deref()) {
                Some(path) => path,
                None => return false,
            },
        };
        match self.save_to(&path) {
            Ok(()) => true,
            Err(error) => {
                self.files.show_error(error);
                false
            }
        }
    }

    /// New, Open or Close, after any unsaved changes are dealt with.
    fn carry_out(&mut self, ctx: &egui::Context, action: files::Discard) {
        match action {
            files::Discard::New => self.new_document(),
            files::Discard::Open => {
                if let Some(path) = files::open_dialog(self.files.path.as_deref())
                    && let Err(error) = self.open_path(&path)
                {
                    self.files.show_error(error);
                }
            }
            files::Discard::Close => self.files.close(ctx),
        }
    }

    /// New, Open or Close, asking "save changes?" first if needed.
    fn request(&mut self, ctx: &egui::Context, action: files::Discard) {
        if self.files.request(&self.doc, action) {
            self.carry_out(ctx, action);
        }
    }

    fn file_menu(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        ui.menu_button("File", |ui| {
            if ui.add(egui::Button::new("New").shortcut_text("Ctrl+N")).clicked() {
                ui.close();
                self.request(&ctx, files::Discard::New);
            }
            if ui.add(egui::Button::new("Open...").shortcut_text("Ctrl+O")).clicked() {
                ui.close();
                self.request(&ctx, files::Discard::Open);
            }
            ui.separator();
            if ui.add(egui::Button::new("Save").shortcut_text("Ctrl+S")).clicked() {
                ui.close();
                self.save(false);
            }
            if ui.add(egui::Button::new("Save As...").shortcut_text("Ctrl+Shift+S")).clicked() {
                ui.close();
                self.save(true);
            }
            ui.separator();
            if ui.button("Exit").clicked() {
                ui.close();
                self.request(&ctx, files::Discard::Close);
            }
        });
    }

    /// The File menu's shortcuts and dialogs, and the window title.
    fn handle_files(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| i.viewport().close_requested()) {
            self.files.on_close_requested(ctx, &self.doc);
        }
        let command = egui::Modifiers::COMMAND;
        let (save_as, save, open, new) = ctx.input_mut(|i| {
            (
                i.consume_key(command | egui::Modifiers::SHIFT, egui::Key::S),
                i.consume_key(command, egui::Key::S),
                i.consume_key(command, egui::Key::O),
                i.consume_key(command, egui::Key::N),
            )
        });
        if save_as {
            self.save(true);
        } else if save {
            self.save(false);
        }
        if open {
            self.request(ctx, files::Discard::Open);
        }
        if new {
            self.request(ctx, files::Discard::New);
        }
        match self.files.dialogs(ctx) {
            Some(files::FileAction::SaveThen(action)) => {
                if self.save(false) {
                    self.carry_out(ctx, action);
                }
            }
            Some(files::FileAction::Discard(action)) => self.carry_out(ctx, action),
            None => {}
        }
        self.files.update_title(ctx, &self.doc);
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
        let ctx = ui.ctx().clone();
        self.handle_files(&ctx);
        if !self.files.is_asking() {
            self.keyboard.handle(&ctx, &mut self.doc, &mut self.tools, &mut self.viewport);
        }

        egui::Panel::top("toolbar").show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                self.file_menu(ui);
                ui.separator();
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

        egui::Panel::left("sidebar").default_size(210.0).show(ui, |ui| {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                outliner::show(ui, &mut self.doc, &mut self.tools);
                ui.separator();
                self.tags.show(ui, &mut self.doc);
                ui.separator();
                self.materials.show(ui, &mut self.doc, &mut self.tools);
                ui.separator();
                tags::entity_info(ui, &mut self.doc, &mut self.tools);
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
