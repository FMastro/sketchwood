# Backlog

Things we deliberately skipped or know are incomplete, to come back to. Newest first within
each section. Remove items when they're done (mention them in the commit).

## Next up (recommended order, 2026-10-17)
1. **Drawers and doors fitted to an opening**: select a cabinet or face-frame opening, pick
   Drawer or Door, and get one sized with the usual clearances: the drawer box 1/2" narrower than
   the opening for side-mount slides (or per the slide chosen), its depth from the slide length;
   the front overlay (1/2", full) or inset (1/16" gap all round); doors single or a pair, with
   hinge-side and meeting gaps. Also a frame-and-panel door (rails and stiles with a floating
   panel in grooves, sized for wood movement).
2. **Hardware as parts**: drawer slides (side-mount, undermount; lengths 12"-24"), hinges
   (concealed/European, butt), shelf pins, pulls and knobs, screws. Placed as simple parametric
   shapes (no licensed models), each counted in the shopping list, and slides set the drawer
   size in item 1 (their clearance and length).
3. **Agent-ready** (Woodworking direction, item 8): a project recipe (JSON), a command line
   (`woodsketch build`, `cutlist`, `cutsheet`, `render`), an MCP server so Claude and other
   assistants can design with WoodSketch, and docs for agents. Templates (and 1 and 2) are the
   building blocks a recipe calls.
4. **.skp import** through Trimble's SketchUp SDK: check its terms allow a free open-source app
   before any work on it.
5. **Release polish** (installers, auto-update, icons and the site's download links are done):
   - Signing: Windows with Azure Artifact Signing (~$9.99/month; individuals in the US/Canada; a
     paid Azure subscription; identity validation takes days; CI signs via OIDC, no stored key).
     macOS with an Apple Developer ID ($99/year) plus notarization, or Gatekeeper blocks it.
     Linux needs none (optionally GPG-signed checksums). Until then SmartScreen and Gatekeeper
     warn. Do it before announcing WoodSketch widely.
   - `.woodsketch` file association on all three (double-click opens it).
   - Windows ARM64 and Linux ARM64 builds; a macOS DMG; Linux .deb/.rpm; SHA-256 checksums.
   - Delta updates (download the previous release before packing, so updates are small).
   - A Download button on woodsketch.net that picks the visitor's OS.

## Woodworking direction (in this order)
SketchUp-familiar modeling, specialized for woodworking: parts know they're wood. Keep general
modeling (rooms, jigs, odd shapes) working; the wood features add on and stay optional.
1. **Parts that know they're wood** (started): grain direction and cut per part, part section in
   Entity Info and a grain arrow are done. Still to come: picking the reference face/edge on
   the model (with build sheets), and the operations applied to a part (joints, profiles,
   holes) stored on it so they can be edited and listed (with joinery).
2. **Board from stock** (done). Later: choose which side of the line a lumber board's width goes
   (it's always to the left of the drag now), Tab to cycle a board's faces against a surface, and
   picking stock straight from the Materials panel.
3. **Joinery** (done: dado, groove and rabbet let in from a butt joint, mortise and tenon
   (through, haunched), dovetails (through, half-blind), dowels, biscuits, loose tenons, splines,
   pocket screws (pockets drawn on the hidden face, not the angled bore), box joint, half-lap,
   cut where parts overlap; reinforced glue-up seams; a hardware list; Change and Remove on
   each joint). Still to come: a fit allowance (mortise a hair bigger); sliding and lapped
   dovetails, variable dovetail spacing; Change keeps the joint's old sizes (it starts from the
   suggestions); joints can't be removed after a part is changed by hand (use Undo); parts in
   different groups (both must be side by side now); parts that aren't square to each other.
4. **Edge profiles** (done: roundover, chamfer, cove, ogee, around a face with mitered corners or
   along a length, from the Edge profile panel). Still to come: bead and other bits; profiling a
   board that already has joints or other cuts (profile first for now); edges around a face and
   along the length on one board; clicking edges in the model to pick them; changing a profile
   later (it's listed, not yet parametric); curved and angled edges.
5. **Build sheets** (done: a printable page per part from the Cut List window, with its size,
   stock, three dimensioned views to scale, and steps in shop order). Still to come: where each
   joint and profile sits (dimensions to it from the reference face and edge, which aren't picked
   yet), hidden lines dashed, an exploded assembly view and glue-up order.
6. **Wood movement checks** (done: Wood Handbook coefficients per species and cut, a moisture swing
   setting, how much each part moves, and warnings where touching parts fight it, in the Cut List
   window). Still to come: arrows and numbers on the model; telling a floating panel or slotted
   fastening from a glued one (every touching cross-grain meeting is flagged now); species data
   for custom woods (only names matching a known species get it); movement in thickness and
   along the grain; parts at an angle.
7. **Templates** (done: cabinet, drawer, face frame, table, each changeable in size). Still to
   come: drawers and doors fitted to an opening, frame-and-panel doors, hardware (Next up, items
   1 and 2); more templates (bookcase with adjustable shelves, bed, workbench); templates keep
   only their sizes, so Change size loses edits made inside. Then **.skp import** through
   Trimble's SDK (the adoption hook; check its redistribution terms early).
8. **Agent-ready**: let AI agents design with WoodSketch, so a woodworker can ask their assistant
   "build me a nightstand, 24\" tall, from 3/4\" cherry" and get a real drawing, cut list,
   cutting diagrams and shopping list. The engine (`doc`) already runs without the UI. Pieces:
   - A project recipe (JSON): parts as boards from stock (material, section or thickness, length,
     place, orientation), joints between them, tags; validated with clear errors an agent can fix
     ("Shelf doesn't touch Side"). Builds a `.woodsketch`.
   - A command line: `woodsketch build recipe.json`, `cutlist`, `cutsheet` (HTML), `export`,
     `render` (PNG views), headless.
   - An MCP server (`woodsketch mcp`) so Claude and other agents can call it directly: add boards,
     make joints, read the cut list and diagrams, render views to check their work.
   - Docs for agents (an AGENTS.md / llms.txt on woodsketch.net with recipes for common
     furniture), and the app opening what an agent made for editing by hand.
9. Later: a community model library (a separate service: accounts, moderation, licensing;
   3D Warehouse's terms forbid aggregating its models), cost estimates (board feet / sheets x
   price), exploded views and assembly order.

## Geometry
- **Auto-joining**: moved or copied geometry doesn't merge with geometry it lands on (shared
  vertices/edges aren't welded). Fine while boards should stay separate; revisit with groups.
- **Coplanar face merging**: two faces on one plane that share an edge stay separate (a seam
  edge remains, e.g. boards glued end to end). Erasing the seam merges them, and Push/Pull
  merges walls flush with a side, but nothing merges automatically otherwise.
- **Collinear leftovers**: merging faces (erasing a splitting line, pulling part of a top up
  flush with a side) keeps the extra vertex where an edge was split; SketchUp heals those.
- **Move can't bend faces**: SketchUp folds a face that would become non-planar (adds a
  diagonal); we refuse the move instead.

## Scale
- No mirroring (negative factors are refused); no Shift to switch corner grips to non-uniform
  or edge grips to uniform.
- Several things scaled together use the red/green/blue axes; a rotated group among them gets
  sheared, and the cut list then measures it only approximately.
- Grips are drawn like Move's rotation grips (red crosses); SketchUp's are green boxes, and it
  hides the ones facing away.

## Circles and arcs
- Curves aren't one entity: a circle is 24 separate edges (SketchUp groups them, so one click
  selects the whole circle and Entity Info shows its radius and sides).
- Soft edges are worked out when drawing (faces meeting under 30° hide their shared edge);
  there's no per-edge Soften/Smooth setting, and facets are shaded flat, not smoothed.
- Arc: no tangent inference, no 3-point Arc or Pie, no Polygon tool. Circles off any face lie
  flat on the ground plane.

## Offset
- Only a face's outer outline is offset; holes aren't (SketchUp offsets them too).
- Can't offset a selection of connected edges (only faces), and the distance doesn't snap
  to points the cursor passes over.
- Corners are always mitered (sharp); no rounded outward corners.

## Guides (Tape Measure)
- Tape Measure: no "resize the model" after measuring (SketchUp offers to scale everything so
  the measured distance becomes the typed one).
- No Protractor guides (angled guide lines), no "hide guides" toggle (only Delete Guides).
- Guide lines snap where they cross edges and other guides, but not where they cross faces.

## Inference / snapping
- Line: a typed length can't be changed after the line is drawn. Rectangle, Arc and Circle
  don't use parallel/perpendicular or "From Point" inference yet (Line, Move and Tape do).
- Faces closed from loose lines face up, else toward the front, else to the right; SketchUp
  orients them to match connected faces. No Reverse Faces command yet.
- Rectangle: square and golden-ratio diagonal hints; drawing rectangles off the axes.
- Snapping only considers screen distance; no "sticky" preference for the last inference.

## Groups and components
- Deliberate difference from SketchUp: starting Line, Rectangle, Circle, Arc, Push/Pull or
  Offset on a closed group's face opens the group, so the work goes into the board. (Paint
  still paints the whole group, as in SketchUp.)
- Selected groups are tinted; SketchUp instead draws their bounding box in blue.
- No renaming or drag-to-reparent in the Outliner, no "make unique" command for components, no
  component browser to place more instances.
- Move stretching doesn't reach into groups from outside (Move moves the whole group).
- Undo snapshots copy every definition; fine now, heavy for big furniture assemblies.
- Copies of a group share a definition until opened (SketchUp's behavior); opening one gives
  it its own copy, which isn't an undo step by itself.

## Cut list
- One sheet size per material (OpenCutList keeps a list of sheet sizes to choose from), and
  no offcuts inventory. Defaults are US sizes.
- Glue-ups: strips are equal widths (no mixing a wide and a narrow board), with a fixed 1/4"
  jointing allowance and 1" trim; edge-glued panels only (no face-glued thick parts like a
  laminated leg); the build sheet doesn't draw the boards.
- No edge banding, no veneer, and no hardware counts beyond listing.
- Packing is heuristic (several orders and rules, best kept), not provably optimal; no
  offcut reuse across materials, no "use my leftover pieces first".
- Solid wood assumes one board width per material (the narrowest standard width holding the
  widest part); real hardwood comes in random widths.
- Parts are named by their group's name; rename in Entity Info (the Outliner can't rename yet).
- The cut sheet is a web page printed from the browser (or saved as PDF there), not a PDF
  written directly; no labels/stickers per part.

## Materials
- No textures (color only): grain shows as an arrow on the selected part, not in the wood.
- **Material collections** (like SketchUp's): save materials into named collections, browse
  them across drawings, and import/export them. Today the library is built in and fixed.
- No veneer or edge-banding material types (OpenCutList has both), no density or price for
  estimates, no length/width/thickness oversize allowances.
- Solid wood picks the thinnest standard thickness at least as thick as the part (planed
  down); a part thinner than the thinnest stock still comes from it, without a warning.
- Paint Bucket: a click paints the whole object (deliberately unlike SketchUp's one face; Ctrl
  paints one face). No Shift (replace everywhere) variant.
- Painted color shows on both sides of a face; SketchUp paints front and back separately.

## Tags
- Only groups and components can be tagged; loose geometry is always Untagged.
- No tag colors or "color by tag", tag folders, dashed tag lines, or an "active tag" that new
  groups get automatically.
- Hidden groups are still listed in the Outliner (dimmed) but can't be selected there.
- Toggling a tag's visibility is an undo step (SketchUp does the same).

## Files
- File dialogs block the window while open (rfd's synchronous API).
- Export: OBJ (+ MTL colors) and ASCII STL only; no glTF, DAE or DXF/SVG (2D for CNC), no
  textures, no "export selection only", and each part isn't a separate STL file. No .skp import.

## UI
- Eraser: no Shift (hide) or Ctrl (soften/smooth) variants; it can't erase a face by itself (select it
  and press Delete).
- Layout follows SketchUp's Windows defaults (menu bar, toolbar, Large Tool Set, Default Tray,
  Measurements at bottom right), but toolbars can't be moved or customized and tray panels
  can't be reordered or undocked. No Components, Styles or Scenes panels yet.
- No right-click context menus (SketchUp's Entity menu: Make Group, Explode, Reverse Faces...).
- Standard views: Iso, Top, Front and Right only (no Back, Left, Bottom); no Parallel
  Projection toggle; tool cursors are the system arrow rather than per-tool icons.
- Sky and ground colors are fixed (SketchUp's Styles panel edits them).
- Keyboard focus never stays on buttons (it would swallow shortcuts and re-press the button
  on Space/Enter), so Tab-navigation between widgets isn't available.

## Rotate / grips
- Grips also appear around loose connected geometry (SketchUp shows them only on groups and
  components). Kept for now since it helps; revisit if it gets in the way.
- Grips on thin boards crowd together; consider hiding grips on faces that are tiny on screen.
- The protractor doesn't auto-orient to edges (SketchUp aligns to the hovered edge's plane)
  or flip when looking at it edge-on.

## Tools
- Not yet: Polygon, Follow Me, Dimensions, Text, Protractor, Section Plane, Walk/Look Around.
- Push/Pull: no Ctrl to keep the original face (push/pull copy), no double-click repeat
  across tools.
- Typing before the first click (e.g. absolute `[x,y,z]` start points).

## Rendering / UI
- Edges are 1px wide; tool previews and highlights are drawn on top without depth (x-ray).
- No hover pre-selection highlight in Select.
- Measurements show fractional inches only; no unit preferences (architectural, metric).
- Faces drawn on the ground face up, where SketchUp shows the blue back face (deliberate).

## Performance
Measured with `tests/perf.rs` (`cargo test --release -p woodsketch-ui-egui --test perf --
--ignored --nocapture`, `PERF_BOARDS=1500` for a big drawing). At 1500 boards (12,900 faces):
hover 1-3 ms, click 9 ms, selection highlight 2 ms, an edit 14-17 ms, full redraw rebuild 40 ms.
- Picking tests every face's plane and box (cached per model), but has no spatial index; a
  BVH would make it constant-ish for very big models.
- Each edit snapshots the whole scene for undo and re-flattens the whole view (~2 ms and ~4 ms
  at 1500 boards); sharing unchanged definitions (Arc) and per-definition caches would cut both.
- Vertex dedupe (`add_vertex`) and edge lookup are linear scans per call.
- The drawing's geometry is rebuilt and re-uploaded on every model change (the selection has
  its own small layer).

## Testing / infra
- Two full test runs once died without a failing assertion, likely a GPU driver crash with
  several wgpu devices at once. UI tests now take turns on the GPU; watch for recurrences.
