//! The Cut List window: parts grouped by material with the stock each is cut from, cutting
//! diagrams with a shopping list, and the saw kerf (each material's standard stock is set in the
//! Materials panel). Exports a CSV, or a cut sheet to print.

use eframe::egui;
use egui_phosphor::regular as ph;
use sketchcraft_doc::cutlist::{self, CutList, MaterialParts};
use sketchcraft_doc::cutplan::{self, Plan, Settings, match_stock};
use sketchcraft_doc::units::{format_length, parse_length};
use sketchcraft_doc::{Document, MaterialKind, cutsheet};

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
    /// The kerf as typed (and the drawing version it was read at).
    kerf_text: Option<(u64, String)>,
    /// The list and plans, for the drawing version they were made from.
    cache: Option<Cache>,
    message: Option<String>,
}

struct Cache {
    model_version: u64,
    list: CutList,
    plans: Vec<Option<Plan>>,
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

    /// Show the window. Returns a material to open in the Materials panel ("Edit material").
    pub fn show(&mut self, ctx: &egui::Context, doc: &mut Document) -> Option<sketchcraft_doc::MaterialId> {
        if !self.open {
            return None;
        }
        let mut edit = None;
        let mut open = self.open;
        egui::Window::new(format!("{} Cut List", ph::TABLE))
            .id(egui::Id::new("cut_list"))
            .open(&mut open)
            .default_size([760.0, 540.0])
            .resizable(true)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    for (tab, label) in [(Tab::Parts, "Parts"), (Tab::Diagrams, "Cutting diagrams"), (Tab::Settings, "Saw kerf")] {
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
                        parts_tab(ui, list, doc, &mut edit);
                    }
                    Tab::Diagrams => {
                        let (list, plans) = self.refresh(doc);
                        diagrams_tab(ui, list, plans, doc);
                    }
                    Tab::Settings => self.settings_tab(ui, doc),
                });
            });
        self.open = open;
        edit
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
        if self.kerf_text.as_ref().is_none_or(|(v, _)| *v != version) {
            self.kerf_text = Some((version, format_length(doc.scene().cut_settings.kerf)));
        }
        let (_, text) = self.kerf_text.as_mut().expect("just filled");
        ui.label("The saw kerf every cut takes, saved with the drawing.");
        ui.weak("Stock sizes (thicknesses, sheet sizes, lumber sections and lengths) are set for each material in the Materials panel.");
        ui.add_space(6.0);
        let changed = ui
            .horizontal(|ui| {
                ui.label("Saw kerf");
                ui.add(egui::TextEdit::singleline(text).desired_width(100.0).hint_text("1/8\"")).changed()
            })
            .inner;
        if changed {
            match parse_length(text).ok().filter(|k| *k >= 0.0) {
                Some(kerf) => {
                    doc.set_cut_settings(Settings { kerf });
                    self.message = None;
                }
                None => self.message = Some("The kerf doesn't read as a length; the last good one is used.".into()),
            }
            // Keep what's typed: this edit isn't a change from elsewhere.
            if let Some((v, _)) = &mut self.kerf_text {
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

fn parts_tab(ui: &mut egui::Ui, list: &CutList, doc: &Document, edit: &mut Option<sketchcraft_doc::MaterialId>) {
    if list.materials.is_empty() {
        ui.label("No parts yet. Each group or component is a part: boards made with Push/Pull count automatically.");
    }
    let needs = list.needs_material();
    if needs > 0 {
        ui.colored_label(
            ui.visuals().warn_fg_color,
            format!(
                "{} {needs} part{} need{} a material (listed first). Paint each one, or paint the group around them.",
                ph::WARNING,
                if needs == 1 { "" } else { "s" },
                if needs == 1 { "s" } else { "" },
            ),
        );
    }
    if list.loose_faces > 0 {
        ui.colored_label(
            ui.visuals().warn_fg_color,
            format!("{} loose faces aren't in any group, so they aren't parts. Group them (Ctrl+G) to list them.", list.loose_faces),
        );
    }
    // What still needs a material first: that's what to fix.
    let ordered = list.materials.iter().filter(|m| m.material.is_none()).chain(list.materials.iter().filter(|m| m.material.is_some()));
    for material in ordered {
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
            ui.painter().rect_filled(rect, 2.0, color32(doc, material));
            match material.material {
                None => {
                    ui.colored_label(ui.visuals().warn_fg_color, egui::RichText::new("Needs a material").strong());
                    ui.weak(plural(material.part_count(), "part"));
                }
                Some(id) => {
                    ui.strong(&material.name);
                    ui.weak(format!("{}  ·  {}  ·  {}", kind_name(material), plural(material.part_count(), "part"), amount(material)));
                    if ui.small_button(format!("{} Edit material", ph::PENCIL_SIMPLE)).clicked() {
                        *edit = Some(id);
                    }
                }
            }
        });
        egui::Grid::new(("parts", &material.name)).striped(true).num_columns(7).spacing([16.0, 4.0]).show(ui, |ui| {
            for heading in ["", "Name", "Qty", "Length", "Width", "Thickness", "Stock"] {
                ui.strong(heading);
            }
            ui.end_row();
            for part in &material.parts {
                ui.strong(&part.label);
                ui.label(part.names.join(", "));
                ui.label(part.count.to_string());
                ui.label(format_length(part.length));
                ui.label(format_length(part.width));
                ui.label(format_length(part.thickness));
                // The standard stock it's cut from, or why there's none (flagged).
                match material.kind.map(|kind| match_stock(part, kind, &material.standard)) {
                    Some(Ok(stock)) => {
                        ui.label(stock.name);
                    }
                    Some(Err(reason)) if material.kind.is_some_and(|k| !matches!(k, MaterialKind::Hardware | MaterialKind::Other)) => {
                        ui.colored_label(ui.visuals().warn_fg_color, format!("{} none", ph::WARNING)).on_hover_text(reason);
                    }
                    _ => {
                        ui.weak("—");
                    }
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

/// `1 part`, `3 parts`.
fn plural(count: usize, noun: &str) -> String {
    if count == 1 { format!("1 {noun}") } else { format!("{count} {noun}s") }
}
