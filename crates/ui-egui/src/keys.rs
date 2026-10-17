//! Keyboard input, SketchUp style: just type. Digits and length punctuation go to the
//! Measurements box; letters switch tools (unless a value is being typed); Enter submits.
//! Arrow keys lock an axis; tapping Ctrl on its own toggles copy in Move.

use eframe::egui;
use woodsketch_doc::{DefinitionKind, Document};
use woodsketch_tools::{Axis, ToolId, Tools};

use crate::viewport::Viewport;

#[derive(Default)]
pub struct Keyboard {
    ctrl_down: bool,
    /// Ctrl went down and nothing else happened yet: releasing it is a tap.
    ctrl_tap_pending: bool,
}

impl Keyboard {
    pub fn handle(&mut self, ctx: &egui::Context, doc: &mut Document, tools: &mut Tools, viewport: &mut Viewport) {
        // The keyboard belongs to the drawing unless a text field is being typed in. Buttons
        // and other widgets don't keep focus: a focused button would swallow shortcuts and
        // re-press itself on Space or Enter.
        if ctx.text_edit_focused() {
            return;
        }
        if let Some(focused) = ctx.memory(|m| m.focused()) {
            ctx.memory_mut(|m| m.surrender_focus(focused));
        }
        let events = ctx.input(|i| i.events.clone());
        for event in events {
            // Follow Ctrl in event order, so even a tap within one frame registers.
            match &event {
                egui::Event::ModifiersChanged(modifiers) => self.track_ctrl(modifiers.command, tools),
                egui::Event::Key { pressed: true, .. } | egui::Event::PointerButton { pressed: true, .. } => {
                    self.ctrl_tap_pending = false;
                }
                _ => {}
            }
            match event {
                egui::Event::Text(text) => {
                    for c in text.chars() {
                        if tools.is_typing() || Tools::starts_value(c) {
                            tools.type_char(c);
                        } else if c == 'g' {
                            // SketchUp's Make Component shortcut.
                            tools.make_group(doc, DefinitionKind::Component);
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
                        // SketchUp's axis-lock keys.
                        egui::Key::ArrowRight => tools.lock_axis(Axis::X),
                        egui::Key::ArrowLeft => tools.lock_axis(Axis::Y),
                        egui::Key::ArrowUp | egui::Key::ArrowDown => tools.lock_axis(Axis::Z),
                        egui::Key::Z if command && modifiers.shift => redo(doc, tools),
                        egui::Key::Z if command => undo(doc, tools),
                        egui::Key::Y if command => redo(doc, tools),
                        egui::Key::A if command => doc.select_all(),
                        egui::Key::G if command => tools.make_group(doc, DefinitionKind::Group),
                        // SketchUp for Windows also binds Zoom Extents to Ctrl+Shift+E.
                        egui::Key::E if command && modifiers.shift => viewport.request_zoom_extents(),
                        _ => {}
                    }
                }
                _ => {}
            }
        }
    }

    /// egui reports no key events for modifiers alone, so a Ctrl tap is Ctrl going down, then
    /// up, with no key press or click in between.
    fn track_ctrl(&mut self, ctrl: bool, tools: &mut Tools) {
        match (self.ctrl_down, ctrl) {
            (false, true) => self.ctrl_tap_pending = true,
            (true, false) if self.ctrl_tap_pending => {
                self.ctrl_tap_pending = false;
                tools.ctrl_tap();
            }
            _ => {}
        }
        self.ctrl_down = ctrl;
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
