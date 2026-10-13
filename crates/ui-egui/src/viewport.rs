//! The 3D viewport widget: renders through [`ViewportRenderer`] into a texture shown as an
//! image, feeds pointer input to the tools, and draws their overlays.

use eframe::egui;
use glam::{DVec3, Vec2};
use sketchcraft_doc::Document;
use sketchcraft_render::{Scene, ViewportRenderer};
use sketchcraft_tools::{Modifiers, Overlay, PointerEvent, SnapKind, Tools};
use sketchcraft_view::OrbitCamera;

/// Scroll distance, in points, that counts as one zoom step (about one mouse-wheel notch).
const POINTS_PER_ZOOM_STEP: f32 = 50.0;

pub struct Viewport {
    pub camera: OrbitCamera,
    renderer: ViewportRenderer,
    texture: Option<egui::TextureId>,
    /// The document version the renderer's scene was built from.
    scene_version: Option<u64>,
    zoom_extents_pending: bool,
    last_hover: Option<Vec2>,
    /// Where the viewport was last laid out, in window points.
    rect: egui::Rect,
}

impl Viewport {
    pub fn new(render_state: &egui_wgpu::RenderState) -> Self {
        Self {
            camera: OrbitCamera::default(),
            renderer: ViewportRenderer::new(&render_state.device, &Scene::reference()),
            texture: None,
            scene_version: None,
            zoom_extents_pending: false,
            last_hover: None,
            rect: egui::Rect::NOTHING,
        }
    }

    pub fn request_zoom_extents(&mut self) {
        self.zoom_extents_pending = true;
    }

    pub fn show(&mut self, ui: &mut egui::Ui, frame: &eframe::Frame, doc: &mut Document, tools: &mut Tools) {
        let (rect, response) = ui.allocate_exact_size(ui.available_size(), egui::Sense::click_and_drag());
        let size = Vec2::new(rect.width(), rect.height());
        self.rect = rect;

        self.navigate(ui, &response, rect, size);
        self.feed_tools(ui, &response, rect, size, doc, tools);
        if self.zoom_extents_pending && size.x > 1.0 && size.y > 1.0 {
            match doc.model().bounds() {
                Some((min, max)) => self.camera.zoom_extents(min.as_vec3(), max.as_vec3(), size),
                None => self.camera = OrbitCamera::default(),
            }
            self.zoom_extents_pending = false;
        }

        let Some(render_state) = frame.wgpu_render_state() else { return };
        if self.scene_version != Some(doc.version()) {
            let mut scene = Scene::reference();
            scene.add_model(doc.model(), doc.selection());
            self.renderer.set_scene(&render_state.device, &scene);
            self.scene_version = Some(doc.version());
        }
        let pixels_per_point = ui.ctx().pixels_per_point();
        let physical = [
            (rect.width() * pixels_per_point).round() as u32,
            (rect.height() * pixels_per_point).round() as u32,
        ];
        let resized = self.renderer.render(
            &render_state.device,
            &render_state.queue,
            physical,
            self.camera.view_proj(size),
            self.camera.eye(),
        );

        let view = self.renderer.output_view().expect("rendered at least once");
        let filter = egui_wgpu::wgpu::FilterMode::Linear;
        let mut egui_renderer = render_state.renderer.write();
        let id = match self.texture {
            Some(id) if resized => {
                egui_renderer.update_egui_texture_from_wgpu_texture(&render_state.device, view, filter, id);
                id
            }
            Some(id) => id,
            None => egui_renderer.register_native_texture(&render_state.device, view, filter),
        };
        drop(egui_renderer);
        self.texture = Some(id);

        let painter = ui.painter_at(rect);
        painter.image(
            id,
            rect,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );
        draw_overlay(&painter, rect, &self.camera, size, &tools.overlay(doc));
    }

    /// Middle-drag orbits, Shift+middle-drag and right-drag pan, the wheel zooms at the cursor.
    fn navigate(&mut self, ui: &egui::Ui, response: &egui::Response, rect: egui::Rect, size: Vec2) {
        let (shift, scroll) = ui.input(|i| (i.modifiers.shift, i.smooth_scroll_delta.y));
        let drag = to_vec2(response.drag_delta());
        if response.dragged_by(egui::PointerButton::Middle) {
            if shift {
                self.camera.pan(drag, size);
            } else {
                self.camera.orbit(drag);
            }
        } else if response.dragged_by(egui::PointerButton::Secondary) {
            self.camera.pan(drag, size);
        }
        if scroll != 0.0
            && let Some(pointer) = response.hover_pos()
        {
            self.camera.zoom_at(scroll / POINTS_PER_ZOOM_STEP, to_vec2(pointer - rect.min), size);
        }
    }

    /// Left-button input goes to the active tool.
    fn feed_tools(
        &mut self,
        ui: &egui::Ui,
        response: &egui::Response,
        rect: egui::Rect,
        size: Vec2,
        doc: &mut Document,
        tools: &mut Tools,
    ) {
        let local = |p: egui::Pos2| to_vec2(p - rect.min);
        let (mods, press_origin, latest) = ui.input(|i| {
            let mods = Modifiers { shift: i.modifiers.shift, ctrl: i.modifiers.command };
            (mods, i.pointer.press_origin(), i.pointer.latest_pos())
        });
        let mut send = |event| tools.pointer(event, doc, &mut self.camera, size);

        if let Some(pos) = response.hover_pos().map(local)
            && self.last_hover != Some(pos)
        {
            self.last_hover = Some(pos);
            send(PointerEvent::Move(pos));
        }

        let primary = egui::PointerButton::Primary;
        if response.drag_started_by(primary)
            && let Some(origin) = press_origin.map(local)
        {
            send(PointerEvent::DragStart { origin });
        }
        if response.dragged_by(primary)
            && let Some(pos) = response.interact_pointer_pos().map(local)
        {
            send(PointerEvent::Drag { pos, delta: to_vec2(response.drag_delta()) });
        }
        if response.drag_stopped_by(primary)
            && let Some(pos) = latest.map(local)
        {
            send(PointerEvent::DragEnd { pos, mods });
        }

        let count = if response.triple_clicked() {
            3
        } else if response.double_clicked() {
            2
        } else if response.clicked() {
            1
        } else {
            0
        };
        if count > 0
            && let Some(pos) = response.interact_pointer_pos().map(local)
        {
            send(PointerEvent::Click { pos, count, mods });
        }
    }
}

fn to_vec2(v: egui::Vec2) -> Vec2 {
    Vec2::new(v.x, v.y)
}

/// Draw tool previews on top of the rendered model: preview edges, the inference point and its
/// tooltip, and the selection window.
fn draw_overlay(painter: &egui::Painter, rect: egui::Rect, camera: &OrbitCamera, size: Vec2, overlay: &Overlay) {
    let screen = |p: DVec3| camera.project(p.as_vec3(), size).map(|s| rect.min + egui::vec2(s.x, s.y));

    let highlight = egui::Stroke::new(3.0, egui::Color32::from_rgb(0, 90, 255));
    for [a, b] in &overlay.highlight {
        if let (Some(a), Some(b)) = (screen(*a), screen(*b)) {
            painter.line_segment([a, b], highlight);
        }
    }

    let preview = egui::Stroke::new(1.5, egui::Color32::BLACK);
    for [a, b] in &overlay.lines {
        if let (Some(a), Some(b)) = (screen(*a), screen(*b)) {
            painter.line_segment([a, b], preview);
        }
    }

    if let Some(snap) = &overlay.snap
        && let Some(pos) = screen(snap.point)
    {
        // SketchUp's inference colors: green endpoints, yellow-ish origin, blue on-face.
        let color = match snap.kind {
            SnapKind::Endpoint => egui::Color32::from_rgb(0, 170, 0),
            SnapKind::Origin => egui::Color32::from_rgb(230, 170, 0),
            SnapKind::OnFace => egui::Color32::from_rgb(0, 90, 255),
            SnapKind::OnPlane | SnapKind::OnGround => egui::Color32::from_gray(90),
        };
        let radius = if matches!(snap.kind, SnapKind::OnPlane | SnapKind::OnGround) { 2.5 } else { 5.0 };
        painter.circle(pos, radius, color, egui::Stroke::new(1.0, egui::Color32::WHITE));
        if let Some(label) = snap.kind.label() {
            let galley = painter.layout_no_wrap(label.into(), egui::FontId::proportional(12.0), egui::Color32::BLACK);
            let text_rect = egui::Rect::from_min_size(pos + egui::vec2(12.0, 10.0), galley.size()).expand(3.0);
            painter.rect(
                text_rect,
                2.0,
                egui::Color32::from_rgb(255, 255, 220),
                egui::Stroke::new(1.0, egui::Color32::GRAY),
                egui::StrokeKind::Outside,
            );
            painter.galley(text_rect.min + egui::vec2(3.0, 3.0), galley, egui::Color32::BLACK);
        }
    }

    if let Some(window) = &overlay.window {
        let a = rect.min + egui::vec2(window.start.x, window.start.y);
        let b = rect.min + egui::vec2(window.end.x, window.end.y);
        let r = egui::Rect::from_two_pos(a, b);
        let stroke = egui::Stroke::new(1.0, egui::Color32::from_rgb(40, 40, 40));
        painter.rect_filled(r, 0.0, egui::Color32::from_rgba_unmultiplied(80, 120, 255, 24));
        if window.crossing {
            let corners = [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()];
            painter.extend(egui::Shape::dashed_line(&corners, stroke, 6.0, 4.0));
        } else {
            painter.rect_stroke(r, 0.0, stroke, egui::StrokeKind::Inside);
        }
    }
}

impl Viewport {
    /// Where a model point appears in the window, if in front of the camera.
    pub fn to_window(&self, point: DVec3) -> Option<egui::Pos2> {
        let size = Vec2::new(self.rect.width(), self.rect.height());
        self.camera.project(point.as_vec3(), size).map(|s| self.rect.min + egui::vec2(s.x, s.y))
    }
}
