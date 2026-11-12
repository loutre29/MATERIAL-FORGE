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
target\release\material_forge.exe --studioshot <project.sprp | mesh> <out.png>  # packshot (MF_STUDIO=<json>, MF_YAW, MF_PITCH, MF_SPP, MF_LONG, MF_FRAME)
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
  when Split is on; saved with the panels (`Panels::swap_views`, `view_layout` 3D / Split / UV,
  `view_split` in layout_v4.json).
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
- Window: see "Canvas layout" under UI conventions (docked panels, tool
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
src/app/dock.rs          canvas layout: docked panels, view bar, tool dock, options bar, app menu host
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

- Panels dock, Painter style, never float free (user request
  2026-10-26): five panels (`dock::PANEL_IDS`: layers stack, library,
  properties, texture set settings, Muse), each in a `Slot`: the left or right
  column (stacked top down by `Panels::order`, heights shared by
  `Panels::weights`, the gap between two drags their split, each keeps
  120 px) or, for the library only, the bottom strip between the columns
  (`bottom_h`; the tool dock and options bar sit above it, `dock_lift`).
  Carrying a header shows a name tag and the highlighted place it would
  take (`drop_preview` / `drop_target`: a column's width or a 140 px band
  at an empty side, the bottom band for the library; over the canvas it
  stays); `place()` computes every rect and `canvas_mid`. The inner edge
  (facing the canvas) sets the column width or the strip height. Window >
  Lock panels freezes them, Reset panels restores the layout. Default
  (user request 2026-10-26): layers top left, library bottom left, Muse
  top right, properties under it. Saved in `layout_v5.json` (older
  layouts are not read, so the new default shows; not in tests).
  `MF_UITEST_ONLY=panels` and the crawl state "library at the bottom"
  check it.
- NOW CLAUDE (v67, 2026-11-06): the panel below was built on Meta's Muse
  for the hackathon and now runs on Claude, ONLY through the Claude app or
  Claude Code (MCP, v68); the API key mode was removed in v69; since v70
  the panel is a small connect card that hides while Claude is linked. Code names
  (`Muse`, `muse.rs`, `Tab::Muse`, MF_MUSE_*, the `muse/` data folder) are
  kept; everything the user reads says Claude.
- Muse panel (`app/muse.rs`, for the Meta Global AI Developer Hackathon,
  submissions due 2026-11-04): texturing by prompt with Meta's Muse
  models on the Meta Model API (`https://api.meta.ai/v1`, Bearer key from
  MODEL_API_KEY or `<data dir>/muse_key.txt`, typed in the panel; blocking
  `ureq` on a worker thread). Parts (`parts()`: each object of a texture
  set when it holds several, else the set), limited to the texture sets
  checked in the panel (any number; Cancel drops a running job: the
  worker checks a flag between steps, a request in flight finishes and
  is ignored). Muse does not trust part names (user, 2026-10-26:
  they are rarely meaningful). Pieces (`app/pieces.rs`, user 2026-10-27:
  "weird gradient when multiple textures like metal and wood are applied
  to the same texture set"; Muse must understand polygons): each part is
  split into its connected mesh pieces (welded by position; a part of a
  single piece splits into its UV islands), alike pieces grouped (same
  area and box within ~15%: chain links, hooks), at most 36 (the smallest
  of a part merge). `look_at` rasterizes the model on the CPU from four
  sides (2x2, or a row of four tall views for a tall model), each piece in
  its palette color with its number drawn on it (3x5 digit font, labels
  never overlap), the rest gray, sent as an image to `muse-spark-1.3`
  (chat completions, vision) with per-piece facts: part, alike count,
  size, center and y span in Position gradient units
  (`bake::position_map`). The numbered view also shows in the chat, so the
  user can say "make piece 3 wood". The last
  view is saved to `<data dir>/muse/last_view.png`; `--museview <project>
  <out.png>` renders it and prints the facts. Muse answers looks: a list
  of pieces and a layer stack (JSON, bottom first): a source (color, pattern with two
  colors, smart material, material, or `texture_prompt` made by
  `muse-image-1.0`), any channel (roughness/metallic/height as values or
  patterns, emissive, opacity, and optional channels: transmission, IOR
  stored (ior-1)/2, absorption, scattering, coat, sheen, specular, which
  get switched on in the set), blend mode, layer opacity, a mask of
  steps (generators with level/contrast/axis, patterns, levels, invert,
  blur, each with a mask blend) and filters (content effects).
  Realism (user, 2026-10-27: results were "really bad, not realistic"):
  the prompt asks for an "analysis" first (what each part is in real
  life, its real size), scanned materials as bases (grouped by category
  in `Shelf::material_names`), physically based values and subtle wear.
  Scanned material layers are tri-planar at Muse's `scale` (about the
  object's size in meters: a scan covers ~1 m), `tint` is the average
  color to reach (divided by the scan's average, `average_color`),
  `relief` scales its height (0 also drops the normal map),
  `roughness_min/max` remap its roughness. Generators get noise at most
  0.1 unless Muse sets it (library defaults up to 0.5 spread wear
  everywhere; position cuts keep 0). Muse folders go on top of the root
  stack (inserting at the selection put them inside a user folder,
  under the user's layers). Each run saves `muse/last_view.png` and
  `muse/last_plan.json`. `--musetest <project> <dir> <prompt> [sets]`
  runs it headless (key from MODEL_API_KEY) and saves before.png,
  result.png (project camera) and result_full.png; MF_MUSE_PLAN=<json>
  replays a saved plan without the API, MF_MUSE_DUMP=1 prints the stack.
  `muse_apply` builds one folder per look and texture set ("Muse:
  <what>"), masked by a Paint mask effect written on the CPU
  (`pieces::owners`: the piece of each texel from the UVs, grown 4 texels
  into empty space; `pieces::mask`), so materials meet on exact polygon
  borders (no ID map needed). A look takes its pieces from the looks
  that had them (their masks get a new texture, undo keeps the old one;
  an emptied look's folder is replaced in place). Muse may still cut a
  zone of one piece with a sharp Position gradient when a single mesh
  really holds two materials.
  Conversation (user, 2026-10-27: "ask muse to modify something without
  always redoing"): the panel is a chat (`Convo`: the system text and
  views and pieces of the first turn, the (user, reply) turns, the
  `Look`s applied). The first message looks at the model; later ones
  resend the history plus "CURRENT LOOKS" and Muse returns only the looks
  it changes or adds (complete lists), or none for a question. One
  checkpoint per turn ("Muse: <message>"). The panel lays out bottom up
  (buttons and message box pinned, the chat scrolls above them). Sets lock while a conversation
  exists; "New conversation" forgets it (layers stay); a different
  document (`doc_key`) resets it. Replies go through `no_dashes`. A plan
  layer's source keys may also sit on the layer itself (`source()`).
  ambientCG (`src/ambientcg.rs`, CC0): the material catalog (~2000, tags,
  popularity) is cached for a week at `<data dir>/ambientcg_catalog.json`;
  its 300 most used tags go in the instructions; a layer source
  `{"ambientcg": "tag words"}` picks the best match (`best`: category,
  tags, popularity) and downloads its 2K JPG zip (1K fallback) into
  `<library>/materials/<category>/<id>` (color, roughness, metallic,
  height, normal GL, material.json), which `texture_materials()` also
  scans, so it shows in the Materials shelf. `--musetest` splits the
  prompt on `||` into turns and saves turnN.png after each but the last.
  Toolset (user, 2026-10-27: Muse ignored blend modes and could not get
  height on glass; "muse needs to know how to use the software better than
  anyone"): the instructions explain the per-channel compositing exactly
  (`common.wgsl blend_layer`: Normal vs Replace on height, every mode's
  math, recipes). A layer's "blend" is one mode for every channel it gives
  (the normal keeps its own) or an object per channel; "channel_opacity"
  per channel; "projection" (mode, scale, rotation, offset); patterns get
  balance, warp, invert and every `ProcKind` but the normal generators;
  generators get noise_scale, seed, height_reactive; masks get threshold
  and levels gamma; filters every kind plus "channels"; anisotropy and its
  angle. "texture_sets" changes height_scale, alpha_mode and
  emissive_intensity (`sets_text` tells Muse the current ones). Height
  calibration (tested on the lantern glass): slope ~ height_scale x range
  x pattern scale, so fine patterns get 0.03 to 0.1, height_scale stays
  0.02 for most objects. Reference images: the Image button or a drop on
  the panel (`muse_drop`, from `RetroApp` file drops) attach images to the
  next message (thumbnails, click removes); they become R1, R2... (copy in
  `muse/refs`, sent as JPEG data URLs of at most 1024 px with their turn,
  resent with the history); a layer source {"reference": n} uses one as a
  texture. Every turn's plan is kept in `muse/plans/turn_N.json`.
  `--musetest` with MF_MUSE_REFS=<image;image> attaches images to the
  first message.
  Since v37 the material choice is the point, not the layers (the user:
  Muse over-layered and washed out good scans): the instructions ask for
  1 to 3 layers per look, no dust, bleaching or wear unless asked or seen
  in a reference, no Cells / Voronoi hammer marks (they read as cracks).
  Each scanned material is listed with its average color and roughness
  (`library::material_stats`, read from its maps once, cached in
  `<data>/material_stats.json` by folder and color map mtime). While the
  Muse panel is open, any dropped image goes to Muse (Windows gives no
  pointer position during a file drag) and a "Drop images for Muse"
  overlay shows while files hover; reference thumbnails sit in fixed rows
  (`ref_row`, bigger on hover); a click on the numbered views shows them
  over the whole window (`muse_view_big`, click or Escape closes).
  MF_MUSE_ATTACH=<image;image> attaches images at start (UI checks).
  v38: scan scale never below 1 (stretched scans looked low quality);
  a scan's tint is normalized to at most 1 per channel (it can darken or
  shift the hue, never brighten), so the tint bar shows the real value
  and white gives the scan back. `--musetest` takes MF_VIEW_TONEMAP (0
  ACES, 1 filmic, 2 AgX-like, 3 linear) and MF_VIEW_EXPOSURE. The
  viewport defaults to linear (clamp) tone mapping (user's choice, back
  to it on 2026-10-28 after a while on AgX-like).
  v40, reliability: (1) the MATERIAL SHEET: every scan's preview sphere
  numbered in list order (`material_lines` returns it; `muse/material_
  sheet.png`), sent as the second image of the first message, the
  numbers also in the text list. (2) SELF CHECK: after a turn that changed
  the model (not a replay, not MF_MUSE_NOCHECK), `Muse.check` waits until
  no set is left to composite, `muse_shot` renders front and back off
  screen (its own `Viewport`, the view's look), and a check turn asks
  Muse to fix only what is clearly wrong; it is remembered without its
  render, its plan is `plans/turn_N_check.json`, its render
  `muse/last_check.png`, and it is its own undo step. (3) GUARDS
  (`guard_plan`, on the plan JSON before it is applied): a scan tinted
  far (CIELAB distance above 20) from its own average gives way to the
  closest scan of its category sharing a material word (`kind_words`:
  wicker is not taken for oak); cell / Voronoi height on a scanned look is
  dropped; at most 4 layers per look.
  v41 panel: not connected, a "Connect Muse" card (opens KEYS_URL
  https://dev.meta.ai/ via explorer; "Paste key and connect" reads the
  clipboard with arboard, or the key is typed) checks the key with a free
  GET /v1/models (401: refused) before saving it; Meta has no official
  sign-in for third-party apps (a community route reuses Muse Code's own
  client: not used). Connected: a bar with the Pieces chip, New
  conversation and a "Connected" chip (Disconnect deletes the key file);
  texture sets as chips; an empty state with example prompts; a status
  card with Cancel; the composer (attachments, text, + Image, Send) is a
  bottom panel `show_inside`, the chat a central panel. PIECES ON THE
  MODEL replace the numbered image in the chat: `muse_update_pieces`
  builds a 1024 piece-color texture per set (`pieces::owners`) shown by
  display code 13 (shaded display color, viewport.wgsl) over the
  material view, and `muse_piece_labels` draws the numbers on the view:
  `piece_label_spots` renders the model at 240 px on the CPU with a depth
  test (cached per camera) and puts each label on the visible pixel
  nearest the middle of what its piece shows. MF_MUSE_PIECES=1 turns them
  on at start (UI checks). An empty MODEL_API_KEY no longer hides the
  saved key.
  v42 PRINTS (labels, logos, screens; a TV session tiled tiny labels all
  over the housing with Transform scale 0.035, guessed planar offsets
  blind, and its self check then deleted the prints). New projection
  mode ProjMode::Decal (code 6): `position` / `orient` (x right, y up, z
  out of the surface) / `axes` (width, height, depth, mesh units) make a
  box; `Projection::decal_uniform` maps the position map into its 0..1
  space (v down) and sources.wgsl `decal_cov` cuts the whole layer to 0
  outside it and on faces turned away (smoothstep on the normal). Muse
  places a print with "decal": [x0, y0, x1, y1] (pixels of the numbered
  views image; `views_of` is the framing `look_at` uses, `Views::hit`
  raycasts the look's triangles, the box edges go onto the surface plane,
  upright as that view shows it, image aspect kept, `trimmed` crops the
  reference's blank border once into muse/prints) or "fit": true
  (`decal_fit`: stretched over the look's pieces along their summed
  normal; refused when they do not face one way). Transform filters are
  dropped on placed prints; a print's "emissive" uses its image (a lit
  screen). Prints do not count in the 4-layer cap, and `keep_prints`
  puts back any print a self check leaves out. --musetest also opens a
  mesh file as a new project (MF_MUSE_PLAN is a file path).
  v43 PRINTS IN THE UV. `surfaces_of` builds the SURFACES image (third
  image of the first message, muse/last_surfaces.png): the main UV islands
  (`pieces::components` with uv, at least 0.4% of the area, up to 24),
  each flattened along its summed normal and upright, all at one scale
  (packed in rows, 1536 wide), what sits in front dimmed, a grid every 5%
  of the longest side, numbered; its facts go in the instructions. A print
  placed with "surface": k, "box" (SURFACES pixels) becomes a decal in that
  island's frame (`Surface::place`). {"text", "font", "bold", "color"}
  draws text with ab_glyph from a Windows font (`text_fonts`,
  `text_image`, cached in muse/prints). `uv_sheet_of` flags attached
  images laid out on a set's UV map (content inside the islands, spread
  over the image) and the message tells Muse to use "uv_sheet": true (UV
  projection, as is). The self check also gets the SURFACES image as the
  model looks now (`surfaces_now`, from read-back final base and emissive;
  muse/last_check_surfaces.png). `keep_prints` only restores prints no
  look of the check kept; `screen_glass` turns see-through glass over a
  glowing print into dark glossy glass carrying that image (transmission
  shows the environment, not the pieces behind). MF_MUSE_SAVE=<sprp> keeps
  a --musetest result as a project.
  v44 REALISM. `muse_prepare` (first message): sets in scope go up to 2048
  px (history cleared) and unbaked ones start a bake (--musetest waits for
  it). Each look carries "finish" (FINISHES) and "age" 0..1;
  `apply_finishes` rebuilds its "Finish:" layers (`finish_layers`:
  roughness variation, smudges, fine scratches, grime from Dirt x grunge,
  dust from Directional light, rubbed, polished or chipped edges, opacity
  by age); they and prints are outside the 4 layer cap. Muse is told not to
  make its own everyday wear. The check render is 2x2: front and back, then
  `muse_closeups` (the two surfaces with the most print area, else the
  largest not facing down, seen from the side; `Surface::to_ru`). A print
  placed with "surface" gets a painted mask of that surface's piece (the
  box no longer prints on what sits in front). `flat_screens` drops height
  layers from looks with a glowing print.
  v45 PRINT PLACEMENT (any object). `surfaces_of` keeps, per surface, which
  piece wins each pixel in front of it (`Surface::cover`) and lists the
  compact parts in front (`front`: "4b" = piece 4's second part, with its
  box; frames around a surface are left out) in the facts
  (muse/last_surfaces.txt). A print can be anchored: "surface", "near",
  "side" (above, below, left, right), "size", "gap" (`Surface::near_box`;
  above or below it is no wider than the space to the next part on its
  row). Muse keeps where each surface print went (`Muse::placed`) and the
  self check gets `placement_facts`: each print's box, the share hidden
  behind pieces in front (pixel by pixel), off its surface, overlaps.
  v46 IMAGES AS TOOLS (any object). `muse_project_files`: the images the
  user imported into the project (Resource.imported) go with the next
  message as references, once each (`Muse::files_sent`, written to
  muse/files; same name as an attached ref: skipped), marked "project
  file". `image_facts` adds each image's size as Muse sees it (max 1024),
  transparency, black and white. `uv_sheet_of` also tells a UV layout
  (fills the islands with a few flat colors: a guide, never a texture)
  and the UV sheet note is conditional (never a photo or logo). Layer
  source {"sample": n, "region", "flatten", "scale"}: `sample_texture`
  cuts the region, evens its light falloff, makes it seamless (half shift
  cross-fade) and it tiles like a scan. Mask steps and *_pattern maps take
  {"reference": n, "uv_sheet" | "scale"}: `image_maps`/`gray_map` import
  a gray version (`gray_image`, lightness x alpha) as "texture_id"; a UV
  sheet channel map puts the layer on the UV map. muse/last_message.txt
  keeps the last user message as sent.
  v47 UV-AWARE PROJECTION (any object). `uv_fit` measures each piece's UVs
  (coverage, degenerate share, uneven texel size, squash, overlap) and the
  length direction of every UV island: UV-space PCA, turned a quarter
  when the strip wraps a tube (its ends meet in 3D), else the 3D long
  axis carried into the UV. `uv_facts` sends this to Muse ("UV LAYOUT of
  each piece", muse/last_uv.txt). Projection UV takes "along": "length" |
  "across": `fill_grain` measures the grain of the fill's own map (image,
  or the pattern rendered at 256, structure tensor in `grain_of`) and
  `uv_turn` rotates it onto the island direction; UV "scale" is in model
  units like tri-planar (`UvFit::unit`). Islands that point different
  ways (`uv_groups`, 15 degrees, at most 6) split the layer into a folder
  with one copy per direction, each masked to its islands
  (`muse_mask_islands`). Cut-outs: `ambientcg::best` skips single /
  decal / leaf assets unless asked, and `is_cutout` rejects a downloaded
  scan whose color map is an object on a flat background. `best` also
  needs a material word match (category or tag), so color and finish
  words alone ("green smooth") pick nothing.
  v48 A CALMER SELF CHECK. The check used to redo every look (retuned
  tints and scales, swapped scans for flat colors), mostly for the worse.
  Now its prompt keeps the first answer unless the render shows a clear
  defect, each returned look must name it in "problem", and
  `guard_check` drops looks without one, or whose base was a scan and is
  no longer a found scan (fix a wrong scan with another scan or a
  texture_prompt, never plain colors). Each ambientCG layer of the
  stored looks carries "got" (name, id, category, tags), so Muse can
  judge the scan it really got. `ambientcg::best(cat, query, avoid)`:
  stems plurals (leaves, leaf), requires the query's material noun (a
  catalog category word) when it has one, "-word" excludes a tag, and a
  new query never gets back a scan the looks already use (`scans_of`,
  `Request::scans`).
  v49 GLOSSY STAYS SMOOTH, PHOTO SAMPLES TRUE TO COLOR (ADP cap and
  glaze). The lumps came from Muse's own instructions (broad waviness
  "can use the whole range"; "polished surfaces still need relief"): on
  a glossy surface the reflection magnifies every bump. Now the HEIGHT
  text and the glass recipe say glossy surfaces (roughness 0.3 or less)
  stay smooth, and `guard_plan` caps every height_pattern of such a look
  at a range of 0.03 (range x layer_opacity). Samples: "flatten" now
  divides out a quadratic fit of the region's lightness (`solve6`), so
  only the light falloff goes and broad washes stay (the old 1/16 blur
  removed the pattern itself); "mean": "#RRGGBB" moves the sample's
  linear average to the surface's real color (a lit photo reads
  lighter); projection "repeat": n sets the scale to n repeats across the
  look's own pieces (their largest side). musetest also writes
  result_check.png (the check's 2x2 render, `muse_test_shot`).
  v50: the viewport defaults to linear (clamp) tone mapping again.
  v51 GPU MEMORY (crash on "Cabane 2.fbx": 117 objects, no materials).
  The FBX loader made one texture set per object when a polygon had no
  material: 117 sets ran the graphics card out of memory and wgpu
  aborted. Objects without a material now share one "DefaultMaterial"
  set, as in Painter (that file's objects all reuse the same plank UVs).
  And the texture sets keep within `SET_BUDGET` (3 GiB, about 96 bytes a
  texel each, `Document::sets_bytes`): an import lowers its resolution
  (`fitting_resolution`, with a message), a resolution change past it is
  refused, and `muse_prepare` raises sets to 2048 only when they fit.
  MF_MUSE_PLAN takes "a.json;b.json" (one plan per turn).
  v52 MUSE ANSWERS THAT ARE NOT VALID JSON ("expected , or ]"). The raw
  answer is written to muse/last_answer.txt. `parse_answer` reads it,
  and when it does not parse, `repair_json` fixes the usual slips
  (comments, missing and trailing commas, raw newlines in strings, a
  bracket of the wrong kind, an answer cut short); a wrong-kind bracket
  is read both ways (missing closer, one too many) and the reading whose
  looks keep the most layers wins. Still unreadable: Muse is asked once
  to send its answer again as valid JSON (one more API call, only then).
  MF_MUSE_PLAN files go through `parse_answer` too.
  v53 ONE RESULT PER PROMPT, TEXTURING STYLE, PHOTO LABELS.
  One result: while Muse works and runs its self check, the viewport
  shows a frozen copy of the view (`Muse.hold`, `muse_hold` /
  `muse_release`, drawn by view3d through `muse_hold_tex`) and the chat
  line waits (`Muse.pending`); the check's reply is merged into it
  ("Refined after looking at the result: ..."), and the check's apply
  takes no checkpoint (one undo step for the prompt). Failure, cancel
  and the check's early returns release the hold.
  Style: `enum Style {Auto, Product, Realistic, Stylized}`, a combo in
  the Muse bar; on Auto Muse picks it (plan "style", kept in
  `Convo.style`). The message states the style, the system prompt has a
  TEXTURING STYLE section, and `apply_style` enforces it (product: finish
  "none", age 0; stylized: finish "none"); the check is stricter for
  product (clean, exact to the reference).
  Photo labels: a print source takes "crop" [x0,y0,x1,y1] or "quad"
  (4 corners TL TR BR BL) on Rn, plus "arc" degrees to unroll a part
  wrapped around a cylinder (`cut_print`: homography `square_to_quad`,
  `bilinear`, output at real proportions in muse/prints/cut_*.png).
  `snap_quad` moves each side along its normal (reach 25% of the short
  side) to the strongest edge, weighted by how close the color 6 px
  inside it is to the quad's most common color (so a label edge wins
  over the bottle's own edge); "snap": false keeps the corners. Print
  "stretch": true fills its box (place() with aspect 0); a stretched
  print whose box covers over 85% of the surface takes the whole
  surface. Tested on ADP_mesh.fbx with the 405839 photo: two different
  first crops snap to the same label (about 198 x 278 px). A photo label
  is only as sharp as the photo (about 200 px wide there).
  v54 PRODUCTS WITH LABELS (user's box.fbx, a medicine carton, 8 photos:
  6 faces on a wooden table, 2 at an angle; v53 stretched portrait
  photos over landscape surfaces, kept the table, missed the nested end
  flaps and put faces on the wrong sides). New `src/app/photo.rs`:
  `find_object` (background = k-means of the photo's border band in Lab,
  the largest region unlike it, holes filled, the largest quad on its
  convex hull; `fill` near 1 for one flat face), `paper_of` (the most
  common light, nearly neutral color), `neutral_gains` / `balanced`
  (white balance in linear light), `turned`, `sides` (the six faces of
  a look's box: walls upright, top and bottom with -z up, as SURFACES),
  `solve_sides` (deals the free face photos to the sides their
  proportions fit, then the turns that fit, scored by how the colors
  change along each shared edge (strip means removed) plus a quarter of
  the mean difference; a move costs 1.5). Edge colors alone cannot tell
  a box from the same box turned 180 deg, and are weak when faces differ
  in color, so Muse gives "up" (the photo edge its letters' tops point
  to) and "toward" for the top and bottom; the solver decides the rest.
  muse.rs: print placement "side": "+x" (`side_decal`: a decal over the
  whole side of the look's frame, from just outside to 15% of its depth
  in, so every piece seen there gets it, flaps and folds included;
  "area" for part of it). `look_frame`: the world axes unless most of
  the surface faces other perpendicular ways. `muse_sides` (before a
  look's layers): cut each face (crop / quad, or `find_object` then
  `snap_quad` at 3% reach), one shared cast taken out (the largest group
  of photos whose papers agree; each of them by its own paper, the
  others by the group's), solve, save muse/prints/side_*.png as
  "cut_file". SIDES facts (`sides_facts`: size, proportions, best view,
  pieces seen, from a 96 x 96 depth raster) go to Muse with SURFACES;
  each photo's facts give its face corners, share, proportions and a
  tinted light color. `cut_quad` split out of `cut_print`; `snap_quad`
  takes its reach. Cylinder wrap: new ProjMode::Wrap (code 7; shader
  `wrap_uv`, coverage inside the height and arc on outward faces,
  0.2 < r < 0.5 of the box; arc in `scale` degrees, 360 below 5).
  "wrap": {facing, from, to, arc, axis} (`wrap_decal`: axis = least
  eigenvector of the normals' covariance (`least_axis`, Jacobi), circle
  by least squares, 64 slice radii (empty slices interpolated), the
  straight part by default; a partial piece (a label mesh) gives its own
  facing, height and arc; what is left out follows the print's
  proportions). `Muse.resolved`: what the app chose for a print (side:
  reference, "turned"; wrap: from, to, arc, facing) is written into the
  stored looks, so the check and later turns see it; "rotate" is only
  Muse's own. The check prompt knows up / turned / wrap and that the
  cast removal is not a defect. MF_MUSE_CHECK=1 runs the check in
  replays (its plan the next MF_MUSE_PLAN file); MF_MUSE_DUMP prints the
  sides and wrap measurements. Real run on box.fbx (2 calls): Muse used
  six side prints with the right proportions; its first "up" for the
  front was wrong and the check fixed it (verified in a replay).
- Mesh maps can be removed one by one in the texture set settings (X on
  Normal, ID, AO, Curvature, Thickness; not Position and World normal,
  which place the tri-planar projections): `TextureSetGpu::clear_map`
  writes the unbaked constant, `TextureSet::removed_maps` (saved in the
  project) shows "removed"; the next bake brings them all back.
- Mask Blur works in UV space: it cannot cross UV seams, so with a large
  radius each UV island blurs alone and the island borders show as
  straight-edged blocks.
- Canvas layout (`app/dock.rs`, since 2026-10-23, the approved design in
  `docs/design/material_forge_design_system.html`): the views fill the
  window (CentralPanel); rounded panels lie over them as egui Areas
  (`dock_panel`: elevation-1 shadow, radius 12, 10 px from the window
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
  black-to-white pill with a knob of the value's gray. Panels dock (see
  above) and resize by their inner edge (`left_w`, `right_w`,
  `bottom_h`); with the left panel hidden,
  a logo + menu button sits in the top left corner (`home_menu`, with
  "Show the layers panel"). View overlays (labels, badge, axes, modifier
  help, toasts) use `visible_part` (the view minus the panels); toasts sit
  under the view bar. Texture set settings is
  a docked panel (left column by default, under the layers; the gear in
  the layers panel toggles it). Panel visibility is `dock::Panels`
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
  are `procedural_smart_materials` (self test and bench only). The 12
  "Hand-painted ..." smart materials (`hand_painted_smart_materials`, no
  files; a painted animated film look, user request 2026-10-26; the
  earlier "Stylized" and "Painterly" sets were refused and removed, they
  read as plastic and camo; then one recipe recolored 12 times was
  refused as "all the same") each have their own build: gilded brass
  (hexagon engraving, glints), rusted plate (chipped teal paint, rust,
  streaks), chem copper (verdigris drips, glowing pockets), graffiti
  bricks (mortar, neon splatter), hextech crystal (Cells facets, glowing
  Cracks veins), shimmer glass (Marble swirls, Dots bubbles), enforcer
  steel (Cells dents, scratches), worn leather, coat cloth (weave),
  cobblestone (Cells tones, Cracks gaps, moss), weathered planks (Bricks
  with aspect 0.25 as boards, old paint), skin (blush, freckles). Shared:
  `painted_base` (the "Brush strokes" procedural mixing two colors),
  `painted_light` (soft cool shadow, warm light, strokes only nudge the
  border: a hard Levels cut made camo blobs), `crevices`, `edge_light`.
  Patterns: Brush strokes (case 28), Paint splatter (29: blobs with
  spiky borders, droplets, drips down -y) and Streaks (30: vertical runs)
  in procedural.wgsl; Bricks, Hexagons, Brush strokes and Splatter are
  `planar_kind`: in tri-planar projection `proc_tri` gives each face its
  own plane (dominant axis, ragged border, no blend). Colors are picked
  in sRGB and converted with `lin()` (fill values are linear).
  `--matshot <dir> procs` renders every procedural in one sheet.
  `--matshot <dir> sm:<name>` renders each such smart material's layers
  added one by one on a sphere, then on the box. The self
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
- Fill layer rows show their base color as a flat swatch (the
  eyedropper picks it); only a base color map (texture or procedural,
  `layers::fill_has_ball`) shows a material ball, rendered like the
  library thumbnails (`Previewer::new_layer`: studio light, the ball
  filling the frame, not graded like the viewport).
- Library: the grid / list button opens a menu: small, medium, large
  thumbnails (`Shelf::thumb_size`, min tile 52 / 66 / 104 px) or list.
  Thumbnails decode on up to (cores - 1, 2..8) worker threads, shrink on
  the worker, every finished one shows the same frame, at most two GPU
  renders per frame (`generate_thumbnails`).
- F frames the view under the mouse (in Split, the last one it was over,
  `last_view_2d`), not both.
- Mask fill effects place their UV projection like fill layers (per-axis
  scale and pivot, `m_uv` in sources.wgsl), so the 2D manipulator scales them.
- egui 0.29.1 is vendored (`vendor/egui`, `[patch.crates-io]`): sub-menus
  open to the left when the screen has no room on the right (menus of a
  panel docked right), see "Material Forge" comments in its menu.rs.
  File > Open recent is a regular flyout sub-menu again. The layers panel
  header has a padlock next to the menu button (locks the panels).

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

- v55 (2026-10-29) LIBRARY: materials Muse downloads (ambientCG, in
  `<library>/materials`) were listed as Starter assets. `Shelf::source_of`:
  a texture material under the library dir is Project when the open
  project uses it (its maps are resources, "<name> color.<ext>";
  `Shelf::project_mats`, rebuilt in `browse`), else Your assets; the
  Project category lists those materials too.
- v55 PACKSHOT STUDIO (`src/studio.rs`, `app/studio_panel.rs`, shader
  `pathtrace.wgsl` "studio" section): button next to Render (Icon::Studio)
  turns it on (path traced, Material display, perspective) and opens its
  flyout (flip_flyout 3, `flyout_at[2]`); again: the panel; again: off.
  `Document.studio` (saved in the project header, serde default) holds up
  to 8 lights (`StudioLight`: on, kind Softbox / Octabox / Gobo projector,
  sRGB color, power, azimuth (0 the camera's side, + its right),
  elevation, distance and sizes in object sizes (half the box diagonal),
  softbox grid angle, gobo pattern / beam / focus / scale / rotation), the
  background (shown exactly: linear, divided by the exposure), ground
  shadows + opacity, room bounce, environment light, follow camera, lens
  mm, format, output size and samples. Power: irradiance pi * power at the
  subject (softbox radiance P pi d^2 / A, gobo intensity P pi d^2), then
  physical inverse square. Shader: every light sampled at each bounce
  (shadow ray; `trace` now takes a far limit) with per-lobe MIS against
  BSDF rays meeting softboxes (`hit_lights`: fronts emit, rays pass
  through; projectors are never met). Gobo projector = thin-lens profile
  spot: a point on the lens (radius 0.08 d), the ray through p crosses the
  focus plane (d / (1 - 0.85 focus)), seen from the lens center that is the
  gate position; outside the gate circle nothing, flat field -25% at the
  edge; patterns `gobo_pattern` (window, blinds, leaves fbm, dots, bars,
  arch, grid). Ground (camera rays only, at the model's lowest y):
  `ground_shade` = background * mix(1, lit / open, opacity), lit with
  shadow rays and the gobo, open without either (beam pools never show,
  shadows and gobo patterns do; plus a bounce term blocked near the
  subject). Misses: the background from the camera and through
  refractions, else background * room + environment * its light. Lens: the
  frame's long side is 36 mm; `studio_camera` sets the view's fov so the
  format's frame (`frame_in`, 92% x 90% of the view) gets it (the old fov
  back when leaving); `studio_overlay` darkens outside the frame and marks
  the lights that are on. "Render and save the image": `StudioJob` (its
  own PathTracer + Viewport at the output size, about 2 M pixel samples a
  frame, the view's tracer waits), then OIDN, the view's post pass and a
  PNG. `--studioshot` renders one headless. Crawl state "packshot studio".

- v56 (2026-10-29) STUDIO, from the user's session (ADP bottle: tall
  strips at power 3.5 and env light to get a dull result): the studio is a
  docked panel (dock.rs panel 5 "studio", Tab::Studio, shown while
  `view.studio`; layout_v6.json, a v5 one is read with it added on top of
  the right column); the flyout is gone. The frame fits the part the
  panels leave visible (`studio_frame` = `visible_part`), and
  `OrbitCamera.shift` (serde skip, a clip-space translation in `proj`)
  puts the target at its center. Softbox power counts the box's whole
  area (`box_irradiance`, 32 x 32), so large near boxes give the power
  asked. LightKind::Flag (black card, kind 4): ends reflection rays
  (`hit_lights` -> LightHit.stop). `SETUPS` / `Studio::setup`: soft high
  key (default), glossy bottle (strips at +-105, front diffuser, top
  octabox, flags at +-40), packaging, dark, window, blinds, leaves;
  "Back to my lights" (`studio_prev`). Lock the camera (`lock_camera`,
  `shot`, restored each frame in `studio_camera`). Background default
  white. `--studioshot` MF_SETUP=<n>.
- v57 (2026-10-29) STUDIO: the frame also stays under the view bar
  (GAP + 46) and above the tool dock / options bar when they overlap it
  horizontally (`studio_frame`, `dock_rect` / `options_rect`).
- v58 (2026-10-29) AUTOSAVE (`app/autosave.rs`): Preferences > Autosave,
  kept in `autosave.json` next to the library (`Autosave::load / save`):
  on (default), every 1..60 minutes (default 10, counted from the first
  change since the last save), save into recovery copies (default;
  `<library>/../autosave/<name>_autosave.sprp` the newest, then `_2`, `_3`...,
  1..20 kept, default 3; the project stays modified) or the project file
  itself (clears modified; a project never saved gets copies). Waits for a
  stroke, bake, save or studio render to end; wakes the app when due. "Open
  the recovery folder". The old display settings slider is gone. Uitest
  `autosave_check` (MF_UITEST_ONLY=autosave); crawl state "preferences".
  Known: the full uitest fails at "no live color under the cursor while
  armed" (eyedropper check), already in v53, not looked into yet.
- v59 (2026-10-29) STUDIO, perfume and cosmetics only (user: "only
  packshot type lights"): gobo projectors removed (enum Gobo, the shader's
  gobo_pattern / noise, LightS.open); an old "Gobo" light reads as a
  Softbox (`kind_or_softbox`). LightKind: Strip, Softbox, Octabox, Scrim
  (diffusion panel), Card (white bounce: its radiance = color * power *
  E / pi, E from the emitting lights at its middle, `Emitter::irradiance`,
  12 x 12, no shadows), Flag. `LightKind::shape` gives a new kind its size
  and soft edges (`set_kind`; "Add a light..." picks the kind).
  StudioLight `feather` (soft edges) and `gradient` (top + / bottom -):
  `profile` in studio.rs and the shader (GpuLight.prof), counted in
  `box_irradiance` so the power holds. Studio: `falloff` + `pool`
  (background darker away from the product: `bg_seen(d)`, around the
  direction camera -> model center, radius atan(pool * 12 / lens mm)),
  `floor_reflection` + `reflection_fade` (glossy plexi floor: the camera
  ray at the ground adds ground_shade * (1 - k) and goes on reflected with
  throughput k, its next hit fading to the background by exp(-t / fade);
  guides from what it shows). `gpu()` now takes the eye.
  `take_lighting` ("Back to my lights"). SETUPS: perfume white two strips
  (default), backlit glass, white plexi, black and gold dark plexi,
  cosmetics jars top light, lipstick and metal, skincare high key.
- v60 (2026-10-30): studio edits are undo steps (history `Entry::Studio`,
  `tracked_studio` in track_edits: frames within 0.8 s merge, so a drag is
  one step; uitest MF_UITEST_ONLY=studioundo). Muse's ambientCG scans go to
  `<data>/muse/scans/<category>/<id>` (`ambientcg::scans_dir`), not the
  library; `move_scans_out_of_library` (at start) moves old ones there
  (material.json source ambientCG). The shelf lists scans only in the
  Project category of a project whose resources hold their maps
  (`library::scan_materials`, Shelf.scans). Path tracer: F0 from the
  specular level (finals' emissive alpha, 0.08 * level, like the viewport);
  the ray off the glossy floor skips `hit_lights` (no light panels in the
  plexi). Filter Curves (FilterKind code 17): `FilterDesc.curves` (RGB, R,
  G, B; up to 8 points), `curve_eval` / stack.wgsl `curve` (monotone
  cubic, flat past the ends), FilterU.curve; editor `curves_ui` in
  panels.rs; selftest `curves_check`. Studio camera angles
  (`Studio.angles`, CameraAngle: view, lens, format; View / Update /
  Remove, "Save this angle", "Render every angle..." into a folder through
  `studio_queue` and `render_studio_shot`). Lighting presets: "Save as
  preset" writes `<library>/lighting/<name>.json` (`studio::save_preset`,
  no angles); they list under "My presets" in the setup menu (Delete
  there) and load with `take_lighting` (keeps camera and angles).
- v61 (2026-11-03): the scan move runs first in `RetroApp::build`, failures
  to stderr.
- v62 (2026-11-03) STUDIO: entering it centers the model in the frame
  (`studio::fit_camera`: from the camera's yaw and pitch, the model's
  points (<= 20000) fill `FRAME_FILL` 0.7 of the frame, the picture of them
  centered; `studio_recenter` frames in `studio_camera`, skipped when the
  camera is locked); "Recenter the model" in Camera (a locked shot
  follows); `--studioshot` MF_FRAME uses it. Leaving the studio
  (`leave_studio`) turns path tracing off. LightKind::Spot "Hard light
  (sun)": a small far disc (kind 2), crisp shadows. SETUPS replaced (8),
  read from the user's Dior / Acqua di Parma references: pure white with
  side strips (ADP e-commerce, default), light grey with a crisp side
  shadow (Dior beauty), warm stone wall and window light (Dior Collection
  Privee), white backlit glass, black rim strips and dark plexi, hard sun
  long shadow (campaign), skincare top light, white plexi reflection;
  `setup` now sets `pool` too. Muse lighting (`app/muse_light.rs`, in the
  studio panel under the setups): photos (Add a photo... or dropped on the
  window while in the studio, `studio_drop`) and a request go to Muse
  Spark with SYSTEM (the studio's lighting model and how to read a photo's
  shadows and highlights) and `to_json` of the lighting now; the answer's
  "lighting" goes through `apply` (kinds by Debug name or display name,
  values clamped, at most 8 lights), then a 640 px 96 spp check render
  (`lighting_check.png` in muse/, through `render_studio_shot`) goes back
  once with CHECK and the corrected lighting is applied. "Back to my
  lights" restores the lighting before the request. MF_MUSE_LIGHT=<file>
  replays an answer; uitest `studio_muse_check` (MF_UITEST_ONLY=studiomuse).
- v63 (2026-11-03): the lighting request moved into Muse's texturing
  conversation (user: "should be in the conversation we already had");
  the studio panel only points there. `muse_light.rs` holds LIGHTING
  (appended to `instructions` in `run`: the studio's model and how to
  read a photo's light; answer key "lighting", no looks), `check_text`,
  `apply`, `to_json`, `jpeg_url`. `muse_send` adds "[PACKSHOT STUDIO now:
  <studio_facts> Its lighting: <json>]" to every message. `muse_answer`
  applies "lighting" (`muse_lighting`: studio_prev kept, the studio
  entered and recentered); a turn with only lighting renders a 640 px 96
  spp studio check (`Muse.light_check`, `render_studio_shot` into
  muse/lighting_check.png), `studio_finish` -> `muse_light_saved` sends it
  back as a check Request, cancelling -> `muse_light_cancelled`. Tests:
  MF_MUSE_PLAN turn;check + MF_MUSE_CHECK + MF_MUSE_OFFLINE (no ambientCG
  catalog) + `muse_test_key`. Crash log: main's panic hook appends to
  `<data>/crash.log` (the v62 "crash" left no trace; the old separate
  studio chat is gone). Presets 0 to 2 retuned on the user's photos with
  matching cameras (ADP square 100 mm pitch 9; Dior serum 3:2 pitch 12;
  Gris Dior 2:3 85 mm pitch 4): a soft shadow needs the key to dominate
  the ground's light (top lights weak, key a 1.4 softbox at distance 4.5,
  not a big near scrim, whose shadow vanishes).
- v64 (2026-11-04) MUSE LIGHTING, from the user's first real try (it baked
  for nothing, light far from the photo): a new conversation's
  `muse_prepare` (2048 px + mesh map bake, history cleared) is deferred to
  the first answer with looks (`Muse.prepare`). `studio::photo_light`
  measures a photo (background at the corners, the product's box from its
  upper part, ground shadow at its foot left / right: darkness and reach in
  product widths, a shadow along its sides, the brighter side); a message
  `about_lighting` sends "[Light measures by the app: Rn ...]" for its
  photos (`Muse.light_facts`), and the lighting check gives the photo's and
  the render's measures. `muse_light::rules()` = LIGHTING + the 8 SETUPS
  as JSON (tuned starting points) + measures and camera sections; the
  answer may hold "camera": {"pitch", "lens_mm"} (applied unless the
  camera is locked, then recentered). `to_json` rounds to 2 decimals.
  `--lightmeasure <img>...` prints the measures. Muse's v63 answer on the
  ADP photo: no shadow (measured: photo right 7%, 0.6 widths; preset 0:
  9%, 1.0; Muse: none), camera at pitch 0.
- v65 (2026-11-04) LIGHTING ONLY KEEPS THE MATERIALS. The user's v64 try:
  Muse's lighting was about preset 0 with the right camera, but it also
  sent looks (a cobalt lacquer body, a blue resin cap), so the bake and the
  texture check ran and the lighting check never did. `lighting_only(text)`
  (about the lighting, no texture / material / color / finish / label /
  logo / print words; "highlight" ignored) sets `Muse.light_only`: the
  message says "asks only for the packshot's lighting", and `muse_answer`
  drops "looks" / "parts" with the note "Your materials were kept". The
  studiomuse uitest replays a turn carrying looks and checks the stacks.
  Preset 0 retuned on region colors of the ADP photo (SP cmp/regions.py:
  cap, shoulder, body left / right means, label): the photo's left side
  is a broad saturated sheen to the edge, its right a dim saturated blue
  with no grey band. Now: key softbox el 22, sheen scrim 0.9 at az -100
  d 2.0 2.6x3.6, front fill 0.5 at az 5 (its reflection hides behind the
  label), back scrim 0.3 at 180 (shoulders), strip right 0.2 at az 135
  (edge line only), top strip 0.15, shadow 0.4. Rules gained where a
  light's reflection lands on a glossy cylinder (sine of half the
  azimuth). The check text now puts the measures after the instructions.
- v66 (2026-11-04) THE CLOSER LIGHTING STAYS. The user found v65's
  Muse result worse: its check pass (driven by the left / right ratio
  alone) dimmed the fill and strips; profile distance to the photo 0.23
  against 0.12 for preset 0. `studio::photo_profile` (product box, its
  color across the width at 3 heights x 16 steps, ground darkening 8 steps
  right / 4 left) and `profile_distance` (mean abs, ground x3);
  `photo_light` now ends with the brightness profile in words (8 steps,
  3 heights) for Muse. `Muse.light_photo` = the photo's profile;
  `muse_light_saved` stores `light_first` (studio, pitch, distance of the
  first render); after the check answer the corrected lighting is
  rendered to muse/lighting_verify.png (`light_verify`) and
  `muse_light_verified` keeps the closer one (note "its first lighting was
  kept" or "The correction is closer to your photo"). Preset 0 fitted by
  coordinate descent (SP cmp/fit.py, 120 renders) on the ADP photo:
  distance 0.121 to 0.064 (key 0.83 at az -60 el 12, sheen scrim 4.0
  wide, fill el 20, back 0.45, strip right az 147, top 0.23).
- v67 (2026-11-06) CLAUDE REPLACES MUSE (user: "muse was for the hackaton.
  But claude is better"; not pushed). `muse.rs` calls Anthropic's
  Messages API: POST api.anthropic.com/v1/messages, `x-api-key`,
  `anthropic-version: 2026-10-21`, beta `server-side-fallback-2026-07-01`;
  body (`request_body`) model `claude-opus-5-5`, max_tokens 64000,
  streamed, top-level `system`, thinking adaptive, `output_config.effort`
  "high" (`EFFORT`), `fallbacks: "default"` (a declined request reruns on
  the model Anthropic picks), top-level `cache_control` ephemeral (later
  turns and the self check reuse the cached prompt and images).
  `image_block` turns the internal data URLs into base64 image blocks
  (re-encoded as JPEG past 5 MB). `read_stream` reads the SSE: text
  deltas, a `fallback` block restarts the text, stop_reason `refusal` is
  an error, `max_tokens` with no text too; `ask` retries 429/500/529 twice
  (10 s, 20 s); read timeout 300 s between events. Key: ANTHROPIC_API_KEY
  or `<data dir>/claude_key.txt` (the old muse_key.txt is no longer read);
  the connect card opens platform.claude.com/settings/keys and checks a
  key with GET /v1/models (free). No image generation: `texture_prompt`
  is gone from the instructions and the self check; an old plan holding
  one gets a note and the layer is skipped. All user-facing "Muse" text
  and comments say Claude (tab, panel header, statuses, notes, folder
  names "Claude: ..."). `claude_api_check` (in MF_UITEST_ONLY=studiomuse)
  checks the body, image block and a canned stream with a fallback,
  offline; crawl state "claude panel" shows the connect card. Also in
  this build (demo work, 2026-11-05): `app/demo.rs` scripted showcase
  player (MF_DEMO=<script.json>, MF_DEMO_OUT=<mp4> via ffmpeg),
  `photo::Found.labels` (light low-chroma quads on the object, given to
  Claude as "paper labels: the quad to print from"), `--photofacts`,
  MF_MUSE_DELAY for replays. Never tested live: no Anthropic key here.
- v68 (2026-11-06) CLAUDE THROUGH THE CLAUDE APP (MCP), no API key (user:
  "can it work without API and only claude connected", chose MCP; not
  pushed). `app/mcp.rs`: `material_forge --mcp` is a stdio MCP server
  (JSON-RPC, protocol 2025-06-18 or the client's) the Claude app or
  Claude Code starts; it forwards tool calls to the OPEN window over
  127.0.0.1 (`<data dir>/mcp.json`: port, token, pid; written by
  `mcp::serve` on the first frame, not in tests unless MF_MCP_SERVE). It
  never starts the app (the Claude Desktop Store build would run it in
  its package: settings virtualized). Tools: texture (message, images =
  file paths, texture_sets, style, new_conversation, resend_instructions),
  answer, wait, look, status, open. A texture call runs `mcp_texture`
  (muse.rs), i.e. `muse_send` as if typed; `run()` sees
  `mcp::turn_active()` and calls `mcp::ask_app` instead of the API: the
  last message (instructions on the first) goes back as the tool result,
  split in parts (60k chars, 700 KB of base64 images, 4 images each;
  "call wait" for the next) and Claude's `answer` call resumes the
  worker. Self check and lighting check go the same way. `mcp_tick`
  (every ui_frame) ends the turn when nothing is busy, compositing or
  baking: DONE + panel notes + `muse_shot` render. Tool calls wait 45 s,
  then say "still on it, call wait"; the bridge sends progress every 8 s
  and hello every 15 s (`mcp::linked()` for 60 s: the panel opens without
  a key, chip "Claude app", composer replaced by a hint, images dropped
  there go with the next request). Connect card: "Add to the Claude app"
  (`claude_app_config`: %LOCALAPPDATA%\Claude-Data\claude_*\ for the Store
  build, else Packages\Claude_*\LocalCache\Roaming\Claude, else
  %APPDATA%\Claude; merges mcpServers.material-forge = exe --mcp, .bak
  first, refuses broken JSON) and "Add to Claude Code" (`claude mcp add -s
  user`). HEADLESS: both redirect / refuse (the crawl clicks every button:
  it once wrote the user's real configs, restored the same day). Tests:
  `mcp::self_check` in studiomuse; end to end `--mcpserve <mesh> <dir>`
  (headless app with the socket, stops on `<dir>/stop`) driven by SP
  `mcp_client.py <exe> <dir>/data <out>` through the real bridge, canned
  answers. Also: "Don't show shortcuts" is remembered (`ui_prefs.json`,
  `persist_show_tool_keys`, checked in MF_UITEST_ONLY=panels).
- v69 (2026-11-06) NO API KEY MODE (user: "can you get rid of the API key
  mode ?"). Gone: the Messages API calls (`request_body`, `read_stream`,
  `ask`, the model / version / effort / fallback constants), the key
  (ANTHROPIC_API_KEY, claude_key.txt, `Muse.key`, `Request.key`), the key
  part of the connect card, the Disconnect chip, the panel's message box,
  Send and EXAMPLES, `muse_test_key`, `claude_api_check`. `run()` answers
  only through `mcp::ask_app` during a Claude app turn, else "Claude
  answers in the Claude app or Claude Code: ask there."; MF_MUSE_PLAN
  replays still work (tests, `--musetest` is replay-only now). The panel
  (`muse_ready`: linked, or a conversation to show) shows the chat, a
  "Claude app" dot (green when linked) and an images box (attach for the
  next request). The connect card: "Add (again) to the Claude app" /
  "Claude Code", says when Material Forge is already a connector
  (`mcp::added`, reads the settings every 3 s) and when the entry starts
  another copy of the app. `image_block` stays: questions are built as
  message blocks and `ask_app` turns them into MCP content.
- v70 (2026-11-08) SMALL CLAUDE CARD + TOOLTIP SHADOW (user: "simplify a
  lot the claude panel so it's top right in the default layout but a small
  layout that eventualy disappear when connected and reappear when not
  connected"; then "when hovering a button ... a weird curve under the
  selection"). The Claude panel is now only a card: header "Claude / Not
  connected", one line (or the last add result), "Add to Claude app"
  (accent) and "Add to Claude Code" side by side. Fixed height `MUSE_H`
  (150) in its column (`place()`: the others share the rest; no split
  grip next to it), top right above properties by default. Hidden while
  `mcp::linked()` (a hello in the last 60 s, or a turn), back when the
  link is gone (`canvas_ui`: `shown[MUSE] &= !linked`). Gone: the chat
  view, the "Claude app" dot, style combo, Pieces chip, texture set chips,
  the images box / + Image, dropping images on the window for Claude
  (`muse_drop`, drops open files again), Cancel (`muse_cancel`),
  `muse_ready`, `muse_body`, `bubble`, `chip`, `ref_row`, `Style::hint`,
  `Ref.thumb`, `ChatLine.refs`. `mcp_tick` now polls the work every frame
  (`muse_poll` left the panel); `mcp_texture` no longer opens the panel.
  Images reach Claude only through the texture tool's `images` paths.
  The streak: `popup_shadow` was `shadow(2)` (blur 40) and a tooltip is
  ~28 px tall, so epaint's shadow mesh folded into a dark diagonal under
  every tooltip. New `style::shadow(3)` (offset 6, blur 16) for popups,
  tooltips and the panel drag tag; keep a shadow's blur under the height
  of what it shadows. New debug mode: `MF_CRAWL_HOVER=<dir>` with
  `--uicrawl` hovers each widget of the scenario (tooltips on) and saves
  crops. The panels uitest checks the card hides while linked
  (`mcp::set_linked`).
- v71 (2026-11-08) (user: "when going inside the windows - claude : I'm
  unable to set it visible"). Window > Panels > Claude now shows the card
  even while Claude is linked (`Muse.peek`, set by `focus_tab`; the card
  then says "Connected"). Unticking it or its close button while linked
  only drops `peek` (the card comes back by itself once the link is gone);
  while not linked they close it for good (`dock.muse = false`, Window
  menu brings it back). A new link (`was_linked` rising edge in
  `canvas_ui`) drops `peek`, so it hides again on each connection.
  `tab_open(Muse)` is what is really shown, so the menu tick matches.

## Tested meshes

Built-in box + sphere (selftest), `LULJETA_SUBSTANCE_EXPORT.fbx` (bounds match
Blender), `Egypt obelisk.fbx`, `WATCH DESIGN.obj` (243k tris).
