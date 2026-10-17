//! The Board panel, at the top of the tray while the Board tool is active: the stock to draw
//! with (a material and one of its standard sizes), and whether lumber lies flat or on edge.

use eframe::egui;
use egui_phosphor::regular as ph;
use woodsketch_doc::board::stock_options;
use woodsketch_doc::library::LIBRARY;
use woodsketch_doc::{Document, MaterialId};
use woodsketch_tools::{Picked, Tools};

pub fn show(ui: &mut egui::Ui, doc: &mut Document, tools: &mut Tools) {
    let board = tools.board_mut();
    board.ensure_stock(doc);
    let woods: Vec<(MaterialId, String)> = doc
        .scene()
        .materials()
        .filter(|(_, m)| !stock_options(m).is_empty())
        .map(|(id, m)| (id, m.name.clone()))
        .collect();

    let mut pick = None;
    egui::Grid::new("board_stock").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
        ui.label("Material");
        let current = board.picked.as_ref().and_then(|p| woods.iter().find(|(id, _)| *id == p.material));
        let shown = current.map_or("Pick a material", |(_, name)| name.as_str());
        egui::ComboBox::from_id_salt("board_material").selected_text(shown).show_ui(ui, |ui| {
            for (id, name) in &woods {
                if ui.selectable_label(current.is_some_and(|(c, _)| c == id), name).clicked() {
                    pick = Some(first_stock(doc, *id));
                }
            }
            if woods.is_empty() {
                ui.weak("No wood materials yet: add one below.");
            }
        });
        ui.end_row();

        if let Some(picked) = board.picked.clone()
            && let Some(material) = doc.scene().material(picked.material)
        {
            ui.label("Stock");
            egui::ComboBox::from_id_salt("board_stock_size").selected_text(&picked.name).show_ui(ui, |ui| {
                for (name, stock) in stock_options(material) {
                    if ui.selectable_label(stock == picked.stock, &name).clicked() {
                        pick = Some(Some(Picked { material: picked.material, stock, name }));
                    }
                }
            });
            ui.end_row();

            if matches!(picked.stock, woodsketch_doc::board::Stock::Section { .. }) {
                ui.label("Lay it");
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut board.on_edge, false, "Flat");
                    ui.selectable_value(&mut board.on_edge, true, "On edge").on_hover_text("Ctrl switches while drawing");
                });
                ui.end_row();
            }
        }
    });
    if let Some(picked) = pick {
        board.picked = picked;
    }

    ui.menu_button(format!("{} Add wood from the library", ph::PLUS), |ui| {
        for preset in LIBRARY.iter().filter(|p| !stock_options(&p.material()).is_empty()) {
            if ui.button(preset.name).clicked() {
                let id = doc.add_preset(preset);
                tools.board_mut().picked = first_stock(doc, id);
                ui.close();
            }
        }
    });
}

fn first_stock(doc: &Document, material: MaterialId) -> Option<Picked> {
    let (name, stock) = stock_options(doc.scene().material(material)?).into_iter().next()?;
    Some(Picked { material, stock, name })
}
