//! The Paint Bucket tool (B). Click a group or component to paint it (its unpainted faces show
//! the material), or, inside an open group, click a face to paint just that face. Alt-click
//! samples the material under the cursor, as in SketchUp.

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
        doc.paint(&[target], *material);
        Ok(())
    }

    pub fn status(&self) -> &'static str {
        "Click a group or face to paint it with the active material (Materials panel). Alt-click to sample one."
    }

    pub fn overlay(&self, doc: &Document, overlay: &mut Overlay) {
        if let Some(entity) = self.hover {
            highlight_entity(doc.view(), entity, overlay);
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
