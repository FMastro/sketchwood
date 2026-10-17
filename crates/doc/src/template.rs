//! Templates: common furniture assemblies built from boards and real joints, so the cut list,
//! build sheets, joint list and movement checks work on them as on parts drawn by hand.
//!
//! - **Cabinet** (or bookcase): sides, top, bottom and shelves between them, a 1/4" plywood
//!   back; dados and rabbets, biscuits, pocket screws or plain butt joints.
//! - **Drawer**: a 1/2" box (front, back, sides) with dovetails, box joints or rabbets, and a
//!   1/4" plywood bottom in grooves.
//! - **Face frame**: stiles and rails, with mortise and tenon, pocket screws or dowels.
//! - **Table**: one leg component turned to each corner, apron components joined to the legs,
//!   a top overhanging 1".
//!
//! Each is one group named for it, made in one undo step, and remembers its sizes
//! ([`Made`] on its definition) so [`Document::rebuild_template`] can change them.

use glam::{DAffine3, DVec3};
use serde::{Deserialize, Serialize};

use crate::board::Board;
use crate::joinery::{Dovetails, Dowels, JoineryError, Slots, Tenon, pocket_screw_for};
use crate::library::LIBRARY;
use crate::{DefinitionKind, Document, Entity, InstanceId, MaterialId};

/// Case and frame stock: 3/4".
const STOCK: f64 = 0.75;
/// Drawer box stock: 1/2".
const DRAWER_STOCK: f64 = 0.5;
/// Backs and drawer bottoms: 1/4" plywood.
const PANEL: f64 = 0.25;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Template {
    Cabinet(Cabinet),
    Drawer(Drawer),
    FaceFrame(FaceFrame),
    Table(Table),
}

/// Overall sizes, in inches. Width along X (red), depth along Y (green) from the front, height
/// up Z (blue).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Cabinet {
    pub width: f64,
    pub height: f64,
    /// Including the back.
    pub depth: f64,
    pub shelves: u32,
    pub back: bool,
    pub joints: CaseJoints,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Drawer {
    pub width: f64,
    pub height: f64,
    pub depth: f64,
    pub joints: DrawerJoints,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct FaceFrame {
    pub width: f64,
    pub height: f64,
    /// Stile width (the uprights).
    pub stile: f64,
    /// Rail width (top and bottom).
    pub rail: f64,
    pub joints: FrameJoints,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Table {
    pub width: f64,
    pub depth: f64,
    /// To the top of the top.
    pub height: f64,
    pub joints: FrameJoints,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CaseJoints {
    /// Shelves in dados; the top and bottom in rabbets.
    Dados,
    Biscuits,
    PocketScrews,
    Butt,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DrawerJoints {
    Dovetails,
    BoxJoints,
    Rabbets,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FrameJoints {
    MortiseAndTenon,
    PocketScrews,
    Dowels,
}

impl CaseJoints {
    pub const ALL: [Self; 4] = [Self::Dados, Self::Biscuits, Self::PocketScrews, Self::Butt];
    pub fn name(self) -> &'static str {
        match self {
            Self::Dados => "Dados and rabbets",
            Self::Biscuits => "Biscuits",
            Self::PocketScrews => "Pocket screws",
            Self::Butt => "Butt joints",
        }
    }
}

impl DrawerJoints {
    pub const ALL: [Self; 3] = [Self::Dovetails, Self::BoxJoints, Self::Rabbets];
    pub fn name(self) -> &'static str {
        match self {
            Self::Dovetails => "Dovetails",
            Self::BoxJoints => "Box joints",
            Self::Rabbets => "Rabbets",
        }
    }
}

impl FrameJoints {
    pub const ALL: [Self; 3] = [Self::MortiseAndTenon, Self::PocketScrews, Self::Dowels];
    pub fn name(self) -> &'static str {
        match self {
            Self::MortiseAndTenon => "Mortise and tenon",
            Self::PocketScrews => "Pocket screws",
            Self::Dowels => "Dowels",
        }
    }
}

impl Default for Cabinet {
    fn default() -> Self {
        Self { width: 30.0, height: 36.0, depth: 12.0, shelves: 2, back: true, joints: CaseJoints::Dados }
    }
}

impl Default for Drawer {
    fn default() -> Self {
        Self { width: 16.0, height: 5.0, depth: 14.0, joints: DrawerJoints::Dovetails }
    }
}

impl Default for FaceFrame {
    fn default() -> Self {
        Self { width: 30.0, height: 36.0, stile: 1.5, rail: 1.5, joints: FrameJoints::MortiseAndTenon }
    }
}

impl Default for Table {
    fn default() -> Self {
        Self { width: 36.0, depth: 20.0, height: 30.0, joints: FrameJoints::MortiseAndTenon }
    }
}

impl Template {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Cabinet(_) => "Cabinet",
            Self::Drawer(_) => "Drawer",
            Self::FaceFrame(_) => "Face frame",
            Self::Table(_) => "Table",
        }
    }

    /// One of each, with starting sizes.
    pub fn all() -> [Self; 4] {
        [Self::Cabinet(Cabinet::default()), Self::Drawer(Drawer::default()), Self::FaceFrame(FaceFrame::default()), Self::Table(Table::default())]
    }

    /// Why these sizes can't be built, if they can't.
    pub fn check(&self) -> Result<(), String> {
        let small = |what: &str, min: f64| format!("The {what} must be over {}.", crate::units::format_length(min));
        match *self {
            Self::Cabinet(c) => {
                if c.width <= 2.0 * STOCK + 1.0 {
                    return Err(small("width", 2.0 * STOCK + 1.0));
                }
                if c.depth <= 2.0 {
                    return Err(small("depth", 2.0));
                }
                let room = c.height - 2.0 * STOCK - f64::from(c.shelves) * STOCK;
                if room <= f64::from(c.shelves + 1) {
                    return Err("Too many shelves for the height.".into());
                }
            }
            Self::Drawer(d) => {
                if d.width <= 4.0 || d.depth <= 4.0 {
                    return Err("The width and depth must be over 4\".".into());
                }
                if d.height <= 2.0 {
                    return Err(small("height", 2.0));
                }
            }
            Self::FaceFrame(f) => {
                if f.stile <= 0.5 || f.rail <= 0.5 {
                    return Err("Stiles and rails must be over 1/2\" wide.".into());
                }
                if f.width <= 2.0 * f.stile + 1.0 {
                    return Err(small("width", 2.0 * f.stile + 1.0));
                }
                if f.height <= 2.0 * f.rail + 1.0 {
                    return Err(small("height", 2.0 * f.rail + 1.0));
                }
            }
            Self::Table(t) => {
                if t.width <= 12.0 || t.depth <= 12.0 {
                    return Err("The width and depth must be over 12\".".into());
                }
                if t.height <= 10.0 {
                    return Err(small("height", 10.0));
                }
            }
        }
        Ok(())
    }
}

/// What a template group was made from, kept on its definition.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Made {
    pub template: Template,
    pub wood: Option<MaterialId>,
}

impl Document {
    /// Build `template` in `wood` beside the drawing, as one group and one undo step, and
    /// select it.
    pub fn add_template(&mut self, template: &Template, wood: Option<MaterialId>) -> Result<InstanceId, String> {
        template.check()?;
        // To the right of everything there is, a foot away.
        let to = self.bounds().map_or(DVec3::ZERO, |(lo, hi)| DVec3::new(hi.x + 12.0, lo.y, 0.0));
        let to = self.context_transform().inverse().transform_point3(to);
        self.as_one_step(|doc| {
            let group = doc.build(template, wood)?;
            doc.place(group, DAffine3::from_translation(to));
            Ok(group)
        })
    }

    /// Build `template` again in place of `instance` (a template group), as one undo step.
    /// Changes made inside it by hand are lost.
    pub fn rebuild_template(&mut self, instance: InstanceId, template: &Template) -> Result<InstanceId, String> {
        template.check()?;
        let made = self.template_of(instance).ok_or("That isn't made from a template.")?;
        let old = self.container().instance(instance).ok_or("That isn't in the group being edited.")?;
        let (transform, tag, material) = (old.transform, old.tag, old.material);
        self.as_one_step(|doc| {
            doc.erase(&[Entity::Instance(instance)]);
            let group = doc.build(template, made.wood)?;
            doc.place(group, transform);
            doc.edit_scene(|scene, path| {
                if let Some(i) = scene.container_mut(path).and_then(|c| c.instance_mut(group)) {
                    i.tag = tag;
                    i.material = material;
                }
            });
            Ok(group)
        })
    }

    /// What the group `instance` was made from, if it's a template.
    pub fn template_of(&self, instance: InstanceId) -> Option<Made> {
        let definition = self.container().instance(instance)?.definition;
        self.scene().definition(definition)?.template
    }

    /// Move the just-made `group` (its corner at the origin) by `transform`.
    fn place(&mut self, group: InstanceId, transform: DAffine3) {
        self.edit_scene(|scene, path| {
            if let Some(i) = scene.container_mut(path).and_then(|c| c.instance_mut(group)) {
                i.transform = transform * i.transform;
            }
        });
    }

    /// Build the parts at the origin and group them, remembering the template.
    fn build(&mut self, template: &Template, wood: Option<MaterialId>) -> Result<InstanceId, String> {
        let mut parts = Parts { doc: self, wood, made: Vec::new() };
        match *template {
            Template::Cabinet(c) => parts.cabinet(c),
            Template::Drawer(d) => parts.drawer(d),
            Template::FaceFrame(f) => parts.face_frame(f),
            Template::Table(t) => parts.table(t),
        }
        .map_err(|e| format!("Couldn't build the {}: {e}", template.name().to_lowercase()))?;
        let made = parts.made;
        self.selection_mut().clear();
        self.selection_mut().extend(made.into_iter().map(Entity::Instance));
        let group = self.group_selection(DefinitionKind::Group).ok_or("Nothing was built.")?;
        self.rename_instance(group, template.name());
        let record = Made { template: *template, wood };
        self.edit_scene(|scene, path| {
            let definition = scene.container(path).and_then(|c| c.instance(group)).map(|i| i.definition);
            if let Some(d) = definition.and_then(|d| scene.definition_mut(d)) {
                d.template = Some(record);
            }
        });
        let selection = self.selection_mut();
        selection.clear();
        selection.extend([Entity::Instance(group)]);
        Ok(group)
    }
}

/// Builds one template's parts, keeping the top-level ones made.
struct Parts<'a> {
    doc: &'a mut Document,
    wood: Option<MaterialId>,
    made: Vec<InstanceId>,
}

impl Parts<'_> {
    fn plywood(&mut self) -> MaterialId {
        let preset = LIBRARY.iter().find(|p| p.name == "Plywood").expect("Plywood is in the library");
        self.doc.add_preset(preset)
    }

    /// A board `[length, width, thickness]`: thickness runs along `length_dir × width_dir`.
    fn board(&mut self, name: &str, material: Option<MaterialId>, origin: DVec3, length_dir: DVec3, width_dir: DVec3, [length, width, thickness]: [f64; 3]) -> InstanceId {
        let id = self.doc.add_board(&Board { origin, length_dir, width_dir, length, width, thickness }, material).expect("templates check their sizes");
        self.doc.rename_instance(id, name);
        self.made.push(id);
        id
    }

    fn component(&mut self, part: InstanceId, name: &str) -> InstanceId {
        self.doc.selection_mut().clear();
        self.doc.selection_mut().extend([Entity::Instance(part)]);
        let id = self.doc.group_selection(DefinitionKind::Component).expect("a part");
        self.doc.rename_instance(id, name);
        id
    }

    /// Copies of `part` turned `degrees` about the vertical through `center`, then moved to
    /// each spot.
    fn copies(&mut self, part: InstanceId, center: DVec3, spots: &[(DVec3, f64)]) -> Vec<InstanceId> {
        let transforms: Vec<DAffine3> = spots
            .iter()
            .map(|&(to, degrees)| DAffine3::from_translation(to) * DAffine3::from_rotation_z(degrees.to_radians()) * DAffine3::from_translation(-center))
            .collect();
        let copies: Vec<InstanceId> = self
            .doc
            .transform_entities(&[Entity::Instance(part)], &transforms, true)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|e| if let Entity::Instance(i) = e { Some(i) } else { None })
            .collect();
        self.made.extend(&copies);
        copies
    }

    /// Join `part`'s end to `into` the way a frame or case joint is chosen.
    fn frame_joint(&mut self, part: InstanceId, into: InstanceId, joints: FrameJoints, tenon: Option<Tenon>) -> Result<(), JoineryError> {
        let fit = self.doc.joint_fit(part, into)?;
        let (thick, wide) = fit.contact;
        match joints {
            FrameJoints::MortiseAndTenon => self.doc.mortise_and_tenon(part, into, tenon.unwrap_or(fit.tenon)),
            FrameJoints::PocketScrews => self.doc.pocket_screws(part, into, pocket_count(wide), pocket_screw_for(thick)),
            FrameJoints::Dowels => {
                // Short enough that holes from two sides of a leg don't meet.
                let depth = if tenon.is_some() { fit.dowels.depth.min(0.875) } else { fit.dowels.depth };
                self.doc.dowels(part, into, Dowels { depth, ..fit.dowels })
            }
        }
    }

    fn cabinet(&mut self, c: Cabinet) -> Result<(), JoineryError> {
        let t = STOCK;
        let deep = if c.back { c.depth - PANEL } else { c.depth };
        let wood = self.wood;
        let left = self.board("Side", wood, DVec3::new(t, 0.0, 0.0), DVec3::Z, DVec3::Y, [c.height, deep, t]);
        let right = self.board("Side", wood, DVec3::new(c.width, 0.0, 0.0), DVec3::Z, DVec3::Y, [c.height, deep, t]);
        let inside = c.width - 2.0 * t;
        let mut across = vec![
            self.board("Bottom", wood, DVec3::new(t, 0.0, 0.0), DVec3::X, DVec3::Y, [inside, deep, t]),
            self.board("Top", wood, DVec3::new(t, 0.0, c.height - t), DVec3::X, DVec3::Y, [inside, deep, t]),
        ];
        let n = f64::from(c.shelves);
        let gap = (c.height - 2.0 * t - n * t) / (n + 1.0);
        for i in 0..c.shelves {
            let z = t + gap * f64::from(i + 1) + t * f64::from(i);
            across.push(self.board("Shelf", wood, DVec3::new(t, 0.0, z), DVec3::X, DVec3::Y, [inside, deep, t]));
        }
        for &part in &across {
            for side in [left, right] {
                let (thick, wide) = (t, deep);
                match c.joints {
                    CaseJoints::Dados => self.doc.let_in(part, side, t / 3.0).map(|_| ())?,
                    CaseJoints::Biscuits => self.doc.slots(part, side, &Slots::biscuits(Slots::suggested_biscuit(thick, wide), wide))?,
                    CaseJoints::PocketScrews => self.doc.pocket_screws(part, side, pocket_count(wide), pocket_screw_for(thick))?,
                    CaseJoints::Butt => {}
                }
            }
        }
        if c.back {
            let plywood = Some(self.plywood());
            self.board("Back", plywood, DVec3::new(0.0, deep, c.height), DVec3::X, -DVec3::Z, [c.width, c.height, PANEL]);
        }
        Ok(())
    }

    fn drawer(&mut self, d: Drawer) -> Result<(), JoineryError> {
        let t = DRAWER_STOCK;
        let wood = self.wood;
        // The front and back run the full width; the sides butt their insides, and the joints
        // carry them out to the corners.
        let front = self.board("Drawer front", wood, DVec3::new(0.0, t, 0.0), DVec3::X, DVec3::Z, [d.width, d.height, t]);
        let back = self.board("Drawer back", wood, DVec3::new(d.width, d.depth - t, 0.0), -DVec3::X, DVec3::Z, [d.width, d.height, t]);
        let left = self.board("Drawer side", wood, DVec3::new(t, t, d.height), DVec3::Y, -DVec3::Z, [d.depth - 2.0 * t, d.height, t]);
        let right = self.board("Drawer side", wood, DVec3::new(d.width - t, d.depth - t, d.height), -DVec3::Y, -DVec3::Z, [d.depth - 2.0 * t, d.height, t]);
        // The bottom, 1/2" up, between the sides, front and back; let into each 1/4".
        let plywood = Some(self.plywood());
        let up = 0.5;
        let bottom = self.board("Drawer bottom", plywood, DVec3::new(t, t, up), DVec3::X, DVec3::Y, [d.width - 2.0 * t, d.depth - 2.0 * t, PANEL]);
        for side in [left, right] {
            for end in [front, back] {
                match d.joints {
                    DrawerJoints::Dovetails => {
                        let tails = Dovetails::suggested(d.height);
                        self.doc.dovetails(side, end, Dovetails { tails, half_blind: false })?;
                    }
                    DrawerJoints::BoxJoints => {
                        let finger = self.doc.joint_fit(side, end)?.finger;
                        self.doc.box_joint(side, end, finger)?;
                    }
                    DrawerJoints::Rabbets => self.doc.let_in(side, end, t / 2.0).map(|_| ())?,
                }
            }
        }
        // The bottom's grooves after the corners: the front and back first (stopping short of
        // the corner joints), then the sides, through their tails.
        for into in [front, back, left, right] {
            self.doc.let_in(bottom, into, PANEL).map(|_| ())?;
        }
        Ok(())
    }

    fn face_frame(&mut self, f: FaceFrame) -> Result<(), JoineryError> {
        let t = STOCK;
        let wood = self.wood;
        let left = self.board("Stile", wood, DVec3::ZERO, DVec3::Z, DVec3::X, [f.height, f.stile, t]);
        let right = self.board("Stile", wood, DVec3::new(f.width - f.stile, 0.0, 0.0), DVec3::Z, DVec3::X, [f.height, f.stile, t]);
        let long = f.width - 2.0 * f.stile;
        let bottom = self.board("Bottom rail", wood, DVec3::new(f.stile, t, 0.0), DVec3::X, DVec3::Z, [long, f.rail, t]);
        let top = self.board("Top rail", wood, DVec3::new(f.stile, t, f.height - f.rail), DVec3::X, DVec3::Z, [long, f.rail, t]);
        for rail in [bottom, top] {
            for stile in [left, right] {
                self.frame_joint(rail, stile, f.joints, None)?;
            }
        }
        Ok(())
    }

    fn table(&mut self, t: Table) -> Result<(), JoineryError> {
        let wood = self.wood;
        let (leg, apron, thick, overhang, set_back) = (1.75, 4.0_f64.min((t.height - STOCK) / 3.0), STOCK, 1.0, 0.25);
        let legs_up = t.height - STOCK;
        // One leg, made a component and turned to each corner so its inside faces (where the
        // joints go) always face in.
        let first = self.board("Leg", wood, DVec3::new(overhang, overhang, 0.0), DVec3::Z, DVec3::X, [legs_up, leg, leg]);
        self.made.pop();
        let front_left = self.component(first, "Leg");
        self.made.push(front_left);
        let c = overhang + leg / 2.0;
        let others = self.copies(front_left, DVec3::new(c, c, 0.0), &[
            (DVec3::new(t.width - c, c, 0.0), 90.0),
            (DVec3::new(t.width - c, t.depth - c, 0.0), 180.0),
            (DVec3::new(c, t.depth - c, 0.0), 270.0),
        ]);
        let [front_right, back_right, back_left] = others[..] else { return Err(JoineryError::NotParts) };

        // Aprons set back from the legs' outside faces; the back a copy of the front turned
        // around, the right a copy of the left.
        let middle = DVec3::new(t.width / 2.0, t.depth / 2.0, 0.0);
        let inner = overhang + leg;
        let front = self.board("Front apron", wood, DVec3::new(inner, overhang + set_back, legs_up), DVec3::X, -DVec3::Z, [t.width - 2.0 * inner, apron, thick]);
        self.made.pop();
        let front = self.component(front, "Front and back apron");
        self.made.push(front);
        let back = self.copies(front, middle, &[(middle, 180.0)])[0];
        let left = self.board("Side apron", wood, DVec3::new(overhang + set_back + thick, inner, legs_up), DVec3::Y, -DVec3::Z, [t.depth - 2.0 * inner, apron, thick]);
        self.made.pop();
        let left = self.component(left, "Side apron");
        self.made.push(left);
        let right = self.copies(left, middle, &[(middle, 180.0)])[0];

        // Tenons 3/4" long, so the two mortises in each leg don't meet.
        let tenon = Tenon { length: 0.75, thickness: 0.25, shoulder: 0.25, haunch: 0.0 };
        for (apron, leg) in [
            (front, front_left),
            (front, front_right),
            (back, back_right),
            (back, back_left),
            (left, front_left),
            (left, back_left),
            (right, front_right),
            (right, back_right),
        ] {
            self.frame_joint(apron, leg, t.joints, Some(tenon))?;
        }
        self.board("Table top", wood, DVec3::new(0.0, 0.0, legs_up), DVec3::X, DVec3::Y, [t.width, t.depth, STOCK]);
        Ok(())
    }
}

/// About one pocket screw every 4", at least two.
fn pocket_count(wide: f64) -> u32 {
    ((wide / 4.0).round() as u32).max(2)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn closed(doc: &Document, part: InstanceId, path: &[InstanceId]) -> bool {
        let container = doc.scene().container(path).unwrap();
        let def = container.instance(part).unwrap().definition;
        let m = &doc.scene().definition(def).unwrap().container.model;
        m.faces().count() > 0 && m.edges().all(|(id, _)| m.edge_faces(id).len() == 2)
    }

    /// Every part inside the template group is a closed solid.
    fn all_closed(doc: &Document, group: InstanceId) -> usize {
        let path = [group];
        let container = doc.scene().container(&path).unwrap();
        let parts: Vec<InstanceId> = container.instances().map(|(id, _)| id).collect();
        for &p in &parts {
            assert!(closed(doc, p, &path), "{:?} is a closed solid", doc.scene().definition(container.instance(p).unwrap().definition).unwrap().name);
        }
        parts.len()
    }

    fn cut_list_names(doc: &Document) -> Vec<(String, usize)> {
        let list = crate::cutlist::cut_list(doc.scene());
        let mut names: Vec<(String, usize)> = list.materials.iter().flat_map(|m| m.parts.iter().map(|p| (p.names.join(", "), p.count))).collect();
        names.sort();
        names
    }

    #[test]
    fn every_template_builds_with_every_joint() {
        let mut templates = Vec::new();
        for joints in CaseJoints::ALL {
            templates.push(Template::Cabinet(Cabinet { joints, ..Default::default() }));
        }
        for joints in DrawerJoints::ALL {
            templates.push(Template::Drawer(Drawer { joints, ..Default::default() }));
        }
        for joints in FrameJoints::ALL {
            templates.push(Template::FaceFrame(FaceFrame { joints, ..Default::default() }));
            templates.push(Template::Table(Table { joints, ..Default::default() }));
        }
        for template in templates {
            let mut doc = Document::default();
            let group = doc.add_template(&template, None).unwrap_or_else(|e| panic!("{template:?}: {e}"));
            all_closed(&doc, group);
            assert!(doc.undo(), "{template:?} is one undo step");
            assert!(doc.container().instances().next().is_none(), "{template:?}: undone, nothing is left");
        }
    }

    #[test]
    fn a_cabinet_lists_its_parts() {
        let mut doc = Document::default();
        let group = doc.add_template(&Template::Cabinet(Cabinet::default()), None).unwrap();
        assert_eq!(all_closed(&doc, group), 7, "two sides, top, bottom, two shelves, back");
        assert_eq!(doc.instance_name(group), Some("Cabinet"));
        let names = cut_list_names(&doc);
        assert!(names.contains(&("Side".into(), 2)), "{names:?}");
        assert!(names.contains(&("Bottom, Top, Shelf".into(), 4)), "same size: {names:?}");
        // Let in 1/4" at each end: 28 1/2" between the sides, 29" long.
        let shelf = doc.scene().container(&[group]).unwrap().instances().find(|(_, i)| doc.scene().definition(i.definition).unwrap().name == "Shelf").unwrap().0;
        doc.enter(group);
        assert_eq!(doc.part_size(shelf).unwrap()[0], 29.0);
        assert!(!doc.joint_list(shelf).is_empty());
    }

    #[test]
    fn a_drawer_keeps_its_outside_size() {
        let mut doc = Document::default();
        let group = doc.add_template(&Template::Drawer(Drawer::default()), None).unwrap();
        let (lo, hi) = doc.bounds().unwrap();
        assert!((hi - lo - DVec3::new(16.0, 14.0, 5.0)).abs().max_element() < 1e-9, "{:?}", hi - lo);
        assert_eq!(all_closed(&doc, group), 5);
    }

    #[test]
    fn a_table_shares_its_leg() {
        let mut doc = Document::default();
        doc.add_template(&Template::Table(Table::default()), None).unwrap();
        let names = cut_list_names(&doc);
        assert!(names.contains(&("Leg".into(), 4)), "{names:?}");
        assert!(names.contains(&("Front and back apron".into(), 2)), "{names:?}");
        let (lo, hi) = doc.bounds().unwrap();
        assert!((hi - lo - DVec3::new(36.0, 20.0, 30.0)).abs().max_element() < 1e-9, "{:?}", hi - lo);
    }

    #[test]
    fn templates_go_beside_the_drawing_and_change_size_in_place() {
        let mut doc = Document::default();
        let first = doc.add_template(&Template::FaceFrame(FaceFrame::default()), None).unwrap();
        let second = doc.add_template(&Template::Cabinet(Cabinet::default()), None).unwrap();
        let x = |doc: &Document, id| doc.container().instance(id).unwrap().transform.translation.x;
        assert_eq!(x(&doc, second), 42.0, "a foot to the right of the face frame");
        // Moved, then made taller: it stays where it was moved.
        doc.edit_scene(|scene, path| scene.container_mut(path).unwrap().instance_mut(first).unwrap().transform.translation.y = 5.0);
        let Some(Made { template: Template::FaceFrame(f), .. }) = doc.template_of(first) else { panic!("a face frame") };
        let taller = doc.rebuild_template(first, &Template::FaceFrame(FaceFrame { height: 40.0, ..f })).unwrap();
        assert_eq!(doc.container().instance(taller).unwrap().transform.translation.y, 5.0);
        assert!(doc.container().instance(first).is_none());
        let Some(Made { template: Template::FaceFrame(f), .. }) = doc.template_of(taller) else { panic!("still a face frame") };
        assert_eq!(f.height, 40.0);
        assert!(doc.undo());
        assert!(doc.container().instance(first).is_some(), "one undo step brings the old one back");
    }

    #[test]
    fn sizes_that_cant_be_built_say_why() {
        let mut doc = Document::default();
        let err = doc.add_template(&Template::Cabinet(Cabinet { height: 6.0, shelves: 5, ..Default::default() }), None).unwrap_err();
        assert!(err.contains("shelves"), "{err}");
        assert!(doc.container().instances().next().is_none());
    }
}