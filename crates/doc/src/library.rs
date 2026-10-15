//! Ready-made materials for a woodshop, so a new drawing has something to paint with: common
//! hardwoods and softwoods, sheet goods and construction lumber, with the cut-list fields
//! (type, grain, thickness, stock sizes) filled in with US defaults.

use crate::cutplan::StockOverride;
use crate::{Document, Material, MaterialId, MaterialKind};

/// One library material.
#[derive(Clone, Copy, Debug)]
pub struct Preset {
    pub name: &'static str,
    pub color: [u8; 3],
    pub kind: MaterialKind,
    pub grained: bool,
    /// Stock thickness, inches.
    pub thickness: Option<f64>,
    /// Sheet size (length, width) when it isn't the drawing's (4' x 8').
    pub sheet: Option<(f64, f64)>,
}

impl Preset {
    pub fn material(&self) -> Material {
        Material {
            name: self.name.to_owned(),
            color: self.color,
            kind: self.kind,
            grained: self.grained,
            thickness: self.thickness,
            stock: StockOverride { sheet: self.sheet, ..StockOverride::default() },
        }
    }
}

const fn wood(name: &'static str, color: [u8; 3]) -> Preset {
    Preset { name, color, kind: MaterialKind::SolidWood, grained: true, thickness: Some(0.75), sheet: None }
}

const fn sheet(name: &'static str, color: [u8; 3], grained: bool, thickness: f64) -> Preset {
    Preset { name, color, kind: MaterialKind::SheetGood, grained, thickness: Some(thickness), sheet: None }
}

pub const LIBRARY: &[Preset] = &[
    wood("Red Oak", [196, 146, 104]),
    wood("White Oak", [205, 172, 122]),
    wood("Hard Maple", [236, 214, 170]),
    wood("Cherry", [178, 102, 66]),
    wood("Walnut", [104, 70, 46]),
    wood("Poplar", [206, 200, 150]),
    wood("Pine", [228, 190, 136]),
    sheet("Plywood 3/4\"", [214, 188, 140], true, 0.75),
    sheet("Plywood 1/2\"", [214, 188, 140], true, 0.5),
    sheet("Plywood 1/4\"", [214, 188, 140], true, 0.25),
    // Baltic birch comes in 5' x 5' sheets.
    Preset { sheet: Some((60.0, 60.0)), ..sheet("Baltic Birch 3/4\"", [232, 214, 178], true, 0.75) },
    sheet("MDF 3/4\"", [170, 140, 108], false, 0.75),
    sheet("Hardboard 1/4\"", [120, 90, 64], false, 0.25),
    Preset {
        name: "Construction Lumber (2x)",
        color: [226, 196, 140],
        kind: MaterialKind::DimensionalLumber,
        grained: true,
        thickness: Some(1.5),
        sheet: None,
    },
    Preset {
        name: "Board Lumber (1x)",
        color: [232, 202, 148],
        kind: MaterialKind::DimensionalLumber,
        grained: true,
        thickness: Some(0.75),
        sheet: None,
    },
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
    fn presets_add_once_with_their_cut_list_fields() {
        let mut doc = Document::default();
        let birch = LIBRARY.iter().find(|p| p.name.starts_with("Baltic")).unwrap();
        let id = doc.add_preset(birch);
        assert_eq!(doc.add_preset(birch), id, "adding it again finds the one in the drawing");
        let m = doc.scene().material(id).unwrap();
        assert_eq!((m.kind, m.thickness, m.stock.sheet), (MaterialKind::SheetGood, Some(0.75), Some((60.0, 60.0))));
        assert_eq!(doc.scene().materials().count(), 1);
    }

    #[test]
    fn library_names_are_unique() {
        let names: std::collections::BTreeSet<_> = LIBRARY.iter().map(|p| p.name).collect();
        assert_eq!(names.len(), LIBRARY.len());
    }
}
