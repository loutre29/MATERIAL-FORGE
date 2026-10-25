# Material Forge (folder: SUBSTANCE PAINTER RETRO)

A clean-room PBR 3D texture painter written in Rust (wgpu 22 + eframe/egui
0.29), modelled on the workflow of Adobe Substance 3D Painter.

**Clean-room rule:** no Adobe binary is disassembled or decompiled and no Adobe
code, shader, preset or asset is copied. Behaviour is reproduced from using the
real application as a black box, its public documentation and open formats
(OBJ, FBX binary layout, glTF, PNG, EXR). Materials, smart materials, masks,
brushes and presets in `library.rs` / `brush.rs` / `export.rs` are authored
here. Keep it that way.

Renamed from SUBSTANCE PAINTER RETRO on 2026-10-21: binary `material_forge.exe`,
window / dialogs say Material Forge, logo `assets/material_forge_logo.png` (the user's
"Mf" tile, 1024 px square, embedded by `src/logo.rs`; `assets/material_forge.ico`
is built from it and embedded by `build.rs` via winresource). Projects keep the `.sprp` extension and the user
library stays in %APPDATA%\SP_RETRO for compatibility.

## Build and run

```
cargo build --release
target\release\material_forge.exe                 # empty session
target\release\material_forge.exe model.fbx       # new project from a mesh (2048 px)
target\release\material_forge.exe project.sprp    # open a project
target\release\material_forge.exe --selftest <out_dir> [mesh]
target\release\material_forge.exe --uitest <out_dir>   # MF_UITEST_ONLY=path or uvgizmo: that check alone (seconds)
target\release\material_forge.exe --bench
target\release\material_forge.exe --uibench <mesh>   # per-section frame timings, headless (SP_BENCH_SIZE=WxH)
target\release\material_forge.exe --uicrawl <out_dir> # clicks every widget: report + screenshots (MF_UITEST_SIZE, MF_CRAWL_ONLY=<state>)
target\release\material_forge.exe --floodbench <mesh> [res]  # magic wand / bucket timings per set
```

UX direction (2026-10-23): read `docs/UX_AUDIT.md` before UI work. Painter
stays the functional reference and its vocabulary stays (texture set, fill
layer, smart material, generator, bake), but the app gets its own look,
Photoshop keys and the Photoshop tools painters ask for (selections, magic
wand, bucket, gradient...), with one place per action.

`--uicrawl` (app/crawl.rs) builds a test project, then in 23 app states
(each layer kind / mask / effect selected, every tool, tabs, flyouts,
dialogs) clicks every clickable widget with real mouse events (left and
right button), then every entry of the popups, menus and windows those
clicks open. After each click it restores the document, the UI fields and
egui's memory. It reports STOLEN (egui gave the click to another widget),
UNREACHABLE, NO EFFECT, CLASH (egui id clash warnings), PANIC, INVARIANT
(selection / target pointing at nothing, stuck stroke) and UNDO (undoing the
click's entries does not restore the stack), and writes `crawl_report.txt`
plus one screenshot per state with the findings outlined. Run it after UI
changes at 1600x950 and 1280x720 (MF_UITEST_SIZE): it must report 0 serious
findings (exit code 0). It takes about 2 minutes; run a copy of the exe so
builds can go on. Tests set MF_DATA_DIR (library, layout and autosaves go
there) and `app::HEADLESS` (native file dialogs return None).

Toolchain: Rust stable (MSVC) in `~/.cargo/bin`, VS 2022 Build Tools.
`--selftest` drives the whole engine headless on the GPU and asserts results
(import, AA bake, smart material, fills, filters, mask stacks, wrap / camera /
UV strokes, material painting, clone, polygon fill, geometry mask, history,
every export preset and format, project save + reload, resolution change,
shelf thumbnails) and writes screenshots to `<out_dir>`. Run it after every
engine change. `--uitest` drives the real UI headless through egui with
synthetic input (every window, layer kind, mask effect, property editor,
display mode, tool and shelf item; mouse strokes with every tool; full
history unwind and replay; save, reopen, resolution change). Run it after UI
changes. `--bench` times compositing of a 22-layer stack.

The device requests the adapter's full limits (the compositor binds up to 20
textures per pass) and FLOAT32_FILTERABLE when available.

## Feature checklist (vs Substance 3D Painter)

Done:
- Import OBJ / glTF / GLB / binary FBX, vertex colors, objects, MikkTSpace;
  mesh reimport keeping layers by material name. Meshes keep their polygons
  (`SubMesh::tri_polys`: quads and n-gons of OBJ / FBX; triangle-only files
  get tris-to-quads pairing, `mesh::quad_polys`); the wireframe hides
  triangulation diagonals; projects store the polygons.
- Texture sets, per-set resolution (resampling), alpha modes, emissive.
- Channels: base color, roughness, metallic, height, emissive, opacity, normal.
- Layers: paint, fill (uniform / texture / 20 procedurals, UV, tri-planar,
  planar, object-space projection), filters (HSL, levels, blur, sharpen,
  gradient map...), folders, 28 blend modes per channel, opacity per channel,
  thumbnails, drag and drop, duplicate, group.
- Masks: effect stacks (paint, fill, 9 generators, levels, blur, invert,
  threshold, ID color selection), white / black masks, smart masks. The
  "Add ..." menus are flat like Painter's (`panels::effect_menu`): Add
  generator / paint / fill / levels / filter / color selection; the
  generator, fill source and filter are then chosen in Properties. Mask
  menu: white, black, bitmap (black + fill effect), color selection.
  Effect rows sit under their layer like Painter's (`layers::effect_row`:
  red tint for mask effects, type icon, name, blend, opacity, remove); the
  bar under a thumbnail is red when that stack has effects. Dragged layer
  rows follow the pointer as a ghost card (`layer_drag_ghost`).
- UV projection like Painter: Filtering (Bilinear HQ / Sharp / Nearest),
  UV Wrap (None / Repeat horizontally / vertically / Repeat), Tiling plus a
  per-axis scale (`Projection::stretch`) and a pivot. The 2D view shows the
  transformation manipulator of the selected UV fill (`app/uvgizmo.rs`):
  drawn along the frame's own screen axes: U (red) and V (green) arrows
  move along one axis (`UvDragKind::MoveAxis`), the yellow square and the
  inside move freely (image, frame and pivot together: pivot += d, offset
  -= d; Shift: one axis), the blue ring rotates around the pivot with a
  fan for the turn so far (Shift: 15 degrees), frame-oriented handles
  scale (Shift: keep aspect, arrows on hover), the pivot cross moves
  alone; hover highlight, direction cursors and a readout by the cursor;
  context toolbar: show, handle size, mirror U / V, reset pivot, reset.
  It edits `uvgizmo::sel_projection(_mut)`: the selected fill layer's
  content, or a selected Fill effect of a mask (the options bar too).
- 2D view rotation (`ViewSettings::uv_rot`, radians): Alt + LMB drag
  turns the view about the middle of its visible part (`visible_part`:
  what the floating panels leave free), drawn while turning with an up
  arrow (Shift: 15 degrees); F resets, a "View rotated N" chip with a
  cross resets. Every UV <-> screen mapping goes
  through `view3d.rs uv_px` / `px_uv` (`uv_of`, `uv_point`, `uv_mapping`,
  `path.rs uv_to_local`, `uvgizmo uv_screen`); uvview.wgsl / ants.wgsl
  take a 2x2 matrix; UV stamps use `angle - uv_rot`; selections on a
  turned view rasterize the polygon in UV.
  The frame is fixed on the texture (`tex_of_frame`: frame q shows
  texture coordinate (q - 0.5) * tiling + 0.5), so it follows the image
  whatever the pivot. A color image's alpha is the fill's coverage in
  every channel (sources.wgsl `cut`): a PNG decal leaves the layers under
  it where it is transparent.
- File > Open recent and the start screen list the last 10 projects
  (`library::recent_projects`, recent.json next to the library).
- Any image, alpha file or built-in alpha drops into any fill channel or
  mask fill slot (`Cmd::AssignAlphaShape` turns a built-in alpha into a
  project image).
- Fill channel slots: right click Clear (back to uniform); the Material mode
  slot: Clear material / Clear all maps. A texture in a scalar channel maps
  0 to 1 as it is (no range change).
- Selections (Photoshop's, in the 3D and the 2D view; `app/selection.rs`,
  `engine/select.rs`, `shaders/select.wgsl`): one selection per texture set
  (`TextureSet::selection`, r8 in UV space, None = nothing selected,
  replaced by a new texture on every change so `Entry::Selection` undo just
  swaps it). Rectangle / ellipse marquee (M, Shift + M), lasso / polygonal
  lasso (L, Shift + L: clicks, Enter or a click on the first point closes,
  Backspace drops a point), magic wand (W). Shift adds, Ctrl subtracts
  (Alt is the camera), Shift + Ctrl intersects, or the context bar modes.
  3D shapes are rasterized on the CPU into a screen-size coverage mask
  (`rasterize_polygon`, 4 sub-rows) and merged per texel through the camera
  (visible surface only unless "Through"); 2D shapes the same in the 2D
  view's pixels. Every paint pass multiplies its weight by the selection
  (`paint.wgsl` binding 6, `PaintPipelines::selection`, set by
  `bind_selection` before each draw; `sel_override` for the bucket).
  Select menu: all, deselect, reselect, invert, fill (Shift + Backspace),
  clear (Delete). Marching ants (`shaders/ants.wgsl`) in both views read
  the selection through the set's nearest-covered map (no false edges on UV
  seams), animated at 16 Hz while a selection exists; a "Selection active"
  badge shows in the view corner.
- Magic wand and paint bucket (G): `src/flood.rs` floods texels of similar
  8-bit sRGB color + alpha (tolerance 0..255, contiguous or global, all
  layers = the set's composited base color or the selected paint layer),
  in UV space plus seam links (`Seams::build`: texel pairs along every mesh
  edge whose two triangles have different UVs, edges keyed by welded
  positions), so contiguous regions follow the surface. The bucket fills
  the region (intersected with the selection) with the brush material.
  4K on LULJETA: readback 0.2 to 0.5 s, flood up to 0.5 s (`--floodbench`).
- Select > Modify: grow / shrink / feather / border by a radius in texels
  (`flood::modify`, chamfer distances through UV neighbors and seam links,
  so it continues across seams). Polygon fill has a Fill / Select result:
  in Select mode its faces, loops, mesh parts or UV chunks become the
  selection (`select_tris`, the select pass over an index subset).
- Gradient (G / Shift + G with the bucket): drag a line in 3D (through the
  camera, visible surface) or 2D (UV); linear / radial / reflected, the
  brush material from full at the start to transparent at the end, times
  the opacity, clipped by the selection (`paint.wgsl` modes 4 / 5, the line
  in `stamps[0]`; `fill_tris_with`).
- Selection brush (Q, X swaps add / subtract): a brush stroke painting
  the selection itself (`PaintPass::Select / SelectErase`, `fs_select`,
  r8 blending w + dst (1 - w) or dst (1 - w); `ActiveStroke::selection`,
  `flush_selection_stamps`; the selection is copied at the stroke start so
  undo swaps back). Select > By mesh map: edges / cavities (curvature),
  occluded areas (AO), thin parts (thickness) over a threshold (needs the
  bake). Select > Mask the selected layer with the selection / New fill
  layer from the selection: a paint mask effect holding the selection.
- Layer solo: Alt + click on a layer's eye shows only that layer (its
  folders shown, its children as they were); again restores every layer's
  visibility (`Cmd::Solo`, `RetroApp::solo`).
- One place per action (2026-10-23): no Mode menu (bake: File > Bake mesh
  maps, render: Viewport > Render or the context bar), the tool strip holds
  tools only, Export is the orange button at the right of the menu bar, the
  Viewport menu no longer repeats the display combos, and disabled
  "not available" entries were removed from every menu.
- Tools: paint, eraser, clone (Ctrl + click source), smudge, polygon fill
  like Painter (`view3d::PolySelect`: rectangular marquee, lasso, polygon
  loop, partial polygon loop; marquee / lasso fill modes triangle / polygon / mesh / UV
  chunk, hit-test intersection or contained center, visible polygons only;
  the active set's wireframe shows, `Frame::wire_only`; the selection builds
  while dragging and fills on release as one undo step backed up by tiles;
  topology and loops in `app/polysel.rs`), color picker, geometry mask
  (hide / isolate, same selection), stencil projection, material painting (any shelf
  material or the selected fill, evaluated per texel).
- Generators can react to height painted below them (anchor-like workflow:
  edge wear around painted scratches and bolts).
- Brush: alphas (the analytic round tip + image tips), size / flow / hardness / spacing,
  angle, follow path, jitters, lazy mouse, symmetry X/Y/Z, wrap and camera
  alignment, line tool (Shift + click), pen pressure (size / flow; untested
  without a tablet), brush presets.
- Starter alphas and textures (own-generated, `tools/gen_starter_assets.py`,
  numpy + PIL, Hack font for the words): `starter_assets/alphas/<folder>/`
  (116 at 512 px: Brush tips, Grunge, Patterns, Shapes, Stitches and
  hardware, Symbols) and `starter_assets/textures/<folder>/` (48 tileable
  1024 px: Grunges, Patterns, Stamps). `library::starter_files` lists them;
  the shelf's Folder menu filters alphas, textures and brushes by folder
  (`item_folder`, `library::file_folder`). The Alphas shelf keeps only the
  analytic Round of the 12 built-in kinds (the others stay for old projects
  and as preset fallbacks).
- Brush presets (`brush::builtin_brushes`, 51 in folders Basic, Artistic,
  Grunge, Stitches, Hardware, Stamps): `BrushPreset::alpha_file` names a
  starter alpha imported as a project resource on apply
  (`apply_brush_preset`: size, symmetry, lazy and pen settings stay the
  user's). Without a project the preset's built-in fallback tip stands in;
  `App::brush_tip` reloads the file when a project opens. Stitches use
  follow path with spacing = the element's length. Thumbnails are strokes
  (`brush::stroke_image`, also the Properties preview), rendered on a
  worker. Follow-path strokes place their first stamp on the first move
  (its direction known); a click without a move stamps at release
  (`Stroke::finish`).
- Path tool (P, `app/path.rs`): non-destructive path layers. A path
  layer is a paint layer with `LayerDesc::paths: Some(Vec<PathDesc>)` and
  `LayerDesc::path_style: Some(PathStyle)` (the layer row shows a path
  badge). All paths of a layer share its style (brush, material, fill,
  `unit` / `uv_unit`): other settings mean another path layer ("New path
  layer" in the Paths section and the options bar, `new_path_layer`).
  Projects from when each path had its own brush migrate on load
  (`migrate_path_style`, called by `sync_path_layers`: the first path's
  settings; `PathDesc::old_*` are read-only serde fields). Its paint is never painted by hand
  (`resolve_paint_target` refuses), it is regenerated: `sync_path_layers`
  (every frame, before `sync_brush_textures`) hashes each path layer's
  paths + resolution + mesh and, when that differs from `path_painted`,
  `paint_path_layer` clears the layer and paints every visible path
  (`path_stamps`, chunks of MAX_STAMPS, one submit per chunk since the
  stamp buffer is written through the queue). Undo needs nothing special:
  edits change the desc, `track_edits` / `checkpoint("Edit path")` snapshot
  it, an undo restores the desc and the hash mismatch repaints.
  `PathDesc`: id, name, visible, uv, closed, points (`PathPoint`: pos,
  nrm, h_in / h_out relative, `PointKind` Auto = handles from the
  neighbors / Smooth = in line / Broken = independent, zero = corner),
  `right` (camera right when made, stamp frame of non follow-path
  brushes), `seed` (jitter). `PathStyle::unit` / `uv_unit`: world or UV
  units per brush point, set by the layer's first 3D / UV path (the brush
  size stays in points).
  3D paths are cubic Beziers in world space, sampled finely and projected
  back onto the surface along the interpolated normal (`doc.pick` both
  ways), stamped in MODE_WRAP with the stamp's tangent in `ext.yzw`
  (paint.wgsl uses it instead of the camera right): camera independent.
  Editing: click on the mesh = new path (on the active path layer, else a
  new "Path layer N"), or a new point when an end of the selected path is
  selected; click on a curve inserts a point; click the other end closes;
  drag points / handles (handles on the point's tangent plane); Shift +
  click a point = Auto / corner, Shift + drag a handle = break the
  tangent (Alt is the camera); Pen mode: click and drag pulls out handles.
  Backspace / Delete remove the point (the path goes with its last one),
  Escape / Enter end the path. A drag is one undo step (track_edits
  merges into the "Edit path" step while `path.drag` is set).
  `sync_path_brush`: while the tool is active on a path layer, the
  layer's style is loaded into `self.brush` / `self.mat` (the tool's own
  are kept in `PathEdit::saved` and come back when leaving the tool or
  the path layer), so every brush / alpha / material control and the
  Ctrl + drag gestures edit all the layer's paths live; `bound` (set,
  layer, JSON last synced) tells an edit from an undo. Properties show a Paths section (select, paint on / off,
  rename, delete). Paths are only drawn while the tool is active; the
  options bar has Curve / Pen, the stitch presets, Closed, End path,
  Delete path, size / flow / spacing.
- Projection tool (Shift + B, `Tool::Projection`, dock icon Stencil):
  paints the stencil texture without the brush's stencil option
  (`stencil_active`); entering it opens Properties > Stencil, centers a
  stencil never placed or out of view (`stencil_fit`) and asks for a
  texture when none is set; options bar: Texture, Fit to view, Opacity,
  then the brush.
- Ctrl + LMB (flow / angle) and Ctrl + RMB (size / hardness) hold the
  cursor: hidden, sent back to the press point after each move
  (`ViewportCommand::CursorPosition`, `BrushGesture`; a move counts once:
  the position before the warp event arrives is skipped); the circle
  stays at the press. Tests set `lock_cursor = false`.
- Split view swap: F4, View > Swap, and a swap button in the view bar
  when Split is on; saved with the panels (`Panels::swap_views`,
  `view_split` in layout_v3.json).
- UV projection frame (2D view manipulator, `uvgizmo.rs tex_of_frame`):
  a higher tiling packs repeats around the frame's middle instead of
  moving the frame to the bottom right.
- Tool shortcuts box (bottom left of the 3D view, `draw_modifier_helper`):
  sits above the canvas status line and above the dock / options bar when
  they reach it (`helper_bottom`); a "Don't show shortcuts" checkbox
  under it (egui Area) turns `ViewSettings::show_tool_keys` off, Edit >
  Preferences turns it back on. The split view divider is not drawn over
  the dock and options bar.
- Brush cursor: image alphas show under the cursor (`draw_alpha_overlay`,
  white with the alpha as coverage, rotated by the angle and, while a
  follow-path stroke moves, its direction; `alpha_overlay` caches it).
- Auto UV (`mesh::AutoUv` on import: meshes without UVs, or every mesh
  with "Auto unwrap" in the New project dialog; `mesh::unwrap_mesh` for an
  open project). Each material group gets its own 0-1 space (one texture
  set). Two methods (`UvMethod`):
  - Smart (default, `mesh/smartuv.rs`): exact weld (as the packer does);
    seams at hard edges (angle option), split normals, part borders, open
    borders, never inside a polygon; dead-end seams (slits) pruned
    (`prune_dead_ends`); every chart cut into a disk (tubes: one cut
    joining their borders, closed parts: a cut between their two farthest
    points; `dijkstra_straight` over edges costing length x visibility x
    turn penalty (TURN), visibility = ambient occlusion openness of 24 rays
    per vertex, creases cheaper); non-manifold edges pair each face with
    its smoothest continuation. Geometry is shared read-only (`Geo`,
    `Group`) and charts flatten in parallel each round. LSCM per disk
    (pinned at its two farthest border points, sparse Cholesky with faer
    0.22). Quad straightening: a chart's quads (`chart_quads`,
    `quad_frame`) are solved by least squares to a grid, rows only or
    columns only (`straighten`); `flatten` keeps it only when nothing folds
    and its p95 / mean stretch stay close to the conformal map's. A chart
    with flips, overlaps (grid SAT test, same rules as the packer), stretch
    over the limit or a triangle crushed over 10x (`CRUSHED`) first gets a
    pelt seam to its farthest tip (`tip_cut`), else is split along a quad
    edge loop through a narrow hidden neck, else by a minimum cut
    (`min_cut`, `cut_by_field`, `grow`: edge length x visibility) between
    the overlapping triangles, by surface direction or by position;
    triangles without area (packer's rule) are `ignored_faces`. Charts are
    sorted by triangle so cached flattenings stay matched.
    `merge_charts` / `try_merge` (parallel) put small islands back into a
    bigger neighbor when the union is a disk, folds nothing and stretches
    under 2.0, part borders kept as seams. `upright` (option "Keep
    upright", default on) returns the allowed quarter turns per chart: +Y
    up charts 0 only, flat ones 0 / 180, straightened ones any quarter
    turn. uvunwrap 0.2 (MIT) is vendored in `vendor/uvunwrap` with our
    additions: `Options::keep_orientation` (skips the min-area-rectangle
    turn in `param.rs finalize`), `MeshInput::face_turns` (allowed turns
    per face, honored by masks, placement and row folding),
    `Options::order` (`PackOrder` Perimeter / Area / Height / LongSide),
    and in `topology.rs` edges shared by more than 2 faces keep matching
    faces together. `pack_best` packs with the 4 orders plus the row
    packer in parallel (free mode: x2 orientation modes) and keeps the
    fullest page. The packer's v grows upward and makes
    islands wind counter-clockwise that way; ours grows downward, so v is
    negated going in and flipped (1 - v) coming out (Fast too), else every
    island is mirrored (textures read backwards). The selftest and
    `--autouv` count mirrored triangles (`mirrored_uvs`). MF_UV_DEBUG=1
    logs every stage.
  - Fast: uvunwrap's own charts (xatlas style).
  Luljeta (52k tris, 4 sets), Smart islands 185/83/69/36 (Fast
  778/181/124/86) in ~4 s, p95 stretch 1.49/1.35/1.32/1.44, coverage
  upright 69/56/64/48 %, free 71/66/70/49 %, no overlap, 16 mirrored of
  51452 (the file's own); ARMOR keeps ~185 islands because of the file's
  hard / split-normal edges (real 90 degree rims). Selftest: overlap
  raster (`uv_overlap`) and a wavy quad grid that must come out straight.
  `--autouv <mesh> <dir> [smart|free|fast]` prints the reports
  (islands, coverage, stretch, seam length), writes `autouv.obj` and four
  checker renders. Top right "Auto UV" button: `app/autouv.rs` window
  (texture set chips: only the chosen sets are unwrapped,
  `mesh::unwrap_sets`, the others keep their UVs, vertices and paint;
  method, hard edges, hide seams, max stretch, keep upright, padding;
  unwrap on a thread; layout preview per set; Apply). Apply = `Document::apply_new_uvs`:
  every paint layer texture and painted mask is resampled to the new
  layout on the GPU (`engine/uvtransfer.rs`: new-UV triangles reading the
  old texture, nearest texel, a 1.5 texel grown pass first for borders),
  then `reimport`; history cleared, bake window opened (maps follow the
  UVs). File > Export mesh (OBJ) writes the UVs out (`mesh::write_obj`,
  v flipped). High polys load with `AutoUv::Skip`.
- Assets panel (left, like SP): all libraries / project / per category,
  search, grid or list, material-ball thumbnails, user library in
  %APPDATA%\SP_RETRO\library. Drag and drop: assets onto layer rows (insert
  above, smart masks apply to that layer), onto the 3D view (texture set
  under the cursor), onto Properties slots (fill channels, mask fill, alpha,
  stencil, brush material); image files dropped on the window become project
  resources; layer rows drag to reorder.
- Window: see "Canvas layout" under UI conventions (floating panels, tool
  dock, view bar, app menu File / Edit (Preferences, Ctrl+K: interface options and shortcuts) / Select / Window / View).
- Bakers (AO 64 rays, edge-aware UV denoise of AO and thickness;
  curvature from the mesh per welded vertex, sharp edges as a falloff,
  scaled to the set's own range; `--bakeshot <mesh> <dir> [res]` writes the
  UV maps): AO, curvature, normal from high poly (optional match by name,
  bolt_low <- bolt_high), world normal, position, thickness, ID (object /
  vertex color / UV island / material), AA 2x2 / 4x4. AO and thickness rays
  run on the GPU (engine/gpubake.rs), identical sample pattern to the CPU.
- Viewport: HDR IBL, key-light shadows, bloom, tonemappers, vignette,
  wireframe, channel and mesh-map views, layer-mask view, 2D UV view
  (paintable), orthographic (F5) and preset views, progressive path-traced
  render mode (F4, engine/pathtrace.rs).
- Export: 7 presets + editable user presets, PNG 8/16, TGA, JPEG, TIFF 8/16,
  EXR, size scaling, padding infinite / dilation / none.
- History for every action (history panel), projects (.sprp v2), autosave
  to %APPDATA%\SP_RETRO\autosave.

Missing (roughly 10% of daily use, by my estimate):
- UDIM tiles (one texture set spanning several UV tiles).
- Layer instancing, particle / physics brushes, Python plugins, text tool.
- Environment importance sampling in the path tracer (it converges slowly
  on small bright lights).
- Substance (.sbsar) materials: proprietary format, out of scope.
- Content effects stacked inside a layer (we use filter layers instead).

## Layout

```
src/main.rs              entry, device limits, --selftest / --bench
src/app/mod.rs           app state, frame loop, history tracking, bake thread, thumbnails
src/app/panels.rs        menu bar, Cmd + run_cmd, shared property widgets (combo, projection, filters)
src/app/props.rs         Properties panel: tool settings, fill / mask / effect editors, drop slots
src/app/layers.rs        right dock: texture set list, layer rows, texture set settings tab
src/app/dock.rs          canvas layout: floating panels, view bar, tool dock, options bar, app menu host
src/app/layout.rs        tool options (the options bar content), flyout switching, geometry mask helpers
src/app/icons.rs         painter-drawn icons, icon button, panel header
src/app/widgets.rs       Painter widgets: label/value slider, gradient slider, color bar, chips, slots, cards, icon tabs, picker popup
src/app/view3d.rs        3D / 2D views, navigation, strokes, clone, smudge, fill, hide, asset drops
src/app/selection.rs     selection tools, magic wand, bucket, fill / clear selection, marching ants overlay hooks
src/flood.rs             flood fill over a set's texels with UV seam links
src/engine/select.rs     selection textures, shape rasterizer, select pass, ants pipeline pieces
src/app/shelf.rs         Assets panel, asset items, thumbnails, drag preview, apply_item
src/app/dialogs.rs       new project, export (preset editor), bake, display settings, history
src/document.rs          layer tree model, masks, fills, filters, plan building, caches keys
src/history.rs           undo / redo (stroke texture backups + stack snapshots)
src/brush.rs             brush settings, alpha shapes, stroke engine
src/library.rs           built-in materials / smart materials / smart masks, user library
src/preview.rs           thumbnail renderer (material ball mini document)
src/engine/composite.rs  plan execution, mask cache, prefix cache, finalize + mips
src/engine/paint.rs      brush pipelines (pass A color/values/alphas, pass B emissive/normal/opacity)
src/engine/viewport.rs   shadow, HDR scene, bloom, post
src/engine/uvview.rs     2D view
src/engine/thumbs.rs     layer thumbnails
src/engine/blit.rs       resampling
src/shaders/*.wgsl       common, procedural, stack (split at //// ENTRY), sources, paint, viewport, post, uvview
src/bake.rs              UV raster, nearest map, BVH, all bakers, connectivity
src/mesh/*.rs            loaders
src/export.rs            presets and writers
src/project.rs           .sprp save / load
src/selftest.rs          headless end-to-end test and bench
```

## UI conventions

- Panels move by their header and resize by every edge and corner (a
  docked panel's inner edge only changes its docked width; anything else
  makes it float, `Panels::rects`; a double click on the header docks it
  back); Window > Lock panels freezes them, Reset panels restores the
  layout. The arrangement is saved in `layout_v3.json` (not in tests).
- Canvas layout (`app/dock.rs`, since 2026-10-23, the approved design in
  `docs/design/material_forge_design_system.html`): the views fill the
  window (CentralPanel); rounded panels float over them as egui Areas
  (`float_panel`: elevation-1 shadow, radius 12, 10 px from the window
  edge; the panel rect is allocated first so the content lies on top for
  hover and wheel): left 272 px (project button with the app menu, Layers /
  Texture sets / History segmented tabs), right column 312 px (Library on
  top, Properties below, the gap between them drags the split). Bars
  (`float_bar`, anchored to the free canvas `canvas_mid`): view bar top
  center (display combos, 3D / Split / UV, camera, lighting flyout,
  render; bake progress under it), History / Bake / Export top right, the
  tool dock bottom center (`DOCK`: sections Select, Paint, Fill, Utilities
  of slots; every tool has its own slot except three stacks: the four
  marquee / lasso shapes, bucket + gradient, picker + hide; a stack shows
  its last tool, a click on the active stack or a right click opens the
  others) and the active tool's options above it (`tool_options`, wraps to
  two rows when narrow). Flyouts (display settings, history) hang under
  their button (`flyout_at`). The canvas corner shows only the last
  message, for 5 seconds (no fps / VRAM line). Sliders
  (`widgets::Slider`): label | pill track with an orange fill and a white
  knob | value box, one 28 px row (`slider_label_width` aligns the label
  column with combo / toggle rows); toggle rows are switches.
  `widgets::slide` is the same slider without its label (grid rows in the
  bake, display and texture set settings windows; no `egui::Slider`
  anywhere); `gradient_slider` (uniform channel values) is a rounded
  black-to-white pill with a knob of the value's gray. Panels move by
  their header (`Panels::offsets`, double click puts one back) and resize
  by their inner edge (`left_w`, `right_w`); with the left panel hidden,
  a logo + menu button sits in the top left corner (`home_menu`, with
  "Show the layers panel"). View overlays (labels, badge, axes, modifier
  help, toasts) use `visible_part` (the view minus the panels); toasts sit
  under the view bar. Texture set settings is
  a window (gear in the layers panel). Panel visibility is `dock::Panels`
  (`tab_open` / `focus_tab` / `toggle_tab` keep the old tab names); there
  is no menu bar, right icon strip or status bar any more (status text is
  faint in the canvas corner, left of the dock). In Split the views divide
  the visible canvas; `uv_fit` frames the UV square in the part the panels
  leave visible. Never let a row overflow its panel (`widgets::label_width`,
  truncating combos, `horizontal_wrapped`).
- Content effects (`LayerDesc.effects`, `Target::ContentEffect`): filters on
  a layer's content; the layer + effects compile to an implicit folder plan
  with the layer mask on the folder. Mask effects are listed under the row
  only when the mask is selected, content effects when the content is.
- Optional channels (`NEXT` = 14, `EXT_NAMES`, enabled per texture set in
  Texture Set Settings > Channels +): Specular level and Transmission (on
  by default, `EXT_ON_DEFAULT`; old projects get them on via `de_ext_on`)
  live in the spare `.w` of the accumulators (mrh.w, emi.w) and finals
  (emissive.w = specular level, normal.w = transmission) and shade the
  viewport. The others (Ambient occlusion, Glossiness, Displacement,
  Scattering, Coat opacity / roughness, Sheen opacity / roughness,
  Anisotropy level / angle, IOR, Absorption) are packed four per extra
  accumulator (`ext_slot(i)`: group (i-2)/4), ACC = 7 textures;
  `Arena::acc` keeps unused groups 1x1 and `TextureSetGpu::final_ext`
  ([Tex;3], `ensure_ext`) is allocated on use. Values are stored 0..1
  (`EXT_RANGE`: angle 0-360, IOR 1-3; Properties shows real units).
  Finalize multiplies the painted AO into orm.r. Fill only (uniform /
  procedural); they composite, export (`Src::Ext`) and display
  (`Display::Ext`), but only AO, specular and transmission change the
  realtime shading. Their chips sit in the fill's channel chip row and
  their slots right after the main ones; in the display menus they follow
  Opacity (C cycles `0..=Display::index(Opacity)`).
- Replace on normal / height: `nrm.w` is a hidden height (common.wgsl
  `blend_layer`); folders and finalize use height - hidden, so Replace
  hides what is under.
- Properties channel slots show a cross (`slot clear:{title}`) once a map
  or procedural is set.
- Library filters: removable chips in the panel header next to
  "Library" (`dock::panel_header_with`, `shelf.rs filter_strip`:
  source, type, folder, "search"); the ones that don't fit go under a
  "+N" chip whose popup lists them with Clear all.
- Typed slider values (`widgets::typed_range`) go past the track: no
  upper limit, below it only down to 0 (tracks from 0) or min / 1000
  (positive tracks); percentages and dragging stay in range. Costly
  counts are capped where set (bake samples 16384, bounces 64), shader
  loops too (`strokes` reach <= 7 cells, blur 64 texels).
- Layer and brush material previews (`Previewer::render_fill`,
  `preview::Look`): one scene-lit material ball for every fill, graded
  like the viewport (environment, exposure, tone mapping, saturation,
  contrast, key light), the environment turned so its key light is in
  front of the ball, up left (`front_lit_rotation`; a ball lit from
  behind read as a wrong material); the texture set's base values and
  optional channels are copied in (what shows where a fill writes
  nothing); cache keys include look, environment and those settings; UV
  fills get stretch[0] x2 on the ball (square texels). `--layershot <project>
  <dir>` puts each fill's thumbnail next to the model with only it shown.
- Color fields: `widgets::color_bar` opens the Painter-style picker window
  (SV square + hue, hex, HSV / RGB, swatches); the eyedropper arms
  `eyedropper_id()` and the 3D view writes the sampled base color back.
- Painting on fill / filter / folder content is refused with a viewport
  toast (like the real app); select the mask or a paint layer.
- Alpha blended sets draw their triangles back to front
  (`document::sort_blended`, re-sorted when the camera or visible geometry
  changes). Sets with the transmission channel use two-sided refraction
  (Wyman): the opaque scene is copied (color + resolved depth), their back
  faces write normal + eye distance (`fs_backface`), then their front faces
  (`fs_transmit`, IOR 1.45) refract in, exit through the recorded back face
  (total internal reflection reflects) and read the scene where the ray
  lands, or the environment in that direction; roughness blurs, base color
  tints, alpha blend mixes over the scene. `--alphashot <dir> [mesh]` renders
  blending (MF_TRANSMIT=1: transmission) for checks.
- Specular anti-aliasing (Toksvig): normal-map mips that average to a
  shorter normal add roughness, so bumps below a pixel (distance,
  thumbnails) keep the material's look. Texture material thumbnails use the
  full 1K maps on a 1024 set.
- Viewport texture magnification: cubic B-spline from four bilinear taps,
  faded in where a texel covers more than a pixel (`sample_hq`).
- Path tracer: transmission (rough refraction, IOR 1.45), cutout and alpha
  blend pass-through, per-slot alpha modes in `PtUniform`. Denoiser: Intel
  Open Image Denoise 2.5.1 (Apache 2.0, `third_party/oidn`, shipped as
  `oidn\` next to the exe; `engine/oidn.rs` loads the DLLs at runtime with
  libloading and runs the "RT" filter on its own thread, GPU through CUDA /
  HIP when available, else CPU). At 4, 16, 64, 256, 1024, 4096 samples and
  the last one, `after_submit` reads the radiance, first-hit albedo and
  normal back and submits a job; `present` uploads the result and copies it
  over the view (results of an older accumulation, `accum_id`, are
  dropped). Until the first image arrives, or without the DLLs
  (MF_NO_OIDN=1 forces it), the raw render shows: there is no built-in
  filter any more (the user wants OIDN or noise, nothing in between).
  Offline renders (`--matpreviews`, `--alphashot`) call `denoise_now`.
  `--alphashot` with MF_PT=<spp> (MF_NODENOISE: raw). View settings >
  Denoiser: Off (noisy render) / Intel Open Image Denoise
  (`PathTracer::denoise_on`), with OIDN's state and device
  (`oidn_status`, `Denoiser::device` from oidnGetDeviceInt "type"); the
  render button's tooltip names the denoiser. Bakes keep their edge-aware
  UV-space filter (OIDN only sees the 2D image and would blur across UV
  islands that touch in UV but not on the mesh).
- Crawl ALIGN check (`crawl::alignment_findings`, report only): text in a
  clickable box must be centered within 2.5 px and inside it (visible part,
  same layer; stacked lines as a block).
- Environment rotation starts at 180 degrees.
- Present mode Mailbox with one queued frame (MF_VSYNC=1: classic vsync).
  Alt + Shift + LMB orbit snaps to the axis views. Ctrl + S shows "Saving"
  then a "Saved" toast (`pending_save`).
- Environment: one CC0 HDRI ships in `starter_assets/environments`
  ("Immenstadter Horn", Poly Haven, chosen as the closest look to Painter's
  default; credits in CREDITS.txt), loaded at startup. Painter's own
  environments are Adobe assets with no free license: never read or ship
  them. Reflections use a GGX-prefiltered copy (`env.rs prefilter`,
  `shaders/prefilter.wgsl`, mip = roughness * 5). Tone mapping defaults to
  linear, bloom / vignette off.
- Assets: library menu (All libraries / Project / Starter assets / Your
  assets), type icons (Materials default), Filters as assets, procedurals
  under Textures with flat thumbnails, smart materials on a rounded box,
  materials on a plain sphere. Hovering a texture shows a 256 px preview.
- Materials shelf: `starter_assets/materials/<Category>/<id>/` (100 CC0
  materials from Poly Haven and ambientCG, 10 per folder, 1K JPG maps
  color / roughness / height / normal (OpenGL) / optional metallic plus
  `material.json`; `index.json` lists sources and download counts).
  `library::texture_materials` scans them (`starter_dir`: next to the exe,
  above it in the repo, or `MF_STARTER_DIR`); `Item::TexMaterial` imports
  the maps as project resources on apply (`tex_material_fill`, reused by
  name) and builds the fill (`TexMaterial::fill`: maps as they are, height
  at 25 %, UV tiling 4). Their thumbnails use `Previewer::new_material`
  (lit exactly like the viewport's defaults: the environment at 180 degrees
  plus its key light, `viewport_key_light`; UV tiling 2 on the sphere).
  They wait for the environment to load and render again when it changes
  (`Shelf::thumb_env`). Starter materials ship a pre-rendered `preview.png`
  (`--matpreviews [filter]`: sphere displaced by the height map, path
  traced with the denoiser in the built-in studio light, alpha from the
  coverage; ambientCG-like), used for the shelf tile and the hover preview. Thumbnails decode the
  maps on a thread (`Shelf::decoded`). One list by name, like Painter. The
  procedural presets (`library::materials`) remain for the brush material
  slot and tests.
- Starter alphas originality: `docs/ALPHA_ORIGINALITY.md`,
  `tools/compare_alphas.py` (correlation with the user's shelf
  screenshots; only generic primitives come close).
- Smart materials (`library::smart_materials`, 18): recipes built on the
  starter texture materials (by name; a recipe whose material is missing
  is left out) with generator masks (edge wear, cavity, dirt, light from
  above, position gradient) and starter grunge textures as mask fills. The
  tree reads its files through placeholder resource ids (`1 << 48 + n`);
  the shelf item is `SmartMaterial(name, tree, Some(starter_dir), files
  with absolute paths)`, remapped to project resources on apply like user
  smart materials. Thumbnails decode the files on a worker
  (`Shelf::decoded_files`, 512 px) and render on the rounded box (user
  smart materials now show their textures too). The old procedural stacks
  are `procedural_smart_materials` (self test and bench only). The self
  test checks every brush tip and recipe and writes `brushes.png` /
  `smart_materials.png` sheets. `--matsheet <dir>` renders every
  material thumbnail into one sheet; `--matshot <dir> [name|calib]` renders
  large spheres (as applied / no height / no bump, or a roughness ramp). The installer ships `starter_assets`.
- Edit tracking hashes the active set each frame (`fingerprint_hash`) and
  snapshots only on change; `History::version` tells explicit history ops
  (commands, undo, redo) apart from panel edits.

Reference: `docs/SP_UX_SPEC.md` (behavior spec built from Adobe's public
docs: shortcuts, panels, layer stack, dialogs) and
`docs/reference_captures/` (captures of the real application on this PC;
Painter was driven black-box: launched with `--enable-remote-scripting`,
`sp.sh python '...'` creates a project from a mesh, `spclick.ps1` posts
clicks to its window without moving the cursor, popups can be captured but
not clicked). The theme lives in `style.rs`, the tokens of the design system
(graphite surfaces BG / PANEL / FIELD / HOVER / PRESSED, LINE /
SEPARATOR outlines, TEXT / TEXT2 / TEXT3, forge orange ACCENT, MASK red,
R_PANEL 12, R_CONTROL 8, GAP 10, `shadow(level)`); painted dark fills use
`style::warm(g)` (a theme gray) instead of `Color32::from_gray`; fonts are
Segoe UI Variable (semibold for `style::bold`) and Cascadia Mono for
numbers, loaded from C:\Windows\Fonts. Library chips: `widgets::pill`.

- Key map (Photoshop's, since 2026-10-23): B brush, E eraser, R smudge,
  S clone, M / L / W selections, G bucket, U polygon fill, H geometry mask,
  I temporary picker, P path, 1..9 / 0 brush flow, Y symmetry, V clone source,
  D lazy mouse, C / Shift + C material and channels, N / Shift + N mesh
  maps, F1 to F4 layouts (F4 swaps), F5 / F6 camera, F8 / F9 / F10 modes,
  Tab hides docks (intercepted in `filter_input`, eframe's raw_input_hook,
  so egui does not move focus), Alt + Q isolate, Ctrl + A / D / Shift + D /
  Shift + I selections, Ctrl + C / V / J / G layers, Alt + arrows reorder.
  The stencil (projection) is an option of the brush (Properties >
  Stencil), not a tool. Gestures: Alt + mouse camera, Ctrl + LMB flow /
  angle, Ctrl + RMB size / hardness, Shift + RMB environment, T + LMB / MMB
  / RMB stencil, plain RMB click quick properties.
- Escape order (`RetroApp::escape`): popup, quick properties, ID pick,
  dialogs, then flyouts. Display settings / History / Log are flyouts
  anchored next to the right icon strip, one at a time.
- Properties: fill / filter / folder layers and non-paint effects always
  show their own properties; paint tools show BRUSH / ALPHA / STENCIL /
  MATERIAL. Never allocate more than `available_width()` inside a panel
  (SidePanel grows every frame otherwise); ComboBox adds padding to `width`.

- Drop zones use `drop_hover` / `drop_take` (app/mod.rs): pointer layer +
  rect, and they only take payloads of their own type. egui's
  `dnd_release_payload` consumes payloads of any type and its hit test can be
  stolen by a tooltip near the cursor; do not use it.
- `mark(ctx, name, rect)` records widget rects for `--uitest` (asset0,
  layer_top, slot:<label>).
- Widget ids: never derive an interactive id from `ui.id()` + a label.
  Child UIs share their parent's `ui.id()` and labels repeat ("Scale" in
  every procedural); a clashing id leaves all but one widget dead. Use the
  allocation's response id (`widgets::Slider` does) or an explicit salt.
- Tests and benches set `persist_layout = false`: they must never read or
  write the user's `%APPDATA%\SP_RETRO\layout_v2.json`.
- Mouse presses on the 3D / 2D views go through `press_is_ours`: the press
  must start on the view itself (egui's hit test), in its layer, and not
  while a popup / menu / quick properties was open (`overlay_open`: the
  click that dismisses a popup never paints).
- Popups: `widgets::popup_below` (opens leftward / upward near the window
  edges). Submenus inside context menus and popups: `widgets::sub_menu`
  (unfolds in place; egui's flyout submenus open over their parent at the
  right edge). Menu bar menus keep egui's `menu_button`.
- Selection, target and folder expansion are not undo steps:
  `fingerprint_hash` skips them (`view_hash` has them). `checkpoint` marks
  the document modified. Layer moves take a `Place` (above / below / into),
  and every op that changes `selected` resets `target` to Content.
- Layer shortcuts (Delete, Ctrl + C / V / D / G) act only when the layer
  stack has the focus (`layers_focus`: last click in it) or the mouse is
  over it. Escape: popup, color picker, quick properties, dialogs, flyouts;
  an open menu bar menu closes itself first.
- Number fields (`widgets::NumberEdit`, used by sliders and the grayscale
  channel bars): click to type, Enter or a click elsewhere applies, Escape
  cancels; an edit is dropped when its field is not drawn or now shows
  another value. Sliders: Shift drag fine-tunes, Ctrl snaps, arrows step.
- Labels are not selectable (`selectable_labels = false`).
- A new mask has no effects (like Painter): painting on it creates its
  paint effect. Geometry mask changes are undo steps (`Entry::Hidden`).
- Layers multi-select: `TextureSet::extra` (Ctrl + click toggles, Shift +
  click ranges over `visible_rows`), `selection()` is everything; drags carry
  `panels::LayerDrag(ids)`, Delete / Ctrl + G / Ctrl + D act on the whole
  selection (`delete_layers`, `group_layers`, `duplicate_layers`,
  `move_layers_onto` with `selection_roots`). `prune_selection` runs per frame.
- Projections have a 3D placement (`Projection::position / orient / axes`,
  `WorldMap` maps the position map to mesh space); the fill / mask shaders
  get it as affine rows (`x0..x2`, `n0..n2`). The viewport manipulator is
  `app/gizmo.rs` (W move, E rotate, R scale; drags win over the brush).
- Eyedropper (`app/eyedrop.rs`): armed by a color field, its click is taken
  out of egui's input in `filter_input`; it samples the mesh base color over
  the 3D view, the screen pixel (GDI) anywhere else, also outside the window.
- Imports ask project or library (`ImportDialog`); export presets of the
  user can be deleted.
- Auto shader alpha counts paint layers only when a stroke painted opacity
  (`LayerDesc::opacity_painted`); blended sets get a depth pre-pass of their
  opaque texels (`fs_prepass`). The eraser never allocates pass B textures.

## Data conventions

- UVs are stored image-space (origin top-left, v down). Loaders flip OBJ/FBX v.
- Tangent space is OpenGL (+v up); DirectX presets flip green on export.
- Paint layer textures are premultiplied rgba16f: color, values (r, m, h),
  alphas (r, m, h, opacity coverage), and lazily emissive, normal, opacity (r16f).
  Mask paint effects are rg16f (value * coverage, coverage). Both start as
  1x1 placeholders (`PaintTex::empty`, `new_mask_paint`) and get their full
  size from `Layer::materialize` before the first stroke / fill; shaders read
  them with `load_clamped`. Unbaked mesh maps (ao, curvature, normal,
  thickness, id) are 1x1 constants too until `alloc_baked`; the mask display
  texture is allocated only while the mask display is on.
- Compositor (`engine/composite.rs`) runs in 1024 tiles: scratch textures
  are tile sized (plus a halo for blurs, mask blurs and the height-reactive
  generator ring, `plan_halo`); mirror / transform filters run untiled.
  Shaders get `tile.wgsl`'s uniform (dispatch origin, scratch origin, owned
  rect): scratch textures are read in tile space, set textures globally.
  Finalize is dispatched over `TextureSetGpu::dilation_rect` (texels whose
  nearest covered texel the tile owns), so gutters have exactly one owner.
  `TextureSet::dirty_region` (set by brush flushes when nothing else changed,
  `dirty` wins) recomposites only that rect; `app/region.rs` finds it from
  the stamps (screen / world / UV bboxes of the triangles under them) and the
  paint pass is scissored to it. Mask caches are refreshed in place only when
  valid elsewhere (`mask_policy`); the prefix cache is kept up to 2048.
  The selftest compares tiled vs untiled and region vs full composites.
- Brush strokes back up only the 256-texel tiles they touch, right before
  painting them (`Entry::stroke_tiles`, `backup_region`); polygon fill and
  mask clears keep whole-texture backups.
- Accumulator slot: col (rgb, opacity), mrh, emi, nrm. Finals per set: base
  (rgb, opacity), orm (ao, rough, metal, height), normal, emissive, with mips.
- Caches: masks per (set uid, layer id, hash of desc + layer.rev + bake_rev);
  prefix of the stack below the edited layer's top-level ancestor. Bump
  `layer.rev = next_rev()` whenever a layer's textures change.

## Procedurals (Painter's shelf)

`procedurals.rs` lists Painter's shelf procedurals by folder (Noises, Noises
Advanced, Grunges, Patterns, Geometric, Gradient; the 20 classic generators
are the "Classic" folder and keep their codes). Each one is a family +
variant of `shaders/patterns.wgsl` (seamless: integer cell counts, wrapped
lattices) with hidden constants `v` and the parameters Properties shows
(`ProcParams::p`). `engine/procgen.rs` renders them once into an rgba16f
texture with mips (size `res_for(set_res)`: half the set, 256 to 1024),
cached by their parameters; fills and mask fills sample that texture with
the projection (flag 4: gray ramp between the two values). The 3D noises
(`ProcKind::is_3d`) are evaluated per texel in object space
(`procedural.wgsl`, `analytic_id`). Looks were authored against the public
Designer node documentation images (clean-room: nothing copied).
`--procsheet <dir>` renders every one to PNG for comparison.
- Flakes (`ProcKind::Flakes`, folder "Normals", analytic_id 27): a normal
  procedural (voronoi cells as tilted mirrors; Scale, Tilt, Coverage) for
  the normal slot (`sources.wgsl` kind 27).
- Fill layer rows show the material ball (metal reads as metal), with
  the exact uniform base color in a small corner square (the eyedropper
  picks it); a fill writing only a uniform color (and opacity) is a flat
  swatch.

## Performance (RTX 4060)

- The 3D and 2D views render only when their inputs change (`view_key`:
  camera / post / set uniforms, size, wireframe, environment, `vis_rev`,
  composites, path tracing in progress).
- Thumbnails are queued only for visible tiles and open pickers
  (`Thumbs::missing`); image files, texture materials and hover previews
  decode on worker threads (two at a time); the asset grid skips rows out of
  view. Swapchain latency 2 (steady 60 fps).
- Still open (from the profiling pass): per-dispatch bind group / buffer
  creation in the compositor and 44 mip dispatches per edit, region search
  over every triangle for wrap strokes, autosave reading textures on the UI
  thread, the prefix cache off at 4096.


2048, 24 layers: full composite ~27 ms, a stroke frame (256 x 256 region)
~2 ms. 4096, 24 layers: full composite ~105 ms, stroke frame ~5 ms.
LULJETA, 4 sets at 4096: ~4 GB of VRAM, paint frame ~5 ms, fill edit ~31 ms,
resolution change of the 4 sets ~4 s (CPU maps in parallel, `make_sets`).
Before tiling / lazy textures that project needed 7.5 GB and paged.
Bake (1024, AA 2x2): 243k-triangle watch 7 s (was 48 s on the CPU).
Path tracer: ~700 samples per second at 960 x 600 on simple scenes.

## Tested meshes

Built-in box + sphere (selftest), `LULJETA_SUBSTANCE_EXPORT.fbx` (bounds match
Blender), `Egypt obelisk.fbx`, `WATCH DESIGN.obj` (243k tris).
