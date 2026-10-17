//! The `.woodsketch` file format: JSON, so it's readable, diffable and easy to recover by hand.
//! Drawings saved before the rename (`.scraft`, format "sketchcraft") still open.
//!
//! ```json
//! { "format": "woodsketch", "version": 1, "units": "inches", "scene": { ... } }
//! ```
//!
//! Ids inside the scene are kept as-is (arena slots), so a saved drawing reloads with the same
//! ids it had. Bump [`VERSION`] when the scene's shape changes, and keep reading old versions.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::Scene;

/// The file extension, without the dot.
pub const EXTENSION: &str = "woodsketch";
/// The extension drawings had before the app was renamed WoodSketch; still opened.
pub const OLD_EXTENSION: &str = "scraft";
const FORMAT: &str = "woodsketch";
/// The format name before the rename; still read.
const OLD_FORMAT: &str = "sketchcraft";
const VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
struct File {
    format: String,
    version: u32,
    /// Always inches for now; recorded so a future metric model unit can be told apart.
    units: String,
    scene: Scene,
}

#[derive(Debug)]
pub enum FileError {
    NotWoodSketch,
    /// Written by a newer WoodSketch.
    TooNew(u32),
    Corrupt(String),
}

impl fmt::Display for FileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotWoodSketch => f.write_str("That isn't a WoodSketch drawing."),
            Self::TooNew(v) => write!(f, "That drawing was saved by a newer WoodSketch (file version {v})."),
            Self::Corrupt(e) => write!(f, "That drawing couldn't be read: {e}"),
        }
    }
}

impl std::error::Error for FileError {}

pub fn to_string(scene: &Scene) -> String {
    let file = File { format: FORMAT.into(), version: VERSION, units: "inches".into(), scene: scene.clone() };
    serde_json::to_string_pretty(&file).expect("scenes always serialize")
}

pub fn from_str(text: &str) -> Result<Scene, FileError> {
    // Check the header first, so other JSON gets a clear message rather than a parse error.
    #[derive(Deserialize)]
    struct Header {
        format: String,
        version: u32,
    }
    let header: Header = serde_json::from_str(text).map_err(|_| FileError::NotWoodSketch)?;
    if header.format != FORMAT && header.format != OLD_FORMAT {
        return Err(FileError::NotWoodSketch);
    }
    if header.version > VERSION {
        return Err(FileError::TooNew(header.version));
    }
    let mut file: File = serde_json::from_str(text).map_err(|e| FileError::Corrupt(e.to_string()))?;
    file.scene.upgrade_materials();
    Ok(file.scene)
}

#[cfg(test)]
mod tests {
    use glam::DVec3;

    use super::*;
    use crate::{DefinitionKind, Document, Entity, MaterialKind};

    /// A drawing using every feature: groups, a component, a nested group, a face with a hole,
    /// tags (one hidden) and materials on groups and faces.
    fn workshop() -> Document {
        let mut doc = Document::default();
        let a = doc.edit(|m| m.add_rectangle(DVec3::ZERO, DVec3::X * 1.5, DVec3::Y * 3.5, DVec3::Z)).unwrap();
        let leg = doc.push_pull(a, 30.0, true).unwrap().unwrap();
        let top = doc.edit(|m| m.add_rectangle(DVec3::new(10.0, 0.0, 0.0), DVec3::X * 24.0, DVec3::Y * 12.0, DVec3::Z)).unwrap();
        doc.edit(|m| m.add_rectangle(DVec3::new(20.0, 4.0, 0.0), DVec3::X * 2.0, DVec3::Y * 2.0, DVec3::Z));
        let top = doc.push_pull(top, 0.75, false).map(|_| top).unwrap();
        let oak = doc.add_material(MaterialKind::SolidWood);
        let ply = doc.add_material(MaterialKind::SheetGood);
        doc.paint(&[Entity::Instance(leg), Entity::Face(top)], Some(oak));
        let legs = doc.add_tag();
        let hidden = doc.add_tag();
        doc.selection_mut().extend([Entity::Instance(leg)]);
        doc.tag_selection(Some(legs));
        let component = doc.group_selection(DefinitionKind::Component).unwrap();
        doc.selection_mut().clear();
        doc.selection_mut().extend([Entity::Instance(component)]);
        doc.group_selection(DefinitionKind::Group);
        doc.set_tag_visible(hidden, false);
        doc.edit_material(ply, |m| m.thickness = Some(0.5));
        doc
    }

    #[test]
    fn a_drawing_survives_a_round_trip() {
        let doc = workshop();
        let text = to_string(doc.scene());
        let loaded = from_str(&text).unwrap();
        // Same content: saving the loaded drawing gives the same file.
        assert_eq!(to_string(&loaded), text);
        let reopened = Document::from_scene(loaded);
        assert_eq!(reopened.bounds(), doc.bounds());
        assert_eq!(reopened.view().model().faces().count(), doc.view().model().faces().count());
        assert_eq!(reopened.scene().materials().count(), 2);
        assert_eq!(reopened.scene().tags().filter(|(_, t)| !t.visible).count(), 1);
    }

    #[test]
    fn a_reopened_drawing_can_be_edited_and_undone() {
        let doc = workshop();
        let mut reopened = Document::from_scene(from_str(&to_string(doc.scene())).unwrap());
        assert!(!reopened.undo(), "a freshly opened drawing has no undo history");
        let face = reopened.edit(|m| m.add_rectangle(DVec3::new(0.0, 40.0, 0.0), DVec3::X, DVec3::Y, DVec3::Z)).unwrap();
        assert!(reopened.model().face(face).is_some());
        assert!(reopened.undo());
        assert!(reopened.model().face(face).is_none());
    }

    #[test]
    fn coordinates_reload_bit_for_bit() {
        let mut doc = Document::default();
        let corner = DVec3::new(-1.9075752155031722, -11.711819318919822, 0.1 + 0.2);
        doc.edit(|m| m.add_rectangle(corner, DVec3::X * 1.5, DVec3::Y * 3.5, DVec3::Z));
        let loaded = Document::from_scene(from_str(&to_string(doc.scene())).unwrap());
        assert_eq!(loaded.bounds(), doc.bounds());
    }

    #[test]
    fn the_file_is_readable_json_with_a_header() {
        let text = to_string(Document::default().scene());
        assert!(text.contains("\"format\": \"woodsketch\""));
        assert!(text.contains("\"version\": 1"));
        assert!(text.contains("\"units\": \"inches\""));
    }

    #[test]
    fn other_files_are_rejected_clearly() {
        assert!(matches!(from_str("not json"), Err(FileError::NotWoodSketch)));
        assert!(matches!(from_str(r#"{"format":"other","version":1}"#), Err(FileError::NotWoodSketch)));
        assert!(matches!(
            from_str(r#"{"format":"woodsketch","version":99,"units":"inches","scene":{}}"#),
            Err(FileError::TooNew(99))
        ));
        assert!(matches!(
            from_str(r#"{"format":"woodsketch","version":1,"units":"inches","scene":42}"#),
            Err(FileError::Corrupt(_))
        ));
    }
}
