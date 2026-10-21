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
window / dialogs say Material Forge, logo in `src/logo.rs` (procedural "Mf" tile,
`--write-logo assets` regenerates `assets/material_forge.ico|_logo.png`, embedded
by `build.rs` via winresource). Projects keep the `.sprp` extension and the user
library stays in %APPDATA%\SP_RETRO for compatibility.

## Build and run

```
cargo build --release
target\release\material_forge.exe                 # empty session
target\release\material_forge.exe model.fbx       # new project from a mesh (2048 px)
target\release\material_forge.exe project.sprp    # open a project
target\release\material_forge.exe --selftest <out_dir> [mesh]
target\release\material_forge.exe --uitest <out_dir>
target\release\material_forge.exe --bench
target\release\material_forge.exe --uibench <mesh>   # per-section frame timings, headless (SP_BENCH_SIZE=WxH)
target\release\material_forge.exe --uicrawl <out_dir> # clicks every widget: report + screenshots (MF_UITEST_SIZE, MF_CRAWL_ONLY=<state>)
```

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
  mesh reimport keeping layers by material name.
- Texture sets, per-set resolution (resampling), alpha modes, emissive.
- Channels: base color, roughness, metallic, height, emissive, opacity, normal.
- Layers: paint, fill (uniform / texture / 20 procedurals, UV, tri-planar,
  planar, object-space projection), filters (HSL, levels, blur, sharpen,
  gradient map...), folders, 28 blend modes per channel, opacity per channel,
  thumbnails, drag and drop, duplicate, group.
- Masks: effect stacks (paint, fill, 9 generators, levels, blur, invert,
  threshold, ID color selection), white / black masks, smart masks.
- Tools: paint, eraser, clone (Ctrl + click source), smudge, polygon fill
  (triangle / mesh part / UV island / set), color picker, geometry mask
  (hide / isolate), stencil projection, material painting (any shelf
  material or the selected fill, evaluated per texel).
- Generators can react to height painted below them (anchor-like workflow:
  edge wear around painted scratches and bolts).
- Brush: alphas (12 procedural + images), size / flow / hardness / spacing,
  angle, follow path, jitters, lazy mouse, symmetry X/Y/Z, wrap and camera
  alignment, line tool (Shift + click), pen pressure (size / flow; untested
  without a tablet), brush presets.
- Assets panel (left, like SP): all libraries / project / per category,
  search, grid or list, material-ball thumbnails, user library in
  %APPDATA%\SP_RETRO\library. Drag and drop: assets onto layer rows (insert
  above, smart masks apply to that layer), onto the 3D view (texture set
  under the cursor), onto Properties slots (fill channels, mask fill, alpha,
  stencil, brush material); image files dropped on the window become project
  resources; layer rows drag to reorder.
- SP-style window: menus File / Edit / Mode / Window / Viewport / Help, icon
  tool strip, Properties panel with resource slots and channel chips,
  Texture Set List + Layers / Texture Set Settings tabs (mesh map
  thumbnails), context toolbar above the views, separate 3D and 2D display
  menus, right icon dock (display settings, shadows, history, log, panels).
- Bakers: AO, curvature, normal from high poly (optional match by name,
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
src/app/layout.rs        tool strip, icon dock, context toolbar, view display menus, status bar
src/app/icons.rs         painter-drawn icons, icon button, panel header
src/app/widgets.rs       Painter widgets: label/value slider, gradient slider, color bar, chips, slots, cards, icon tabs, picker popup
src/app/view3d.rs        3D / 2D views, navigation, strokes, clone, smudge, fill, hide, asset drops
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

- Docking (`app/dock.rs`, egui_dock 0.14): Viewport, Assets, Properties,
  Texture Set List, Layers, Texture Set Settings are dock tabs (drag, split,
  float, close; Window > Views / Reset UI). Each tab body is laid out in a
  child UI bounded to the visible rect; never let a row overflow the panel
  (egui grows max_rect; use `widgets::label_width`, truncating combos,
  `horizontal_wrapped`).
- Content effects (`LayerDesc.effects`, `Target::ContentEffect`): filters on
  a layer's content; the layer + effects compile to an implicit folder plan
  with the layer mask on the folder. Mask effects are listed under the row
  only when the mask is selected, content effects when the content is.
- Optional channels (`NEXT`, `EXT_NAMES`): Specular level and Transmission,
  enabled per texture set (Texture Set Settings > Channels +), fill only
  (uniform / procedural), stored in the spare `.w` of the accumulators
  (mrh.w, emi.w) and finals (emissive.w = specular level, normal.w =
  transmission); export sources SpecularLevel / Transmission; display modes.
- Color fields: `widgets::color_bar` opens the Painter-style picker window
  (SV square + hue, hex, HSV / RGB, swatches); the eyedropper arms
  `eyedropper_id()` and the 3D view writes the sampled base color back.
- Painting on fill / filter / folder content is refused with a viewport
  toast (like the real app); select the mask or a paint layer.
- Environment: Painter's own `Panorama.exr` is loaded at startup from the
  installed Painter (read in place, not copied); its other environments are
  listed in Assets. Tone mapping defaults to linear, bloom / vignette off.
- Assets: library menu (All libraries / Project / Starter assets / Your
  assets), type icons (Materials default), Filters as assets, procedurals
  under Textures with flat thumbnails, smart materials on a rounded box.
- Edit tracking hashes the active set each frame (`fingerprint_hash`) and
  snapshots only on change; `History::version` tells explicit history ops
  (commands, undo, redo) apart from panel edits.

Reference: `docs/SP_UX_SPEC.md` (behavior spec built from Adobe's public
docs: shortcuts, panels, layer stack, dialogs) and
`docs/reference_captures/` (captures of the real application on this PC;
Painter was driven black-box: launched with `--enable-remote-scripting`,
`sp.sh python '...'` creates a project from a mesh, `spclick.ps1` posts
clicks to its window without moving the cursor, popups can be captured but
not clicked). Measured palette lives in `style.rs`; fonts are Segoe UI
regular + bold loaded from C:\Windows\Fonts (`style::bold`).

- Key map follows the real app: 1 paint, 2 eraser, 3 projection, 4 polygon
  fill, 5 smudge, 6 clone, P temporary picker, V clone source, D lazy mouse,
  C / B / M views, F1 to F4 layouts (F4 swaps), F5 / F6 camera, F8 / F9 / F10
  modes, Tab hides docks (intercepted in `filter_input`, eframe's
  raw_input_hook, so egui does not move focus), Alt + Q isolate, Ctrl + C / V
  / D / G layers, Alt + arrows reorder. Gestures: Alt + mouse camera, Ctrl +
  LMB flow / angle, Ctrl + RMB size / hardness, Shift + RMB environment,
  S + LMB / MMB / RMB stencil, plain RMB click quick properties.
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
  write the user's `%APPDATA%\SP_RETRO\layout.json`.
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

## Performance (RTX 4060)

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
