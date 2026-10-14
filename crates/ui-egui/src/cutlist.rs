//! The Cut List window: parts grouped by material, cutting diagrams on standard stock with a
//! shopping list, and the stock sizes and saw kerf they assume (saved with the drawing, with
//! per-material sizes). Exports a CSV, or a cut sheet to print.

use std::collections::HashMap;

use eframe::egui;
use egui_phosphor::regular as ph;
use sketchcraft_doc::cutlist::{self, CutList, MaterialParts};
use sketchcraft_doc::cutplan::{self, Plan, Settings, StockOverride};
use sketchcraft_doc::units::{format_length, parse_length, parse_lengths};
use sketchcraft_doc::{Document, MaterialId, MaterialKind, cutsheet};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Tab {
    #[default]
    Parts,
    Diagrams,
    Settings,
}

#[derive(Default)]
pub struct CutListWindow {
    pub open: bool,
    pub tab: Tab,
    /// The drawing's settings as typed (and the drawing version they were read at).
    settings_text: Option<(u64, SettingsText)>,
    /// Each material's own stock sizes as typed.
    stock_text: HashMap<MaterialId, StockText>,
    /// The list and plans, for the drawing version they were made from.
    cache: Option<Cache>,
    message: Option<String>,
}

struct Cache {
    model_version: u64,
    list: CutList,
    plans: Vec<Option<Plan>>,
}

struct SettingsText {
    kerf: String,
    sheet_length: String,
    sheet_width: String,
    board_lengths: String,
    board_widths: String,
    lumber_lengths: String,
}

fn list_text(values: &[f64]) -> String {
    values.iter().map(|&v| format_length(v)).collect::<Vec<_>>().join(", ")
}

/// A length, or `None` if it doesn't parse or is negative.
fn one(s: &str) -> Option<f64> {
    parse_length(s).ok().filter(|v| *v >= 0.0)
}

/// A comma-separated list of positive lengths.
fn many(s: &str) -> Option<Vec<f64>> {
    let values: Vec<f64> = parse_lengths(s).ok()?.into_iter().flatten().filter(|v| *v > 0.0).collect();
    (!values.is_empty()).then_some(values)
}

impl SettingsText {
    fn from(settings: &Settings) -> Self {
        Self {
            kerf: format_length(settings.kerf),
            sheet_length: format_length(settings.sheet.0),
            sheet_width: format_length(settings.sheet.1),
            board_lengths: list_text(&settings.board_lengths),
            board_widths: list_text(&settings.board_widths),
            lumber_lengths: list_text(&settings.lumber_lengths),
        }
    }

    fn parse(&self) -> Option<Settings> {
        Some(Settings {
            kerf: one(&self.kerf)?,
            sheet: (one(&self.sheet_length)?, one(&self.sheet_width)?),
            board_lengths: many(&self.board_lengths)?,
            board_widths: many(&self.board_widths)?,
            lumber_lengths: many(&self.lumber_lengths)?,
        })
    }
}

/// A material's own sizes as typed; blank fields use the drawing's.
struct StockText {
    first: String,
    second: String,
}

impl StockText {
    fn from(kind: MaterialKind, stock: &StockOverride) -> Self {
        match kind {
            MaterialKind::SheetGood => Self {
                first: stock.sheet.map(|s| format_length(s.0)).unwrap_or_default(),
                second: stock.sheet.map(|s| format_length(s.1)).unwrap_or_default(),
            },
            _ => Self {
                first: stock.lengths.as_deref().map(list_text).unwrap_or_default(),
                second: stock.widths.as_deref().map(list_text).unwrap_or_default(),
            },
        }
    }

    /// The override, or `None` if something typed doesn't parse.
    fn parse(&self, kind: MaterialKind) -> Option<StockOverride> {
        let blank = |s: &str| s.trim().is_empty();
        Some(match kind {
            MaterialKind::SheetGood => StockOverride {
                sheet: if blank(&self.first) && blank(&self.second) {
                    None
                } else {
                    Some((one(&self.first)?, one(&self.second)?))
                },
                ..StockOverride::default()
            },
            _ => StockOverride {
                lengths: if blank(&self.first) { None } else { Some(many(&self.first)?) },
                widths: if blank(&self.second) || kind != MaterialKind::SolidWood { None } else { Some(many(&self.second)?) },
                ..StockOverride::default()
            },
        })
    }
}

impl CutListWindow {
    /// The current cut list and plans, recomputed when the drawing changes.
    pub fn refresh(&mut self, doc: &Document) -> (&CutList, &[Option<Plan>]) {
        if self.cache.as_ref().is_none_or(|c| c.model_version != doc.model_version()) {
            let list = cutlist::cut_list(doc.scene());
            let plans = cutplan::plan_all(doc.scene(), &list);
            self.cache = Some(Cache { model_version: doc.model_version(), list, plans });
        }
        let cache = self.cache.as_ref().expect("just filled");
        (&cache.list, &cache.plans)
    }

    pub fn show(&mut self, ctx: &egui::Context, doc: &mut Document) {
        if !self.open {
            return;
        }
        let mut open = self.open;
        egui::Window::new(format!("{} Cut List", ph::TABLE))
            .id(egui::Id::new("cut_list"))
            .open(&mut open)
            .default_size([760.0, 540.0])
            .resizable(true)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    for (tab, label) in [(Tab::Parts, "Parts"), (Tab::Diagrams, "Cutting diagrams"), (Tab::Settings, "Stock & kerf")] {
                        if ui.selectable_label(self.tab == tab, label).clicked() {
                            self.tab = tab;
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let print = ui
                            .button(format!("{} Print cut sheet...", ph::PRINTER))
                            .on_hover_text("Save the parts and cutting diagrams as a page to print, or save as PDF, from your browser");
                        if print.clicked() {
                            self.export_sheet(doc);
                        }
                        if ui.button(format!("{} Export CSV...", ph::FILE_CSV)).clicked() {
                            self.export_csv(doc);
                        }
                    });
                });
                if let Some(message) = &self.message {
                    ui.colored_label(ui.visuals().warn_fg_color, message);
                }
                ui.separator();
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| match self.tab {
                    Tab::Parts => {
                        let (list, _) = self.refresh(doc);
                        parts_tab(ui, list, doc);
                    }
                    Tab::Diagrams => {
                        let (list, plans) = self.refresh(doc);
                        diagrams_tab(ui, list, plans, doc);
                    }
                    Tab::Settings => self.settings_tab(ui, doc),
                });
            });
        self.open = open;
    }

    fn export_csv(&mut self, doc: &Document) {
        let (list, _) = self.refresh(doc);
        let csv = cutlist::to_csv(list);
        let Some(path) = rfd::FileDialog::new().add_filter("CSV", &["csv"]).set_file_name("cut list.csv").save_file() else {
            return;
        };
        self.message = match std::fs::write(&path, csv) {
            Ok(()) => None,
            Err(e) => Some(format!("Couldn't save {}: {e}", path.display())),
        };
    }

    /// Save the printable cut sheet and open it in the browser to print.
    fn export_sheet(&mut self, doc: &Document) {
        let html = self.cut_sheet(doc, "Cut list");
        let Some(path) = rfd::FileDialog::new().add_filter("Web page", &["html"]).set_file_name("cut sheet.html").save_file() else {
            return;
        };
        self.message = match std::fs::write(&path, html) {
            Ok(()) => open_with_system(&path).err().map(|e| format!("Saved, but couldn't open it: {e}")),
            Err(e) => Some(format!("Couldn't save {}: {e}", path.display())),
        };
    }

    /// The cut sheet as a web page.
    pub fn cut_sheet(&mut self, doc: &Document, title: &str) -> String {
        let (list, plans) = self.refresh(doc);
        cutsheet::to_html(doc.scene(), list, plans, title)
    }

    fn settings_tab(&mut self, ui: &mut egui::Ui, doc: &mut Document) {
        // Retyped from the drawing whenever it changes underneath (undo, open).
        let version = doc.model_version();
        if self.settings_text.as_ref().is_none_or(|(v, _)| *v != version) {
            self.settings_text = Some((version, SettingsText::from(&doc.scene().cut_settings)));
            self.stock_text.clear();
        }
        let (_, text) = self.settings_text.as_mut().expect("just filled");
        ui.label("Standard stock for the cutting diagrams, saved with the drawing (inches unless you type units). Lists are comma-separated.");
        ui.add_space(6.0);
        let mut changed = false;
        egui::Grid::new("cut_settings").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
            let mut row = |ui: &mut egui::Ui, label: &str, value: &mut String, hint: &str| {
                ui.label(label);
                changed |= ui.add(egui::TextEdit::singleline(value).desired_width(260.0).hint_text(hint)).changed();
                ui.end_row();
            };
            row(ui, "Saw kerf", &mut text.kerf, "1/8\"");
            row(ui, "Sheet length", &mut text.sheet_length, "96\"");
            row(ui, "Sheet width", &mut text.sheet_width, "48\"");
            row(ui, "Solid wood board lengths", &mut text.board_lengths, "72, 96, 120");
            row(ui, "Solid wood board widths", &mut text.board_widths, "3.5, 5.5, 7.25");
            row(ui, "Lumber lengths", &mut text.lumber_lengths, "96, 120, 144, 192");
        });
        if changed {
            match text.parse() {
                Some(settings) => {
                    doc.set_cut_settings(settings);
                    self.message = None;
                }
                None => self.message = Some("Some stock sizes don't read as lengths; the last good ones are used.".into()),
            }
            // Keep what's typed: this edit isn't a change from elsewhere.
            if let Some((v, _)) = &mut self.settings_text {
                *v = doc.model_version();
            }
        }
        ui.add_space(8.0);
        if ui.button("Reset to US defaults").clicked() {
            doc.set_cut_settings(Settings::default());
            self.message = None;
        }

        self.material_stock(ui, doc);
    }

    /// Each stock material's own sizes (OpenCutList keeps sizes per material).
    fn material_stock(&mut self, ui: &mut egui::Ui, doc: &mut Document) {
        let materials: Vec<(MaterialId, String, MaterialKind, StockOverride)> = doc
            .scene()
            .materials()
            .filter(|(_, m)| matches!(m.kind, MaterialKind::SheetGood | MaterialKind::SolidWood | MaterialKind::DimensionalLumber))
            .map(|(id, m)| (id, m.name.clone(), m.kind, m.stock.clone()))
            .collect();
        if materials.is_empty() {
            return;
        }
        ui.add_space(12.0);
        ui.strong("Per material");
        ui.weak("Leave blank to use the sizes above.");
        ui.add_space(4.0);
        let mut edits = Vec::new();
        egui::Grid::new("material_stock").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
            for (id, name, kind, stock) in &materials {
                let text = self.stock_text.entry(*id).or_insert_with(|| StockText::from(*kind, stock));
                ui.label(name);
                let (first, second) = match kind {
                    MaterialKind::SheetGood => ("Sheet length", Some("width")),
                    MaterialKind::SolidWood => ("Board lengths", Some("widths")),
                    _ => ("Lengths", None),
                };
                let mut changed = false;
                ui.horizontal(|ui| {
                    ui.weak(first);
                    changed |= ui.add(egui::TextEdit::singleline(&mut text.first).desired_width(140.0)).changed();
                    if let Some(label) = second {
                        ui.weak(label);
                        changed |= ui.add(egui::TextEdit::singleline(&mut text.second).desired_width(140.0)).changed();
                    }
                });
                ui.end_row();
                if changed {
                    edits.push((*id, text.parse(*kind)));
                }
            }
        });
        for (id, parsed) in edits {
            match parsed {
                Some(stock) => {
                    doc.edit_material(id, |m| m.stock = stock);
                    self.message = None;
                }
                None => self.message = Some("Some stock sizes don't read as lengths; the last good ones are used.".into()),
            }
            if let Some((v, _)) = &mut self.settings_text {
                *v = doc.model_version();
            }
        }
    }
}

/// Open a file with the system's default program (the browser, for a web page).
fn open_with_system(path: &std::path::Path) -> std::io::Result<()> {
    #[cfg(windows)]
    let mut command = {
        let mut c = std::process::Command::new("cmd");
        c.args(["/C", "start", ""]).arg(path);
        c
    };
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut c = std::process::Command::new("open");
        c.arg(path);
        c
    };
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut command = {
        let mut c = std::process::Command::new("xdg-open");
        c.arg(path);
        c
    };
    command.spawn().map(|_| ())
}

fn kind_name(material: &MaterialParts) -> &'static str {
    material.kind.map_or("no material", MaterialKind::name)
}

/// The total that fits the material: board feet, square feet or linear feet.
fn amount(material: &MaterialParts) -> String {
    match material.kind {
        Some(MaterialKind::SheetGood) => format!("{:.1} sq ft", material.square_feet()),
        Some(MaterialKind::DimensionalLumber) => format!("{:.1} linear ft", material.linear_feet()),
        Some(MaterialKind::SolidWood) => format!("{:.2} board ft", material.board_feet()),
        _ => format!("{} pieces", material.part_count()),
    }
}

fn color32(doc: &Document, material: &MaterialParts) -> egui::Color32 {
    let rgb = material.material.and_then(|m| doc.scene().material(m)).map_or([230, 230, 225], |m| m.color);
    egui::Color32::from_rgb(rgb[0], rgb[1], rgb[2])
}

fn parts_tab(ui: &mut egui::Ui, list: &CutList, doc: &Document) {
    if list.materials.is_empty() {
        ui.label("No parts yet. Each group or component is a part: boards made with Push/Pull count automatically.");
    }
    if list.loose_faces > 0 {
        ui.colored_label(
            ui.visuals().warn_fg_color,
            format!("{} loose faces aren't in any group, so they aren't parts. Group them (Ctrl+G) to list them.", list.loose_faces),
        );
    }
    for material in &list.materials {
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
            ui.painter().rect_filled(rect, 2.0, color32(doc, material));
            ui.strong(&material.name);
            ui.weak(format!("{}  ·  {} parts  ·  {}", kind_name(material), material.part_count(), amount(material)));
        });
        egui::Grid::new(("parts", &material.name)).striped(true).num_columns(6).spacing([16.0, 4.0]).show(ui, |ui| {
            for heading in ["", "Name", "Qty", "Length", "Width", "Thickness"] {
                ui.strong(heading);
            }
            ui.end_row();
            for part in &material.parts {
                ui.strong(&part.label);
                ui.label(part.names.join(", "));
                ui.label(part.count.to_string());
                ui.label(format_length(part.length));
                ui.label(format_length(part.width));
                // Flag parts thinner or thicker than the material's stock.
                let off = material.stock_thickness.is_some_and(|t| (t - part.thickness).abs() > 1.0 / 64.0);
                let thickness = egui::RichText::new(format_length(part.thickness));
                if off {
                    ui.label(thickness.color(ui.visuals().warn_fg_color)).on_hover_text(format!(
                        "The material's stock is {} thick",
                        format_length(material.stock_thickness.unwrap_or_default())
                    ));
                } else {
                    ui.label(thickness);
                }
                ui.end_row();
            }
        });
    }
}

fn diagrams_tab(ui: &mut egui::Ui, list: &CutList, plans: &[Option<Plan>], doc: &Document) {
    if list.materials.is_empty() {
        ui.label("No parts to cut yet.");
    }
    for (material, plan) in list.materials.iter().zip(plans) {
        ui.add_space(8.0);
        ui.heading(&material.name);
        let Some(plan) = plan else {
            ui.weak(match material.kind {
                Some(MaterialKind::Hardware) => "Hardware isn't cut from stock.",
                None => "Paint these parts with a material (and set its type) to plan the cuts.",
                _ => "This material type isn't cut from stock. Set it to sheet good, solid wood or dimensional lumber.",
            });
            continue;
        };
        let buy: Vec<String> = plan.shopping_list().into_iter().map(|(what, n)| format!("{n} × {what}")).collect();
        ui.horizontal_wrapped(|ui| {
            ui.label(format!("{} Buy:", ph::SHOPPING_CART));
            ui.strong(buy.join(",  "));
            ui.weak(format!("({:.0}% used)", plan.yield_fraction() * 100.0));
        });
        for (label, reason) in &plan.unplaced {
            ui.colored_label(ui.visuals().warn_fg_color, format!("Part {label}: {reason}"));
        }
        let color = color32(doc, material);
        for (i, stock) in plan.stocks.iter().enumerate() {
            ui.add_space(4.0);
            ui.label(format!("{}. {}  ·  {:.0}% used", i + 1, stock.description, stock.used() * 100.0));
            draw_stock(ui, stock, color);
        }
    }
}

/// A piece of stock to scale, with its parts labeled. Length runs left to right.
fn draw_stock(ui: &mut egui::Ui, stock: &cutplan::Stock, color: egui::Color32) {
    let max_width = ui.available_width().min(720.0);
    let scale = max_width / stock.length as f32;
    // Keep long, narrow lumber visible.
    let height = (stock.width as f32 * scale).max(22.0);
    let y_scale = height / stock.width as f32;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(max_width, height), egui::Sense::hover());
    let painter = ui.painter_at(rect.expand(1.0));
    painter.rect(rect, 0.0, egui::Color32::from_gray(225), egui::Stroke::new(1.0, egui::Color32::GRAY), egui::StrokeKind::Inside);
    let fill = color.gamma_multiply(0.85);
    for p in &stock.placements {
        let min = rect.min + egui::vec2(p.x as f32 * scale, p.y as f32 * y_scale);
        let part = egui::Rect::from_min_size(min, egui::vec2(p.length as f32 * scale, p.width as f32 * y_scale));
        painter.rect(part, 1.0, fill, egui::Stroke::new(1.0, egui::Color32::from_gray(40)), egui::StrokeKind::Inside);
        let text = if part.width() > 90.0 && part.height() > 26.0 {
            format!("{}\n{} x {}", p.label, format_length(p.length), format_length(p.width))
        } else {
            p.label.clone()
        };
        let font = egui::FontId::proportional(if part.height() < 18.0 { 10.0 } else { 12.0 });
        painter.text(part.center(), egui::Align2::CENTER_CENTER, text, font, egui::Color32::BLACK);
    }
}
