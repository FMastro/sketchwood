//! The WoodSketch window: a 3D viewport inside SketchUp-style chrome (`chrome.rs`).

mod chrome;
mod cutlist;
pub mod files;
mod icons;
mod keys;
mod materials;
mod autosave;
mod board;
mod joinery;
mod outliner;
mod part;
mod profile;
mod tags;
pub mod updates;
mod viewport;

use std::path::{Path, PathBuf};

use eframe::egui;
use woodsketch_doc::Document;
use woodsketch_tools::Tools;
use viewport::Viewport;

/// Where the recent-drawings list is kept in eframe's settings.
const RECENT_KEY: &str = "recent_drawings";
/// Whether to check for updates at startup, in eframe's settings.
const UPDATES_KEY: &str = "check_updates";

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
    /// Drawings opened or saved lately, newest first (File > Open Recent); kept between runs.
    recent: Vec<PathBuf>,
    /// Writes unsaved changes now and then, for recovery after a crash.
    autosave: autosave::Autosave,
    /// Drawings autosaved by a session that crashed, offered back at start.
    leftovers: Vec<autosave::Leftover>,
    /// Autosave failed once (shown once, not every interval).
    /// The updater the app binary plugs in (none in tests or builds without one).
    updater: Option<updates::Updater>,
    /// Check for updates at startup (a setting, kept between runs).
    check_updates: bool,
    /// Whether the startup check has run, and whether the user asked for the last one.
    update_started: bool,
    update_asked: bool,
    autosave_failed: bool,
}

impl WoodSketchApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let render_state = cc
            .wgpu_render_state
            .as_ref()
            .expect("WoodSketch needs eframe's wgpu renderer");
        icons::install(&cc.egui_ctx);
        cc.egui_ctx.set_visuals(chrome::visuals());
        // Autosave only for the real app (tests have no settings storage).
        let dir = cc.storage.is_some().then(|| eframe::storage_dir("WoodSketch")).flatten().map(|d| d.join("autosave"));
        let autosave = autosave::Autosave::new(dir);
        let leftovers = autosave.leftovers();
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
            recent: cc.storage.and_then(|s| eframe::get_value(s, RECENT_KEY)).unwrap_or_default(),
            autosave,
            leftovers,
            autosave_failed: false,
            updater: None,
            check_updates: cc.storage.and_then(|s| eframe::get_value(s, UPDATES_KEY)).unwrap_or(true),
            update_started: false,
            update_asked: false,
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
        self.autosave.clear();
    }

    /// Start a new, empty drawing (without asking about unsaved changes).
    pub fn new_document(&mut self) {
        self.replace_document(Document::default(), None);
    }

    /// Open the drawing at `path` (without asking about unsaved changes).
    pub fn open_path(&mut self, path: &Path) -> Result<(), String> {
        let doc = files::Files::read(path)?;
        self.replace_document(doc, Some(path.to_owned()));
        files::remember(&mut self.recent, path);
        Ok(())
    }

    /// Save the drawing to `path`, which becomes its file.
    /// Export the drawing to `path` in the File > Export units.
    pub fn export_to(&self, path: &Path, format: files::ExportFormat) -> Result<(), String> {
        files::export_to(&self.doc, path, format, self.export_units)
    }

    pub fn save_to(&mut self, path: &Path) -> Result<(), String> {
        self.files.save_to(&self.doc, path)?;
        self.autosave.clear();
        files::remember(&mut self.recent, path);
        Ok(())
    }

    /// Autosave into `dir` (and offer back what's already there), as the app does with its
    /// settings folder; for tests, which have none.
    pub fn use_autosave_folder(&mut self, dir: PathBuf) {
        self.autosave = autosave::Autosave::new(Some(dir));
        self.leftovers = self.autosave.leftovers();
    }

    /// Write unsaved changes now and then, and offer back drawings a crash left behind.
    fn autosave_and_recover(&mut self, ctx: &egui::Context) {
        let dirty = self.is_dirty();
        match self.autosave.tick(&self.doc, self.files.path.as_deref(), dirty, std::time::Instant::now()) {
            Ok(Some(wait)) => ctx.request_repaint_after(wait),
            Ok(None) => {}
            Err(error) if !self.autosave_failed => {
                self.autosave_failed = true;
                self.files.show_error(error);
            }
            Err(_) => {}
        }
        if self.leftovers.is_empty() {
            return;
        }
        let mut recover = None;
        let mut discard = false;
        egui::Modal::new(egui::Id::new("recover")).show(ctx, |ui| {
            ui.set_max_width(380.0);
            ui.heading("Recover unsaved work?");
            ui.label("WoodSketch didn't close properly last time. It kept a copy of your unsaved changes:");
            for leftover in &self.leftovers {
                let when = match leftover.when.elapsed().map(|d| d.as_secs() / 60) {
                    Ok(0) => ", just now".to_owned(),
                    Ok(1) => ", a minute ago".to_owned(),
                    Ok(m) if m < 120 => format!(", {m} minutes ago"),
                    Ok(m) => format!(", {} hours ago", m / 60),
                    Err(_) => String::new(),
                };
                ui.horizontal(|ui| {
                    ui.label(format!("{}{when}", leftover.name()));
                    if ui.button("Recover").clicked() {
                        recover = Some(leftover.clone());
                    }
                });
            }
            ui.add_space(6.0);
            if ui.button(if self.leftovers.len() > 1 { "Discard all" } else { "Discard" }).clicked() {
                discard = true;
            }
        });
        if let Some(leftover) = recover {
            match files::Files::read(&leftover.file) {
                Ok(doc) => {
                    self.replace_document(doc, leftover.original.clone());
                    // Recovered work isn't saved yet.
                    self.files.mark_unsaved();
                    leftover.discard();
                    self.leftovers.retain(|l| l.file != leftover.file);
                    // Any others wait for the next start.
                    self.leftovers.clear();
                }
                Err(error) => {
                    leftover.discard();
                    self.leftovers.retain(|l| l.file != leftover.file);
                    self.files.show_error(error);
                }
            }
        }
        if discard {
            for leftover in self.leftovers.drain(..) {
                leftover.discard();
            }
        }
    }

    /// The build sheets for the drawing, as a web page (what File > Print build sheets saves).
    pub fn build_sheets_html(&mut self) -> String {
        let title = format!("{} build sheets", self.files.name());
        self.cut_list.build_sheets(&self.doc, &title)
    }

    /// Whether there are changes since the drawing was opened or last saved.
    pub fn is_dirty(&self) -> bool {
        self.files.is_dirty(&self.doc)
    }

    pub fn file_path(&self) -> Option<&Path> {
        self.files.path.as_deref()
    }

    /// Plug in the updater (the app binary does, with Velopack).
    pub fn set_updater(&mut self, updater: updates::Updater) {
        self.updater = Some(updater);
    }

    /// The quiet check at startup (when it's on), and the result of one the user asked for.
    fn keep_up_to_date(&mut self) {
        let Some(updater) = &self.updater else { return };
        if !self.update_started && self.check_updates {
            self.update_started = true;
            (updater.check)();
        }
        if self.update_asked {
            let state = updater.state();
            if !matches!(state, updates::UpdateState::Checking | updates::UpdateState::Idle) {
                self.update_asked = false;
                if !matches!(state, updates::UpdateState::Ready(_)) {
                    self.files.show_error(updates::describe(&state));
                }
            }
        }
    }

    /// Help > Check for Updates: check now, and say what happened.
    pub(crate) fn check_for_updates(&mut self) {
        match &self.updater {
            Some(updater) => {
                self.update_asked = true;
                self.update_started = true;
                (updater.check)();
            }
            None => self.files.show_error(updates::describe(&updates::UpdateState::NotInstalled)),
        }
    }

    /// "Restart to update": after any unsaved changes are dealt with.
    pub(crate) fn restart_to_update(&mut self, ctx: &egui::Context) {
        self.request(ctx, files::Discard::RestartToUpdate);
    }

    /// The update state for the status bar, if there's an updater.
    pub(crate) fn update_state(&self) -> Option<updates::UpdateState> {
        self.updater.as_ref().map(updates::Updater::state)
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
            files::Discard::OpenRecent(i) => {
                if let Some(path) = self.recent.get(i).cloned()
                    && let Err(error) = self.open_path(&path)
                {
                    // Gone or unreadable: drop it from the list.
                    self.recent.retain(|p| *p != path);
                    self.files.show_error(error);
                }
            }
            files::Discard::Close => self.files.close(ctx),
            files::Discard::RestartToUpdate => {
                self.autosave.clear();
                if let Some(updater) = &self.updater {
                    (updater.restart)();
                }
            }
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
    fn on_exit(&mut self) {
        // Closed properly (saved, or the changes declined): nothing to recover.
        self.autosave.clear();
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, RECENT_KEY, &self.recent);
        eframe::set_value(storage, UPDATES_KEY, &self.check_updates);
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.handle_files(&ctx);
        self.autosave_and_recover(&ctx);
        self.keep_up_to_date();
        if !self.files.is_asking() && self.leftovers.is_empty() {
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
