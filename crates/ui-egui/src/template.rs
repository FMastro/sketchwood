//! The Templates panel in the tray: make a cabinet, drawer, face frame or table from a few
//! sizes, or change the sizes of one that's selected.

use eframe::egui;
use woodsketch_doc::library::LIBRARY;
use woodsketch_doc::template::{CaseJoints, DrawerJoints, FrameJoints, Made, Template};
use woodsketch_doc::units::{format_length, parse_length};
use woodsketch_doc::{Document, InstanceId, MaterialKind};

/// Making a new one: which template (with its sizes) and wood, kept between frames.
#[derive(Clone)]
struct Draft {
    template: Template,
    wood: String,
}

/// The panel for making a new template. `true` when one was added (to zoom out to it).
pub fn new(ui: &mut egui::Ui, doc: &mut Document) -> bool {
    let mut added = false;
    let id = egui::Id::new("template_draft");
    let mut draft = ui.data(|d| d.get_temp::<Draft>(id)).unwrap_or_else(|| Draft { template: Template::all()[0], wood: "Pine".into() });
    ui.horizontal_wrapped(|ui| {
        for template in Template::all() {
            let on = std::mem::discriminant(&template) == std::mem::discriminant(&draft.template);
            if ui.selectable_label(on, template.name()).clicked() && !on {
                draft.template = template;
            }
        }
    });
    ui.add_space(4.0);
    let woods = woods(doc);
    egui::Grid::new("template_wood").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
        ui.label("Wood");
        egui::ComboBox::from_id_salt("template_wood_pick").selected_text(&draft.wood).height(400.0).show_ui(ui, |ui| {
            for name in &woods {
                ui.selectable_value(&mut draft.wood, name.clone(), name);
            }
        });
        ui.end_row();
    });
    let valid = sizes(ui, "new", &mut draft.template);
    let check = draft.template.check();
    if let Err(why) = &check
        && valid
    {
        ui.colored_label(ui.visuals().warn_fg_color, why);
    }
    let make = ui.add_enabled(valid && check.is_ok(), egui::Button::new(format!("Add the {}", draft.template.name().to_lowercase())));
    if make.on_hover_text("Built beside the drawing, as one group; one Undo takes it out").clicked() {
        let wood = wood_id(doc, &draft.wood);
        added = true;
        if let Err(e) = doc.add_template(&draft.template, wood) {
            added = false;
            ui.data_mut(|d| d.insert_temp(egui::Id::new("template_error"), e));
        } else {
            ui.data_mut(|d| d.remove::<String>(egui::Id::new("template_error")));
        }
    }
    if let Some(e) = ui.data(|d| d.get_temp::<String>(egui::Id::new("template_error"))) {
        ui.colored_label(ui.visuals().error_fg_color, e);
    }
    ui.data_mut(|d| d.insert_temp(id, draft));
    added
}

/// The panel for a selected template group: its sizes, to change.
pub fn change(ui: &mut egui::Ui, doc: &mut Document, instance: InstanceId, made: Made) {
    let id = egui::Id::new(("template_change", instance));
    let mut template = ui.data(|d| d.get_temp::<Template>(id)).unwrap_or(made.template);
    let valid = sizes(ui, &format!("{instance:?}"), &mut template);
    let check = template.check();
    if let Err(why) = &check
        && valid
    {
        ui.colored_label(ui.visuals().warn_fg_color, why);
    }
    let changed = template != made.template;
    let button = ui.add_enabled(valid && check.is_ok() && changed, egui::Button::new("Change size"));
    if button.on_hover_text("Builds it again with these sizes, where it is. Changes made inside it by hand are lost.").clicked() {
        match doc.rebuild_template(instance, &template) {
            Ok(_) => ui.data_mut(|d| d.remove::<Template>(id)),
            Err(e) => {
                ui.colored_label(ui.visuals().error_fg_color, e);
            }
        }
        return;
    }
    ui.data_mut(|d| d.insert_temp(id, template));
}

/// The woods a template can be made of: the drawing's, then the library's others.
fn woods(doc: &Document) -> Vec<String> {
    let usable = |kind: MaterialKind| matches!(kind, MaterialKind::SolidWood | MaterialKind::SheetGood);
    let mut names: Vec<String> = doc.scene().materials().filter(|(_, m)| usable(m.kind)).map(|(_, m)| m.name.clone()).collect();
    for preset in LIBRARY.iter().filter(|p| usable(p.kind)) {
        if !names.iter().any(|n| n == preset.name) {
            names.push(preset.name.to_owned());
        }
    }
    names
}

/// The drawing's material named `name`, adding it from the library if it isn't there yet.
fn wood_id(doc: &mut Document, name: &str) -> Option<woodsketch_doc::MaterialId> {
    if let Some((id, _)) = doc.scene().materials().find(|(_, m)| m.name == name) {
        return Some(id);
    }
    LIBRARY.iter().find(|p| p.name == name).map(|p| doc.add_preset(p))
}

/// The template's sizes and joints; `false` while a size doesn't read as one.
fn sizes(ui: &mut egui::Ui, key: &str, template: &mut Template) -> bool {
    let mut valid = true;
    // Each template keeps its own typed text, so switching from Cabinet to Drawer doesn't carry a
    // cabinet's width over.
    let key = &format!("{key} {}", template.name());
    egui::Grid::new(("template_sizes", key, template.name())).num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
        let mut length = |ui: &mut egui::Ui, label: &str, value: &mut f64| {
            valid &= length_field(ui, (key, label), label, value);
        };
        match template {
            Template::Cabinet(c) => {
                length(ui, "Width", &mut c.width);
                length(ui, "Height", &mut c.height);
                length(ui, "Depth", &mut c.depth);
                ui.label("Shelves");
                ui.add(egui::DragValue::new(&mut c.shelves).range(0..=20));
                ui.end_row();
                ui.label("Back");
                ui.checkbox(&mut c.back, "1/4\" plywood");
                ui.end_row();
                pick(ui, "Joints", &mut c.joints, &CaseJoints::ALL, |j| j.name());
            }
            Template::Drawer(d) => {
                length(ui, "Width", &mut d.width);
                length(ui, "Height", &mut d.height);
                length(ui, "Depth", &mut d.depth);
                pick(ui, "Corners", &mut d.joints, &DrawerJoints::ALL, |j| j.name());
            }
            Template::FaceFrame(f) => {
                length(ui, "Width", &mut f.width);
                length(ui, "Height", &mut f.height);
                length(ui, "Stiles", &mut f.stile);
                length(ui, "Rails", &mut f.rail);
                pick(ui, "Joints", &mut f.joints, &FrameJoints::ALL, |j| j.name());
            }
            Template::Table(t) => {
                length(ui, "Width", &mut t.width);
                length(ui, "Depth", &mut t.depth);
                length(ui, "Height", &mut t.height);
                pick(ui, "Joints", &mut t.joints, &FrameJoints::ALL, |j| j.name());
            }
        }
    });
    ui.weak(match template {
        Template::Cabinet(_) => "3/4\" sides, top, bottom and shelves; outside sizes.",
        Template::Drawer(_) => "A 1/2\" box with a 1/4\" plywood bottom in grooves; outside sizes.",
        Template::FaceFrame(_) => "3/4\" stiles full height, rails between them.",
        Template::Table(_) => "1 3/4\" legs, 3/4\" aprons and top (1\" overhang); height to the top.",
    });
    valid
}

/// A length typed as in the Measurements box (`30`, `2'6"`, `760mm`). Keeps what's typed
/// while it doesn't read; `false` then.
fn length_field(ui: &mut egui::Ui, key: (&str, &str), label: &str, value: &mut f64) -> bool {
    let id = egui::Id::new(("template_length", key));
    let mut text = ui.data(|d| d.get_temp::<String>(id)).unwrap_or_else(|| format_length(*value));
    ui.label(label);
    let response = ui.add(egui::TextEdit::singleline(&mut text).desired_width(80.0));
    ui.end_row();
    let parsed = parse_length(&text).ok().filter(|v| *v > 0.0);
    if let Some(v) = parsed {
        *value = v;
    }
    if response.lost_focus() && parsed.is_some() {
        text = format_length(*value);
    }
    ui.data_mut(|d| d.insert_temp(id, text));
    parsed.is_some()
}

fn pick<T: Copy + PartialEq>(ui: &mut egui::Ui, label: &str, value: &mut T, all: &[T], name: impl Fn(T) -> &'static str) {
    ui.label(label);
    egui::ComboBox::from_id_salt(("template_pick", label)).selected_text(name(*value)).show_ui(ui, |ui| {
        for &option in all {
            ui.selectable_value(value, option, name(option));
        }
    });
    ui.end_row();
}
