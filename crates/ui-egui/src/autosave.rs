//! Autosave and crash recovery. While a drawing has unsaved changes, a copy is written every
//! [`INTERVAL`] to WoodSketch's settings folder (not next to the drawing, so synced folders
//! don't fill with temporary files). Saving, opening, starting a new drawing or closing normally
//! removes it; one still there when WoodSketch starts means it didn't close properly, and is
//! offered back.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use woodsketch_doc::Document;
use woodsketch_doc::file;

/// How often unsaved changes are written.
pub const INTERVAL: Duration = Duration::from_secs(120);

/// A drawing autosaved by a session that didn't close properly.
#[derive(Clone, Debug, PartialEq)]
pub struct Leftover {
    /// The autosaved copy.
    pub file: PathBuf,
    /// The drawing's own file, if it had been saved before.
    pub original: Option<PathBuf>,
    pub when: SystemTime,
}

impl Leftover {
    /// What the drawing was called: its file's name, or "Untitled".
    pub fn name(&self) -> String {
        self.original
            .as_deref()
            .and_then(Path::file_stem)
            .map_or_else(|| "Untitled".to_owned(), |s| s.to_string_lossy().into_owned())
    }

    /// Delete the autosaved copy (recovered or not wanted).
    pub fn discard(&self) {
        let _ = std::fs::remove_file(&self.file);
        let _ = std::fs::remove_file(self.file.with_extension("path"));
    }
}

pub struct Autosave {
    /// Where autosaves go; `None` turns autosave off (tests).
    dir: Option<PathBuf>,
    /// The model version last written, so an unchanged drawing isn't written again.
    written_version: Option<u64>,
    last: Instant,
}

impl Autosave {
    pub fn new(dir: Option<PathBuf>) -> Self {
        Self { dir, written_version: None, last: Instant::now() }
    }

    /// This session's autosave file.
    fn file(&self) -> Option<PathBuf> {
        Some(self.dir.as_ref()?.join(format!("autosave-{}.woodsketch", std::process::id())))
    }

    /// Write the drawing if it has unsaved changes not yet autosaved and [`INTERVAL`] has passed.
    /// Returns how long until it should be checked again (to schedule a repaint), if ever.
    pub fn tick(&mut self, doc: &Document, original: Option<&Path>, dirty: bool, now: Instant) -> Result<Option<Duration>, String> {
        if self.dir.is_none() || !dirty || self.written_version == Some(doc.model_version()) {
            return Ok(None);
        }
        let due = self.last + INTERVAL;
        if now < due {
            return Ok(Some(due - now));
        }
        self.write(doc, original)?;
        self.last = now;
        Ok(None)
    }

    /// Write the drawing now.
    pub fn write(&mut self, doc: &Document, original: Option<&Path>) -> Result<(), String> {
        let Some(path) = self.file() else { return Ok(()) };
        let dir = self.dir.as_ref().expect("a file needs a folder");
        std::fs::create_dir_all(dir).map_err(|e| format!("Couldn't autosave: {e}"))?;
        write_safely(&path, &file::to_string(doc.scene())).map_err(|e| format!("Couldn't autosave: {e}"))?;
        let note = original.map_or_else(String::new, |p| p.to_string_lossy().into_owned());
        write_safely(&path.with_extension("path"), &note).map_err(|e| format!("Couldn't autosave: {e}"))?;
        self.written_version = Some(doc.model_version());
        Ok(())
    }

    /// The drawing was saved, replaced or closed: its autosave isn't needed.
    pub fn clear(&mut self) {
        if let Some(path) = self.file() {
            let _ = std::fs::remove_file(&path);
            let _ = std::fs::remove_file(path.with_extension("path"));
        }
        self.written_version = None;
        self.last = Instant::now();
    }

    /// Autosaves left by sessions that didn't close properly, newest first.
    pub fn leftovers(&self) -> Vec<Leftover> {
        let Some(dir) = &self.dir else { return Vec::new() };
        let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
        let mine = self.file();
        let mut found: Vec<Leftover> = entries
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "woodsketch") && Some(p) != mine.as_ref())
            .map(|file| {
                let original = std::fs::read_to_string(file.with_extension("path"))
                    .ok()
                    .filter(|s| !s.trim().is_empty())
                    .map(|s| PathBuf::from(s.trim()));
                let when = std::fs::metadata(&file).and_then(|m| m.modified()).unwrap_or(SystemTime::UNIX_EPOCH);
                Leftover { file, original, when }
            })
            .collect();
        found.sort_by_key(|l| std::cmp::Reverse(l.when));
        found
    }
}

/// Write `text` to `path` without ever leaving it half-written: write a temporary file next to
/// it, then swap it in.
pub fn write_safely(path: &Path, text: &str) -> std::io::Result<()> {
    let mut temp = path.as_os_str().to_owned();
    temp.push(".tmp");
    let temp = PathBuf::from(temp);
    std::fs::write(&temp, text)?;
    std::fs::rename(&temp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&temp);
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("woodsketch-autosave-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn drawing() -> Document {
        let mut doc = Document::default();
        doc.edit(|m| m.add_rectangle(glam::DVec3::ZERO, glam::DVec3::X, glam::DVec3::Y, glam::DVec3::Z));
        doc
    }

    #[test]
    fn unsaved_changes_are_written_after_the_interval_and_cleared_on_save() {
        let dir = folder("tick");
        let mut autosave = Autosave::new(Some(dir.clone()));
        let doc = drawing();
        let start = Instant::now();
        let original = Path::new("C:/Shop/Nightstand.woodsketch");
        assert!(autosave.tick(&doc, Some(original), true, start).unwrap().is_some(), "not yet");
        assert!(autosave.file().is_some_and(|f| !f.exists()));
        autosave.tick(&doc, Some(original), true, start + INTERVAL).unwrap();
        let file = autosave.file().unwrap();
        assert!(file.exists(), "written after the interval");
        assert_eq!(autosave.tick(&doc, Some(original), true, start + INTERVAL * 3).unwrap(), None, "unchanged: not again");

        // Another session (a new start after a crash) finds it, with the drawing's own file.
        let next = Autosave { dir: Some(dir.clone()), written_version: None, last: Instant::now() };
        let mut leftovers = next.leftovers();
        assert!(leftovers.is_empty(), "this session's own file isn't a leftover");
        std::fs::rename(&file, dir.join("autosave-1.woodsketch")).unwrap();
        std::fs::rename(file.with_extension("path"), dir.join("autosave-1.path")).unwrap();
        leftovers = next.leftovers();
        assert_eq!(leftovers.len(), 1);
        assert_eq!(leftovers[0].original.as_deref(), Some(original));
        assert_eq!(leftovers[0].name(), "Nightstand");
        let recovered = crate::files::Files::read(&leftovers[0].file).unwrap();
        assert_eq!(recovered.model().faces().count(), 1);
        leftovers[0].discard();
        assert!(next.leftovers().is_empty());

        autosave.write(&doc, None).unwrap();
        autosave.clear();
        assert!(!autosave.file().unwrap().exists(), "saving clears it");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn off_without_a_folder() {
        let mut autosave = Autosave::new(None);
        assert_eq!(autosave.tick(&drawing(), None, true, Instant::now() + INTERVAL * 2).unwrap(), None);
        assert!(autosave.leftovers().is_empty());
    }

    #[test]
    fn safe_writes_replace_the_file_whole() {
        let dir = folder("safe");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("a.woodsketch");
        write_safely(&path, "first").unwrap();
        write_safely(&path, "second").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "second");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1, "no temporary file left");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
