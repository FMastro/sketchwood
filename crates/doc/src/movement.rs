//! Wood movement: how much solid wood swells and shrinks across its grain as the seasons change
//! its moisture content, and where that will fight the parts it's joined to.
//!
//! Movement across a board = width x the species' dimensional change coefficient (per 1%
//! moisture change, tangential for flat-sawn, radial for quartersawn) x the moisture swing. The
//! coefficients are from the USDA Forest Products Laboratory's Wood Handbook (public domain).
//! Along the grain, wood barely moves; plywood, MDF and other sheet goods are stable.
//!
//! A part that moves across its grain where it meets a part that won't move that way (its grain
//! runs across, or it's a sheet good) can't be glued or screwed solid along that stretch: it
//! needs room to move (slotted holes, tabletop buttons, a floating panel, glue at the middle).

use glam::{DAffine3, DVec3};
use serde::{Deserialize, Serialize};

use crate::units::format_length;
use crate::{Container, MaterialId, MaterialKind, Scene};

/// A typical indoor year: about 6% moisture content in a dry winter, 10% in a humid summer.
pub const DEFAULT_SWING: f64 = 4.0;

/// A species' dimensional change coefficients: the fraction of its width a board gains or
/// loses per 1% change in moisture content.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Movement {
    /// Across a flat-sawn board (along the growth rings).
    pub tangential: f64,
    /// Across a quartersawn board (across the rings).
    pub radial: f64,
}

/// Wood Handbook coefficients for the library's species (and common names for them).
pub fn species(name: &str) -> Option<Movement> {
    let name = name.to_ascii_lowercase();
    let table: &[(&[&str], f64, f64)] = &[
        (&["red oak"], 0.00369, 0.00158),
        (&["white oak"], 0.00365, 0.00180),
        (&["hard maple", "sugar maple"], 0.00353, 0.00165),
        (&["soft maple", "red maple"], 0.00289, 0.00137),
        (&["cherry"], 0.00248, 0.00126),
        (&["walnut"], 0.00274, 0.00190),
        (&["poplar", "tulip"], 0.00289, 0.00158),
        (&["ash"], 0.00274, 0.00169),
        (&["birch"], 0.00338, 0.00256),
        (&["hickory"], 0.00411, 0.00259),
        (&["mahogany"], 0.00172, 0.00125),
        (&["cedar"], 0.00234, 0.00111),
        (&["douglas fir", "fir"], 0.00267, 0.00165),
        (&["pressure-treated", "southern pine", "southern yellow pine", "yellow pine"], 0.00259, 0.00165),
        (&["pine"], 0.00212, 0.00071),
    ];
    table.iter().find(|(names, _, _)| names.iter().any(|n| name.contains(n))).map(|&(_, tangential, radial)| Movement { tangential, radial })
}

/// How much one part moves across its width.
#[derive(Clone, Debug, PartialEq)]
pub struct PartMovement {
    pub name: String,
    pub material: String,
    /// Across the grain.
    pub width: f64,
    /// Total seasonal change in width, inches.
    pub movement: f64,
    /// Assumed flat-sawn because the cut is "Not sure".
    pub assumed_flat: bool,
}

/// Two parts meeting where one moves and the other won't.
#[derive(Clone, Debug, PartialEq)]
pub struct Conflict {
    /// The part that moves.
    pub moving: String,
    /// The part that holds it.
    pub fixed: String,
    /// How long the stretch where they meet is, across the moving part's grain.
    pub span: f64,
    /// How much the moving part moves over that stretch, inches.
    pub movement: f64,
    pub why: &'static str,
}

impl Conflict {
    pub fn describe(&self) -> String {
        format!(
            "{} moves about {} across its grain over the {} where it meets {}, which {}. Don't glue or screw that whole stretch: glue only the middle, or use slotted holes, buttons or a floating fit.",
            self.moving,
            format_length(self.movement),
            format_length(self.span),
            self.fixed,
            self.why
        )
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Report {
    pub parts: Vec<PartMovement>,
    pub conflicts: Vec<Conflict>,
}

/// Less than this over a joint isn't worth a warning (1/16"): ordinary apron tenons glued into
/// legs move a few hundredths, which glue and the tenon's give take without trouble.
const NEGLIGIBLE: f64 = 1.0 / 16.0;
const TOUCH: f64 = 1e-3;

/// A part placed in the drawing, square to the axes.
struct Placed {
    name: String,
    lo: DVec3,
    hi: DVec3,
    /// The world axis its grain runs along, and the one its width runs across.
    grain: usize,
    across: usize,
    material: Option<MaterialId>,
    cut: crate::part::Cut,
}

fn collect(scene: &Scene, container: &Container, inherited: Option<MaterialId>, transform: DAffine3, found: &mut Vec<Placed>) {
    for (_, instance) in container.instances() {
        if !scene.is_visible(instance) {
            continue;
        }
        let Some(def) = scene.definition(instance.definition) else { continue };
        let material = instance.material.or(inherited);
        let transform = transform * instance.transform;
        if let Some(extents) = crate::part::extents(def, transform.matrix3)
            && let Some((lo, hi)) = def.container.model.bounds()
        {
            let [l, w, _] = crate::part::axes(extents, &def.part);
            let world_axis = |local: usize| {
                let d = transform.matrix3 * DVec3::AXES[local];
                let a = d.abs();
                (a.max_element() > 0.999 * d.length()).then(|| (0..3).max_by(|&i, &j| a[i].total_cmp(&a[j])).unwrap_or(0))
            };
            // Only parts square to the drawing's axes are checked against each other.
            if let (Some(grain), Some(across)) = (world_axis(l), world_axis(w)) {
                let corners = (0..8).map(|i| {
                    transform.transform_point3(DVec3::new(
                        if i & 1 == 0 { lo.x } else { hi.x },
                        if i & 2 == 0 { lo.y } else { hi.y },
                        if i & 4 == 0 { lo.z } else { hi.z },
                    ))
                });
                let (mut a, mut b) = (DVec3::splat(f64::INFINITY), DVec3::splat(f64::NEG_INFINITY));
                for c in corners {
                    a = a.min(c);
                    b = b.max(c);
                }
                let material = material.or_else(|| def.container.model.faces().find_map(|(_, f)| f.material));
                found.push(Placed { name: def.name.clone(), lo: a, hi: b, grain, across, material, cut: def.part.cut });
            }
        }
        collect(scene, &def.container, material, transform, found);
    }
}

/// The movement coefficient across a part's width, if it's solid wood that moves.
fn coefficient(scene: &Scene, material: Option<MaterialId>, cut: crate::part::Cut) -> Option<f64> {
    let m = scene.material(material?)?;
    if !matches!(m.kind, MaterialKind::SolidWood | MaterialKind::DimensionalLumber) {
        return None;
    }
    let movement = m.movement?;
    Some(match cut.assumed() {
        crate::part::Cut::QuarterSawn => movement.radial,
        _ => movement.tangential,
    })
}

/// Check the drawing for a moisture `swing` (percent): each solid-wood part's movement across
/// its width, and where parts meeting will fight it.
pub fn check(scene: &Scene, swing: f64) -> Report {
    let mut parts = Vec::new();
    collect(scene, &scene.root, None, DAffine3::IDENTITY, &mut parts);
    let mut report = Report::default();
    for p in &parts {
        let Some(c) = coefficient(scene, p.material, p.cut) else { continue };
        let width = p.hi[p.across] - p.lo[p.across];
        let material = p.material.and_then(|m| scene.material(m)).map_or_else(String::new, |m| m.name.clone());
        report.parts.push(PartMovement {
            name: p.name.clone(),
            material,
            width,
            movement: width * c * swing,
            assumed_flat: p.cut == crate::part::Cut::Unknown,
        });
    }
    report.parts.sort_by(|a, b| b.movement.total_cmp(&a.movement));

    for (i, a) in parts.iter().enumerate() {
        let Some(c) = coefficient(scene, a.material, a.cut) else { continue };
        for (j, b) in parts.iter().enumerate() {
            if i == j {
                continue;
            }
            // Touching or overlapping (a joint).
            let lo = a.lo.max(b.lo);
            let hi = a.hi.min(b.hi);
            if (hi - lo).min_element() < -TOUCH {
                continue;
            }
            let d = a.across;
            // Where they meet, stretched across `a`'s grain; both must share some of it.
            let span = hi[d] - lo[d];
            let others: Vec<usize> = (0..3).filter(|&k| k != d).collect();
            let meet = others.iter().filter(|&&k| hi[k] - lo[k] > TOUCH).count();
            if span <= TOUCH || meet == 0 {
                continue;
            }
            // Does `b` move with `a` across that direction? Only solid wood with its width that way.
            let b_moves = coefficient(scene, b.material, b.cut).is_some() && b.across == d;
            if b_moves {
                continue;
            }
            let why = if coefficient(scene, b.material, b.cut).is_some() || b.material.and_then(|m| scene.material(m)).is_some_and(|m| m.kind == MaterialKind::DimensionalLumber || m.kind == MaterialKind::SolidWood) {
                if b.grain == d { "runs the other way and won't move with it" } else { "won't move with it" }
            } else {
                "is a stable sheet or other material that won't move with it"
            };
            let movement = span * c * swing;
            if movement >= NEGLIGIBLE {
                report.conflicts.push(Conflict { moving: a.name.clone(), fixed: b.name.clone(), span, movement, why });
            }
        }
    }
    report.conflicts.sort_by(|a, b| b.movement.total_cmp(&a.movement));
    report
}

impl Scene {
    /// The seasonal moisture swing the drawing is checked for, percent.
    pub fn moisture_swing(&self) -> f64 {
        self.moisture_swing.unwrap_or(DEFAULT_SWING)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::Board;
    use crate::library::LIBRARY;
    use crate::{Document, InstanceId};

    fn board(doc: &mut Document, wood: &str, name: &str, origin: DVec3, length_dir: DVec3, width_dir: DVec3, [l, w, t]: [f64; 3]) -> InstanceId {
        let m = doc.add_preset(LIBRARY.iter().find(|p| p.name == wood).unwrap());
        let id = doc.add_board(&Board { origin, length_dir, width_dir, length: l, width: w, thickness: t }, Some(m)).unwrap();
        doc.rename_instance(id, name);
        id
    }

    #[test]
    fn a_cherry_top_moves_about_five_thirty_seconds() {
        let mut doc = Document::default();
        board(&mut doc, "Cherry", "Top", DVec3::ZERO, DVec3::X, DVec3::Y, [30.0, 16.0, 0.75]);
        let report = check(doc.scene(), doc.scene().moisture_swing());
        let top = &report.parts[0];
        // 16" x 0.00248 x 4% = 0.159".
        assert!((top.movement - 0.15872).abs() < 1e-6, "{}", top.movement);
        assert!(top.assumed_flat);
        assert!(report.conflicts.is_empty(), "nothing holds it");
    }

    #[test]
    fn quartersawn_moves_about_half() {
        let mut doc = Document::default();
        let top = board(&mut doc, "White Oak", "Top", DVec3::ZERO, DVec3::X, DVec3::Y, [30.0, 16.0, 0.75]);
        let flat = check(doc.scene(), 4.0).parts[0].movement;
        doc.edit_part(top, |p| p.cut = crate::part::Cut::QuarterSawn);
        let quarter = check(doc.scene(), 4.0).parts[0].movement;
        assert!(quarter < flat * 0.55, "{quarter} vs {flat}");
    }

    #[test]
    fn a_top_on_a_side_apron_and_a_breadboard_end_warn_but_a_shelf_in_a_side_doesnt() {
        let mut doc = Document::default();
        // A shelf in a side: both are 11 1/4" deep along Y and move together.
        board(&mut doc, "Red Oak", "Side", DVec3::ZERO, DVec3::Z, DVec3::Y, [30.0, 11.25, 0.75]);
        board(&mut doc, "Red Oak", "Shelf", DVec3::new(-0.25, 0.0, 12.0), DVec3::X, DVec3::Y, [20.0, 11.25, 0.75]);
        assert!(check(doc.scene(), 4.0).conflicts.is_empty(), "a shelf in a side moves with it");

        // A top (grain along X, 16" across Y) with a breadboard end across its end (grain along Y).
        let mut doc = Document::default();
        board(&mut doc, "Cherry", "Top", DVec3::ZERO, DVec3::X, DVec3::Y, [30.0, 16.0, 0.75]);
        board(&mut doc, "Cherry", "Breadboard end", DVec3::new(30.0, 0.0, 0.0), DVec3::Y, -DVec3::X, [16.0, 2.5, 0.75]);
        let report = check(doc.scene(), 4.0);
        let conflict = report.conflicts.iter().find(|c| c.moving == "Top").expect("the top fights the breadboard end");
        assert_eq!(conflict.fixed, "Breadboard end");
        assert!((conflict.span - 16.0).abs() < 1e-9);
        assert!(conflict.describe().contains("runs the other way"));
    }

    #[test]
    fn solid_wood_on_plywood_warns_and_plywood_doesnt_move() {
        let mut doc = Document::default();
        board(&mut doc, "Plywood", "Case top", DVec3::ZERO, DVec3::X, DVec3::Y, [30.0, 16.0, 0.75]);
        board(&mut doc, "Walnut", "Veneer slab", DVec3::new(0.0, 0.0, 0.75), DVec3::X, DVec3::Y, [30.0, 16.0, 0.75]);
        let report = check(doc.scene(), 4.0);
        assert_eq!(report.parts.len(), 1, "plywood isn't listed: it's stable");
        assert!(report.conflicts.iter().any(|c| c.moving == "Veneer slab" && c.fixed == "Case top"));
    }
}
