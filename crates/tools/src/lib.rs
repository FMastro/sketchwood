//! Tools and their state machines, independent of the UI toolkit. The UI translates pointer and
//! keyboard input into calls on [`Tools`], and draws what [`Tools::overlay`] returns.

pub mod pick;
mod push_pull;
mod rectangle;
mod select;

use glam::{DVec3, Vec2};
use sketchcraft_doc::Document;
use sketchcraft_view::OrbitCamera;

pub use pick::{Snap, SnapKind};
use pick::Screen;
use push_pull::PushPullTool;
use rectangle::RectangleTool;
use select::SelectTool;

/// Every tool on the toolbar, with SketchUp's default shortcuts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ToolId {
    Select,
    Eraser,
    PaintBucket,
    Line,
    Arc,
    Rectangle,
    Circle,
    PushPull,
    Move,
    Rotate,
    Scale,
    Offset,
    TapeMeasure,
    Orbit,
    Pan,
    Zoom,
}

impl ToolId {
    pub const ALL: [Self; 16] = [
        Self::Select,
        Self::Eraser,
        Self::PaintBucket,
        Self::Line,
        Self::Arc,
        Self::Rectangle,
        Self::Circle,
        Self::PushPull,
        Self::Move,
        Self::Rotate,
        Self::Scale,
        Self::Offset,
        Self::TapeMeasure,
        Self::Orbit,
        Self::Pan,
        Self::Zoom,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Select => "Select",
            Self::Eraser => "Eraser",
            Self::PaintBucket => "Paint Bucket",
            Self::Line => "Line",
            Self::Arc => "Arc",
            Self::Rectangle => "Rectangle",
            Self::Circle => "Circle",
            Self::PushPull => "Push/Pull",
            Self::Move => "Move",
            Self::Rotate => "Rotate",
            Self::Scale => "Scale",
            Self::Offset => "Offset",
            Self::TapeMeasure => "Tape Measure",
            Self::Orbit => "Orbit",
            Self::Pan => "Pan",
            Self::Zoom => "Zoom",
        }
    }

    /// The key that activates the tool (SketchUp defaults).
    pub fn shortcut(self) -> char {
        match self {
            Self::Select => ' ',
            Self::Eraser => 'e',
            Self::PaintBucket => 'b',
            Self::Line => 'l',
            Self::Arc => 'a',
            Self::Rectangle => 'r',
            Self::Circle => 'c',
            Self::PushPull => 'p',
            Self::Move => 'm',
            Self::Rotate => 'q',
            Self::Scale => 's',
            Self::Offset => 'f',
            Self::TapeMeasure => 't',
            Self::Orbit => 'o',
            Self::Pan => 'h',
            Self::Zoom => 'z',
        }
    }

    pub fn shortcut_label(self) -> String {
        match self.shortcut() {
            ' ' => "Space".to_owned(),
            c => c.to_ascii_uppercase().to_string(),
        }
    }

    /// The tool a typed character activates. Uppercase letters are Shift combinations, which
    /// are not tool shortcuts.
    pub fn from_shortcut(c: char) -> Option<Self> {
        Self::ALL.into_iter().find(|tool| tool.shortcut() == c)
    }

    pub fn is_available(self) -> bool {
        matches!(self, Self::Select | Self::Rectangle | Self::PushPull | Self::Orbit | Self::Pan | Self::Zoom)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Modifiers {
    pub shift: bool,
    pub ctrl: bool,
}

/// Left-button pointer input in viewport coordinates (points, origin top-left).
#[derive(Clone, Copy, Debug)]
pub enum PointerEvent {
    Move(Vec2),
    /// Press and release without dragging; `count` is 2 or 3 for double and triple clicks.
    Click { pos: Vec2, count: u8, mods: Modifiers },
    DragStart { origin: Vec2 },
    Drag { pos: Vec2, delta: Vec2 },
    DragEnd { pos: Vec2, mods: Modifiers },
}

/// What tools want drawn on top of the viewport.
#[derive(Default)]
pub struct Overlay {
    /// Preview geometry, e.g. the rectangle being sized.
    pub lines: Vec<[DVec3; 2]>,
    /// Highlighted model edges, e.g. the face Push/Pull would act on.
    pub highlight: Vec<[DVec3; 2]>,
    /// The inference point under the cursor.
    pub snap: Option<Snap>,
    pub window: Option<SelectionWindow>,
}

#[derive(Clone, Copy, Debug)]
pub struct SelectionWindow {
    pub start: Vec2,
    pub end: Vec2,
    /// Dragged right-to-left: selects anything touched, drawn dashed.
    pub crossing: bool,
}

/// Points of vertical drag per zoom step with the Zoom tool.
const ZOOM_DRAG_POINTS_PER_STEP: f32 = 25.0;

pub struct Tools {
    active: ToolId,
    select: SelectTool,
    rectangle: RectangleTool,
    push_pull: PushPullTool,
    /// Measurements box text being typed.
    typed: String,
    /// One-off feedback (errors, "coming soon"), shown until the next action.
    message: Option<String>,
}

impl Default for Tools {
    fn default() -> Self {
        Self {
            active: ToolId::Select,
            select: SelectTool::default(),
            rectangle: RectangleTool::default(),
            push_pull: PushPullTool::default(),
            typed: String::new(),
            message: None,
        }
    }
}

impl Tools {
    pub fn active(&self) -> ToolId {
        self.active
    }

    pub fn activate(&mut self, tool: ToolId) {
        if !tool.is_available() {
            self.message = Some(format!("{} ({}) is coming soon.", tool.name(), tool.shortcut_label()));
            return;
        }
        self.cancel();
        self.active = tool;
    }

    /// Abandon whatever the active tool is in the middle of.
    pub fn cancel(&mut self) {
        self.typed.clear();
        self.message = None;
        self.select.cancel();
        self.rectangle.cancel();
        self.push_pull.cancel();
    }

    pub fn pointer(&mut self, event: PointerEvent, doc: &mut Document, camera: &mut OrbitCamera, size: Vec2) {
        if matches!(event, PointerEvent::Click { .. } | PointerEvent::DragStart { .. }) {
            self.message = None;
        }
        match self.active {
            ToolId::Orbit | ToolId::Pan | ToolId::Zoom => {
                if let PointerEvent::Drag { delta, .. } = event {
                    match self.active {
                        ToolId::Orbit => camera.orbit(delta),
                        ToolId::Pan => camera.pan(delta, size),
                        _ => camera.zoom_at(-delta.y / ZOOM_DRAG_POINTS_PER_STEP, size * 0.5, size),
                    }
                }
                return;
            }
            _ => {}
        }

        let screen = Screen { camera, size };
        let result = match (self.active, event) {
            (ToolId::Select, PointerEvent::Click { pos, count, mods }) => {
                self.select.click(doc, screen, pos, count, mods);
                Ok(())
            }
            (ToolId::Select, PointerEvent::DragStart { origin }) => {
                self.select.drag_start(origin);
                Ok(())
            }
            (ToolId::Select, PointerEvent::Drag { pos, .. }) => {
                self.select.drag(pos);
                Ok(())
            }
            (ToolId::Select, PointerEvent::DragEnd { pos, mods }) => {
                self.select.drag_end(doc, screen, pos, mods);
                Ok(())
            }
            (ToolId::Rectangle, PointerEvent::Move(pos) | PointerEvent::Drag { pos, .. }) => {
                self.rectangle.hover(doc, screen, pos);
                Ok(())
            }
            (ToolId::Rectangle, PointerEvent::Click { pos, .. }) => {
                self.typed.clear();
                self.rectangle.click(doc, screen, pos)
            }
            (ToolId::Rectangle, PointerEvent::DragStart { origin }) => {
                self.rectangle.drag_start(doc, screen, origin);
                Ok(())
            }
            (ToolId::Rectangle, PointerEvent::DragEnd { pos, .. }) => self.rectangle.drag_end(doc, screen, pos),
            (ToolId::PushPull, PointerEvent::Move(pos) | PointerEvent::Drag { pos, .. }) => {
                self.push_pull.hover(doc, screen, pos);
                Ok(())
            }
            (ToolId::PushPull, PointerEvent::Click { pos, count, .. }) => {
                self.typed.clear();
                self.push_pull.click(doc, screen, pos, count)
            }
            (ToolId::PushPull, PointerEvent::DragStart { origin }) => {
                self.push_pull.drag_start(doc, screen, origin);
                Ok(())
            }
            (ToolId::PushPull, PointerEvent::DragEnd { pos, .. }) => self.push_pull.drag_end(doc, screen, pos),
            _ => Ok(()),
        };
        if let Err(message) = result {
            self.message = Some(message);
        }
    }

    /// Whether the user is typing into the Measurements box. While typing, letters go to the
    /// box instead of switching tools (so `38mm` doesn't activate Move).
    pub fn is_typing(&self) -> bool {
        !self.typed.is_empty()
    }

    /// Whether `c` starts a Measurements entry rather than a tool shortcut.
    pub fn starts_value(c: char) -> bool {
        c.is_ascii_digit() || matches!(c, '.' | '-' | '+' | ',' | ';' | '\'' | '"' | '/')
    }

    pub fn type_char(&mut self, c: char) {
        self.message = None;
        self.typed.push(c);
    }

    pub fn backspace(&mut self) {
        self.typed.pop();
    }

    /// Submit the Measurements box.
    pub fn enter(&mut self, doc: &mut Document) {
        let typed = std::mem::take(&mut self.typed);
        if typed.trim().is_empty() {
            return;
        }
        let result = match self.active {
            ToolId::Rectangle => self.rectangle.enter(doc, &typed),
            ToolId::PushPull => self.push_pull.enter(doc, &typed),
            _ => Err(format!("{} doesn't take typed values.", self.active.name())),
        };
        if let Err(message) = result {
            self.message = Some(message);
        }
    }

    /// Esc: clear typing, else cancel the operation in progress, else drop the selection.
    pub fn escape(&mut self, doc: &mut Document) {
        if self.is_typing() {
            self.typed.clear();
        } else if self.select.is_busy() || self.rectangle.is_busy() || self.push_pull.is_busy() {
            self.cancel();
        } else if self.active == ToolId::Select && !doc.selection().is_empty() {
            doc.selection_mut().clear();
        }
    }

    pub fn status(&self) -> String {
        if let Some(message) = &self.message {
            return message.clone();
        }
        match self.active {
            ToolId::Select => "Select objects. Ctrl adds, Shift toggles, Ctrl+Shift removes. \
                               Drag right for window, left for crossing selection."
                .into(),
            ToolId::Rectangle => self.rectangle.status().into(),
            ToolId::PushPull => self.push_pull.status().into(),
            ToolId::Orbit => "Drag to orbit.".into(),
            ToolId::Pan => "Drag to pan.".into(),
            ToolId::Zoom => "Drag up to zoom in, down to zoom out.".into(),
            _ => String::new(),
        }
    }

    pub fn measurement_label(&self) -> &'static str {
        match self.active {
            ToolId::Rectangle => "Dimensions",
            ToolId::PushPull => "Distance",
            _ => "Measurements",
        }
    }

    /// The Measurements box contents: what's being typed, else the active tool's live value.
    pub fn measurement(&self) -> String {
        if self.is_typing() {
            return self.typed.clone();
        }
        match self.active {
            ToolId::Rectangle => self.rectangle.measurement(),
            ToolId::PushPull => self.push_pull.measurement(),
            _ => String::new(),
        }
    }

    pub fn overlay(&self, doc: &Document) -> Overlay {
        let mut overlay = Overlay::default();
        match self.active {
            ToolId::Select => self.select.overlay(&mut overlay),
            ToolId::Rectangle => self.rectangle.overlay(&mut overlay),
            ToolId::PushPull => self.push_pull.overlay(doc.model(), &mut overlay),
            _ => {}
        }
        overlay
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortcuts_are_unique_and_round_trip() {
        for tool in ToolId::ALL {
            assert_eq!(ToolId::from_shortcut(tool.shortcut()), Some(tool));
        }
        assert_eq!(ToolId::from_shortcut(' '), Some(ToolId::Select));
        assert_eq!(ToolId::from_shortcut('R'), None);
    }

    #[test]
    fn shortcut_letters_never_start_values() {
        assert!(ToolId::ALL.iter().all(|t| !Tools::starts_value(t.shortcut())));
    }

    #[test]
    fn unavailable_tools_explain_instead_of_switching() {
        let mut tools = Tools::default();
        tools.activate(ToolId::Move);
        assert_eq!(tools.active(), ToolId::Select);
        assert!(tools.status().contains("coming soon"));
        tools.activate(ToolId::Rectangle);
        assert_eq!(tools.active(), ToolId::Rectangle);
        assert_eq!(tools.status(), "Select first corner.");
    }

    #[test]
    fn escape_clears_typing_before_selection() {
        let mut doc = Document::default();
        doc.edit(|m| m.add_rectangle(DVec3::ZERO, DVec3::X, DVec3::Y, DVec3::Z));
        doc.select_all();
        let mut tools = Tools::default();
        tools.type_char('1');
        tools.escape(&mut doc);
        assert!(!tools.is_typing());
        assert!(!doc.selection().is_empty());
        tools.escape(&mut doc);
        assert!(doc.selection().is_empty());
    }
}
