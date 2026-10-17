# WoodSketch

A SketchUp-style 3D modeler in pure Rust (egui/eframe 0.36 + wgpu 30), structured like the
ArtCraft "Craft" apps (github.com/storytold). Primary use: woodworking/furniture design.

## Clean-room rule
Reimplement SketchUp's *behavior* from using the app, its manual and public docs. Never
decompile, disassemble or copy code from SketchUp's binaries or SDK (no REA on them). `.skp`
reading (`doc::skp`) was worked out from `.skp` files only, without the SDK; keep it that way
(published notes from others who worked from files are fine for ideas, never their code).

## Layout
- `crates/view`: camera/view math. Pure `glam`, no GPU/UI deps, unit-tested.
- `crates/doc`: `Model` (vertices/edges/faces with holes, f64 inches), `Scene` (groups and
  components: definitions + instances), `View` (whole drawing flattened for picking/drawing),
  `Document` (scene + editing path, selection, snapshot undo/redo, version counters), `units` (Measurements-box parsing and
  fractional-inch formatting).
- `crates/tools`: toolkit-independent tools (`Tools`, `ToolId` with SketchUp shortcuts),
  picking, snapping and axis inference (`pick`), and one state machine per tool. The UI sends
  `PointerEvent`s and keystrokes; tools return an `Overlay` to draw.
- `crates/render`: wgpu renderer. Draws into an offscreen MSAA texture that the UI shows as
  an egui image (own depth buffer, independent of egui's pass). A sky pass (gradient above the
  horizon, plain ground below) comes first. The optional grid (View > Grid) is a no-depth-write pass
  under faces; front/back face colors chosen per fragment from the eye position.
- `crates/ui-egui`: the window. `chrome.rs` lays it out like SketchUp on Windows (menu bar,
  toolbar, Large Tool Set on the left, Default Tray on the right, status bar with the
  Measurements box; light theme) so it reads as SketchUp's spiritual successor, without its
  branding or artwork. Keyboard routing (`keys.rs`), viewport input and overlays
  (`viewport.rs`). `tests/workflow.rs` drives the real app headlessly with `egui_kittest` (set
  `WOODSKETCH_SNAPSHOT_DIR` to save rendered PNGs).
- `apps/woodsketch`: the binary.

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
- `cargo run -p woodsketch`
- `cargo test --workspace`
- `cargo clippy --workspace --all-targets` (keep it warning-free)

## Backlog
`docs/BACKLOG.md` lists known gaps and skipped work. When you skip or simplify something,
add it there; when you finish an item, remove it.

## Roadmap (MVP)
1. Viewport, grid, axes, orbit/pan/zoom (done)
2. Select + Rectangle with typed dimensions, all tool shortcuts, undo/redo, Delete (done)
3. Push/Pull + icon toolbar (Phosphor icons via `egui-phosphor`) (done)
   - `doc::push_pull`: free face → closed solid; prism cap → moves (no seam); face set in a
     surface → block or pocket. Tool: typed distance, retype after, double-click repeats.
4. Move (done): `doc::transform` (move vertices with stretch + planarity check, copy);
   tool with axis inference, arrow-key locks, Ctrl-tap copy, `3x` / `/3` arrays, typed
   distance or `x,y,z` / `<x,y,z>` / `[x,y,z]`.
5. Midpoint and on-edge snapping (done): `pick::snap_feature`, priority endpoint > midpoint > on-edge
6. Rotate (done): protractor tool (Q) + Move rotation grips on bounding-box faces; 15° snap,
   typed angles, Ctrl-tap copy, `3x` / `/3` rotational arrays; axis guide lines through the center and
   a 4° snap onto red/green/blue (over the 15° steps)
7. Groups and components (done): `doc::scene` (Scene, Container, Definition, Instance, View),
   editing path (double-click in, Esc / click outside out), G / Ctrl+G, explode, Outliner panel.
   G on one group (or Ctrl+G on one component) converts it in place instead of wrapping it.
   The Outliner shows the whole model as a tree (clicking a nested part opens its group and
   selects it; parts needing a material are flagged).
   Push/Pull auto-groups a board pulled from a free face (setting, on by default). Tools work
   in the edited container's coordinates via `Screen::in_context`; picking/snapping/drawing use
   the flattened `View` (edited geometry keeps its ids; picks resolve to instances).
8. Tags (done): on groups/components only (SketchUp best practice); hidden tags drop their
   instances from the `View` (not drawn, picked or snapped). Tags panel (add, rename,
   show/hide, delete → Untagged) and Entity Info tag picker in the left sidebar.
9. Materials (done): `Material` (name, color, kind, grained, thickness) on instances and faces;
   faces show their own paint, else the nearest painted group's (`View::face_material`). Paint
   Bucket (B, Alt samples), Materials panel (clicking one activates the Paint Bucket), Entity
   Info material picker. Material edits merge into one undo step (`edit_scene_merging`).
10. Save/open (done): `.woodsketch` JSON (`.scraft` before the rename, still opened) (`doc::file`, versioned header, serde derives on the scene),
   File menu + Ctrl+N/O/S/Shift+S, native dialogs (`rfd`), title shows `*` when unsaved,
   "Save changes?" before New/Open/Close, `woodsketch drawing.woodsketch` opens a file. File > Open Recent
   (last 10, kept with eframe's persistence; the window itself isn't restored). Saves write a temp file
   and swap it in; unsaved changes autosave every 2 minutes to the settings folder
   (`ui-egui::autosave`), offered back at start after a crash.
11. Cut list (done): `doc::cutlist` (parts = visible groups/components with faces, grouped by
   material + size, labeled A, B...; CSV) and `doc::cutplan` (sheet goods + solid wood: 2D
   guillotine packing, grain-aware; lumber: smallest standard section, 1D packing over each
   stock length, boards split when shorter ones cost no more). Cut List window (toolbar): Parts,
   Cutting diagrams with shopping list, Stock & kerf settings.
12. Line tool and face splitting (done): `doc::lines` (`Model::add_line` splits crossed edges;
   faces are rebuilt per plane from the planar graph, so a line across a face splits it, a loop
   on a face or closed in free space makes a face, and erasing a seam merges coplanar faces).
   Push/Pull cuts a rabbet or notch when a wall is flush with a side face (and extends that
   side when pulling). Line tool (L): chains points, axis inference and arrow locks, typed
   lengths or `x,y,z`; ends when a face forms, on a repeated click, or with Esc.
13. Tape Measure + guides (done): `Guide` (endless line or point) entities in the model, saved,
   selectable and erasable; Edit > Delete Guides. Tape (T): from an edge, a parallel guide line at
   the offset; from a point, a guide point; Ctrl measures only; typing places or moves the guide.
   Snapping: guide points, guide/edge and guide/guide intersections, on guide lines. Rectangle
   now draws through `Model::draw_polygon`, so it splits faces it overlaps.
14. Offset (done): `Model::offset_outline` / `offset_face` (mitered corners, refuses outlines that
   fold over), drawn through `draw_polygon` so an inward offset makes a frame and a panel. Tool (F):
   click a face (or select one), move in or out, click or type; retyping changes it.
15. Circle (C) and Arc (A) (done): `doc::curves` (circle and 2-point arc points, 24 / 12 sides,
   `Ns` to change, `Nr` for an arc radius), drawn as lines so they split faces. Push/Pull cuts
   through when pushed to the far face (dowel holes, rounded corners). Edges between faces
   meeting under 30° are drawn soft (hidden).
16. Export (done): `doc::export` (visible geometry in world coordinates, triangles wound to their
   front; OBJ named per group with an MTL of material colors; ASCII STL), File > Export with
   inches or millimeters.
17. Cut-list polish (done): stock sizes and kerf saved with the drawing (`Scene::cut_settings`), each
   material can override them (`Material::stock`, `cutplan::plan_all`); Entity Info renames groups
   and components (the part name); `doc::cutsheet` writes a printable HTML cut sheet (parts, what
   to buy, SVG diagrams) that opens in the browser.
18. Scale (done): `doc::scaling` (factors along three axes about an anchor); tool (S) with 26 grips
   on the box (corners even, edges two ways, faces one way), opposite grip fixed or Ctrl for the
   center, typed factors or sizes (`36"`), retyping changes it. A single group scales along its
   own axes into its transform; the cut list measures parts through their groups' scale.
19. Eraser (done): `Document::erase`; tool (E) erases the edge, guide, group or component clicked, or
   everything dragged over as one undo step; faces only go with their edges (a seam between
   coplanar faces merges them). Every toolbar tool is now available.
20. Materials rethought (OpenCutList's model): a material lists the standard stock it comes in
   (`cutplan::Standard`: sheet thicknesses and sizes, lumber sections and lengths, solid wood
   thicknesses, lengths and widths, US defaults per type); `cutplan::match_stock` picks each
   part's stock from its own size (a 1.5 x 3.5 x 84 stud of Pine Lumber -> 2x4, 8'). Library of
   species/products (`doc::library`); older drawings upgrade on open. Paint Bucket paints the
   whole object (Ctrl: one face).
21. Parts know their grain (done): `doc::part` (`PartInfo` on each definition: grain axis or longest
   side, flat-sawn / quartersawn); the cut list measures length along the grain; Entity Info part
   section (size, stock, grain, cut) and a grain arrow on the selected part.
22. Board from stock (done): `doc::board` (`Stock`, `stock_options`, `parse_board` for `2x4x96` /
   `3/4x11 1/4x30`, `Document::add_board`: a group whose own axes are length, width, thickness).
   Board tool (W): lumber drags out its length (flat, Ctrl on edge), sheets and solid wood drag a
   rectangle at their thickness; Board panel in the tray picks the stock. Custom size in the panel adds a
   size to the material (`Document::add_stock`); typed `3,6 1/2` / `3x6 1/2x96` is a one-off. Push/Pull on a free face
   snaps to the drawing's standard thicknesses.
23. Joinery: `doc::joinery` (`Document::joint_fit` reads how two sibling parts sit: butted,
   overlapping, at a corner, crossing; `let_in` (dado / rabbet, the part grows by the depth),
   `mortise_and_tenon`, `dowels`, `box_joint`, `half_lap`, `cut_overlap`). Each is rectangles or
   circles drawn on faces and pushed/pulled on a copy of the scene, one undo step, recorded in
   `Scene::joints`. Joinery panel in the tray for two selected parts (joint picker, sizes, how to
   place the parts); Entity Info lists joints.
   Copies of a shared component: a cut, tenon or hole the design already has is recognized
   (`already_done`, `repeat`), so one leg component turned to each corner gets two mortises.
   Biscuits and loose tenons (`Slots`: matching slots, shallower in a face), pocket screws (listed,
   not drawn), through tenons; each joint's pieces (`Joint::hardware`) and glue-up seams
   (`PartInfo::edge_joint`) add up in `CutList::hardware` (Parts tab, cut sheet).
   Dovetails (`Document::dovetails`: tails grown into the pins board, trapezoid sockets and waste
   at 1:8), splines (`Spline`), haunched tenons, pocket-hole pockets. Each joint keeps its cuts
   (`Joint::ops`, `Scene::joint_bases`), so `Document::remove_joint` rebuilds its parts without it
   (refused if a part was changed by hand since); the panel's Change removes and re-picks it.
24. Edge profiles: `doc::profile` (`Profile` roundover / chamfer / cove / ogee as a corner curve;
   `Edges::AroundFace` (stacked inset rings, mitered corners) or `Edges::Along` (a profiled
   cross-section extruded); `Document::profile_edges` rebuilds a plain board's geometry, one undo
   step, recorded in `Scene::profiles`). Edge profile panel in the tray for one selected part, edges
   named by where they face (Top, Front...).
25. Build sheets: `doc::buildsheet::to_html` (one sheet per part definition, in cut-list order: name,
   letter, count, finished size, stock, grain; face / edge / end views as SVG to scale with overall
   dimensions; steps from `Scene::joints` and `Scene::profiles` in shop order). Cut List window >
   Print build sheets.
   Wide solid-wood parts glue up from narrower boards (`PartInfo::glue_up`, `cutplan::glue_up`: auto
   when wider than the widest board, aiming for boards 7 1/4" or narrower; strips B-1, B-2... in the
   diagrams, a glue-up step on the build sheet; Entity Info picks Auto or 1-5 boards).
26. Wood movement: `doc::movement` (`species` coefficients from the USDA Wood Handbook on
   `Material::movement`, filled from the library and by name for older drawings; `check` gives each
   solid-wood part's movement across its grain for `Scene::moisture_swing`, default 4%, and
   conflicts where a touching part won't move that way). Cut List window > Wood movement tab.
27. Releases and updates: a `v*` tag (matching Cargo.toml's version) runs `.github/workflows/release.yml`:
   Windows x64, macOS arm64 and x64, Linux x64 builds, each packed by Velopack (`vpk`, version
   pinned to the `velopack` crate) on its own channel, into a DRAFT GitHub release (publish it to
   ship). The app runs `VelopackApp` first, then checks GitHub Releases in the background at startup
   (Help > Check for updates at startup; Help > Check for Updates...), downloads quietly, and offers
   "Restart to update" in the status bar (after "Save changes?"). The UI side (`ui-egui::updates`)
   has no Velopack dependency; `apps/woodsketch/src/update.rs` plugs it in. CI
   (`.github/workflows/ci.yml`) lints and runs the non-GPU tests on all three platforms.

## Direction
Specialized for woodworking: SketchUp-familiar modeling where parts know they're wood (species,
grain, joinery, profiles, cut list, build sheets, movement checks). The ordered plan is at the
top of `docs/BACKLOG.md` ("Woodworking direction"); keep general modeling working alongside it.
28. Templates: `doc::template` (`Template` cabinet / drawer / face frame / table, each from a few sizes
   and a joint choice, built from boards and the joinery API, grouped, one undo step via
   `Document::as_one_step`, sizes kept on the definition as `Made` so `rebuild_template` changes
   them in place). Templates panel in the tray; "<Template> sizes" for a selected one.
29. Copy and paste: `doc::clipboard` (`Clip` = the source scene with only the copies at the top, in
   world coordinates; pasting into the same drawing reuses definitions, into another imports
   definitions, materials by name, tags, joints and profiles). `Tools::copy / cut / paste /
   paste_in_place / duplicate`; Paste is carried by Move (`MoveTool::carry`) until a click, joined
   into one undo step, Esc takes it out. Ctrl+C/X/V arrive as egui `Copy`/`Cut`/`Paste` events (a
   note goes on the system clipboard so Ctrl+V is reported); Ctrl+D duplicates.
30. SketchUp files: `doc::skp::read` opens `.skp` from SketchUp 2021+ (a UTF-16 header, then a ZIP:
   `model.dat` is nested records, 2-byte tag + 4-byte length; materials' colors in
   `materials/*/material.xml`). Reads component definitions, top-level geometry, vertices, edges,
   faces with holes and materials, instances (placement = rotation rows, translation, divisor),
   materials (matched to the library by name) and layers as tags. File > Open opens one as a new
   unsaved drawing; File > Import SketchUp Model... pastes one in place (`Clip::from_scene`). Set
   `WOODSKETCH_SKP=<file>` to run the real-file tests in `skp` and `workflow.rs`. Flat things
   (thinner than 1/64") aren't cut-list parts.
