//! The application: state, frame loop and GPU orchestration. UI pieces live
//! in the submodules (panels, viewport input, shelf, dialogs).

mod autouv;
mod dialogs;
mod dock;
mod eyedrop;
mod gizmo;
mod gradient;
mod icons;
mod keymap;
mod polysel;
mod uvgizmo;
mod layers;
mod layout;
mod panels;
mod path;
mod props;
mod region;
mod selection;
mod shelf;
mod crawl;
mod uitest;
mod view3d;
mod widgets;

pub use crawl::ui_crawl;
pub use uitest::{ui_bench, ui_smoke};

/// Set by the headless UI tests: native file dialogs would block them.
pub static HEADLESS: AtomicBool = AtomicBool::new(false);

/// What waits for the unsaved-changes prompt.
enum Discard {
    Open(PathBuf),
    Create(PathBuf, u32),
}

/// A native file dialog, or None when running headless.
pub(crate) fn file_dialog() -> Option<rfd::FileDialog> {
    (!HEADLESS.load(std::sync::atomic::Ordering::Relaxed)).then(rfd::FileDialog::new)
}

use crate::bake::{self, BakeSettings, HighPoly};
use crate::brush::{AlphaSource, BrushMaterial, BrushSettings, Stroke};
use crate::camera::OrbitCamera;
use crate::document::{Document, Target, TextureSet};
use crate::engine::paint::{PaintPass, Stamp};
use crate::engine::texture::f32_to_f16_bytes;
use crate::engine::viewport::{CameraUniform, DrawSet, Frame, PostUniform, SetUniform};
use crate::engine::Engine;
use crate::history::{Entry, History};
use crate::{mesh, project, style};
use eframe::egui::{self, Key, Modifiers, Rect};
use eframe::egui_wgpu;
use glam::{Mat4, Vec2, Vec3};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Instant;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    Paint,
    Erase,
    Clone,
    Smudge,
    PolygonFill,
    Picker,
    /// Geometry mask: hide parts of the mesh.
    Hide,
    /// Marquee and lasso selections.
    Select(selection::SelShape),
    /// Magic wand: selects similar colors.
    Wand,
    /// Paint bucket: fills similar colors with the brush material.
    Bucket,
    /// Gradient of the brush material (drag a line).
    Gradient,
    /// Paints the selection with the brush (Q; X swaps add / subtract).
    SelBrush,
    /// Path (P): a curve of points that one brush stroke follows.
    Path,
    /// Projection (Shift + B): the brush paints the stencil texture laid
    /// over the view.
    Projection,
}

impl Tool {
    /// Tools that paint with brush strokes.
    pub fn is_brush(self) -> bool {
        matches!(self, Tool::Paint | Tool::Erase | Tool::Clone | Tool::Smudge | Tool::SelBrush | Tool::Projection)
    }

    /// Tools that use the brush (its cursor, its properties).
    pub fn uses_brush(self) -> bool {
        self.is_brush() || self == Tool::Path
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PolyMode {
    Triangle,
    /// One face of the source mesh (quad or n-gon).
    Polygon,
    Mesh,
    UvIsland,
}

impl PolyMode {
    pub const ALL: [PolyMode; 4] = [PolyMode::Triangle, PolyMode::Polygon, PolyMode::Mesh, PolyMode::UvIsland];
    pub fn name(self) -> &'static str {
        match self {
            PolyMode::Triangle => "Triangle fill",
            PolyMode::Polygon => "Polygon fill",
            PolyMode::Mesh => "Mesh fill",
            PolyMode::UvIsland => "UV chunk fill",
        }
    }
    pub fn icon(self) -> icons::Icon {
        match self {
            PolyMode::Triangle => icons::Icon::SelTriangle,
            PolyMode::Polygon => icons::Icon::SelPolygon,
            PolyMode::Mesh => icons::Icon::SelMesh,
            PolyMode::UvIsland => icons::Icon::SelIsland,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Display {
    Material,
    BaseColor,
    Roughness,
    Metallic,
    Normal,
    Height,
    Ao,
    Emissive,
    Opacity,
    UvChecker,
    MapAo,
    MapCurvature,
    MapNormal,
    MapWorldNormal,
    MapPosition,
    MapThickness,
    MapId,
    LayerMask,
    SpecularLevel,
    Transmission,
    /// An optional channel past the first two (`document::EXT_NAMES`).
    Ext(u8),
}

impl Display {
    pub const ALL: [Display; 32] = [
        Display::Material,
        Display::BaseColor,
        Display::Roughness,
        Display::Metallic,
        Display::Normal,
        Display::Height,
        Display::Ao,
        Display::Emissive,
        Display::Opacity,
        Display::SpecularLevel,
        Display::Transmission,
        Display::Ext(2),
        Display::Ext(3),
        Display::Ext(4),
        Display::Ext(5),
        Display::Ext(6),
        Display::Ext(7),
        Display::Ext(8),
        Display::Ext(9),
        Display::Ext(10),
        Display::Ext(11),
        Display::Ext(12),
        Display::Ext(13),
        Display::UvChecker,
        Display::MapAo,
        Display::MapCurvature,
        Display::MapNormal,
        Display::MapWorldNormal,
        Display::MapPosition,
        Display::MapThickness,
        Display::MapId,
        Display::LayerMask,
    ];
    /// Position in `ALL`.
    pub fn index(self) -> usize {
        Display::ALL.iter().position(|d| *d == self).unwrap_or(0)
    }
    pub fn name(self) -> &'static str {
        match self {
            Display::Material => "Material",
            Display::BaseColor => "Base color",
            Display::Roughness => "Roughness",
            Display::Metallic => "Metallic",
            Display::Normal => "Normal",
            Display::Height => "Height",
            Display::Ao => "Ambient occlusion (applied)",
            Display::Emissive => "Emissive",
            Display::Opacity => "Opacity",
            Display::UvChecker => "UV checker",
            Display::MapAo => "Mesh map: ambient occlusion",
            Display::MapCurvature => "Mesh map: curvature",
            Display::MapNormal => "Mesh map: normal",
            Display::MapWorldNormal => "Mesh map: world normal",
            Display::MapPosition => "Mesh map: position",
            Display::MapThickness => "Mesh map: thickness",
            Display::MapId => "Mesh map: ID",
            Display::LayerMask => "Selected layer mask",
            Display::SpecularLevel => "Specular level",
            Display::Transmission => "Transmission",
            Display::Ext(i) => crate::document::EXT_NAMES[i as usize],
        }
    }
    /// (viewport display code, display texture kind)
    fn code(self) -> (f32, f32) {
        match self {
            Display::Material => (0.0, 0.0),
            Display::BaseColor => (1.0, 0.0),
            Display::Roughness => (2.0, 0.0),
            Display::Metallic => (3.0, 0.0),
            Display::Normal => (4.0, 0.0),
            Display::Height => (5.0, 0.0),
            Display::Ao => (6.0, 0.0),
            Display::Emissive => (7.0, 0.0),
            Display::Opacity => (8.0, 0.0),
            Display::UvChecker => (9.0, 0.0),
            Display::MapNormal | Display::MapWorldNormal => (10.0, 2.0),
            Display::MapPosition | Display::MapId => (10.0, 1.0),
            Display::SpecularLevel => (11.0, 0.0),
            Display::Transmission => (12.0, 0.0),
            // One component of an optional group's final.
            Display::Ext(i) => (10.0, 3.0 + crate::document::ext_slot(i as usize).map(|s| s.1).unwrap_or(0) as f32),
            _ => (10.0, 0.0),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Layout {
    Both,
    Only3D,
    Only2D,
}

#[derive(Clone)]
pub struct ViewSettings {
    /// Channel shown by the 3D view.
    pub display: Display,
    /// Channel shown by the 2D view.
    pub display_2d: Display,
    pub exposure: f32,
    pub env_rotation: f32,
    pub bg_blur: f32,
    pub bg_intensity: f32,
    pub flat_bg: bool,
    pub tonemap: u32,
    pub bloom: f32,
    pub bloom_threshold: f32,
    pub vignette: f32,
    pub saturation: f32,
    pub contrast: f32,
    pub shadows: bool,
    pub shadow_strength: f32,
    pub light_intensity: f32,
    pub wireframe: bool,
    pub layout: Layout,
    pub uv_zoom: f32,
    pub uv_pan: [f32; 2],
    /// 2D view rotation (radians): the UV space turns around its (0, 0)
    /// before the pan (Alt + left drag, F resets it).
    pub uv_rot: f32,
    pub uv_wire: f32,
    /// The current tool's shortcuts in the bottom left corner of the view.
    pub show_tool_keys: bool,
    /// Wireframe lines (both views, linear color) and the 3D view's opacity.
    pub wire_color: [f32; 3],
    pub wire_opacity: f32,
    pub show_assets: bool,
    /// F4: 2D view on the left, 3D on the right.
    pub swap_views: bool,
    /// Share of the left view when both are shown (drag the divider).
    pub split: f32,
    pub show_properties: bool,
    /// Path-traced render mode.
    pub path_trace: bool,
}

impl Default for ViewSettings {
    fn default() -> Self {
        ViewSettings {
            display: Display::Material,
            display_2d: Display::BaseColor,
            exposure: 0.0,
            // 180 degrees: the default environment's best side.
            env_rotation: 180.0,
            bg_blur: 4.0,
            bg_intensity: 1.0,
            flat_bg: true,
            // The real application's defaults: linear tone mapping, no post effects.
            tonemap: 3,
            bloom: 0.0,
            bloom_threshold: 1.5,
            vignette: 0.0,
            saturation: 1.0,
            contrast: 1.0,
            shadows: true,
            shadow_strength: 0.7,
            light_intensity: 1.2,
            wireframe: false,
            layout: Layout::Only3D,
            uv_zoom: 0.0,
            uv_pan: [0.0, 0.0],
            uv_rot: 0.0,
            uv_wire: 0.35,
            show_tool_keys: true,
            wire_color: [0.85, 0.85, 0.9],
            wire_opacity: 0.35,
            show_assets: true,
            swap_views: false,
            split: 0.5,
            show_properties: true,
            path_trace: false,
        }
    }
}

#[derive(Clone)]
pub struct Stencil {
    pub enabled: bool,
    pub resource: Option<u64>,
    /// Center in viewport points.
    pub center: Vec2,
    /// Height in points.
    pub size: f32,
    /// Degrees.
    pub rotation: f32,
    pub opacity: f32,
    pub drive_color: bool,
    pub drive_height: bool,
    pub height_strength: f32,
}

impl Default for Stencil {
    fn default() -> Self {
        Stencil {
            enabled: false,
            resource: None,
            center: Vec2::new(400.0, 300.0),
            size: 300.0,
            rotation: 0.0,
            opacity: 0.5,
            drive_color: true,
            drive_height: false,
            height_strength: 0.5,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PaintTarget {
    Content,
    MaskPaint(u64),
}

pub struct ActiveStroke {
    pub stroke: Stroke,
    pub layer_id: u64,
    pub target: PaintTarget,
    pub erase: bool,
    pub channels: [bool; 7],
    /// Painting in the 2D view.
    pub uv: bool,
    /// Selection brush: paints the set's selection, not a layer.
    pub selection: bool,
    /// History version right after this stroke's entry was pushed (its tiles
    /// get backed up while that entry is the last one).
    pub history_version: u64,
}

/// Ctrl + LMB (flow / angle) or Ctrl + RMB (size / hardness): the cursor
/// stays where the press was and only its motion changes the brush.
#[derive(Clone, Copy, Debug)]
pub struct BrushGesture {
    /// Where the cursor is held (and the brush circle drawn).
    anchor: egui::Pos2,
    /// Position the motion is measured from.
    last: egui::Pos2,
    /// Pointer position when the cursor was sent back: until the move
    /// event of that warp arrives, the old position must not count again.
    pending: Option<egui::Pos2>,
}

pub struct NewProjectDialog {
    pub path: String,
    pub res: u32,
    pub error: Option<String>,
    /// UVs: the mesh's, or generated (auto UV).
    pub auto_uv: crate::mesh::AutoUv,
}

pub struct ExportDialog {
    pub dir: String,
    pub presets: Vec<crate::export::ExportPreset>,
    pub preset: usize,
    pub opts: crate::export::ExportOptions,
    pub sets: Vec<bool>,
}

/// What an imported file is used as.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ImportKind {
    Texture,
    Alpha,
    Environment,
}

impl ImportKind {
    pub fn name(self) -> &'static str {
        match self {
            ImportKind::Texture => "Texture",
            ImportKind::Alpha => "Alpha (brush shape)",
            ImportKind::Environment => "Environment",
        }
    }

    /// Library folder of the kind.
    pub fn folder(self) -> &'static str {
        match self {
            ImportKind::Texture => "textures",
            ImportKind::Alpha => "alphas",
            ImportKind::Environment => "environments",
        }
    }
}

/// Import resources: like Painter, every import asks where the files go:
/// the project (saved in the .sprp) or the library (every project).
pub struct ImportDialog {
    pub files: Vec<(PathBuf, ImportKind)>,
    /// true: the user library; false: the open project.
    pub to_library: bool,
}

impl ImportDialog {
    pub fn new(files: Vec<PathBuf>, kind: ImportKind, to_library: bool) -> Self {
        let files = files
            .into_iter()
            .map(|f| {
                let env = f.extension().and_then(|e| e.to_str()).is_some_and(|e| crate::library::ENV_EXTS.iter().any(|x| x.eq_ignore_ascii_case(e)));
                let k = if env { ImportKind::Environment } else { kind };
                (f, k)
            })
            .collect();
        ImportDialog { files, to_library }
    }
}

pub struct BakeDialog {
    pub settings: BakeSettings,
    pub high: String,
    pub sets: Vec<bool>,
}

type BakeOutput = Result<Vec<(usize, bake::BakeResult)>, String>;

pub struct BakeJob {
    pub progress: Arc<AtomicU32>,
    pub total: u32,
    pub cancel: Arc<AtomicBool>,
    pub handle: Option<JoinHandle<BakeOutput>>,
    pub generation: u64,
}

pub struct RetroApp {
    engine: Engine,
    renderer: Arc<egui::mutex::RwLock<egui_wgpu::Renderer>>,
    vp_tex: Option<egui::TextureId>,
    uv_tex: Option<egui::TextureId>,
    vp_rect: Rect,
    uv_rect: Rect,
    doc: Option<Document>,
    generation: u64,
    camera: OrbitCamera,
    tool: Tool,
    poly_mode: PolyMode,
    brush: BrushSettings,
    mat: BrushMaterial,
    uploaded_alpha: Option<AlphaSource>,
    uploaded_stencil: Option<u64>,
    stencil: Stencil,
    stroke: Option<ActiveStroke>,
    /// Polygon fill or hide drag in progress.
    poly_drag: Option<view3d::PolyDrag>,
    /// Selection shape being drawn.
    sel_drag: Option<selection::SelDrag>,
    /// What a plain selection drag does (the context bar's mode buttons).
    sel_op: crate::engine::select::SelOp,
    /// 3D selections also take surfaces hidden behind others or facing away.
    sel_through: bool,
    /// The selection removed last (Reselect).
    last_selection: Option<(usize, Arc<crate::engine::Tex>)>,
    /// Clock of the marching ants.
    anim_clock: Instant,
    /// Magic wand and paint bucket: color tolerance (0 to 255), flood
    /// through neighbors only, sample the visible result (all layers).
    wand_tolerance: f32,
    wand_contiguous: bool,
    wand_all_layers: bool,
    /// UV seam links of a set, by (mesh, submesh, resolution).
    seams: Option<((usize, usize, u32), Arc<crate::flood::Seams>)>,
    /// A selection painting uses instead of the set's (the bucket's region).
    sel_override: Option<Arc<crate::engine::Tex>>,
    /// Gradient: 0 linear, 1 radial, 2 reflected; the line being dragged
    /// (2D view, start, end; view local points).
    gradient_kind: u8,
    grad_drag: Option<(bool, Vec2, Vec2)>,
    /// Gradient in the 3D view: every texel of the set takes it, hidden
    /// or facing away too (off: only what the camera sees).
    gradient_through: bool,
    /// Gradient stops on layer content (colors) and on masks (grays).
    grad_color: Vec<gradient::GradStop>,
    grad_mask: Vec<gradient::GradStop>,
    /// The stop selected in the editor.
    grad_sel: usize,
    /// Content gradients paint the stops' colors (off: the material's
    /// color, the stops only fade it).
    grad_stop_colors: bool,
    /// G comes back to the bucket or the gradient, whichever was used last.
    last_fill_tool: Tool,
    /// Select > Modify window: the operation and its radius (texels).
    sel_modify: Option<(crate::flood::Modify, f32)>,
    /// Polygon fill makes a selection of the faces instead of filling them.
    poly_to_selection: bool,
    /// Selection brush: adds (true) or subtracts.
    sel_brush_add: bool,
    /// Path tool: the selected path and what is dragged.
    path: path::PathEdit,
    /// Path layers: the state of their paths when last painted, by layer.
    path_painted: std::collections::HashMap<u64, u64>,
    /// Auto UV mode of the project being created.
    new_auto_uv: crate::mesh::AutoUv,
    /// The tool of the previous frame (tool changes set the tool up).
    tool_seen: Tool,
    /// Name of the last brush preset applied (the path's brush menu).
    brush_preset: String,
    /// Brush cursor: the alpha laid over the view, for its key.
    alpha_overlay: Option<(String, egui::TextureHandle)>,
    /// Starter tip file of the last brush preset and the alpha it set: a
    /// project opened later loads the tip again while the brush keeps it.
    brush_tip: Option<(String, AlphaSource)>,
    /// Select > By mesh map window: the map and its threshold.
    sel_by_map: Option<(selection::MapSel, f32)>,
    /// The soloed layer and every layer's visibility before (Alt + eye).
    solo: Option<(usize, u64, Vec<(u64, bool)>)>,
    /// The marquee and lasso shapes M and L come back to.
    last_marquee: selection::SelShape,
    last_lasso: selection::SelShape,
    /// The stroke ended with stamps still queued: end it after they paint.
    stroke_end_pending: bool,
    /// Shift + orbit: the unsnapped yaw / pitch the mouse drives.
    orbit_free: Option<(f32, f32)>,
    /// 2D view UV manipulator: drag in progress, shown, handle size (0-2).
    uv_drag: Option<uvgizmo::UvDrag>,
    uv_gizmo_show: bool,
    uv_gizmo_size: u8,
    /// Polygon fill toolbar: selection type, rectangle hit test, visible only.
    poly_select: view3d::PolySelect,
    poly_hit: view3d::PolyHit,
    poly_visible_only: bool,
    /// Polygon topology of one submesh: (mesh pointer, submesh), topology.
    poly_topo: Option<((usize, usize), Arc<polysel::PolyTopo>)>,
    pending: Vec<Stamp>,
    /// Texels under the current stroke's stamps (scissor, dirty region).
    region_finder: region::RegionFinder,
    /// Region painted by the previous flush (smudge refreshes its snapshot there).
    prev_region: Option<crate::engine::composite::Region>,
    history: History,
    baseline: Option<(u64, usize, String)>,
    last_edit: Instant,
    view: ViewSettings,
    new_dialog: Option<NewProjectDialog>,
    export_dialog: Option<ExportDialog>,
    bake_dialog: Option<BakeDialog>,
    /// The Auto UV window.
    uv_dialog: Option<autouv::UvDialog>,
    /// Textures let go during a frame that still draws them: kept until the
    /// next frame starts (freeing them earlier fails the GPU submit).
    retired: Vec<egui::TextureHandle>,
    import_dialog: Option<ImportDialog>,
    bake_job: Option<BakeJob>,
    show_view_settings: bool,
    show_set_settings: bool,
    show_prefs: bool,
    /// Single-key shortcuts (editable in the Shortcuts window) and the
    /// action waiting for its new key there.
    keymap: keymap::Keymap,
    keymap_capture: Option<keymap::Action>,
    show_log: bool,
    show_history: bool,
    shelf: shelf::Shelf,
    log: Vec<String>,
    status: String,
    title: String,
    startup: Option<PathBuf>,
    pending_res: Option<u32>,
    /// End of the last 3D stroke (shift + click draws a line from it).
    line_end: Option<Vec2>,
    /// Mask color-selection effect waiting for an ID color pick.
    id_pick: Option<u64>,
    /// Clone tool source (texture set, UV).
    clone_src: Option<(usize, [f32; 2])>,
    /// Current clone / smudge source offset in texels.
    sample_offset: [f32; 2],
    /// Smudge: UV under the cursor at the previous frame.
    smudge_prev: Option<[f32; 2]>,
    smudge_strength: f32,
    /// Layer stack thumbnails: (layer id, is mask) -> (content key, texture, egui id).
    thumbs: std::collections::HashMap<(u64, bool), (u64, crate::engine::Tex, egui::TextureId)>,
    last_thumbs: Instant,
    /// Latest pen pressure (1 when no pen is in use).
    pressure: f32,
    /// Path tracer reset key (camera, environment, view size).
    pt_key: u64,
    /// What the 3D and 2D views last showed: unchanged frames skip their
    /// GPU work (moving the mouse over a panel redraws nothing).
    view_key: (u64, u64),
    /// Back-to-front order of the alpha blended sets: what it was sorted for,
    /// and the triangle centers per submesh (mesh pointer, generation).
    sort_key: u64,
    tri_centers: Option<((usize, u64), Arc<Vec<Vec<Vec3>>>)>,
    last_autosave: Instant,
    /// Minutes between autosaves (0 = off).
    autosave_minutes: f32,
    /// Which floating panels show (None: the default arrangement).
    dock: Option<dock::Panels>,
    /// Read and write the user's panel arrangement (off in tests and benches).
    #[allow(dead_code)]
    pub(super) persist_layout: bool,
    /// Ctrl + drag brush gestures hold the system cursor in place (off in
    /// the headless tests, which have no cursor to move).
    pub(super) lock_cursor: bool,
    /// The Ctrl + drag brush gesture in progress.
    gesture: Option<BrushGesture>,
    /// The canvas between the floating panels (bars and overlays center on it).
    canvas_mid: Rect,
    /// Tool dock: the entry each group shows (its last used tool).
    dock_last: Vec<dock::DockItem>,
    /// Where the display settings (0) and history (1) flyouts hang from.
    flyout_at: [Option<egui::Pos2>; 2],
    /// The message the canvas corner shows, since when.
    status_shown: (String, Instant),
    /// Where the tool dock was drawn (the status text stays left of it).
    dock_rect: Rect,
    /// Where the tool options bar was drawn.
    options_rect: Rect,
    /// The saved panel arrangement was read (first frame); last save (when, what).
    layout_loaded: bool,
    layout_saved: (Instant, String),
    /// The Texture Set Settings tab was drawn this frame (mesh map thumbnails).
    set_settings_visible: bool,
    /// Logo texture for the start screen.
    logo_tex: Option<egui::TextureHandle>,
    /// Warning over the viewport (message, when).
    toast: Option<(String, Instant)>,
    /// Close requested with unsaved changes: the save prompt is open.
    close_prompt: bool,
    /// Save armed by Ctrl + S: path, frames its notice has been shown.
    pending_save: Option<(PathBuf, u8)>,
    /// Opening or creating a project over unsaved changes: asked first.
    discard_prompt: Option<Discard>,
    /// The user confirmed closing (saved or discarded).
    allow_close: bool,
    /// Default environment decoding in the background (path, pixels).
    env_loading: Option<std::thread::JoinHandle<Option<(PathBuf, u32, u32, Vec<[f32; 4]>)>>>,
    /// Surface point the current Alt + LMB orbit turns around.
    orbit_pivot: Option<Vec3>,
    /// Per-section frame timings (milliseconds, count) when profiling.
    pub(crate) profile: Option<std::collections::BTreeMap<&'static str, (f64, u32)>>,
    /// Edit tracking: (set, hash, state) at the end of the last frame.
    tracked: Option<(usize, u64, crate::document::SetState)>,
    /// Document generation `tracked` belongs to.
    tracked_doc: u64,
    /// History version when edits were last tracked: undo / redo / pushes
    /// since then (even outside a frame) are not edits of their own.
    tracked_history: u64,
    /// Quick properties popup position (plain right click in the viewport).
    quick_props: Option<egui::Pos2>,
    /// Tool to restore after the temporary color picker (P).
    prev_tool: Option<Tool>,
    /// Tab: hide the docks.
    ui_hidden: bool,
    /// Lazy mouse distance restored by D.
    lazy_saved: f32,
    /// Ctrl + C: (texture set, layer id).
    layer_clipboard: Option<(usize, u64)>,
    /// Layer menu "Copy channel blending parameters".
    blend_clipboard: Option<([crate::document::BlendMode; crate::document::NCH], [f32; crate::document::NCH])>,
    /// Mask menu "Copy mask": (texture set, layer id).
    mask_clipboard: Option<(usize, u64)>,
    /// Alt + Q: visibility of the sets before isolating.
    isolate_saved: Option<Vec<bool>>,
    /// Last Properties icon tab clicked.
    props_tab: usize,
    /// Brush stroke preview (cache key, texture).
    stroke_preview: Option<(String, egui::TextureHandle)>,
    /// Brush material ball (content hash, texture, egui id).
    brush_ball: Option<(u64, crate::engine::Tex, egui::TextureId)>,
    brush_ball_at: Instant,
    /// Channel whose blend mode and opacity the layer rows show.
    row_channel: usize,
    /// Fill layer previews: layer id -> (content hash, texture, egui id).
    fill_thumbs: std::collections::HashMap<u64, (u64, crate::engine::Tex, egui::TextureId)>,
    /// Mesh map thumbnails of the active set: map index -> (key, texture, egui id).
    map_thumbs: std::collections::HashMap<usize, (u64, crate::engine::Tex, egui::TextureId)>,
    /// A popup, menu or quick properties was open when the frame started:
    /// the click that dismisses it must not paint.
    overlay_open: bool,
    /// The current mouse press started on the 3D / 2D view itself (not on a
    /// widget drawn over it, nor while dismissing a popup).
    vp_press_ok: bool,
    uv_press_ok: bool,
    /// 2D view rotation drag: the rotation at the press.
    uv_rot_from: Option<f32>,
    /// Layers panel body of the last frame, and whether the last click was
    /// in it (Delete, Ctrl + C / V / D / G act on layers only then).
    layers_rect: Rect,
    layers_focus: bool,
    /// Selection before the last `Cmd::Select` (Alt + click mask toggling).
    last_select: Option<(Option<u64>, crate::document::Target)>,
    /// Projection manipulator: mode (W / E / R) and the drag in progress.
    gizmo_mode: gizmo::GizmoMode,
    gizmo_drag: Option<gizmo::GizmoDrag>,
    /// Eyedropper: press kept from egui (the pick), its release to drop,
    /// the color under the cursor, the left button state outside the window.
    eyedrop_press: Option<egui::Pos2>,
    eyedrop_swallow: bool,
    eyedrop_live: Option<[f32; 3]>,
    eyedrop_was_down: bool,
    /// Clone source on the mesh (mesh space), and during a stroke the
    /// screen offset from the brush to the point being sampled.
    clone_world: Option<Vec3>,
    clone_offset_px: Option<egui::Vec2>,
}

impl RetroApp {
    pub fn new(cc: &eframe::CreationContext, startup: Option<PathBuf>) -> anyhow::Result<Self> {
        let rs = cc
            .wgpu_render_state
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("the wgpu renderer is required"))?;
        let adapter_name = rs.adapter.get_info().name;
        let engine = Engine::new(rs.device.clone(), rs.queue.clone(), adapter_name);
        Ok(Self::build(engine, rs.renderer.clone(), &cc.egui_ctx, startup))
    }

    pub fn build(
        engine: Engine,
        renderer: Arc<egui::mutex::RwLock<egui_wgpu::Renderer>>,
        ctx: &egui::Context,
        startup: Option<PathBuf>,
    ) -> Self {
        style::apply(ctx);
        RetroApp {
            engine,
            renderer,
            vp_tex: None,
            uv_tex: None,
            vp_rect: Rect::NOTHING,
            uv_rect: Rect::NOTHING,
            doc: None,
            generation: 0,
            camera: OrbitCamera::default(),
            tool: Tool::Paint,
            poly_mode: PolyMode::Polygon,
            brush: BrushSettings::default(),
            mat: BrushMaterial::default(),
            uploaded_alpha: None,
            uploaded_stencil: None,
            stencil: Stencil::default(),
            stroke: None,
            poly_drag: None,
            sel_drag: None,
            sel_op: Default::default(),
            sel_through: true,
            last_selection: None,
            anim_clock: Instant::now(),
            wand_tolerance: 32.0,
            wand_contiguous: true,
            wand_all_layers: true,
            seams: None,
            sel_override: None,
            gradient_kind: 0,
            grad_drag: None,
            gradient_through: true,
            grad_color: gradient::default_color(BrushMaterial::default().color),
            grad_mask: gradient::default_mask(),
            grad_sel: 0,
            grad_stop_colors: true,
            last_fill_tool: Tool::Bucket,
            sel_modify: None,
            poly_to_selection: false,
            sel_brush_add: true,
            path: Default::default(),
            path_painted: Default::default(),
            new_auto_uv: Default::default(),
            uv_dialog: None,
            retired: Vec::new(),
            tool_seen: Tool::Paint,
            brush_preset: String::new(),
            alpha_overlay: None,
            brush_tip: None,
            sel_by_map: None,
            solo: None,
            last_marquee: selection::SelShape::Rect,
            last_lasso: selection::SelShape::Lasso,
            stroke_end_pending: false,
            orbit_free: None,
            uv_drag: None,
            uv_gizmo_show: true,
            uv_gizmo_size: 1,
            poly_select: view3d::PolySelect::Marquee,
            poly_hit: view3d::PolyHit::Intersection,
            poly_visible_only: true,
            poly_topo: None,
            pending: Vec::new(),
            region_finder: Default::default(),
            prev_region: None,
            history: History::default(),
            baseline: None,
            last_edit: Instant::now(),
            view: ViewSettings::default(),
            new_dialog: None,
            export_dialog: None,
            bake_dialog: None,
            import_dialog: None,
            bake_job: None,
            show_view_settings: false,
            show_set_settings: false,
            show_prefs: false,
            keymap: keymap::Keymap::load(),
            keymap_capture: None,
            show_log: false,
            show_history: false,
            shelf: shelf::Shelf::default(),
            log: Vec::new(),
            status: "Welcome. File > New project to import a mesh (OBJ, FBX, glTF/GLB).".into(),
            title: String::new(),
            startup,
            pending_res: None,
            line_end: None,
            id_pick: None,
            clone_src: None,
            sample_offset: [0.0; 2],
            smudge_prev: None,
            smudge_strength: 0.6,
            thumbs: Default::default(),
            last_thumbs: Instant::now(),
            pressure: 1.0,
            pt_key: 0,
            view_key: (0, 0),
            sort_key: 0,
            tri_centers: None,
            last_autosave: Instant::now(),
            autosave_minutes: 10.0,
            dock: None,
            persist_layout: true,
            lock_cursor: true,
            gesture: None,
            canvas_mid: Rect::NOTHING,
            dock_rect: Rect::NOTHING,
            options_rect: Rect::NOTHING,
            layout_loaded: false,
            layout_saved: (Instant::now(), String::new()),
            dock_last: Vec::new(),
            flyout_at: [None; 2],
            status_shown: (String::new(), Instant::now()),
            set_settings_visible: false,
            logo_tex: None,
            toast: None,
            close_prompt: false,
            pending_save: None,
            discard_prompt: None,
            allow_close: false,
            // The default environment, decoded off the UI thread.
            env_loading: Some(std::thread::spawn(|| {
                let p = crate::library::default_environment()?;
                let (w, h, px) = crate::engine::env::load_hdr(&p).ok()?;
                Some((p, w, h, px))
            })),
            orbit_pivot: None,
            profile: None,
            tracked: None,
            tracked_doc: 0,
            tracked_history: 0,
            quick_props: None,
            prev_tool: None,
            ui_hidden: false,
            lazy_saved: 30.0,
            layer_clipboard: None,
            mask_clipboard: None,
            blend_clipboard: None,
            isolate_saved: None,
            props_tab: 0,
            stroke_preview: None,
            brush_ball: None,
            brush_ball_at: Instant::now(),
            row_channel: 0,
            fill_thumbs: Default::default(),
            map_thumbs: Default::default(),
            overlay_open: false,
            vp_press_ok: true,
            uv_press_ok: true,
            uv_rot_from: None,
            layers_rect: Rect::NOTHING,
            layers_focus: false,
            last_select: None,
            gizmo_mode: Default::default(),
            gizmo_drag: None,
            eyedrop_press: None,
            eyedrop_swallow: false,
            eyedrop_live: None,
            eyedrop_was_down: false,
            clone_world: None,
            clone_offset_px: None,
        }
    }

    fn message(&mut self, msg: impl Into<String>) {
        let msg = msg.into();
        log::info!("{msg}");
        self.status = msg.clone();
        self.log.push(msg);
    }

    // ------------------------------------------------------------ project

    fn create_project(&mut self, path: PathBuf, res: u32) -> anyhow::Result<()> {
        let mesh = mesh::load_with(&path, self.new_auto_uv)?;
        let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("mesh").to_string();
        let notes = mesh.notes.clone();
        let (min, max) = (mesh.bbox_min, mesh.bbox_max);
        let tris = mesh.triangle_count();
        let sets = mesh.submeshes.len();
        let doc = Document::new(&self.engine, mesh, name.clone(), res);
        let empty: Vec<String> =
            doc.sets.iter().filter(|s| !s.raster.tri.iter().any(|t| *t >= 0)).map(|s| s.name.clone()).collect();
        self.install_doc(doc);
        self.camera = OrbitCamera::default();
        self.camera.frame(min, max);
        self.frame_mesh();
        for n in notes {
            self.message(n);
        }
        for e in empty {
            self.message(format!("Texture set '{e}' has degenerate UVs (zero area): it cannot be painted."));
        }
        self.message(format!("Imported '{name}': {tris} triangles, {sets} texture set(s) at {res}x{res}"));
        Ok(())
    }

    fn install_doc(&mut self, doc: Document) {
        self.doc = Some(doc);
        self.reset_doc_state();
        self.layer_clipboard = None;
        self.mask_clipboard = None;
        // Resource ids restart per document: forget the previous one's.
        self.shelf.forget_project();
        self.path_reset();
        self.path_painted.clear();
        let tip = self.brush_tip.clone().filter(|(_, src)| *src == self.brush.alpha).map(|(f, _)| f);
        if matches!(self.brush.alpha, AlphaSource::Resource(_)) {
            self.brush.alpha = AlphaSource::Builtin(crate::brush::AlphaKind::Round);
        }
        // The brush preset's tip comes back in the new project.
        if let (Some(file), Some(doc)) = (tip, self.doc.as_mut()) {
            if let Some(Ok(id)) = crate::library::starter_alpha(&file).map(|p| doc.import_resource(&self.engine, &p, crate::document::ResKind::Alpha)) {
                self.brush.alpha = AlphaSource::Resource(id);
                self.brush_tip = Some((file, self.brush.alpha.clone()));
            }
        }
        self.uploaded_alpha = None;
    }

    /// Everything that indexes the document's sets or mesh is dropped
    /// (new document, or a reimported mesh).
    fn reset_doc_state(&mut self) {
        // Dialogs and picks of the previous document index its sets.
        self.export_dialog = None;
        self.bake_dialog = None;
        self.close_uv_dialog();
        self.id_pick = None;
        self.isolate_saved = None;
        self.clone_src = None;
        self.quick_props = None;
        self.poly_drag = None;
        self.sel_drag = None;
        self.last_selection = None;
        self.seams = None;
        self.solo = None;
        self.poly_topo = None;
        self.generation += 1;
        self.history.clear();
        self.baseline = None;
        self.stroke = None;
        self.pending.clear();
        self.uploaded_stencil = None;
        self.stencil.resource = None;
        if let Some(job) = &self.bake_job {
            job.cancel.store(true, Ordering::Relaxed);
        }
        self.engine.compositor.trim();
        self.engine.procgen.trim();
    }

    /// Opens a `.sprp` project, or starts a new project from a mesh file.
    /// True (and the save prompt armed) when unsaved changes would be lost.
    fn ask_discard(&mut self, then: Discard) -> bool {
        let modified = self.doc.as_ref().is_some_and(|d| d.modified);
        if modified && !HEADLESS.load(Ordering::Relaxed) {
            self.discard_prompt = Some(then);
            return true;
        }
        false
    }

    fn open_path(&mut self, p: &std::path::Path) {
        let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
        if ext == "sprp" {
            if self.ask_discard(Discard::Open(p.to_path_buf())) {
                return;
            }
            self.open_project_file(p);
        } else if crate::library::ENV_EXTS.iter().any(|e| *e == ext) {
            self.load_environment(p);
        } else if crate::library::IMAGE_EXTS.iter().any(|e| *e == ext) {
            // Images dropped on the window: the import dialog asks where
            // they go (several dropped files share one dialog).
            self.ask_import(vec![p.to_path_buf()], ImportKind::Texture);
        } else {
            self.new_dialog = Some(NewProjectDialog { path: p.display().to_string(), res: 2048, error: None, auto_uv: Default::default() });
        }
    }

    fn open_project_file(&mut self, p: &std::path::Path) {
        {
            match project::load(&self.engine, p) {
                Ok((doc, cam)) => {
                    self.install_doc(doc);
                    self.camera = cam;
                    crate::library::add_recent(p);
                    self.message(format!("Opened {}", p.display()));
                }
                Err(e) => self.message(format!("Could not open project: {e:#}")),
            }
        }
    }

    fn open_project(&mut self) {
        if let Some(path) = file_dialog().and_then(|f| f.add_filter("Material Forge project", &["sprp"]).pick_file()) {
            self.open_path(&path);
        }
    }

    /// Save from the menu or Ctrl + S: a "Saving" notice is drawn first,
    /// the (blocking) save runs on the next frame, then a "Saved" toast.
    fn save_project(&mut self, save_as: bool) {
        let Some(path) = self.save_path(save_as) else { return };
        self.pending_save = Some((path, 0));
    }

    /// Where to save: the project's file, or the Save As dialog.
    fn save_path(&mut self, save_as: bool) -> Option<PathBuf> {
        let doc = self.doc.as_ref()?;
        match (&doc.path, save_as) {
            (Some(p), false) => Some(p.clone()),
            _ => file_dialog().and_then(|f| f.add_filter("Material Forge project", &["sprp"]).set_file_name(format!("{}.sprp", doc.mesh_name)).save_file()),
        }
    }

    /// Runs a save armed by `save_project` once its notice was shown.
    fn run_pending_save(&mut self, ctx: &egui::Context) {
        let Some((path, frames)) = self.pending_save.take() else { return };
        if frames < 1 {
            self.pending_save = Some((path, frames + 1));
            ctx.request_repaint();
            return;
        }
        let t = Instant::now();
        self.save_to(&path);
        if !self.doc.as_ref().is_some_and(|d| d.modified) {
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let size = std::fs::metadata(&path).map(|m| m.len() as f64 / 1e6).unwrap_or(0.0);
            self.toast(format!("Saved {name} ({size:.1} MB, {:.1} s)", t.elapsed().as_secs_f32()));
        }
    }

    /// "Saving <name>..." over the window while a save is armed or running.
    fn saving_notice(&self, ctx: &egui::Context) {
        let Some((path, _)) = &self.pending_save else { return };
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        egui::Area::new(egui::Id::new("saving notice"))
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .interactable(false)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).fill(style::PANEL).inner_margin(egui::Margin::symmetric(18.0, 12.0)).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.add(egui::Spinner::new().size(18.0).color(style::ACCENT));
                        ui.label(egui::RichText::new(format!("Saving {name}...")).size(13.0).color(egui::Color32::from_gray(230)));
                    });
                });
            });
    }

    /// Saves synchronously (the unsaved-changes prompts wait for it).
    fn save_project_now(&mut self, save_as: bool) {
        if let Some(path) = self.save_path(save_as) {
            self.save_to(&path);
        }
    }

    fn save_to(&mut self, path: &std::path::Path) {
        let Some(doc) = &self.doc else { return };
        let path = path.to_path_buf();
        match project::save(&self.engine, doc, &self.camera, &path) {
            Ok(()) => {
                if let Some(doc) = &mut self.doc {
                    doc.path = Some(path.clone());
                    doc.modified = false;
                }
                crate::library::add_recent(&path);
                self.message(format!("Saved {}", path.display()));
            }
            Err(e) => self.message(format!("Save failed: {e:#}")),
        }
    }

    fn reimport_mesh(&mut self) {
        let Some(path) = file_dialog().and_then(|f| f.add_filter("Mesh", &["obj", "fbx", "gltf", "glb"]).pick_file()) else { return };
        let Some(doc) = &mut self.doc else { return };
        match mesh::load(&path) {
            Ok(m) => {
                let notes = doc.reimport(&self.engine, m);
                self.reset_doc_state();
                for n in notes {
                    self.message(n);
                }
                self.message(format!("Mesh reimported from {} (re-bake mesh maps if the shape changed)", path.display()));
            }
            Err(e) => self.message(format!("Reimport failed: {e:#}")),
        }
    }

    /// File > Export mesh: the mesh as OBJ with the project's UVs (the
    /// generated ones after auto UV).
    pub(super) fn export_mesh(&mut self) {
        let Some(doc) = &self.doc else { return };
        let Some(path) = file_dialog().and_then(|f| f.add_filter("OBJ", &["obj"]).set_file_name(format!("{}.obj", doc.mesh_name)).save_file()) else { return };
        match mesh::write_obj(&doc.mesh, &path) {
            Ok(()) => self.message(format!("Mesh exported to {}", path.display())),
            Err(e) => self.message(format!("Mesh export failed: {e:#}")),
        }
    }

    fn load_environment(&mut self, p: &std::path::Path) {
        match crate::engine::env::load_hdr(p) {
            Ok((w, h, px)) => {
                let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                let e = crate::engine::env::Environment::build(&self.engine.device, &self.engine.queue, w, h, px, name);
                self.engine.set_environment(e);
                self.message(format!("Environment loaded: {}", p.display()));
            }
            Err(e) => self.message(format!("Could not load environment: {e:#}")),
        }
    }

    // ------------------------------------------------------------ history

    /// Records the active set's current state before a structural change.
    fn checkpoint(&mut self, label: &str) {
        let Some(doc) = &self.doc else { return };
        let set = doc.active;
        let state = doc.sets[set].state();
        self.history.push(Entry::State { label: label.into(), set, state });
        self.baseline = None;
        if let Some(doc) = &mut self.doc {
            doc.modified = true;
        }
    }

    /// Detects property edits made through the UI since the last frame and
    /// records them (rapid slider drags are merged into one entry).
    /// Status message also shown over the viewport.
    pub(super) fn toast(&mut self, msg: String) {
        self.status = msg.clone();
        self.toast = Some((msg, Instant::now()));
    }

    /// Window close: ask to save when the project has unsaved changes.
    fn handle_close(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| i.viewport().close_requested()) && !self.allow_close {
            if self.doc.as_ref().is_some_and(|d| d.modified) {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.close_prompt = true;
            }
        }
        if self.discard_prompt.is_some() {
            self.discard_window(ctx);
        }
        if !self.close_prompt {
            return;
        }
        let name = self.doc.as_ref().map(|d| d.path.as_ref().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| d.mesh_name.clone())).unwrap_or_default();
        let mut choice = None;
        egui::Window::new(egui::RichText::new("MATERIAL FORGE").font(style::bold(11.5)))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.set_width(360.0);
                ui.add_space(4.0);
                ui.label(egui::RichText::new(format!("Save the changes to \"{name}\" before closing?")).size(13.0));
                ui.label(egui::RichText::new("Your changes will be lost if you don't save them.").size(11.5).color(egui::Color32::from_gray(160)));
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui.add(egui::Button::new("Save").fill(style::ACCENT).min_size(egui::vec2(90.0, 26.0))).clicked() {
                        choice = Some(0);
                    }
                    if ui.add(egui::Button::new("Don't save").min_size(egui::vec2(90.0, 26.0))).clicked() {
                        choice = Some(1);
                    }
                    if ui.add(egui::Button::new("Cancel").min_size(egui::vec2(90.0, 26.0))).clicked() {
                        choice = Some(2);
                    }
                });
            });
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            choice = Some(2);
        }
        match choice {
            Some(0) => {
                self.save_project_now(false);
                // Saved (a cancelled Save As dialog leaves it modified).
                if !self.doc.as_ref().is_some_and(|d| d.modified) {
                    self.allow_close = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                self.close_prompt = false;
            }
            Some(1) => {
                self.allow_close = true;
                self.close_prompt = false;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            Some(_) => self.close_prompt = false,
            None => {}
        }
    }

    /// "Save the changes?" before opening or creating another project.
    fn discard_window(&mut self, ctx: &egui::Context) {
        let name = self.doc.as_ref().map(|d| d.path.as_ref().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| d.mesh_name.clone())).unwrap_or_default();
        let mut choice = None;
        egui::Window::new(egui::RichText::new("MATERIAL FORGE").font(style::bold(11.5)))
            .id(egui::Id::new("discard prompt"))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.set_width(360.0);
                ui.add_space(4.0);
                ui.label(egui::RichText::new(format!("Save the changes to \"{name}\" first?")).size(13.0));
                ui.label(egui::RichText::new("Your changes will be lost if you don't save them.").size(11.5).color(egui::Color32::from_gray(160)));
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui.add(egui::Button::new("Save").fill(style::ACCENT).min_size(egui::vec2(90.0, 26.0))).clicked() {
                        choice = Some(0);
                    }
                    if ui.add(egui::Button::new("Don't save").min_size(egui::vec2(90.0, 26.0))).clicked() {
                        choice = Some(1);
                    }
                    if ui.add(egui::Button::new("Cancel").min_size(egui::vec2(90.0, 26.0))).clicked() {
                        choice = Some(2);
                    }
                });
            });
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            choice = Some(2);
        }
        let go = match choice {
            Some(0) => {
                self.save_project_now(false);
                !self.doc.as_ref().is_some_and(|d| d.modified)
            }
            Some(1) => true,
            Some(_) => {
                self.discard_prompt = None;
                false
            }
            None => false,
        };
        if go {
            match self.discard_prompt.take() {
                Some(Discard::Open(p)) => self.open_project_file(&p),
                Some(Discard::Create(p, res)) => {
                    if let Err(e) = self.create_project(p.clone(), res) {
                        self.message(format!("Could not import {}: {e:#}", p.display()));
                    }
                }
                None => {}
            }
        }
    }

    fn prof(&mut self, name: &'static str, t: Instant) {
        if let Some(p) = &mut self.profile {
            let e = p.entry(name).or_insert((0.0, 0));
            e.0 += t.elapsed().as_secs_f64() * 1000.0;
            e.1 += 1;
        }
    }

    /// Records property edits made through the panels as undo steps. The
    /// state of the active set at the end of the previous frame is kept with
    /// a hash; only a changed hash costs a snapshot. Frames that pushed their
    /// own history entry (commands, undo, redo) just re-baseline.
    fn track_edits(&mut self, history_len_before: u64) {
        let Some(doc) = &mut self.doc else {
            self.tracked = None;
            return;
        };
        let set = doc.active;
        let hash = doc.sets[set].fingerprint_hash();
        let explicit = self.history.version != history_len_before || self.history.version != self.tracked_history;
        match self.tracked.take() {
            Some((s, h, state)) if s == set && h != hash && !explicit && self.tracked_doc == self.generation => {
                doc.modified = true;
                doc.sets[set].dirty = true;
                // A path drag is one step: the frames after its first
                // change go to the step it pushed.
                let dragging_path = self.path.drag.is_some();
                let merge = matches!(self.history.undo.last(), Some(Entry::State { label, set: s, .. })
                    if *s == set && ((label == "Edit properties" && self.last_edit.elapsed().as_secs_f32() < 0.8) || (label == "Edit path" && dragging_path)));
                if !merge {
                    self.history.push(Entry::State { label: "Edit properties".into(), set, state });
                }
                self.last_edit = Instant::now();
                self.tracked = Some((set, hash, doc.sets[set].state()));
            }
            Some((s, h, state)) if s == set && h == hash && self.tracked_doc == self.generation => {
                // Selection changes are not edits, but an undo restores the
                // selection of its time: keep the snapshot's selection current.
                // After a history change (stroke, fill, undo) the textures
                // may be new Arcs with the same hash: snapshot them again, or a
                // later property edit would push the old ones.
                let s_now = &doc.sets[set];
                if explicit || state.selected != s_now.selected || state.target != s_now.target {
                    self.tracked = Some((s, h, s_now.state()));
                } else {
                    self.tracked = Some((s, h, state));
                }
            }
            _ => {
                self.tracked = Some((set, hash, doc.sets[set].state()));
            }
        }
        self.tracked_doc = self.generation;
        self.tracked_history = self.history.version;
    }

    fn undo(&mut self) {
        if let Some(doc) = &mut self.doc {
            if let Some(l) = self.history.undo(&self.engine, doc) {
                self.status = format!("Undo: {l}");
            }
        }
    }

    fn redo(&mut self) {
        if let Some(doc) = &mut self.doc {
            if let Some(l) = self.history.redo(&self.engine, doc) {
                self.status = format!("Redo: {l}");
            }
        }
    }

    // ------------------------------------------------------------ baking

    fn start_bake(&mut self, settings: BakeSettings, which: Vec<bool>) {
        let Some(doc) = &mut self.doc else { return };
        doc.bake_settings = settings.clone();
        let mesh = doc.mesh.clone();
        let sets: Vec<(usize, usize, u32)> = doc
            .sets
            .iter()
            .enumerate()
            .filter(|(i, _)| which.get(*i).copied().unwrap_or(true))
            .map(|(i, s)| (i, s.submesh, s.resolution()))
            .collect();
        let total = sets.iter().map(|s| bake::bake_rows(s.2, &settings)).sum();
        let progress = Arc::new(AtomicU32::new(0));
        let cancel = Arc::new(AtomicBool::new(false));
        let (p, c) = (progress.clone(), cancel.clone());
        let (device, queue) = (self.engine.device.clone(), self.engine.queue.clone());
        let handle = std::thread::spawn(move || -> BakeOutput {
            let high = match &settings.high_poly {
                Some(path) => Some(HighPoly::new(mesh::load_with(path, mesh::AutoUv::Skip).map_err(|e| format!("High poly: {e:#}"))?)),
                None => None,
            };
            let tris = bake::mesh_triangles(&mesh);
            let bvh = bake::build_bvh(&mesh, &tris);
            // Rays on the GPU against whichever mesh occludes (high poly when given).
            let gpu = crate::engine::gpubake::GpuBaker::new(device, queue, high.as_ref().map(|h| &h.bvh).unwrap_or(&bvh));
            let mut out = Vec::new();
            for (i, sub, res) in sets {
                if c.load(Ordering::Relaxed) {
                    return Err("Bake cancelled".into());
                }
                out.push((i, bake::bake(&mesh, sub, res, &bvh, high.as_ref(), &settings, &p, &c, Some(&gpu))));
            }
            if c.load(Ordering::Relaxed) {
                return Err("Bake cancelled".into());
            }
            Ok(out)
        });
        self.bake_job = Some(BakeJob { progress, total, cancel, handle: Some(handle), generation: self.generation });
        self.message("Baking mesh maps...");
    }

    fn poll_bake(&mut self) {
        let finished = self.bake_job.as_ref().and_then(|j| j.handle.as_ref()).map(|h| h.is_finished()).unwrap_or(false);
        if !finished {
            return;
        }
        let mut job = self.bake_job.take().unwrap();
        let result = job.handle.take().unwrap().join().unwrap_or_else(|_| Err("Baker crashed".into()));
        if job.generation != self.generation {
            return;
        }
        match result {
            Ok(results) => {
                let Some(doc) = &mut self.doc else { return };
                for (i, r) in results {
                    apply_bake(&self.engine, &mut doc.sets[i], &r);
                }
                doc.modified = true;
                self.message("Bake finished: AO, curvature, normal, world normal, thickness and ID maps are ready.");
            }
            Err(e) => self.message(e),
        }
    }

    // ------------------------------------------------------------ GPU frame

    fn key_light(&self) -> (Vec3, Mat4) {
        let sh = &self.engine.env.sh;
        let lum = |c: &[f32; 4]| 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
        // L1 band: sh[1] ~ y, sh[2] ~ z, sh[3] ~ x (environment space).
        let d_env = Vec3::new(lum(&sh[3]), lum(&sh[1]), lum(&sh[2])).normalize_or(Vec3::Y);
        let a = -self.view.env_rotation.to_radians();
        let (s, c) = a.sin_cos();
        let mut dir = Vec3::new(c * d_env.x + s * d_env.z, d_env.y, -s * d_env.x + c * d_env.z).normalize_or(Vec3::Y);
        if dir.y < 0.2 {
            dir = Vec3::new(dir.x, 0.2, dir.z).normalize();
        }
        let (min, max) = self.doc.as_ref().map(|d| (d.mesh.bbox_min, d.mesh.bbox_max)).unwrap_or((Vec3::splat(-1.0), Vec3::splat(1.0)));
        let center = (min + max) * 0.5;
        let r = ((max - min).length() * 0.5).max(1e-3);
        let eye = center + dir * r * 2.0;
        let up = if dir.y.abs() > 0.95 { Vec3::Z } else { Vec3::Y };
        let view = Mat4::look_at_rh(eye, center, up);
        let proj = Mat4::orthographic_rh(-r, r, -r, r, 0.01 * r, 4.0 * r);
        (dir, proj * view)
    }

    fn gpu_frame(&mut self) {
        let mut encoder = self.engine.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("frame") });
        self.flush_stamps(&mut encoder);
        if self.stroke_end_pending {
            self.end_stroke();
        }
        let capture = self.doc.as_ref().and_then(|d| d.sets[d.active].selected);
        let stroke_layer = self.stroke.as_ref().map(|s| s.layer_id);
        if self.stroke.is_some() {
            self.view.path_trace = false;
        }
        let mut composited = false;
        let show_mask = self.view.display == Display::LayerMask;
        if let Some(doc) = &mut self.doc {
            let a = doc.active;
            if show_mask && doc.sets[a].gpu.ensure_mask_view(&self.engine.device) {
                doc.sets[a].dirty = true;
            }
            composited = doc.sets.iter().any(|s| s.dirty || s.dirty_region.is_some());
            // Cache the stack below the layer being edited (painted or selected).
            for (i, s) in doc.sets.iter_mut().enumerate() {
                s.focus = if i == doc.active { stroke_layer.or(s.selected) } else { None };
            }
            let resources = &doc.resources;
            for s in doc.sets.iter_mut().filter(|s| s.dirty || s.dirty_region.is_some()) {
                s.composite(&self.engine, &mut encoder, resources, capture);
            }
        }
        self.sort_blended();
        let vp = &self.engine.viewport;
        let aspect = vp.width as f32 / vp.height as f32;
        let view_proj = self.camera.view_proj(aspect);
        let (light_dir, light_vp) = self.key_light();
        let env = &self.engine.env;
        let v = &self.view;
        let (disp, disp_kind) = v.display.code();
        let raw = disp > 0.5;
        let diag = self.doc.as_ref().map(|d| d.mesh.diagonal()).unwrap_or(1.0);
        let camera = CameraUniform {
            view_proj: view_proj.to_cols_array_2d(),
            inv_view_proj: view_proj.inverse().to_cols_array_2d(),
            eye: self.camera.eye().extend(diag).into(),
            env: [v.env_rotation.to_radians(), 2f32.powf(v.exposure), v.bg_blur, (env.mip_count as f32 - 3.0).max(1.0)],
            display: [disp, if v.flat_bg { 1.0 } else { 0.0 }, if v.shadows { v.shadow_strength } else { 0.0 }, v.bg_intensity],
            sh: env.sh,
            light_view_proj: light_vp.to_cols_array_2d(),
            light: [light_dir.x, light_dir.y, light_dir.z, v.light_intensity],
            wire: [v.wire_color[0], v.wire_color[1], v.wire_color[2], v.wire_opacity],
        };
        // Exposure is applied in the post pass; the mesh shader outputs radiance.
        let post = PostUniform {
            a: [2f32.powf(v.exposure), v.tonemap as f32, v.bloom, v.bloom_threshold],
            b: [v.vignette, v.saturation, v.contrast, if raw { 1.0 } else { 0.0 }],
            texel: [0.0; 4],
        };
        let sets: Vec<DrawSet> = match &self.doc {
            Some(doc) => doc
                .sets
                .iter()
                .map(|s| DrawSet {
                    submesh: s.submesh,
                    base: &s.gpu.final_base,
                    orm: &s.gpu.final_orm,
                    normal: &s.gpu.final_normal,
                    emissive: &s.gpu.final_emissive,
                    display: display_texture(s, v.display),
                    params: SetUniform {
                        k: [
                            s.shader_alpha(),
                            s.settings.alpha_cutoff,
                            s.settings.emissive_intensity,
                            disp_kind,
                        ],
                    },
                    visible: s.settings.visible,
                    // On by default: the refraction passes only run once a layer uses it.
                    transmissive: s.uses_ext(1),
                })
                .collect(),
            None => Vec::new(),
        };
        let pt = v.path_trace && self.doc.is_some() && v.display == Display::Material;
        if pt {
            let doc = self.doc.as_ref().unwrap();
            if self.engine.pathtracer.scene != self.generation {
                self.engine.pathtracer.build_scene(&self.engine.device, doc, self.generation);
            }
            let key = {
                use std::hash::{Hash, Hasher};
                let mut h = std::collections::hash_map::DefaultHasher::new();
                for f in view_proj.to_cols_array() {
                    f.to_bits().hash(&mut h);
                }
                (v.env_rotation.to_bits(), v.flat_bg, v.bg_blur.to_bits(), v.bg_intensity.to_bits(), env.name.as_str()).hash(&mut h);
                (vp.width, vp.height, self.engine.pathtracer.bounces).hash(&mut h);
                h.finish()
            };
            if key != self.pt_key || composited {
                self.pt_key = key;
                self.engine.pathtracer.reset();
            }
            if !self.engine.pathtracer.done() {
                let mut emi = [0.0f32; 8];
                let (mut am, mut cut) = ([0.0f32; 8], [0.5f32; 8]);
                let slots: Vec<[&crate::engine::Tex; 4]> = doc
                    .sets
                    .iter()
                    .take(crate::engine::pathtrace::MAX_SETS)
                    .enumerate()
                    .map(|(i, s)| {
                        emi[i] = s.settings.emissive_intensity;
                        am[i] = s.shader_alpha();
                        cut[i] = s.settings.alpha_cutoff;
                        [&s.gpu.final_base, &s.gpu.final_orm, &s.gpu.final_normal, &s.gpu.final_emissive]
                    })
                    .collect();
                let u = crate::engine::pathtrace::PtUniform {
                    inv_view_proj: view_proj.inverse().to_cols_array_2d(),
                    eye: self.camera.eye().extend(diag).into(),
                    size: [0; 4],
                    env: [v.env_rotation.to_radians(), v.bg_intensity, if v.flat_bg { 1.0 } else { 0.0 }, v.bg_blur],
                    emi_a: [emi[0], emi[1], emi[2], emi[3]],
                    emi_b: [emi[4], emi[5], emi[6], emi[7]],
                    amode_a: [am[0], am[1], am[2], am[3]],
                    amode_b: [am[4], am[5], am[6], am[7]],
                    cut_a: [cut[0], cut[1], cut[2], cut[3]],
                    cut_b: [cut[4], cut[5], cut[6], cut[7]],
                };
                let hdr = self.engine.viewport.hdr();
                self.engine.pathtracer.render(&self.engine.device, &mut encoder, u, &slots, &self.engine.env.view, hdr);
            }
        }
        // The denoised image (Open Image Denoise) over the path traced one.
        let denoised = pt && self.engine.pathtracer.present(&self.engine.device, &mut encoder, self.engine.viewport.hdr());
        let wire_only = self.doc.as_ref().filter(|_| !pt && matches!(self.tool, Tool::PolygonFill | Tool::Hide)).map(|d| d.sets[d.active].submesh);
        let key_3d = {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            bytemuck::bytes_of(&camera).hash(&mut h);
            bytemuck::bytes_of(&post).hash(&mut h);
            for s in &sets {
                bytemuck::bytes_of(&s.params).hash(&mut h);
                (s.visible, s.display as *const crate::engine::Tex as usize, s.base as *const crate::engine::Tex as usize).hash(&mut h);
            }
            (vp.width, vp.height, v.shadows, v.wireframe, wire_only, pt, self.generation, env.name.as_str(), env.mip_count).hash(&mut h);
            (v.wire_color.map(f32::to_bits), v.wire_opacity.to_bits()).hash(&mut h);
            self.doc.as_ref().map(|d| d.gpu_mesh.vis_rev).hash(&mut h);
            h.finish()
        };
        // Marching ants: the dashes move a pixel every 1/16 s.
        let ants_phase = (self.anim_clock.elapsed().as_secs_f32() * 16.0).floor() % 10.0;
        let sel_overlay = self.doc.as_ref().filter(|_| !pt).and_then(|d| {
            let s = &d.sets[d.active];
            let sel = s.selection.as_ref()?;
            Some(crate::engine::select::SelOverlay { submesh: s.submesh, selection: sel, nearest: &s.gpu.nearest, phase: ants_phase })
        });
        let key_3d = {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            key_3d.hash(&mut h);
            sel_overlay.as_ref().map(|o| (o.selection as *const crate::engine::Tex as usize, o.phase.to_bits())).hash(&mut h);
            h.finish()
        };
        let pt_running = pt && (!self.engine.pathtracer.done() || denoised);
        let render_3d = composited || pt_running || key_3d != self.view_key.0;
        self.view_key.0 = key_3d;
        let frame = Frame {
            camera,
            post,
            mesh: self.doc.as_ref().map(|d| &d.gpu_mesh),
            sets,
            shadows: v.shadows,
            wireframe: v.wireframe && !pt,
            wire_only,
            selection: sel_overlay,
            path_traced: pt,
        };
        if render_3d {
            self.engine.viewport.render(&self.engine.device, &self.engine.queue, &mut encoder, &frame);
        }
        drop(frame);

        let key_2d = {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            (self.view.layout as u8, Display::ALL.iter().position(|d| *d == self.view.display_2d).unwrap_or(0) as u8, self.view.uv_wire.to_bits(), self.view.uv_zoom.to_bits(), self.view.uv_pan[0].to_bits(), self.view.uv_pan[1].to_bits(), self.view.uv_rot.to_bits()).hash(&mut h);
            self.view.wire_color.map(f32::to_bits).hash(&mut h);
            (self.generation, self.doc.as_ref().map(|d| d.active)).hash(&mut h);
            (self.engine.uvview.width, self.engine.uvview.height).hash(&mut h);
            self.doc.as_ref().and_then(|d| d.sets[d.active].selection.as_ref()).map(|t| (Arc::as_ptr(t) as usize, ants_phase.to_bits())).hash(&mut h);
            h.finish()
        };
        let render_2d = composited || key_2d != self.view_key.1;
        self.view_key.1 = key_2d;
        if self.view.layout != Layout::Only3D && render_2d {
            if let Some(doc) = &self.doc {
                let s = &doc.sets[doc.active];
                let (tex, mode) = uv_texture(s, self.view.display_2d);
                self.engine.uvview.render(
                    &self.engine.device,
                    &mut encoder,
                    tex,
                    mode,
                    self.view.uv_zoom,
                    self.view.uv_pan,
                    self.view.uv_rot,
                    Some((&doc.gpu_mesh, s.submesh)),
                    [self.view.wire_color[0], self.view.wire_color[1], self.view.wire_color[2], self.view.uv_wire],
                    s.selection
                        .as_ref()
                        .map(|t| crate::engine::select::SelOverlay { submesh: s.submesh, selection: t, nearest: &s.gpu.nearest, phase: ants_phase })
                        .as_ref(),
                );
            }
        }
        self.engine.queue.submit(Some(encoder.finish()));
        if pt {
            self.engine.pathtracer.after_submit(&self.engine.device, &self.engine.queue);
        }
    }

    /// Sorts the triangles of alpha blended sets back to front when the
    /// camera, the visible geometry or the blended sets change.
    fn sort_blended(&mut self) {
        let Some(doc) = &self.doc else { return };
        let blended: Vec<usize> = doc.sets.iter().filter(|s| s.settings.visible && s.shader_alpha() > 1.5).map(|s| s.submesh).collect();
        let vp = &self.engine.viewport;
        let view_proj = self.camera.view_proj(vp.width as f32 / vp.height as f32);
        let key = {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            bytemuck::bytes_of(&view_proj.to_cols_array()).hash(&mut h);
            (&blended, doc.gpu_mesh.vis_rev, self.generation, Arc::as_ptr(&doc.mesh) as usize).hash(&mut h);
            h.finish()
        };
        if key == self.sort_key {
            return;
        }
        self.sort_key = key;
        let ck = (Arc::as_ptr(&doc.mesh) as usize, self.generation);
        let centers = match &self.tri_centers {
            Some((k, c)) if *k == ck => c.clone(),
            _ => {
                let c = Arc::new(crate::document::tri_centers(&doc.mesh));
                self.tri_centers = Some((ck, c.clone()));
                c
            }
        };
        let eye = self.camera.eye();
        let fwd = (Vec3::from(self.camera.target) - eye).normalize_or(Vec3::NEG_Z);
        let ortho = self.camera.ortho;
        let doc = self.doc.as_mut().unwrap();
        crate::document::sort_blended(doc, &self.engine, &centers, &blended, eye, fwd, ortho);
    }

    /// Saves a recovery copy of a modified project every few minutes to
    /// %APPDATA%\SP_RETRO\autosave (the project file itself is untouched).
    fn autosave(&mut self) {
        if self.autosave_minutes <= 0.0 || self.stroke.is_some() || self.bake_job.is_some() {
            return;
        }
        if self.last_autosave.elapsed().as_secs_f32() < self.autosave_minutes * 60.0 {
            return;
        }
        self.last_autosave = Instant::now();
        let Some(doc) = &self.doc else { return };
        if !doc.modified {
            return;
        }
        let dir = crate::library::library_dir().parent().map(|p| p.join("autosave")).unwrap_or_else(|| PathBuf::from("autosave"));
        let _ = std::fs::create_dir_all(&dir);
        let name: String = doc.mesh_name.chars().map(|c| if c.is_alphanumeric() || c == '_' || c == '-' { c } else { '_' }).collect();
        let path = dir.join(format!("{name}_autosave.sprp"));
        match project::save(&self.engine, doc, &self.camera, &path) {
            Ok(()) => self.status = format!("Autosaved to {}", path.display()),
            Err(e) => self.status = format!("Autosave failed: {e:#}"),
        }
    }

    /// Refreshes the layer stack thumbnails of the active set (throttled).
    fn open_bake_dialog(&mut self) {
        let Some(d) = self.doc.as_ref() else { return };
        if self.bake_job.is_some() {
            return;
        }
        let settings = d.bake_settings.clone();
        let high = settings.high_poly.as_ref().map(|p| p.display().to_string()).unwrap_or_default();
        self.bake_dialog = Some(BakeDialog { settings, high, sets: vec![true; d.sets.len()] });
    }

    /// Material previews for fill layers and mesh map thumbnails.
    /// The viewport look material previews copy.
    pub(super) fn preview_look(&self) -> crate::preview::Look {
        let v = &self.view;
        crate::preview::Look {
            env_rotation: v.env_rotation,
            exposure: v.exposure,
            tonemap: v.tonemap as u32,
            saturation: v.saturation,
            contrast: v.contrast,
            light_intensity: v.light_intensity,
        }
    }

    fn update_fill_and_map_thumbs(&mut self) {
        use std::hash::{Hash, Hasher};
        // Thumbnails follow the viewport look and environment.
        let look = self.preview_look();
        let env = self.engine.env.name.clone();
        let Some(doc) = &self.doc else { return };
        let s = &doc.sets[doc.active];
        // The set's base values show where a fill writes nothing.
        let st = &s.settings;
        let base = format!("{:?}{:?}{}{}{:?}", st.base_color, st.base_values, st.height_scale, st.emissive_intensity, st.ext_channels);
        let mut fills = Vec::new();
        crate::document::visit(&s.layers, &mut |l| {
            if let crate::document::LayerKind::Fill(f) = &l.desc.kind {
                let mut h = std::collections::hash_map::DefaultHasher::new();
                format!("{f:?}").hash(&mut h);
                (look.key(), &env, &base).hash(&mut h);
                fills.push((l.desc.id, h.finish(), f.clone()));
            }
        });
        let mut rendered = 0;
        for (id, key, f) in fills {
            if rendered >= 3 || self.fill_thumbs.get(&id).is_some_and(|t| t.0 == key) {
                continue;
            }
            let tree = crate::document::LayerTree {
                desc: crate::document::LayerDesc::new(0, String::new(), crate::document::LayerKind::Fill(f)),
                children: Vec::new(),
            };
            let tex = self.shelf.preview(&self.engine, &tree, &doc.resources, look, Some(&s.settings));
            let mut r = self.renderer.write();
            match self.fill_thumbs.get_mut(&id) {
                Some(e) => {
                    r.update_egui_texture_from_wgpu_texture(&self.engine.device, &tex.view, wgpu::FilterMode::Linear, e.2);
                    e.0 = key;
                    e.1 = tex;
                }
                None => {
                    let tid = r.register_native_texture(&self.engine.device, &tex.view, wgpu::FilterMode::Linear);
                    self.fill_thumbs.insert(id, (key, tex, tid));
                }
            }
            rendered += 1;
        }
        if !s.baked || !self.set_settings_visible {
            return;
        }
        let key = s.gpu.uid.rotate_left(13) ^ s.bake_rev;
        let mut enc = self.engine.device.create_command_encoder(&Default::default());
        let mut any = false;
        for i in 0..layers::MESH_MAPS.len() {
            if self.map_thumbs.get(&i).is_some_and(|t| t.0 == key) {
                continue;
            }
            let entry = self.map_thumbs.entry(i).or_insert_with(|| {
                let t = crate::engine::thumbs::Thumbnailer::new_target(&self.engine.device);
                let tid = self.renderer.write().register_native_texture(&self.engine.device, &t.view, wgpu::FilterMode::Linear);
                (u64::MAX, t, tid)
            });
            let (src, gray) = layers::map_texture(&s.gpu, i);
            self.engine.thumbs.render(&self.engine.device, &mut enc, src, &entry.1, gray);
            entry.0 = key;
            any = true;
        }
        if any {
            self.engine.queue.submit(Some(enc.finish()));
        }
    }

    fn update_layer_thumbs(&mut self) {
        if self.last_thumbs.elapsed().as_secs_f32() < 0.3 {
            return;
        }
        self.last_thumbs = Instant::now();
        self.update_fill_and_map_thumbs();
        let Some(doc) = &self.doc else { return };
        let s = &doc.sets[doc.active];
        let uid = s.gpu.uid;
        let mut jobs: Vec<(u64, bool, u64)> = Vec::new();
        crate::document::visit(&s.layers, &mut |l| {
            if l.paint.is_some() {
                jobs.push((l.desc.id, false, l.rev ^ uid.rotate_left(17)));
            }
            if l.desc.mask.is_some() {
                if let Some(h) = self.engine.compositor.with_cached_mask(uid, l.desc.id, |_, h| h) {
                    jobs.push((l.desc.id, true, h ^ uid.rotate_left(29)));
                }
            }
        });
        let mut enc = self.engine.device.create_command_encoder(&Default::default());
        let mut any = false;
        for (id, mask, key) in jobs {
            if self.thumbs.get(&(id, mask)).is_some_and(|t| t.0 == key) {
                continue;
            }
            let entry = self.thumbs.entry((id, mask)).or_insert_with(|| {
                let t = crate::engine::thumbs::Thumbnailer::new_target(&self.engine.device);
                let tid = self.renderer.write().register_native_texture(&self.engine.device, &t.view, wgpu::FilterMode::Linear);
                (u64::MAX, t, tid)
            });
            let Some(l) = crate::document::find(&s.layers, id) else { continue };
            if mask {
                let thumbs = &self.engine.thumbs;
                let device = &self.engine.device;
                let dst = &entry.1;
                self.engine.compositor.with_cached_mask(uid, id, |t, _| thumbs.render(device, &mut enc, t, dst, true));
            } else if let Some(p) = &l.paint {
                self.engine.thumbs.render(&self.engine.device, &mut enc, &p.color, &entry.1, false);
            }
            entry.0 = key;
            any = true;
        }
        if any {
            self.engine.queue.submit(Some(enc.finish()));
        }
    }

    // ------------------------------------------------------------ input

    /// Escape closes the innermost transient thing: popup / menu, quick
    /// properties, dialogs, then floating docks. Returns true when it did.
    fn escape(&mut self, ctx: &egui::Context) -> bool {
        if ctx.data(|d| d.get_temp::<egui::Id>(widgets::eyedropper_id())).is_some() {
            ctx.data_mut(|d| d.remove::<egui::Id>(widgets::eyedropper_id()));
            self.status = "Eyedropper cancelled.".into();
            return true;
        }
        if ctx.memory(|m| m.any_popup_open()) {
            ctx.memory_mut(|m| m.close_popup());
            return true;
        }
        if widgets::close_color_picker(ctx) {
            return true;
        }
        if self.sel_cancel() {
            return true;
        }
        if self.tool == Tool::Path && self.path_deselect() {
            return true;
        }
        if self.quick_props.take().is_some() {
            return true;
        }
        if self.id_pick.take().is_some() {
            self.status = "ID color pick cancelled.".into();
            return true;
        }
        if self.import_dialog.take().is_some() || self.new_dialog.take().is_some() || self.export_dialog.take().is_some() || self.bake_dialog.take().is_some() {
            return true;
        }
        for flag in [&mut self.show_prefs, &mut self.show_view_settings, &mut self.show_history, &mut self.show_log] {
            if *flag {
                *flag = false;
                return true;
            }
        }
        false
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        // A key being assigned in the Shortcuts window: nothing else acts.
        if self.keymap_capture.is_some() {
            if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
                self.keymap_capture = None;
            }
            return;
        }
        if ctx.input(|i| i.key_pressed(Key::Escape) && !i.modifiers.any()) {
            // Text fields handle their own Escape (cancel edit), and an open
            // menu bar menu closes itself.
            if !ctx.wants_keyboard_input() && !menu_open(ctx) && self.escape(ctx) {
                ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape));
            }
        }
        if ctx.wants_keyboard_input() {
            return;
        }
        let cs = Modifiers::COMMAND | Modifiers::SHIFT;
        let act = |m: Modifiers, k: Key| ctx.input_mut(|i| i.consume_key(m, k));
        if act(cs, Key::S) {
            self.save_project(true);
        } else if act(Modifiers::COMMAND, Key::S) {
            self.save_project(false);
        }
        if act(cs, Key::E) {
            self.open_export();
        }
        if act(cs, Key::B) {
            self.open_bake_dialog();
        }
        let busy = self.stroke.is_some() || self.poly_drag.is_some();
        if busy {
        } else if act(cs, Key::Z) || act(Modifiers::COMMAND, Key::Y) {
            self.redo();
        } else if act(Modifiers::COMMAND, Key::Z) {
            self.undo();
        }
        if act(Modifiers::COMMAND, Key::K) {
            self.show_prefs = !self.show_prefs;
        }
        if act(Modifiers::COMMAND, Key::N) {
            self.new_dialog = Some(NewProjectDialog { path: String::new(), res: 2048, error: None, auto_uv: Default::default() });
        }
        if act(Modifiers::COMMAND, Key::O) {
            self.open_project();
        }
        // Layer commands act when the layer stack has the focus (last click
        // in it, or the mouse over it), like the real application.
        let over_layers = ctx.input(|i| i.pointer.hover_pos()).is_some_and(|p| self.layers_rect.contains(p));
        let layer_keys = self.layers_focus || over_layers;
        let has_layer = layer_keys && self.doc.as_ref().is_some_and(|d| d.sets[d.active].selected.is_some());
        if act(Modifiers::COMMAND, Key::J) && has_layer {
            self.run_cmd(panels::Cmd::Duplicate);
        }
        if act(Modifiers::COMMAND, Key::G) && has_layer {
            self.run_cmd(panels::Cmd::Group);
        }
        if act(Modifiers::COMMAND, Key::C) && has_layer {
            let d = self.doc.as_ref().unwrap();
            self.layer_clipboard = d.sets[d.active].selected.map(|id| (d.active, id));
            self.status = "Layer copied (Ctrl+V pastes it above the selection).".into();
        }
        if layer_keys && act(Modifiers::COMMAND, Key::V) {
            self.paste_layer();
        }
        // Alt + arrows reorder the selected effect, or the layer.
        let alt_up = act(Modifiers::ALT, Key::ArrowUp);
        let alt_down = act(Modifiers::ALT, Key::ArrowDown);
        if (alt_up || alt_down) && has_layer {
            let d = if alt_up { 1 } else { -1 };
            let target = self.doc.as_ref().map(|doc| doc.sets[doc.active].target);
            match target {
                Some(crate::document::Target::MaskEffect(e)) => self.run_cmd(panels::Cmd::MoveEffect(e, d)),
                Some(crate::document::Target::ContentEffect(e)) => self.run_cmd(panels::Cmd::MoveContentEffect(e, d)),
                _ => self.run_cmd(panels::Cmd::Move(d)),
            }
        }
        if act(Modifiers::ALT, Key::Q) {
            self.toggle_isolate();
        }
        // Path: Enter ends the selected path, Backspace / Delete remove its
        // selected point (before Delete removes a layer).
        if self.tool == Tool::Path && self.selected_path().is_some() {
            if act(Modifiers::NONE, Key::Enter) {
                self.path_deselect();
            }
            if act(Modifiers::NONE, Key::Backspace) || act(Modifiers::NONE, Key::Delete) {
                self.path_remove_point();
            }
        }
        let set_tool_early = |app: &mut Self, t: Tool| {
            if app.tool != t {
                app.sel_drag = None;
            }
            app.tool = t;
            app.prev_tool = None;
        };
        let km = self.keymap;
        let keys = ctx.input(|i| {
            use keymap::Action as A;
            let plain = !i.modifiers.command && !i.modifiers.alt;
            let p = |k| plain && !i.modifiers.shift && i.key_pressed(k);
            let q = |a| km.pressed(i, a);
            [
                q(A::Brush),
                q(A::Eraser),
                q(A::Marquee) || q(A::MarqueeShape),
                q(A::Lasso) || q(A::LassoShape),
                q(A::PolygonFill),
                q(A::Smudge),
                q(A::Clone),
                q(A::Picker),
                q(A::Hide),
                q(A::SwapViews),
                q(A::Perspective),
                q(A::Ortho),
                q(A::Baking),
                q(A::Painting),
                q(A::Rendering),
                q(A::SizeDown),
                q(A::SizeUp),
                q(A::Frame),
                q(A::NextChannel),
                q(A::PrevChannel),
                q(A::NextMap),
                q(A::PrevMap),
                q(A::SwapValue),
                q(A::Lazy),
                q(A::Split),
                q(A::Only3D),
                q(A::Only2D),
                p(Key::Delete),
                q(A::Symmetry),
                q(A::MarqueeShape) || q(A::LassoShape),
                q(A::Wand),
                q(A::FillTool) || q(A::FillSwitch),
                q(A::SelBrush),
                q(A::Path),
                q(A::Projection),
            ]
        });
        let [kb, ke, km, kl, ku, kr, ks, ki, kh, f4, f5, f6, f8, f9, f10, lb, rb, f, c, sc, kn, sn, x, d, f1, f2, f3, del, ky, cycle_shape, kw, kg, kq, kp, kproj] = keys;
        if kq {
            set_tool_early(self, Tool::SelBrush);
        }
        if kp {
            set_tool_early(self, Tool::Path);
        }
        if kproj {
            set_tool_early(self, Tool::Projection);
        }
        if kw {
            set_tool_early(self, Tool::Wand);
        }
        if kg {
            // G: bucket or gradient (the last used); Shift + G switches.
            let fill_map = self.keymap;
            let shift = ctx.input(|i| fill_map.pressed(i, keymap::Action::FillSwitch));
            let next = match self.tool {
                Tool::Bucket if shift => Tool::Gradient,
                Tool::Gradient if shift => Tool::Bucket,
                Tool::Bucket | Tool::Gradient => self.tool,
                _ => self.last_fill_tool,
            };
            self.last_fill_tool = next;
            set_tool_early(self, next);
        }
        // Photoshop keys. Number keys set the brush flow (1 = 10 %, 0 = 100 %).
        let digit = ctx.input(|i| {
            let plain = !i.modifiers.any();
            [Key::Num0, Key::Num1, Key::Num2, Key::Num3, Key::Num4, Key::Num5, Key::Num6, Key::Num7, Key::Num8, Key::Num9]
                .iter()
                .position(|k| plain && i.key_pressed(*k))
        });
        if let Some(n) = digit {
            self.brush.flow = if n == 0 { 1.0 } else { n as f32 / 10.0 };
            self.status = format!("Flow {} %", (self.brush.flow * 100.0).round());
        }
        if ky {
            // Y: symmetry on (X) / off.
            let on = self.brush.symmetry.iter().any(|s| *s);
            self.brush.symmetry = if on { [false; 3] } else { [true, false, false] };
            self.status = if on { "Symmetry off".into() } else { "Symmetry on (X)".into() };
        }
        let set_tool = |app: &mut Self, t: Tool| {
            if app.tool != t {
                app.sel_drag = None;
            }
            app.tool = t;
            app.prev_tool = None;
        };
        use selection::SelShape;
        if kb {
            set_tool(self, Tool::Paint);
        }
        if ke {
            set_tool(self, Tool::Erase);
        }
        if km {
            // M: rectangle select; Shift + M switches rectangle / ellipse.
            let next = match self.tool {
                Tool::Select(SelShape::Rect) if cycle_shape => SelShape::Ellipse,
                Tool::Select(SelShape::Ellipse) if cycle_shape => SelShape::Rect,
                Tool::Select(s @ (SelShape::Rect | SelShape::Ellipse)) => s,
                _ => self.last_marquee,
            };
            self.last_marquee = next;
            set_tool(self, Tool::Select(next));
        }
        if kl {
            let next = match self.tool {
                Tool::Select(SelShape::Lasso) if cycle_shape => SelShape::PolyLasso,
                Tool::Select(SelShape::PolyLasso) if cycle_shape => SelShape::Lasso,
                Tool::Select(s @ (SelShape::Lasso | SelShape::PolyLasso)) => s,
                _ => self.last_lasso,
            };
            self.last_lasso = next;
            set_tool(self, Tool::Select(next));
        }
        if ku {
            set_tool(self, Tool::PolygonFill);
        }
        if kr {
            set_tool(self, Tool::Smudge);
        }
        if ks {
            set_tool(self, Tool::Clone);
        }
        if kh {
            set_tool(self, Tool::Hide);
        }
        if ki && self.tool != Tool::Picker {
            // Temporary tool: the previous one comes back after a pick.
            self.prev_tool = Some(self.tool);
            self.tool = Tool::Picker;
        }
        if f4 {
            self.view.swap_views = !self.view.swap_views;
        }
        if f5 {
            self.camera.ortho = false;
        }
        if f6 {
            self.camera.ortho = true;
        }
        if f8 {
            self.open_bake_dialog();
        }
        if f9 {
            self.view.path_trace = false;
            self.bake_dialog = None;
        }
        if f10 {
            self.view.path_trace = !self.view.path_trace;
        }
        if lb {
            self.brush.size = (self.brush.size / 1.15).max(1.0);
        }
        if rb {
            self.brush.size = (self.brush.size * 1.15).min(1000.0);
        }
        if f {
            self.frame_mesh();
            self.view.uv_zoom = 0.0;
            self.view.uv_rot = 0.0;
        }
        // C / Shift + C: material and channels, N / Shift + N: mesh maps.
        let cycle = |cur: Display, range: std::ops::RangeInclusive<usize>, step: isize| -> Display {
            let (lo, hi) = (*range.start() as isize, *range.end() as isize);
            let i = Display::ALL.iter().position(|d| *d == cur).unwrap() as isize;
            let n = hi - lo + 1;
            let next = if i < lo || i > hi { if step > 0 { lo } else { hi } } else { lo + (i - lo + step).rem_euclid(n) };
            Display::ALL[next as usize]
        };
        if c || sc {
            self.view.display = cycle(self.view.display, 0..=Display::index(Display::Opacity), if sc { -1 } else { 1 });
        }
        if kn || sn {
            self.view.display = cycle(self.view.display, Display::index(Display::MapAo)..=Display::index(Display::MapId), if sn { -1 } else { 1 });
        }
        if x {
            if self.tool == Tool::SelBrush {
                self.sel_brush_add = !self.sel_brush_add;
                self.status = if self.sel_brush_add { "Selection brush: add".into() } else { "Selection brush: subtract".into() };
            } else {
                self.mat.mask_value = 1.0 - self.mat.mask_value;
            }
        }
        if d {
            if self.brush.lazy > 0.0 {
                self.lazy_saved = self.brush.lazy;
                self.brush.lazy = 0.0;
            } else {
                self.brush.lazy = self.lazy_saved.max(10.0);
            }
            self.status = if self.brush.lazy > 0.0 { "Lazy mouse on".into() } else { "Lazy mouse off".into() };
        }
        if f1 {
            self.view.layout = Layout::Both;
        }
        if f2 {
            self.view.layout = Layout::Only3D;
        }
        if f3 {
            self.view.layout = Layout::Only2D;
        }
        // Selections (Photoshop): Ctrl + A all, Ctrl + D deselect, Ctrl +
        // Shift + D reselect, Ctrl + Shift + I invert, Shift + Backspace
        // fills, Delete clears; the polygonal lasso closes with Enter and
        // drops its last point with Backspace.
        if act(Modifiers::COMMAND, Key::A) {
            self.select_all();
        }
        if act(cs, Key::D) {
            self.reselect();
        } else if act(Modifiers::COMMAND, Key::D) {
            self.deselect();
        }
        if act(cs, Key::I) {
            self.invert_selection();
        }
        if act(Modifiers::SHIFT, Key::Backspace) {
            self.fill_selection(false);
        }
        if self.sel_drag.is_some() {
            if act(Modifiers::NONE, Key::Enter) {
                self.sel_commit();
            }
            if act(Modifiers::NONE, Key::Backspace) {
                self.sel_back();
            }
        }
        if del && !has_layer && self.has_selection() {
            self.fill_selection(true);
        }
        if del && has_layer {
            let target = self.doc.as_ref().map(|doc| doc.sets[doc.active].target);
            match target {
                Some(crate::document::Target::MaskEffect(e)) => self.run_cmd(panels::Cmd::RemoveEffect(e)),
                Some(crate::document::Target::ContentEffect(e)) => self.run_cmd(panels::Cmd::RemoveContentEffect(e)),
                _ => self.run_cmd(panels::Cmd::Delete),
            }
        }
    }

    /// Ctrl + V: a copy of the copied layer above the selection.
    fn paste_layer(&mut self) {
        let Some((set, src)) = self.layer_clipboard else { return };
        let Some(doc) = &self.doc else { return };
        if set >= doc.sets.len() || crate::document::find(&doc.sets[set].layers, src).is_none() {
            self.status = "The copied layer no longer exists.".into();
            return;
        }
        if set != doc.active {
            // Another texture set (Painter pastes across sets): painted
            // pixels come along when the resolutions match.
            let dst = doc.active;
            let same_res = doc.sets[set].resolution() == doc.sets[dst].resolution();
            self.checkpoint("Paste layer");
            let doc = self.doc.as_mut().unwrap();
            let Some(layer) = crate::document::find(&doc.sets[set].layers, src).cloned() else { return };
            let copy = if same_res {
                let mut enc = self.engine.device.create_command_encoder(&Default::default());
                let c = doc.deep_copy(&self.engine, &layer, &mut enc);
                self.engine.queue.submit(Some(enc.finish()));
                c
            } else {
                doc.instantiate(&self.engine, dst, &layer.tree())
            };
            doc.insert_layer(dst, copy);
            if !same_res && (layer.paint.is_some() || !layer.mask_paint.is_empty()) {
                self.toast("Pasted without its painted pixels: the texture sets have different resolutions.".into());
            }
            return;
        }
        let target = doc.sets[set].selected;
        self.checkpoint("Paste layer");
        let doc = self.doc.as_mut().unwrap();
        doc.sets[set].selected = Some(src);
        doc.duplicate_layer(&self.engine, set);
        if let (Some(new), Some(t)) = (doc.sets[set].selected, target) {
            if t != src {
                doc.move_layer_onto(set, new, t, Some(crate::document::Place::Above));
            }
        }
    }

    /// Alt + Q: show only the active texture set, again to restore.
    fn toggle_isolate(&mut self) {
        let Some(doc) = &mut self.doc else { return };
        match self.isolate_saved.take() {
            Some(v) => {
                for (s, vis) in doc.sets.iter_mut().zip(v) {
                    s.settings.visible = vis;
                }
            }
            None => {
                self.isolate_saved = Some(doc.sets.iter().map(|s| s.settings.visible).collect());
                let a = doc.active;
                for (i, s) in doc.sets.iter_mut().enumerate() {
                    s.settings.visible = i == a;
                }
            }
        }
    }

    fn open_export(&mut self) {
        let Some(doc) = &self.doc else { return };
        let dir = doc
            .path
            .as_ref()
            .and_then(|p| p.parent())
            .map(|p| p.join("textures"))
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_default().join("textures"));
        let mut presets = crate::export::builtin_presets();
        presets.extend(crate::export::load_user_presets(&crate::library::sub_dir("export")));
        self.export_dialog = Some(ExportDialog {
            dir: dir.display().to_string().replace(r"\\?\", ""),
            presets,
            preset: 0,
            opts: Default::default(),
            sets: vec![true; doc.sets.len()],
        });
    }
}

/// Color button that only writes the value back on a real edit (egui's
/// button round-trips colors through a lossy conversion every frame).
/// Payload of type `T` being dragged over `rect` (drop zone highlight).
/// Uses the layer under the pointer rather than the widget hit test, which a
/// tooltip near the cursor can steal.
pub(super) fn drop_hover<T: std::any::Any + Send + Sync>(ui: &egui::Ui, rect: Rect) -> Option<Arc<T>> {
    if ui.rect_contains_pointer(rect) {
        egui::DragAndDrop::payload::<T>(ui.ctx())
    } else {
        None
    }
}

/// Takes a payload of type `T` released over `rect`. Payloads of other
/// types are left for the other drop zones.
pub(super) fn drop_take<T: std::any::Any + Send + Sync>(ui: &egui::Ui, rect: Rect) -> Option<Arc<T>> {
    if ui.rect_contains_pointer(rect) && ui.input(|i| i.pointer.any_released()) && egui::DragAndDrop::has_payload_of_type::<T>(ui.ctx()) {
        egui::DragAndDrop::take_payload::<T>(ui.ctx())
    } else {
        None
    }
}

/// Remembers where a widget was drawn (the UI test drags onto it).
pub(super) fn mark(ctx: &egui::Context, name: impl std::fmt::Display, r: Rect) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::new(("ui mark", name.to_string())), r));
}

pub(super) fn marked(ctx: &egui::Context, name: &str) -> Option<Rect> {
    ctx.data(|d| d.get_temp::<Rect>(egui::Id::new(("ui mark", name.to_string()))))
}

pub(crate) fn color_edit(ui: &mut egui::Ui, c: &mut [f32; 3]) -> egui::Response {
    let mut tmp = *c;
    let r = ui.color_edit_button_rgb(&mut tmp);
    if r.changed() {
        *c = tmp;
    }
    r
}

/// Texture shown by display modes that sample `t_disp`.
fn display_texture(s: &TextureSet, d: Display) -> &crate::engine::Tex {
    let g = &s.gpu;
    match d {
        Display::MapAo => &g.baked_ao,
        Display::MapCurvature => &g.baked_curvature,
        Display::MapNormal => &g.baked_normal,
        Display::MapWorldNormal => &g.baked_wnormal,
        Display::MapPosition => &g.baked_position,
        Display::MapThickness => &g.baked_thickness,
        Display::MapId => &g.baked_id,
        Display::LayerMask => &g.mask_view,
        Display::Ext(i) => crate::document::ext_slot(i as usize).map(|s| &g.final_ext[s.0]).unwrap_or(&g.final_base),
        _ => &g.final_base,
    }
}

/// (texture, uv view mode) for the 2D view.
fn uv_texture(s: &TextureSet, d: Display) -> (&crate::engine::Tex, u32) {
    let g = &s.gpu;
    match d {
        Display::Roughness => (&g.final_orm, 2),
        Display::Metallic => (&g.final_orm, 3),
        Display::Ao => (&g.final_orm, 1),
        Display::Height => (&g.final_orm, 6),
        Display::Normal => (&g.final_normal, 5),
        Display::Emissive => (&g.final_emissive, 0),
        Display::Opacity => (&g.final_base, 4),
        Display::MapAo | Display::MapCurvature | Display::MapThickness | Display::LayerMask => (display_texture(s, d), 1),
        Display::MapNormal | Display::MapWorldNormal => (display_texture(s, d), 5),
        Display::MapPosition | Display::MapId => (display_texture(s, d), 0),
        Display::Ext(i) => (display_texture(s, d), 1 + crate::document::ext_slot(i as usize).map(|s| s.1).unwrap_or(0) as u32),
        _ => (&g.final_base, 0),
    }
}

pub fn apply_bake(engine: &Engine, s: &mut TextureSet, r: &bake::BakeResult) {
    let q = &engine.queue;
    s.gpu.alloc_baked(&engine.device, None);
    let g = &s.gpu;
    g.baked_ao.write_bytes(q, &f32_to_f16_bytes(&r.ao));
    g.baked_curvature.write_bytes(q, &f32_to_f16_bytes(&r.curvature));
    g.baked_normal.write_bytes(q, &f32_to_f16_bytes(bytemuck::cast_slice(&r.normal)));
    g.baked_wnormal.write_bytes(q, &f32_to_f16_bytes(bytemuck::cast_slice(&r.wnormal)));
    g.baked_thickness.write_bytes(q, &f32_to_f16_bytes(&r.thickness));
    g.baked_id.write_bytes(q, &f32_to_f16_bytes(bytemuck::cast_slice(&r.id)));
    s.baked = true;
    s.bake_rev += 1;
    s.dirty = true;
}

impl eframe::App for RetroApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.ui_frame(ctx);
    }

    fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        self.filter_input(ctx, raw);
    }
}

impl RetroApp {
    /// Runs before egui sees the input: Tab hides the docks (as in the real
    /// application) instead of moving keyboard focus, unless text is being typed.
    pub(crate) fn filter_input(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        self.eyedrop_filter_input(ctx, raw);
        if ctx.wants_keyboard_input() {
            return;
        }
        let is_tab = |e: &egui::Event| matches!(e, egui::Event::Key { key: Key::Tab, modifiers, .. } if !modifiers.any());
        let presses = raw.events.iter().filter(|e| is_tab(e) && matches!(e, egui::Event::Key { pressed: true, repeat: false, .. })).count();
        // Shift + Tab would move egui's focus onto a button and silence every shortcut.
        raw.events.retain(|e| !matches!(e, egui::Event::Key { key: Key::Tab, modifiers, .. } if !modifiers.command && !modifiers.alt));
        if presses % 2 == 1 {
            self.ui_hidden = !self.ui_hidden;
        }
    }
}

impl RetroApp {
    /// One UI + GPU frame (eframe's update, also driven by the UI smoke test).
    pub fn ui_frame(&mut self, ctx: &egui::Context) {
        // Nothing draws last frame's retired textures any more.
        self.retired.clear();
        if self.tool != self.tool_seen {
            self.tool_seen = self.tool;
            self.tool_changed();
        }
        self.handle_close(ctx);
        self.saving_notice(ctx);
        self.run_pending_save(ctx);
        self.poll_bake();
        if self.env_loading.as_ref().is_some_and(|h| h.is_finished()) {
            if let Some(Some((p, w, h, px))) = self.env_loading.take().and_then(|h| h.join().ok()) {
                let name = p.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                let e = crate::engine::env::Environment::build(&self.engine.device, &self.engine.queue, w, h, px, name);
                self.engine.set_environment(e);
                self.log.push(format!("Environment: {}", p.display()));
            }
        } else if self.env_loading.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }
        if self.bake_job.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        // The marching ants move.
        if self.has_selection() {
            ctx.request_repaint_after(std::time::Duration::from_millis(62));
        }
        // Stamps beyond one frame's capacity, or a stroke waiting to end.
        if !self.pending.is_empty() || self.stroke_end_pending {
            ctx.request_repaint();
        }
        if let Some(p) = self.startup.take() {
            let is_project = p.extension().is_some_and(|e| e.eq_ignore_ascii_case("sprp"));
            if is_project {
                self.open_path(&p);
            } else if let Err(e) = self.create_project(p.clone(), 2048) {
                self.message(format!("Could not import {}: {e:#}", p.display()));
            }
        }
        if let Some(res) = self.pending_res.take() {
            if let Some(doc) = &mut self.doc {
                let set = doc.active;
                doc.change_resolution(&self.engine, set, res);
                self.history.clear();
                self.engine.compositor.trim();
                self.engine.procgen.trim();
                self.message(format!("Texture set resolution changed to {res}"));
            }
        }
        let dropped = ctx.input(|i| i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).collect::<Vec<_>>());
        for p in &dropped {
            self.open_path(p);
        }

        let history_len_before = self.history.version;
        self.overlay_open = self.quick_props.is_some()
            || ctx.memory(|m| m.any_popup_open() || m.areas().visible_layer_ids().iter().any(|l| l.order == egui::Order::Foreground));
        widgets::begin_frame(ctx);
        self.eyedrop_frame(ctx);
        if let Some(doc) = &mut self.doc {
            let a = doc.active;
            doc.sets[a].prune_selection();
        }
        if let Some(p) = ctx.input(|i| if i.pointer.any_pressed() { i.pointer.press_origin() } else { None }) {
            self.layers_focus = self.layers_rect.contains(p);
        }

        // Pen pressure arrives as touch events with a force.
        ctx.input(|i| {
            for e in &i.events {
                if let egui::Event::Touch { force, phase, .. } = e {
                    match (phase, force) {
                        (egui::TouchPhase::End | egui::TouchPhase::Cancel, _) => self.pressure = 1.0,
                        (_, Some(f)) => self.pressure = f.clamp(0.0, 1.0),
                        _ => {}
                    }
                }
            }
        });
        let t = Instant::now();
        self.shortcuts(ctx);
        self.prof("shortcuts", t);
        let t = Instant::now();
        self.dialogs(ctx);
        self.prof("dialogs", t);
        let t = Instant::now();
        self.quick_properties(ctx);
        self.prof("quick_properties", t);
        let t = Instant::now();
        self.canvas_ui(ctx);
        self.prof("canvas + views", t);
        let t = Instant::now();
        self.drag_preview(ctx);
        layers::layer_drag_ghost(ctx);
        self.prof("drag_preview", t);

        if self.stroke.is_none() {
            let t = Instant::now();
            self.track_edits(history_len_before);
            self.prof("track_edits", t);
        }
        let t = Instant::now();
        self.sync_path_brush();
        self.sync_path_layers();
        self.prof("sync_path_layers", t);
        let t = Instant::now();
        self.sync_brush_textures();
        self.prof("sync_brush_textures", t);
        let t = Instant::now();
        self.gpu_frame();
        self.prof("gpu_frame", t);
        // Thumbnails refresh between drags: navigation and strokes stay light.
        let t = Instant::now();
        if !ctx.input(|i| i.pointer.any_down()) {
            self.update_layer_thumbs();
        }
        self.prof("update_layer_thumbs", t);
        if self.view.show_properties {
            let t = Instant::now();
            self.update_brush_ball();
            self.prof("update_brush_ball", t);
        }
        if self.view.path_trace && !self.engine.pathtracer.done() {
            ctx.request_repaint();
        } else if self.view.path_trace && self.engine.pathtracer.denoise_pending() {
            ctx.request_repaint_after(std::time::Duration::from_millis(30));
        }
        let t = Instant::now();
        self.autosave();
        self.prof("autosave", t);
        let t = Instant::now();
        self.generate_thumbnails(ctx);
        self.prof("generate_thumbnails", t);

        let title = match &self.doc {
            Some(d) => format!(
                "Material Forge - {}{}",
                d.path.as_ref().map(|p| p.display().to_string().replace(r"\\?\", "")).unwrap_or_else(|| d.mesh_name.clone()),
                if d.modified { " *" } else { "" }
            ),
            None => "Material Forge".into(),
        };
        if title != self.title {
            self.title = title.clone();
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));
        }
    }
}

/// Paint passes and target textures for a stroke frame.
pub fn passes_for(erase: bool, mask: bool, channels: [bool; 7]) -> Vec<PaintPass> {
    if mask {
        return vec![if erase { PaintPass::MaskErase } else { PaintPass::Mask }];
    }
    let mut v = vec![if erase { PaintPass::EraseA } else { PaintPass::PaintA }];
    if channels[4] || channels[5] || channels[6] {
        v.push(if erase { PaintPass::EraseB } else { PaintPass::PaintB });
    }
    v
}

impl RetroApp {
    /// Target selection for painting on the active set: content of a paint
    /// layer, or a paint effect of a mask (created on demand).
    fn resolve_paint_target(&mut self) -> Result<(u64, PaintTarget), String> {
        let doc = self.doc.as_mut().ok_or("No project")?;
        let set = doc.active;
        let s = &doc.sets[set];
        let layer = s.selected_layer().ok_or("Select or add a layer to paint on.")?;
        let lid = layer.desc.id;
        let target = s.target;
        let is_paint = layer.paint.is_some();
        if layer.desc.paths.is_some() && matches!(target, Target::Content | Target::ContentEffect(_)) {
            return Err("This is a path layer: its paint comes from its paths (Path tool). Paint on another layer, or on its mask.".into());
        }
        match target {
            Target::Content | Target::ContentEffect(_) if is_paint => Ok((lid, PaintTarget::Content)),
            Target::MaskEffect(eid) if layer.mask_paint.contains_key(&eid) => Ok((lid, PaintTarget::MaskPaint(eid))),
            _ => {
                if !target.is_mask() {
                    // Like the real application: fill / filter / folder content
                    // is not paintable; the user picks a mask or a paint layer.
                    return Err("Painting is not possible on this layer: select its mask (or add one), or add a paint layer.".into());
                }
                // Paint into the top paint effect of the mask, creating one.
                let top = layer.desc.mask.as_ref().and_then(|m| {
                    m.effects.iter().rev().find(|e| layer.mask_paint.contains_key(&e.id)).map(|e| e.id)
                });
                if let Some(eid) = top {
                    doc.sets[set].target = Target::MaskEffect(eid);
                    return Ok((lid, PaintTarget::MaskPaint(eid)));
                }
                if target.is_mask() {
                    let needs_mask = layer.desc.mask.is_none();
                    let had = needs_mask;
                    self.history.push(Entry::State { label: "Add paint effect".into(), set, state: doc.sets[set].state() });
                    let eid = doc
                        .add_mask_effect(&self.engine, set, crate::document::MaskEffectKind::Paint)
                        .ok_or("No layer selected")?;
                    if had {
                        // A fresh mask on a fill or folder starts black so painting reveals it.
                        if let Some(l) = doc.sets[set].selected_layer_mut() {
                            if let Some(m) = &mut l.desc.mask {
                                m.base = 0.0;
                            }
                        }
                    }
                    return Ok((lid, PaintTarget::MaskPaint(eid)));
                }
                Err("Nothing to paint on".into())
            }
        }
    }
}

/// A menu bar menu (or one of its submenus) is open: they are Foreground
/// areas egui does not count as popups.
fn menu_open(ctx: &egui::Context) -> bool {
    ctx.memory(|m| {
        m.areas().visible_layer_ids().iter().any(|l| l.order == egui::Order::Foreground && l.id != egui::Id::new("quick properties"))
    }) && !ctx.memory(|m| m.any_popup_open())
}
