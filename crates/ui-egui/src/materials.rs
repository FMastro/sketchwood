//! The Materials panel: pick the Paint Bucket's material and edit materials, including the
//! fields cut lists need (type, grain, stock thickness).

use eframe::egui;
use egui_phosphor::regular as ph;
use sketchcraft_doc::units::{format_length, parse_length};
use sketchcraft_doc::{Document, Entity, MaterialId, MaterialKind};
use sketchcraft_tools::{ToolId, Tools};

#[derive(Default)]
pub struct MaterialsPanel {
    /// The material being renamed and the text typed so far.
    renaming: Option<(MaterialId, String)>,
    /// The thickness being typed for the active material.
    thickness: Option<(MaterialId, String)>,
    /// Whether the text box being edited got its initial focus.
    focused: bool,
}

impl MaterialsPanel {
    pub fn show(&mut self, ui: &mut egui::Ui, doc: &mut Document, tools: &mut Tools) {
        ui.horizontal(|ui| {
            ui.heading("Materials");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.menu_button(ph::PLUS, |ui| {
                    for kind in MaterialKind::ALL {
                        if ui.button(kind.name()).clicked() {
                            let id = doc.add_material(kind);
                            activate(tools, Some(id));
                            ui.close();
                        }
                    }
                })
                .response
                .on_hover_text("Add a material");
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
        for (id, material) in materials {
            ui.horizontal(|ui| {
                let mut color = material.color;
                if egui::color_picker::color_edit_button_srgb(ui, &mut color).changed() {
                    doc.edit_material(id, |m| m.color = color);
                }
                match &mut self.renaming {
                    Some((renaming, text)) if *renaming == id => {
                        let edit = ui.add(egui::TextEdit::singleline(text).desired_width(110.0));
                        if !self.focused {
                            edit.request_focus();
                            self.focused = true;
                        }
                        let cancel = ui.input(|i| i.key_pressed(egui::Key::Escape));
                        if cancel || edit.lost_focus() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            let name = text.trim().to_owned();
                            if !cancel && !name.is_empty() {
                                doc.edit_material(id, |m| m.name = name);
                            }
                            edit.surrender_focus();
                            self.renaming = None;
                        }
                    }
                    _ => {
                        let active = tools.active_material == Some(id);
                        let response = ui.selectable_label(active, &material.name).on_hover_text(format!(
                            "{}. Click to paint with it; double-click to rename.",
                            material.kind.name()
                        ));
                        if response.double_clicked() {
                            self.renaming = Some((id, material.name.clone()));
                            self.focused = false;
                        } else if response.clicked() {
                            activate(tools, Some(id));
                        }
                    }
                }
            });
        }

        if let Some(id) = tools.active_material {
            ui.add_space(4.0);
            self.details(ui, doc, tools, id);
        }
    }

    /// Cut-list fields of the active material.
    fn details(&mut self, ui: &mut egui::Ui, doc: &mut Document, tools: &mut Tools, id: MaterialId) {
        let Some(material) = doc.scene().material(id).cloned() else { return };
        egui::Frame::group(ui.style()).show(ui, |ui| {
            egui::Grid::new("material_details").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
                ui.label("Type");
                egui::ComboBox::from_id_salt("material_kind").selected_text(material.kind.name()).show_ui(ui, |ui| {
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
                let editing = self.thickness.as_ref().is_some_and(|(m, _)| *m == id);
                if !editing {
                    self.thickness = Some((id, shown));
                }
                if let Some((_, text)) = &mut self.thickness {
                    let edit = ui.add(egui::TextEdit::singleline(text).hint_text("e.g. 3/4\"").desired_width(80.0));
                    let commit = edit.lost_focus() || (edit.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)));
                    if commit {
                        let thickness = if text.trim().is_empty() { Ok(None) } else { parse_length(text).map(|t| Some(t.abs())) };
                        if let Ok(thickness) = thickness {
                            doc.edit_material(id, |m| m.thickness = thickness);
                        }
                        // Show the stored value (or revert a typo).
                        self.thickness = None;
                        edit.surrender_focus();
                    } else if !edit.has_focus() {
                        // Keep in step with undo and the like when not being edited.
                        self.thickness = None;
                    }
                }
                ui.end_row();
            });
            if ui.button(format!("{} Delete material", ph::TRASH)).on_hover_text("Painted things go back to the default").clicked() {
                doc.delete_material(id);
                tools.active_material = None;
            }
        });
    }
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
