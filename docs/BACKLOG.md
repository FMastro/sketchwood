# Backlog

Things we deliberately skipped or know are incomplete, to come back to. Newest first within
each section. Remove items when they're done (mention them in the commit).

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
- Push/Pull cuts right through only when pushed exactly to the far face; pushing further
  makes a stray cap below the board.

## Offset
- Only a face's outer outline is offset; holes aren't (SketchUp offsets them too).
- Can't offset a selection of connected edges (only faces), and the distance doesn't snap
  to points the cursor passes over.
- Corners are always mitered (sharp); no rounded outward corners.

## Guides (Tape Measure)
- Only the edited group's guides are shown and snapped to; guides inside closed groups are
  hidden (SketchUp shows them all).
- Guides are drawn over the model without depth, so faces don't hide them.
- Move and Rotate don't move selected guides; guides can't be made from a point toward a
  direction (SketchUp makes a guide line when the tape starts on a point with Ctrl).
- No Protractor guides (angled guide lines), no "hide guides" toggle (only Delete Guides).
- Guide lines snap where they cross edges and other guides, but not where they cross faces.

## Inference / snapping
- Line tool: no parallel/perpendicular-to-edge inference and no "from point" alignment; a
  typed length can't be changed after the line is drawn.
- Faces closed from loose lines face up, else toward the front, else to the right; SketchUp
  orients them to match connected faces. No Reverse Faces command yet.
- Parallel / perpendicular inference, "from point" (dotted alignment to a hovered point),
  and Shift to lock the current inference.
- Rectangle: square and golden-ratio diagonal hints; drawing rectangles off the axes.
- Snapping only considers screen distance; no "sticky" preference for the last inference.

## Groups and components
- No dashed bounding box around the group being edited (SketchUp draws one).
- Selected groups are tinted; SketchUp instead draws their bounding box in blue.
- No renaming in the Outliner, no nested tree (only the current level is listed), no
  "make unique" command for components, no component browser to place more instances.
- Push/Pull and Move stretching don't reach into groups from outside; open the group first
  (same as SketchUp, but a "scale this board's length" shortcut would help woodworking).
- Undo snapshots copy every definition; fine now, heavy for big furniture assemblies.
- Copies of a group share a definition until opened (SketchUp's behavior); opening one gives
  it its own copy, which isn't an undo step by itself.

## Cut list
- One sheet size per material (OpenCutList keeps a list of sheet sizes to choose from), and
  no offcuts inventory. Defaults are US sizes.
- Grain always runs along a part's longest side; no per-part grain override, no edge banding,
  no veneer or hardware counts beyond listing.
- Packing is heuristic (several orders and rules, best kept), not provably optimal; no
  offcut reuse across materials, no "use my leftover pieces first".
- Solid wood assumes one board width per material (the narrowest standard width holding the
  widest part); real hardwood comes in random widths.
- Parts are named by their group's name; rename in Entity Info. The Outliner can't rename yet.
- The cut sheet is a web page printed from the browser (or saved as PDF there), not a PDF
  written directly; no labels/stickers per part.

## Materials
- No textures (color only), so no visible grain direction; parts' grain is assumed along their
  longest side for now (as OpenCutList does by default).
- Dimensional lumber stores one thickness, not a section (e.g. 1.5" x 3.5"); no list of stock
  thicknesses yet (each material has one thickness and optional stock sizes).
- Paint Bucket has no Ctrl (paint connected faces) or Shift (replace everywhere) variants.
- Painted color shows on both sides of a face; SketchUp paints front and back separately.

## Tags
- Only groups and components can be tagged; loose geometry is always Untagged.
- No tag colors or "color by tag", tag folders, dashed tag lines, or an "active tag" that new
  groups get automatically.
- Hidden groups are still listed in the Outliner (dimmed) but can't be selected there.
- Toggling a tag's visibility is an undo step (SketchUp does the same).

## Files
- No autosave or crash recovery, no recent-files list, no Windows file association (double-
  clicking a .scraft needs an installer or registry entry).
- Saving writes the file in place; a crash mid-write could truncate it (write to a temp file and
  rename instead).
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
- Eraser (drag-erase, Shift to hide), Line, Arc, Circle, Polygon, Offset, Scale, Tape
  Measure + guides, Paint Bucket.
- Push/Pull: Ctrl to keep the original face (push/pull copy), double-click repeat across
  tools, and pushing a face into an opposite face to cut a hole through.
- Typing before the first click (e.g. absolute `[x,y,z]` start points).

## Rendering / UI
- Edges are 1px wide; tool previews and highlights are drawn on top without depth (x-ray).
- No hover pre-selection highlight in Select.
- Measurements show fractional inches only; no unit preferences (architectural, metric).
- Faces drawn on the ground face up, where SketchUp shows the blue back face (deliberate).

## Performance
- Vertex dedupe, edge lookup, picking and occlusion tests are linear scans per call (and
  occlusion is edges x faces). Fine for furniture; needs spatial indexing for big models.
- The whole scene is rebuilt and re-uploaded on every document change.

## Testing / infra
- Two full test runs once died without a failing assertion, likely a GPU driver crash with
  several wgpu devices at once. UI tests now take turns on the GPU; watch for recurrences.
