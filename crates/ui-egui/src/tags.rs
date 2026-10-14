//! The Tags panel (SketchUp's renamed layers) and the Entity Info tag picker.

use eframe::egui;
use egui_phosphor::regular as ph;
use sketchcraft_doc::{Document, TagId};
use sketchcraft_tools::Tools;

#[derive(Default)]
pub struct TagsPanel {
    /// The tag being renamed, and the text typed so far.
    renaming: Option<(TagId, String)>,
    /// Whether the rename box got its initial focus.
    focused: bool,
}

impl TagsPanel {
    pub fn show(&mut self, ui: &mut egui::Ui, doc: &mut Document) {
        ui.horizontal(|ui| {
            ui.heading("Tags");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(ph::PLUS).on_hover_text("Add a tag").clicked() {
                    let tag = doc.add_tag();
                    let name = doc.scene().tag(tag).map(|t| t.name.clone()).unwrap_or_default();
                    self.renaming = Some((tag, name));
                    self.focused = false;
                }
            });
        });

        // Untagged is always there and always visible, as in SketchUp.
        ui.horizontal(|ui| {
            ui.add_enabled(false, egui::Button::new(ph::EYE).frame(false));
            ui.label("Untagged");
        });

        let mut tags: Vec<(TagId, String, bool)> =
            doc.scene().tags().map(|(id, t)| (id, t.name.clone(), t.visible)).collect();
        tags.sort_by_key(|(_, name, _)| name.to_lowercase());
        for (id, name, visible) in tags {
            ui.horizontal(|ui| {
                let eye = if visible { ph::EYE } else { ph::EYE_SLASH };
                let hint = if visible { "Hide everything with this tag" } else { "Show it again" };
                if ui.add(egui::Button::new(eye).frame(false)).on_hover_text(hint).clicked() {
                    doc.set_tag_visible(id, !visible);
                }

                match &mut self.renaming {
                    Some((renaming, text)) if *renaming == id => {
                        let edit = ui.add(egui::TextEdit::singleline(text).desired_width(110.0));
                        if !self.focused {
                            edit.request_focus();
                            self.focused = true;
                        }
                        let cancel = ui.input(|i| i.key_pressed(egui::Key::Escape));
                        let done = edit.lost_focus() || ui.input(|i| i.key_pressed(egui::Key::Enter));
                        if cancel || done {
                            if !cancel {
                                doc.rename_tag(id, text);
                            }
                            // Hand the keyboard back to the drawing.
                            edit.surrender_focus();
                            self.renaming = None;
                        }
                    }
                    _ => {
                        let label = if visible { egui::RichText::new(&name) } else { egui::RichText::new(&name).weak() };
                        let response = ui.add(egui::Label::new(label).sense(egui::Sense::click()));
                        if response.on_hover_text("Double-click to rename").double_clicked() {
                            self.renaming = Some((id, name.clone()));
                            self.focused = false;
                        }
                    }
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(egui::Button::new(ph::TRASH).frame(false)).on_hover_text("Delete tag (its groups become Untagged)").clicked() {
                        doc.delete_tag(id);
                    }
                });
            });
        }
    }
}

/// SketchUp's Entity Info, for now just the tag of the selected groups and components.
pub fn entity_info(ui: &mut egui::Ui, doc: &mut Document, tools: &mut Tools) {
    ui.heading("Entity Info");
    let count = doc.selection().len();
    if count == 0 {
        ui.weak("Nothing selected.");
        return;
    }
    let instances = doc.selection().iter().filter(|e| matches!(e, sketchcraft_doc::Entity::Instance(_))).count();
    ui.label(match (count, instances) {
        (1, 1) => doc
            .selection()
            .iter()
            .find_map(|e| if let sketchcraft_doc::Entity::Instance(i) = e { doc.instance_name(i) } else { None })
            .unwrap_or("Group")
            .to_owned(),
        (n, 0) => format!("{n} loose entities"),
        (n, i) => format!("{n} selected ({i} groups/components)"),
    });
    if instances == 0 {
        ui.weak("Tags go on groups and components. Group this first (Ctrl+G).");
        return;
    }

    let name_of = |doc: &Document, tag: Option<TagId>| match tag.and_then(|t| doc.scene().tag(t)) {
        Some(t) => t.name.clone(),
        None => "Untagged".to_owned(),
    };
    let current = doc.selection_tag();
    let shown = match current {
        Some(tag) => name_of(doc, tag),
        None => "(mixed)".to_owned(),
    };
    let mut choices: Vec<(Option<TagId>, String)> = vec![(None, "Untagged".to_owned())];
    choices.extend(doc.scene().tags().map(|(id, t)| (Some(id), t.name.clone())));

    let mut picked = None;
    ui.horizontal(|ui| {
        ui.label(format!("{} Tag", ph::TAG));
        egui::ComboBox::from_id_salt("entity_info_tag").selected_text(shown).show_ui(ui, |ui| {
            for (tag, name) in &choices {
                if ui.selectable_label(current == Some(*tag), name).clicked() {
                    picked = Some(*tag);
                }
            }
        });
    });
    if let Some(tag) = picked {
        tools.cancel();
        doc.tag_selection(tag);
    }
}
