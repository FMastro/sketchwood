//! The Materials panel: the drawing's materials (click one to paint with it), a library of
//! common shop materials to add, and an editor for the selected material: name, color, and the
//! fields cut lists need (type, grain, thickness, stock sizes).

use eframe::egui;
use egui_phosphor::regular as ph;
use sketchcraft_doc::cutplan::lumber_sizes;
use sketchcraft_doc::library::LIBRARY;
use sketchcraft_doc::cutplan::Standard;
use sketchcraft_doc::units::{format_length, parse_length, parse_lengths};
use sketchcraft_doc::{Document, Entity, MaterialId, MaterialKind};
use sketchcraft_tools::{ToolId, Tools};


#[derive(Default)]
pub struct MaterialsPanel;

impl MaterialsPanel {
    pub fn show(&mut self, ui: &mut egui::Ui, doc: &mut Document, tools: &mut Tools) {
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.menu_button(ph::PLUS, |ui| add_menu(ui, doc, tools)).response.on_hover_text("Add a material");
            });
        });

        // No material: the default look; painting with it removes paint.
        ui.horizontal(|ui| {
            swatch(ui, [250, 250, 246]);
            let none = ui.selectable_label(tools.active_material.is_none(), "No material (default)").on_hover_text(
                "The plain default look. Painting with it removes paint. It isn't a material you can edit: add one with +.",
            );
            if none.clicked() {
                activate(tools, None);
            }
        });

        let materials: Vec<_> = doc.scene().materials().map(|(id, m)| (id, m.clone())).collect();
        for (id, material) in &materials {
            ui.horizontal(|ui| {
                swatch(ui, material.color);
                let active = tools.active_material == Some(*id);
                let response = ui
                    .selectable_label(active, &material.name)
                    .on_hover_text(format!("{}. Click to paint with it and edit it below.", material.kind.name()));
                if response.clicked() {
                    activate(tools, Some(*id));
                }
            });
        }

        if materials.is_empty() {
            ui.add_space(2.0);
            ui.weak(format!("No materials yet. Click {} to add one, like Red Oak or Plywood 3/4\".", ph::PLUS));
        } else if tools.active_material.is_none() && tools.active() == ToolId::PaintBucket {
            ui.add_space(2.0);
            ui.colored_label(
                ui.visuals().warn_fg_color,
                "No material is selected: painting removes paint. Pick a material above to paint with.",
            );
        }

        if let Some(id) = tools.active_material {
            ui.add_space(4.0);
            editor(ui, doc, tools, id);
        }
    }
}

/// The + menu: library materials, or a blank one of a type.
fn add_menu(ui: &mut egui::Ui, doc: &mut Document, tools: &mut Tools) {
    ui.weak("Library");
    for preset in LIBRARY {
        let added = ui
            .horizontal(|ui| {
                swatch(ui, preset.color);
                ui.button(preset.name).clicked()
            })
            .inner;
        if added {
            let id = doc.add_preset(preset);
            activate(tools, Some(id));
            ui.close();
        }
    }
    ui.separator();
    ui.menu_button("New blank material", |ui| {
        for kind in MaterialKind::ALL {
            if ui.button(kind.name()).clicked() {
                let id = doc.add_material(kind);
                activate(tools, Some(id));
                ui.close();
            }
        }
    });
}

/// A text field showing `current`, editable: while it has focus what's typed is kept (even if
/// it doesn't parse yet). Returns the text whenever it changes.
fn text_field(ui: &mut egui::Ui, key: egui::Id, current: String, hint: &str, width: f32) -> Option<String> {
    let mut text = ui.data(|d| d.get_temp::<String>(key)).unwrap_or(current);
    let response = ui.add(egui::TextEdit::singleline(&mut text).hint_text(hint).desired_width(width));
    let changed = response.changed().then(|| text.clone());
    if response.has_focus() {
        ui.data_mut(|d| d.insert_temp(key, text));
    } else {
        ui.data_mut(|d| d.remove::<String>(key));
    }
    changed
}

/// Lengths as typed text: `3/4", 1/2"`.
fn list_text(values: &[f64]) -> String {
    values.iter().map(|&v| format_length(v)).collect::<Vec<_>>().join(", ")
}

/// Comma-separated lengths; `None` if any doesn't read as one.
fn parse_list(text: &str) -> Option<Vec<f64>> {
    let mut values: Vec<f64> = parse_lengths(text).ok()?.into_iter().map(|v| v.map(f64::abs)).collect::<Option<_>>()?;
    values.retain(|v| *v > 0.0);
    values.sort_by(f64::total_cmp);
    values.dedup();
    Some(values)
}

/// Sheet sizes as typed text: `96" x 48", 60" x 60"`.
fn sizes_text(sizes: &[(f64, f64)]) -> String {
    sizes.iter().map(|&(l, w)| format!("{} x {}", format_length(l), format_length(w))).collect::<Vec<_>>().join(", ")
}

fn parse_sizes(text: &str) -> Option<Vec<(f64, f64)>> {
    text.split([',', ';'])
        .filter(|s| !s.trim().is_empty())
        .map(|s| {
            let (l, w) = s.split_once(['x', 'X', '×'])?;
            Some((parse_length(l.trim()).ok()?.abs(), parse_length(w.trim()).ok()?.abs()))
        })
        .collect()
}

/// The selected material's editor.
fn editor(ui: &mut egui::Ui, doc: &mut Document, tools: &mut Tools, id: MaterialId) {
    let Some(material) = doc.scene().material(id).cloned() else { return };
    let key = |field: &str| egui::Id::new(("material", id, field.to_owned()));
    let standard = material.standard.clone();
    egui::Frame::group(ui.style()).show(ui, |ui| {
        egui::Grid::new(("material_editor", id)).num_columns(2).spacing([8.0, 5.0]).show(ui, |ui| {
            ui.label("Name");
            if let Some(name) = text_field(ui, key("name"), material.name.clone(), "Name", 150.0)
                && !name.trim().is_empty()
            {
                doc.edit_material(id, |m| m.name = name.trim().to_owned());
            }
            ui.end_row();

            ui.label("Color");
            let mut color = material.color;
            if egui::color_picker::color_edit_button_srgb(ui, &mut color).changed() {
                doc.edit_material(id, |m| m.color = color);
            }
            ui.end_row();

            ui.label("Type");
            egui::ComboBox::from_id_salt(("material_kind", id)).selected_text(material.kind.name()).show_ui(ui, |ui| {
                for kind in MaterialKind::ALL {
                    if ui.selectable_label(material.kind == kind, kind.name()).clicked() && kind != material.kind {
                        // A different type comes in different stock: start from its standards.
                        doc.edit_material(id, |m| {
                            m.kind = kind;
                            m.standard = Standard::for_kind(kind);
                        });
                    }
                }
            });
            ui.end_row();

            ui.label("Grain");
            let mut grained = material.grained;
            if ui.checkbox(&mut grained, "Has grain").on_hover_text("Parts must follow the grain when cut").changed() {
                doc.edit_material(id, |m| m.grained = grained);
            }
            ui.end_row();

            // The standard stock it comes in; the cut list picks from it for each part.
            let mut list_row = |ui: &mut egui::Ui, label: &str, field: &str, values: &[f64], hint: &str, apply: fn(&mut Standard, Vec<f64>)| {
                ui.label(label);
                if let Some(text) = text_field(ui, key(field), list_text(values), hint, 160.0)
                    && let Some(values) = parse_list(&text)
                {
                    doc.edit_material(id, |m| apply(&mut m.standard, values));
                }
                ui.end_row();
            };
            match material.kind {
                MaterialKind::SheetGood => {
                    list_row(ui, "Thicknesses", "thicknesses", &standard.thicknesses, "1/4, 1/2, 3/4", |s, v| s.thicknesses = v);
                    ui.label("Sheet sizes");
                    if let Some(text) = text_field(ui, key("sheets"), sizes_text(&standard.sheets), "96 x 48, 60 x 60", 160.0)
                        && let Some(sheets) = parse_sizes(&text)
                    {
                        doc.edit_material(id, |m| m.standard.sheets = sheets);
                    }
                    ui.end_row();
                }
                MaterialKind::SolidWood => {
                    list_row(ui, "Thicknesses", "thicknesses", &standard.thicknesses, "3/4, 1, 1 1/4", |s, v| s.thicknesses = v);
                    list_row(ui, "Board lengths", "lengths", &standard.lengths, "72, 96, 120", |s, v| s.lengths = v);
                    list_row(ui, "Board widths", "widths", &standard.widths, "3 1/2, 5 1/2", |s, v| s.widths = v);
                }
                MaterialKind::DimensionalLumber => {
                    list_row(ui, "Lengths", "lengths", &standard.lengths, "96, 120, 144", |s, v| s.lengths = v);
                }
                MaterialKind::Hardware | MaterialKind::Other => {
                    ui.label("");
                    ui.weak("Not cut from stock; the cut list counts the parts.");
                    ui.end_row();
                }
            }
        });

        if material.kind == MaterialKind::DimensionalLumber {
            egui::CollapsingHeader::new(format!("Sections ({} of {})", standard.sections.len(), lumber_sizes().len()))
                .id_salt(("lumber_sections", id))
                .show(ui, |ui| {
                    ui.weak("Nominal and actual sizes it comes in; each part gets the smallest that fits.");
                    egui::Grid::new(("lumber_table", id)).striped(true).num_columns(2).show(ui, |ui| {
                        for (name, t, w) in lumber_sizes() {
                            let mut on = standard.sections.iter().any(|&(st, sw)| (st - t).abs() < 1e-9 && (sw - w).abs() < 1e-9);
                            if ui.checkbox(&mut on, name).changed() {
                                doc.edit_material(id, |m| {
                                    m.standard.sections.retain(|&(st, sw)| (st - t).abs() > 1e-9 || (sw - w).abs() > 1e-9);
                                    if on {
                                        m.standard.sections.push((t, w));
                                        m.standard.sections.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
                                    }
                                });
                            }
                            ui.label(format!("{} x {}", format_length(t), format_length(w)));
                            ui.end_row();
                        }
                    });
                });
        }

        if ui.button(format!("{} Delete material", ph::TRASH)).on_hover_text("Painted things go back to no material").clicked() {
            doc.delete_material(id);
            tools.active_material = None;
        }
    });
}

/// Make `material` the Paint Bucket's and switch to it, as clicking a material does in SketchUp.
fn activate(tools: &mut Tools, material: Option<MaterialId>) {
    tools.active_material = material;
    if tools.active() != ToolId::PaintBucket {
        tools.activate(ToolId::PaintBucket);
    }
}

fn swatch(ui: &mut egui::Ui, color: [u8; 3]) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::hover());
    ui.painter().rect(
        rect,
        3.0,
        egui::Color32::from_rgb(color[0], color[1], color[2]),
        egui::Stroke::new(1.0, egui::Color32::GRAY),
        egui::StrokeKind::Inside,
    );
}

/// Entity Info's material picker for the selection.
pub fn entity_info_material(ui: &mut egui::Ui, doc: &mut Document, tools: &mut Tools) {
    if !doc.selection().iter().any(|e| !matches!(e, Entity::Edge(_))) {
        return;
    }
    let name_of = |doc: &Document, m: Option<MaterialId>| match m.and_then(|m| doc.scene().material(m)) {
        Some(m) => m.name.clone(),
        None => "Default".to_owned(),
    };
    let current = doc.selection_material();
    let shown = match current {
        Some(m) => name_of(doc, m),
        None => "(mixed)".to_owned(),
    };
    let mut choices: Vec<(Option<MaterialId>, String)> = vec![(None, "Default".to_owned())];
    choices.extend(doc.scene().materials().map(|(id, m)| (Some(id), m.name.clone())));
    let mut picked = None;
    ui.horizontal(|ui| {
        ui.label(format!("{} Material", ph::PAINT_BUCKET));
        egui::ComboBox::from_id_salt("entity_info_material").selected_text(shown).show_ui(ui, |ui| {
            for (material, name) in &choices {
                if ui.selectable_label(current == Some(*material), name).clicked() {
                    picked = Some(*material);
                }
            }
        });
    });
    if let Some(material) = picked {
        tools.cancel();
        let selected: Vec<Entity> = doc.selection().iter().collect();
        doc.paint(&selected, material);
    }
}
