//! The window around the viewport, laid out the way SketchUp users expect on Windows: a menu
//! bar, a toolbar of standard and view buttons, the two-column Large Tool Set down the left,
//! the Default Tray of collapsible panels on the right, and the status bar with the
//! Measurements box at the bottom right.

use eframe::egui;
use woodsketch_doc::DefinitionKind;
use woodsketch_doc::export::ExportUnits;
use woodsketch_tools::{ToolId, Tools};
use woodsketch_view::StandardView;

use crate::{WoodSketchApp, files, icons, outliner, tags};

const BUTTON_SIZE: f32 = 32.0;

/// The Large Tool Set, two columns, grouped as in SketchUp.
const TOOL_SET: &[&[ToolId]] = &[
    &[ToolId::Select, ToolId::Eraser, ToolId::PaintBucket],
    &[ToolId::Line, ToolId::Arc, ToolId::Rectangle, ToolId::Circle, ToolId::Board],
    &[ToolId::PushPull, ToolId::Move, ToolId::Rotate, ToolId::Scale, ToolId::Offset],
    &[ToolId::TapeMeasure],
    &[ToolId::Orbit, ToolId::Pan, ToolId::Zoom],
];

const VIEWS: [(&str, StandardView); 4] = [
    ("Iso", StandardView::Iso),
    ("Top", StandardView::Top),
    ("Front", StandardView::Front),
    ("Right", StandardView::Right),
];

/// A light theme in the spirit of SketchUp's Windows interface: pale grey chrome, white fields,
/// a soft blue selection.
pub fn visuals() -> egui::Visuals {
    let mut v = egui::Visuals::light();
    v.panel_fill = egui::Color32::from_gray(240);
    v.window_fill = egui::Color32::from_gray(248);
    v.extreme_bg_color = egui::Color32::WHITE;
    v.faint_bg_color = egui::Color32::from_gray(232);
    v.selection.bg_fill = egui::Color32::from_rgb(204, 228, 247);
    v.selection.stroke = egui::Stroke::new(1.0, egui::Color32::from_rgb(0, 84, 153));
    v.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, egui::Color32::from_gray(210));
    v
}

impl WoodSketchApp {
    pub(crate) fn menu_bar(&mut self, ui: &mut egui::Ui) {
        egui::Panel::top("menu bar").show(ui, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                self.file_menu(ui);
                self.edit_menu(ui);
                self.view_menu(ui);
                self.camera_menu(ui);
                tool_menu(ui, &mut self.tools, "Draw", &[ToolId::Line, ToolId::Arc, ToolId::Rectangle, ToolId::Circle, ToolId::Board]);
                self.tools_menu(ui);
                self.window_menu(ui);
                self.help_menu(ui);
            });
        });
    }

    fn file_menu(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        ui.menu_button("File", |ui| {
            if ui.add(egui::Button::new("New").shortcut_text("Ctrl+N")).clicked() {
                ui.close();
                self.request(&ctx, files::Discard::New);
            }
            if ui.add(egui::Button::new("Open...").shortcut_text("Ctrl+O")).clicked() {
                ui.close();
                self.request(&ctx, files::Discard::Open);
            }
            ui.add_enabled_ui(!self.recent.is_empty(), |ui| {
                ui.menu_button("Open Recent", |ui| {
                    let mut open = None;
                    for (i, path) in self.recent.iter().enumerate() {
                        let name = path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
                        if ui.button(format!("{} {name}", i + 1)).on_hover_text(path.display().to_string()).clicked() {
                            open = Some(i);
                        }
                    }
                    if let Some(i) = open {
                        ui.close();
                        self.request(&ctx, files::Discard::OpenRecent(i));
                    }
                });
            });
            ui.separator();
            if ui.add(egui::Button::new("Save").shortcut_text("Ctrl+S")).clicked() {
                ui.close();
                self.save(false);
            }
            if ui.add(egui::Button::new("Save As...").shortcut_text("Ctrl+Shift+S")).clicked() {
                ui.close();
                self.save(true);
            }
            ui.separator();
            ui.menu_button("Export", |ui| {
                for (label, format) in [("OBJ...", files::ExportFormat::Obj), ("STL...", files::ExportFormat::Stl)] {
                    if ui.button(label).clicked() {
                        ui.close();
                        if let Some(path) = files::export_dialog(self.files.path.as_deref(), format)
                            && let Err(error) = self.export_to(&path, format)
                        {
                            self.files.show_error(error);
                        }
                    }
                }
                ui.separator();
                ui.label("Units");
                ui.radio_value(&mut self.export_units, ExportUnits::Inches, "Inches");
                ui.radio_value(&mut self.export_units, ExportUnits::Millimeters, "Millimeters");
            });
            ui.separator();
            if ui.button("Exit").clicked() {
                ui.close();
                self.request(&ctx, files::Discard::Close);
            }
        });
    }

    fn edit_menu(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("Edit", |ui| {
            if ui.add(egui::Button::new("Undo").shortcut_text("Ctrl+Z")).clicked() {
                ui.close();
                self.undo();
            }
            if ui.add(egui::Button::new("Redo").shortcut_text("Ctrl+Y")).clicked() {
                ui.close();
                self.redo();
            }
            ui.separator();
            let selected = !self.doc.selection().is_empty();
            if ui.add_enabled(selected, egui::Button::new("Cut").shortcut_text("Ctrl+X")).clicked() {
                ui.close();
                self.tools.cut(&mut self.doc);
                crate::keys::note_copy(ui.ctx(), &self.tools);
            }
            if ui.add_enabled(selected, egui::Button::new("Copy").shortcut_text("Ctrl+C")).clicked() {
                ui.close();
                self.tools.copy(&self.doc);
                crate::keys::note_copy(ui.ctx(), &self.tools);
            }
            let can_paste = self.tools.can_paste();
            if ui.add_enabled(can_paste, egui::Button::new("Paste").shortcut_text("Ctrl+V")).on_hover_text("Follows the mouse; click to put it down").clicked() {
                ui.close();
                self.tools.paste(&mut self.doc);
            }
            if ui.add_enabled(can_paste, egui::Button::new("Paste in Place").shortcut_text("Ctrl+Shift+V")).clicked() {
                ui.close();
                self.tools.paste_in_place(&mut self.doc);
            }
            if ui.add_enabled(selected, egui::Button::new("Duplicate").shortcut_text("Ctrl+D")).on_hover_text("A copy right beside it").clicked() {
                ui.close();
                self.tools.duplicate(&mut self.doc);
            }
            ui.separator();
            if ui.add(egui::Button::new("Delete").shortcut_text("Delete")).clicked() {
                ui.close();
                self.tools.cancel();
                self.doc.erase_selection();
            }
            if ui.button("Delete Guides").clicked() {
                ui.close();
                self.tools.delete_guides(&mut self.doc);
            }
            ui.separator();
            if ui.add(egui::Button::new("Select All").shortcut_text("Ctrl+A")).clicked() {
                ui.close();
                self.doc.select_all();
            }
            if ui.button("Select None").clicked() {
                ui.close();
                self.doc.selection_mut().clear();
            }
            ui.separator();
            if ui.add(egui::Button::new("Make Group").shortcut_text("Ctrl+G")).clicked() {
                ui.close();
                self.tools.make_group(&mut self.doc, DefinitionKind::Group);
            }
            if ui.add(egui::Button::new("Make Component...").shortcut_text("G")).clicked() {
                ui.close();
                self.tools.make_group(&mut self.doc, DefinitionKind::Component);
            }
            if ui.button("Explode").clicked() {
                ui.close();
                self.tools.explode(&mut self.doc);
            }
        });
    }

    fn view_menu(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("View", |ui| {
            ui.checkbox(&mut self.viewport.show_grid, "Grid");
            ui.checkbox(&mut self.show_tray, "Default Tray");
        });
    }

    fn camera_menu(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("Camera", |ui| {
            ui.menu_button("Standard Views", |ui| {
                for (name, view) in VIEWS {
                    if ui.button(name).clicked() {
                        ui.close();
                        self.viewport.camera.set_view(view);
                    }
                }
            });
            ui.separator();
            for tool in [ToolId::Orbit, ToolId::Pan, ToolId::Zoom] {
                tool_item(ui, &mut self.tools, tool);
            }
            if ui.add(egui::Button::new("Zoom Extents").shortcut_text("Shift+Z")).clicked() {
                ui.close();
                self.viewport.request_zoom_extents();
            }
        });
    }

    fn tools_menu(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("Tools", |ui| {
            for tool in [ToolId::Select, ToolId::Eraser, ToolId::PaintBucket] {
                tool_item(ui, &mut self.tools, tool);
            }
            ui.separator();
            for tool in [ToolId::Move, ToolId::Rotate, ToolId::Scale, ToolId::PushPull, ToolId::Offset] {
                tool_item(ui, &mut self.tools, tool);
            }
            ui.separator();
            tool_item(ui, &mut self.tools, ToolId::TapeMeasure);
            ui.separator();
            if ui.button("Cut List...").clicked() {
                ui.close();
                self.cut_list.open = true;
            }
        });
    }

    fn window_menu(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("Window", |ui| {
            ui.checkbox(&mut self.show_tray, "Default Tray");
            ui.checkbox(&mut self.cut_list.open, "Cut List");
        });
    }

    fn help_menu(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("Help", |ui| {
            if ui.button("Check for Updates...").clicked() {
                ui.close();
                self.check_for_updates();
            }
            ui.checkbox(&mut self.check_updates, "Check for updates at startup")
                .on_hover_text("A quick look at WoodSketch's releases on GitHub when it opens; new versions download in the background");
            ui.separator();
            if ui.button("About WoodSketch").clicked() {
                ui.close();
                self.show_about = true;
            }
        });
    }

    pub(crate) fn about_window(&mut self, ctx: &egui::Context) {
        egui::Window::new("About WoodSketch")
            .open(&mut self.show_about)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label(format!("WoodSketch {}", env!("CARGO_PKG_VERSION")));
                ui.label("A lightweight 3D modeler for woodworking, in the spirit of SketchUp.");
                ui.label("Not affiliated with or endorsed by Trimble.");
            });
    }

    /// Standard and view buttons under the menu bar.
    pub(crate) fn toolbar(&mut self, ui: &mut egui::Ui) {
        egui::Panel::top("toolbar").show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                let ctx = ui.ctx().clone();
                if icon_button(ui, icons::NEW, false, true, "New (Ctrl+N)") {
                    self.request(&ctx, files::Discard::New);
                }
                if icon_button(ui, icons::OPEN, false, true, "Open (Ctrl+O)") {
                    self.request(&ctx, files::Discard::Open);
                }
                if icon_button(ui, icons::SAVE, false, true, "Save (Ctrl+S)") {
                    self.save(false);
                }
                ui.separator();
                if icon_button(ui, icons::UNDO, false, true, "Undo (Ctrl+Z)") {
                    self.undo();
                }
                if icon_button(ui, icons::REDO, false, true, "Redo (Ctrl+Y)") {
                    self.redo();
                }
                ui.separator();
                if icon_button(ui, icons::ZOOM_EXTENTS, false, true, "Zoom Extents (Shift+Z)") {
                    self.viewport.request_zoom_extents();
                }
                for (name, view) in VIEWS {
                    let label = if view == StandardView::Iso { format!("{} {name}", icons::ISO) } else { name.into() };
                    let response = ui
                        .add(egui::Button::selectable(false, label).min_size(egui::vec2(0.0, BUTTON_SIZE)))
                        .on_hover_text(format!("{name} view"));
                    if response.clicked() {
                        response.surrender_focus();
                        self.viewport.camera.set_view(view);
                    }
                }
                ui.separator();
                let open = self.cut_list.open;
                if icon_button(ui, icons::CUT_LIST, open, true, "Cut List: parts and cutting diagrams") {
                    self.cut_list.open = !open;
                }
            });
        });
    }

    /// SketchUp's Large Tool Set: every tool, two to a row.
    pub(crate) fn tool_set(&mut self, ui: &mut egui::Ui) {
        // Two buttons and the gap between them, plus the panel margins.
        let width = BUTTON_SIZE * 2.0 + 2.0 + 16.0;
        egui::Panel::left("tool set").resizable(false).exact_size(width).show(ui, |ui| {
            ui.spacing_mut().item_spacing = egui::vec2(2.0, 2.0);
            for (i, group) in TOOL_SET.iter().enumerate() {
                if i > 0 {
                    ui.separator();
                }
                for pair in group.chunks(2) {
                    ui.horizontal(|ui| {
                        for &tool in pair {
                            let hint = format!("{} ({})", tool.name(), tool.shortcut_label());
                            let hint = if tool.is_available() { hint } else { format!("{hint}: coming soon") };
                            let selected = self.tools.active() == tool;
                            if icon_button(ui, icons::tool(tool), selected, tool.is_available(), &hint) {
                                self.tools.activate(tool);
                            }
                        }
                    });
                }
            }
        });
    }

    /// SketchUp's Default Tray: Entity Info, Materials, Tags and Outliner, each collapsible.
    pub(crate) fn tray(&mut self, ui: &mut egui::Ui) {
        if !self.show_tray {
            return;
        }
        egui::Panel::right("default tray").default_size(250.0).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.strong("Default Tray");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button(icons::CLOSE).on_hover_text("Hide the tray (Window > Default Tray)").clicked() {
                        self.show_tray = false;
                    }
                });
            });
            ui.separator();
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                if self.tools.active() == ToolId::Board {
                    tray_section(ui, "Board", |ui| crate::board::show(ui, &mut self.doc, &mut self.tools));
                }
                tray_section(ui, "Entity Info", |ui| tags::entity_info(ui, &mut self.doc, &mut self.tools));
                if let Some(pair) = crate::joinery::selected_pair(&self.doc) {
                    tray_section(ui, "Joinery", |ui| crate::joinery::show(ui, &mut self.doc, pair));
                }
                if let [woodsketch_doc::Entity::Instance(part)] = self.doc.selection().iter().collect::<Vec<_>>()[..] {
                    if let Some(made) = self.doc.template_of(part) {
                        tray_section(ui, &format!("{} sizes", made.template.name()), |ui| crate::template::change(ui, &mut self.doc, part, made));
                    }
                    tray_section(ui, "Edge profile", |ui| crate::profile::show(ui, &mut self.doc, part));
                }
                // Folded until wanted: making a new cabinet, drawer, face frame or table.
                egui::CollapsingHeader::new(egui::RichText::new("Templates").strong())
                    .id_salt("Templates")
                    .default_open(false)
                    .show(ui, |ui| {
                        if crate::template::new(ui, &mut self.doc) {
                            self.viewport.request_zoom_extents();
                        }
                    });
                tray_section(ui, "Materials", |ui| self.materials.show(ui, &mut self.doc, &mut self.tools));
                tray_section(ui, "Tags", |ui| self.tags.show(ui, &mut self.doc));
                tray_section(ui, "Outliner", |ui| outliner::show(ui, &mut self.doc, &mut self.tools));
            });
        });
    }

    pub(crate) fn status_bar(&mut self, ui: &mut egui::Ui) {
        egui::Panel::bottom("status").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    measurements_box(ui, &self.tools);
                    ui.label(self.tools.measurement_label());
                    ui.separator();
                    // A new version, downloaded in the background: offer the restart, never force it.
                    if let Some(crate::updates::UpdateState::Ready(version)) = self.update_state() {
                        let restart = ui
                            .button(format!("{} Restart to update to {version}", egui_phosphor::regular::ARROW_CIRCLE_UP))
                            .on_hover_text("Installs the new version and reopens WoodSketch (you'll be asked to save first)");
                        if restart.clicked() {
                            let ctx = ui.ctx().clone();
                            self.restart_to_update(&ctx);
                        }
                        ui.separator();
                    }
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.label(egui::RichText::new(icons::HELP).color(egui::Color32::from_rgb(0, 102, 204)));
                        ui.add(egui::Label::new(self.tools.status()).truncate());
                    });
                });
            });
        });
    }

    fn undo(&mut self) {
        self.tools.cancel();
        self.doc.undo();
    }

    fn redo(&mut self) {
        self.tools.cancel();
        self.doc.redo();
    }
}

/// A tray panel: a bar with the panel's name that folds it open and closed.
fn tray_section(ui: &mut egui::Ui, title: &str, contents: impl FnOnce(&mut egui::Ui)) {
    egui::CollapsingHeader::new(egui::RichText::new(title).strong())
        .id_salt(title)
        .default_open(true)
        .show(ui, contents);
}

/// A menu of tools, each with its shortcut.
fn tool_menu(ui: &mut egui::Ui, tools: &mut Tools, title: &str, list: &[ToolId]) {
    ui.menu_button(title, |ui| {
        for &tool in list {
            tool_item(ui, tools, tool);
        }
    });
}

fn tool_item(ui: &mut egui::Ui, tools: &mut Tools, tool: ToolId) {
    let button = egui::Button::new(tool.name()).shortcut_text(tool.shortcut_label());
    if ui.add_enabled(tool.is_available(), button).clicked() {
        ui.close();
        tools.activate(tool);
    }
}

/// A square icon button with a tooltip. Returns whether it was clicked.
fn icon_button(ui: &mut egui::Ui, icon: &str, selected: bool, enabled: bool, hint: &str) -> bool {
    let button = egui::Button::selectable(selected, egui::RichText::new(icon).size(20.0))
        .min_size(egui::vec2(BUTTON_SIZE, BUTTON_SIZE));
    let response = ui.add_enabled(enabled, button);
    let response = if enabled { response.on_hover_text(hint) } else { response.on_disabled_hover_text(hint) };
    if response.clicked() {
        // Don't keep keyboard focus, or Space would press the button again.
        response.surrender_focus();
        return true;
    }
    false
}

/// SketchUp's Measurements box: shows live values, and whatever is being typed.
fn measurements_box(ui: &mut egui::Ui, tools: &Tools) {
    let text = tools.measurement();
    let color = if tools.is_typing() { ui.visuals().strong_text_color() } else { ui.visuals().text_color() };
    egui::Frame::new()
        .fill(ui.visuals().extreme_bg_color)
        .stroke(egui::Stroke::new(1.0, egui::Color32::from_gray(170)))
        .corner_radius(2.0)
        .inner_margin(egui::Margin::symmetric(6, 2))
        .show(ui, |ui| {
            ui.set_min_width(150.0);
            ui.label(egui::RichText::new(text).monospace().color(color));
        });
}
