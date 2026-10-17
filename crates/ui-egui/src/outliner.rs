//! The Outliner panel: where you are (which group is open), the groups and components at this
//! level, grouping commands, and the auto-group setting.

use eframe::egui;
use egui_phosphor::regular as ph;
use woodsketch_doc::{DefinitionKind, Document, Entity, InstanceId, MaterialId};
use woodsketch_tools::Tools;

pub fn show(ui: &mut egui::Ui, doc: &mut Document, tools: &mut Tools) {
    breadcrumb(ui, doc, tools);
    ui.separator();

    if doc.scene().root.instances().next().is_none() {
        ui.weak("No groups or components yet.");
    }
    // The whole model as a tree; act on a click after drawing it.
    let mut action = None;
    ui.vertical(|ui| rows(ui, doc, &[], None, &mut action));
    match action {
        Some(Action::Select { at, id, additive }) => {
            go_to(doc, tools, &at);
            let selection = doc.selection_mut();
            if !additive {
                selection.clear();
            }
            selection.toggle([Entity::Instance(id)]);
        }
        Some(Action::Open { at, id }) => {
            go_to(doc, tools, &at);
            tools.cancel();
            doc.enter(id);
        }
        None => {}
    }

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
    let path = doc.path().to_vec();
    let names = doc.path_names();
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

/// What a click in the tree asks for: select a part (opening the groups it's in), or open it.
enum Action {
    Select { at: Vec<InstanceId>, id: InstanceId, additive: bool },
    Open { at: Vec<InstanceId>, id: InstanceId },
}

/// The groups and components in the container at `at`, each with its own inside it (folded
/// unless the group being edited is in there). `inherited` is the material painted on an
/// enclosing group, which parts inside show.
fn rows(ui: &mut egui::Ui, doc: &Document, at: &[InstanceId], inherited: Option<MaterialId>, action: &mut Option<Action>) {
    let scene = doc.scene();
    let Some(container) = scene.container(at) else { return };
    for (id, inst) in container.instances() {
        let Some(def) = scene.definition(inst.definition) else { continue };
        let mut here = at.to_vec();
        here.push(id);
        let material = inst.material.or(inherited);
        let model = &def.container.model;
        let needs_material = model.faces().next().is_some() && material.is_none() && model.faces().all(|(_, f)| f.material.is_none());
        let icon = match def.kind {
            DefinitionKind::Group => ph::CUBE,
            DefinitionKind::Component => ph::PUZZLE_PIECE,
        };
        let visible = scene.is_visible(inst);
        let editing = doc.path() == here.as_slice();
        let selected = doc.path() == at && doc.selection().contains(Entity::Instance(id));
        let row = |ui: &mut egui::Ui| -> Option<egui::Response> {
            let mut text = egui::RichText::new(format!("{icon}  {}", def.name));
            if editing {
                text = text.strong();
            }
            if !visible {
                // Its tag is hidden: listed, but it can't be picked until shown again.
                ui.weak(format!("{icon}  {}  (hidden)", def.name));
                return None;
            }
            let response = ui.selectable_label(selected, text);
            let response = if editing { response.on_hover_text("Open: you're working inside it") } else { response };
            if needs_material {
                ui.colored_label(ui.visuals().warn_fg_color, ph::WARNING).on_hover_text("Needs a material for the cut list");
            }
            Some(response)
        };
        let response = if def.container.instances().next().is_some() {
            let open = doc.path().starts_with(&here);
            let state = egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), ui.make_persistent_id(("outliner", &here)), open);
            let (_, header, _) = state
                .show_header(ui, |ui| ui.horizontal(row).inner)
                .body(|ui| rows(ui, doc, &here, material, action));
            header.inner
        } else {
            // Line up with rows that have a fold arrow.
            ui.horizontal(|ui| {
                ui.add_space(ui.spacing().indent);
                row(ui)
            })
            .inner
        };
        if let Some(response) = response {
            if response.double_clicked() {
                *action = Some(Action::Open { at: at.to_vec(), id });
            } else if response.clicked() {
                let additive = ui.input(|i| i.modifiers.command);
                *action = Some(Action::Select { at: at.to_vec(), id, additive });
            }
        }
    }
}

/// Make `at` the container being edited: close groups until it's on the way, then open down.
fn go_to(doc: &mut Document, tools: &mut Tools, at: &[InstanceId]) {
    if doc.path() == at {
        return;
    }
    tools.cancel();
    while !at.starts_with(doc.path()) {
        doc.exit();
    }
    for &id in &at[doc.path().len()..] {
        if !doc.enter(id) {
            break;
        }
    }
}
