//! A model plus its selection and undo history.

use std::collections::BTreeSet;

use glam::{DAffine3, DVec3};

use crate::scene::{Container, DefinitionKind, Instance, InstanceId, Scene, View};
use crate::{Entity, FaceId, Model, MoveError, PushPullError};

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

/// What undo restores: the drawing and which group was being edited.
#[derive(Clone, Default)]
struct State {
    scene: Scene,
    path: Vec<InstanceId>,
}

pub struct Document {
    state: State,
    selection: Selection,
    undo: Vec<State>,
    redo: Vec<State>,
    /// The whole drawing flattened for picking, snapping and drawing; rebuilt on every change.
    view: View,
    model_version: u64,
    version: u64,
}

impl Default for Document {
    fn default() -> Self {
        let state = State::default();
        let view = View::build(&state.scene, &state.path);
        Self { state, selection: Selection::default(), undo: Vec::new(), redo: Vec::new(), view, model_version: 0, version: 0 }
    }
}

impl Document {
    pub fn scene(&self) -> &Scene {
        &self.state.scene
    }

    /// The groups being edited, outermost first; empty at the top level.
    pub fn path(&self) -> &[InstanceId] {
        &self.state.path
    }

    /// The container being edited.
    pub fn container(&self) -> &Container {
        self.state.scene.container(&self.state.path).unwrap_or(&self.state.scene.root)
    }

    /// Loose geometry of the container being edited, in its own coordinates.
    pub fn model(&self) -> &Model {
        &self.container().model
    }

    pub fn view(&self) -> &View {
        &self.view
    }

    /// Coordinates of the container being edited → world coordinates.
    pub fn context_transform(&self) -> DAffine3 {
        self.state.scene.transform(&self.state.path)
    }

    /// Bounds of the whole drawing, in world coordinates.
    pub fn bounds(&self) -> Option<(DVec3, DVec3)> {
        self.state.scene.bounds()
    }

    pub fn instance_name(&self, id: InstanceId) -> Option<&str> {
        let instance = self.container().instance(id)?;
        self.state.scene.definition(instance.definition).map(|d| d.name.as_str())
    }

    pub fn selection(&self) -> &Selection {
        &self.selection
    }

    pub fn selection_mut(&mut self) -> &mut Selection {
        self.version += 1;
        &mut self.selection
    }

    /// Changes whenever the drawing, the selection or the editing context changes.
    pub fn version(&self) -> u64 {
        self.version
    }

    /// Changes whenever the drawing changes, including undo and redo.
    pub fn model_version(&self) -> u64 {
        self.model_version
    }

    /// Change the loose geometry being edited, as one undoable step.
    pub fn edit<R>(&mut self, change: impl FnOnce(&mut Model) -> R) -> R {
        self.edit_container(|container| change(&mut container.model))
    }

    /// Change the container being edited (geometry and instances), as one undoable step.
    pub fn edit_container<R>(&mut self, change: impl FnOnce(&mut Container) -> R) -> R {
        self.edit_scene(|scene, path| change(scene.container_mut(path).expect("the editing path stays valid")))
    }

    /// Change the scene (e.g. adding definitions), as one undoable step.
    pub fn edit_scene<R>(&mut self, change: impl FnOnce(&mut Scene, &[InstanceId]) -> R) -> R {
        self.undo.push(self.state.clone());
        if self.undo.len() > MAX_UNDO {
            self.undo.remove(0);
        }
        self.redo.clear();
        let result = change(&mut self.state.scene, &self.state.path);
        self.changed();
        result
    }

    pub fn undo(&mut self) -> bool {
        let Some(previous) = self.undo.pop() else { return false };
        self.redo.push(std::mem::replace(&mut self.state, previous));
        self.changed();
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(next) = self.redo.pop() else { return false };
        self.undo.push(std::mem::replace(&mut self.state, next));
        self.changed();
        true
    }

    pub fn erase_selection(&mut self) -> bool {
        if self.selection.is_empty() {
            return false;
        }
        let doomed: Vec<Entity> = self.selection.iter().collect();
        self.edit_container(|container| {
            for &entity in &doomed {
                if let Entity::Instance(id) = entity {
                    container.remove_instance(id);
                }
            }
            container.model.erase(doomed);
        });
        true
    }

    pub fn select_all(&mut self) {
        let container = self.container();
        let all: Vec<Entity> =
            container.model.entities().chain(container.instances().map(|(id, _)| Entity::Instance(id))).collect();
        let selection = self.selection_mut();
        selection.clear();
        selection.extend(all);
    }

    /// Push/pull a face of the container being edited, as one undoable step. When
    /// `auto_group` is set and the face was free-standing, the new solid becomes a group, so
    /// every board is a separate part (SketchUp best practice, and what cut lists need).
    /// Returns the new group, if any.
    pub fn push_pull(&mut self, face: FaceId, distance: f64, auto_group: bool) -> Result<Option<InstanceId>, PushPullError> {
        let model = self.model();
        let free = model.face_edges(face).into_iter().all(|e| model.edge_faces(e).len() <= 1);
        let mut trial = model.clone();
        trial.push_pull(face, distance)?;
        Ok(self.edit_scene(|scene, path| {
            let container = scene.container_mut(path).expect("the editing path stays valid");
            container.model = trial;
            if !(free && auto_group) {
                return None;
            }
            let solid: Vec<Entity> = container.model.connected(Entity::Face(face)).into_iter().collect();
            scene.group(path, &solid, DefinitionKind::Group)
        }))
    }

    /// Move or rotate `entities` of the container being edited by `transforms[0]`, or with
    /// `copy`, place one copy per transform (arrays). Loose geometry stretches connected faces;
    /// groups and components move whole, and copies of them are new instances of the same
    /// definition. One undoable step; on error nothing changes. Returns the copies.
    pub fn transform_entities(
        &mut self,
        entities: &[Entity],
        transforms: &[DAffine3],
        copy: bool,
    ) -> Result<Vec<Entity>, MoveError> {
        let mut trial = self.container().clone();
        let loose: Vec<Entity> = entities.iter().copied().filter(|e| !matches!(e, Entity::Instance(_))).collect();
        let instances: Vec<InstanceId> =
            entities.iter().filter_map(|e| if let Entity::Instance(i) = e { Some(*i) } else { None }).collect();
        let mut copies = Vec::new();
        if copy {
            for t in transforms {
                copies.extend(trial.model.copy_entities_with(&loose, |p| t.transform_point3(p)));
                for &id in &instances {
                    if let Some(&inst) = trial.instance(id) {
                        let new = trial.add_instance(Instance { transform: *t * inst.transform, ..inst });
                        copies.push(Entity::Instance(new));
                    }
                }
            }
        } else if let Some(t) = transforms.first() {
            let vertices = trial.model.entity_vertices(loose);
            trial.model.transform_vertices(&vertices, |p| t.transform_point3(p))?;
            for &id in &instances {
                if let Some(inst) = trial.instance_mut(id) {
                    inst.transform = *t * inst.transform;
                }
            }
        }
        self.edit_container(|container| *container = trial);
        Ok(copies)
    }

    /// Make the selection a group (or component) and select it. `None` if nothing's selected.
    pub fn group_selection(&mut self, kind: DefinitionKind) -> Option<InstanceId> {
        let entities: Vec<Entity> = self.selection.iter().collect();
        if entities.is_empty() {
            return None;
        }
        let instance = self.edit_scene(|scene, path| scene.group(path, &entities, kind))?;
        let selection = self.selection_mut();
        selection.clear();
        selection.extend([Entity::Instance(instance)]);
        Some(instance)
    }

    /// Explode the selected groups and components back into loose geometry, selecting it.
    pub fn explode_selection(&mut self) -> bool {
        let instances: Vec<InstanceId> =
            self.selection.iter().filter_map(|e| if let Entity::Instance(i) = e { Some(i) } else { None }).collect();
        if instances.is_empty() {
            return false;
        }
        let added = self.edit_scene(|scene, path| instances.iter().flat_map(|&i| scene.explode(path, i)).collect::<Vec<_>>());
        let selection = self.selection_mut();
        selection.clear();
        selection.extend(added);
        true
    }

    /// Start editing inside `instance` (a double-click on it). A group shared with copies gets
    /// its own definition first, so only this copy changes.
    pub fn enter(&mut self, instance: InstanceId) -> bool {
        if self.container().instance(instance).is_none() {
            return false;
        }
        let path = self.state.path.clone();
        self.state.scene.make_unique_if_group(&path, instance);
        self.state.path.push(instance);
        self.selection.clear();
        self.context_changed();
        true
    }

    /// Stop editing the innermost group, selecting it. `false` at the top level.
    pub fn exit(&mut self) -> bool {
        let Some(instance) = self.state.path.pop() else { return false };
        self.selection.clear();
        self.selection.extend([Entity::Instance(instance)]);
        self.context_changed();
        true
    }

    fn context_changed(&mut self) {
        self.version += 1;
        self.view = View::build(&self.state.scene, &self.state.path);
    }

    fn changed(&mut self) {
        self.model_version += 1;
        // Undo can restore a path into groups that no longer exist.
        while self.state.scene.container(&self.state.path).is_none() {
            self.state.path.pop();
        }
        let container = self.state.scene.container(&self.state.path).expect("the root always exists");
        self.selection.0.retain(|&e| container.contains(e));
        self.context_changed();
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

    fn profile(doc: &mut Document) -> crate::FaceId {
        doc.edit(|m| m.add_rectangle(DVec3::new(10.0, 0.0, 0.0), DVec3::X * 1.5, DVec3::Y * 3.5, DVec3::Z)).unwrap()
    }

    #[test]
    fn push_pull_auto_groups_a_new_board_in_one_undo_step() {
        let mut doc = Document::default();
        let face = profile(&mut doc);
        let group = doc.push_pull(face, 96.0, true).unwrap().expect("a free face becomes a group");
        assert!(doc.model().is_empty());
        assert_eq!(doc.container().instances().count(), 1);
        assert_eq!(doc.instance_name(group), Some("Group#1"));
        assert_eq!(doc.bounds(), Some((DVec3::new(10.0, 0.0, 0.0), DVec3::new(11.5, 3.5, 96.0))));
        assert_eq!(doc.view().model().faces().count(), 6);
        doc.undo();
        assert_eq!(doc.model().faces().count(), 1);
        assert_eq!(doc.container().instances().count(), 0);
    }

    #[test]
    fn push_pull_without_auto_group_leaves_loose_geometry() {
        let mut doc = Document::default();
        let face = profile(&mut doc);
        assert_eq!(doc.push_pull(face, 96.0, false).unwrap(), None);
        assert_eq!(doc.model().faces().count(), 6);
    }

    #[test]
    fn entering_a_group_edits_inside_and_exiting_selects_it() {
        let mut doc = Document::default();
        let face = profile(&mut doc);
        let group = doc.push_pull(face, 96.0, true).unwrap().unwrap();
        assert!(doc.enter(group));
        assert_eq!(doc.path(), &[group]);
        assert_eq!(doc.model().faces().count(), 6);
        assert_eq!(doc.context_transform().translation, DVec3::new(10.0, 0.0, 0.0));
        // Lengthen the board from inside: push its top up 12".
        let top = doc.model().faces().find(|(_, f)| f.normal.z > 0.5).map(|(id, _)| id).unwrap();
        doc.push_pull(top, 12.0, true).unwrap();
        assert_eq!(doc.bounds().unwrap().1.z, 108.0);
        assert!(doc.exit());
        assert!(doc.path().is_empty());
        assert!(doc.selection().contains(Entity::Instance(group)));
        assert!(!doc.exit());
    }

    #[test]
    fn grouping_the_selection_and_exploding_it_back() {
        let mut doc = Document::default();
        let face = profile(&mut doc);
        doc.push_pull(face, 96.0, false).unwrap();
        doc.select_all();
        let component = doc.group_selection(DefinitionKind::Component).unwrap();
        assert_eq!(doc.instance_name(component), Some("Component#1"));
        assert_eq!(doc.selection().len(), 1);
        assert!(doc.explode_selection());
        assert_eq!(doc.model().faces().count(), 6);
        assert_eq!(doc.container().instances().count(), 0);
    }

    #[test]
    fn deleting_a_selected_group() {
        let mut doc = Document::default();
        let face = profile(&mut doc);
        doc.push_pull(face, 96.0, true).unwrap();
        doc.select_all();
        assert!(doc.erase_selection());
        assert_eq!(doc.bounds(), None);
    }
}
