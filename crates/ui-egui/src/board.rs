//! The Board panel, at the top of the tray while the Board tool is active: the stock to draw
//! with (a material and one of its standard sizes), and whether lumber lies flat or on edge.

use eframe::egui;
use woodsketch_doc::board::{Stock, default_stock, stock_options};
use woodsketch_doc::library::LIBRARY;
use woodsketch_doc::units::{format_length, parse_length};
use woodsketch_doc::{Document, MaterialId, MaterialKind};
use woodsketch_tools::{Picked, Tools};

pub fn show(ui: &mut egui::Ui, doc: &mut Document, tools: &mut Tools) {
    let board = tools.board_mut();
    board.ensure_stock(doc);
    let woods: Vec<(MaterialId, String, MaterialKind)> = doc
        .scene()
        .materials()
        .filter(|(_, m)| !stock_options(m).is_empty())
        .map(|(id, m)| (id, m.name.clone(), m.kind))
        .collect();

    let mut pick = None;
    egui::Grid::new("board_stock")
        .num_columns(2)
        .spacing([8.0, 4.0])
        .show(ui, |ui| {
            ui.label("Material");
            let current = board
                .picked
                .as_ref()
                .and_then(|p| woods.iter().find(|(id, _, _)| *id == p.material));
            let shown = current.map_or_else(
                || "Pick a material".to_owned(),
                |(_, name, kind)| {
                    // "Pine (boards)", but not "Pine Lumber (lumber)".
                    if name
                        .to_lowercase()
                        .contains(noun(*kind).trim_end_matches('s'))
                    {
                        name.clone()
                    } else {
                        format!("{name} ({})", noun(*kind))
                    }
                },
            );
            egui::ComboBox::from_id_salt("board_material")
                .selected_text(shown)
                .height(520.0)
                .show_ui(ui, |ui| {
                    // By type, so Pine Lumber (2x4s) and Pine (boards) don't get mixed up: the drawing's
                    // woods first, then the library's others (picking one adds it).
                    for (kind, heading) in KINDS {
                        ui.label(egui::RichText::new(heading).strong());
                        ui.indent(heading, |ui| {
                            for (id, name, _) in woods.iter().filter(|w| w.2 == kind) {
                                if ui
                                    .selectable_label(
                                        current.is_some_and(|(c, _, _)| c == id),
                                        name.as_str(),
                                    )
                                    .clicked()
                                {
                                    pick = Some(first_stock(doc, *id));
                                }
                            }
                            let library = LIBRARY.iter().filter(|p| {
                                p.kind == kind && !woods.iter().any(|(_, name, _)| name == p.name)
                            });
                            for preset in library {
                                if ui
                                    .selectable_label(false, preset.name)
                                    .on_hover_text("From the library: adds it to the drawing")
                                    .clicked()
                                {
                                    let id = doc.add_preset(preset);
                                    pick = Some(first_stock(doc, id));
                                }
                            }
                        });
                    }
                });
            ui.end_row();

            if let Some(picked) = board.picked.clone()
                && let Some(material) = doc.scene().material(picked.material)
            {
                ui.label("Stock");
                egui::ComboBox::from_id_salt("board_stock_size")
                    .selected_text(label(&picked.name, picked.stock))
                    .height(560.0)
                    .show_ui(ui, |ui| {
                        // First, so it shows above a long list of sizes.
                        if ui.selectable_label(false, "Custom size...").on_hover_text("Add a size this material comes in (or type it in the Measurements box)").clicked() {
                            ui.data_mut(|d| d.insert_temp(custom_id(), true));
                        }
                        ui.separator();
                        for (name, stock) in stock_options(material) {
                            if ui
                                .selectable_label(stock == picked.stock, label(&name, stock))
                                .clicked()
                            {
                                pick = Some(Some(Picked {
                                    material: picked.material,
                                    stock,
                                    name,
                                }));
                            }
                        }
                    });
                ui.end_row();

                if matches!(picked.stock, Stock::Section { .. }) {
                    ui.label("Lay it");
                    ui.horizontal(|ui| {
                        ui.selectable_value(&mut board.on_edge, false, "Flat");
                        ui.selectable_value(&mut board.on_edge, true, "On edge")
                            .on_hover_text("Ctrl switches while drawing");
                    });
                    ui.end_row();
                }
            }
        });
    if let Some(picked) = pick {
        board.picked = picked;
    }
    if ui.data(|d| d.get_temp::<bool>(custom_id())).unwrap_or(false)
        && let Some(picked) = board.picked.clone()
        && let Some(added) = custom_size(ui, doc, picked.material, matches!(picked.stock, Stock::Section { .. }))
    {
        board.picked = Some(added);
    }
    if board
        .picked
        .as_ref()
        .is_some_and(|p| matches!(p.stock, Stock::Section { .. }))
    {
        ui.weak("Lumber is named by its nominal size; the actual size is in brackets. Draw any length: the cut list picks the board to buy (8', 10'...). For a size not listed, pick Custom size or type it, like 3,6 1/2.");
    } else if board
        .picked
        .as_ref()
        .is_some_and(|p| matches!(p.stock, Stock::Thickness(_)))
    {
        ui.weak("Drawn as a rectangle at this thickness. For 2x4s and other framing sizes, pick a Lumber material such as Pine Lumber.");
    }
}

/// The kinds of material boards are drawn from, as the dropdown groups them.
const KINDS: [(MaterialKind, &str); 3] = [
    (MaterialKind::DimensionalLumber, "Lumber: 2x4, 2x6..."),
    (MaterialKind::SolidWood, "Boards: 3/4\", 1\"... thick"),
    (MaterialKind::SheetGood, "Sheets: plywood, MDF"),
];

fn noun(kind: MaterialKind) -> &'static str {
    match kind {
        MaterialKind::DimensionalLumber => "lumber",
        MaterialKind::SolidWood => "boards",
        _ => "sheets",
    }
}

fn custom_id() -> egui::Id {
    egui::Id::new("board_custom_size")
}

/// The "Custom size" form: a thickness (and width, for lumber) added to the material's sizes,
/// so the cut list knows it. Returns the stock added.
fn custom_size(ui: &mut egui::Ui, doc: &mut Document, material: MaterialId, lumber: bool) -> Option<Picked> {
    let fields_id = egui::Id::new("board_custom_fields");
    let (mut thickness, mut width) = ui.data(|d| d.get_temp::<(String, String)>(fields_id)).unwrap_or_default();
    let mut added = None;
    let mut close = false;
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.strong("Custom size");
        egui::Grid::new("board_custom_grid").num_columns(2).show(ui, |ui| {
            ui.label("Thickness");
            ui.add(egui::TextEdit::singleline(&mut thickness).hint_text("1 1/4\"").desired_width(80.0));
            ui.end_row();
            if lumber {
                ui.label("Width");
                ui.add(egui::TextEdit::singleline(&mut width).hint_text("2 1/2\"").desired_width(80.0));
                ui.end_row();
            }
        });
        let t = parse_length(&thickness).ok().filter(|t| *t > 0.0);
        let w = parse_length(&width).ok().filter(|w| *w > 0.0);
        let stock = match (lumber, t, w) {
            (true, Some(thickness), Some(width)) => Some(Stock::Section { thickness, width }),
            (false, Some(t), _) => Some(Stock::Thickness(t)),
            _ => None,
        };
        let name = doc.scene().material(material).map_or_else(String::new, |m| m.name.clone());
        ui.horizontal(|ui| {
            if ui.add_enabled(stock.is_some(), egui::Button::new(format!("Add to {name}"))).clicked()
                && let Some(stock) = stock
                && let Some((name, stock)) = doc.add_stock(material, stock)
            {
                added = Some(Picked { material, stock, name });
                close = true;
            }
            if ui.button("Cancel").clicked() {
                close = true;
            }
        });
        ui.weak("It's added to the material's sizes, so the cut list knows it.");
    });
    ui.data_mut(|d| {
        if close {
            d.remove::<(String, String)>(fields_id);
            d.insert_temp(custom_id(), false);
        } else {
            d.insert_temp(fields_id, (thickness, width));
        }
    });
    added
}

/// A lumber section shows its actual size too: `2x4 (1 1/2" x 3 1/2")`.
fn label(name: &str, stock: Stock) -> String {
    match stock {
        Stock::Section { thickness, width } => format!(
            "{name} ({} x {})",
            format_length(thickness),
            format_length(width)
        ),
        Stock::Thickness(_) => name.to_owned(),
    }
}

fn first_stock(doc: &Document, material: MaterialId) -> Option<Picked> {
    let (name, stock) = default_stock(doc.scene().material(material)?)?;
    Some(Picked {
        material,
        stock,
        name,
    })
}
