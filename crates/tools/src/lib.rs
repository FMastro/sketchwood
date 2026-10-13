//! Tools and their state machines, independent of the UI toolkit. The UI translates pointer and
//! keyboard input into calls on [`Tools`], and draws what [`Tools::overlay`] returns.

pub mod pick;
mod move_tool;
mod push_pull;
mod rotate;
mod rectangle;
mod select;

use glam::{DVec3, Vec2};
use sketchcraft_doc::Document;
use sketchcraft_view::OrbitCamera;

pub use pick::{Snap, SnapKind};
use pick::Screen;
use move_tool::MoveTool;
use push_pull::PushPullTool;
use rotate::RotateTool;
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
        matches!(
            self,
            Self::Select
                | Self::Rectangle
                | Self::PushPull
                | Self::Move
                | Self::Rotate
                | Self::Orbit
                | Self::Pan
                | Self::Zoom
        )
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
    /// A rubber band from a base point, colored by the axis it follows.
    pub guide: Option<Guide>,
    pub protractor: Option<Protractor>,
    /// Rotation grips, and whether each is under the cursor.
    pub grips: Vec<(DVec3, bool)>,
    /// The inference point under the cursor.
    pub snap: Option<Snap>,
    pub window: Option<SelectionWindow>,
}

/// The model axes, in SketchUp's colors: red X, green Y, blue Z.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    X,
    Y,
    Z,
}

impl Axis {
    pub const ALL: [Self; 3] = [Self::X, Self::Y, Self::Z];

    pub fn dir(self) -> DVec3 {
        match self {
            Self::X => DVec3::X,
            Self::Y => DVec3::Y,
            Self::Z => DVec3::Z,
        }
    }

    /// SketchUp-style inference tooltip.
    pub fn label(self) -> &'static str {
        match self {
            Self::X => "On Red Axis",
            Self::Y => "On Green Axis",
            Self::Z => "On Blue Axis",
        }
    }
}

/// SketchUp's protractor: a circle in the plane through `center` with `normal`, with rays to
/// the starting direction and the current one.
#[derive(Clone, Copy, Debug)]
pub struct Protractor {
    pub center: DVec3,
    pub normal: DVec3,
    pub from: Option<DVec3>,
    pub to: Option<DVec3>,
}

/// How copies are laid out: `count` copies at multiples of the move or turn, or (divided)
/// spread evenly up to it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Array {
    pub count: u32,
    pub divide: bool,
}

/// `3x`, `x3`, `*3` → 3 copies; `/3` → divide into 3.
pub(crate) fn parse_array(typed: &str) -> Option<Array> {
    let t = typed.trim().to_ascii_lowercase();
    let (number, divide) = match t.strip_prefix('/') {
        Some(n) => (n, true),
        None => (t.strip_prefix('*').or_else(|| t.strip_prefix('x')).or_else(|| t.strip_suffix('x'))?, false),
    };
    let count: u32 = number.trim().parse().ok()?;
    (count >= 1).then_some(Array { count, divide })
}

/// Outline an entity, e.g. what a tool would pick up.
pub(crate) fn highlight_entity(model: &sketchcraft_doc::Model, entity: sketchcraft_doc::Entity, overlay: &mut Overlay) {
    use sketchcraft_doc::Entity;
    match entity {
        Entity::Edge(id) => {
            if let Some(edge) = model.edge(id) {
                overlay.highlight.push(model.edge_points(edge));
            }
        }
        Entity::Face(id) => {
            for edge in model.face_edges(id).into_iter().filter_map(|e| model.edge(e)) {
                overlay.highlight.push(model.edge_points(edge));
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Guide {
    pub from: DVec3,
    pub to: DVec3,
    pub axis: Option<Axis>,
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
    move_tool: MoveTool,
    rotate: RotateTool,
    /// A grip rotation started by press-and-drag; finishes on release.
    grip_dragging: bool,
    /// Move just rotated with a grip, so typed angles and arrays go to that rotation.
    grip_rotated: bool,
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
            move_tool: MoveTool::default(),
            rotate: RotateTool::default(),
            grip_dragging: false,
            grip_rotated: false,
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

    /// Move's rotation grips: a click or press on a grip rotates instead of moving, using the
    /// Rotate tool's logic. Returns `None` when the event is ordinary Move input.
    fn grip_pointer(&mut self, event: PointerEvent, doc: &mut Document, screen: Screen<'_>) -> Option<Result<(), String>> {
        if self.rotate.is_busy() {
            return Some(match event {
                PointerEvent::Move(pos) | PointerEvent::Drag { pos, .. } => {
                    self.rotate.hover(doc, screen, pos);
                    Ok(())
                }
                PointerEvent::Click { pos, .. } => self.rotate.click(doc, screen, pos),
                // Press on a grip, drag to turn, release to finish.
                PointerEvent::DragEnd { pos, .. } if self.grip_dragging => {
                    self.grip_dragging = false;
                    self.rotate.click(doc, screen, pos)
                }
                _ => Ok(()),
            });
        }
        let pos = match event {
            PointerEvent::Click { pos, .. } => pos,
            PointerEvent::DragStart { origin } => origin,
            _ => return None,
        };
        // A new click or press ends the previous grip rotation's chance to be retyped.
        self.grip_rotated = false;
        let grip = self.move_tool.grip_under(doc, screen, pos)?;
        self.typed.clear();
        self.rotate.begin(grip.entities, grip.vertices, grip.center, grip.normal, grip.from);
        self.grip_dragging = matches!(event, PointerEvent::DragStart { .. });
        self.grip_rotated = true;
        Some(Ok(()))
    }

    /// Whether Move is turning with a grip, or just did (so typing changes that rotation).
    fn grip_active(&self) -> bool {
        self.active == ToolId::Move && (self.rotate.is_busy() || self.grip_rotated)
    }

    /// Abandon whatever the active tool is in the middle of.
    pub fn cancel(&mut self) {
        self.grip_dragging = false;
        self.grip_rotated = false;
        self.typed.clear();
        self.message = None;
        self.select.cancel();
        self.rectangle.cancel();
        self.push_pull.cancel();
        self.move_tool.cancel();
        self.rotate.cancel();
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
        if self.active == ToolId::Move
            && let Some(result) = self.grip_pointer(event, doc, screen)
        {
            if let Err(message) = result {
                self.message = Some(message);
            }
            return;
        }
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
            (ToolId::Move, PointerEvent::Move(pos) | PointerEvent::Drag { pos, .. }) => {
                self.move_tool.hover(doc, screen, pos);
                Ok(())
            }
            (ToolId::Move, PointerEvent::Click { pos, .. }) => {
                self.typed.clear();
                self.move_tool.click(doc, screen, pos)
            }
            (ToolId::Move, PointerEvent::DragStart { origin }) => self.move_tool.drag_start(doc, screen, origin),
            (ToolId::Move, PointerEvent::DragEnd { pos, .. }) => self.move_tool.drag_end(doc, screen, pos),
            (ToolId::Rotate, PointerEvent::Move(pos) | PointerEvent::Drag { pos, .. }) => {
                self.rotate.hover(doc, screen, pos);
                Ok(())
            }
            (ToolId::Rotate, PointerEvent::Click { pos, .. }) => {
                self.typed.clear();
                self.rotate.click(doc, screen, pos)
            }
            (ToolId::Rotate, PointerEvent::DragStart { origin }) => self.rotate.drag_start(doc, screen, origin),
            (ToolId::Rotate, PointerEvent::DragEnd { pos, .. }) => self.rotate.drag_end(doc, screen, pos),
            _ => Ok(()),
        };
        if let Err(message) = result {
            self.message = Some(message);
        }
    }

    /// A tap of Ctrl on its own: toggles copying in Move.
    pub fn ctrl_tap(&mut self) {
        match self.active {
            ToolId::Move if self.rotate.is_busy() => self.rotate.toggle_copy(),
            ToolId::Move => self.move_tool.toggle_copy(),
            ToolId::Rotate => self.rotate.toggle_copy(),
            _ => {}
        }
    }

    /// Arrow keys lock drawing and moving to an axis (pressing again unlocks).
    pub fn lock_axis(&mut self, axis: Axis) {
        match self.active {
            ToolId::Move => self.move_tool.lock_axis(axis),
            ToolId::Rotate => self.rotate.lock_axis(axis),
            _ => {}
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
            ToolId::Move if self.grip_active() => self.rotate.enter(doc, &typed),
            ToolId::Move => self.move_tool.enter(doc, &typed),
            ToolId::Rotate => self.rotate.enter(doc, &typed),
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
        } else if self.select.is_busy() || self.rectangle.is_busy() || self.push_pull.is_busy() || self.move_tool.is_busy() || self.rotate.is_busy() {
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
            ToolId::Move if self.rotate.is_busy() => self.rotate.status(),
            ToolId::Move => self.move_tool.status(),
            ToolId::Rotate => self.rotate.status(),
            ToolId::Orbit => "Drag to orbit.".into(),
            ToolId::Pan => "Drag to pan.".into(),
            ToolId::Zoom => "Drag up to zoom in, down to zoom out.".into(),
            _ => String::new(),
        }
    }

    pub fn measurement_label(&self) -> &'static str {
        match self.active {
            ToolId::Rectangle => "Dimensions",
            ToolId::Move if self.grip_active() => "Angle",
            ToolId::PushPull | ToolId::Move => "Distance",
            ToolId::Rotate => "Angle",
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
            ToolId::Move if self.grip_active() => self.rotate.measurement(),
            ToolId::Move => self.move_tool.measurement(),
            ToolId::Rotate => self.rotate.measurement(),
            _ => String::new(),
        }
    }

    pub fn overlay(&self, doc: &Document) -> Overlay {
        let mut overlay = Overlay::default();
        match self.active {
            ToolId::Select => self.select.overlay(&mut overlay),
            ToolId::Rectangle => self.rectangle.overlay(&mut overlay),
            ToolId::PushPull => self.push_pull.overlay(doc.model(), &mut overlay),
            ToolId::Move if self.rotate.is_busy() => self.rotate.overlay(doc, &mut overlay),
            ToolId::Move => self.move_tool.overlay(doc, &mut overlay),
            ToolId::Rotate => self.rotate.overlay(doc, &mut overlay),
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
        tools.activate(ToolId::Line);
        assert_eq!(tools.active(), ToolId::Select);
        assert!(tools.status().contains("coming soon"));
        tools.activate(ToolId::Rectangle);
        assert_eq!(tools.active(), ToolId::Rectangle);
        assert_eq!(tools.status(), "Select first corner.");
    }

    #[test]
    fn move_rotation_grip_turns_a_board_in_place() {
        use sketchcraft_view::StandardView;
        let mut doc = Document::default();
        doc.edit(|m| {
            let face = m.add_rectangle(DVec3::new(10.0, 0.0, 0.0), DVec3::X * 1.5, DVec3::Y * 3.5, DVec3::Z).unwrap();
            m.push_pull(face, 10.0).unwrap();
        });
        let mut camera =
            OrbitCamera { target: glam::Vec3::new(10.75, 1.75, 5.0), distance: 60.0, ..Default::default() };
        camera.set_view(StandardView::Iso);
        let size = Vec2::new(1000.0, 800.0);
        let mut tools = Tools::default();
        tools.activate(ToolId::Move);

        // Hovering the board shows grips on the box faces facing the camera.
        let middle = camera.project(glam::Vec3::new(10.75, 0.0, 5.0), size).unwrap();
        tools.pointer(PointerEvent::Move(middle), &mut doc, &mut camera, size);
        let overlay = tools.overlay(&doc);
        assert_eq!(overlay.grips.len(), 12, "3 visible box faces x 4 grips");

        // Click a grip on the top face (z = 10) and turn a quarter.
        let top_grip = overlay.grips.iter().map(|(p, _)| *p).find(|p| (p.z - 10.0).abs() < 1e-9).unwrap();
        let at = |p: DVec3, camera: &OrbitCamera| camera.project(p.as_vec3(), size).unwrap();
        tools.pointer(PointerEvent::Move(at(top_grip, &camera)), &mut doc, &mut camera, size);
        assert!(tools.overlay(&doc).protractor.is_some());
        let mods = Modifiers::default();
        tools.pointer(PointerEvent::Click { pos: at(top_grip, &camera), count: 1, mods }, &mut doc, &mut camera, size);
        assert_eq!(tools.measurement_label(), "Angle");
        let center = DVec3::new(10.75, 1.75, 10.0);
        let quarter = center + (top_grip - center).cross(DVec3::Z) * -1.0;
        tools.pointer(PointerEvent::Move(at(quarter, &camera)), &mut doc, &mut camera, size);
        for c in "90".chars() {
            tools.type_char(c);
        }
        tools.enter(&mut doc);

        // Turned about its own vertical center line: same footprint center, swapped sides.
        let (lo, hi) = doc.model().bounds().unwrap();
        assert!(((lo + hi) * 0.5).abs_diff_eq(DVec3::new(10.75, 1.75, 5.0), 1e-9), "{lo} {hi}");
        assert!((hi - lo).abs_diff_eq(DVec3::new(3.5, 1.5, 10.0), 1e-9), "{}", hi - lo);
        assert_eq!(tools.active(), ToolId::Move);
        assert_eq!(tools.measurement(), "90.0°");
        // Typing again changes that rotation, as with the Rotate tool.
        for c in "45".chars() {
            tools.type_char(c);
        }
        tools.enter(&mut doc);
        let (lo, hi) = doc.model().bounds().unwrap();
        assert!((hi - lo).x > 3.5 && (hi - lo).x < 4.0, "45° footprint {}", hi - lo);
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
