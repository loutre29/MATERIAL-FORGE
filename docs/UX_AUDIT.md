# Material Forge: UX audit and simplification plan (2026-10-23)

Goal: make Material Forge feel like its own product, much simpler than
Substance 3D Painter, and give painters the Photoshop tools the Painter
community has asked for since 2015 (selections, magic wand, paint bucket,
gradients, moving painted content), in the 3D view and the 2D UV view.

Sources: a full inventory of the current UI (every menu, tool, panel,
dialog, shortcut) and a survey of Adobe's community ideas board, Polycount
and competitor docs (Mari, 3DCoat, Blender, ArmorPaint). Painter 12.2.0
(2026-10-21) is the latest version checked.

## 0. Decisions (user, 2026-10-23)

- Shortcuts: Photoshop keys (section 3.4).
- Vocabulary: keep Painter's words (texture set, fill layer, smart
  material, generator, bake...). Section 3.1 is NOT applied; only the
  selection concepts are new words.
- Masks: keep the effect stack as it is (the "simple mask" of 3.6 is NOT
  applied).
- Order: selection tools (phases 1 and 2) and the simplification pass
  (phase 3, minus renames and masks) are built together.

## 0b. Progress

Done (2026-10-23): selection core (rectangle, ellipse, lasso, polygonal
lasso in 3D and 2D, add / subtract / intersect, Select menu, marching
ants, clipping of every paint path, fill / clear, undo), magic wand,
paint bucket, gradient, grow / shrink / feather / border, polygon fill to
selection, Photoshop keys, tool strip of tools only, Mode menu removed,
Export button, disabled entries removed, dock icons fixed.

Also done: selection brush (Q), select by mesh map, mask / fill layer
from the selection, layer solo (Alt + click on the eye).

Assets (2026-10-23): new starter alphas (116, by folder, stitches and
hardware included) and grunge / pattern / stamp textures (48), 51 brush
presets with real tips (stitches, zipper, rivets, chalk, charcoal, spray,
splatter, leaks...), 18 smart materials built on the scanned materials,
Folder menu for alphas, textures and brushes, and a Path tool (P) that
strokes a curve with any brush (stitches and seams follow it).

2026-10-24 (user feedback): alpha overlay under the cursor, Projection
tool (Shift + B), pen mode for the path with a stitch preset menu, text
alignment pass (crawler ALIGN check, status elided before the dock, view
overlays clear of the dock, value boxes sized to their numbers), alpha
originality report, auto UV on import with OBJ mesh export, denoiser
choice with Open Image Denoise's device shown.

Next: load a mask as selection, move / transform selection content, blur
/ sharpen brushes, merge down / flatten, a docked color panel, symmetry
everywhere, paint across texture sets, a faster wand (cached colors).

## 1. What the community asks for (and Painter still lacks)

Ranked by signal, with Painter's status as of 12.2:

| # | Request | Painter 12.2 | Material Forge today |
|---|---|---|---|
| 1 | Selection tools: marquee, ellipse, lasso, polygonal lasso, in 3D and 2D | missing (staff: "not yet") | polygon fill only (topology based, gone after release) |
| 2 | Magic wand / select by color on painted content | missing (only ID-map color selection) | ID-map color selection mask effect |
| 3 | Paint bucket: fill a painted region by color | missing | missing |
| 4 | Move / scale / rotate / warp painted content | decals only | missing |
| 5 | Paint across texture sets | missing (top voted idea, 44 votes) | missing |
| 6 | Performance with many layers and 4K | still a complaint | tiled compositor, region updates (good) |
| 7 | Solo / isolate a layer | missing (plugin promised) | missing |
| 8 | Seam-free 3D blur | in development | missing (UV blur only) |
| 9 | Hide / isolate by mesh island or UV island with one click | partial | geometry mask with marquee / lasso / loops |
| 10 | Gradient tool dragged in the viewport | missing | missing (gradient map filter only) |
| 11 | Symmetry everywhere (multi-axis, 2D view, fills) | partial | wrap brush only |
| 12 | Hand-painting comfort: docked color panel, swatches, one-key pick | missing | color picker popup, P picks the material |
| 13 | Real blend / blur / liquify brushes | partial (smudge) | smudge only |
| 14 | Visible save failures, safer autosave, finer undo | open | autosave, history panel |
| 15 | Photoshop round trip (PSD with layers and masks) | under review | missing |
| 16 | Flatten / merge layers | added in 12.0 | missing |
| 17 | Text tool | added in 10.0 | missing |
| 18 | Less jargon, less clutter, a readable Properties panel | top UX complaint | same jargon as Painter |
| 19 | No subscription, Linux | Steam-only perpetual | (business decision) |

Competitor reference for selections: Mari has a marquee with lasso,
rectangle, polygon, ellipse and magic wand shapes, Replace / Add / Subtract
/ Intersect, feather, and a transformable paint buffer. 3DCoat freezes
(= selects) with rectangle, ellipse, polygon and freehand lassos, grows,
shrinks, feathers, inverts and fills the unfrozen area. Nobody offers a
magic wand that works on painted content across UV seams in 3D: that is
our opening.

## 2. What is wrong with the current UI (from the inventory)

### 2.1 The same action in too many places

- Bake: 6 entry points (Mode menu, tool strip, context bar icon, Texture
  set settings button, F8, Ctrl+Shift+B).
- Add a mask: 6 entry points. Toggle a mask: 5.
- Display settings: 3 places. Shadows and wireframe: 2 each. Display mode:
  menu with 20 entries, corner combo, C / B / M keys.
- Brush size / flow / hardness / spacing: in the context bar and again in
  Properties (hardness 3 times).
- Filters exist as two concepts (filter layers and content effects).
  Levels exists 3 ways, Invert 4 ways.
- Three different pickers (tool P, the color-field eyedropper, "Pick from
  mesh" for ID colors).
- "Projection" means three things: a tool (3), the fill mapping, and a
  stencil toggle. Pressing 1 silently turns the stencil off.

### 2.2 Painter jargon a newcomer cannot decode

Texture set, smart material, smart mask, generator, mesh maps, bake, ID
map, color selection, Material mode + "Or", channel chips (rough / metal /
nrm / op / emiss), blend abbreviations (Ldge, AddS, NMC), tri-planar,
"Object 3D (procedurals)", hit-test, contained center, UV chunk fill,
partial polygon loop, shader alpha, height to normal, cage distance,
padding infinite, histogram scan, highpass, balance, disorder, painted
height, geometry mask.

### 2.3 Concepts that are heavier than they need to be

- Mask = a stack of up to 8 kinds of effects with 9 blend modes. Most
  masks are "paint + one auto mask + contrast". The stack is a power-user
  tool shown to everyone.
- Every fill shows a 7-row per-channel blending table.
- Polygon fill, geometry mask (hide) and the color selection mask effect
  are three separate tools that all answer "which part of the model?".
  None of them leaves a selection you can reuse.
- Mode menu: "Baking" is a dialog, not a mode; "Rendering" is a view
  toggle.
- New project needs a dialog, then a separate bake before masks work.

### 2.4 Half-finished items still on screen

Disabled entries: Add mask with height combination (2 places), Add
compare mask (2), Add anchor point (2), Add generator / paint / fill /
color selection on content. The polygon fill hint says the eraser unfills
(it cannot). Symmetry only works with the wrap brush. The stencil is off
in the 2D view. A static "Main shader" label. An unreachable Procedurals
folder row. Dock icons mismatched (Properties shows the layer icon, Layers
the mask icon). Mirror V is a text button. Fixed already: a log line still
said the environment came "from the installed Substance 3D Painter" (it
loads the bundled CC0 HDRI).

## 3. The new model (one idea per thing, Photoshop vocabulary)

### 3.1 Words (rejected: Painter's words stay)

| Painter word (today) | Material Forge word |
|---|---|
| Texture set | Part (one per material of the model) |
| Fill layer | Material layer |
| Filter layer / content effect | Adjustment layer (one concept) |
| Folder | Group |
| Smart material | Material preset (saved layer group) |
| Smart mask | Mask preset |
| Generator | Auto mask (Edges, Cavities, Dirt, Wear, Height, Light, Thickness, Occlusion) |
| Mesh maps / bake | Model maps / Analyze model |
| ID map + color selection | Select by part / color (a selection tool) |
| Polygon fill / geometry mask | Select faces / islands / objects, then Fill or Hide |
| Material mode "Or" channels | Material (one slot) or Custom channels |
| Channel chips color / rough / metal | full names, icons with tooltips |

### 3.2 Selections become first class (new)

One active selection per part, stored as a soft grayscale texture in UV
space. Every tool reads it.

- Made in the 3D view or the 2D view with: Rectangle, Ellipse, Lasso,
  Polygonal lasso, Magic wand, Selection brush, Select faces (face / loop /
  island / object / material color), Select by model map (edges, cavities,
  occlusion, thickness, height above / below a threshold: unique to us).
- Modifiers like Photoshop: Shift add, Alt subtract, Shift+Alt intersect,
  plus the same as buttons in the context bar.
- Select menu: All, Deselect, Reselect, Invert, Grow, Shrink, Feather,
  Smooth, Border, Select visible faces only, Save selection as mask, Load
  mask as selection.
- In 3D, shapes select through the camera (visible surface only by
  default, "through" optional); in 2D they select in UV space. Magic wand
  and grow / shrink / feather cross UV seams (seam table from the mesh),
  so they behave in 3D like they look.
- Marching ants in both views; the rest of the model can be dimmed.
- Everything is clipped by the selection: brush, eraser, smudge, clone,
  material painting, bucket, gradient, adjustment layers created while a
  selection exists get it as their mask.
- One-click results: Fill selection (Shift+Backspace), New material layer
  from selection (its mask is the selection), Hide selection, Isolate
  selection.

This single concept replaces polygon fill, the geometry mask tool's own
selection UI, the ID color selection effect and the "Pick from mesh" ID
picker.

### 3.3 New painting tools

- Paint bucket: click fills the region of similar color (tolerance,
  contiguous on the surface across seams or global, sample this layer or
  all layers) with the brush color or a material; inside a selection it
  fills the selection.
- Gradient: linear / radial / reflected, dragged in 3D (screen space onto
  the surface) or 2D, color or material to transparent, clipped by the
  selection.
- Blur / sharpen brush (seam aware in phase 4), smudge stays.
- Move / transform selection content (Mari's paint buffer idea): lift the
  selected pixels of a paint layer, move / rotate / scale them in 2D or on
  the surface, commit.
- Layer solo (Alt+click the eye), Merge down, Flatten group.
- Color panel: a docked HSV wheel + swatches + recent colors, I picks a
  color anywhere (one picker, Alt+click with the brush).

### 3.4 Tool strip (left, grouped, Photoshop keys)

| Key | Tool group (flyout) |
|---|---|
| V | Move / transform (fills, stencil, selection content; replaces W / E / R and the UV gizmo mode) |
| M | Rectangle, Ellipse select |
| L | Lasso, Polygonal lasso |
| W | Magic wand, Selection brush, Select faces |
| B | Brush (stencil is an option of the brush, not a tool) |
| E | Eraser |
| G | Paint bucket, Gradient |
| R | Smudge, Blur, Sharpen |
| S | Clone |
| I | Color picker |
| H | Hide / isolate faces |

Bake, Export and Render leave the tool strip: Export is one button in the
top bar (Ctrl+E repeats the last export), Render is a view toggle,
analyzing the model happens automatically on import (section 3.6).

### 3.5 Panels

- Left: Tools strip + Library (Materials, Presets, Brushes, Stamps (alphas),
  Images, Patterns (procedurals), Lighting (environments)). Search on top.
- Center: 3D view, 2D view on demand (Tab key cycles 3D / split / 2D).
- Right: Layers on top; below it one contextual panel that shows only what
  the current tool or layer needs, with Basic sections open and an
  "Advanced" fold for the rest (per-channel blending, projection details,
  jitters, stencil).
- Top bar: menus (File, Edit, Select, Layer, View, Help), then the tool's
  options (size, opacity / flow, hardness, spacing; shown only here, not
  again in the panel), then Export.
- Removed: Mode menu, right icon dock (its entries move to View and the
  panels), separate Texture set settings tab (a Part settings popover from
  the part menu).

### 3.6 Simpler flows

- Open a model: drop the file, it opens with defaults (2K, one part per
  material), model maps are computed in the background; auto masks work
  as soon as they are ready. No New project dialog unless asked.
- (Rejected: masks keep the stack.) Masks: a layer has one mask = painting + optional auto mask + four
  sliders (contrast, softness, invert, threshold). The effect stack stays
  available behind "Advanced mask stack" for power users.
- Blending: one blend mode + opacity per layer by default; per-channel
  blending under Advanced.
- Export: choose the target (Unreal, Unity, Godot, Blender, glTF /
  Sketchfab, Photoshop PSD later), folder, size. Custom presets under
  Advanced (already done in the compact export window).
- Display: one corner menu per view (Material, each channel, model maps,
  mask); lighting and post in one View > Lighting popover.

## 4. How it fits the engine (implementation notes)

- Selection texture: `R8Unorm` per part at the set resolution, allocated
  on first use, 1x1 white when nothing is selected. Bound in paint group 0
  and multiplied into `brush_weight` in every `fs_*` of `paint.wgsl`;
  polygon fill / bucket / gradient draws (`MODE_FULL`) read it too.
- Combining shapes: a pass rasterizes the texture in UV (`vs_uv`) and
  tests each texel against a shape mask. 3D: the texel's world position is
  projected with the view camera and depth tested (as camera-space stamps
  are), then looked up in a screen-size coverage mask rasterized on the CPU
  from the rectangle / ellipse / lasso polygon (4x4 supersampled edges).
  2D: the shape mask is in UV space directly. Ops by blend state: replace,
  add (max), subtract (dst * (1 - src)), intersect (dst * src).
- Magic wand and bucket: read back the source (the layer or the part's
  composited channel), flood fill on the CPU in UV space with a seam table
  (texel pairs along UV seams from mesh edges shared by two triangles with
  different UVs), tolerance in perceptual color distance; upload the result
  as the shape mask.
- Grow / shrink / feather: UV-space max / min / gaussian passes with the
  seam table (phase 2), then world-distance versions using the position map
  (phase 4, also used by the 3D blur).
- Marching ants: the 3D and 2D view shaders sample the selection and draw
  an animated dashed edge where it crosses 0.5 (`fwidth`); the views only
  animate while a selection exists (low rate, `view_key` gets a tick).
- History: `Entry::Selection` (full backup of the R8 texture: 16 MB at
  4096). Selections are saved in the project.
- Select faces reuses `app/polysel.rs` (loops, islands, visibility) and
  rasterizes the chosen triangles into the selection instead of filling.

## 5. Phases

1. Selection core: selection texture, clipping of every paint path,
   rectangle / ellipse / lasso / polygonal lasso in 3D and 2D, add /
   subtract / intersect, Select menu (all, deselect, invert, save as mask,
   load from mask), marching ants, Fill selection, New material layer from
   selection, history and save. New tool strip with the Photoshop keys.
2. Magic wand, paint bucket, selection brush, select faces (replaces
   polygon fill and the geometry mask selection), select by model map,
   grow / shrink / feather / smooth / border, gradient tool.
3. Simplification pass: renames, top bar and panel layout, contextual
   panel with Advanced folds, simple masks, one place per action, removal
   of disabled items, drop-to-open with background model analysis.
4. Move / transform selection content, blur / sharpen brushes, seam-free
   3D blur, solo, merge down / flatten, color panel, symmetry in all
   spaces, paint across parts, PSD export.

Each phase ends green on `--selftest`, `--uitest` (3 sizes) and
`--uicrawl` (0 serious findings at 1600x950 and 1280x720).
