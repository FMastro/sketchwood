//! The Board panel, at the top of the tray while the Board tool is active: the stock to draw
//! with (a material and one of its standard sizes), and whether lumber lies flat or on edge.

use eframe::egui;
use woodsketch_doc::board::{Stock, default_stock, stock_options};
use woodsketch_doc::library::LIBRARY;
use woodsketch_doc::units::format_length;
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
        egui::ComboBox::from_id_salt("board_material").selected_text(shown).height(480.0).show_ui(ui, |ui| {
            // The drawing's woods, then the library's other woods (picking one adds it).
            for (id, name) in &woods {
                if ui.selectable_label(current.is_some_and(|(c, _)| c == id), name).clicked() {
                    pick = Some(first_stock(doc, *id));
                }
            }
            let library: Vec<_> = LIBRARY
                .iter()
                .filter(|p| !stock_options(&p.material()).is_empty() && !woods.iter().any(|(_, name)| name == p.name))
                .collect();
            if !library.is_empty() {
                ui.separator();
                ui.weak("Library");
                for preset in library {
                    if ui.selectable_label(false, preset.name).clicked() {
                        let id = doc.add_preset(preset);
                        pick = Some(first_stock(doc, id));
                    }
                }
            }
        });
        ui.end_row();

        if let Some(picked) = board.picked.clone()
            && let Some(material) = doc.scene().material(picked.material)
        {
            ui.label("Stock");
            egui::ComboBox::from_id_salt("board_stock_size").selected_text(label(&picked.name, picked.stock)).show_ui(ui, |ui| {
                for (name, stock) in stock_options(material) {
                    if ui.selectable_label(stock == picked.stock, label(&name, stock)).clicked() {
                        pick = Some(Some(Picked { material: picked.material, stock, name }));
                    }
                }
            });
            ui.end_row();

            if matches!(picked.stock, Stock::Section { .. }) {
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
    if board.picked.as_ref().is_some_and(|p| matches!(p.stock, Stock::Section { .. })) {
        ui.weak("Lumber is named by its nominal size; the actual size is in brackets. Draw any length: the cut list picks the board to buy (8', 10'...).");
    }
}

/// A lumber section shows its actual size too: `2x4 (1 1/2" x 3 1/2")`.
fn label(name: &str, stock: Stock) -> String {
    match stock {
        Stock::Section { thickness, width } => format!("{name} ({} x {})", format_length(thickness), format_length(width)),
        Stock::Thickness(_) => name.to_owned(),
    }
}

fn first_stock(doc: &Document, material: MaterialId) -> Option<Picked> {
    let (name, stock) = default_stock(doc.scene().material(material)?)?;
    Some(Picked { material, stock, name })
}
