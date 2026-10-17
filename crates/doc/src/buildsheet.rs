//! Build sheets: a printable web page with one sheet per part, for the shop. Each has the part's
//! name, cut-list letter and count, material and stock, finished size and grain; three
//! dimensioned views (face, edge, end) drawn to scale, showing its joints and profiles; and the
//! steps in shop order (mill to size, joinery, profiles, sand). It says what to make, not how
//! to set up a machine.

use std::collections::BTreeMap;
use std::fmt::Write;

use glam::{DMat3, DVec2, DVec3};

use crate::cutlist::{CutList, Part};
use crate::cutplan::match_stock;
use crate::joinery::JointKind;
use crate::units::format_length;
use crate::{Container, DefinitionId, Model, Scene};

/// Escape text for HTML element content.
fn esc(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// A part to make: one definition with faces, however many copies.
struct Sheet {
    definition: DefinitionId,
    count: usize,
    material: Option<crate::MaterialId>,
    /// How much each of its own axes is stretched (copies' scale).
    scale: DVec3,
}

fn collect(scene: &Scene, container: &Container, inherited: Option<crate::MaterialId>, linear: DMat3, found: &mut BTreeMap<DefinitionId, Sheet>) {
    for (_, instance) in container.instances() {
        if !scene.is_visible(instance) {
            continue;
        }
        let Some(def) = scene.definition(instance.definition) else { continue };
        let material = instance.material.or(inherited);
        let linear = linear * instance.transform.matrix3;
        if def.container.model.faces().next().is_some() {
            let material = material.or_else(|| def.container.model.faces().find_map(|(_, f)| f.material));
            let scale = DVec3::from_array([0, 1, 2].map(|i| (linear * DVec3::AXES[i]).length()));
            found.entry(instance.definition).or_insert(Sheet { definition: instance.definition, count: 0, material, scale }).count += 1;
        }
        collect(scene, &def.container, material, linear, found);
    }
}

/// The build sheets for the drawing, one per part, in cut-list order.
pub fn to_html(scene: &Scene, list: &CutList, title: &str) -> String {
    let mut found = BTreeMap::new();
    collect(scene, &scene.root, None, DMat3::IDENTITY, &mut found);

    let mut html = String::new();
    let _ = write!(
        html,
        r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8"><title>{title}</title>
<style>
body {{ font: 13px/1.45 system-ui, sans-serif; color: #222; margin: 24px; }}
h1 {{ font-size: 20px; margin: 0 0 4px; }}
h2 {{ font-size: 18px; margin: 0 0 2px; }}
.label {{ display: inline-block; min-width: 22px; padding: 0 6px; margin-right: 6px; border: 2px solid #222; border-radius: 4px; text-align: center; }}
.meta {{ color: #555; margin-bottom: 10px; }}
.sheet {{ margin-bottom: 36px; }}
.facts td {{ padding: 1px 12px 1px 0; vertical-align: top; }}
.facts td:first-child {{ color: #666; }}
ol {{ margin: 6px 0 0 20px; padding: 0; }}
li {{ margin: 2px 0; }}
svg {{ display: block; margin: 10px 0; }}
svg text {{ font: 11px system-ui, sans-serif; fill: #333; }}
@media print {{ .sheet {{ page-break-after: always; }} .noprint {{ display: none; }} body {{ margin: 0; }} }}
</style></head><body>
<h1>{title}</h1>
<p class="meta noprint"><button onclick="window.print()">Print</button> One part per page. Sizes are finished sizes, in inches.</p>
"#,
        title = esc(title)
    );

    // Cut-list order: by the letter of the row each part falls in.
    let mut sheets: Vec<(String, &Sheet, Option<&Part>)> = found
        .values()
        .map(|sheet| {
            let row = row_for(scene, list, sheet);
            (row.map_or_else(|| "~".to_owned(), |p| p.label.clone()), sheet, row)
        })
        .collect();
    sheets.sort_by(|a, b| (a.0.len(), &a.0).cmp(&(b.0.len(), &b.0)));
    if sheets.is_empty() {
        html.push_str("<p>No parts yet. Each group or component is a part.</p>");
    }
    for (label, sheet, row) in sheets {
        write_sheet(&mut html, scene, sheet, &label, row);
    }
    html.push_str("</body></html>\n");
    html
}

/// The cut-list row a part falls in (same material and size).
fn row_for<'a>(scene: &Scene, list: &'a CutList, sheet: &Sheet) -> Option<&'a Part> {
    let def = scene.definition(sheet.definition)?;
    let [length, width, thickness] = crate::part::size(crate::part::extents(def, DMat3::from_diagonal(sheet.scale))?, &def.part);
    let close = |a: f64, b: f64| (a - b).abs() < 1.0 / 64.0;
    list.materials
        .iter()
        .filter(|m| m.material == sheet.material)
        .flat_map(|m| &m.parts)
        .find(|p| close(p.length, length) && close(p.width, width) && close(p.thickness, thickness))
}

fn write_sheet(html: &mut String, scene: &Scene, sheet: &Sheet, label: &str, row: Option<&Part>) {
    let Some(def) = scene.definition(sheet.definition) else { return };
    let Some(extents) = crate::part::extents(def, DMat3::from_diagonal(sheet.scale)) else { return };
    let axes = crate::part::axes(extents, &def.part);
    let [length, width, thickness] = axes.map(|i| extents[i]);
    let material = sheet.material.and_then(|m| scene.material(m));

    let _ = write!(
        html,
        r#"<section class="sheet"><h2><span class="label">{}</span>{}</h2>
<div class="meta">Make {}</div>
<table class="facts">
"#,
        esc(label),
        esc(&def.name),
        if sheet.count == 1 { "1".to_owned() } else { format!("{}, all the same", sheet.count) }
    );
    let size = format!("{} x {} x {}", format_length(length), format_length(width), format_length(thickness));
    let _ = writeln!(html, "<tr><td>Finished size</td><td>{} <span class=\"meta\">(length x width x thickness)</span></td></tr>", esc(&size));
    let stock = material.map_or_else(
        || "No material yet".to_owned(),
        |m| {
            let part = Part { count: 1, length, width, thickness, ..Part::default() };
            match match_stock(&part, m.kind, &m.standard) {
                Ok(stock) => format!("{}, {}", m.name, stock.name),
                Err(_) => m.name.clone(),
            }
        },
    );
    let _ = writeln!(html, "<tr><td>Material</td><td>{}</td></tr>", esc(&stock));
    let cut = match def.part.cut {
        crate::part::Cut::Unknown => String::new(),
        c => format!(", {}", c.name().to_lowercase()),
    };
    let _ = writeln!(html, "<tr><td>Grain</td><td>Along the length{}</td></tr>", esc(&cut));
    if let Some(row) = row
        && row.count > sheet.count
    {
        let _ = writeln!(html, "<tr><td>Cut list</td><td>Row {}: {} parts this size in all</td></tr>", esc(&row.label), row.count);
    }
    html.push_str("</table>\n");

    html.push_str(&views(&def.container.model, sheet.scale, axes, [length, width, thickness]));

    html.push_str("<b>Steps</b><ol>\n");
    // A solid-wood glue-up: its boards come first.
    let glue = material.filter(|m| m.kind == crate::MaterialKind::SolidWood).and_then(|m| {
        let part = Part { count: 1, length, width, thickness, glue_up: def.part.glue_up, ..Part::default() };
        let n = crate::cutplan::glue_up(&part, &m.standard)?;
        let (l, w) = crate::cutplan::glue_up_board(&part, n);
        let boards = format!("{n} boards {} x {} x {}", format_length(thickness), format_length(w), format_length(l));
        // Reinforced seams: what goes in each.
        let seams = def.part.edge_joint.piece(thickness).map(|(piece, spacing)| {
            let per_seam = crate::part::EdgeJoint::per_seam(spacing, l);
            format!("{} {piece}s along each of the {} seams", per_seam, n - 1)
        });
        Some((boards, seams))
    });
    for step in steps(scene, sheet.definition, &stock, &size, glue.as_ref().map(|(b, s)| (b.as_str(), s.as_deref()))) {
        let _ = writeln!(html, "<li>{}</li>", esc(&step));
    }
    html.push_str("</ol></section>\n");
}

/// What to do to the part, in shop order.
fn steps(scene: &Scene, def: DefinitionId, stock: &str, size: &str, glue: Option<(&str, Option<&str>)>) -> Vec<String> {
    let name = |d: DefinitionId| scene.definition(d).map_or_else(|| "another part".to_owned(), |d| d.name.clone());
    let mut steps = match glue {
        Some((boards, seams)) => vec![
            format!("Mill {boards} from {stock}."),
            seams.map_or_else(String::new, |s| format!("Cut for {s}.")),
            format!("Glue them edge to edge, alternating the end grain (rings up, then down); when dry, flatten and cut to {size}, grain along the length."),
        ],
        None => vec![format!("Mill to {size} from {stock}, grain along the length.")],
    };
    steps.retain(|s| !s.is_empty());
    for joint in scene.joints.iter().filter(|j| j.cut == def || j.into == def) {
        let other = name(if joint.cut == def { joint.into } else { joint.cut });
        let kind = joint.kind.name().to_lowercase();
        let step = match (joint.kind, joint.into == def) {
            (JointKind::MortiseAndTenon, true) => format!("Cut the mortise for {other} ({}).", joint.detail),
            (JointKind::MortiseAndTenon, false) => format!("Cut the tenon that goes into {other} ({}).", joint.detail),
            (JointKind::Dowels, _) => format!("Drill for dowels to join {other} ({}).", joint.detail),
            (JointKind::Biscuits, _) => format!("Cut biscuit slots to join {other} ({}).", joint.detail),
            (JointKind::LooseTenons, _) => format!("Cut loose tenon mortises to join {other} ({}).", joint.detail),
            (JointKind::PocketScrews, false) => format!("Drill pocket holes to screw into {other} ({}).", joint.detail),
            (JointKind::PocketScrews, true) => format!("{other} is pocket-screwed to it ({}).", joint.detail),
            (JointKind::BoxJoint | JointKind::HalfLap, _) => format!("Cut the {kind} with {other} ({}).", joint.detail),
            (_, true) => format!("Cut the {kind} for {other} ({}).", joint.detail),
            // The part that fits into another's dado or rabbet: already sized for it.
            (_, false) => format!("Fits into the {kind} in {other} ({}); its size includes that.", joint.detail),
        };
        steps.push(step);
    }
    for profile in scene.profiles.iter().filter(|p| p.part == def) {
        steps.push(format!("Rout the edges: {}.", profile.describe()));
    }
    steps.push("Sand.".to_owned());
    steps
}

/// The three views, to scale: the face (length across, width up), the edge below it (length
/// across, thickness up) and the end beside that (width across, thickness up), each with its
/// overall dimensions.
fn views(model: &Model, scale: DVec3, [l, w, t]: [usize; 3], [length, width, thickness]: [f64; 3]) -> String {
    const ROOM: f64 = 640.0;
    const GAP: f64 = 60.0; // between views, and room for dimensions
    let lo = model.bounds().map_or(DVec3::ZERO, |b| b.0);
    // Points in the part's own scaled axes, from its corner.
    let at = |p: DVec3| (p - lo) * scale;
    let s = (ROOM / (length + width)).min(260.0 / (width + thickness)).min(40.0);
    let segments = drawn_edges(model);

    let face_h = width * s;
    let edge_h = thickness * s;
    let (x0, y_face) = (GAP, GAP);
    let y_edge = y_face + face_h + GAP;
    let x_end = x0 + length * s + GAP;
    let total_w = x_end + width * s + GAP;
    let total_h = y_edge + edge_h + GAP * 0.8;

    let mut svg = format!(r#"<svg width="{total_w:.0}" height="{total_h:.0}" viewBox="0 0 {total_w:.1} {total_h:.1}" xmlns="http://www.w3.org/2000/svg">"#);
    // Each view: which axes go across and up, and where its corner is (SVG y grows down).
    let view = |svg: &mut String, across: usize, up: usize, ox: f64, oy: f64, height: f64| {
        for [a, b] in &segments {
            let (pa, pb) = (at(*a), at(*b));
            let flat = |p: DVec3| DVec2::new(ox + p[across] * s, oy + height - p[up] * s);
            let (fa, fb) = (flat(pa), flat(pb));
            if fa.distance(fb) < 0.3 {
                continue;
            }
            let _ = write!(svg, r##"<line x1="{:.2}" y1="{:.2}" x2="{:.2}" y2="{:.2}" stroke="#222" stroke-width="1"/>"##, fa.x, fa.y, fb.x, fb.y);
        }
    };
    view(&mut svg, l, w, x0, y_face, face_h);
    view(&mut svg, l, t, x0, y_edge, edge_h);
    view(&mut svg, w, t, x_end, y_edge, edge_h);
    // Overall dimensions.
    dimension(&mut svg, DVec2::new(x0, y_face - 16.0), DVec2::new(x0 + length * s, y_face - 16.0), &format_length(length));
    dimension(&mut svg, DVec2::new(x0 - 16.0, y_face + face_h), DVec2::new(x0 - 16.0, y_face), &format_length(width));
    dimension(&mut svg, DVec2::new(x0 - 16.0, y_edge + edge_h), DVec2::new(x0 - 16.0, y_edge), &format_length(thickness));
    dimension(&mut svg, DVec2::new(x_end, y_edge + edge_h + 16.0), DVec2::new(x_end + width * s, y_edge + edge_h + 16.0), &format_length(width));
    for (text, x, y) in [("Face", x0, y_face + face_h + 14.0), ("Edge", x0, y_edge + edge_h + 30.0), ("End", x_end, y_edge - 8.0)] {
        let _ = write!(svg, r##"<text x="{x:.1}" y="{y:.1}" fill="#777">{text}</text>"##);
    }
    svg.push_str("</svg>\n");
    svg
}

/// A dimension line from `a` to `b` with end ticks and its text.
fn dimension(svg: &mut String, a: DVec2, b: DVec2, text: &str) {
    let dir = (b - a).normalize_or_zero();
    let tick = DVec2::new(-dir.y, dir.x) * 4.0;
    let _ = write!(svg, r##"<g stroke="#888" stroke-width="0.8"><line x1="{:.1}" y1="{:.1}" x2="{:.1}" y2="{:.1}"/>"##, a.x, a.y, b.x, b.y);
    for p in [a, b] {
        let _ = write!(svg, r#"<line x1="{:.1}" y1="{:.1}" x2="{:.1}" y2="{:.1}"/>"#, p.x - tick.x, p.y - tick.y, p.x + tick.x, p.y + tick.y);
    }
    svg.push_str("</g>");
    let mid = (a + b) / 2.0;
    if dir.x.abs() >= dir.y.abs() {
        let _ = write!(svg, r#"<text x="{:.1}" y="{:.1}" text-anchor="middle">{}</text>"#, mid.x, mid.y - 4.0, esc(text));
    } else {
        let _ = write!(
            svg,
            r#"<text x="{:.1}" y="{:.1}" text-anchor="middle" transform="rotate(-90 {:.1} {:.1})">{}</text>"#,
            mid.x - 5.0,
            mid.y,
            mid.x - 5.0,
            mid.y,
            esc(text)
        );
    }
}

/// The model's edges worth drawing: not the soft ones inside a curve (faces meeting under 30°).
fn drawn_edges(model: &Model) -> Vec<[DVec3; 2]> {
    let soft = 30f64.to_radians().cos();
    model
        .edges()
        .filter(|(id, _)| {
            let faces = model.edge_faces(*id);
            match faces[..] {
                [a, b] => match (model.face(a), model.face(b)) {
                    (Some(fa), Some(fb)) => fa.normal.dot(fb.normal) < soft,
                    _ => true,
                },
                _ => true,
            }
        })
        .map(|(_, e)| model.edge_points(e))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::Board;
    use crate::library::LIBRARY;
    use crate::{Document, InstanceId};

    fn board(doc: &mut Document, origin: DVec3, length_dir: DVec3, width_dir: DVec3, [l, w, t]: [f64; 3]) -> InstanceId {
        let oak = doc.add_preset(LIBRARY.iter().find(|p| p.name == "Red Oak").unwrap());
        doc.add_board(&Board { origin, length_dir, width_dir, length: l, width: w, thickness: t }, Some(oak)).unwrap()
    }

    #[test]
    fn a_sheet_per_part_with_size_views_and_steps_in_order() {
        let mut doc = Document::default();
        let side = board(&mut doc, DVec3::ZERO, DVec3::Z, DVec3::Y, [30.0, 11.25, 0.75]);
        let shelf = board(&mut doc, DVec3::new(0.0, 0.0, 12.0), DVec3::X, DVec3::Y, [20.0, 11.25, 0.75]);
        doc.rename_instance(side, "Side");
        doc.rename_instance(shelf, "Shelf");
        doc.let_in(shelf, side, 0.25).unwrap();
        let list = crate::cutlist::cut_list(doc.scene());
        let html = to_html(doc.scene(), &list, "Bookcase");
        assert_eq!(html.matches("<section class=\"sheet\">").count(), 2, "one sheet per part");
        assert!(html.contains("Side") && html.contains("Shelf"));
        assert!(html.contains("30\" x 11 1/4\" x 3/4\""), "the side's finished size");
        assert!(html.contains("20 1/4\" x 11 1/4\" x 3/4\""), "the shelf includes its let-in");
        assert!(html.contains("Cut the dado for Shelf (1/4\" deep)."), "the side's step");
        assert!(html.contains("Fits into the dado in Side"), "the shelf's note");
        assert_eq!(html.matches("<svg").count(), 2, "drawings for each");
        let mill = html.find("Mill to").unwrap();
        let dado = html.find("Cut the dado").unwrap();
        let sand = html.rfind("Sand.").unwrap();
        assert!(mill < dado && dado < sand, "shop order");
    }

    #[test]
    fn copies_make_one_sheet_with_a_count_and_profiles_are_steps() {
        let mut doc = Document::default();
        let leg = board(&mut doc, DVec3::ZERO, DVec3::Z, DVec3::X, [28.0, 1.5, 1.5]);
        doc.profile_edges(leg, crate::profile::Profile::Chamfer, 0.125, crate::profile::Edges::Along { axis: 0, corners: [true; 4] }).unwrap();
        doc.selection_mut().extend([crate::Entity::Instance(leg)]);
        doc.group_selection(crate::DefinitionKind::Component);
        let copies: Vec<glam::DAffine3> = (1..4).map(|i| glam::DAffine3::from_translation(DVec3::X * 10.0 * f64::from(i))).collect();
        doc.transform_entities(&[crate::Entity::Instance(leg)], &copies, true).unwrap();
        let list = crate::cutlist::cut_list(doc.scene());
        let html = to_html(doc.scene(), &list, "Table");
        assert_eq!(html.matches("<section class=\"sheet\">").count(), 1);
        assert!(html.contains("Make 4, all the same"));
        assert!(html.contains("Rout the edges: 1/8\" Chamfer, 4 edges."));
    }

    #[test]
    fn a_wide_top_says_to_glue_it_up_first() {
        let mut doc = Document::default();
        board(&mut doc, DVec3::ZERO, DVec3::X, DVec3::Y, [36.0, 20.0, 0.75]);
        let list = crate::cutlist::cut_list(doc.scene());
        let html = to_html(doc.scene(), &list, "Table");
        assert!(html.contains("Mill 3 boards 3/4\" x ~ 6 15/16\" x 37\" from Red Oak"), "{html}");
        let glue = html.find("Glue them edge to edge").unwrap();
        assert!(glue < html.find("Sand.").unwrap());
    }
}
