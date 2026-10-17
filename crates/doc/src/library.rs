//! Ready-made materials for a woodshop, so a new drawing has something to paint with: common
//! hardwoods and softwoods, sheet goods and construction lumber. Each is a kind of stock, not
//! one size: Plywood comes in several thicknesses, Pine Lumber in every standard section, and
//! the cut list picks what each part needs.

use crate::cutplan::Standard;
use crate::{Document, Material, MaterialId, MaterialKind};

/// One library material.
#[derive(Clone, Copy, Debug)]
pub struct Preset {
    pub name: &'static str,
    pub color: [u8; 3],
    pub kind: MaterialKind,
    pub grained: bool,
    /// Thicknesses it comes in, when they differ from its type's defaults.
    pub thicknesses: &'static [f64],
    /// Sheet sizes, when they differ from 4' x 8'.
    pub sheets: &'static [(f64, f64)],
}

impl Preset {
    pub fn material(&self) -> Material {
        let mut material = Material::new(self.name, self.color, self.kind, self.grained);
        if !self.thicknesses.is_empty() {
            material.standard.thicknesses = self.thicknesses.to_vec();
        }
        if !self.sheets.is_empty() {
            material.standard.sheets = self.sheets.to_vec();
        }
        if matches!(self.kind, MaterialKind::SolidWood | MaterialKind::DimensionalLumber) {
            material.movement = crate::movement::species(self.name);
        }
        material
    }

    /// The standard stock it comes in.
    pub fn standard(&self) -> Standard {
        self.material().standard
    }
}

const fn of(name: &'static str, color: [u8; 3], kind: MaterialKind, grained: bool) -> Preset {
    Preset { name, color, kind, grained, thicknesses: &[], sheets: &[] }
}

const WOOD: MaterialKind = MaterialKind::SolidWood;
const SHEET: MaterialKind = MaterialKind::SheetGood;
const LUMBER: MaterialKind = MaterialKind::DimensionalLumber;

pub const LIBRARY: &[Preset] = &[
    of("Red Oak", [196, 146, 104], WOOD, true),
    of("White Oak", [205, 172, 122], WOOD, true),
    of("Hard Maple", [236, 214, 170], WOOD, true),
    of("Cherry", [178, 102, 66], WOOD, true),
    of("Walnut", [104, 70, 46], WOOD, true),
    of("Poplar", [206, 200, 150], WOOD, true),
    of("Pine", [228, 190, 136], WOOD, true),
    of("Plywood", [214, 188, 140], SHEET, true),
    of("Pine Plywood", [222, 194, 140], SHEET, true),
    // Baltic birch comes in 5' x 5' sheets, and thinner.
    Preset { thicknesses: &[0.125, 0.25, 0.5, 0.75], sheets: &[(60.0, 60.0)], ..of("Baltic Birch Plywood", [232, 214, 178], SHEET, true) },
    of("MDF", [170, 140, 108], SHEET, false),
    Preset { thicknesses: &[0.125, 0.25], ..of("Hardboard", [120, 90, 64], SHEET, false) },
    of("Pine Lumber", [226, 196, 140], LUMBER, true),
    of("Douglas Fir Lumber", [214, 170, 120], LUMBER, true),
    of("Pressure-Treated Lumber", [190, 180, 120], LUMBER, true),
];

impl Document {
    /// Add a library material to the drawing (or find the one already added under its name).
    pub fn add_preset(&mut self, preset: &Preset) -> MaterialId {
        if let Some((id, _)) = self.scene().materials().find(|(_, m)| m.name == preset.name) {
            return id;
        }
        let material = preset.material();
        self.edit_scene(|scene, _| scene.add_material(material))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_add_once_with_their_standard_stock() {
        let mut doc = Document::default();
        let birch = LIBRARY.iter().find(|p| p.name.starts_with("Baltic")).unwrap();
        let id = doc.add_preset(birch);
        assert_eq!(doc.add_preset(birch), id, "adding it again finds the one in the drawing");
        let m = doc.scene().material(id).unwrap();
        assert_eq!(m.kind, MaterialKind::SheetGood);
        assert_eq!(m.standard.sheets, vec![(60.0, 60.0)]);
        assert!(m.standard.thicknesses.contains(&0.5) && m.standard.thicknesses.contains(&0.75));
        assert_eq!(doc.scene().materials().count(), 1);
        let pine = LIBRARY.iter().find(|p| p.name == "Pine Lumber").unwrap().standard();
        assert!(pine.sections.contains(&(1.5, 3.5)) && pine.sections.contains(&(1.5, 9.25)), "every standard section");
    }

    #[test]
    fn library_names_are_unique() {
        let names: std::collections::BTreeSet<_> = LIBRARY.iter().map(|p| p.name).collect();
        assert_eq!(names.len(), LIBRARY.len());
    }
}
