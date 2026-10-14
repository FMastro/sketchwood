//! A model plus its selection and undo history.

use std::collections::BTreeSet;

use glam::{DAffine3, DVec3};

use crate::scene::{
    Container, DefinitionKind, Instance, InstanceId, Material, MaterialId, MaterialKind, Scene, TagId, View,
};
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
    /// The key of the last merging edit, while further edits with it join the same undo step.
    coalescing: Option<String>,
    version: u64,
}

impl Default for Document {
    fn default() -> Self {
        let state = State::default();
        let view = View::build(&state.scene, &state.path);
        Self {
            state,
            selection: Selection::default(),
            undo: Vec::new(),
            redo: Vec::new(),
            view,
            model_version: 0,
            coalescing: None,
            version: 0,
        }
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
        self.coalescing = None;
        self.push_undo();
        let result = change(&mut self.state.scene, &self.state.path);
        self.changed();
        result
    }

    /// Like [`Self::edit_scene`], but consecutive edits with the same `key` (e.g. dragging a
    /// color picker) merge into one undo step.
    pub fn edit_scene_merging<R>(&mut self, key: &str, change: impl FnOnce(&mut Scene, &[InstanceId]) -> R) -> R {
        if self.coalescing.as_deref() != Some(key) || !self.redo.is_empty() {
            self.push_undo();
        }
        self.coalescing = Some(key.to_owned());
        let result = change(&mut self.state.scene, &self.state.path);
        self.changed();
        result
    }

    fn push_undo(&mut self) {
        self.undo.push(self.state.clone());
        if self.undo.len() > MAX_UNDO {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    pub fn undo(&mut self) -> bool {
        self.coalescing = None;
        let Some(previous) = self.undo.pop() else { return false };
        self.redo.push(std::mem::replace(&mut self.state, previous));
        self.changed();
        true
    }

    pub fn redo(&mut self) -> bool {
        self.coalescing = None;
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

    /// Add a tag named like SketchUp's defaults ("Tag", "Tag1", "Tag2", ...).
    pub fn add_tag(&mut self) -> TagId {
        let taken = |name: &str| self.state.scene.tags().any(|(_, t)| t.name == name);
        let name = std::iter::once("Tag".to_owned())
            .chain((1..).map(|i| format!("Tag{i}")))
            .find(|n| !taken(n))
            .expect("some name is free");
        self.edit_scene(|scene, _| scene.add_tag(name))
    }

    pub fn rename_tag(&mut self, tag: TagId, name: &str) {
        let name = name.trim();
        if name.is_empty() || self.state.scene.tag(tag).is_none_or(|t| t.name == name) {
            return;
        }
        self.edit_scene(|scene, _| {
            if let Some(t) = scene.tag_mut(tag) {
                t.name = name.to_owned();
            }
        });
    }

    /// Delete a tag; what had it becomes Untagged.
    pub fn delete_tag(&mut self, tag: TagId) {
        if self.state.scene.tag(tag).is_some() {
            self.edit_scene(|scene, _| scene.remove_tag(tag));
        }
    }

    /// Show or hide everything with this tag.
    pub fn set_tag_visible(&mut self, tag: TagId, visible: bool) {
        if self.state.scene.tag(tag).is_some_and(|t| t.visible != visible) {
            self.edit_scene(|scene, _| {
                if let Some(t) = scene.tag_mut(tag) {
                    t.visible = visible;
                }
            });
        }
    }

    /// The tag shared by every selected group or component: `Some(tag)` if they agree
    /// (`Some(None)` for Untagged), `None` if mixed or nothing's selected.
    pub fn selection_tag(&self) -> Option<Option<TagId>> {
        let container = self.container();
        let mut tags = self.selection.iter().filter_map(|e| match e {
            Entity::Instance(id) => container.instance(id).map(|i| i.tag),
            _ => None,
        });
        let first = tags.next()?;
        tags.all(|t| t == first).then_some(first)
    }

    /// Tag the selected groups and components (loose geometry stays Untagged, as SketchUp
    /// recommends). Returns how many were tagged.
    pub fn tag_selection(&mut self, tag: Option<TagId>) -> usize {
        let instances: Vec<InstanceId> =
            self.selection.iter().filter_map(|e| if let Entity::Instance(i) = e { Some(i) } else { None }).collect();
        if instances.is_empty() {
            return 0;
        }
        self.edit_container(|container| {
            for &id in &instances {
                if let Some(inst) = container.instance_mut(id) {
                    inst.tag = tag;
                }
            }
        });
        instances.len()
    }

    /// Add a material of `kind` with woodworking defaults (name, color, grain, thickness).
    pub fn add_material(&mut self, kind: MaterialKind) -> MaterialId {
        let count = self.state.scene.materials().count();
        // Wood tones first; the palette cycles.
        const PALETTE: [[u8; 3]; 8] = [
            [222, 184, 135], // pine
            [196, 152, 96],  // oak
            [110, 74, 46],   // walnut
            [235, 210, 160], // maple
            [168, 92, 60],   // cherry
            [214, 188, 140], // birch plywood
            [165, 140, 110], // MDF
            [150, 150, 155], // steel
        ];
        let (base, color, grained, thickness) = match kind {
            MaterialKind::SolidWood => ("Wood", PALETTE[count % 5], true, None),
            MaterialKind::SheetGood => ("Plywood", PALETTE[5], true, Some(0.75)),
            MaterialKind::DimensionalLumber => ("Lumber", PALETTE[0], true, Some(1.5)),
            MaterialKind::Hardware => ("Hardware", PALETTE[7], false, None),
            MaterialKind::Other => ("Material", PALETTE[6], false, None),
        };
        let taken = |name: &str| self.state.scene.materials().any(|(_, m)| m.name == name);
        let name = std::iter::once(base.to_owned())
            .chain((2..).map(|i| format!("{base} {i}")))
            .find(|n| !taken(n))
            .expect("some name is free");
        self.edit_scene(|scene, _| scene.add_material(Material { name, color, kind, grained, thickness }))
    }

    /// Change a material. Consecutive edits of the same material merge into one undo step.
    pub fn edit_material(&mut self, id: MaterialId, change: impl FnOnce(&mut Material)) {
        let Some(mut edited) = self.state.scene.material(id).cloned() else { return };
        change(&mut edited);
        if self.state.scene.material(id) == Some(&edited) {
            return;
        }
        self.edit_scene_merging(&format!("material {id:?}"), |scene, _| {
            if let Some(m) = scene.material_mut(id) {
                *m = edited;
            }
        });
    }

    /// Delete a material; everything painted with it goes back to the default.
    pub fn delete_material(&mut self, id: MaterialId) {
        if self.state.scene.material(id).is_some() {
            self.edit_scene(|scene, _| scene.remove_material(id));
        }
    }

    /// Paint faces and groups/components of the container being edited (`None` = default).
    /// Edges can't be painted. Returns how many were painted.
    pub fn paint(&mut self, entities: &[Entity], material: Option<MaterialId>) -> usize {
        let paintable: Vec<Entity> =
            entities.iter().copied().filter(|e| !matches!(e, Entity::Edge(_)) && self.container().contains(*e)).collect();
        if paintable.is_empty() {
            return 0;
        }
        self.edit_container(|container| {
            for &entity in &paintable {
                match entity {
                    Entity::Face(id) => container.model.set_face_material(id, material),
                    Entity::Instance(id) => {
                        if let Some(inst) = container.instance_mut(id) {
                            inst.material = material;
                        }
                    }
                    Entity::Edge(_) => {}
                }
            }
        });
        paintable.len()
    }

    /// The material painted directly on a face or group/component of the edited container.
    pub fn material_of(&self, entity: Entity) -> Option<MaterialId> {
        match entity {
            Entity::Face(id) => self.model().face(id)?.material,
            Entity::Instance(id) => self.container().instance(id)?.material,
            Entity::Edge(_) => None,
        }
    }

    /// The material shared by every selected face and group: `Some(material)` if they agree
    /// (`Some(None)` for the default), `None` if mixed or nothing paintable is selected.
    pub fn selection_material(&self) -> Option<Option<MaterialId>> {
        let mut materials = self.selection.iter().filter(|e| !matches!(e, Entity::Edge(_))).map(|e| self.material_of(e));
        let first = materials.next()?;
        materials.all(|m| m == first).then_some(first)
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
        let scene = &self.state.scene;
        let container = scene.container(&self.state.path).expect("the root always exists");
        // Hidden groups (tag turned off) can't stay selected.
        self.selection.0.retain(|&e| match e {
            Entity::Instance(id) => container.instance(id).is_some_and(|i| scene.is_visible(i)),
            _ => container.contains(e),
        });
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

    /// Two boards, each its own group.
    fn two_groups(doc: &mut Document) -> (InstanceId, InstanceId) {
        let a = doc.edit(|m| m.add_rectangle(DVec3::ZERO, DVec3::X * 1.5, DVec3::Y * 3.5, DVec3::Z)).unwrap();
        let a = doc.push_pull(a, 10.0, true).unwrap().unwrap();
        let b = doc.edit(|m| m.add_rectangle(DVec3::X * 10.0, DVec3::X * 1.5, DVec3::Y * 3.5, DVec3::Z)).unwrap();
        let b = doc.push_pull(b, 10.0, true).unwrap().unwrap();
        (a, b)
    }

    #[test]
    fn hiding_a_tag_hides_its_groups_from_view_and_selection() {
        let mut doc = Document::default();
        let (a, b) = two_groups(&mut doc);
        let legs = doc.add_tag();
        doc.selection_mut().extend([Entity::Instance(a)]);
        assert_eq!(doc.tag_selection(Some(legs)), 1);
        assert_eq!(doc.selection_tag(), Some(Some(legs)));
        assert_eq!(doc.view().model().faces().count(), 12);

        doc.set_tag_visible(legs, false);
        assert_eq!(doc.view().model().faces().count(), 6);
        assert!(doc.selection().is_empty(), "hidden groups drop out of the selection");
        // The other, untagged board is still there.
        assert!(doc.view().resolve(Entity::Face(doc.view().model().faces().next().unwrap().0)) == Some(Entity::Instance(b)));

        doc.undo();
        assert_eq!(doc.view().model().faces().count(), 12);
    }

    #[test]
    fn deleting_a_tag_untags_and_shows_its_groups() {
        let mut doc = Document::default();
        let (a, _) = two_groups(&mut doc);
        let tag = doc.add_tag();
        doc.selection_mut().extend([Entity::Instance(a)]);
        doc.tag_selection(Some(tag));
        doc.set_tag_visible(tag, false);
        doc.delete_tag(tag);
        assert_eq!(doc.scene().tags().count(), 0);
        assert_eq!(doc.container().instance(a).unwrap().tag, None);
        assert_eq!(doc.view().model().faces().count(), 12);
    }

    #[test]
    fn tag_names_follow_sketchup_defaults_and_rename() {
        let mut doc = Document::default();
        let first = doc.add_tag();
        let second = doc.add_tag();
        assert_eq!(doc.scene().tag(first).unwrap().name, "Tag");
        assert_eq!(doc.scene().tag(second).unwrap().name, "Tag1");
        doc.rename_tag(first, "  Legs ");
        assert_eq!(doc.scene().tag(first).unwrap().name, "Legs");
        doc.rename_tag(first, "   ");
        assert_eq!(doc.scene().tag(first).unwrap().name, "Legs");
    }

    #[test]
    fn mixed_selection_has_no_single_tag() {
        let mut doc = Document::default();
        let (a, b) = two_groups(&mut doc);
        let tag = doc.add_tag();
        doc.selection_mut().extend([Entity::Instance(a)]);
        doc.tag_selection(Some(tag));
        doc.selection_mut().extend([Entity::Instance(b)]);
        assert_eq!(doc.selection_tag(), None);
    }

    #[test]
    fn painting_a_group_colors_its_faces_through_the_view() {
        let mut doc = Document::default();
        let (a, b) = two_groups(&mut doc);
        let oak = doc.add_material(MaterialKind::SolidWood);
        assert_eq!(doc.scene().material(oak).unwrap().name, "Wood");
        assert_eq!(doc.paint(&[Entity::Instance(a)], Some(oak)), 1);
        let view = doc.view();
        let painted = view.model().faces().filter(|(id, _)| view.face_material(*id) == Some(oak)).count();
        assert_eq!(painted, 6, "only board a's faces show oak");
        assert_eq!(doc.material_of(Entity::Instance(b)), None);
    }

    #[test]
    fn a_face_painted_inside_overrides_the_group() {
        let mut doc = Document::default();
        let (a, _) = two_groups(&mut doc);
        let wood = doc.add_material(MaterialKind::SolidWood);
        let edge_band = doc.add_material(MaterialKind::Other);
        doc.paint(&[Entity::Instance(a)], Some(wood));
        doc.enter(a);
        let top = doc.model().faces().find(|(_, f)| f.normal.z > 0.5).map(|(id, _)| id).unwrap();
        doc.paint(&[Entity::Face(top)], Some(edge_band));
        let view = doc.view();
        // Inside, the group's faces still inherit wood, except the painted top.
        assert_eq!(view.face_material(top), Some(edge_band));
        let wood_faces = view.model().faces().filter(|(id, _)| view.face_material(*id) == Some(wood)).count();
        assert_eq!(wood_faces, 5);
    }

    #[test]
    fn push_pull_keeps_the_painted_face_on_the_moved_end() {
        let mut doc = Document::default();
        let face = profile(&mut doc);
        let ply = doc.add_material(MaterialKind::SheetGood);
        doc.paint(&[Entity::Face(face)], Some(ply));
        doc.push_pull(face, 0.75, false).unwrap();
        let painted: Vec<_> = doc.model().faces().filter(|(_, f)| f.material == Some(ply)).map(|(_, f)| f.normal).collect();
        // The original face (now the bottom) and the new top keep the paint; walls don't.
        assert_eq!(painted.len(), 2);
        assert_eq!(doc.scene().material(ply).unwrap().thickness, Some(0.75));
    }

    #[test]
    fn deleting_a_material_unpaints_everything() {
        let mut doc = Document::default();
        let (a, _) = two_groups(&mut doc);
        let wood = doc.add_material(MaterialKind::SolidWood);
        doc.paint(&[Entity::Instance(a)], Some(wood));
        doc.delete_material(wood);
        assert_eq!(doc.material_of(Entity::Instance(a)), None);
        assert_eq!(doc.scene().materials().count(), 0);
    }

    #[test]
    fn dragging_a_color_is_one_undo_step() {
        let mut doc = Document::default();
        let wood = doc.add_material(MaterialKind::SolidWood);
        for shade in 0..20u8 {
            doc.edit_material(wood, |m| m.color = [shade, shade, shade]);
        }
        assert_eq!(doc.scene().material(wood).unwrap().color, [19, 19, 19]);
        doc.undo();
        assert_ne!(doc.scene().material(wood).unwrap().color, [0, 0, 0], "back to before the drag");
        assert!(doc.scene().material(wood).is_some(), "the material itself wasn't undone");
        doc.undo();
        assert!(doc.scene().material(wood).is_none());
    }

    #[test]
    fn default_names_count_up() {
        let mut doc = Document::default();
        let a = doc.add_material(MaterialKind::SheetGood);
        let b = doc.add_material(MaterialKind::SheetGood);
        assert_eq!(doc.scene().material(a).unwrap().name, "Plywood");
        assert_eq!(doc.scene().material(b).unwrap().name, "Plywood 2");
    }
}
