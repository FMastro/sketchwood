//! Copy and paste. [`Document::copy_selection`] takes the selection (loose geometry, groups and
//! components) in world coordinates; [`Document::paste`] puts it into the container being
//! edited, as one undo step.
//!
//! Pasted into the drawing it came from, components stay copies of the same component. Pasted
//! into another drawing, their definitions come along, with their materials (matched by name),
//! tags, joints and edge profiles.

use std::collections::BTreeMap;

use glam::{DAffine3, DVec3};

use crate::{Container, DefinitionId, Document, Entity, Instance, MaterialId, Model, Scene, TagId};

/// What was copied: the source drawing with only the copies at its top level, in world
/// coordinates.
#[derive(Clone)]
pub struct Clip {
    scene: Scene,
    drawing: u64,
    bounds: (DVec3, DVec3),
}

impl Clip {
    /// The copied things' bounding box, in world coordinates when copied.
    pub fn bounds(&self) -> (DVec3, DVec3) {
        self.bounds
    }
}

impl Document {
    /// Copy the selection, or `None` when nothing that can be pasted is selected.
    pub fn copy_selection(&self) -> Option<Clip> {
        let world = self.context_transform();
        let source = self.container();
        let model = &source.model;
        let mut root = Container::default();
        let mut edges = std::collections::BTreeSet::new();
        for entity in self.selection().iter() {
            match entity {
                Entity::Face(f) => {
                    let Some(face) = model.face(f) else { continue };
                    let place = |ids: &[crate::VertexId]| -> Vec<DVec3> { ids.iter().map(|&v| world.transform_point3(model.vertex(v))).collect() };
                    let holes: Vec<Vec<DVec3>> = face.holes.iter().map(|h| place(h)).collect();
                    if let Some(copy) = root.model.add_face_with_holes(&place(&face.outer), &holes) {
                        root.model.set_face_material(copy, face.material);
                    }
                    edges.extend(model.face_edges(f));
                }
                Entity::Edge(e) => {
                    edges.insert(e);
                }
                Entity::Instance(i) => {
                    if let Some(&inst) = source.instance(i) {
                        root.add_instance(Instance { transform: world * inst.transform, ..inst });
                    }
                }
                _ => {}
            }
        }
        for e in edges {
            let Some(edge) = model.edge(e) else { continue };
            let (a, b) = (world.transform_point3(model.vertex(edge.a)), world.transform_point3(model.vertex(edge.b)));
            let (a, b) = (root.model.add_vertex(a), root.model.add_vertex(b));
            root.model.add_edge(a, b);
        }
        let mut scene = self.scene().clone();
        scene.root = root;
        let bounds = scene.bounds()?;
        Some(Clip { scene, drawing: self.drawing(), bounds })
    }

    /// Paste `clip` moved by `offset` (world), into the container being edited, selecting what
    /// was pasted. One undo step. Returns what was pasted.
    pub fn paste(&mut self, clip: &Clip, offset: DVec3) -> Vec<Entity> {
        let place = self.context_transform().inverse() * DAffine3::from_translation(offset);
        let same = clip.drawing == self.drawing() && uses_only(&clip.scene, self.scene());
        let pasted = self.edit_scene(|scene, path| {
            let mut import = Import { from: &clip.scene, definitions: BTreeMap::new(), materials: BTreeMap::new(), tags: BTreeMap::new() };
            let mut instances = Vec::new();
            for (_, &inst) in clip.scene.root.instances() {
                let inst = if same { inst } else { import.instance(scene, inst) };
                instances.push(Instance { transform: place * inst.transform, ..inst });
            }
            let mut model = clip.scene.root.model.clone();
            if !same {
                import.model(scene, &mut model);
            }
            if !same {
                import.records(scene);
            }
            let Some(container) = scene.container_mut(path) else { return Vec::new() };
            let mut pasted: Vec<Entity> = instances.into_iter().map(|i| Entity::Instance(container.add_instance(i))).collect();
            pasted.extend(copy_model(&model, &mut container.model, place));
            pasted
        });
        let selection = self.selection_mut();
        selection.clear();
        selection.extend(pasted.iter().copied());
        pasted
    }
}

/// Whether everything `clip` refers to is still in `scene` (the same drawing, not undone past
/// where it was made).
fn uses_only(clip: &Scene, scene: &Scene) -> bool {
    let instances = clip.root.instances().all(|(_, i)| {
        scene.definition(i.definition).is_some()
            && i.material.is_none_or(|m| scene.material(m).is_some())
            && i.tag.is_none_or(|t| scene.tag(t).is_some())
    });
    let faces = clip.root.model.faces().all(|(_, f)| f.material.is_none_or(|m| scene.material(m).is_some()));
    instances && faces
}

/// Copy `from`'s faces and loose edges into `into`, placed by `place`; returns the new entities.
fn copy_model(from: &Model, into: &mut Model, place: DAffine3) -> Vec<Entity> {
    let mut added = Vec::new();
    let mut face_edges = std::collections::BTreeSet::new();
    for (f, face) in from.faces() {
        let points = |ids: &[crate::VertexId]| -> Vec<DVec3> { ids.iter().map(|&v| place.transform_point3(from.vertex(v))).collect() };
        let holes: Vec<Vec<DVec3>> = face.holes.iter().map(|h| points(h)).collect();
        if let Some(copy) = into.add_face_with_holes(&points(&face.outer), &holes) {
            into.set_face_material(copy, face.material);
            added.push(Entity::Face(copy));
            added.extend(into.face_edges(copy).into_iter().map(Entity::Edge));
        }
        face_edges.extend(from.face_edges(f));
    }
    for (e, edge) in from.edges() {
        if face_edges.contains(&e) {
            continue;
        }
        let (a, b) = (into.add_vertex(place.transform_point3(from.vertex(edge.a))), into.add_vertex(place.transform_point3(from.vertex(edge.b))));
        if let Some(copy) = into.add_edge(a, b) {
            added.push(Entity::Edge(copy));
        }
    }
    added.sort();
    added.dedup();
    added
}

/// Bringing definitions, materials and tags from another drawing, each once.
struct Import<'a> {
    from: &'a Scene,
    definitions: BTreeMap<DefinitionId, DefinitionId>,
    materials: BTreeMap<MaterialId, MaterialId>,
    tags: BTreeMap<TagId, TagId>,
}

impl Import<'_> {
    fn instance(&mut self, scene: &mut Scene, inst: Instance) -> Instance {
        Instance {
            definition: self.definition(scene, inst.definition),
            material: inst.material.map(|m| self.material(scene, m)),
            tag: inst.tag.map(|t| self.tag(scene, t)),
            ..inst
        }
    }

    fn definition(&mut self, scene: &mut Scene, id: DefinitionId) -> DefinitionId {
        if let Some(&new) = self.definitions.get(&id) {
            return new;
        }
        let mut definition = self.from.definition(id).expect("instances name their definitions").clone();
        let inside: Vec<_> = definition.container.instances().map(|(i, &inst)| (i, inst)).collect();
        for (i, inst) in inside {
            let moved = self.instance(scene, inst);
            if let Some(slot) = definition.container.instance_mut(i) {
                *slot = moved;
            }
        }
        self.model(scene, &mut definition.container.model);
        if let Some(made) = &mut definition.template {
            made.wood = made.wood.map(|m| self.material(scene, m));
        }
        let new = scene.add_definition(definition);
        self.definitions.insert(id, new);
        new
    }

    /// The face materials in `model`, brought over.
    fn model(&mut self, scene: &mut Scene, model: &mut Model) {
        let faces: Vec<_> = model.faces().filter_map(|(f, face)| face.material.map(|m| (f, m))).collect();
        for (f, m) in faces {
            let to = self.material(scene, m);
            model.set_face_material(f, Some(to));
        }
    }

    /// The drawing's material of the same name, else a copy.
    fn material(&mut self, scene: &mut Scene, id: MaterialId) -> MaterialId {
        if let Some(&new) = self.materials.get(&id) {
            return new;
        }
        let material = self.from.material(id).expect("copied things name their materials").clone();
        let existing = scene.materials().find(|(_, m)| m.name == material.name).map(|(i, _)| i);
        let new = existing.unwrap_or_else(|| scene.add_material(material));
        self.materials.insert(id, new);
        new
    }

    fn tag(&mut self, scene: &mut Scene, id: TagId) -> TagId {
        if let Some(&new) = self.tags.get(&id) {
            return new;
        }
        let tag = self.from.tag(id).expect("copied things name their tags").clone();
        let existing = scene.tags().find(|(_, t)| t.name == tag.name).map(|(i, _)| i);
        let new = existing.unwrap_or_else(|| {
            let new = scene.add_tag(tag.name.clone());
            if let Some(t) = scene.tag_mut(new) {
                t.visible = tag.visible;
            }
            new
        });
        self.tags.insert(id, new);
        new
    }

    /// The joints, joint bases and profiles of the definitions brought over.
    fn records(&self, scene: &mut Scene) {
        let map = |id: DefinitionId| self.definitions.get(&id).copied();
        for joint in &self.from.joints {
            if let (Some(cut), Some(into)) = (map(joint.cut), map(joint.into)) {
                let mut joint = joint.clone();
                joint.cut = cut;
                joint.into = into;
                for op in &mut joint.ops {
                    op.part = map(op.part).unwrap_or(op.part);
                }
                scene.joints.push(joint);
            }
        }
        for (id, base) in &self.from.joint_bases {
            if let Some(new) = map(*id) {
                let mut base = base.clone();
                for (f, m) in base.faces().filter_map(|(f, face)| face.material.map(|m| (f, m))).collect::<Vec<_>>() {
                    base.set_face_material(f, self.materials.get(&m).copied());
                }
                scene.joint_bases.push((new, base));
            }
        }
        for profile in &self.from.profiles {
            if let Some(part) = map(profile.part) {
                scene.profiles.push(crate::profile::ProfileRecord { part, ..profile.clone() });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::Board;
    use crate::template::{Drawer, Template};

    fn board(doc: &mut Document) -> crate::InstanceId {
        doc.add_board(&Board { origin: DVec3::ZERO, length_dir: DVec3::X, width_dir: DVec3::Y, length: 20.0, width: 4.0, thickness: 0.75 }, None).unwrap()
    }

    #[test]
    fn paste_puts_a_copy_where_asked_and_shares_components() {
        let mut doc = Document::default();
        let part = board(&mut doc);
        doc.selection_mut().extend([Entity::Instance(part)]);
        let part = doc.group_selection(crate::DefinitionKind::Component).unwrap();
        let clip = doc.copy_selection().unwrap();
        assert_eq!(clip.bounds(), (DVec3::ZERO, DVec3::new(20.0, 4.0, 0.75)));
        let pasted = doc.paste(&clip, DVec3::new(0.0, 10.0, 0.0));
        let [Entity::Instance(copy)] = pasted[..] else { panic!("one instance: {pasted:?}") };
        let (a, b) = (doc.container().instance(part).unwrap(), doc.container().instance(copy).unwrap());
        assert_eq!(a.definition, b.definition, "the same component");
        assert_eq!(b.transform.translation.y, 10.0);
        assert!(doc.selection().contains(Entity::Instance(copy)));
        assert!(doc.undo());
        assert_eq!(doc.container().instances().count(), 1, "one undo step");
    }

    #[test]
    fn loose_geometry_pastes_as_geometry() {
        let mut doc = Document::default();
        let face = doc.edit(|m| m.add_face(&[DVec3::ZERO, DVec3::X, DVec3::new(1.0, 1.0, 0.0), DVec3::Y]).unwrap());
        doc.selection_mut().extend([Entity::Face(face)]);
        let clip = doc.copy_selection().unwrap();
        let pasted = doc.paste(&clip, DVec3::new(5.0, 0.0, 0.0));
        assert_eq!(pasted.iter().filter(|e| matches!(e, Entity::Face(_))).count(), 1);
        assert_eq!(pasted.iter().filter(|e| matches!(e, Entity::Edge(_))).count(), 4);
        assert_eq!(doc.model().faces().count(), 2);
    }

    #[test]
    fn pasting_into_another_drawing_brings_materials_and_joints() {
        let mut from = Document::default();
        let walnut = from.add_preset(crate::library::LIBRARY.iter().find(|p| p.name == "Walnut").unwrap());
        let drawer = from.add_template(&Template::Drawer(Drawer::default()), Some(walnut)).unwrap();
        let joints = from.scene().joints.len();
        assert!(joints > 0);
        from.selection_mut().clear();
        from.selection_mut().extend([Entity::Instance(drawer)]);
        let clip = from.copy_selection().unwrap();

        let mut to = Document::default();
        let pasted = to.paste(&clip, DVec3::ZERO);
        let [Entity::Instance(copy)] = pasted[..] else { panic!("the drawer") };
        assert_eq!(to.instance_name(copy), Some("Drawer"));
        assert!(to.scene().materials().any(|(_, m)| m.name == "Walnut"));
        assert_eq!(to.scene().joints.len(), joints);
        assert!(to.template_of(copy).is_some(), "still a template, to change its size");
        let list = crate::cutlist::cut_list(to.scene());
        assert_eq!(list.materials.iter().map(|m| m.parts.len()).sum::<usize>(), crate::cutlist::cut_list(from.scene()).materials.iter().map(|m| m.parts.len()).sum::<usize>());
    }
}
