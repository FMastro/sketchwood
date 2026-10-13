# Backlog

Things we deliberately skipped or know are incomplete, to come back to. Newest first within
each section. Remove items when they're done (mention them in the commit).

## Geometry
- **Auto-joining**: moved or copied geometry doesn't merge with geometry it lands on (shared
  vertices/edges aren't welded). Fine while boards should stay separate; revisit with groups.
- **Coplanar face merging**: two faces on one plane that share an edge stay separate (a seam
  edge remains, e.g. boards glued end to end, or a block pulled flush with a side).
- **Partial overlaps**: a rectangle that partly overlaps a coplanar face doesn't split it;
  the two faces overlap and z-fight. Needs real face splitting (with the Line tool).
- **Pockets at an edge**: a pocket whose wall is flush with an existing side face doesn't cut
  a notch into that face.
- **Line tool and edge-splits-face**: drawing an edge across a face should split it.
- **Move can't bend faces**: SketchUp folds a face that would become non-planar (adds a
  diagonal); we refuse the move instead.

## Inference / snapping
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
