//! A model plus its selection and undo history.

use std::collections::BTreeSet;

use crate::{Entity, Model};

/// Snapshots kept for undo. Models are small enough that whole-model snapshots are cheap.
const MAX_UNDO: usize = 200;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Selection(BTreeSet<Entity>);

impl Selection {
    pub fn contains(&self, entity: Entity) -> bool {
        self.0.contains(&entity)
    }

    pub fn iter(&self) -> impl Iterator<Item = Entity> + '_ {
        self.0.iter().copied()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn clear(&mut self) {
        self.0.clear();
    }

    pub fn extend(&mut self, entities: impl IntoIterator<Item = Entity>) {
        self.0.extend(entities);
    }

    pub fn remove(&mut self, entities: impl IntoIterator<Item = Entity>) {
        for entity in entities {
            self.0.remove(&entity);
        }
    }

    pub fn toggle(&mut self, entities: impl IntoIterator<Item = Entity>) {
        for entity in entities {
            if !self.0.remove(&entity) {
                self.0.insert(entity);
            }
        }
    }
}

#[derive(Default)]
pub struct Document {
    model: Model,
    selection: Selection,
    undo: Vec<Model>,
    redo: Vec<Model>,
    model_version: u64,
    version: u64,
}

impl Document {
    pub fn model(&self) -> &Model {
        &self.model
    }

    pub fn selection(&self) -> &Selection {
        &self.selection
    }

    pub fn selection_mut(&mut self) -> &mut Selection {
        self.version += 1;
        &mut self.selection
    }

    /// Changes whenever the model or the selection changes.
    pub fn version(&self) -> u64 {
        self.version
    }

    /// Changes whenever the model changes, including undo and redo.
    pub fn model_version(&self) -> u64 {
        self.model_version
    }

    /// Change the model as one undoable step.
    pub fn edit<R>(&mut self, change: impl FnOnce(&mut Model) -> R) -> R {
        self.undo.push(self.model.clone());
        if self.undo.len() > MAX_UNDO {
            self.undo.remove(0);
        }
        self.redo.clear();
        let result = change(&mut self.model);
        self.changed();
        result
    }

    pub fn undo(&mut self) -> bool {
        let Some(previous) = self.undo.pop() else { return false };
        self.redo.push(std::mem::replace(&mut self.model, previous));
        self.changed();
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(next) = self.redo.pop() else { return false };
        self.undo.push(std::mem::replace(&mut self.model, next));
        self.changed();
        true
    }

    pub fn erase_selection(&mut self) -> bool {
        if self.selection.is_empty() {
            return false;
        }
        let doomed: Vec<Entity> = self.selection.iter().collect();
        self.edit(|model| model.erase(doomed));
        true
    }

    pub fn select_all(&mut self) {
        let all: Vec<Entity> = self.model.entities().collect();
        let selection = self.selection_mut();
        selection.clear();
        selection.extend(all);
    }

    fn changed(&mut self) {
        self.model_version += 1;
        self.version += 1;
        let model = &self.model;
        self.selection.0.retain(|&e| model.contains(e));
    }
}

#[cfg(test)]
mod tests {
    use glam::DVec3;

    use super::*;

    fn add_square(doc: &mut Document) {
        doc.edit(|m| m.add_rectangle(DVec3::ZERO, DVec3::X, DVec3::Y, DVec3::Z));
    }

    #[test]
    fn undo_and_redo_restore_models() {
        let mut doc = Document::default();
        add_square(&mut doc);
        assert!(!doc.model().is_empty());
        assert!(doc.undo());
        assert!(doc.model().is_empty());
        assert!(doc.redo());
        assert!(!doc.model().is_empty());
        assert!(!doc.redo());
    }

    #[test]
    fn erasing_drops_erased_entities_from_selection() {
        let mut doc = Document::default();
        add_square(&mut doc);
        doc.select_all();
        assert_eq!(doc.selection().len(), 5);
        assert!(doc.erase_selection());
        assert!(doc.selection().is_empty());
        assert!(doc.model().is_empty());
        // Undo brings the geometry back, but not the selection.
        doc.undo();
        assert!(!doc.model().is_empty());
    }

    #[test]
    fn new_edit_clears_redo() {
        let mut doc = Document::default();
        add_square(&mut doc);
        doc.undo();
        add_square(&mut doc);
        assert!(!doc.redo());
    }
}
