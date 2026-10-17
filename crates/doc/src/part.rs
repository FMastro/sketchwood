//! What makes a group or component a woodworking part: which way the grain runs and how the
//! board was sawn, and its size measured the way a woodworker does (length along the grain,
//! then width, then thickness).
//!
//! Stored on the definition, so every copy of a component shares it.

use glam::{DMat3, DVec3};
use serde::{Deserialize, Serialize};

use crate::{Definition, Scene};

/// One of a part's own axes (its group or component's red, green or blue).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PartAxis {
    X,
    Y,
    Z,
}

impl PartAxis {
    pub const ALL: [Self; 3] = [Self::X, Self::Y, Self::Z];

    pub fn index(self) -> usize {
        match self {
            Self::X => 0,
            Self::Y => 1,
            Self::Z => 2,
        }
    }

    pub fn dir(self) -> DVec3 {
        DVec3::AXES[self.index()]
    }
}

/// How a board was sawn from the log: it sets how much it moves across its width.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Cut {
    /// Plain-sawn: the most common, and moves most (tangential shrinkage).
    #[default]
    FlatSawn,
    /// Quartersawn (or rift): about half the movement (radial shrinkage).
    QuarterSawn,
}

impl Cut {
    pub const ALL: [Self; 2] = [Self::FlatSawn, Self::QuarterSawn];

    pub fn name(self) -> &'static str {
        match self {
            Self::FlatSawn => "Flat-sawn",
            Self::QuarterSawn => "Quartersawn",
        }
    }
}

/// A part's woodworking properties.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PartInfo {
    /// The part's own axis the grain runs along; `None` follows its longest side (as
    /// OpenCutList assumes).
    #[serde(default)]
    pub grain: Option<PartAxis>,
    #[serde(default)]
    pub cut: Cut,
}

/// A part's size: `[length, width, thickness]`, length along the grain.
pub type PartSize = [f64; 3];

/// The extents of `definition`'s own geometry along its axes, stretched by `linear` (the
/// enclosing groups' and its own rotation and scale). `None` without faces: an assembly of other
/// parts, or loose lines.
pub fn extents(definition: &Definition, linear: DMat3) -> Option<DVec3> {
    let model = &definition.container.model;
    model.faces().next()?;
    let points: Vec<DVec3> = model.vertices().map(|(_, p)| p).collect();
    let lo = points.iter().copied().reduce(DVec3::min)?;
    let hi = points.iter().copied().reduce(DVec3::max)?;
    // Rotation doesn't change sizes; scale along each axis does. Rounded to a billionth of an
    // inch, so a turned board is still exactly 96" rather than 95.99999999999999".
    let size = |i: usize| (hi - lo)[i] * (linear * DVec3::AXES[i]).length();
    Some(DVec3::from_array([0, 1, 2].map(|i| (size(i) * 1e9).round() / 1e9)))
}

/// `[length, width, thickness]` from a part's extents: length along the grain (the longest side
/// unless set), then the wider of the other two.
pub fn size(extents: DVec3, info: &PartInfo) -> PartSize {
    let grain = info.grain.map_or_else(|| (0..3).max_by(|&a, &b| extents[a].total_cmp(&extents[b])).unwrap_or(0), PartAxis::index);
    let others: Vec<f64> = (0..3).filter(|&i| i != grain).map(|i| extents[i]).collect();
    [extents[grain], others[0].max(others[1]), others[0].min(others[1])]
}

impl Scene {
    /// The size of the part an instance of `definition` is, at the top level of its transform
    /// `linear`. `None` for assemblies.
    pub fn part_size(&self, definition: crate::DefinitionId, linear: DMat3) -> Option<PartSize> {
        let def = self.definition(definition)?;
        Some(size(extents(def, linear)?, &def.part))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn length_follows_the_grain() {
        // A cabinet door 12" tall, 30" wide, 3/4" thick, grain running up it (its Z).
        let extents = DVec3::new(30.0, 0.75, 12.0);
        assert_eq!(size(extents, &PartInfo::default()), [30.0, 12.0, 0.75], "longest side by default");
        let up = PartInfo { grain: Some(PartAxis::Z), ..PartInfo::default() };
        assert_eq!(size(extents, &up), [12.0, 30.0, 0.75], "along the grain when set");
    }
}

#[cfg(test)]
mod in_drawing {
    use glam::DVec3;

    use super::*;
    use crate::{Document, MaterialKind};

    #[test]
    fn a_door_with_grain_running_up_lists_its_length_along_the_grain() {
        let mut doc = Document::default();
        // A 30" wide, 12" tall, 3/4" door standing up (grain should run up its 12").
        let face = doc.edit(|m| m.add_rectangle(DVec3::ZERO, DVec3::X * 30.0, DVec3::Y * 0.75, DVec3::Z)).unwrap();
        let door = doc.push_pull(face, 12.0, true).unwrap().unwrap();
        let oak = doc.add_material(MaterialKind::SolidWood);
        doc.paint(&[crate::Entity::Instance(door)], Some(oak));
        assert_eq!(doc.part_size(door), Some([30.0, 12.0, 0.75]));

        doc.edit_part(door, |p| {
            p.grain = Some(PartAxis::Z);
            p.cut = Cut::QuarterSawn;
        });
        assert_eq!(doc.part_size(door), Some([12.0, 30.0, 0.75]));
        let list = crate::cutlist::cut_list(doc.scene());
        let part = &list.materials[0].parts[0];
        assert_eq!((part.length, part.width, part.thickness), (12.0, 30.0, 0.75), "the cut list follows the grain");
        // Saved with the drawing, and one undo step.
        let reloaded = crate::file::from_str(&crate::file::to_string(doc.scene())).unwrap();
        assert!(reloaded.definitions().any(|(_, d)| d.part.grain == Some(PartAxis::Z) && d.part.cut == Cut::QuarterSawn));
        doc.undo();
        assert_eq!(doc.part_info(door), Some(PartInfo::default()));
    }
}
