# Substance 3D Painter UX spec (clean-room reference for a Rust/egui clone)

Target behavior: Substance 3D Painter 9.x (2023) and 10.x (2024), with notes where 11.x/12.x (2025) changed things.
Everything below comes from Adobe's public documentation (the official markdown source of the Experience League / helpx docs, repo `AdobeDocs/substance-3d-painter.en`), the screenshots shipped with those docs, and the release notes. Nothing was copied from the application itself.

Conventions used in this file:

* "LMB / MMB / RMB" = left / middle / right mouse button. "Ctrl" = Cmd on macOS unless noted.
* "(doc)" = stated in the official docs. "(screenshot)" = read from an official doc screenshot. "(unverified)" = from memory or inference, not confirmed by a source; implement it but treat it as adjustable.
* Version tags: [v9], [v10], [v11], [v12] mark the version where a behavior appeared.

---

## 0. Big-picture UX principles (what makes it "feel" like Painter)

1. Everything is a dock. Panels are Qt dock widgets with an uppercase title bar ("LAYERS", "PROPERTIES - PAINT", "TEXTURE SET LIST"), a float/undock button and a close (X) button at the right of the title. They can be tabbed, docked left/right/bottom, floated, or hidden. (doc, screenshot)
2. The viewport is king. Tools act on the viewport; the contextual toolbar above it changes per tool; RMB in the viewport (no modifier) pops the Properties panel at the cursor as a quick menu. (doc)
3. Modifier-key gestures do most of the work: Alt+mouse = camera, Ctrl+mouse = brush parameters, Shift+RMB = environment rotation, S+mouse = stencil, Shift+LMB = straight line. (doc)
4. Property editing is inline: sliders are always visible with a numeric field; resources are "slot" buttons that open a mini-shelf (resource picker popup) with a search field. (doc, screenshot)
5. The layer stack is dense: two-line rows (blend mode on line 1, opacity on line 2) at the right, thumbnails at the left, effects shown as indented child rows directly under the selected layer. (screenshot)
6. Dark neutral grays, a single cyan-blue accent for selection/focus, an orange accent for "has effects" indicators. (screenshot pixel samples)
7. Non-destructive and per-channel: the layer stack header channel dropdown decides which channel's blend mode / opacity / thumbnail the rows show. (doc)

---

## 1. Window layout (Painting mode)

### 1.1 Default arrangement (unverified for exact default, confirmed pieces marked)

```
+---------------------------------------------------------------------------------------------+
| Menu bar: File  Edit  Mode  Window  Viewport  Python  JavaScript  Help         (doc, scr.)  |
+--+------------------+---------------------------------------------------+------------------+--+
|T | ASSETS (dock)    | Contextual toolbar (tool params | viewport ctrls) | TEXTURE SET LIST |D |
|o | breadcrumb       +---------------------------------------------------+ LAYERS           |o |
|o | search           |                         |                         | (tabbed/stacked) |c |
|l | type filter icons|        3D view          |        2D view          +------------------+k |
|s | thumbnail grid   |   (display mode dropdown, camera dropdown top-R) | PROPERTIES - xxx |s |
|  |                  |   axis gizmo bottom-R   |   U/V axis bottom-R     | TEXTURE SET      |  |
|  | bottom bar       |   progress bar at bottom of viewport              | SETTINGS (tab)   |  |
+--+------------------+---------------------------------------------------+------------------+--+
| Status bar (right side: "Cache Disk Usage: 1% | Version: x.y.z")                (screenshot)|
+---------------------------------------------------------------------------------------------+
```

* Tools toolbar: vertical, far left; can only dock vertically left or right. (doc)
* Assets: docked vertically on the left on first launch; can be undocked and placed horizontally. (doc)
* Docks toolbar: vertical strip at the far right with one icon per dock (Texture Set List, Layers, Properties, Texture Set Settings, Display settings, Shader settings, History, Log, Substance 3D Assets [v10], Send To/Export [v11]). Clicking an icon shows that dock floating next to the button; clicking again hides it; if you drag the dock away it becomes a regular floating window that can be docked. Can only dock vertically left/right. (doc, screenshot)
* Contextual toolbar: cannot be moved, always at the top of the viewports. Left part changes per tool/context, right part is fixed viewport display controls. (doc)
* Plugins toolbar: lists enabled scripting plugins. (doc)
* Window > Views lists: Assets, Display Settings, History, Layers, Log, Properties - <context>, Python Console, Renderer Settings, Shader settings, Texture Set List, Texture Set Settings (+ QML console). Window > Toolbars: Docks, Plugins, Tools. Window > Hide UI, Reset UI. (doc, screenshot)
* Tab key: Hide UI (hide all docks, maximize viewports). (doc) A Linux screenshot shows Window > Hide UI as Ctrl+Shift+P (unverified per platform).

### 1.2 Modes

| Mode | Key | Notes |
|---|---|---|
| Bake mesh maps (Baking mode) | F8 | Separate panel layout (see 9.3). Also "croissant" button at top right of viewport, Mode menu. (doc) |
| Painting | F9 | Default. (doc) |
| Rendering (Iray) | F10 | Camera icon button in the viewport toolbar, tooltip "Rendering (Iray)". (doc, screenshot) |

---

## 2. Visual design

### 2.1 Palette (sampled from official doc screenshots; approximate)

| Token | Hex | Where |
|---|---|---|
| `bg.chrome` | `#333333` | Menu bar, panel bodies, toolbars around the viewport, status bar, dock strip |
| `bg.panel_title` | `#2B2B2B` | Dock title bar strip ("LAYERS" etc.) |
| `bg.panel_inner` | `#262626` / `#252525` | Layer rows, list areas, scroll content inside some panels |
| `bg.field` | `#2D2D2D` | Combo boxes / text fields (slightly lighter than inner bg) |
| `bg.viewport` | `#1C1C1C` to `#242424` | Viewport background (actually a user-set 2-color gradient, see prefs) |
| `bg.slot_button` | `#4D4D4D` | Resource slot buttons in Properties (older/flat style) |
| `bg.row_separator` | `#141414` / `#191919` | 1px dark lines between layer rows |
| `accent` | `#17A0FF` to `#1AB1FF` | Selected layer row 1px border, selected item text, active tab underline, enabled channel-button outline, focused field border, dock-toolbar active icon |
| `accent.alt` | `#38AEFF` | Texture Set List selected-row border |
| `accent.effects` | `#FF8922` | Thin bar under a thumbnail when that stack (content or mask) has effects (doc text says "red"; screenshots show orange) |
| `bar.no_effects` | `#4A4A4A` (unverified) | Same bar when no effects (doc: "Grey equals no effects") |
| `text.primary` | `#D0D0D0` to `#E0E0E0` (unverified) | Labels |
| `text.secondary` | `#9D9F9E` | Menu text, inactive tabs |
| `text.disabled` | `#6A6A6A` (unverified) | Grayed menu items/fields |
| `text.resource_name` | tan/yellow `#E6C07B`-ish (screenshot) | Resource name subtitle inside a loaded slot ("Shape") |
| `slider.track` | `#565656` / `#707070` | Unfilled track; filled part lighter `#B8B8B8`-ish |
| `slider.handle` | `#C8C8C8` circle | Round knob; focused knob shows ring |
| `button.primary` | Spectrum-like blue (`#1473E6`-ish, unverified) | Pill buttons such as "Bake selected textures" [v10+] |
| `button.secondary` | dark fill + 1-2px light outline, pill shape | "Return to painting mode", "Cancel" [v10+] |
| `error` | red `#E34850`-ish | Failed bake items, error text in log |

### 2.2 Typography and density

* UI font: sans serif (Source Sans / system-like), base size about 12 px at 100% scaling (unverified). Dock titles are UPPERCASE, bold, about 12 px. Properties section headers are UPPERCASE bold ("BRUSH", "ALPHA", "STENCIL", "MATERIAL", "FILL", "GENERAL PROPERTIES", "CHANNELS", "MESH MAPS"). Sub-groups are Title Case bold with a chevron ("> Attributes", "v UV transformations"). (screenshot)
* Dense: layer rows about 46 px tall (two text lines on the right + 32 px thumbnails), effect child rows about 26 px. Properties rows about 24 px for combos, slider rows about 36 px (label line + track line). (screenshot, approximate)
* Icons: monochrome light-gray line icons, about 16 to 20 px; active toggles get the accent color or a lighter background.

### 2.3 Widget styles

* **Slider** (doc "Sliders" page + screenshots):
  * Layout: label at top-left, numeric value at top-right (right-aligned, small underline-style text field), thin horizontal track below spanning full width, round handle.
  * Click anywhere on the track: value jumps to cursor. Drag: change value. Shift+drag: precision drag (more mouse distance per unit). After clicking (focus), Up/Down arrow keys step the value. Click the numeric field above the slider to type a value (Enter commits). (doc)
  * Soft range: dragging/typing can go past the displayed range for some params (gif "slider-soft-range") (unverified exact rule).
  * Some sliders have a trailing icon button at the right: pen-pressure toggle (Size, Flow), reset (circle arrow), or "edit/linked" pencil in older UI. (screenshot)
  * Grayscale value sliders show a black-to-white gradient track. (screenshot)
* **Dropdown (combo)**: dark field `#2D2D2D`, 1px border, chevron "v" at right; when focused, Up/Down arrows and mouse wheel change the value directly (doc, for blend mode dropdowns). Menus use the same dark bg with a lighter hover row.
* **Collapsible section**: "v" / ">" chevron left of the title; [v11.1+] each group also has a reset icon at the right and a settings menu icon on the section header. (screenshot)
* **Checkbox**: small dark square with a light check mark; label to the right. (screenshot)
* **Tabs**: flat, uppercase labels; active tab has a 2px accent underline and slightly lighter bg (e.g. Export window "SETTINGS / OUTPUT TEMPLATES / LIST OF EXPORTS", Log "GENERAL / ASSETS", Baking log tabs). (screenshot)
* **Section-jump icon tabs**: the top of Properties and Texture Set Settings shows a row of square icon tabs (one per section, e.g. Fill icon, Material icon); active one has accent underline; clicking scrolls/filters to that section (unverified: scroll vs filter). (screenshot)
* **Buttons**: [v10+] pill-shaped (fully rounded) buttons in dialogs (Cancel outlined, primary filled). Older dialogs: flat rectangular gray buttons. (screenshot)
* **Tooltips**: dark box with light text, appears on hover; tool buttons show "Name" and the shortcut on the right in flyout menus ("Eraser 2", "Physical eraser Ctrl+2"). Viewport toolbar buttons show tooltips like "Rendering (Iray)", dock buttons show the dock name ("Shader settings"). (screenshot)
* **Info icons**: [v12 New project dialog] each row ends with a circled "i" icon that shows a tooltip. (screenshot)

---

## 3. Keyboard and mouse shortcuts

All editable shortcuts are in Edit > Settings > Shortcuts. Right-click a shortcut entry to reset it to default. (doc)
Some non-editable shortcuts only work while the mouse is over a specific panel (copy/paste layers requires the mouse over the layer stack). (doc)

### 3.1 Editable shortcuts (Windows; macOS in parentheses when different) (doc)

**Application / project**

| Action | Key |
|---|---|
| Hide UI | Tab |
| Undo | Ctrl+Z |
| Redo | Ctrl+Y (Cmd+Y; Linux menu shows Ctrl+Shift+Z) |
| Baking mode | F8 |
| Painting mode | F9 |
| Rendering (Iray) | F10 |
| Open project | Ctrl+O |
| Close project | Ctrl+F4 (Cmd+W) |
| Save project | Ctrl+S |
| Save as | (none) |
| New project | Ctrl+N |
| Quit | Alt+F4 (Cmd+Q) |
| Bake Mesh Maps (open baking) | Ctrl+Shift+B |
| Export textures | Ctrl+Shift+E |
| Pause engine computation (toggle) | Shift+Escape |

**Manipulator (fill projections)**

| Action | Key |
|---|---|
| Show/Hide manipulator | Q |
| Increase / decrease manipulator size | + / - |
| Cycle manipulation space (Object/World) | T |
| Toggle warp edition mode (Warp transform / Edit vertices) | Shift+V |
| Translate / Rotate / Scale manipulator | W / E / R |
| Surface (snap to mesh) manipulator | Shift+W |
| Symmetry toggle | L |

**Tools**

| Action | Key |
|---|---|
| Paint tool | 1 |
| Paint tool + particles (Physical paint) | Ctrl+1 |
| Eraser | 2 |
| Eraser + particles | Ctrl+2 |
| Projection | 3 |
| Projection + particles | Ctrl+3 |
| Polygon Fill | 4 |
| Smudge | 5 |
| Clone (relative source) | 6 |
| Clone (absolute source) | Ctrl+6 |
| Paint along path tool | unassigned by default [v9] |
| Increase / decrease tool size | ] / [ |
| Invert grayscale (when painting a mask; swaps black/white) | X |
| Pick stroke material (material picker, temporary tool) | P |
| Lazy mouse toggle | D |
| Hide/ignore excluded geometry (geometry mask) | Alt+H |
| Ignore stencil (hold, temporary) | N |
| Resume previous stroke | A |
| Toggle animation of particles | Space |

**View**

| Action | Key |
|---|---|
| Perspective camera | F5 |
| Orthographic camera | F6 |
| Display next channel (solo) | C |
| Display previous channel | Shift+C |
| Display material (lit) | M |
| Display next mesh map (solo) | B |
| Display previous mesh map | Shift+B |
| Center whole mesh (frame) | F ([v11] focuses the path when editing a path) |
| Toggle quick mask edition | U |
| Clear/disable quick mask | Y (Viewport menu labels Y as "Enable quick mask") |
| Invert quick mask | I |
| Viewport layout 3D/2D | F1 |
| 3D only | F2 |
| 2D only | F3 |
| Swap 2D/3D | F4 |
| Texture Set isolate | Alt+Q |

**Mouse gestures (editable)**

| Action | Gesture |
|---|---|
| Use tool / paint | LMB |
| Straight line from last stroke point | Shift+LMB (dotted guide line is shown while Shift is held) |
| Straight line snapped every 5 degrees | Ctrl+Shift+LMB |
| Camera orbit (around cursor position) | Alt+LMB |
| Camera orbit snapped to 90 degrees / orthogonal | Alt+Shift+LMB (mac Shift+Cmd+LMB) |
| Camera pan | Alt+MMB (mac Option+Cmd+LMB); alternative Ctrl+Alt+LMB |
| Camera zoom/dolly | Alt+RMB (mac: MMB). Mouse wheel also zooms (Wheel speed pref) |
| Stencil rotate | S+LMB |
| Stencil snap rotate (90 degrees) | Shift+S+LMB |
| Stencil translate | S+MMB |
| Stencil zoom (scale) | S+RMB |
| Brush size (horizontal) / hardness (vertical) | Ctrl+RMB drag |
| Brush flow (horizontal) / rotation angle (vertical) | Ctrl+LMB drag |
| Rotate environment (horizontal) | Shift+RMB drag |
| Auto-focus (depth of field distance) | Ctrl+MMB click on mesh |
| Texture Set selection (pick the texture set under cursor) | Ctrl+Alt+RMB |
| Viewport quick menu (Properties popup at cursor) | RMB |
| Set Clone tool source | V (press with cursor over mesh; doc also lists V+LMB) |

Notes:
* Hardness via Ctrl+RMB only works if the alpha substance exposes a parameter with identifier "hardness". (doc)
* Stencil transform reset: press S, then click the "Reset" button that appears at the top right of the viewport. (doc)

### 3.2 Non-editable shortcuts (doc)

| Action | Key | Context |
|---|---|---|
| Copy layer(s) | Ctrl+C | mouse over layer stack |
| Cut layer(s) | Ctrl+X | layer stack |
| Paste layer(s) | Ctrl+V | layer stack |
| Delete layer(s) | Delete | layer stack |
| Duplicate layer(s) | Ctrl+D | layer stack |
| Group layer(s) into folder | Ctrl+G | layer stack |
| Copy layer content (paint + effects) | Ctrl+Shift+C (mac Option+Cmd+C) | layer stack |
| Paste layer content | Ctrl+Shift+V (mac Option+Cmd+V) | layer stack. CONFLICT: the layer context menu screenshot shows "Paste layer as instance Ctrl+Shift+V", and the instancing doc says Ctrl+Shift+V pastes as instance. Implement Ctrl+Shift+V = Paste as instance (menu wins, unverified) |
| Instantiate across texture sets... | Ctrl+Shift+D | context menu (screenshot) |
| Flatten group | Ctrl+M | [v12] (doc) |
| Display mask in viewport | Alt+LMB on mask thumbnail | layer stack |
| Disable/enable mask | Shift+LMB on mask thumbnail | layer stack |
| Copy / paste geometry mask | Ctrl+Alt+C / Ctrl+Alt+V | geometry mask icon menu (screenshot) |
| Drop material into a selection only | Ctrl while drag-dropping a material into the viewport | viewport |
| Manipulator snapping (3D) / keep ratio (2D) | Shift while dragging manipulator | viewport |
| Manipulator mirror around pivot (2D) | Ctrl while dragging | viewport |
| Manipulator precision mode | Ctrl while dragging translate/rotate/scale | [v10] |
| Move effect up / down | Alt+Up / Alt+Down | effect selected (doc, screenshot) |
| Move effect to top / bottom | Alt+Shift+Up / Alt+Shift+Down | (screenshot) |
| Scroll any panel by dragging | Ctrl+Alt+LMB drag inside the panel | any panel (doc, Assets page) |
| Change blend mode | Up/Down arrows or mouse wheel while the blend dropdown has focus | (doc) |
| Slider step | Up/Down arrows after clicking a slider | (doc) |
| Path: exit path edition | Escape or Enter | [v9] |
| Path: select all points | Ctrl+A | [v9] |
| Path: delete path | Backspace | [v11] |
| Re-enable all material channels | Alt+click the Base Color channel button | [v11] |

### 3.3 Escape behavior (partly unverified)

* Closes the open popup/menu/mini-shelf/color picker (standard Qt behavior, unverified for color picker which is "semi-persistent").
* Closes modal dialogs as Cancel (unverified).
* Exits path editing (doc). Shift+Escape toggles engine pause (doc).
* During a stroke it does not cancel the stroke (unverified).

---

## 4. Viewport

### 4.1 Structure (doc)

* Four parts: contextual toolbar (top), 3D view, 2D view (UVs of the selected Texture Set), progress bar (gray/green bar at the bottom of the viewport while the engine computes).
* Default layout: 3D view left, 2D view right (F1). Layout dropdown in the contextual toolbar right side: "3D/2D F2..." items with icons: 3D/2D (F1), 3D only (F2), 2D only (F3), separator, Swap 3D/2D (F4). (screenshot)

### 4.2 Navigation (doc)

| Movement | 3D view | 2D view |
|---|---|---|
| Orbit / rotate | Alt+LMB, orbits around the cursor position | Alt+LMB rotates the UV space around the cursor |
| Snap rotate | Alt+Shift+LMB snaps to orthogonal angles | same |
| Pan | Alt+MMB (or Ctrl+Alt+LMB) | same |
| Zoom | Alt+RMB drag, mouse wheel | same |
| Frame | F (whole mesh) | F (unverified for 2D) |

* Camera rotation mode dropdown: Free rotation (X, Y, Z) or Constrained rotation (default, no roll). (doc)
* Perspective mode dropdown: Perspective (default) / Orthographic. F5/F6. (doc)
* Preferences: Rotation speed multiplier, Zoom speed (+ inverse direction), Wheel speed (+ inverse direction). (doc)
* Moving the camera while an imported camera is active switches back to "Default camera". (doc)
* 2D view: only UVs in [0,1] are paintable in regular projects (outside shown but not interactive; light-gray background behind the 0-1 tile). UDIM projects show a tile grid with UDIM numbers. (doc)

### 4.3 Viewport overlays and on-screen elements

* Top right (both views): Display mode dropdown ("Material" by default). Dropdown contents: "Material", italic header "Single channel" followed by each Texture Set channel (Base Color, Height, Roughness, Metallic, Normal, "Normal+Height+Mesh", ...), "Mask" (when a mask is selected), italic header "Mesh maps" followed by each baked map. (screenshot) 3D view also has a camera dropdown (only when imported cameras exist, e.g. "Default camera", modified cameras shown in italics with a trailing "*"). (doc, screenshot)
* 2D view: "UV Tile information" toggle next to the display mode (UDIM projects only). (doc)
* Bottom right: axis gizmo (3D: XYZ, Y up; 2D: U/V). (doc)
* Bottom left: keyboard helper (when "Show keyboard helper" pref is on, pressing Ctrl/Shift/Alt shows the available modifier actions) and grid unit when the grid is shown. (doc)
* Top left: color profile name (when a LUT/color profile is loaded). [Baking mode] Baking visualization settings panel at top left. (doc)
* Brush cursor: dotted/dashed white circle at brush size, with a faint preview of the alpha/material inside. (screenshot) Lazy mouse on: an extra gray circle (the lazy radius) around the cursor, stroke trails behind. (doc) Symmetry: a second mirrored cursor (Show Cursor), hidden while painting if "Hide While Painting". (doc)
* Straight line: hold Shift to show a dotted line from the last stroke end to the cursor. (doc)
* Stencil: the stencil image is drawn over the viewport at "Stencil display opacity", optionally hidden while painting (Display settings > Tool display). Press S to manipulate; a "Reset" button appears top right. (doc)
* Progress bar: thin gray/green bar at the bottom edge of the viewport while computing. (doc)
* Path tool [v9]: path lines, vertex handles, a "Paths" list panel inside the viewport (top), a dotted preview segment from the last point to the cursor [v11]. (doc)

### 4.4 Contextual toolbar right side (fixed viewport controls) (doc, screenshot)

From left to right (exact order unverified): layout mode (3D/2D) dropdown, perspective/ortho dropdown (cube icon), camera rotation mode dropdown (camera icon), Rendering (Iray) button (camera icon), [v10+] Baking mode button ("croissant" icon), Painting mode button (brush icon). Each icon with a tiny chevron opens a small menu with icons and shortcuts.

---

## 5. Toolbars

### 5.1 Tools toolbar (left, vertical) (doc, screenshot)

Order top to bottom (from the tool list doc and "Path is the third icon from the top"):

1. Paint (1). Variant flyout: Paint / Physical paint (Ctrl+1).
2. Eraser (2). Flyout: Eraser / Physical eraser (Ctrl+2).
3. Path [v9]. Flyout: Paint along path / Erase along path / Smudge along path, [v11] Filled path, [v11.1] Ribbon.
4. Projection (3). Flyout: Projection / Physical projection (Ctrl+3).
5. Polygon Fill (4).
6. Smudge (5).
7. Clone (6). (Ctrl+6 absolute source variant)
8. Material Picker (P). Temporary: after one pick, the previous tool is re-equipped.
9. Quick Mask (U to edit).

Details:
* Buttons with variants show a tiny chevron in the bottom-right corner; clicking the chevron (or press-and-hold, unverified) opens a flyout listing variants with their shortcuts right-aligned. (screenshot)
* Tools are only usable when a paint layer (or paint effect / mask) is selected. With a fill layer or folder selected, the toolbar is effectively inactive and Properties shows the layer's properties. (doc)
* Separators between groups (unverified positions: after Path, after Clone).
* Active tool: highlighted background (lighter gray) and/or accent tint (unverified).

### 5.2 Contextual toolbar, left part (per context)

**Paint / Eraser / Smudge / Clone / Projection** (doc, screenshot):
`Size [slider 10] [pressure icon v] | Flow [slider 100] [pressure icon v] | Stroke opacity [slider 100] | Spacing [slider 20] | [lazy mouse toggle icon] Distance [slider, grayed when off] | [symmetry toggle] [symmetry settings v] | [hide excluded geometry]`
* Each field is an inline mini slider: label above-left, value above-right, slider track below. No popup is needed; the value field is editable on click. (screenshot)
* Pressure icon button: toggles pen pressure control for that param; the chevron opens a menu for the pressure mapping (unverified content).
* Symmetry settings popup [v11.1 layout] (screenshot): segmented buttons "Mirror X | Mirror Y | Mirror Z" (Radial mode also available); OFFSET section with reset icon and X/Y/Z sliders (unused axes grayed); SYMMETRY PLANE: Show Plane, Show Intersection; SYMMETRY CURSOR: Show Cursor, Hide While Painting; MANIPULATOR: Show Manipulator, Manipulator Size (0.4). Popup closes on click outside (unverified).

**Polygon Fill**: four mode toggle buttons: Triangle fill, Polygon fill, Mesh fill, UV chunk fill (combinable/switchable); plus symmetry. (doc)

**Fill layer selected, UV projection** (screenshot): `[show/hide manipulator] [handle size menu: Small/Medium/Large] | [mirror X] [mirror Y] | [reset pivot] | [reset transformation]`.

**Fill layer, 3D projections (tri-planar, planar, spherical, cylindrical, warp)** (screenshot): `[show/hide manipulator] Manipulator Size [0.3125] | [translate] [rotate] [scale] [surface (planar/warp only)] Grid steps [0.1] (+ Angle steps in settings) | [space: Local/World v] | [mirror X] [mirror Y] [mirror Z] | [reset transformation]` + symmetry [v11.1].

**Path tool** [v9]: show/hide viewport interface, display settings (handle size, path width, path color, unselected path color, normals), reverse direction, toggle corner/smooth, custom tangents, open/close path, delete vertex, symmetry, hide/ignore excluded geometry; per-vertex Size and Opacity sliders [v11.1]. (doc)

### 5.3 Docks toolbar (right) (doc, screenshot)

Vertical icon strip. Click: toggle a floating popover of that dock next to the strip (with accent border while open); click again to hide. Drag away: becomes a normal floating dock. Icons seen: Texture Set List, Layers, Properties (brush icon), Texture Set Settings (grid icon), Display settings (monitor+gear), Shader settings (globe), History (clock), Log (document), [v10] Substance 3D Assets, [v11] Send To / export window.

### 5.4 Status bar

Bottom of the main window, mostly empty; right side shows "Cache Disk Usage: N% | Version: x.y.z". (screenshot) Temporary messages may appear at left (unverified).

---

## 6. Properties panel

### 6.1 General anatomy (doc, screenshot)

* Dock title is "PROPERTIES - <CONTEXT>": "PROPERTIES - PAINT", "PROPERTIES - FILL", "PROPERTIES - FILTER", "PROPERTIES - COMPARE MASK", "PROPERTIES - LAYER 1 INSTANCE", "Properties - Folder 1 Source". When nothing applicable: "No properties found". (screenshot)
* Opened from the Docks toolbar, or as a quick popup by RMB in the viewport. (doc)
* Top: (paint tools) a preview strip: brush stroke preview on the left (click/drag in it to draw a custom test stroke; no pen pressure) and material preview sphere on the right (click/drag to rotate the lighting). (doc)
* Below the preview: a row of section icon tabs (accent underline on the active one). (screenshot)
* Sections are UPPERCASE headers; in [v11.1+] parameters are grouped in collapsible groups with per-group reset icons.
* Right-click inside Properties: "Create tool preset" (and similar preset actions). (doc)
* [v11.1] Path tools: a "Presets" category button row at the top (Favorites + categories; "Show more in Assets"). (doc)

### 6.2 Common widgets

**Resource slot** (screenshot):
* Empty: full-width gray button, bold title (e.g. "Stencil", "Material mode", "Filter", "Generator", "Base Color") and a subtitle "No Resource Selected" or "uniform color".
* Loaded: thumbnail on the left, bold title ("Alpha"), resource name below in tan text ("Shape"), an "X" (clear) button at the right, and a small chevron/dropdown under it (resource options, unverified content). Collapsible sub-groups follow: "> Attributes", "> Parameters" (the substance's exposed params), [v11.1] "> Resolution" (Resolution: Auto/Texture Set/Custom, Factor).
* Click the slot: opens the mini-shelf (resource picker popup) anchored below the slot: a search field prefilled with a usage query (for example `u:filter`), a reset button at the right of the search, a thumbnail grid with truncated names; older UI also had tabs "resources | anchor points" (anchor points tab lists anchors usable as references). Click a thumbnail to load it and close the popup. Escape / click outside closes. (screenshot; close behavior unverified)
* Drag from Assets onto a slot: loads it. [v9.1] Drag an external file from the OS onto a slot: imports and assigns. (doc)

**Material channel toggle buttons** (screenshot): a row of small outlined buttons with short channel labels: `color`, `height`, `rough`, `metal`, `nrm` (+ `emiss`, `opacity`, user channels, unverified labels). Enabled = accent outline/left bar and bright text; disabled = no outline, dimmed. Click toggles the channel on/off for painting/filling. Alt+click on Base Color re-enables all [v11]. When a mask is the target, the row shows a single `mask` button. (doc, screenshot)

**Material mode slot**: "Material mode / No Resource Selected" slot. Loading a material (sbsar or preset) here drives all channels at once and exposes its parameters. Then a centered "Or" separator line, then one slot per enabled channel (doc, screenshot):
* Color channel: slot "Base Color / uniform color" + a full-width color swatch bar (click opens the color picker).
* Grayscale channel: slot "Height / uniform color" + a gradient slider (black to white track) with value field (e.g. 0) and a small icon at the right (unverified: link/reset).
* Each slot can receive a bitmap/procedural/anchor point instead of the uniform value.

### 6.3 Paint tool (also Eraser, Smudge, Clone, Projection) (doc, screenshot)

Section order: [preview] > BRUSH > ALPHA > (PHYSICS only for Physical variants) > STENCIL > MATERIAL.

**BRUSH** (v9/v10 flat order; values shown are doc screenshot values / known defaults):

| Param | Type / default | Notes |
|---|---|---|
| Size | slider, 10 (screenshot), pressure toggle | relative to Size Space |
| Flow | slider 0-100, 100, pressure toggle | per-stamp opacity |
| Stroke opacity | slider 0-100, 100 | global stroke max opacity, no pressure |
| Spacing | slider, 20 (screenshot) | percent of size (unverified unit) |
| Angle | slider degrees, 0 (unverified default) | |
| Follow path | checkbox, off | needs 2+ stamps |
| Size jitter | 0-1, 0 | |
| Flow jitter | 0-1, 0 | |
| Angle jitter | 0-1, 0 | |
| Position jitter | 0-1, 0 | |
| Alignment | dropdown: Camera, Tangent \| Wrap (default), Tangent \| Planar, UV | |
| Backface culling | slider (angle), default 90 (unverified) | |
| Size space | dropdown: Object (default), Viewport, Texture | |

[v11.1+] grouped as (screenshot): `Size` (Size, Minimum Size (%), Spacing), `Opacity`, `Orientation`, `Jitter`, `Projection`, each collapsible with a reset icon; the BRUSH header has a settings menu icon.

**ALPHA**: resource slot (title "Alpha", e.g. "Shape"), then "> Attributes" and "> Parameters" groups (e.g. Hardness for the default alpha). (screenshot)

**PHYSICS** (Physical variants only): dt (0.05), "Emitter" slot (default_emitter) with its params, "Receiver" slot (default_receiver), then emitter/receiver params (e.g. Lifetime 3, Velocity_scale 0.3). (screenshot)

**STENCIL**: "Tiling mode" dropdown first (No Tiling (default), Horizontal Tiling, Vertical Tiling, H and V Tiling), then the "Stencil" slot ("No Resource Selected"). (doc, screenshot)

**MATERIAL**: channel toggle row, Material mode slot, "Or", per-channel slots (see 6.2). Eraser: only the channel toggles (no resources). (doc)

Clone specifics: "Clone source behavior" toggle (source follows after release vs. returns to the V-set point). (doc)
Projection: same as stencil controls; S+mouse transforms the projection. (doc)
Smudge/Clone: recommended use on a paint layer with all channels in Passthrough. (doc)

### 6.4 Fill layer / Fill effect (doc, screenshot)

Title "PROPERTIES - FILL". Icon tabs: Fill, Material. [v11.1] adds a SYMMETRY section for 3D projections.

**FILL**:

| Param | Values / default |
|---|---|
| Projection | Fill (match per UV Tile), UV projection (default, unverified), Tri-planar projection, Planar projection, Spherical projection, Cylindrical projection, Warp projection, [v9.1] UV set to UV set projection |
| Filtering | Bilinear \| HQ (default), Bilinear \| Sharp, Nearest |
| UV Wrap | None, Repeat horizontally, Repeat vertically, Repeat (default) (not on tri-planar) |
| Shape crop (3D projections) | Project cropped to shape / Projection extends outside shape (default) |
| Hardness (tri-planar) | blend between planes |
| Depth culling (planar) | checkbox + Hardness |
| Backface culling (planar) | checkbox + Angle + Hardness |

**v UV transformations** (collapsible):
* Scale mode: Tiling (default) / Physical Size (+ Custom Size, Size (cm)) [physical size feature].
* Scale: two sliders (U, V) with a lock button between them (locked = uniform; second slider grayed). Default 1. [v10.1] lock auto-disables when tiling is non-uniform.
* Rotation: slider, 0.
* Offset: two sliders (U, V), 0 (not available for tri-planar).

**3D projection settings** (3D projections): Offset (XYZ), Rotation (XYZ degrees), Scale (XYZ).

**MATERIAL**: same as paint tool material section. [v11] new fill layers/effects enable only Base Color by default.

Manipulator interaction (doc): click-drag inside = translate (Shift constrains to an axis), drag outside = rotate (Shift snaps 45 degrees), drag handles = scale (Shift keeps ratio, Ctrl mirrors around pivot, Ctrl+Shift both). 3D gizmo: axis/plane/screen handles, Shift = stepping by Grid steps / Angle steps.

### 6.5 Effects (doc, screenshot)

| Effect | Properties |
|---|---|
| Paint | Behaves like a paint layer inside the stack (paint with tools). |
| Fill | Same as fill layer (6.4). |
| Filter | Section "Filter" (or channel buttons first when on content: `color height rough metal nrm`; `mask` when on mask). Then a "Filter" slot ("No Resource Selected" or the filter name, "(Outdated)" suffix when outdated). Clicking opens the mini-shelf filtered with `u:filter`. Then the filter's parameters (e.g. Blur: Seed [Random button], Blur Intensity 0.11, Blur Wrap Off). |
| Generator | "Generator" slot (mini-shelf filtered to generators) then its parameters, plus mesh map inputs. |
| Levels | "Levels" header; row: "Affected channel" dropdown (left) and "Colors" dropdown (RGB / R / G / B) at right; top markers (shadows 0, midtones 1, highlights 1) over a histogram; bottom markers (output black 0, white 1); buttons: Reset, Auto, Invert; checkbox Clamp. One channel per Levels effect. |
| Compare mask (mask only) | COMPARE MASK: Channel dropdown; Compare: [source dropdown: Layers Below / This Layer / This Mask / Constant] [operator: lesser than / within tolerance / greater than] [target dropdown]; Constant; Hardness (0.9 shown); SOURCE CHANNELS HISTOGRAM. |
| Color selection | mask from ID map colors picked in the viewport (unverified params: picked colors list, tolerance, hardness). |
| Anchor point | Shows the list of layers referencing it. References: from a fill/fill effect/filter input, the mini-shelf "anchor points" tab; a "jump to anchor" button exists on the referencing slot. Only anchors below the referencing layer, same Texture Set. |

### 6.6 Mask selected

* Clicking the mask thumbnail targets the mask: paint tools paint grayscale; Properties MATERIAL section shows a single grayscale slot/slider (value 0-1) instead of channels; X inverts the grayscale value. (doc, unverified for exact layout)
* Viewport display mode can be switched to "Mask" (Alt+click the mask thumbnail). (doc)

### 6.7 Layer instance / folder

* Clicking an instance icon on a layer switches Properties to "Layer instance hierarchy:" tree listing `TextureSet / Layer Source` and `TextureSet / Layer Instance`; clicking an entry jumps to that layer and switches the Texture Set. (doc, screenshot)

---

## 7. Layer stack ("LAYERS" dock)

### 7.1 Header (doc, screenshot)

Left: **channel/view-mode dropdown** (e.g. "Base Color"), listing the Texture Set channels; it sets which channel's thumbnail, blend mode and opacity the rows display and edit.

Right, toolbar buttons in order ([v10+] screenshot):

| # | Icon | Action |
|---|---|---|
| 1 | magic wand | Add effect: dropdown menu of effects for the selected layer content or mask (Add generator, Add paint, Add fill, Add levels, Add compare mask (mask only), Add filter, Add color selection, Add anchor point) |
| 2 | mask icon | Create mask: menu: Add white mask, Add black mask, Add bitmap mask, Add mask with color selection, Add mask with height combination |
| 3 | brush (older: layers+ icon) | Add paint layer above selection |
| 4 | bucket | Add fill layer above selection |
| 5 | sphere icon | Add smart material: opens a mini-shelf of smart materials, inserts above selection |
| 6 | folder | Add folder above selection |
| 7 | trash | Delete selected layer/folder/effect |

All new items are inserted above the currently selected layer (doc).

### 7.2 Row anatomy (screenshot)

```
| [eye] [fold?] [thumb][mask thumb]  Name..............  [geo-mask] [instance?]  Norm v |
|               [bar ] [bar      ]                                                100  v |
```

* Eye icon (visibility toggle) at far left (older versions: a round radio-like dot). Hidden layers show a dimmed/empty eye.
* Folder rows: folder icon (closed/open) left of the thumbnail; click it to expand/collapse. Children are indented. (screenshot) (Collapse via clicking the folder icon is unverified but strongly implied by the open/closed icons.)
* Content thumbnail (about 32 px). [v10.1] "simplified thumbnails" default shows a type icon (brush for paint, sphere for fill) instead of a computed image.
* Mask thumbnail to the right of the content thumbnail when a mask exists. Selected thumbnail (content or mask) gets the accent border. While dragging a resource over a layer without a mask, a "ghost mask" placeholder icon appears; dropping on it creates a mask with that resource [v9.1].
* Under each thumbnail: a thin horizontal bar: gray = no effects, orange (doc says red) = at least one effect in that stack.
* Name: single line, ellipsized. Selected layer name in accent color.
* Geometry mask icon (dotted square, or count badge when partially excluded) on the right side before the blend column [v7.1+]. Click to enter geometry-mask editing; right-click for Copy geometry mask (Ctrl+Alt+C), Paste into geometry mask (Ctrl+Alt+V), Include all, Exclude all.
* Instance button (when the layer is an instance source/target): click shows the instance hierarchy in Properties.
* Right column, two lines: blend mode abbreviation with chevron (line 1: "Norm", "Pthr", "Ldge", "Drk", ...) and opacity with chevron (line 2: "100"). Click the blend label: dropdown of blend modes; Up/Down/wheel cycle it when focused. Click the opacity: a slider popup (unverified: popup vs inline drag). Both apply to the channel selected in the header dropdown. Right-click blend mode: "Apply to all channels". (doc)
* Row separators: 1px darker lines. Selected row: 1px accent border around the row and darker/tinted background.

### 7.3 Effects inside a layer (doc, screenshot)

* When a layer (content) is selected, its content effects are listed as child rows right below it; when its mask is selected, the mask effects are listed instead. (doc)
* Effect row: `[eye] [type icon] Name ......... [Norm v] [100 v] [x]`. Name examples: "Levels - Base Color", "Filter (empty)", "Generator (empty)", "Fill", "Paint", "Sharpen", "Mask Editor". The "x" deletes the effect. Some effects (filters) show no blend/opacity. (screenshot)
* Reorder effects by drag (drop indicator line), right-click menu, or Alt+Up/Down. (doc)
* Effect right-click menu (screenshot, exact):
  1. Cut effect(s) Ctrl+X
  2. Copy effect(s) Ctrl+C
  3. Paste layer(s) Ctrl+V (disabled when clipboard empty)
  4. Duplicate effect(s) Ctrl+D
  5. Remove effect(s) Del
  6. (separator)
  7. Move effect(s) to top Alt+Shift+Up
  8. Move effect(s) up Alt+Up
  9. Move effect(s) down Alt+Down
  10. Move effect(s) to bottom Alt+Shift+Down
  11. (separator)
  12. Add generator / Add paint / Add fill / Add levels / Add compare mask / Add filter / Add color selection / Add anchor point (each with its icon; items not valid in context are disabled)

### 7.4 Selection and editing (doc)

* Click: select layer (content). Click the mask thumbnail: select the mask. Click an effect row: select the effect.
* Ctrl+click: add/remove from multi-selection. Shift+click: range select from first to last.
* Double-click the name: rename inline (unverified for layers; doc confirms for Texture Sets, user channels, export templates, paths).
* Drag a row: a horizontal bar shows the drop position (above a layer / between two layers / below). Drop onto a folder row: moves the layer inside. Ctrl+drag: duplicates. (doc)
* Drag from Assets: material -> new fill layer; smart material -> new folder of layers; filter -> new layer in Passthrough on all channels; smart mask -> onto a layer: adds a black mask if none, else merges effects; Ctrl while dropping a smart mask replaces the effect list; Ctrl while dropping a fill/material creates it inside a black mask; Alt while dropping on a thumbnail replaces existing effects [v9.1]. Hovering a thumbnail while dragging switches the visible stack to that content/mask stack so you can drop precisely [v9.1]. External files can be dropped to import + create [v9.1]. (doc)
* Alt+LMB on mask thumbnail: viewport shows the mask. Shift+LMB on mask thumbnail: toggle mask on/off. (doc)

### 7.5 Layer right-click menu (screenshots, order partially unverified)

When the layer has no mask:
1. Add white mask
2. Add black mask
3. Add bitmap mask
4. Add mask with color selection
5. Add mask with height combination (also sets Height blend to Normal)
6. (separator)
7. Create smart material (folders)
8. Flatten group Ctrl+M [v12]
9. Export flattened group to files [v12]
10. (separator)
11. Cut layer(s) Ctrl+X
12. Copy layer(s) Ctrl+C
13. Paste layer(s) Ctrl+V
14. Paste layer as instance Ctrl+Shift+V
15. Duplicate layer(s) Ctrl+D
16. Instantiate across texture sets... Ctrl+Shift+D
17. Remove layer(s) Del
18. (separator)
19. Group layer(s) Ctrl+G
20. Row of color swatches (layer color tag) (screenshot shows a row of colored squares; purpose unverified)
21. Gamma corrected alpha/mask (checkable) (screenshot, older)
22. Add generator / Add paint / Add fill / Add levels / Add filter / Add color selection / Add anchor point (unverified that these appear in the layer menu in v9/10; they did in older versions)

When a mask exists, the mask block becomes (unverified order): Remove mask, Toggle mask, Invert mask background, Copy mask content, Paste into mask, Create smart mask. (doc lists these actions but not their order) Adding a new mask or removing it destroys the mask and its effects (doc warning).

### 7.6 Blend modes (doc, grouped with separators in the dropdown)

1. Normal, Passthrough, Disable, Replace
2. Multiply, Divide, Inverse Divide, Darken (Min), Lighten (Max)
3. Linear Dodge (Add), Subtract, Inverse Subtract, Difference, Exclusion, Signed Addition (AddSub)
4. Overlay, Screen, Linear Burn, Color Burn, Color Dodge
5. Soft Light, Hard Light, Vivid Light, Linear Light, Pin Light
6. Tint, Saturation, Color, Value
7. Normal Map Combine, Normal Map Detail, Normal Map Inverse Detail

Row abbreviations seen: Norm, Pthr, Ldge, Drk. Others (unverified): Disa, Repl, Mul, Div, IDiv, Lght, Sub, ISub, Diff, Excl, AddS, Ovrl, Scrn, LBrn, CBrn, CDdg, SftL, HrdL, VvdL, LinL, PinL, Tint, Sat, Col, Val, NMC, NMD, NMID.

Default: Normal for all channels, except Height which defaults to Linear Dodge (Add) (doc, compare-mask note).

### 7.7 Other layer stack facts

* Bottom layer is computed first; folder content is processed before siblings. (doc)
* Each layer has per-channel blend mode and opacity. (doc)
* Layer instancing: instance source/target icons; only the source is editable; cycles disable the instance. (doc)
* Flatten [v12]: creates a new fill layer with bitmaps and disables the source group. (doc)

---

## 8. Texture Sets

### 8.1 TEXTURE SET LIST dock (doc, screenshot)

* Header row: left icons: visibility menu (eye with menu: Show All, Hide All, Invert Show/Hide) and Focus mode (isolate current Texture Set; click again to exit). Right: "Settings" dropdown button.
* Settings menu: Hide Empty Descriptions (default), Hide All Descriptions, Show All Descriptions, Import Shader Parameters, Reassign Texture Sets.
* Rows: `[eye] [checkbox in baking mode] **Name** (bold) ........ [Shader instance dropdown "Main shader"]`. Optional description line under the name. Selected row: accent border. Click a row: makes it the edited Texture Set (updates layer stack, shader settings). Only one Texture Set editable at a time. Sub-stacks (material layering) listed under the name.
* Right-click a name: Show/Hide texture set, Edit name, Reset name to <original>, Edit Description. Double-click the name: rename.
* Shader button per row: assign shader instance or "New shader instance".

### 8.2 TEXTURE SET SETTINGS dock (doc, screenshot)

Icon tabs at top (General, Channels/Material, Mesh maps). Sections:

**GENERAL PROPERTIES**: Name (text), Description (multi-line box with placeholder "Description"), Size: [dropdown 2048] [lock button] [dropdown grayed when locked] (128 up to 4096 in-app), Shader Instance [dropdown "Main shader"].

**CHANNELS**: label "Channels" with "+" button at the right (menu: Supported channels / Unsupported channels / User channels). Each row: bold channel name, format label at right ("sRGB8", "L8", "L16F", "RGB16F"...), color-managed icon (when applicable), gear button (channel settings menu: sRGB8, L8, RGB8, L16, RGB16, L16F, RGB16F, L32F, RGB32F, plus other actions), "x" remove. User channels renamed by double-click. Older layout: name field + format dropdown + "-" button.

**Mixing settings**: Normal mixing (Replace / Combine (default)), Height to normal method (Sharp / Smooth (Sobel) (default)), Ambient occlusion mixing (Replace / Multiply (default); grayed when the AO channel is missing), UV padding (3D Space Neighbor (default) / 2D Space Neighbor).

**MESH MAPS**: "Bake Mesh Maps" button at the top right; then a slot per map: Normal, World space normal, ID, Ambient occlusion, Curvature, Position, Thickness (+ Height, Bent normals, Opacity in newer bakers). Slot = thumbnail + bold map name + file name (e.g. "01_Head_normal_base") + "x". Click a slot to pick a texture; drag images onto it.

---

## 9. Assets panel (formerly Shelf) (doc, screenshot)

### 9.1 Layout

* Title "ASSETS".
* Breadcrumb row: "> All libraries" with arrows; clicking an arrow shows the on-disk structure to navigate; grayed items contain no assets of the selected type.
* Search field with magnifier icon. Supports advanced queries: `n:` name, `s:` shelf/library (incl. "session", "project"), `p:` path, `u:` usage; comma = AND; `-` exclude; suffixes: `!` contains (no fuzzy), `#` regex, `=` equal, `^` starts with, `&` ends with; escape spaces with `\` or quotes. Saved search tags appear as chips in the field (hover for info).
* Asset type icon row (single select by default, Ctrl+click for multi): Materials (default), Smart materials, Smart masks, Filters, Brushes (brushes, particles, tools), Alphas, Textures, Environment maps, [v10] Fonts. At the far right: a grid icon (thumbnail size / view). In reduced width, types collapse into a single dropdown menu.
* Thumbnail grid: square thumbnails (materials as spheres), name under each thumbnail ellipsized ("Concrete ..."), small badges at the bottom-right (substance/procedural marker, update marker). Selected thumbnail: accent border. Scroll with wheel, scrollbar, or Ctrl+Alt+drag.
* Bottom bar: left: toggles for "Filter by path" and "Saved searches" sections; right: reload/auto-update [v11], "new sub-library tab" from current search, "+" import resources. Thumbnail size: Small, Medium (default), Large, List. (exact icon mapping unverified)
* Optional side sections: Filter by path (tree, "Hide non-applicable folders" on by default), Saved searches (right-click: Create new tab, Rename, Delete).

### 9.2 Interactions

* Single click a brush/tool/particle preset: loads it into the current tool (unverified; doc says clicking a particle preset enables physics).
* Drag and drop targets: viewport (creates layer; materials -> fill layer; Ctrl = only on selection via Color ID; Alt = force decal/warp projection), layer stack (new layer/effect/mask, see 7.4), Properties slots, environment map onto the viewport (sets environment), Display settings env slot.
* Double click: no documented action (unverified; do nothing or open preview).
* Right-click a thumbnail (screenshot, exact order): info block (Name, Usages, Path, Category, Tags, Author, Physical size (in cm)), separator, Reload, Delete, separator, Export resource..., Set custom preview..., Show in >, Add to favorites (or Remove from favorites), separator, Close.
* Sub-library tabs: new dock tab with a fixed query; can be docked anywhere; resetting returns to its query.

### 9.3 Import resources dialog (doc, screenshot)

Opened by File > Import resources..., the Assets "+" button, or dropping files on the Assets panel.
* Top: "Import your resources and set their usage" with buttons "Add resources" and "Remove selected resources".
* Filter dropdown: "All imported resources (N)" (filter by usage, e.g. undefined).
* Table rows: file name | Prefix field (placeholder "Optional prefix/path...") | Usage button/dropdown ("undefined", texture, alpha, environment, basematerial, filter, generator, ...). Multi-select rows to edit usage together.
* Bottom: "Import your resources to:" dropdown: Current session / Project "<name>" / Shelf "<name>" (writable library). Buttons: Import (disabled until valid), Cancel. Hint text "To add a new shelf, see the Edit / Settings menu".

---

## 10. Dialogs, popups and secondary panels

### 10.1 Docking rules

* Docks: tab together, split, float, close; title bar buttons: float/dock toggle and close. Closed docks are reopened via Window > Views or the Docks toolbar. Window > Reset UI restores defaults. (doc)
* Color picker: floating only, cannot be docked; semi-persistent (stays open until context changes, e.g. switching from paint layer to fill layer). (doc)
* Menus/mini-shelves/flyouts: close on selection, Escape, or click outside (unverified but standard).
* Modal dialogs: New project, Export textures, Import resources, Settings, Project configuration, Instantiate across texture sets, bake result dialog (unverified modality for Export: it is a separate window with its own title bar and help "?" button).

### 10.2 Display settings dock (doc)

Top: View mode dropdown (Lighting / Single channel / Mesh maps sections). Then:

**Environment**: Environment Map (slot, mini-shelf of environment maps), Override environment map color space (color managed projects), Environment Opacity, Environment Exposure (EV, keep 0), Environment Rotation (also Shift+RMB), Environment Blur, Environment Alignment (World (default) / Local).
**Shadows**: Shadows (on/off), Computation mode (Intensive / Average / Lightweight (default)), Shadows opacity.
**Camera**: Preset (camera selector, Restore button, lock icon), Field of View (degrees), Focal length (mm), Focus Distance (Ctrl+MMB sets it), Aperture, Display ratio.
**Post effects**: (see post processing) DOF, bloom, vignette, etc. (unverified list).
**Temporal anti-aliasing**: enable, Accumulations (16 recommended, 64 for high contrast).
**Subsurface scattering**, **Color profile** (LUT), **Tone mapping**: Function (Linear (default) / ACES).
**Viewport settings**: Texture filtering: Anisotropic Filtering (Disabled / Low 2spp / Medium 4spp (default) / High 8spp / Very High 16spp), MipMap bias (0 Soft (default) to 3 Very Sharp). Camera frame (+ Gate mask opacity). Tool display: Hide stencil when painting, Stencil display opacity, Projection preview channel. Mesh wireframe: Show mesh wireframe, Wireframe color, Wireframe opacity. Channel display (single channel mode only): Display solo view without lighting (unlit), Scale HDR values, Use +/- color for HDR values, Color channels (RGBA / Grayscale+Alpha / R / G / B / A). Grid: Show grid, Axis (Y default), Grid color, Grid opacity.

### 10.3 Shader settings dock (doc, screenshot)

Top: Undo / Redo buttons (independent shader undo stack), shader file button (e.g. "pbr-metal-rough-with-alpha-test (Outdated)", click opens mini-shelf of shaders), Instance name field, "Restore defaults" button. Then SHADER PARAMETERS (depends on shader; ASM [v9.1] has Translucency, Absorption, etc. with tooltips). Then Displacement: Source Channel (Height default / Displacement), Scale unit [v11.1] (Normalized / Scene / Physical size (cm)), Scale amount. Tessellation: Subdivision Mode (Uniform (default) / Edge Length), Subdivision Count (1-32), Max Length.

### 10.4 Baking mode [v10 era] (doc, screenshot)

Entered with F8, Mode > Bake mesh maps, or the croissant button. Panels:
* **TEXTURE SET LIST** (with checkboxes per Texture Set to include in bake).
* **MESH MAP BAKERS**: "Common settings" button (gear) at top; settings menu icon (Synchronize settings, Sync all settings to more Texture sets..., Sync all settings to all Texture Sets, Check all, Uncheck all, Invert checked mesh maps, Apply checked to more Texture sets..., Apply checked to all Texture sets). Rows: [checkbox] [map icon] **Map name** [visualize eye] [quick bake play] [auto-rebake] [sync link icon]. Maps: Normal, World space normal, ID, Ambient occlusion, Curvature, Position, Thickness, Height, Bent normals, Opacity. Click-drag across checkboxes toggles many. Right-click a map: "Apply <map> settings to more Texture sets...".
* **MESH MAP SETTINGS**: breadcrumb "> TextureSet > Common settings" (or the map name). Common: Output Size [dropdown][lock][dropdown], Dilation Width (slider), Apply Diffusion; High poly parameters: Use Low Poly Mesh as High Poly Mesh, High Definition Meshes (file list + file button), Cage (Distance based / Automatic / Custom file), Use Cage, Cage File, Max Frontal Distance, Max Rear Distance, Relative to Bounding Box, Average Normals, Ignore Backface, Antialiasing (None / ...), Match (Always / By mesh name), Low poly mesh suffix "_low", High poly mesh suffix "_high", Ignore backfaces suffix "_ignorebf"; Skew correction section. Per-map settings listed in doc (AO: Secondary rays, Min/Max occluder distance, Relative to bounding box, Spread angle, Distribution, Ignore backface, Self occlusion, Attenuation, Ground plane, Ground plane offset; Curvature: Method, Secondary rays, Sampling radius, Self intersection, Auto tonemapping, Min/Max; ID: Color source (Vertex Color / Material Color / File ID / Mesh ID/Polygroup), Color Generator (Random / Hue shift / Grayscale); Position: Mode, Axis, Normalization type, Normalization scale; Thickness; Height: Normalization, Scaling divisor; Bent normals).
* **BAKING LOG**: tabs BAKING PROCESS / HIGH POLY PARAMETERS / MATCHING BY NAME; placeholder "Baking progress details appear here."
* Viewport: "Baking visualization" overlay panel (top left) with collapsible groups (High definition mesh (HP): Mesh color, Matching error color; Cage: Cage surface + opacity, Cage wireframe + opacity; UV seams; Project mesh (LP); Neutral material), "Show for selected Texture Set only". Bottom center buttons: primary pill "Bake selected textures" with a split chevron, outlined pill "Return to painting mode".
* Result dialog: title, "Baking finished N/N", red error line ("1 baking process failed. See log for details."), list of maps with a check or red X, a teal progress bar "100%", buttons "Open Log" and "OK". (older screenshot)

### 10.5 Export textures window (doc, screenshot)

Title "Export textures" (own window with ?, minimize, maximize, close). Top-right button "Include all" (unverified purpose). Tabs: SETTINGS | OUTPUT TEMPLATES | LIST OF EXPORTS.
* SETTINGS: left list: "Global settings" (selected by default) then one row per Texture Set with checkbox; list menu icon (Check all, Uncheck all, Invert all). Right "General Export Parameters" with a reset icon: Output directory (path field + browse), Output template (dropdown, e.g. "Document channels + Normal + AO (No Alpha)"), File type ([png v] [8/16 bits v]; or "Based on output template"), Size (Based on each Texture Set's size / 128 ... 8192), Padding ([No padding (passthrough) / Dilation infinite / Dilation + transparent / Dilation + default background color / Diffusion ...] + distance dropdown e.g. 16), Export shaders parameters (checkbox), Export USD asset [v9+]. Selecting a Texture Set row shows overrides and an "Output maps" list (enable/disable maps, override format/bit depth).
* Bottom bar: "Reset all" (left), "N textures selected for export" text, Cancel, Save settings, Export (Export switches to LIST OF EXPORTS).
* OUTPUT TEMPLATES: presets list (+ Add, Duplicate, Remove; double-click renames; right-click menu), Output maps list with map type buttons (Gray, RGB, R+G+B, RGB+A, R+G+B+A), map name field with "$" keyword button ($project, $mesh, $textureset, $udim, $colorSpace), format and bit-depth dropdowns, source lists (Input maps, Mesh maps, Converted maps) dragged into slots.
* LIST OF EXPORTS: per Texture Set list and a console with status/errors.

### 10.6 New project dialog (doc, screenshot)

[v9/10 style] Template dropdown ("Select template..."), File [Select...], Project Settings: Document Resolution (1024 default in older screenshot), Normal Map Format (DirectX/OpenGL), Compute Tangent Space Per Fragment, UV Tile Settings (UDIMs) "Cannot be changed later", Import settings (Import cameras, Auto-unwrap + Options), Import baked maps (Add), buttons OK / Cancel.
[v12 style] BASIC SETTINGS: 3D mesh ("No file selected" + Select...), Project template ("Default (OpenPBR)"), Document resolution (px) ("2048 x 2048"). v ADVANCED SETTINGS: Normal map format (OpenGL), Compute tangent space per fragment, UV Tile settings (UDIMs) (with academy and info icons; italic warning "You won't be able to change UV Tile settings after creating the project."), Use UV Tile workflow + 2 radio options, Import settings (Import cameras checked, Auto-unwrap + "Auto-unwrap options"), collapsible Import baked maps, Physical size, Color management. Buttons: Cancel (outlined pill), Create (filled, disabled until a mesh is chosen). Each row has an info "i" icon.

### 10.7 Settings (Edit > Settings) (doc)

Pages: General (Interface options: Language, Show keyboard helper, Show world axes, Background color (2-color gradient), Only display the selected material when painting, Viewport Scaling (None/Auto); Layer stack options: Default UV scale for materials, Use simplified thumbnails; Camera options: Rotation speed, Zoom speed (+inverse), Wheel speed (+inverse); Baking options; Preview options; Temporary files; Sparse virtual textures; Iray hardware; Privacy), Libraries, Shortcuts (list, edit with pen icon, right-click to reset).

### 10.8 Color picker (doc, screenshot)

Floating, not dockable, title "<Channel> color" (e.g. "Base color color"). Vertical layout:
1. Display selector (color-managed projects only).
2. Spectrum: large saturation/value square with crosshair + vertical hue bar on its right.
3. Current color (left rectangle) and previous color (right rectangle, click to restore); "#" hex field (always sRGB); eyedropper button (click, then click anywhere on screen; Shift while picking in the viewport picks the raw channel value).
4. Sliders: mode dropdown HSV / RGB; settings: Dynamic sliders (colored track backgrounds), Floating point values (0-1) vs 0-255 / degrees and percent.
5. Working color space value (color managed only).
6. Swatches: "+" adds the current color (only if different from last swatch), click a swatch to load, hover shows hex, right-click a swatch to delete; settings menu: delete all. Swatches persist across projects/sessions.
Every color field in the app also has a small eyedropper next to it.

### 10.9 HISTORY dock (doc, screenshot)

Plain list, first item "<empty>", then action names ("Add layer", "Add fill layer", "color", "Add stroke", "Add mask", "Add generator", "Set generator"). Current state highlighted with accent border. Click an item to go back to that state; doing a new action from a past state discards the future. Global across Texture Sets; session only (not saved).

### 10.10 LOG dock (doc, screenshot)

Two full-width tabs GENERAL | ASSETS (accent underline on active). Lines prefixed with a bracketed source: "[Plugin - autosave] Autosaving...", "[Paint Document] Project was last saved with ...". Warnings/errors colored (unverified colors: yellow/red).

### 10.11 Main menus (doc, screenshot)

* File: New... Ctrl+N, Open... Ctrl+O, Recent Files >, (sep) Open Sample..., (sep) Close, (sep) Save Ctrl+S, Save And Compact, Save As..., Save As Copy..., Save As Template..., Clean..., (sep) Import resources..., Export Mesh..., Export Textures... Ctrl+Shift+E, (sep) Send to >, (sep) Quit.
* Edit: Undo, Redo, Project Configuration..., Settings..., Bake Mesh Maps Ctrl+Shift+B.
* Mode: Bake mesh maps, Painting, Rendering (Iray).
* Window: Views >, Toolbars >, (sep) Hide UI, Reset UI.
* Viewport: Display material M, Display next channel C, Display previous channel Shift+C, Display next mesh map B, Display previous mesh map Shift+B, (sep) Show entire mesh F, (sep) Enable quick mask Y, Edit quick mask U, Invert quick mask I.
* Python / JavaScript: one entry per plugin with enable/disable, reload, configure, about.
* Help: Tutorials, Release notes, Documentation, Shortcuts list, Scripting documentation, Forums, Report a bug, Show log, Export log, Give feedback, Manage my account, Sign out, Welcome screen, What's new, Home screen, Check for updates, About, Partners.
* Menu look: dark bg, shortcut right-aligned in lighter gray, disabled items dimmed, separators as thin lines, submenu arrow ">" at right.

---

## 11. Small details that sell the illusion

* Tooltips everywhere with the action name; tool flyouts show the shortcut right-aligned. (screenshot)
* Keyboard helper overlay in the viewport bottom-left when holding Ctrl/Shift/Alt (lists what the modifier does with each mouse button). (doc pref)
* While the engine computes, the progress bar at the bottom of the viewport fills; thumbnails update lazily.
* Pause engine (Shift+Esc) shows a paused state (unverified visual).
* New layer names: "Layer 1", "Fill layer 1", "Folder 1"; copies/duplicates get a consistent suffix [v11] (unverified exact pattern, e.g. "Layer 1 copy").
* Effect default names: "Filter (empty)", "Generator (empty)", "Levels - Base Color", "Fill", "Paint", "Compare mask". (screenshot)
* Instances named "<Layer> Source" / "<Layer> Instance". (screenshot)
* Material preview sphere is interactive (drag rotates light). Brush preview strip is drawable.
* Viewport background is a vertical 2-color gradient (pref "Background color", two swatches). (doc)
* Only the selected Texture Set is shown while painting if the pref is on. (doc)
* Clicking empty space in the layer stack does not deselect (unverified).
* Panels remember scroll position per context (unverified).

---

## 12. egui implementation checklist (derived)

1. Theme: set `Visuals::dark()` then override: `panel_fill #333333`, `extreme_bg_color #262626`, `faint_bg_color #2B2B2B`, `selection.bg_fill` accent at low alpha and `selection.stroke` accent `#1AA4FF` 1px, widget rounding 2 px (pill 999 for dialog buttons), item spacing about (6, 4).
2. Write a `PainterSlider` widget: label + right value field on one line, full-width thin track (2 px) + 10 px round knob on the next; click-jump, drag, Shift precision (x0.1), Up/Down step when focused, click value to type.
3. Write a `ResourceSlot` widget (empty/loaded states, X clear, chevron, drop target) and a `MiniShelf` popup (search prefilled with `u:<usage>`, grid, Escape/click-outside close).
4. Channel toggle row widget (outlined chips).
5. Layer row widget: fixed 46 px height, eye / fold / thumb / mask thumb / name / geo-mask / blend+opacity column; child effect rows 26 px with indent; drag-and-drop with insertion line; context menus exactly as section 7.3 / 7.5.
6. Global input router implementing section 3 (modifier gestures in the viewport take precedence over tool LMB).
7. Dock system: use `egui_dock` with uppercase titles, float/close buttons, plus a right-side icon strip that toggles floating popover docks.
8. Contextual toolbar driven by the current tool/selection (section 5.2).

---

## 13. Sources

Primary (official Adobe documentation source, rendered at experienceleague.adobe.com/en/docs/substance-3d-painter/using/... and helpx.adobe.com/substance-3d-painter/...; helpx returned HTTP 403 to automated fetches, so the identical markdown source in Adobe's public GitHub repo was used):

* https://github.com/AdobeDocs/substance-3d-painter.en (repo root, `help/` folder, and `help/assets/` screenshots)
* help/interface/settings/shortcuts.md : https://helpx.adobe.com/substance-3d-painter/interface/settings/shortcuts.html and https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/settings/shortcuts
* help/interface/interface.md, toolbars.md, properties.md, history.md, color-picker.md, project-configuration.md
* help/interface/miscellaneous/sliders.md, log.md
* help/interface/viewport/viewport.md, 2d-view.md, 3d-view.md, camera-management.md
* help/interface/layer-stack/layer-stack.md, creating-layers.md, managing-layers.md, masking-and-effects.md, blending-modes.md, layer-instancing.md, geometry-mask.md, flatten-layers.md
* help/interface/texture-set/texture-set.md, texture-set-list.md, texture-set-settings.md
* help/interface/assets/assets.md, navigation.md, customizing-the-layout.md, sub-library-tab.md, filter-by-path.md, saved-searches.md, advanced-search-queries.md
* help/interface/display-settings/display-settings.md, environment-settings.md, camera-settings.md, viewport-settings.md
* help/interface/shader-settings/shader-settings.md
* help/interface/main-menu/file-menu.md, edit-menu.md, mode-menu.md, window-menu.md, viewport-menu.md, plugins-menu.md, help-menu.md
* help/interface/settings/general-preferences.md
* help/interface/baking-panels/mesh-map-bakers.md, common-mesh-map-settings.md, baking-log.md
* help/baking/baking-interface.md, mesh-map-settings.md
* help/export/export.md, export-window/export-window.md, export-settings.md, output-templates.md, list-of-exports.md
* help/getting-started/project-creation.md
* help/content/importing-assets/adding-resources-via-the-import-window.md
* help/features/effects/effects.md, fill.md, paint.md, filter.md, generator.md, levels.md, anchor-point.md, compare-mask.md
* help/features/smart-materials-and-masks.md
* help/painting/painting.md, lazy-mouse.md, straight-line.md, symmetry/symmetry.md, symmetry/mirror-symmetry.md, symmetry/radial-symmetry.md
* help/painting/tool-list/tool-list.md, paint-brush.md, eraser.md, projection.md, polygon-fill.md, smudge-tool.md, clone-tool.md, quick-mask.md, path.md
* help/painting/fill-projections/fill-projections.md, uv-projection.md, tri-planar-projection.md, planar-projection.md
* help/release-notes/version-9-0.md, version-9-1.md, version-10-0.md, version-10-1.md, version-11-0.md, version-11-1.md
* Screenshots read from help/assets/: layer-stack.png, layer-stack-icons.png, layer-stack-thumbnails.png, layer-menu.png, effects-contextual-menu.png, paste-as-layer-instance.png, v12_flatten_menu.jpg, geo-mask-contextual-menu.png, level-menu.png, layer-icon.png, tool-toolbar.png, v90_path_toolbar.png, contextual-toolbar.png, contextual-toolbar_1.png, contextual-toolbar-manipulator.png, paint-toolbar.png, dock-toolbar.png, right-bar.png, properties-window.png, fill-properties.png, properties-mat.png, material.png, alpha-1.png, stencil.png, brush-4.png, physics-1.png, tools_ui_groups.png, symmetry-menu.png, texture-set-list.png, texture-set-settings.png, mesh-maps.png, mixing.png, v10_assets_overview_1.png, v10_assets_overview_2.png, v10_assets_list.png, favorite-right-click.png, import-window-240.png, mask-view-mode.png, viewport-viewmode.png, viewport-menu.png, file-menu.png, window-menu.png, edit-menu.png, toolbar-14.png, history-list.png, log.png, shader-settings-window.png, export-textures-window.png, new-project-window.png, New-project-window-full.png, settings-interface.png, baking-mode-overview.jpg, baking-dialog.png, v1210_baking_meshmapbakers.png, image2015-3-20-11-24-24.png (levels), compare-mask.png, anchor-point-resource.png, slider-mode.png, and frames of group-layer.gif, layer-mask.gif, layer-stack-selection-ui.gif, layer-drag.gif, re-order-effects.gif, v91_drop_new_mask.gif, picker-persistent.gif, quick-menu-optim.gif, filter-select-shelf.gif, instance-properties-optim.gif, ui-dock-collapse-recall-optim.gif, value-edit-slider.gif, slider-soft-range.gif, hide-slider.gif

Secondary (search results consulted for confirmation):

* https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/layer-stack/layer-instancing
* https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/layer-stack/masking-and-effects
* https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/release-notes/old-versions/version-2017-4
* https://80.lv/articles/substance-painter-update-layer-instances-and-more
* https://community.adobe.com/t5/substance-3d-painter-discussions/no-shortcut-keys-possible-for-zoom-in-or-out-in-substance-painter/m-p/13996304
