# SketchCraft

A SketchUp-style 3D modeler in pure Rust (egui/eframe 0.36 + wgpu 30), structured like the
ArtCraft "Craft" apps (github.com/storytold). Primary use: woodworking/furniture design.

## Clean-room rule
Reimplement SketchUp's *behavior* from using the app, its manual and public docs. Never
decompile, disassemble or copy code from SketchUp's binaries. `.skp` interop, when added,
goes through Trimble's official SketchUp C SDK.

## Layout
- `crates/view`: camera/view math. Pure `glam`, no GPU/UI deps, unit-tested.
- `crates/doc`: `Model` (vertices/edges/faces with holes, f64 inches), `Document`
  (selection, snapshot undo/redo, version counters), `units` (Measurements-box parsing and
  fractional-inch formatting).
- `crates/tools`: toolkit-independent tools (`Tools`, `ToolId` with SketchUp shortcuts),
  picking and endpoint snapping (`pick`), Select and Rectangle state machines. The UI sends
  `PointerEvent`s and keystrokes; tools return an `Overlay` to draw.
- `crates/render`: wgpu renderer. Draws into an offscreen MSAA texture that the UI shows as
  an egui image (own depth buffer, independent of egui's pass). Grid is a no-depth-write pass
  under faces; front/back face colors chosen per fragment from the eye position.
- `crates/ui-egui`: window, toolbar, status bar, Measurements box, keyboard routing
  (`keys.rs`), viewport input and overlays. `tests/workflow.rs` drives the real app headlessly
  with `egui_kittest` (set `SKETCHCRAFT_SNAPSHOT_DIR` to save rendered PNGs).
- `apps/sketchcraft`: the binary.
- Planned: groups/components, tags and materials in `doc`; more tools in `tools`.

Keep logic out of `ui-egui`: anything testable belongs in a plain crate.

## Keyboard model (SketchUp's)
Just type: digits and length punctuation start a Measurements entry; once typing, letters go
to the box too (so `38mm` isn't Move). Otherwise letters are tool shortcuts, Space is Select,
`Z` (Shift+Z) is Zoom Extents. Enter submits, Esc clears typing / cancels / deselects.

## Conventions
- Z up; red = X, green = Y, blue = Z (SketchUp). Model units are **inches**.
- Screen coordinates: egui points, origin top-left of the viewport, Y down.
- `unsafe_code` is forbidden workspace-wide, so no `bytemuck` derives; vertices are `[f32; N]`.
- Render target colors are linear; build them with `srgb(r, g, b)`.
- wgpu 30 / glam 0.34 / egui 0.36 APIs differ from older docs: `App::ui` (not `update`),
  `egui::Panel` (not `TopBottomPanel`), `glam::camera::rh::proj::directx::perspective`
  (wgpu clip space), `Option`-wrapped pipeline fields, `immediate_size`, `multiview_mask`.

## Commands
- `cargo run -p sketchcraft`
- `cargo test --workspace`
- `cargo clippy --workspace --all-targets` (keep it warning-free)

## Roadmap (MVP)
1. Viewport, grid, axes, orbit/pan/zoom (done)
2. Select + Rectangle with typed dimensions, all tool shortcuts, undo/redo, Delete (done)
   - Known gaps: partially overlapping coplanar faces don't split; endpoint/origin snapping
     only (no axis/midpoint/on-edge inference yet).
3. Push/Pull + icon toolbar (Phosphor icons via `egui-phosphor`) (done)
   - `doc::push_pull`: free face → closed solid; prism cap → moves (no seam); face set in a
     surface → block or pocket. Tool: typed distance, retype after, double-click repeats.
   - Known gaps: no coplanar-face merging; pocket walls touching an existing side don't cut it.
4. Move (done): `doc::transform` (move vertices with stretch + planarity check, copy);
   tool with axis inference, arrow-key locks, Ctrl-tap copy, `3x` / `/3` arrays, typed
   distance or `x,y,z` / `<x,y,z>` / `[x,y,z]`.
5. Line tool, face splitting, fuller inference (midpoint, on-edge); Rotate, Offset, Circle
6. Tape Measure + guides
7. Groups and components (shared definitions), outliner
8. Tags (SketchUp's renamed Layers)
9. Materials, with fields for OpenCutList-style cut lists (type, thickness, grain)
10. Native save/load; OBJ/glTF export. Later: `.skp` import via the C SDK, cut lists.
