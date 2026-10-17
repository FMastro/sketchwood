//! New / Open / Save / Save As, the window title, and "save changes?" before losing work.

use std::path::{Path, PathBuf};

use eframe::egui;
use woodsketch_doc::Document;
use woodsketch_doc::export::{self, ExportUnits};
use woodsketch_doc::file::{self, EXTENSION, OLD_EXTENSION};

/// Something that would discard the drawing, waiting on the "save changes?" answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Discard {
    New,
    Open,
    /// Open the drawing at this place in the recent list.
    OpenRecent(usize),
    Close,
    /// Install the downloaded update and restart.
    RestartToUpdate,
}

#[derive(Default)]
pub struct Files {
    /// Where the drawing was last saved or opened from.
    pub path: Option<PathBuf>,
    /// The document's model version when last saved or opened; anything else is unsaved.
    saved_version: u64,
    /// Waiting for "save changes?" before doing this.
    pending: Option<Discard>,
    /// An error to show (failed to open or save).
    error: Option<String>,
    /// Set once the user has agreed to close, so the next close request goes through.
    closing: bool,
    title: String,
}

impl Files {
    pub fn mark_saved(&mut self, doc: &Document) {
        self.saved_version = doc.model_version();
    }

    /// Count the drawing as unsaved (recovered after a crash).
    pub fn mark_unsaved(&mut self) {
        self.saved_version = u64::MAX;
    }

    pub fn is_dirty(&self, doc: &Document) -> bool {
        doc.model_version() != self.saved_version
    }

    pub fn name(&self) -> String {
        self.path
            .as_deref()
            .and_then(Path::file_stem)
            .map_or_else(|| "Untitled".to_owned(), |s| s.to_string_lossy().into_owned())
    }

    /// Write the drawing to `path` and remember it as the drawing's file.
    pub fn save_to(&mut self, doc: &Document, path: &Path) -> Result<(), String> {
        crate::autosave::write_safely(path, &file::to_string(doc.scene())).map_err(|e| format!("Couldn't save {}: {e}", path.display()))?;
        self.path = Some(path.to_owned());
        self.mark_saved(doc);
        Ok(())
    }

    /// Read a drawing from `path`.
    pub fn read(path: &Path) -> Result<Document, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("Couldn't open {}: {e}", path.display()))?;
        let scene = file::from_str(&text).map_err(|e| e.to_string())?;
        Ok(Document::from_scene(scene))
    }

    pub fn show_error(&mut self, error: String) {
        self.error = Some(error);
    }

    /// Ask before discarding unsaved work; returns whether `action` can happen right away.
    pub fn request(&mut self, doc: &Document, action: Discard) -> bool {
        if self.is_dirty(doc) {
            self.pending = Some(action);
            false
        } else {
            true
        }
    }

    /// The window is closing: let it, or hold it while asking about unsaved changes.
    pub fn on_close_requested(&mut self, ctx: &egui::Context, doc: &Document) {
        if !self.closing && !self.request(doc, Discard::Close) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        }
    }

    /// Keep the window title as "Name* - WoodSketch".
    pub fn update_title(&mut self, ctx: &egui::Context, doc: &Document) {
        let dirty = if self.is_dirty(doc) { "*" } else { "" };
        let title = format!("{}{dirty} - WoodSketch", self.name());
        if title != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }
    }

    /// The "save changes?" and error dialogs. Returns the action to carry out now, if any.
    pub fn dialogs(&mut self, ctx: &egui::Context) -> Option<FileAction> {
        let mut action = None;
        if let Some(error) = self.error.clone() {
            egui::Modal::new(egui::Id::new("file_error")).show(ctx, |ui| {
                ui.set_max_width(360.0);
                ui.label(error);
                if ui.button("OK").clicked() {
                    self.error = None;
                }
            });
        }
        if let Some(pending) = self.pending {
            egui::Modal::new(egui::Id::new("save_changes")).show(ctx, |ui| {
                ui.set_max_width(320.0);
                ui.heading("Save changes?");
                ui.label(format!("Do you want to save the changes to {}?", self.name()));
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Save").clicked() {
                        action = Some(FileAction::SaveThen(pending));
                        self.pending = None;
                    }
                    if ui.button("Don't Save").clicked() {
                        action = Some(FileAction::Discard(pending));
                        self.pending = None;
                    }
                    if ui.button("Cancel").clicked() {
                        self.pending = None;
                    }
                });
            });
        }
        action
    }

    /// Whether a dialog is up (the drawing shouldn't take keyboard input meanwhile).
    pub fn is_asking(&self) -> bool {
        self.pending.is_some() || self.error.is_some()
    }

    /// Let the window close, now that the user has decided about unsaved changes.
    pub fn close(&mut self, ctx: &egui::Context) {
        self.closing = true;
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }
}

/// What to do after the "save changes?" dialog.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileAction {
    SaveThen(Discard),
    Discard(Discard),
}

/// The native dialog for choosing where to save.
pub fn save_dialog(current: Option<&Path>) -> Option<PathBuf> {
    let name = current.and_then(Path::file_name).map_or_else(|| format!("Untitled.{EXTENSION}"), |n| n.to_string_lossy().into_owned());
    let mut dialog = rfd::FileDialog::new().add_filter("WoodSketch drawing", &[EXTENSION]).set_file_name(name);
    if let Some(dir) = current.and_then(Path::parent) {
        dialog = dialog.set_directory(dir);
    }
    dialog.save_file()
}

/// The native dialog for choosing a drawing to open.
pub fn open_dialog(current: Option<&Path>) -> Option<PathBuf> {
    let mut dialog = rfd::FileDialog::new().add_filter("WoodSketch drawing", &[EXTENSION, OLD_EXTENSION]);
    if let Some(dir) = current.and_then(Path::parent) {
        dialog = dialog.set_directory(dir);
    }
    dialog.pick_file()
}

/// A 3D export format (File > Export).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportFormat {
    Obj,
    Stl,
}

impl ExportFormat {
    fn extension(self) -> &'static str {
        match self {
            Self::Obj => "obj",
            Self::Stl => "stl",
        }
    }
}

/// Write the drawing to `path` (an OBJ also gets its `.mtl` alongside).
pub fn export_to(doc: &Document, path: &Path, format: ExportFormat, units: ExportUnits) -> Result<(), String> {
    let fail = |e: std::io::Error| format!("Couldn't export {}: {e}", path.display());
    match format {
        ExportFormat::Stl => std::fs::write(path, export::to_stl(doc.scene(), units)).map_err(fail),
        ExportFormat::Obj => {
            let mtl_path = path.with_extension("mtl");
            let mtl_name = mtl_path.file_name().map_or_else(|| "materials.mtl".into(), |n| n.to_string_lossy().into_owned());
            let (obj, mtl) = export::to_obj(doc.scene(), units, &mtl_name);
            std::fs::write(path, obj).map_err(fail)?;
            std::fs::write(&mtl_path, mtl).map_err(fail)
        }
    }
}

/// The native dialog for choosing where to export, named after the drawing.
pub fn export_dialog(current: Option<&Path>, format: ExportFormat) -> Option<PathBuf> {
    let ext = format.extension();
    let stem = current.and_then(Path::file_stem).map_or_else(|| "Untitled".into(), |s| s.to_string_lossy().into_owned());
    let label = match format {
        ExportFormat::Obj => "OBJ model",
        ExportFormat::Stl => "STL model",
    };
    let mut dialog = rfd::FileDialog::new().add_filter(label, &[ext]).set_file_name(format!("{stem}.{ext}"));
    if let Some(dir) = current.and_then(Path::parent) {
        dialog = dialog.set_directory(dir);
    }
    dialog.save_file()
}

/// How many drawings File > Open Recent lists.
pub const RECENT_COUNT: usize = 10;

/// Put `path` first in the recent list (once), keeping the newest [`RECENT_COUNT`].
pub fn remember(recent: &mut Vec<PathBuf>, path: &Path) {
    let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_owned());
    recent.retain(|p| *p != path);
    recent.insert(0, path);
    recent.truncate(RECENT_COUNT);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recent_drawings_newest_first_once_each() {
        let mut recent = Vec::new();
        for name in ["a.woodsketch", "b.woodsketch", "a.woodsketch"] {
            remember(&mut recent, Path::new(name));
        }
        let names: Vec<_> = recent.iter().map(|p| p.file_name().unwrap().to_str().unwrap()).collect();
        assert_eq!(names, ["a.woodsketch", "b.woodsketch"]);
        for i in 0..20 {
            remember(&mut recent, Path::new(&format!("{i}.woodsketch")));
        }
        assert_eq!(recent.len(), RECENT_COUNT);
    }
}
