//! Boards drawn from stock: a material's standard sizes (a 2x4, a 3/4" sheet) as something to
//! draw with, and the board itself, a group made of one box whose own axes are its length, width
//! and thickness (so its grain runs along its length).

use glam::{DAffine3, DMat3, DVec3};

use crate::cutplan::{lumber_sizes, section_name};
use crate::units::{format_length, parse_length};
use crate::{Container, Document, InstanceId, Material, MaterialId, MaterialKind, Model};

/// What a board is cut from.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Stock {
    /// Dimensional lumber: a fixed section (actual thickness and width); only the length is drawn.
    Section { thickness: f64, width: f64 },
    /// Sheet goods and solid wood: a thickness; the length and width are drawn.
    Thickness(f64),
}

impl Stock {
    pub fn thickness(self) -> f64 {
        match self {
            Self::Section { thickness, .. } | Self::Thickness(thickness) => thickness,
        }
    }
}

/// The stock a material can be drawn from, with a name for each: `2x4`, `3/4" sheet`,
/// `3/4" board`. Hardware and other materials have none.
pub fn stock_options(material: &Material) -> Vec<(String, Stock)> {
    let standard = &material.standard;
    match material.kind {
        MaterialKind::DimensionalLumber => standard
            .sections
            .iter()
            .map(|&(thickness, width)| (section_name(thickness, width), Stock::Section { thickness, width }))
            .collect(),
        MaterialKind::SheetGood => {
            standard.thicknesses.iter().map(|&t| (format!("{} sheet", format_length(t)), Stock::Thickness(t))).collect()
        }
        MaterialKind::SolidWood => {
            standard.thicknesses.iter().map(|&t| (format!("{} board", format_length(t)), Stock::Thickness(t))).collect()
        }
        MaterialKind::Hardware | MaterialKind::Other => Vec::new(),
    }
}

/// A board typed into the Measurements box: `2x4x96` or `2x4x8'` (a lumber section and a
/// length), or `3/4x5.5x30` (thickness x width x length).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TypedBoard {
    pub stock: Stock,
    pub width: f64,
    pub length: f64,
}

pub fn parse_board(text: &str) -> Option<TypedBoard> {
    let lower = text.trim().to_ascii_lowercase();
    let parts: Vec<&str> = lower.split(['x', '×']).map(str::trim).collect();
    let [a, b, c] = parts[..] else { return None };
    let length = parse_length(c).ok().filter(|l| *l > 0.0)?;
    // A nominal lumber section first: "2x4" means 1 1/2" x 3 1/2".
    if let Some((_, thickness, width)) = lumber_sizes().into_iter().find(|(name, _, _)| *name == format!("{a}x{b}")) {
        return Some(TypedBoard { stock: Stock::Section { thickness, width }, width, length });
    }
    let thickness = parse_length(a).ok().filter(|t| *t > 0.0)?;
    let width = parse_length(b).ok().filter(|w| *w > 0.0)?;
    Some(TypedBoard { stock: Stock::Thickness(thickness), width, length })
}

/// A board's place: it starts at `origin` and runs `length` along `length_dir`, `width` along
/// `width_dir` and its thickness along `length_dir × width_dir`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Board {
    pub origin: DVec3,
    pub length_dir: DVec3,
    pub width_dir: DVec3,
    pub length: f64,
    pub width: f64,
    pub thickness: f64,
}

impl Board {
    /// The board's own axes (length, width, thickness) placed in its container.
    pub fn transform(&self) -> DAffine3 {
        let u = self.length_dir.normalize();
        let v = self.width_dir.normalize();
        DAffine3 { matrix3: DMat3::from_cols(u, v, u.cross(v)), translation: self.origin }
    }

    /// The eight corners, for previews.
    pub fn corners(&self) -> [DVec3; 8] {
        let t = self.transform();
        std::array::from_fn(|i| {
            let local = DVec3::new(
                if i & 1 == 0 { 0.0 } else { self.length },
                if i & 2 == 0 { 0.0 } else { self.width },
                if i & 4 == 0 { 0.0 } else { self.thickness },
            );
            t.transform_point3(local)
        })
    }

    /// The twelve edges, for previews.
    pub fn edges(&self) -> Vec<[DVec3; 2]> {
        let c = self.corners();
        let mut edges = Vec::with_capacity(12);
        for i in 0..8 {
            for bit in [1, 2, 4] {
                if i & bit == 0 {
                    edges.push([c[i], c[i | bit]]);
                }
            }
        }
        edges
    }
}

/// A box `length` x `width` x `thickness` from the origin along X, Y and Z.
fn board_model(length: f64, width: f64, thickness: f64) -> Model {
    let mut model = Model::default();
    let bottom = model
        .add_rectangle(DVec3::ZERO, DVec3::X * length, DVec3::Y * width, DVec3::Z)
        .expect("a board has a size");
    model.push_pull(bottom, thickness).expect("a board has a thickness");
    model
}

impl Document {
    /// Add a board as a new group in the container being edited, painted with `material`, as
    /// one undo step. Its grain runs along its length.
    pub fn add_board(&mut self, board: &Board, material: Option<MaterialId>) -> Option<InstanceId> {
        if board.length <= crate::EPS || board.width <= crate::EPS || board.thickness <= crate::EPS {
            return None;
        }
        let mut container = Container::default();
        container.model = board_model(board.length, board.width, board.thickness);
        let transform = board.transform();
        self.edit_scene(|scene, path| scene.place_group(path, container, transform, material))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::LIBRARY;

    #[test]
    fn typed_boards() {
        let stud = parse_board("2x4x96").unwrap();
        assert_eq!(stud.stock, Stock::Section { thickness: 1.5, width: 3.5 });
        assert_eq!((stud.width, stud.length), (3.5, 96.0));
        assert_eq!(parse_board("2x4x8'").unwrap().length, 96.0);
        let shelf = parse_board("3/4 x 11 1/4 x 30").unwrap();
        assert_eq!(shelf.stock, Stock::Thickness(0.75));
        assert_eq!((shelf.width, shelf.length), (11.25, 30.0));
        assert_eq!(parse_board("2x4"), None);
        assert_eq!(parse_board("96"), None);
        assert_eq!(parse_board("2x4x0"), None);
    }

    #[test]
    fn materials_offer_their_standard_stock() {
        let pine = LIBRARY.iter().find(|p| p.name == "Pine Lumber").unwrap().material();
        let options = stock_options(&pine);
        assert!(options.iter().any(|(name, s)| name == "2x4" && *s == Stock::Section { thickness: 1.5, width: 3.5 }));
        let birch = LIBRARY.iter().find(|p| p.name.starts_with("Baltic")).unwrap().material();
        assert!(stock_options(&birch).iter().any(|(name, s)| name == "3/4\" sheet" && *s == Stock::Thickness(0.75)));
    }

    #[test]
    fn a_board_is_a_painted_group_along_its_own_axes() {
        let mut doc = Document::default();
        let pine = doc.add_preset(LIBRARY.iter().find(|p| p.name == "Pine Lumber").unwrap());
        // A stud standing up: length up the blue axis, width along red, thickness along green.
        let board =
            Board { origin: DVec3::new(10.0, 0.0, 0.0), length_dir: DVec3::Z, width_dir: DVec3::X, length: 84.0, width: 3.5, thickness: 1.5 };
        let stud = doc.add_board(&board, Some(pine)).unwrap();
        assert_eq!(doc.part_size(stud), Some([84.0, 3.5, 1.5]));
        let (lo, hi) = doc.bounds().unwrap();
        assert!(lo.abs_diff_eq(DVec3::new(10.0, 0.0, 0.0), 1e-9), "{lo}");
        assert!(hi.abs_diff_eq(DVec3::new(13.5, 1.5, 84.0), 1e-9), "{hi}");
        let list = crate::cutlist::cut_list(doc.scene());
        assert_eq!(list.materials[0].parts[0].length, 84.0);
        doc.undo();
        assert!(doc.bounds().is_none(), "one undo step");
    }
}
