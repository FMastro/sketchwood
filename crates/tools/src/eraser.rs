//! The Eraser (E). Click an edge, guide, group or component to erase it, or drag across several
//! to mark them and erase them all on release, as one undo step. Erasing an edge takes the
//! faces it bounds with it, except that an edge between two faces on one plane merges them
//! (how you heal a face a line split). Faces themselves aren't erased: erase their edges.

use std::collections::BTreeSet;

use glam::Vec2;
use woodsketch_doc::{Document, Entity};

use crate::pick::Screen;
use crate::{Overlay, highlight_entity, pick_resolved};

#[derive(Default)]
pub struct EraserTool {
    hover: Option<Entity>,
    /// Marked by dragging, erased on release.
    marked: BTreeSet<Entity>,
    dragging: bool,
}

impl EraserTool {
    pub fn is_busy(&self) -> bool {
        self.dragging
    }

    pub fn cancel(&mut self) {
        self.marked.clear();
        self.dragging = false;
    }

    pub fn hover(&mut self, doc: &Document, screen: Screen<'_>, cursor: Vec2) {
        self.hover = erasable(doc, screen, cursor);
        if self.dragging
            && let Some(entity) = self.hover
        {
            self.marked.insert(entity);
        }
    }

    pub fn click(&mut self, doc: &mut Document, screen: Screen<'_>, cursor: Vec2) -> Result<(), String> {
        match erasable(doc, screen, cursor) {
            Some(entity) => {
                doc.erase(&[entity]);
                self.hover = None;
                Ok(())
            }
            None => Err("Click an edge, guide, group or component to erase it".into()),
        }
    }

    pub fn drag_start(&mut self, doc: &Document, screen: Screen<'_>, origin: Vec2) {
        self.marked.clear();
        self.dragging = true;
        self.hover(doc, screen, origin);
    }

    pub fn drag_end(&mut self, doc: &mut Document, screen: Screen<'_>, cursor: Vec2) {
        self.hover(doc, screen, cursor);
        self.dragging = false;
        let marked: Vec<Entity> = std::mem::take(&mut self.marked).into_iter().collect();
        if !marked.is_empty() {
            doc.erase(&marked);
        }
        self.hover = None;
    }

    pub fn status(&self) -> &'static str {
        "Click or drag over edges, guides or groups to erase them."
    }

    pub fn overlay(&self, doc: &Document, overlay: &mut Overlay) {
        let view = doc.view();
        for &entity in self.marked.iter().chain(&self.hover) {
            match entity {
                Entity::Guide(id) => {
                    if let Some(guide) = doc.model().guide(id) {
                        overlay.guides.push((*guide, true));
                    }
                }
                _ => highlight_entity(view, entity, overlay),
            }
        }
    }
}

/// What a click would erase: anything under the cursor except faces.
fn erasable(doc: &Document, screen: Screen<'_>, cursor: Vec2) -> Option<Entity> {
    pick_resolved(doc, screen, cursor).filter(|e| !matches!(e, Entity::Face(_)))
}

#[cfg(test)]
mod tests {
    use glam::DVec3;
    use woodsketch_view::{OrbitCamera, StandardView};

    use super::*;

    const SIZE: Vec2 = Vec2::new(1000.0, 800.0);

    fn camera() -> OrbitCamera {
        let mut camera = OrbitCamera { target: glam::Vec3::new(5.0, 5.0, 0.0), distance: 40.0, ..Default::default() };
        camera.set_view(StandardView::Top);
        camera
    }

    fn at(screen: Screen<'_>, x: f64, y: f64) -> Vec2 {
        screen.project(DVec3::new(x, y, 0.0)).unwrap()
    }

    #[test]
    fn erasing_a_splitting_line_heals_the_face() {
        let mut doc = Document::default();
        doc.edit(|m| {
            m.add_rectangle(DVec3::ZERO, DVec3::X * 10.0, DVec3::Y * 10.0, DVec3::Z);
            m.add_line(DVec3::new(4.0, 0.0, 0.0), DVec3::new(4.0, 10.0, 0.0));
        });
        assert_eq!(doc.model().faces().count(), 2);
        let camera = camera();
        let screen = Screen::new(&camera, SIZE);
        let mut tool = EraserTool::default();
        tool.click(&mut doc, screen, at(screen, 4.0, 6.0)).unwrap();
        assert_eq!(doc.model().faces().count(), 1);
        // Clicking inside a face erases nothing.
        assert!(tool.click(&mut doc, screen, at(screen, 7.0, 7.0)).is_err());
        assert_eq!(doc.model().faces().count(), 1);
    }

    #[test]
    fn dragging_erases_everything_crossed_in_one_step() {
        let mut doc = Document::default();
        doc.edit(|m| {
            for x in [2.0, 4.0, 6.0] {
                m.add_line(DVec3::new(x, 0.0, 0.0), DVec3::new(x, 10.0, 0.0));
            }
        });
        let camera = camera();
        let screen = Screen::new(&camera, SIZE);
        let mut tool = EraserTool::default();
        tool.drag_start(&doc, screen, at(screen, 1.0, 5.0));
        for x in [1.5, 2.0, 3.0, 4.0, 5.0, 6.0, 6.5] {
            tool.hover(&doc, screen, at(screen, x, 5.0));
        }
        assert_eq!(tool.marked.len(), 3);
        tool.drag_end(&mut doc, screen, at(screen, 6.5, 5.0));
        assert!(doc.model().is_empty());
        doc.undo();
        assert_eq!(doc.model().edges().count(), 3, "one undo brings all three back");
    }

    #[test]
    fn guides_and_groups_erase_too() {
        let mut doc = Document::default();
        doc.edit(|m| m.add_guide(woodsketch_doc::Guide::Line { point: DVec3::new(0.0, 8.0, 0.0), dir: DVec3::X }));
        let face = doc.edit(|m| m.add_rectangle(DVec3::ZERO, DVec3::X * 3.0, DVec3::Y * 3.0, DVec3::Z)).unwrap();
        doc.push_pull(face, 1.0, true).unwrap();
        let camera = camera();
        let screen = Screen::new(&camera, SIZE);
        let mut tool = EraserTool::default();
        tool.click(&mut doc, screen, at(screen, 9.0, 8.0)).unwrap();
        assert_eq!(doc.model().guides().count(), 0);
        let top = screen.project(DVec3::new(1.5, 1.5, 1.0)).unwrap();
        tool.click(&mut doc, screen, top).unwrap();
        assert_eq!(doc.container().instances().count(), 0);
    }
}
