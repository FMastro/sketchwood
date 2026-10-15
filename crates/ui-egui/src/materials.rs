//! The Materials panel: the drawing's materials (click one to paint with it), a library of
//! common shop materials to add, and an editor for the selected material: name, color, and the
//! fields cut lists need (type, grain, thickness, stock sizes).

use eframe::egui;
use egui_phosphor::regular as ph;
use sketchcraft_doc::cutplan::lumber_sizes;
use sketchcraft_doc::library::LIBRARY;
use sketchcraft_doc::units::{format_length, parse_length};
use sketchcraft_doc::{Document, Entity, MaterialId, MaterialKind};
use sketchcraft_tools::{ToolId, Tools};

use crate::cutlist::StockText;

#[derive(Default)]
pub struct MaterialsPanel;

impl MaterialsPanel {
    pub fn show(&mut self, ui: &mut egui::Ui, doc: &mut Document, tools: &mut Tools) {
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.menu_button(ph::PLUS, |ui| add_menu(ui, doc, tools)).response.on_hover_text("Add a material");
            });
        });

        // The default material: painting with it removes paint.
        ui.horizontal(|ui| {
            swatch(ui, [250, 250, 246]);
            if ui.selectable_label(tools.active_material.is_none(), "Default").clicked() {
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
                "Default is selected: painting removes paint. Pick a material above to paint with.",
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

/// The selected material's editor.
fn editor(ui: &mut egui::Ui, doc: &mut Document, tools: &mut Tools, id: MaterialId) {
    let Some(material) = doc.scene().material(id).cloned() else { return };
    let defaults = doc.scene().cut_settings.clone();
    let key = |field: &str| egui::Id::new(("material", id, field.to_owned()));
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
                    if ui.selectable_label(material.kind == kind, kind.name()).clicked() {
                        doc.edit_material(id, |m| m.kind = kind);
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

            ui.label("Thickness");
            let shown = material.thickness.map(format_length).unwrap_or_default();
            if let Some(text) = text_field(ui, key("thickness"), shown, "e.g. 3/4\"", 80.0) {
                let thickness = if text.trim().is_empty() { Ok(None) } else { parse_length(&text).map(|t| Some(t.abs())) };
                if let Ok(thickness) = thickness {
                    doc.edit_material(id, |m| m.thickness = thickness);
                }
            }
            ui.end_row();

            // Stock sizes for the cutting diagrams; blank uses the drawing's (Cut List > Stock).
            let list = |values: &[f64]| values.iter().map(|&v| format_length(v)).collect::<Vec<_>>().join(", ");
            let stock = StockText::from(material.kind, &material.stock);
            let fields: &[(&str, &str, String, String)] = &match material.kind {
                MaterialKind::SheetGood => vec![
                    ("Sheet length", "first", stock.first.clone(), format_length(defaults.sheet.0)),
                    ("Sheet width", "second", stock.second.clone(), format_length(defaults.sheet.1)),
                ],
                MaterialKind::SolidWood => vec![
                    ("Board lengths", "first", stock.first.clone(), list(&defaults.board_lengths)),
                    ("Board widths", "second", stock.second.clone(), list(&defaults.board_widths)),
                ],
                MaterialKind::DimensionalLumber => {
                    vec![("Lengths", "first", stock.first.clone(), list(&defaults.lumber_lengths))]
                }
                _ => vec![],
            };
            for (label, which, value, hint) in fields {
                ui.label(*label).on_hover_text("Stock sizes for the cutting diagrams; blank uses the drawing's");
                if let Some(text) = text_field(ui, key(which), value.clone(), hint, 150.0) {
                    let mut edited = StockText::from(material.kind, &material.stock);
                    if *which == "first" {
                        edited.first = text;
                    } else {
                        edited.second = text;
                    }
                    if let Some(stock) = edited.parse(material.kind) {
                        doc.edit_material(id, |m| m.stock = stock);
                    }
                }
                ui.end_row();
            }
        });

        if material.kind == MaterialKind::DimensionalLumber {
            egui::CollapsingHeader::new("Standard lumber sizes").id_salt(("lumber_sizes", id)).show(ui, |ui| {
                ui.weak("Nominal size and actual size; the cut list picks the smallest that fits each part.");
                egui::Grid::new(("lumber_table", id)).striped(true).num_columns(2).show(ui, |ui| {
                    for (name, t, w) in lumber_sizes() {
                        ui.label(name);
                        ui.label(format!("{} x {}", format_length(t), format_length(w)));
                        ui.end_row();
                    }
                });
            });
        }

        if ui.button(format!("{} Delete material", ph::TRASH)).on_hover_text("Painted things go back to the default").clicked() {
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
