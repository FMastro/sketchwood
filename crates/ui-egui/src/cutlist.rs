//! The Cut List window: parts grouped by material, cutting diagrams on standard stock with a
//! shopping list, and the stock sizes and saw kerf they assume.

use eframe::egui;
use egui_phosphor::regular as ph;
use sketchcraft_doc::cutlist::{self, CutList, MaterialParts};
use sketchcraft_doc::cutplan::{self, Plan, Settings};
use sketchcraft_doc::units::{format_length, parse_length, parse_lengths};
use sketchcraft_doc::{Document, MaterialKind};

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
    settings: Settings,
    /// Settings as typed, applied when they parse.
    settings_text: Option<SettingsText>,
    /// The list and plans, for the document version and settings they were made from.
    cache: Option<Cache>,
    message: Option<String>,
}

struct Cache {
    model_version: u64,
    settings: Settings,
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

impl SettingsText {
    fn from(settings: &Settings) -> Self {
        let list = |values: &[f64]| values.iter().map(|&v| format_length(v)).collect::<Vec<_>>().join(", ");
        Self {
            kerf: format_length(settings.kerf),
            sheet_length: format_length(settings.sheet.0),
            sheet_width: format_length(settings.sheet.1),
            board_lengths: list(&settings.board_lengths),
            board_widths: list(&settings.board_widths),
            lumber_lengths: list(&settings.lumber_lengths),
        }
    }
}

impl CutListWindow {
    /// The current cut list and plans, recomputed when the drawing or settings change.
    pub fn refresh(&mut self, doc: &Document) -> (&CutList, &[Option<Plan>]) {
        let stale = self.cache.as_ref().is_none_or(|c| c.model_version != doc.model_version() || c.settings != self.settings);
        if stale {
            let list = cutlist::cut_list(doc.scene());
            let plans = list.materials.iter().map(|m| cutplan::plan(m, &self.settings)).collect();
            self.cache = Some(Cache { model_version: doc.model_version(), settings: self.settings.clone(), list, plans });
        }
        let cache = self.cache.as_ref().expect("just filled");
        (&cache.list, &cache.plans)
    }

    pub fn show(&mut self, ctx: &egui::Context, doc: &Document) {
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
                    Tab::Settings => self.settings_tab(ui),
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

    fn settings_tab(&mut self, ui: &mut egui::Ui) {
        let text = self.settings_text.get_or_insert_with(|| SettingsText::from(&self.settings));
        ui.label("Standard stock the cutting diagrams use (inches unless you type units). Lists are comma-separated.");
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
            let one = |s: &str| parse_length(s).ok().filter(|v| *v >= 0.0);
            let many = |s: &str| -> Option<Vec<f64>> {
                let values: Vec<f64> = parse_lengths(s).ok()?.into_iter().flatten().filter(|v| *v > 0.0).collect();
                (!values.is_empty()).then_some(values)
            };
            let parsed = (|| {
                Some(Settings {
                    kerf: one(&text.kerf)?,
                    sheet: (one(&text.sheet_length)?, one(&text.sheet_width)?),
                    board_lengths: many(&text.board_lengths)?,
                    board_widths: many(&text.board_widths)?,
                    lumber_lengths: many(&text.lumber_lengths)?,
                })
            })();
            match parsed {
                Some(settings) => {
                    self.settings = settings;
                    self.message = None;
                }
                None => self.message = Some("Some stock sizes don't read as lengths; the last good ones are used.".into()),
            }
        }
        ui.add_space(8.0);
        if ui.button("Reset to US defaults").clicked() {
            self.settings = Settings::default();
            self.settings_text = None;
            self.message = None;
        }
    }
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
