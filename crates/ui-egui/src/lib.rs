//! The WoodSketch window: a 3D viewport inside SketchUp-style chrome (`chrome.rs`).

mod chrome;
mod cutlist;
pub mod files;
mod icons;
mod keys;
mod materials;
mod board;
mod outliner;
mod part;
mod tags;
mod viewport;

use std::path::{Path, PathBuf};

use eframe::egui;
use woodsketch_doc::Document;
use woodsketch_tools::Tools;
use viewport::Viewport;

pub struct WoodSketchApp {
    doc: Document,
    tools: Tools,
    keyboard: keys::Keyboard,
    tags: tags::TagsPanel,
    materials: materials::MaterialsPanel,
    viewport: Viewport,
    files: files::Files,
    cut_list: cutlist::CutListWindow,
    show_tray: bool,
    show_about: bool,
    /// Units for File > Export.
    export_units: woodsketch_doc::export::ExportUnits,
}

impl WoodSketchApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let render_state = cc
            .wgpu_render_state
            .as_ref()
            .expect("WoodSketch needs eframe's wgpu renderer");
        icons::install(&cc.egui_ctx);
        cc.egui_ctx.set_visuals(chrome::visuals());
        Self {
            doc: Document::default(),
            tools: Tools::default(),
            keyboard: keys::Keyboard::default(),
            tags: tags::TagsPanel::default(),
            materials: materials::MaterialsPanel::default(),
            viewport: Viewport::new(render_state),
            files: files::Files::default(),
            cut_list: cutlist::CutListWindow::default(),
            show_tray: true,
            show_about: false,
            export_units: woodsketch_doc::export::ExportUnits::Inches,
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
    /// Export the drawing to `path` in the File > Export units.
    pub fn export_to(&self, path: &Path, format: files::ExportFormat) -> Result<(), String> {
        files::export_to(&self.doc, path, format, self.export_units)
    }

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

    /// The drawing, for building test scenes directly.
    pub fn document_mut(&mut self) -> &mut Document {
        &mut self.doc
    }

    pub fn tools(&self) -> &Tools {
        &self.tools
    }

    /// Where a model point (inches) appears in the window. For tests that click on geometry.
    pub fn to_window(&self, point: glam::DVec3) -> Option<egui::Pos2> {
        self.viewport.to_window(point)
    }
}

impl eframe::App for WoodSketchApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.handle_files(&ctx);
        if !self.files.is_asking() {
            self.keyboard.handle(&ctx, &mut self.doc, &mut self.tools, &mut self.viewport);
        }

        // Panels claim space in order: bars across the top and bottom, then the sides.
        self.menu_bar(ui);
        self.toolbar(ui);
        self.status_bar(ui);
        self.tool_set(ui);
        self.tray(ui);

        // "Edit material" in the Cut List opens it in the tray.
        if let Some(material) = self.cut_list.show(&ctx, &mut self.doc) {
            self.materials.editing = Some(material);
            self.show_tray = true;
        }
        self.about_window(&ctx);

        egui::CentralPanel::no_frame()
            .show(ui, |ui| self.viewport.show(ui, frame, &mut self.doc, &mut self.tools));
    }
}
