//! Reading SketchUp files (`.skp`) from SketchUp 2021 and later, without Trimble's SDK.
//!
//! The format was worked out by examining `.skp` files only (clean room: no SketchUp or SDK code
//! was decompiled or read). A file is a short UTF-16 header naming the version (`{26.2.243}`),
//! then a ZIP archive holding `model.dat` (the drawing), `materials/<name>/material.xml` (colors)
//! and thumbnails. `model.dat` is nested records: a 2-byte tag, a 4-byte length, then the
//! contents (more records, or a value). Numbers are little-endian; lengths are inches.
//!
//! What's read: component definitions (name, geometry, nested instances), the top-level
//! geometry, vertices, edges, faces with holes and their material, instances (definition,
//! placement, material, layer), materials (name and color) and layers (as tags). Not yet:
//! groups (they come in as components), textures, hidden and soft edges, guides, scenes, and
//! files from SketchUp 2020 and older (a different format).

use std::collections::HashMap;
use std::fmt;
use std::io::Read;

use glam::{DAffine3, DVec3};

use crate::{Container, Definition, DefinitionId, DefinitionKind, Instance, Material, MaterialId, MaterialKind, Scene, TagId};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkpError {
    /// Not a SketchUp file at all.
    NotSketchUp,
    /// From SketchUp 2020 or older, saved in the older format.
    TooOld,
    /// It looks like a SketchUp file, but something in it can't be read.
    Damaged(String),
}

impl fmt::Display for SkpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotSketchUp => write!(f, "This isn't a SketchUp file."),
            Self::TooOld => write!(
                f,
                "This SketchUp file is from SketchUp 2020 or older, which WoodSketch can't read yet. Open it in SketchUp 2021 or later and save it again."
            ),
            Self::Damaged(why) => write!(f, "This SketchUp file couldn't be read ({why})."),
        }
    }
}

impl std::error::Error for SkpError {}

/// A SketchUp drawing read into a scene.
#[derive(Debug)]
pub struct Imported {
    pub scene: Scene,
    /// The SketchUp version that saved it, like `26.2.243`.
    pub version: String,
    /// Faces whose outline couldn't be followed, left out.
    pub skipped_faces: usize,
}

/// Read a `.skp` file's bytes.
pub fn read(bytes: &[u8]) -> Result<Imported, SkpError> {
    if !bytes.starts_with(&[0xff, 0xfe, 0xff]) {
        return Err(SkpError::NotSketchUp);
    }
    let version = header_version(bytes);
    let Some(zip_at) = find(bytes, b"PK\x03\x04") else {
        return Err(SkpError::TooOld);
    };
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(&bytes[zip_at..])).map_err(|e| SkpError::Damaged(e.to_string()))?;
    let mut entry = |name: &str| -> Option<Vec<u8>> {
        let mut file = archive.by_name(name).ok()?;
        let mut data = Vec::new();
        file.read_to_end(&mut data).ok()?;
        Some(data)
    };
    let model = entry("model.dat").ok_or_else(|| SkpError::Damaged("no model.dat".into()))?;
    let top = records(&model).next().ok_or_else(|| SkpError::Damaged("empty model.dat".into()))?;
    let mut reader = Reader { scene: Scene::default(), materials: HashMap::new(), tags: HashMap::new(), definitions: HashMap::new(), skipped_faces: 0 };
    reader.materials(top, &mut entry);
    reader.layers(top);
    reader.definitions(top);
    if let Some(entities) = top.child(0x01f6).and_then(|r| r.child(0x1388)) {
        reader.scene.root = reader.container(entities);
    }
    Ok(Imported { scene: reader.scene, version, skipped_faces: reader.skipped_faces })
}

/// The version in the header's braces, like `{26.2.243}` (UTF-16).
fn header_version(bytes: &[u8]) -> String {
    let head = &bytes[..bytes.len().min(256)];
    let text: String = head.as_chunks::<2>().0.iter().map(|&c| u16::from_le_bytes(c)).filter_map(|u| char::from_u32(u.into())).collect();
    text.split_once('{').and_then(|(_, rest)| rest.split_once('}')).map(|(v, _)| v.to_owned()).unwrap_or_default()
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// One record: a tag and its contents.
#[derive(Clone, Copy)]
struct Record<'a> {
    tag: u16,
    body: &'a [u8],
}

/// The records laid end to end in `bytes`; stops at anything that doesn't fit.
fn records(bytes: &[u8]) -> impl Iterator<Item = Record<'_>> {
    let mut at = 0;
    std::iter::from_fn(move || {
        let head = bytes.get(at..at + 6)?;
        let tag = u16::from_le_bytes([head[0], head[1]]);
        let len = u32::from_le_bytes([head[2], head[3], head[4], head[5]]) as usize;
        let body = bytes.get(at + 6..at + 6 + len)?;
        at += 6 + len;
        Some(Record { tag, body })
    })
}

impl<'a> Record<'a> {
    fn children(self) -> impl Iterator<Item = Record<'a>> {
        records(self.body)
    }

    fn child(self, tag: u16) -> Option<Record<'a>> {
        self.children().find(|r| r.tag == tag)
    }

    fn all(self, tag: u16) -> impl Iterator<Item = Record<'a>> {
        self.children().filter(move |r| r.tag == tag)
    }

    /// A little-endian whole number of any width up to 8 bytes (ids, counts, flags).
    fn int(self) -> Option<u64> {
        (!self.body.is_empty() && self.body.len() <= 8).then(|| self.body.iter().rev().fold(0u64, |n, &b| (n << 8) | u64::from(b)))
    }

    fn floats(self) -> Vec<f64> {
        self.body.as_chunks::<8>().0.iter().map(|&c| f64::from_le_bytes(c)).collect()
    }

    fn text(self) -> String {
        String::from_utf8_lossy(self.body).into_owned()
    }

    /// The entity id: in an id record (`05dc` > `05de`), directly or inside the drawing
    /// element's common part (`07d0`).
    fn id(self) -> Option<u64> {
        let common = self.child(0x07d0).unwrap_or(self);
        common.child(0x05dc)?.child(0x05de)?.int()
    }

    /// The material (`07d1`) and layer (`07d2`) every drawing element has.
    fn common(self, tag: u16) -> Option<u64> {
        self.child(0x07d0)?.child(tag)?.int()
    }
}

struct Reader {
    scene: Scene,
    materials: HashMap<u64, MaterialId>,
    tags: HashMap<u64, TagId>,
    definitions: HashMap<u64, DefinitionId>,
    skipped_faces: usize,
}

impl Reader {
    /// Materials (`01f7`): names here, colors in each one's `material.xml`.
    fn materials(&mut self, top: Record<'_>, entry: &mut impl FnMut(&str) -> Option<Vec<u8>>) {
        let Some(list) = top.child(0x01f7).and_then(|r| r.child(0x30d4)).and_then(|r| r.child(0x30d5)) else { return };
        for record in list.all(0x32c8) {
            let (Some(id), Some(name)) = (record.id(), record.child(0x32cc).map(Record::text)) else { continue };
            // Its folder, from the thumbnail's path (`materials/<folder>/thumbnail.jpg`).
            let folder = record
                .child(0x32ce)
                .and_then(|r| r.child(0x2134))
                .map(Record::text)
                .and_then(|p| p.rsplit_once('/').map(|(dir, _)| dir.to_owned()))
                .unwrap_or_else(|| format!("materials/{name}"));
            let color = entry(&format!("{folder}/material.xml")).and_then(|xml| xml_color(&String::from_utf8_lossy(&xml))).unwrap_or([200, 200, 200]);
            let material = material_for(&name, color);
            let existing = self.scene.materials().find(|(_, m)| m.name == material.name).map(|(i, _)| i);
            let mid = existing.unwrap_or_else(|| self.scene.add_material(material));
            self.materials.insert(id, mid);
        }
    }

    /// Layers (`01f8`) become tags; Layer0 is Untagged.
    fn layers(&mut self, top: Record<'_>) {
        let Some(list) = top.child(0x01f8).and_then(|r| r.child(0x3a98)).and_then(|r| r.child(0x3a99)) else { return };
        for record in list.all(0x3c8c) {
            let (Some(id), Some(name)) = (record.id(), record.child(0x3c8d).map(Record::text)) else { continue };
            if name == "Layer0" || name == "Untagged" {
                continue;
            }
            let tag = self.scene.add_tag(name);
            self.tags.insert(id, tag);
        }
    }

    /// Component definitions (`01f9` > `1770` > `1771` > `157c`): every one made first, then
    /// filled, since instances can name any of them.
    fn definitions(&mut self, top: Record<'_>) {
        let Some(list) = top.child(0x01f9).and_then(|r| r.child(0x1770)).and_then(|r| r.child(0x1771)) else { return };
        let found: Vec<(u64, String, Record<'_>)> = list
            .all(0x157c)
            .filter_map(|d| {
                let entities = d.child(0x1388)?;
                Some((entities.id()?, d.child(0x157e).map(Record::text).unwrap_or_default(), entities))
            })
            .collect();
        for (id, name, _) in &found {
            let definition = Definition {
                name: if name.is_empty() { "Component".into() } else { name.clone() },
                kind: DefinitionKind::Component,
                container: Container::default(),
                part: Default::default(),
                template: None,
            };
            let new = self.scene.add_definition(definition);
            self.definitions.insert(*id, new);
        }
        for (id, _, entities) in found {
            let container = self.container(entities);
            if let Some(d) = self.scene.definition_mut(self.definitions[&id]) {
                d.container = container;
            }
        }
    }

    /// One entity list (`1388`): vertices, edges, faces and instances.
    fn container(&mut self, entities: Record<'_>) -> Container {
        let vertices: HashMap<u64, DVec3> = entities
            .child(0x1389)
            .into_iter()
            .flat_map(|r| r.all(0x09c4))
            .filter_map(|v| {
                let p = v.child(0x09c5)?.floats();
                Some((v.id()?, DVec3::new(*p.first()?, *p.get(1)?, *p.get(2)?)))
            })
            .collect();
        let edges: HashMap<u64, (u64, u64)> = entities
            .child(0x138a)
            .into_iter()
            .flat_map(|r| r.all(0x0bb8))
            .filter_map(|e| Some((e.id()?, (e.child(0x0bb9)?.int()?, e.child(0x0bba)?.int()?))))
            .collect();

        let mut container = Container::default();
        let model = &mut container.model;
        let mut in_faces = std::collections::HashSet::new();
        for face in entities.child(0x138b).into_iter().flat_map(|r| r.all(0x0dac)) {
            let loops: Vec<Vec<u64>> = face
                .child(0x0dae)
                .into_iter()
                .flat_map(|r| r.all(0x1194))
                .map(|l| l.child(0x1195).into_iter().flat_map(|c| c.all(0x0fa0)).filter_map(|c| c.child(0x0fa1)?.int()).collect())
                .collect();
            in_faces.extend(loops.iter().flatten().copied());
            let normal = face.child(0x0dad).map(Record::floats).filter(|p| p.len() >= 3).map(|p| DVec3::new(p[0], p[1], p[2]));
            let points: Option<Vec<Vec<DVec3>>> = loops.iter().map(|l| walk(l, &edges, &vertices)).collect();
            let Some(mut points) = points.filter(|p| !p.is_empty()) else {
                self.skipped_faces += 1;
                continue;
            };
            let mut outer = points.remove(0);
            if let Some(n) = normal
                && newell(&outer).dot(n) < 0.0
            {
                outer.reverse();
            }
            let material = face.common(0x07d1).or_else(|| face.child(0x0daf).and_then(Record::int)).and_then(|m| self.materials.get(&m).copied());
            match model.add_face_with_holes(&outer, &points) {
                Some(id) => model.set_face_material(id, material),
                None => self.skipped_faces += 1,
            }
        }
        // Edges on no face (lines drawn alone).
        for (id, (a, b)) in &edges {
            if in_faces.contains(id) {
                continue;
            }
            if let (Some(&a), Some(&b)) = (vertices.get(a), vertices.get(b)) {
                let (a, b) = (model.add_vertex(a), model.add_vertex(b));
                model.add_edge(a, b);
            }
        }
        for instance in entities.child(0x138c).into_iter().flat_map(|r| r.all(0x1964)) {
            let Some(&definition) = instance.child(0x1967).and_then(Record::int).and_then(|d| self.definitions.get(&d)) else { continue };
            let transform = instance.child(0x1966).map(Record::floats).and_then(|m| placement(&m)).unwrap_or(DAffine3::IDENTITY);
            container.add_instance(Instance {
                definition,
                transform,
                tag: instance.common(0x07d2).and_then(|l| self.tags.get(&l).copied()),
                material: instance.common(0x07d1).and_then(|m| self.materials.get(&m).copied()),
            });
        }
        container
    }
}

/// A loop's corners, following its edges end to end (each edge's direction in the loop is
/// worked out from which end the next edge shares).
fn walk(edges_in_loop: &[u64], edges: &HashMap<u64, (u64, u64)>, vertices: &HashMap<u64, DVec3>) -> Option<Vec<DVec3>> {
    let ends: Vec<(u64, u64)> = edges_in_loop.iter().map(|e| edges.get(e).copied()).collect::<Option<_>>()?;
    if ends.len() < 3 {
        return None;
    }
    let (a, b) = ends[0];
    let (c, d) = ends[1];
    // Start at the first edge's end not shared with the second.
    let mut at = if a == c || a == d { b } else { a };
    let mut corners = Vec::with_capacity(ends.len());
    for &(a, b) in &ends {
        corners.push(*vertices.get(&at)?);
        at = if a == at {
            b
        } else if b == at {
            a
        } else {
            return None;
        };
    }
    Some(corners)
}

fn newell(points: &[DVec3]) -> DVec3 {
    (0..points.len()).fold(DVec3::ZERO, |n, i| n + points[i].cross(points[(i + 1) % points.len()]))
}

/// An instance's placement: 13 numbers, the rotation's three rows, the translation, and a divisor
/// (1 normally). (Read as columns, a beam whose length runs along its own Z lands backwards.)
fn placement(m: &[f64]) -> Option<DAffine3> {
    if m.len() < 12 {
        return None;
    }
    let w = m.get(12).copied().filter(|w| w.abs() > 1e-12).unwrap_or(1.0);
    let v = |i: usize| DVec3::new(m[i], m[i + 1], m[i + 2]) / w;
    let rotation = glam::DMat3::from_cols(v(0), v(3), v(6)).transpose();
    let affine = DAffine3 { matrix3: rotation, translation: v(9) };
    affine.is_finite().then_some(affine)
}

/// `colorRed="15" colorGreen="15" colorBlue="15"` in a `material.xml`.
fn xml_color(xml: &str) -> Option<[u8; 3]> {
    let channel = |name: &str| -> Option<u8> {
        let at = xml.find(&format!("{name}=\""))? + name.len() + 2;
        xml[at..].split('"').next()?.trim().parse().ok()
    };
    Some([channel("colorRed")?, channel("colorGreen")?, channel("colorBlue")?])
}

/// A drawing material for a SketchUp one: the library's when the name names a wood or sheet in it
/// (`Walnut`, `[Wood_Cherry]`, `Plywood 3/4`), else solid wood or a sheet good by its name.
fn material_for(name: &str, color: [u8; 3]) -> Material {
    let plain = name.replace(['_', '[', ']'], " ").to_lowercase();
    let preset = crate::library::LIBRARY
        .iter()
        .filter(|p| plain.contains(&p.name.to_lowercase()))
        .max_by_key(|p| p.name.len());
    if let Some(preset) = preset {
        let mut material = preset.material();
        material.name = name.to_owned();
        material.color = color;
        return material;
    }
    let sheet = ["plywood", "mdf", "hardboard", "melamine", "particle"].iter().any(|s| plain.contains(s));
    if sheet {
        Material::new(name, color, MaterialKind::SheetGood, false)
    } else {
        Material::new(name, color, MaterialKind::SolidWood, true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Records written the way `.skp` files lay them out.
    fn rec(tag: u16, body: &[u8]) -> Vec<u8> {
        let mut out = tag.to_le_bytes().to_vec();
        out.extend((body.len() as u32).to_le_bytes());
        out.extend(body);
        out
    }

    fn id(n: u16) -> Vec<u8> {
        rec(0x05dc, &rec(0x05de, &n.to_le_bytes()))
    }

    fn floats(v: &[f64]) -> Vec<u8> {
        v.iter().flat_map(|f| f.to_le_bytes()).collect()
    }

    /// A 2x1 rectangle on the ground, as a definition's entities.
    fn rectangle_entities(entity: u16) -> Vec<u8> {
        let corners = [(10, [0.0, 0.0, 0.0]), (11, [2.0, 0.0, 0.0]), (12, [2.0, 1.0, 0.0]), (13, [0.0, 1.0, 0.0])];
        let vertices: Vec<u8> = corners.iter().flat_map(|(n, p)| rec(0x09c4, &[id(*n), rec(0x09c5, &floats(p))].concat())).collect();
        // Edges listed in a jumbled direction, as in real files.
        let edge_ends = [(20, 10, 11), (21, 12, 11), (22, 12, 13), (23, 10, 13)];
        let edges: Vec<u8> = edge_ends
            .iter()
            .flat_map(|(e, a, b)| rec(0x0bb8, &[rec(0x07d0, &id(*e)), rec(0x0bb9, &(*a as u16).to_le_bytes()), rec(0x0bba, &(*b as u16).to_le_bytes())].concat()))
            .collect();
        let coedges: Vec<u8> = [20u16, 21, 22, 23].iter().flat_map(|e| rec(0x0fa0, &[rec(0x0fa1, &e.to_le_bytes()), rec(0x0fa2, &[0])].concat())).collect();
        let face = rec(
            0x0dac,
            &[rec(0x07d0, &[id(30), rec(0x07d1, &77u16.to_le_bytes())].concat()), rec(0x0dad, &floats(&[0.0, 0.0, -1.0, 0.0])), rec(0x0dae, &rec(0x1194, &rec(0x1195, &coedges)))].concat(),
        );
        rec(0x1388, &[rec(0x07d0, &id(entity)), rec(0x1389, &vertices), rec(0x138a, &edges), rec(0x138b, &face)].concat())
    }

    fn file(model: &[u8], material_xml: &str) -> Vec<u8> {
        let mut zip_bytes = Vec::new();
        {
            let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut zip_bytes));
            let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
            zip.start_file("model.dat", options).unwrap();
            std::io::Write::write_all(&mut zip, model).unwrap();
            zip.start_file("materials/Walnut/material.xml", options).unwrap();
            std::io::Write::write_all(&mut zip, material_xml.as_bytes()).unwrap();
            zip.finish().unwrap();
        }
        let mut out = vec![0xff, 0xfe, 0xff, 0x0a];
        out.extend("{26.2.243}".encode_utf16().flat_map(u16::to_le_bytes));
        out.extend(zip_bytes);
        out
    }

    fn drawing() -> Vec<u8> {
        let material = rec(0x32c8, &[id(77), rec(0x32cc, b"Walnut"), rec(0x32ce, &rec(0x2134, b"materials/Walnut/thumbnail.jpg"))].concat());
        let materials = rec(0x01f7, &rec(0x30d4, &rec(0x30d5, &material)));
        let definition = rec(0x157c, &[rectangle_entities(50), rec(0x157e, b"Shelf")].concat());
        let definitions = rec(0x01f9, &rec(0x1770, &rec(0x1771, &definition)));
        // Two instances: one in place, one turned 90 degrees and moved up 10".
        let instance = |m: &[f64]| rec(0x1964, &[rec(0x07d0, &id(60)), rec(0x1966, &floats(m)), rec(0x1967, &50u16.to_le_bytes())].concat());
        let placed = [
            instance(&[1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]),
            instance(&[0.0, 1.0, 0.0, -1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 10.0, 1.0]),
        ]
        .concat();
        let root = rec(0x01f6, &rec(0x1388, &[rec(0x07d0, &id(1)), rec(0x138c, &placed)].concat()));
        let top = rec(0x01f4, &[materials, definitions, root].concat());
        file(&top, r#"<mat:material name="Walnut" colorRed="94" colorGreen="64" colorBlue="40">"#)
    }

    #[test]
    fn reads_components_faces_materials_and_placement() {
        let imported = read(&drawing()).unwrap();
        assert_eq!(imported.version, "26.2.243");
        assert_eq!(imported.skipped_faces, 0);
        let scene = &imported.scene;
        let instances: Vec<&Instance> = scene.root.instances().map(|(_, i)| i).collect();
        assert_eq!(instances.len(), 2);
        assert_eq!(instances[0].definition, instances[1].definition, "copies of one component");
        assert_eq!(instances[1].transform.translation.z, 10.0);
        let definition = scene.definition(instances[0].definition).unwrap();
        assert_eq!(definition.name, "Shelf");
        let model = &definition.container.model;
        assert_eq!(model.faces().count(), 1);
        let (_, face) = model.faces().next().unwrap();
        assert!(face.normal.z < -0.99, "faces keep their side: {:?}", face.normal);
        let walnut = scene.material(face.material.unwrap()).unwrap();
        assert_eq!((walnut.name.as_str(), walnut.color, walnut.kind), ("Walnut", [94, 64, 40], MaterialKind::SolidWood));
    }

    #[test]
    fn placements_are_rotation_rows_then_translation() {
        // A beam lying along its own Z, laid along red (as in a real file): its own Z becomes +X.
        let m = [0.0, 0.0, 1.0, 0.0, 1.0, 0.0, -1.0, 0.0, 0.0, -7.0, 1.0, 82.0, 1.0];
        let p = placement(&m).unwrap();
        assert!((p.transform_point3(DVec3::new(0.0, 0.0, 95.5)) - DVec3::new(88.5, 1.0, 82.0)).length() < 1e-9);
        assert!((p.transform_vector3(DVec3::X) - DVec3::new(0.0, 0.0, -1.0)).length() < 1e-9);
    }

    #[test]
    fn refuses_what_it_cant_read() {
        assert_eq!(read(b"hello").unwrap_err(), SkpError::NotSketchUp);
        assert_eq!(read(&[0xff, 0xfe, 0xff, 0x0e, 0, 0, 0, 0]).unwrap_err(), SkpError::TooOld);
    }

    #[test]
    fn library_woods_are_recognized_in_sketchup_names() {
        assert_eq!(material_for("[Wood_Cherry_Original]", [1, 2, 3]).kind, MaterialKind::SolidWood);
        assert_eq!(material_for("Baltic Birch Plywood 3/4", [1, 2, 3]).standard.sheets, vec![(60.0, 60.0)]);
        assert_eq!(material_for("Sheet MDF", [1, 2, 3]).kind, MaterialKind::SheetGood);
    }

    /// A real file, when one is given: `WOODSKETCH_SKP=path cargo test -p woodsketch-doc skp -- --nocapture`.
    #[test]
    fn reads_a_real_file() {
        let Ok(path) = std::env::var("WOODSKETCH_SKP") else { return };
        let imported = read(&std::fs::read(&path).unwrap()).unwrap();
        let scene = &imported.scene;
        println!("{path}: SketchUp {}, {} skipped faces", imported.version, imported.skipped_faces);
        for (_, d) in scene.definitions() {
            println!("  {}: {} faces, {} instances", d.name, d.container.model.faces().count(), d.container.instances().count());
        }
        println!("  top level: {} faces, {} instances", scene.root.model.faces().count(), scene.root.instances().count());
        println!("  materials: {:?}", scene.materials().map(|(_, m)| m.name.clone()).collect::<Vec<_>>());
    }
}
