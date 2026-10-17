//! Groups and components, SketchUp style.
//!
//! A [`Definition`] holds geometry (and nested instances) in its own local coordinates. An
//! [`Instance`] places a definition with a transform. A group is a definition with one instance;
//! a component's definition is shared, so editing it changes every instance. Copying a group
//! shares its definition until one copy is edited, which then gets its own (as in SketchUp).
//!
//! The top level of a drawing and the inside of every definition are both a [`Container`]:
//! loose geometry plus instances. A [`Path`] of instance ids from the root names the container
//! being edited.
use serde::{Deserialize, Serialize};

use std::collections::BTreeSet;

use glam::{DAffine3, DVec3};

use crate::arena::Arena;
use crate::{EdgeId, Entity, FaceId, GuideId, Model, VertexId};

/// An instance within one [`Container`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct InstanceId(pub(crate) u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct DefinitionId(u32);

/// Instances from the root down to the container being edited; empty for the top level.
pub type Path = [InstanceId];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DefinitionKind {
    Group,
    Component,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Definition {
    pub name: String,
    pub kind: DefinitionKind,
    pub container: Container,
    /// Its woodworking properties as a part (grain, cut); older files have the defaults.
    #[serde(default)]
    pub part: crate::part::PartInfo,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Instance {
    pub definition: DefinitionId,
    /// Definition coordinates → container coordinates. Rigid (rotation and translation).
    pub transform: DAffine3,
    /// `None` is SketchUp's "Untagged", which is always visible.
    pub tag: Option<TagId>,
    /// Painted material, shown on every face inside that has none of its own.
    pub material: Option<MaterialId>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct MaterialId(u32);

#[cfg(test)]
impl MaterialId {
    pub(crate) fn for_tests(id: u32) -> Self {
        Self(id)
    }
}

/// What a material is, for cut lists (the categories OpenCutList uses).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MaterialKind {
    /// Boards milled from rough stock; has grain; parts listed with their thickness.
    SolidWood,
    /// Plywood, MDF and such, bought as sheets of a fixed thickness.
    SheetGood,
    /// Ready-milled lumber like a 2x4, bought by section and length.
    DimensionalLumber,
    /// Hinges, screws: counted, not cut.
    Hardware,
    Other,
}

impl MaterialKind {
    pub const ALL: [Self; 5] = [Self::SolidWood, Self::SheetGood, Self::DimensionalLumber, Self::Hardware, Self::Other];

    pub fn name(self) -> &'static str {
        match self {
            Self::SolidWood => "Solid wood",
            Self::SheetGood => "Sheet good",
            Self::DimensionalLumber => "Dimensional lumber",
            Self::Hardware => "Hardware",
            Self::Other => "Other",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Material {
    pub name: String,
    /// sRGB.
    pub color: [u8; 3],
    pub kind: MaterialKind,
    /// Whether the material has a grain direction (solid wood, most plywood) that parts must
    /// follow when laid out for cutting.
    pub grained: bool,
    /// The standard stock it comes in (thicknesses, sheet sizes, lumber sections, lengths); the
    /// cut list picks the right one for each part.
    #[serde(default)]
    pub standard: crate::cutplan::Standard,
    /// From older drawings, before materials listed their standard stock: one stock thickness
    /// and size overrides. Read once to fill in `standard` ([`Scene::upgrade_materials`]).
    #[serde(default, skip_serializing)]
    pub(crate) thickness: Option<f64>,
    #[serde(default, skip_serializing)]
    pub(crate) stock: crate::cutplan::StockOverride,
    /// Solid wood: how much it moves with moisture (from its species); `None` for stable
    /// materials and woods we don't have data for.
    #[serde(default)]
    pub movement: Option<crate::movement::Movement>,
}

impl Material {
    /// A material of `kind` with that type's standard stock (US defaults).
    pub fn new(name: impl Into<String>, color: [u8; 3], kind: MaterialKind, grained: bool) -> Self {
        Self {
            name: name.into(),
            color,
            kind,
            grained,
            standard: crate::cutplan::Standard::for_kind(kind),
            thickness: None,
            stock: Default::default(),
            movement: None,
        }
    }
}

impl Scene {
    /// Fill in the standard stock of materials from older drawings: the type's defaults, plus
    /// the stock thickness and sizes they had set.
    pub(crate) fn upgrade_materials(&mut self) {
        for material in self.materials.iter_mut() {
            // Older drawings: movement data from the species' name.
            if material.movement.is_none() && matches!(material.kind, MaterialKind::SolidWood | MaterialKind::DimensionalLumber) {
                material.movement = crate::movement::species(&material.name);
            }
            if !material.standard.is_empty() {
                continue;
            }
            let mut standard = crate::cutplan::Standard::for_kind(material.kind);
            if let Some(t) = material.thickness.take()
                && matches!(material.kind, MaterialKind::SheetGood | MaterialKind::SolidWood)
                && !standard.thicknesses.iter().any(|s| (s - t).abs() < 1e-9)
            {
                standard.thicknesses.push(t);
                standard.thicknesses.sort_by(f64::total_cmp);
            }
            let old = std::mem::take(&mut material.stock);
            if let Some(sheet) = old.sheet {
                standard.sheets = vec![sheet];
            }
            if let Some(lengths) = old.lengths {
                standard.lengths = lengths;
            }
            if let Some(widths) = old.widths {
                standard.widths = widths;
            }
            material.standard = standard;
        }
    }
}

/// A tag (SketchUp's renamed layers). Tags go on groups and components; hiding a tag hides
/// everything tagged with it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct TagId(u32);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tag {
    pub name: String,
    pub visible: bool,
}

/// Loose geometry plus placed instances.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Container {
    pub model: Model,
    instances: Arena<Instance>,
}

impl Container {
    pub fn instance(&self, id: InstanceId) -> Option<&Instance> {
        self.instances.get(id.0)
    }

    pub fn instance_mut(&mut self, id: InstanceId) -> Option<&mut Instance> {
        self.instances.get_mut(id.0)
    }

    pub fn instances(&self) -> impl Iterator<Item = (InstanceId, &Instance)> {
        self.instances.iter().map(|(i, inst)| (InstanceId(i), inst))
    }

    pub fn add_instance(&mut self, instance: Instance) -> InstanceId {
        InstanceId(self.instances.insert(instance))
    }

    pub fn remove_instance(&mut self, id: InstanceId) -> Option<Instance> {
        self.instances.remove(id.0)
    }

    pub fn contains(&self, entity: Entity) -> bool {
        match entity {
            Entity::Instance(id) => self.instance(id).is_some(),
            other => self.model.contains(other),
        }
    }
}

/// Everything in a drawing: the top-level container and all definitions.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Scene {
    pub root: Container,
    definitions: Arena<Definition>,
    tags: Arena<Tag>,
    materials: Arena<Material>,
    /// Stock sizes and kerf for the cutting diagrams (older files get the US defaults).
    #[serde(default)]
    pub cut_settings: crate::cutplan::Settings,
    /// Joints made between parts, for listing them (older files have none).
    #[serde(default)]
    pub joints: Vec<crate::joinery::Joint>,
    /// Edge profiles made on parts, for listing them.
    #[serde(default)]
    pub profiles: Vec<crate::profile::ProfileRecord>,
    /// The seasonal moisture swing wood movement is checked for, percent (`None`: 4%).
    #[serde(default)]
    pub moisture_swing: Option<f64>,
    /// Counters for default names, like SketchUp's "Group#1", "Component#1".
    next_group: u32,
    next_component: u32,
}

impl Scene {
    pub fn definition(&self, id: DefinitionId) -> Option<&Definition> {
        self.definitions.get(id.0)
    }

    pub fn definition_mut(&mut self, id: DefinitionId) -> Option<&mut Definition> {
        self.definitions.get_mut(id.0)
    }

    pub fn definitions(&self) -> impl Iterator<Item = (DefinitionId, &Definition)> {
        self.definitions.iter().map(|(i, d)| (DefinitionId(i), d))
    }

    pub fn tags(&self) -> impl Iterator<Item = (TagId, &Tag)> {
        self.tags.iter().map(|(i, t)| (TagId(i), t))
    }

    pub fn tag(&self, id: TagId) -> Option<&Tag> {
        self.tags.get(id.0)
    }

    pub fn tag_mut(&mut self, id: TagId) -> Option<&mut Tag> {
        self.tags.get_mut(id.0)
    }

    pub fn add_tag(&mut self, name: impl Into<String>) -> TagId {
        TagId(self.tags.insert(Tag { name: name.into(), visible: true }))
    }

    /// Delete a tag; everything that had it becomes Untagged (SketchUp's default choice).
    pub fn remove_tag(&mut self, id: TagId) -> Option<Tag> {
        let tag = self.tags.remove(id.0)?;
        let containers = std::iter::once(&mut self.root).chain(self.definitions.iter_mut().map(|d| &mut d.container));
        for container in containers {
            for instance in container.instances.iter_mut() {
                if instance.tag == Some(id) {
                    instance.tag = None;
                }
            }
        }
        Some(tag)
    }

    pub fn materials(&self) -> impl Iterator<Item = (MaterialId, &Material)> {
        self.materials.iter().map(|(i, m)| (MaterialId(i), m))
    }

    pub fn material(&self, id: MaterialId) -> Option<&Material> {
        self.materials.get(id.0)
    }

    pub fn material_mut(&mut self, id: MaterialId) -> Option<&mut Material> {
        self.materials.get_mut(id.0)
    }

    pub fn add_material(&mut self, material: Material) -> MaterialId {
        MaterialId(self.materials.insert(material))
    }

    /// Delete a material; everything painted with it goes back to the default.
    pub fn remove_material(&mut self, id: MaterialId) -> Option<Material> {
        let material = self.materials.remove(id.0)?;
        let containers = std::iter::once(&mut self.root).chain(self.definitions.iter_mut().map(|d| &mut d.container));
        for container in containers {
            container.model.forget_material(id);
            for instance in container.instances.iter_mut() {
                if instance.material == Some(id) {
                    instance.material = None;
                }
            }
        }
        Some(material)
    }

    /// Whether an instance is shown: Untagged, or its tag is visible.
    pub fn is_visible(&self, instance: &Instance) -> bool {
        instance.tag.and_then(|t| self.tag(t)).is_none_or(|t| t.visible)
    }

    /// How many instances of `definition` exist anywhere.
    pub fn instance_count(&self, definition: DefinitionId) -> usize {
        std::iter::once(&self.root)
            .chain(self.definitions.iter().map(|(_, d)| &d.container))
            .flat_map(|c| c.instances())
            .filter(|(_, inst)| inst.definition == definition)
            .count()
    }

    /// The container at `path`, if every step exists.
    pub fn container(&self, path: &Path) -> Option<&Container> {
        let mut container = &self.root;
        for &id in path {
            container = &self.definition(container.instance(id)?.definition)?.container;
        }
        Some(container)
    }

    pub fn container_mut(&mut self, path: &Path) -> Option<&mut Container> {
        let mut definition = None;
        let mut container = &self.root;
        for &id in path {
            let def = container.instance(id)?.definition;
            container = &self.definition(def)?.container;
            definition = Some(def);
        }
        match definition {
            None => Some(&mut self.root),
            Some(def) => self.definition_mut(def).map(|d| &mut d.container),
        }
    }

    /// The transform from the container at `path` to the root (world) coordinates.
    pub fn transform(&self, path: &Path) -> DAffine3 {
        let mut transform = DAffine3::IDENTITY;
        let mut container = &self.root;
        for &id in path {
            let Some(instance) = container.instance(id) else { break };
            transform *= instance.transform;
            let Some(definition) = self.definition(instance.definition) else { break };
            container = &definition.container;
        }
        transform
    }

    /// Add `contents` as a new group in the container at `path`, placed by `transform` and
    /// painted with `material`. Returns the new instance.
    pub fn place_group(&mut self, path: &Path, contents: Container, transform: DAffine3, material: Option<MaterialId>) -> Option<InstanceId> {
        self.container(path)?;
        self.next_group += 1;
        let name = format!("Group#{}", self.next_group);
        let definition = DefinitionId(self.definitions.insert(Definition {
            name,
            kind: DefinitionKind::Group,
            container: contents,
            part: Default::default(),
        }));
        Some(self.container_mut(path)?.add_instance(Instance { definition, transform, tag: None, material }))
    }

    /// Group (or componentize) `entities` of the container at `path`: their geometry and
    /// instances move into a new definition, replaced by one instance of it. The definition's
    /// origin is the corner of their bounding box. Returns the new instance, or `None` if
    /// there's nothing to group.
    pub fn group(&mut self, path: &Path, entities: &[Entity], kind: DefinitionKind) -> Option<InstanceId> {
        let container = self.container(path)?;
        let model = &container.model;
        let faces: Vec<FaceId> = entities.iter().filter_map(|e| if let Entity::Face(f) = e { Some(*f) } else { None }).collect();
        let mut edges: BTreeSet<EdgeId> =
            entities.iter().filter_map(|e| if let Entity::Edge(id) = e { Some(*id) } else { None }).collect();
        // Faces take their edges along.
        edges.extend(faces.iter().flat_map(|&f| model.face_edges(f)));
        let instances: Vec<InstanceId> =
            entities.iter().filter_map(|e| if let Entity::Instance(i) = e { Some(*i) } else { None }).collect();

        // The origin: the bounding box's minimum corner, over geometry and instance boxes.
        let mut points: Vec<DVec3> = model.entity_vertices(edges.iter().map(|&e| Entity::Edge(e))).iter().map(|&v| model.vertex(v)).collect();
        for &id in &instances {
            if let Some(inst) = container.instance(id) {
                points.extend(self.instance_box_corners(inst));
            }
        }
        let origin = points.iter().copied().reduce(DVec3::min)?;
        let to_local = DAffine3::from_translation(-origin);

        // Build the definition's contents in local coordinates.
        let mut inside = Container::default();
        for &f in &faces {
            let Some(face) = model.face(f) else { continue };
            let local = |ids: &[VertexId]| -> Vec<DVec3> { ids.iter().map(|&v| model.vertex(v) - origin).collect() };
            let holes: Vec<Vec<DVec3>> = face.holes.iter().map(|h| local(h)).collect();
            if let Some(copy) = inside.model.add_face_with_holes(&local(&face.outer), &holes) {
                inside.model.set_face_material(copy, face.material);
            }
        }
        for &e in &edges {
            let Some(edge) = model.edge(e) else { continue };
            let (a, b) = (inside.model.add_vertex(model.vertex(edge.a) - origin), inside.model.add_vertex(model.vertex(edge.b) - origin));
            inside.model.add_edge(a, b);
        }
        for &id in &instances {
            if let Some(&inst) = container.instance(id) {
                inside.add_instance(Instance { transform: to_local * inst.transform, ..inst });
            }
        }

        let name = match kind {
            DefinitionKind::Group => {
                self.next_group += 1;
                format!("Group#{}", self.next_group)
            }
            DefinitionKind::Component => {
                self.next_component += 1;
                format!("Component#{}", self.next_component)
            }
        };
        let definition = DefinitionId(self.definitions.insert(Definition { name, kind, container: inside, part: Default::default() }));

        // Take the originals out: faces, then edges no remaining face needs.
        let container = self.container_mut(path)?;
        container.model.erase(faces.iter().map(|&f| Entity::Face(f)));
        let still_used: BTreeSet<EdgeId> = container.model.faces().flat_map(|(f, _)| container.model.face_edges(f)).collect();
        let loose: Vec<Entity> = edges.iter().filter(|e| !still_used.contains(e)).map(|&e| Entity::Edge(e)).collect();
        container.model.erase(loose);
        for &id in &instances {
            container.remove_instance(id);
        }
        Some(container.add_instance(Instance {
            definition,
            transform: DAffine3::from_translation(origin),
            tag: None,
            material: None,
        }))
    }

    /// Replace `instance` (in the container at `path`) with its definition's contents, placed
    /// where the instance was. Returns what was added.
    pub fn explode(&mut self, path: &Path, instance: InstanceId) -> Vec<Entity> {
        let Some(container) = self.container(path) else { return Vec::new() };
        let Some(&inst) = container.instance(instance) else { return Vec::new() };
        let Some(definition) = self.definition(inst.definition).cloned() else { return Vec::new() };
        let Some(container) = self.container_mut(path) else { return Vec::new() };
        container.remove_instance(instance);
        let place = |p: DVec3| inst.transform.transform_point3(p);
        let everything: Vec<Entity> = definition.container.model.entities().collect();
        let mut added = container.model.copy_entities_with_from(&definition.container.model, &everything, place);
        for (_, nested) in definition.container.instances() {
            let id = container.add_instance(Instance { transform: inst.transform * nested.transform, ..*nested });
            added.push(Entity::Instance(id));
        }
        added
    }

    /// A group's definition shared with copies gets its own before editing, so the edit
    /// changes only this copy (SketchUp's behavior). Components stay shared.
    /// Turn a group into a component or back, in place (no wrapper around it). A definition
    /// shared with other copies gets its own first. A default name (`Group#3`) becomes the next
    /// default of the new kind; a name the user gave is kept.
    pub fn convert_instance(&mut self, path: &Path, instance: InstanceId, kind: DefinitionKind) -> bool {
        let Some(definition) = self.container(path).and_then(|c| c.instance(instance)).map(|i| i.definition) else { return false };
        let Some(def) = self.definition(definition) else { return false };
        if def.kind == kind {
            return false;
        }
        let definition = if self.instance_count(definition) > 1 {
            let new = DefinitionId(self.definitions.insert(def.clone()));
            if let Some(inst) = self.container_mut(path).and_then(|c| c.instance_mut(instance)) {
                inst.definition = new;
            }
            new
        } else {
            definition
        };
        let default_name = |name: &str| {
            ["Group#", "Component#"].iter().any(|p| name.strip_prefix(p).is_some_and(|n| n.parse::<u32>().is_ok()))
        };
        let rename = self.definition(definition).is_some_and(|d| default_name(&d.name));
        let name = match kind {
            DefinitionKind::Group if rename => {
                self.next_group += 1;
                Some(format!("Group#{}", self.next_group))
            }
            DefinitionKind::Component if rename => {
                self.next_component += 1;
                Some(format!("Component#{}", self.next_component))
            }
            _ => None,
        };
        if let Some(def) = self.definition_mut(definition) {
            def.kind = kind;
            if let Some(name) = name {
                def.name = name;
            }
        }
        true
    }

    pub fn make_unique_if_group(&mut self, path: &Path, instance: InstanceId) {
        let Some(container) = self.container(path) else { return };
        let Some(inst) = container.instance(instance) else { return };
        let definition = inst.definition;
        let Some(def) = self.definition(definition) else { return };
        if def.kind != DefinitionKind::Group || self.instance_count(definition) < 2 {
            return;
        }
        let copy = def.clone();
        let new = DefinitionId(self.definitions.insert(copy));
        if let Some(inst) = self.container_mut(path).and_then(|c| c.instance_mut(instance)) {
            inst.definition = new;
        }
    }

    /// The eight corners of an instance's bounding box, in its container's coordinates.
    pub fn instance_box_corners(&self, instance: &Instance) -> Vec<DVec3> {
        let Some((lo, hi)) = self.definition_bounds(instance.definition) else { return Vec::new() };
        (0..8)
            .map(|i| {
                let corner = DVec3::new(
                    if i & 1 == 0 { lo.x } else { hi.x },
                    if i & 2 == 0 { lo.y } else { hi.y },
                    if i & 4 == 0 { lo.z } else { hi.z },
                );
                instance.transform.transform_point3(corner)
            })
            .collect()
    }

    /// A definition's bounding box in its own coordinates, nested instances included.
    pub fn definition_bounds(&self, definition: DefinitionId) -> Option<(DVec3, DVec3)> {
        let def = self.definition(definition)?;
        let mut points: Vec<DVec3> = def.container.model.vertices().map(|(_, p)| p).collect();
        for (_, inst) in def.container.instances() {
            points.extend(self.instance_box_corners(inst));
        }
        let lo = points.iter().copied().reduce(DVec3::min)?;
        let hi = points.iter().copied().reduce(DVec3::max)?;
        Some((lo, hi))
    }

    /// Bounds of everything, in world coordinates.
    pub fn bounds(&self) -> Option<(DVec3, DVec3)> {
        let mut points: Vec<DVec3> = self.root.model.vertices().map(|(_, p)| p).collect();
        for (_, inst) in self.root.instances() {
            points.extend(self.instance_box_corners(inst));
        }
        let lo = points.iter().copied().reduce(DVec3::min)?;
        let hi = points.iter().copied().reduce(DVec3::max)?;
        Some((lo, hi))
    }
}

/// Who owns a piece of geometry in a [`View`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Owner {
    /// Geometry of the container being edited, with the same ids as its model.
    Active,
    /// Inside this instance of the container being edited.
    Instance(InstanceId),
    /// Outside the container being edited (its parents and their other instances).
    Outside,
}

/// Every piece of geometry in the drawing flattened into one [`Model`], in the coordinates of
/// the container being edited. Picking, snapping and drawing all work on this. The edited
/// container's own geometry keeps its ids, so picks on it can be used to edit it directly.
#[derive(Clone, Debug, Default)]
pub struct View {
    model: Model,
    /// Where each owner's appended geometry starts (first vertex, first edge, first face), who
    /// owns it, and the material it inherits from its enclosing groups.
    segments: Vec<Segment>,
    /// The material the edited container's own geometry inherits (from the open groups).
    active_paint: Option<MaterialId>,
}

#[derive(Clone, Copy, Debug)]
struct Segment {
    start: (VertexId, EdgeId, FaceId, GuideId),
    owner: Owner,
    paint: Option<MaterialId>,
}

impl View {
    pub fn build(scene: &Scene, path: &Path) -> Self {
        let Some(active) = scene.container(path) else { return Self::default() };
        // What each depth of the open-group path inherits: the innermost painted group wins.
        let mut inherited = vec![None; path.len() + 1];
        for depth in 0..path.len() {
            let own = scene.container(&path[..depth]).and_then(|c| c.instance(path[depth])).and_then(|i| i.material);
            inherited[depth + 1] = own.or(inherited[depth]);
        }
        let mut view = Self { model: active.model.clone(), segments: Vec::new(), active_paint: inherited[path.len()] };
        for (id, inst) in active.instances() {
            view.append_instance(scene, inst, &DAffine3::IDENTITY, Owner::Instance(id), view.active_paint);
        }
        // Everything outside, brought into the edited container's coordinates.
        let to_local = scene.transform(path).inverse();
        for depth in 0..path.len() {
            let Some(container) = scene.container(&path[..depth]) else { break };
            let transform = to_local * scene.transform(&path[..depth]);
            view.append_model(&container.model, &transform, Owner::Outside, inherited[depth]);
            for (id, inst) in container.instances() {
                if id != path[depth] {
                    view.append_instance(scene, inst, &transform, Owner::Outside, inherited[depth]);
                }
            }
        }
        view
    }

    fn append_model(&mut self, model: &Model, transform: &DAffine3, owner: Owner, paint: Option<MaterialId>) {
        self.segments.push(Segment { start: self.model.next_ids(), owner, paint });
        self.model.append(model, transform);
    }

    /// Hidden instances (tag turned off) are left out entirely: not drawn, picked or snapped to.
    fn append_instance(
        &mut self,
        scene: &Scene,
        instance: &Instance,
        parent: &DAffine3,
        owner: Owner,
        inherited: Option<MaterialId>,
    ) {
        if !scene.is_visible(instance) {
            return;
        }
        let Some(definition) = scene.definition(instance.definition) else { return };
        let transform = *parent * instance.transform;
        let paint = instance.material.or(inherited);
        self.append_model(&definition.container.model, &transform, owner, paint);
        for (_, nested) in definition.container.instances() {
            self.append_instance(scene, nested, &transform, owner, paint);
        }
    }

    pub fn model(&self) -> &Model {
        &self.model
    }

    fn segment_where(&self, is_past: impl Fn(&(VertexId, EdgeId, FaceId, GuideId)) -> bool) -> Option<&Segment> {
        // Segments are appended in id order, so the ones an id is past form a prefix.
        let past = self.segments.partition_point(|s| is_past(&s.start));
        past.checked_sub(1).map(|i| &self.segments[i])
    }

    fn owner_where(&self, is_past: impl Fn(&(VertexId, EdgeId, FaceId, GuideId)) -> bool) -> Owner {
        self.segment_where(is_past).map_or(Owner::Active, |s| s.owner)
    }

    /// The material a face shows: its own paint, else its enclosing group's, else `None` (the
    /// default material).
    pub fn face_material(&self, id: FaceId) -> Option<MaterialId> {
        let own = self.model.face(id)?.material;
        own.or_else(|| self.segment_where(|start| id >= start.2).map_or(self.active_paint, |s| s.paint))
    }


    pub fn vertex_owner(&self, id: VertexId) -> Owner {
        self.owner_where(|start| id >= start.0)
    }

    pub fn owner(&self, entity: Entity) -> Owner {
        match entity {
            Entity::Edge(id) => self.owner_where(|start| id >= start.1),
            Entity::Face(id) => self.owner_where(|start| id >= start.2),
            Entity::Instance(id) => Owner::Instance(id),
            Entity::Guide(id) => self.owner_where(|start| id >= start.3),
        }
    }

    /// What picking `entity` (of this view) selects in the edited container: the entity itself,
    /// the instance it's inside, or nothing for geometry outside.
    pub fn resolve(&self, entity: Entity) -> Option<Entity> {
        match self.owner(entity) {
            Owner::Active => Some(entity),
            Owner::Instance(id) => Some(Entity::Instance(id)),
            Owner::Outside => None,
        }
    }

    /// The view's vertices belonging to `entities` of the edited container (instances bring all
    /// their geometry).
    pub fn vertices_of(&self, entities: &[Entity]) -> BTreeSet<VertexId> {
        let instances: BTreeSet<InstanceId> =
            entities.iter().filter_map(|e| if let Entity::Instance(i) = e { Some(*i) } else { None }).collect();
        let mut vertices = self.model.entity_vertices(entities.iter().copied().filter(|e| self.owner(*e) == Owner::Active));
        vertices.extend(
            self.model.vertices().map(|(v, _)| v).filter(|&v| matches!(self.vertex_owner(v), Owner::Instance(i) if instances.contains(&i))),
        );
        vertices
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An 8' 2x4 at `at` as loose geometry in `container`.
    fn board(container: &mut Container, at: DVec3) -> Vec<Entity> {
        let face = container.model.add_rectangle(at, DVec3::X * 1.5, DVec3::Y * 3.5, DVec3::Z).unwrap();
        container.model.push_pull(face, 96.0).unwrap();
        container.model.connected(Entity::Face(face)).into_iter().collect()
    }

    #[test]
    fn grouping_moves_geometry_into_a_definition() {
        let mut scene = Scene::default();
        let entities = board(&mut scene.root, DVec3::new(10.0, 5.0, 0.0));
        let group = scene.group(&[], &entities, DefinitionKind::Group).unwrap();
        assert!(scene.root.model.is_empty());
        let inst = *scene.root.instance(group).unwrap();
        assert_eq!(inst.transform.translation, DVec3::new(10.0, 5.0, 0.0));
        let def = scene.definition(inst.definition).unwrap();
        assert_eq!(def.name, "Group#1");
        assert_eq!(def.container.model.faces().count(), 6);
        assert_eq!(scene.definition_bounds(inst.definition), Some((DVec3::ZERO, DVec3::new(1.5, 3.5, 96.0))));
        assert_eq!(scene.bounds(), Some((DVec3::new(10.0, 5.0, 0.0), DVec3::new(11.5, 8.5, 96.0))));
    }

    #[test]
    fn view_flattens_instances_and_resolves_picks_to_them() {
        let mut scene = Scene::default();
        let a = board(&mut scene.root, DVec3::ZERO);
        let group = scene.group(&[], &a, DefinitionKind::Group).unwrap();
        board(&mut scene.root, DVec3::new(20.0, 0.0, 0.0));
        let view = View::build(&scene, &[]);
        assert_eq!(view.model().faces().count(), 12);
        // Loose geometry keeps its ids; the group's resolves to the instance.
        let loose_face = scene.root.model.faces().next().unwrap().0;
        assert_eq!(view.resolve(Entity::Face(loose_face)), Some(Entity::Face(loose_face)));
        let grouped = view.model().faces().map(|(id, _)| Entity::Face(id)).find(|&e| view.owner(e) != Owner::Active).unwrap();
        assert_eq!(view.resolve(grouped), Some(Entity::Instance(group)));
        assert_eq!(view.vertices_of(&[Entity::Instance(group)]).len(), 8);
    }

    #[test]
    fn editing_inside_sees_the_outside_in_local_coordinates() {
        let mut scene = Scene::default();
        let a = board(&mut scene.root, DVec3::new(10.0, 0.0, 0.0));
        let group = scene.group(&[], &a, DefinitionKind::Group).unwrap();
        board(&mut scene.root, DVec3::new(20.0, 0.0, 0.0));
        let view = View::build(&scene, &[group]);
        // The group's own geometry is active, at local coordinates.
        let active: Vec<_> = view.model().vertices().filter(|(v, _)| view.vertex_owner(*v) == Owner::Active).collect();
        assert_eq!(active.len(), 8);
        assert!(active.iter().all(|(_, p)| p.x <= 1.5));
        // The loose board outside is 10" further along, relative to the group.
        let outside_min_x = view
            .model()
            .vertices()
            .filter(|(v, _)| view.vertex_owner(*v) == Owner::Outside)
            .map(|(_, p)| p.x)
            .fold(f64::INFINITY, f64::min);
        assert_eq!(outside_min_x, 10.0);
    }

    #[test]
    fn copying_a_group_shares_until_edited_components_stay_shared() {
        let mut scene = Scene::default();
        let a = board(&mut scene.root, DVec3::ZERO);
        let group = scene.group(&[], &a, DefinitionKind::Group).unwrap();
        let inst = *scene.root.instance(group).unwrap();
        let copy = scene.root.add_instance(Instance { transform: DAffine3::from_translation(DVec3::X * 16.0), ..inst });
        assert_eq!(scene.instance_count(inst.definition), 2);
        scene.make_unique_if_group(&[], copy);
        assert_ne!(scene.root.instance(copy).unwrap().definition, inst.definition);

        let b = board(&mut scene.root, DVec3::new(0.0, 20.0, 0.0));
        let leg = scene.group(&[], &b, DefinitionKind::Component).unwrap();
        let leg_inst = *scene.root.instance(leg).unwrap();
        let other = scene.root.add_instance(Instance { transform: DAffine3::from_translation(DVec3::X * 16.0), ..leg_inst });
        scene.make_unique_if_group(&[], other);
        assert_eq!(scene.root.instance(other).unwrap().definition, leg_inst.definition);
    }

    #[test]
    fn grouping_a_group_nests_it_and_explode_restores() {
        let mut scene = Scene::default();
        let a = board(&mut scene.root, DVec3::new(5.0, 0.0, 0.0));
        let inner = scene.group(&[], &a, DefinitionKind::Group).unwrap();
        let outer = scene.group(&[], &[Entity::Instance(inner)], DefinitionKind::Group).unwrap();
        assert_eq!(scene.root.instances().count(), 1);
        assert_eq!(scene.bounds(), Some((DVec3::new(5.0, 0.0, 0.0), DVec3::new(6.5, 3.5, 96.0))));
        let added = scene.explode(&[], outer);
        assert_eq!(added, vec![Entity::Instance(added_instance(&added))]);
        let exploded = scene.explode(&[], added_instance(&added));
        assert_eq!(exploded.iter().filter(|e| matches!(e, Entity::Face(_))).count(), 6);
        assert_eq!(scene.bounds(), Some((DVec3::new(5.0, 0.0, 0.0), DVec3::new(6.5, 3.5, 96.0))));
    }

    fn added_instance(added: &[Entity]) -> InstanceId {
        added.iter().find_map(|e| if let Entity::Instance(i) = e { Some(*i) } else { None }).unwrap()
    }
}
