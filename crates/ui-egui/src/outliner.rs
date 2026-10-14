//! The Outliner panel: where you are (which group is open), the groups and components at this
//! level, grouping commands, and the auto-group setting.

use eframe::egui;
use egui_phosphor::regular as ph;
use sketchcraft_doc::{DefinitionKind, Document, Entity};
use sketchcraft_tools::Tools;

pub fn show(ui: &mut egui::Ui, doc: &mut Document, tools: &mut Tools) {
    ui.heading("Outliner");
    breadcrumb(ui, doc, tools);
    ui.separator();

    let scene = doc.scene();
    let rows: Vec<_> = doc
        .container()
        .instances()
        .filter_map(|(id, inst)| {
            let def = scene.definition(inst.definition)?;
            Some((id, def.name.clone(), def.kind, scene.is_visible(inst)))
        })
        .collect();
    if rows.is_empty() {
        ui.weak("No groups or components here yet.");
    }
    ui.vertical(|ui| {
        for (id, name, kind, visible) in rows {
            let icon = match kind {
                DefinitionKind::Group => ph::CUBE,
                DefinitionKind::Component => ph::PUZZLE_PIECE,
            };
            if !visible {
                // Its tag is hidden: listed, but it can't be picked until shown again.
                ui.weak(format!("{icon}  {name}  (hidden)"));
                continue;
            }
            let selected = doc.selection().contains(Entity::Instance(id));
            let response = ui.selectable_label(selected, format!("{icon}  {name}"));
            if response.double_clicked() {
                tools.cancel();
                doc.enter(id);
            } else if response.clicked() {
                let additive = ui.input(|i| i.modifiers.command);
                let selection = doc.selection_mut();
                if !additive {
                    selection.clear();
                }
                selection.toggle([Entity::Instance(id)]);
            }
        }
    });

    ui.separator();
    ui.horizontal_wrapped(|ui| {
        if ui.button(format!("{} Group", ph::CUBE)).on_hover_text("Make Group (Ctrl+G)").clicked() {
            tools.make_group(doc, DefinitionKind::Group);
        }
        if ui.button(format!("{} Component", ph::PUZZLE_PIECE)).on_hover_text("Make Component (G)").clicked() {
            tools.make_group(doc, DefinitionKind::Component);
        }
        if ui.button("Explode").on_hover_text("Turn the selected groups back into loose geometry").clicked() {
            tools.explode(doc);
        }
    });
    let mut auto_group = tools.auto_group();
    if ui
        .checkbox(&mut auto_group, "Auto-group new boards")
        .on_hover_text("When Push/Pull turns a free-standing face into a solid, make it a group")
        .changed()
    {
        tools.set_auto_group(auto_group);
    }
}

/// "Model › Group#1 › Leg", with a button to close the innermost open group.
fn breadcrumb(ui: &mut egui::Ui, doc: &mut Document, tools: &mut Tools) {
    let scene = doc.scene();
    let path = doc.path().to_vec();
    let names: Vec<String> = (0..path.len())
        .filter_map(|depth| {
            let instance = scene.container(&path[..depth])?.instance(path[depth])?;
            scene.definition(instance.definition).map(|d| d.name.clone())
        })
        .collect();
    ui.horizontal_wrapped(|ui| {
        ui.label("Model");
        for name in &names {
            ui.label("›");
            ui.strong(name);
        }
    });
    if !path.is_empty() && ui.button("Close group (Esc)").clicked() {
        tools.cancel();
        doc.exit();
    }
}
