//! Keyboard input, SketchUp style: just type. Digits and length punctuation go to the
//! Measurements box; letters switch tools (unless a value is being typed); Enter submits.

use eframe::egui;
use sketchcraft_doc::Document;
use sketchcraft_tools::{ToolId, Tools};

use crate::viewport::Viewport;

pub fn handle(ctx: &egui::Context, doc: &mut Document, tools: &mut Tools, viewport: &mut Viewport) {
    // A focused text field gets the keyboard instead.
    if ctx.egui_wants_keyboard_input() {
        return;
    }
    let events = ctx.input(|i| i.events.clone());
    for event in events {
        match event {
            egui::Event::Text(text) => {
                for c in text.chars() {
                    if tools.is_typing() || Tools::starts_value(c) {
                        tools.type_char(c);
                    } else if c == 'Z' {
                        viewport.request_zoom_extents();
                    } else if let Some(tool) = ToolId::from_shortcut(c) {
                        tools.activate(tool);
                    }
                }
            }
            egui::Event::Key { key, pressed: true, modifiers, .. } => {
                let command = modifiers.command;
                match key {
                    egui::Key::Enter => tools.enter(doc),
                    egui::Key::Backspace if tools.is_typing() => tools.backspace(),
                    egui::Key::Escape => tools.escape(doc),
                    egui::Key::Delete if !tools.is_typing() => {
                        tools.cancel();
                        doc.erase_selection();
                    }
                    egui::Key::Z if command && modifiers.shift => redo(doc, tools),
                    egui::Key::Z if command => undo(doc, tools),
                    egui::Key::Y if command => redo(doc, tools),
                    egui::Key::A if command => doc.select_all(),
                    // SketchUp for Windows also binds Zoom Extents to Ctrl+Shift+E.
                    egui::Key::E if command && modifiers.shift => viewport.request_zoom_extents(),
                    _ => {}
                }
            }
            _ => {}
        }
    }
}

fn undo(doc: &mut Document, tools: &mut Tools) {
    tools.cancel();
    doc.undo();
}

fn redo(doc: &mut Document, tools: &mut Tools) {
    tools.cancel();
    doc.redo();
}
