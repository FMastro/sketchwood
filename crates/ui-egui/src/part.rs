//! The part section of Entity Info, for one selected group or component: its size measured
//! the way a woodworker does (length along the grain), the stock it's cut from, and its grain
//! direction and cut.

use eframe::egui;
use egui_phosphor::regular as ph;
use woodsketch_doc::cutlist::Part;
use woodsketch_doc::cutplan::match_stock;
use woodsketch_doc::part::{Cut, PartAxis};
use woodsketch_doc::units::format_length;
use woodsketch_doc::{Document, Entity, InstanceId, MaterialKind};

pub fn show(ui: &mut egui::Ui, doc: &mut Document, instance: InstanceId) {
    let (Some(size), Some(extents), Some(info)) = (doc.part_size(instance), doc.part_extents(instance), doc.part_info(instance)) else {
        ui.weak("An assembly: its parts are the groups and components inside.");
        return;
    };
    let [length, width, thickness] = size;
    let material = doc.material_of(Entity::Instance(instance)).and_then(|m| doc.scene().material(m)).cloned();

    egui::Grid::new(("part_info", instance)).num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
        ui.label("Size");
        ui.label(format!("{} x {} x {}", format_length(length), format_length(width), format_length(thickness)))
            .on_hover_text("Length (along the grain) x width x thickness");
        ui.end_row();

        ui.label("Stock");
        match &material {
            None => {
                ui.colored_label(ui.visuals().warn_fg_color, "Needs a material");
            }
            Some(m) => {
                let part = Part { label: String::new(), names: Vec::new(), count: 1, length, width, thickness };
                match match_stock(&part, m.kind, &m.standard) {
                    Ok(stock) => {
                        ui.label(format!("{}, {}", m.name, stock.name));
                    }
                    Err(reason) if !matches!(m.kind, MaterialKind::Hardware | MaterialKind::Other) => {
                        ui.colored_label(ui.visuals().warn_fg_color, format!("{} {}", ph::WARNING, m.name)).on_hover_text(reason);
                    }
                    Err(_) => {
                        ui.label(&m.name);
                    }
                }
            }
        }
        ui.end_row();

        ui.label("Grain");
        let side = |axis: PartAxis| format!("Along the {} side", format_length(extents[axis.index()]));
        let shown = info.grain.map_or_else(|| "Along its length".to_owned(), side);
        let mut picked = None;
        egui::ComboBox::from_id_salt(("part_grain", instance)).selected_text(shown).show_ui(ui, |ui| {
            if ui.selectable_label(info.grain.is_none(), "Along its length").on_hover_text("The longest side (the usual case)").clicked() {
                picked = Some(None);
            }
            for axis in PartAxis::ALL {
                if ui.selectable_label(info.grain == Some(axis), side(axis)).clicked() {
                    picked = Some(Some(axis));
                }
            }
        });
        if let Some(grain) = picked {
            doc.edit_part(instance, |p| p.grain = grain);
        }
        ui.end_row();

        // Sawn boards only: sheet goods don't move like solid wood.
        let sawn = material.as_ref().is_none_or(|m| matches!(m.kind, MaterialKind::SolidWood | MaterialKind::DimensionalLumber));
        if sawn {
            ui.label("Cut");
            let mut picked = None;
            egui::ComboBox::from_id_salt(("part_cut", instance)).selected_text(info.cut.name()).show_ui(ui, |ui| {
                for cut in Cut::ALL {
                    let hint = match cut {
                        Cut::Unknown => "Movement checks will assume flat-sawn, which moves the most",
                        Cut::FlatSawn => "Plain-sawn: growth rings run across the end grain; moves the most",
                        Cut::QuarterSawn => "Growth rings run up and down the end grain; moves about half as much",
                    };
                    if ui.selectable_label(info.cut == cut, cut.name()).on_hover_text(hint).clicked() {
                        picked = Some(cut);
                    }
                }
            });
            if let Some(cut) = picked {
                doc.edit_part(instance, |p| p.cut = cut);
            }
            ui.end_row();
        }
    });
}
