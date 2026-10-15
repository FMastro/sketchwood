//! The Paint Bucket tool (B). Click anything to paint the whole object: a group or component,
//! or a face, which paints the open group it's in (the part) or all the loose faces connected to
//! it. Ctrl-click paints just the one face. Alt-click samples the material under the cursor, as
//! in SketchUp.

use glam::Vec2;
use sketchcraft_doc::{Document, Entity, MaterialId};

use crate::pick::Screen;
use crate::{Modifiers, Overlay, highlight_entity, pick_resolved};

#[derive(Default)]
pub struct PaintTool {
    hover: Option<Entity>,
}

impl PaintTool {
    pub fn hover(&mut self, doc: &Document, screen: Screen<'_>, cursor: Vec2) {
        self.hover = pick_resolved(doc, screen, cursor).filter(|e| !matches!(e, Entity::Edge(_)));
    }

    /// Paint what's under the cursor with `material`, or with Alt, sample it into `material`.
    pub fn click(
        &mut self,
        doc: &mut Document,
        screen: Screen<'_>,
        cursor: Vec2,
        mods: Modifiers,
        material: &mut Option<MaterialId>,
    ) -> Result<(), String> {
        self.hover(doc, screen, cursor);
        let Some(target) = self.hover else {
            return Err("Click a group, component or face to paint it.".into());
        };
        if mods.alt {
            *material = doc.material_of(target);
            return Ok(());
        }
        match target {
            Entity::Face(face) if !mods.ctrl => doc.paint_object(face, *material),
            _ => {
                doc.paint(&[target], *material);
            }
        }
        Ok(())
    }

    pub fn status(&self) -> &'static str {
        "Click a board to paint all of it with the material picked in the Materials panel. Ctrl-click: one face; Alt-click: sample."
    }

    pub fn overlay(&self, doc: &Document, overlay: &mut Overlay) {
        // A face lights up the whole object a click would paint.
        match self.hover {
            Some(Entity::Face(face)) => {
                for entity in doc.model().connected(Entity::Face(face)) {
                    highlight_entity(doc.view(), entity, overlay);
                }
            }
            Some(entity) => highlight_entity(doc.view(), entity, overlay),
            None => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use glam::DVec3;
    use sketchcraft_doc::MaterialKind;
    use sketchcraft_view::{OrbitCamera, StandardView};

    use super::*;

    const SIZE: Vec2 = Vec2::new(1000.0, 800.0);

    #[test]
    fn click_paints_a_board_and_alt_click_samples() {
        let mut doc = Document::default();
        let face = doc.edit(|m| m.add_rectangle(DVec3::ZERO, DVec3::X * 1.5, DVec3::Y * 3.5, DVec3::Z)).unwrap();
        let board = doc.push_pull(face, 10.0, true).unwrap().unwrap();
        let walnut = doc.add_material(MaterialKind::SolidWood);
        let mut camera = OrbitCamera { target: glam::Vec3::new(0.75, 1.75, 5.0), distance: 40.0, ..Default::default() };
        camera.set_view(StandardView::Iso);
        let screen = Screen::new(&camera, SIZE);
        let side = screen.project(DVec3::new(0.75, 0.0, 5.0)).unwrap();

        let mut tool = PaintTool::default();
        let mut active = Some(walnut);
        tool.click(&mut doc, screen, side, Modifiers::default(), &mut active).unwrap();
        assert_eq!(doc.material_of(Entity::Instance(board)), Some(walnut));

        let mut sampled = None;
        tool.click(&mut doc, screen, side, Modifiers { alt: true, ..Default::default() }, &mut sampled).unwrap();
        assert_eq!(sampled, Some(walnut));

        // Painting with "no material" restores the default.
        let mut none = None;
        tool.click(&mut doc, screen, side, Modifiers::default(), &mut none).unwrap();
        assert_eq!(doc.material_of(Entity::Instance(board)), None);
        assert!(tool.click(&mut doc, screen, Vec2::new(3.0, 3.0), Modifiers::default(), &mut none).is_err());
    }
}

#[cfg(test)]
mod whole_object {
    use glam::DVec3;
    use sketchcraft_doc::MaterialKind;
    use sketchcraft_view::{OrbitCamera, StandardView};

    use super::*;

    const SIZE: Vec2 = Vec2::new(1000.0, 800.0);

    fn setup(group: bool) -> (Document, OrbitCamera, Option<sketchcraft_doc::InstanceId>) {
        let mut doc = Document::default();
        let face = doc.edit(|m| m.add_rectangle(DVec3::ZERO, DVec3::X * 1.5, DVec3::Y * 3.5, DVec3::Z)).unwrap();
        let board = doc.push_pull(face, 10.0, group).unwrap();
        let mut camera = OrbitCamera { target: glam::Vec3::new(0.75, 1.75, 5.0), distance: 40.0, ..Default::default() };
        camera.set_view(StandardView::Iso);
        (doc, camera, board)
    }

    #[test]
    fn inside_an_open_group_a_face_paints_the_whole_board() {
        let (mut doc, camera, board) = setup(true);
        let board = board.unwrap();
        let pine = doc.add_material(MaterialKind::DimensionalLumber);
        doc.enter(board);
        let screen = Screen::in_context(&camera, SIZE, doc.context_transform());
        let side = screen.project(DVec3::new(0.75, 0.0, 5.0)).unwrap();
        PaintTool::default().click(&mut doc, screen, side, Modifiers::default(), &mut Some(pine)).unwrap();
        assert!(doc.model().faces().all(|(_, f)| f.material.is_none()), "faces show the board's material");
        doc.exit();
        assert_eq!(doc.material_of(Entity::Instance(board)), Some(pine), "the part itself is pine");
        doc.undo();
        assert_eq!(doc.material_of(Entity::Instance(board)), None, "one undo step");
    }

    #[test]
    fn loose_geometry_paints_all_connected_faces_and_ctrl_just_one() {
        let (mut doc, camera, _) = setup(false);
        let pine = doc.add_material(MaterialKind::SolidWood);
        let screen = Screen::new(&camera, SIZE);
        let side = screen.project(DVec3::new(0.75, 0.0, 5.0)).unwrap();
        let mut tool = PaintTool::default();
        tool.click(&mut doc, screen, side, Modifiers::default(), &mut Some(pine)).unwrap();
        assert!(doc.model().faces().all(|(_, f)| f.material == Some(pine)));
        let ctrl = Modifiers { ctrl: true, ..Default::default() };
        tool.click(&mut doc, screen, side, ctrl, &mut None).unwrap();
        assert_eq!(doc.model().faces().filter(|(_, f)| f.material.is_none()).count(), 1, "just that face");
    }
}
