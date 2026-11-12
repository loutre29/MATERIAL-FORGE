//! The shelf: materials, smart materials, smart masks, brushes, alphas,
//! textures, procedurals and environments, with lazily rendered thumbnails.

use super::*;
use crate::brush::{alpha_image, builtin_brushes, AlphaKind, BrushPreset};
use crate::document::*;
use crate::library;
use crate::preview::{Previewer, THUMB};
use super::icons::{self, Icon};
use eframe::egui::{Color32, Pos2, RichText, TextureHandle};
use std::collections::HashMap;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Category {
    Materials,
    SmartMaterials,
    SmartMasks,
    Brushes,
    Alphas,
    Textures,
    Procedurals,
    Environments,
    /// Filters (filter layers / effects).
    Filters,
    /// Every library category at once.
    All,
    /// Images imported into the open project.
    Project,
}

impl Category {
    const ALL: [Category; 9] = [
        Category::Materials,
        Category::SmartMaterials,
        Category::SmartMasks,
        Category::Brushes,
        Category::Alphas,
        Category::Textures,
        Category::Procedurals,
        Category::Environments,
        Category::Filters,
    ];
}

/// What a shelf item applies.
#[derive(Clone)]
pub(super) enum Item {
    Material(String, FillDesc),
    /// A material made of texture maps (starter assets).
    TexMaterial(Box<library::TexMaterial>),
    SmartMaterial(String, LayerTree, Option<PathBuf>, Vec<(u64, String)>),
    SmartMask(String, MaskDesc),
    Brush(BrushPreset),
    Alpha(AlphaKind),
    AlphaFile(PathBuf),
    Texture(PathBuf),
    Procedural(ProcKind),
    Environment(PathBuf),
    /// A resource of the open document (id, name).
    ProjectTexture(u64, String),
    /// A filter, applied as a filter layer.
    Filter(FilterKind),
}

/// Where an asset comes from (the Assets breadcrumb menu).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Source {
    All,
    Project,
    Starter,
    User,
}

impl Source {
    pub const ALL: [Source; 4] = [Source::All, Source::Project, Source::Starter, Source::User];
    pub fn name(self) -> &'static str {
        match self {
            Source::All => "All libraries",
            Source::Project => "Project",
            Source::Starter => "Starter assets",
            Source::User => "Your assets",
        }
    }
}

impl Item {
    pub(super) fn kind_name(&self) -> &'static str {
        match self {
            Item::Material(..) | Item::TexMaterial(_) => "Material",
            Item::SmartMaterial(..) => "Smart material",
            Item::SmartMask(..) => "Smart mask",
            Item::Brush(_) => "Brush",
            Item::Alpha(_) | Item::AlphaFile(_) => "Alpha",
            Item::Texture(_) => "Texture",
            Item::Procedural(_) => "Procedural",
            Item::Environment(_) => "Environment",
            Item::ProjectTexture(..) => "Project resource",
            Item::Filter(_) => "Filter",
        }
    }

    pub(super) fn name(&self) -> String {
        match self {
            Item::ProjectTexture(_, n) => n.clone(),
            Item::Filter(k) => k.name().into(),
            Item::Material(n, _) | Item::SmartMaterial(n, ..) | Item::SmartMask(n, _) => n.clone(),
            Item::TexMaterial(m) => m.name.clone(),
            Item::Brush(b) => b.name.clone(),
            Item::Alpha(k) => k.name().into(),
            Item::Procedural(k) => k.name().into(),
            Item::AlphaFile(p) | Item::Texture(p) | Item::Environment(p) => {
                p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
            }
        }
    }
    pub(super) fn key(&self) -> String {
        match self {
            Item::ProjectTexture(id, _) => format!("res:{id}"),
            Item::Filter(k) => format!("filter:{}", k.name()),
            Item::Material(n, _) => format!("mat:{n}"),
            // Same key form as procedural materials: a fill's material slot
            // finds its thumbnail by name.
            Item::TexMaterial(m) => format!("mat:{}", m.name),
            Item::SmartMaterial(n, _, d, _) => format!("sm:{n}:{}", d.as_ref().map(|d| d.display().to_string()).unwrap_or_default()),
            Item::SmartMask(n, _) => format!("mask:{n}"),
            Item::Brush(b) => format!("brush:{}", b.name),
            Item::Alpha(k) => format!("alpha:{}", k.name()),
            Item::AlphaFile(p) => format!("alphafile:{}", p.display()),
            Item::Texture(p) => format!("tex:{}", p.display()),
            Item::Procedural(k) => format!("proc:{}", k.name()),
            Item::Environment(p) => format!("env:{}", p.display()),
        }
    }
}

pub struct Shelf {
    category: Category,
    filter: String,
    items: HashMap<Category, Vec<Item>>,
    thumbs: HashMap<String, egui::TextureId>,
    handles: HashMap<String, TextureHandle>,
    /// Keeps GPU thumbnails alive (registered as native egui textures).
    gpu_thumbs: Vec<crate::engine::Tex>,
    wanted: Vec<Item>,
    /// Keys of `wanted` (constant-time duplicate check).
    wanted_keys: std::collections::HashSet<String>,
    /// Plain images (textures, alphas, environments) decoded off the UI
    /// thread for thumbnails: key, image (None: unreadable).
    decoded_img: std::sync::Arc<std::sync::Mutex<HashMap<String, Option<image::RgbaImage>>>>,
    /// Texture files of smart materials decoded for thumbnails: key,
    /// (resource id in the tree, image).
    decoded_files: std::sync::Arc<std::sync::Mutex<HashMap<String, Vec<(u64, image::RgbaImage)>>>>,
    previewer: Option<Previewer>,
    /// Rounded box previewer (smart materials, smart masks).
    previewer_box: Option<Previewer>,
    /// Flat square previewer (procedural textures).
    previewer_flat: Option<Previewer>,
    /// Texture materials, lit by the viewport's environment.
    previewer_mat: Option<Previewer>,
    /// Fill layer row balls (studio lit, like the library).
    previewer_layer: Option<Previewer>,
    /// Environment the material thumbnails were lit by.
    thumb_env: String,
    loaded: bool,
    list_view: bool,
    /// Grid thumbnail size: 0 small, 1 medium, 2 large.
    thumb_size: u8,
    pub(super) source: Source,
    builtin_brushes: std::collections::HashSet<String>,
    /// Downloaded materials (Claude's ambientCG scans, out of the library):
    /// shown only in the Project category of a project that uses them.
    scans: Vec<Item>,
    /// The scans the open project uses, by name.
    project_mats: std::collections::HashSet<String>,
    /// Key of the highlighted asset.
    selected: Option<String>,
    /// Procedurals: Painter's shelf folder shown (None: all).
    proc_folder: Option<&'static str>,
    /// Materials: category shown (None: all).
    mat_folder: Option<&'static str>,
    /// Alphas, textures, brushes: the folder shown in that category.
    file_folder: Option<(Category, String)>,
    /// Texture material maps decoded off the UI thread for thumbnails:
    /// key, (map stem, image).
    decoded: std::sync::Arc<std::sync::Mutex<HashMap<String, Vec<(&'static str, image::RgbaImage)>>>>,
    decoding: std::collections::HashSet<String>,
    /// Large hover previews of texture maps: texture, source size.
    previews: HashMap<String, (TextureHandle, [u32; 2])>,
    /// Hover previews decoding on a worker, and the files that failed.
    preview_decoded: std::sync::Arc<std::sync::Mutex<HashMap<String, Option<(image::RgbaImage, [u32; 2])>>>>,
    preview_pending: std::collections::HashSet<String>,
    preview_failed: std::collections::HashSet<String>,
}

impl Default for Shelf {
    fn default() -> Self {
        Shelf {
            category: Category::Materials,
            filter: String::new(),
            items: HashMap::new(),
            thumbs: HashMap::new(),
            handles: HashMap::new(),
            gpu_thumbs: Vec::new(),
            wanted: Vec::new(),
            wanted_keys: Default::default(),
            decoded_img: Default::default(),
            decoded_files: Default::default(),
            previewer: None,
            previewer_box: None,
            previewer_flat: None,
            previewer_mat: None,
            previewer_layer: None,
            thumb_env: String::new(),
            loaded: false,
            list_view: false,
            thumb_size: 1,
            source: Source::All,
            builtin_brushes: Default::default(),
            scans: Vec::new(),
            project_mats: Default::default(),
            selected: None,
            proc_folder: None,
            mat_folder: None,
            file_folder: None,
            decoded: Default::default(),
            decoding: Default::default(),
            previews: HashMap::new(),
            preview_decoded: Default::default(),
            preview_pending: Default::default(),
            preview_failed: Default::default(),
        }
    }
}

impl Shelf {
    /// Every loaded item of every category (smoke test).
    pub(super) fn all_items(&mut self) -> Vec<Item> {
        self.load();
        Category::ALL.iter().flat_map(|c| self.items.get(c).cloned().unwrap_or_default()).collect()
    }

    /// What the user can change in the panel (the crawler restores it).
    pub(super) fn ui_state(&self) -> (Category, String, bool, Source, Option<String>) {
        (self.category, self.filter.clone(), self.list_view, self.source, self.selected.clone())
    }

    pub(super) fn set_ui_state(&mut self, s: (Category, String, bool, Source, Option<String>)) {
        (self.category, self.filter, self.list_view, self.source, self.selected) = s;
    }

    pub(super) fn set_category(&mut self, i: usize) {
        self.category = Category::ALL[i % Category::ALL.len()];
    }

    /// Items of the current Assets category.
    pub(super) fn browse(&mut self, doc: Option<&Document>) -> Vec<Item> {
        self.load();
        let project: Vec<Item> = doc
            .map(|d| {
                let mut v: Vec<&Resource> = d.resources.values().collect();
                // What the user imported: the maps and tips materials and
                // brushes pulled in are the library's items already.
                v.retain(|r| r.imported);
                v.sort_by_key(|r| r.id);
                v.into_iter().map(|r| Item::ProjectTexture(r.id, r.name.clone())).collect()
            })
            .unwrap_or_default();
        // A scan is the project's when its maps are among the project's
        // resources ("<name> color.jpg").
        self.project_mats = self
            .scans
            .iter()
            .filter_map(|it| match it {
                Item::TexMaterial(m) => Some(m),
                _ => None,
            })
            .filter(|m| {
                let color = m.map("color").and_then(|p| p.extension().map(|x| x.to_string_lossy().into_owned())).unwrap_or_default();
                let name = format!("{} color.{color}", m.name);
                doc.is_some_and(|d| d.resources.values().any(|r| r.name == name))
            })
            .map(|m| m.name.clone())
            .collect();
        let mut project = project;
        project.extend(self.scans.iter().filter(|it| matches!(it, Item::TexMaterial(m) if self.project_mats.contains(&m.name))).cloned());
        let get = |c: Category| self.items.get(&c).cloned().unwrap_or_default();
        let mut items: Vec<Item> = match self.category {
            Category::All => {
                let mut v: Vec<Item> = Category::ALL.iter().flat_map(|c| get(*c)).collect();
                v.extend(project.iter().filter(|it| matches!(it, Item::ProjectTexture(..))).cloned());
                v
            }
            Category::Project => project.clone(),
            Category::Textures => {
                let mut v = get(Category::Textures);
                v.extend(get(Category::Procedurals));
                v.extend(project.iter().filter(|it| matches!(it, Item::ProjectTexture(..))).cloned());
                v
            }
            c => get(c),
        };
        let src = self.source;
        if src != Source::All {
            items.retain(|it| self.source_of(it) == src);
        }
        items
    }

    /// Library an item belongs to.
    fn source_of(&self, it: &Item) -> Source {
        let user_dir = library::library_dir();
        match it {
            Item::ProjectTexture(..) => Source::Project,
            Item::SmartMaterial(n, _, dir, _) => {
                if dir.as_ref().is_some_and(|d| d.starts_with(&user_dir)) || n.ends_with("(user)") {
                    Source::User
                } else {
                    Source::Starter
                }
            }
            Item::SmartMask(n, _) => {
                if n.ends_with("(user)") {
                    Source::User
                } else {
                    Source::Starter
                }
            }
            Item::Brush(b) => {
                if self.builtin_brushes.contains(&b.name) {
                    Source::Starter
                } else {
                    Source::User
                }
            }
            Item::AlphaFile(p) | Item::Texture(p) | Item::Environment(p) => {
                if p.starts_with(&user_dir) {
                    Source::User
                } else {
                    Source::Starter
                }
            }
            // Downloaded by Claude: only ever shown as the project's.
            Item::TexMaterial(m) if self.project_mats.contains(&m.name) && m.dir.starts_with(crate::ambientcg::scans_dir()) => Source::Project,
            Item::TexMaterial(m) if m.dir.starts_with(&user_dir) => Source::User,
            Item::Material(..) | Item::TexMaterial(_) | Item::Alpha(_) | Item::Procedural(_) | Item::Filter(_) => Source::Starter,
        }
    }

    /// Queues a thumbnail render for `item` unless it exists or is queued.
    pub(super) fn want(&mut self, item: Item) {
        let key = item.key();
        if !self.thumbs.contains_key(&key) && self.wanted_keys.insert(key) {
            self.wanted.push(item);
        }
    }

    /// Loaded items of one library category.
    pub(super) fn items_of(&mut self, c: Category) -> Vec<Item> {
        self.load();
        self.items.get(&c).cloned().unwrap_or_default()
    }

    pub(super) fn thumb_map(&self) -> HashMap<String, egui::TextureId> {
        self.thumbs.clone()
    }

    pub(super) fn show_category(&mut self, c: Category) {
        self.category = c;
    }

    /// Renders a material preview (layer stack fill thumbnails, the brush
    /// material): a sphere lit by the scene's environment and graded like
    /// the viewport (`look`), so it looks like the layer on the model.
    pub(super) fn preview(&mut self, engine: &crate::engine::Engine, tree: &LayerTree, resources: &HashMap<u64, Resource>, look: crate::preview::Look, set: Option<&SetSettings>) -> crate::engine::Tex {
        let pv = self.previewer_layer.get_or_insert_with(|| Previewer::new_layer(engine));
        pv.render_fill(engine, tree, resources, look, set)
    }

    pub fn refresh(&mut self) {
        self.loaded = false;
    }

    /// The library's scanned materials, plain presets and smart materials
    /// (Claude chooses among them).
    pub(super) fn material_names(&mut self) -> (Vec<library::TexMaterial>, Vec<String>, Vec<String>) {
        self.load();
        let names = |c: Category| self.items.get(&c).map(|v| v.iter().map(Item::name).collect()).unwrap_or_default();
        let mats = self.items.get(&Category::Materials).cloned().unwrap_or_default();
        let tex = mats.iter().filter_map(|i| if let Item::TexMaterial(m) = i { Some((**m).clone()) } else { None }).collect();
        let presets = mats.iter().filter(|i| matches!(i, Item::Material(..))).map(Item::name).collect();
        (tex, presets, names(Category::SmartMaterials))
    }

    /// A material or smart material of the library by name.
    pub(super) fn material_item(&mut self, name: &str) -> Option<Item> {
        self.load();
        [Category::SmartMaterials, Category::Materials]
            .iter()
            .flat_map(|c| self.items.get(c).into_iter().flatten())
            .find(|i| i.name().eq_ignore_ascii_case(name.trim()))
            .cloned()
    }

    fn load(&mut self) {
        if self.loaded {
            return;
        }
        self.loaded = true;
        let mut m = HashMap::new();
        // Texture materials (starter assets); the procedural presets when
        // the folder is missing.
        let tex: Vec<Item> = library::texture_materials().into_iter().map(|m| Item::TexMaterial(Box::new(m))).collect();
        self.scans = library::scan_materials().into_iter().map(|m| Item::TexMaterial(Box::new(m))).collect();
        m.insert(
            Category::Materials,
            if tex.is_empty() { library::materials().into_iter().map(|x| Item::Material(x.name.into(), x.fill)).collect() } else { tex },
        );
        // Starter smart materials read the starter files by absolute path.
        let starter = library::starter_dir();
        let mut sm: Vec<Item> = library::smart_materials()
            .into_iter()
            .map(|x| {
                let files = x.textures.into_iter().map(|(id, p)| (id, p.display().to_string())).collect();
                Item::SmartMaterial(x.name, x.tree, starter.clone(), files)
            })
            .collect();
        sm.extend(
            library::user_smart_materials()
                .into_iter()
                .map(|u| Item::SmartMaterial(format!("{} (user)", u.file.name), u.file.tree, Some(u.dir), u.file.textures)),
        );
        m.insert(Category::SmartMaterials, sm);
        let mut masks: Vec<Item> = library::smart_masks().into_iter().map(|x| Item::SmartMask(x.name.into(), x.mask)).collect();
        masks.extend(library::user_smart_masks().into_iter().map(|x| Item::SmartMask(format!("{} (user)", x.name), x.mask)));
        m.insert(Category::SmartMasks, masks);
        self.builtin_brushes = builtin_brushes().into_iter().map(|b| b.name).collect();
        let mut brushes: Vec<Item> = builtin_brushes().into_iter().map(Item::Brush).collect();
        brushes.extend(library::user_brushes().into_iter().map(Item::Brush));
        m.insert(Category::Brushes, brushes);
        // The analytic round tip (its edge follows the hardness), the starter
        // alphas by folder, then the user's.
        let mut alphas: Vec<Item> = vec![Item::Alpha(AlphaKind::Round)];
        alphas.extend(library::starter_files("alphas").into_iter().map(Item::AlphaFile));
        alphas.extend(library::files_with(&library::sub_dir("alphas"), library::IMAGE_EXTS).into_iter().map(Item::AlphaFile));
        m.insert(Category::Alphas, alphas);
        let mut textures: Vec<Item> = library::starter_files("textures").into_iter().map(Item::Texture).collect();
        textures.extend(library::files_with(&library::sub_dir("textures"), library::IMAGE_EXTS).into_iter().map(Item::Texture));
        m.insert(Category::Textures, textures);
        // Painter's procedurals in its folder order, the classic ones last.
        let mut procs: Vec<ProcKind> = ProcKind::ALL.to_vec();
        procs.sort_by_key(|k| crate::procedurals::FOLDERS.iter().position(|f| *f == crate::procedurals::folder_of(*k)).unwrap_or(99));
        m.insert(Category::Procedurals, procs.into_iter().map(Item::Procedural).collect());
        m.insert(
            Category::Environments,
            library::files_with(&library::sub_dir("environments"), library::ENV_EXTS)
                .into_iter()
                .chain(library::starter_environments())
                .map(Item::Environment)
                .collect(),
        );
        m.insert(Category::Filters, FilterKind::ALL.iter().map(|k| Item::Filter(*k)).collect());
        self.items = m;
    }

    /// egui texture for a document resource (stencil overlay).
    pub fn texture_for_resource(&mut self, ctx: &egui::Context, r: &Resource) -> egui::TextureId {
        let key = format!("res:{}", r.id);
        if let Some(t) = self.thumbs.get(&key) {
            return *t;
        }
        let img = &*r.image;
        let ci = egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw());
        let h = ctx.load_texture(&key, ci, egui::TextureOptions::LINEAR);
        let id = h.id();
        self.handles.insert(key.clone(), h);
        self.thumbs.insert(key, id);
        id
    }

    /// Drops what belongs to the previous document (resource ids restart).
    pub(super) fn forget_project(&mut self) {
        self.thumbs.retain(|k, _| !k.starts_with("res:"));
        self.handles.retain(|k, _| !k.starts_with("res:"));
        self.previews.retain(|k, _| !k.starts_with("res:"));
    }

    /// Large preview of a texture-like item for its hover popup (loaded on
    /// first hover): texture, its size in pixels when known.
    pub(super) fn preview_for(&mut self, ctx: &egui::Context, item: &Item, doc: Option<&crate::document::Document>) -> Option<(egui::TextureId, Option<[u32; 2]>)> {
        let key = item.key();
        if let Some((h, d)) = self.previews.get(&key) {
            return Some((h.id(), Some(*d)));
        }
        let thumb = self.thumbs.get(&key).copied().filter(|t| *t != egui::TextureId::default()).map(|t| (t, None));
        // Files decode on a worker; the thumbnail shows meanwhile (and for
        // files that cannot be read).
        let file = match item {
            Item::Texture(p) | Item::AlphaFile(p) => Some(p.clone()),
            Item::TexMaterial(m) => m.preview().or_else(|| m.map("color")),
            _ => None,
        };
        if let Some(path) = file {
            let ready = self.preview_decoded.lock().ok().and_then(|mut d| d.remove(&key));
            match ready {
                Some(Some((img, dims))) => {
                    let ci = egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw());
                    let h = ctx.load_texture(format!("preview {key}"), ci, egui::TextureOptions::LINEAR);
                    let id = h.id();
                    self.previews.insert(key, (h, dims));
                    return Some((id, Some(dims)));
                }
                Some(None) => {
                    self.preview_failed.insert(key);
                    return thumb;
                }
                None => {
                    if !self.preview_failed.contains(&key) && self.preview_pending.insert(key.clone()) {
                        let (out, key) = (self.preview_decoded.clone(), key.clone());
                        std::thread::spawn(move || {
                            let img = crate::imageio::open(&path).ok().map(|i| {
                                let full = i.to_rgba8();
                                let dims = [full.width(), full.height()];
                                (image::imageops::thumbnail(&full, 256, 256), dims)
                            });
                            if let Ok(mut d) = out.lock() {
                                d.insert(key, img);
                            }
                        });
                    }
                    ctx.request_repaint_after(std::time::Duration::from_millis(30));
                    return thumb;
                }
            }
        }
        let full = match item {
            Item::ProjectTexture(id, _) => doc.and_then(|d| d.resources.get(id)).map(|r| (*r.image).clone()),
            // Generated looks: their thumbnail.
            Item::Procedural(_) | Item::Alpha(_) | Item::Environment(_) => {
                return self.thumbs.get(&key).copied().filter(|t| *t != egui::TextureId::default()).map(|t| (t, None));
            }
            _ => None,
        }?;
        let dims = [full.width(), full.height()];
        let img = image::imageops::thumbnail(&full, 256, 256);
        let ci = egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw());
        let h = ctx.load_texture(format!("preview {key}"), ci, egui::TextureOptions::LINEAR);
        let id = h.id();
        self.previews.insert(key, (h, dims));
        Some((id, Some(dims)))
    }

    fn image_thumb(&mut self, ctx: &egui::Context, key: &str, img: image::RgbaImage) -> egui::TextureId {
        let img = image::imageops::thumbnail(&img, THUMB, THUMB);
        let ci = egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw());
        let h = ctx.load_texture(key, ci, egui::TextureOptions::LINEAR);
        let id = h.id();
        self.handles.insert(key.to_string(), h);
        self.thumbs.insert(key.to_string(), id);
        id
    }
}

fn load_image_any(path: &std::path::Path) -> Option<image::RgbaImage> {
    let img = crate::imageio::open(path).ok()?;
    let is_hdr = matches!(img, image::DynamicImage::ImageRgb32F(_) | image::DynamicImage::ImageRgba32F(_));
    if is_hdr {
        let f = img.into_rgba32f();
        let small = image::imageops::thumbnail(&f, THUMB * 2, THUMB);
        let (w, h) = small.dimensions();
        Some(image::RgbaImage::from_fn(w, h, |x, y| {
            let p = small.get_pixel(x, y).0;
            let t = |v: f32| ((v / (1.0 + v)).powf(1.0 / 2.2) * 255.0) as u8;
            image::Rgba([t(p[0]), t(p[1]), t(p[2]), 255])
        }))
    } else {
        Some(img.into_rgba8())
    }
}

impl RetroApp {
    pub(super) fn assets_ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        self.shelf.load();
        let mut apply: Option<Item> = None;
        let mut import = false;
        let mut save_smart = false;
        // Asset types as chips (the main five, the others under More).
        const MAIN: [(Category, &str); 5] = [
            (Category::Materials, "Materials"),
            (Category::SmartMaterials, "Smart"),
            (Category::Brushes, "Brushes"),
            (Category::Alphas, "Alphas"),
            (Category::Textures, "Textures"),
        ];
        const MORE: [(Category, &str); 4] = [(Category::Procedurals, "Patterns"), (Category::SmartMasks, "Masks"), (Category::Filters, "Filters"), (Category::Environments, "Lighting")];
        // Every filter in force, each a chip with a cross next to the
        // panel's name, so a short list never reads as an empty library.
        let cat = self.shelf.category;
        let folder_now: Option<String> = match cat {
            Category::Materials => self.shelf.mat_folder.map(String::from),
            Category::Procedurals => self.shelf.proc_folder.map(String::from),
            Category::All => None,
            _ => self.shelf.file_folder.as_ref().filter(|(c, _)| *c == cat).map(|(_, f)| f.clone()),
        };
        let mut chips: Vec<(String, u8)> = Vec::new();
        if self.shelf.source != Source::All {
            chips.push((self.shelf.source.name().to_string(), 0));
        }
        if cat != Category::All {
            let name = MAIN.iter().chain(MORE.iter()).find(|(c, _)| *c == cat).map(|x| x.1).unwrap_or("Type");
            chips.push((name.to_string(), 1));
        }
        if let Some(f) = &folder_now {
            chips.push((f.clone(), 2));
        }
        if !self.shelf.filter.trim().is_empty() {
            let mut s: String = self.shelf.filter.trim().chars().take(14).collect();
            if self.shelf.filter.trim().chars().count() > 14 {
                s.push_str("...");
            }
            chips.push((format!("\"{s}\""), 3));
        }
        let mut gone: Vec<u8> = Vec::new();
        {
            {
                // Header: the library source (menu), list / grid, more actions;
                // the filters in force between them.
                let sub = if chips.is_empty() { self.shelf.source.name() } else { "" };
                let filters = |ui: &mut egui::Ui, strip: Rect| filter_strip(ui, strip, &chips, &mut gone);
                super::dock::panel_header_with(ui, "Library", sub, |ui| {
                    let more = icons::button(ui, Icon::Menu, false, 26.0, "More");
                    let mid = ui.id().with("library more");
                    if more.clicked() {
                        ui.memory_mut(|m| m.toggle_popup(mid));
                    }
                    super::widgets::popup_below(ui, mid, &more, egui::PopupCloseBehavior::CloseOnClick, |ui| {
                        ui.set_min_width(240.0);
                        for src in Source::ALL {
                            let on = self.shelf.source == src;
                            if ui.add(egui::SelectableLabel::new(on, src.name())).clicked() {
                                self.shelf.source = src;
                            }
                        }
                        ui.separator();
                        if ui.button("Import resources...").clicked() {
                            import = true;
                        }
                        if ui.button("Save the selected layer as a smart material").clicked() {
                            save_smart = true;
                        }
                        if ui.button("Open the library folder").clicked() {
                            let _ = std::process::Command::new("explorer").arg(library::library_dir()).spawn();
                        }
                        if ui.button("Refresh").clicked() {
                            self.shelf.refresh();
                        }
                    });
                    // The view: a list, or thumbnails of three sizes.
                    let lv = self.shelf.list_view;
                    let b = icons::button(ui, if lv { Icon::List } else { Icon::Grid }, false, 26.0, "Thumbnail size / list");
                    super::mark(ctx, "library view", b.rect);
                    let pid = egui::Id::new("library view menu");
                    if b.clicked() {
                        ui.memory_mut(|m| m.toggle_popup(pid));
                    }
                    super::widgets::popup_below(ui, pid, &b, egui::PopupCloseBehavior::CloseOnClick, |ui| {
                        ui.set_min_width(150.0);
                        for (k, name) in ["Small thumbnails", "Medium thumbnails", "Large thumbnails"].into_iter().enumerate() {
                            if ui.selectable_label(!lv && self.shelf.thumb_size == k as u8, name).clicked() {
                                self.shelf.list_view = false;
                                self.shelf.thumb_size = k as u8;
                            }
                        }
                        if ui.selectable_label(lv, "List").clicked() {
                            self.shelf.list_view = true;
                        }
                    });
                }, filters);
                for k in std::mem::take(&mut gone) {
                    match k {
                        0 => self.shelf.source = Source::All,
                        1 => self.shelf.category = Category::All,
                        2 => match cat {
                            Category::Materials => self.shelf.mat_folder = None,
                            Category::Procedurals => self.shelf.proc_folder = None,
                            _ => self.shelf.file_folder = None,
                        },
                        _ => self.shelf.filter.clear(),
                    }
                }
                egui::Frame::none().inner_margin(egui::Margin { left: 10.0, right: 10.0, top: 10.0, bottom: 4.0 }).show(ui, |ui| {
                    // Search: a rounded field.
                    let (sr, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 32.0), egui::Sense::hover());
                    ui.painter().rect(sr, crate::style::R_CONTROL, crate::style::FIELD, egui::Stroke::new(1.0f32, crate::style::SEPARATOR));
                    icons::paint(ui.painter(), Rect::from_center_size(egui::pos2(sr.min.x + 16.0, sr.center().y), egui::vec2(15.0, 15.0)), Icon::Search, crate::style::TEXT3);
                    let er = Rect::from_min_max(egui::pos2(sr.min.x + 30.0, sr.min.y + 6.0), egui::pos2(sr.max.x - 28.0, sr.max.y - 6.0));
                    ui.put(er, egui::TextEdit::singleline(&mut self.shelf.filter).hint_text("Search materials, brushes, stamps").frame(false).clip_text(true));
                    if !self.shelf.filter.is_empty() {
                        let cr = Rect::from_center_size(egui::pos2(sr.max.x - 15.0, sr.center().y), egui::vec2(18.0, 18.0));
                        let c = ui.interact(cr, ui.id().with("clear search"), egui::Sense::click()).on_hover_text("Clear the search");
                        icons::paint(ui.painter(), cr.shrink(2.0), Icon::Close, if c.hovered() { crate::style::TEXT } else { crate::style::TEXT3 });
                        if c.clicked() {
                            self.shelf.filter.clear();
                        }
                    }
                    ui.add_space(8.0);
                    // Asset types: the active one again shows all. Materials and
                    // patterns get their folder as one compact menu.
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(5.0, 5.0);
                        for (c, name) in MAIN {
                            let on = self.shelf.category == c;
                            if super::widgets::pill(ui, name, on).clicked() {
                                self.shelf.category = if on { Category::All } else { c };
                            }
                        }
                        let in_more = MORE.iter().find(|(c, _)| *c == self.shelf.category).map(|x| x.1);
                        let more = super::widgets::pill(ui, &format!("{} +", in_more.unwrap_or("More")), in_more.is_some());
                        let pid = ui.id().with("library more types");
                        if more.clicked() {
                            ui.memory_mut(|m| m.toggle_popup(pid));
                        }
                        super::widgets::popup_below(ui, pid, &more, egui::PopupCloseBehavior::CloseOnClick, |ui| {
                            ui.set_min_width(150.0);
                            for (c, name) in MORE {
                                if ui.add(egui::SelectableLabel::new(self.shelf.category == c, name)).clicked() {
                                    self.shelf.category = c;
                                }
                            }
                        });
                    });
                    // Folders of materials (categories) and patterns: one menu.
                    let cat = self.shelf.category;
                    let folders: Vec<String> = match cat {
                        Category::Materials => {
                            let items = self.shelf.items_of(Category::Materials);
                            library::MATERIAL_CATEGORIES
                                .iter()
                                .filter(|c| items.iter().any(|i| matches!(i, Item::TexMaterial(m) if m.category == **c)))
                                .map(|c| c.to_string())
                                .collect()
                        }
                        Category::Procedurals => crate::procedurals::FOLDERS.iter().map(|f| f.to_string()).collect(),
                        Category::Alphas | Category::Textures | Category::Brushes => {
                            let mut v: Vec<String> = Vec::new();
                            for f in self.shelf.items_of(cat).iter().filter_map(item_folder) {
                                if !v.contains(&f) {
                                    v.push(f);
                                }
                            }
                            v
                        }
                        _ => Vec::new(),
                    };
                    if !folders.is_empty() {
                        ui.add_space(6.0);
                        let cur: Option<String> = match cat {
                            Category::Materials => self.shelf.mat_folder.map(String::from),
                            Category::Procedurals => self.shelf.proc_folder.map(String::from),
                            _ => self.shelf.file_folder.as_ref().filter(|(c, _)| *c == cat).map(|(_, f)| f.clone()),
                        };
                        let mut pick = cur.clone();
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(if cat == Category::Materials { "Category" } else { "Folder" }).size(11.5).color(crate::style::TEXT3));
                            egui::ComboBox::from_id_salt("library folder").selected_text(cur.as_deref().unwrap_or("All")).width(150.0).show_ui(ui, |ui| {
                                ui.selectable_value(&mut pick, None, "All");
                                for f in &folders {
                                    ui.selectable_value(&mut pick, Some(f.clone()), f.as_str());
                                }
                            });
                        });
                        if pick != cur {
                            match cat {
                                Category::Materials => {
                                    self.shelf.mat_folder = pick.and_then(|p| library::MATERIAL_CATEGORIES.iter().copied().find(|c| *c == p))
                                }
                                Category::Procedurals => {
                                    self.shelf.proc_folder = pick.and_then(|p| crate::procedurals::FOLDERS.iter().copied().find(|c| *c == p))
                                }
                                _ => self.shelf.file_folder = pick.map(|p| (cat, p)),
                            }
                        }
                    }
                });
                let filter = self.shelf.filter.to_lowercase();
                let folder = self.shelf.proc_folder.filter(|_| self.shelf.category == Category::Procedurals);
                let mat_folder = self.shelf.mat_folder.filter(|_| self.shelf.category == Category::Materials);
                let file_folder = self.shelf.file_folder.as_ref().filter(|(c, _)| *c == self.shelf.category).map(|(_, f)| f.clone());
                let items: Vec<Item> = self
                    .shelf
                    .browse(self.doc.as_ref())
                    .into_iter()
                    .filter(|i| filter.is_empty() || i.name().to_lowercase().contains(&filter))
                    .filter(|i| match (folder, i) {
                        (Some(f), Item::Procedural(k)) => crate::procedurals::folder_of(*k) == f,
                        _ => true,
                    })
                    .filter(|i| match (mat_folder, i) {
                        (Some(f), Item::TexMaterial(m)) => m.category == f,
                        _ => true,
                    })
                    .filter(|i| file_folder.as_ref().is_none_or(|f| item_folder(i).as_ref() == Some(f)))
                    .collect();
                // Project resources show their own image as thumbnail.
                if let Some(doc) = &self.doc {
                    for it in &items {
                        if let Item::ProjectTexture(id, _) = it {
                            if let Some(r) = doc.resources.get(id) {
                                self.shelf.texture_for_resource(ctx, r);
                            }
                        }
                    }
                }
                egui::ScrollArea::vertical()
                    .id_salt("asset grid")
                    .auto_shrink([false, false])
                    .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
                    .show(ui, |ui| {
                    egui::Frame::none().inner_margin(egui::Margin::symmetric(6.0, 2.0)).show(ui, |ui| {
                        if items.is_empty() {
                            ui.weak(if self.shelf.category == Category::Project {
                                "No project resources. Import images or drop them on the window."
                            } else {
                                "Nothing here."
                            });
                        }
                        let w = ui.available_width();
                        let list = self.shelf.list_view;
                        let min_tile = [52.0, 66.0, 104.0][self.shelf.thumb_size.min(2) as usize];
                        let cols = if list { 1 } else { ((w / min_tile).floor() as usize).max(2) };
                        let gap = 4.0;
                        let tile = if list { 30.0 } else { ((w - gap * (cols as f32 - 1.0)) / cols as f32).floor() };
                        let row_h = if list { 32.0 } else { tile + 14.0 };
                        for (ri, chunk) in items.chunks(cols).enumerate() {
                            let (row, _) = ui.allocate_exact_size(egui::vec2(w, row_h), egui::Sense::hover());
                            // Rows out of view only take their space (and ask
                            // no thumbnail); the first row stays for tests.
                            if ri > 0 && !ui.is_rect_visible(row) {
                                continue;
                            }
                            for (k, item) in chunk.iter().enumerate() {
                                let key = item.key();
                                let thumb = self.shelf.thumbs.get(&key).copied().filter(|t| *t != egui::TextureId::default());
                                if !self.shelf.thumbs.contains_key(&key) {
                                    self.shelf.want(item.clone());
                                }
                                let cell = if list {
                                    row
                                } else {
                                    Rect::from_min_size(row.min + egui::vec2(k as f32 * (tile + gap), 0.0), egui::vec2(tile, row_h))
                                };
                                if std::ptr::eq(item, &items[0]) {
                                    super::mark(ui.ctx(), "asset0", cell);
                                }
                                let resp = ui.interact(cell, ui.id().with(("asset", &key)), egui::Sense::click_and_drag());
                                // Texture maps: a large preview, like Painter's shelf.
                                let preview = if resp.hovered() { self.shelf.preview_for(ctx, item, self.doc.as_ref()) } else { None };
                                let resp = match preview {
                                    Some((tex, dims)) => resp.on_hover_ui(|ui| {
                                        ui.image((tex, egui::vec2(256.0, 256.0)));
                                        ui.label(RichText::new(item.name()).strong());
                                        let size = dims.map(|d| format!("  {} x {}", d[0], d[1])).unwrap_or_default();
                                        ui.label(RichText::new(format!("{}{size}", item.kind_name())).size(11.0).color(Color32::from_gray(160)));
                                    }),
                                    None => resp.on_hover_text(format!("{}\n{}", item.name(), item.kind_name())),
                                };
                                let selected = self.shelf.selected.as_deref() == Some(key.as_str());
                                let p = ui.painter();
                                if selected {
                                    p.rect_filled(cell, 3.0, crate::style::ROW_SELECTED);
                                } else if resp.hovered() {
                                    p.rect_filled(cell, 3.0, crate::style::warm(50));
                                }
                                let tr = if list {
                                    Rect::from_min_size(cell.min + egui::vec2(2.0, 1.0), egui::vec2(tile, tile))
                                } else {
                                    Rect::from_min_size(cell.min + egui::vec2(3.0, 2.0), egui::vec2(tile - 6.0, tile - 6.0))
                                };
                                match thumb {
                                    Some(t) => {
                                        p.image(t, tr, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
                                    }
                                    None => {
                                        p.rect_filled(tr, 4.0, crate::style::warm(45));
                                    }
                                }
                                let font = egui::FontId::proportional(if list { 12.0 } else { 10.5 });
                                let (pos, max_w, align) = if list {
                                    (Pos2::new(tr.max.x + 8.0, cell.center().y), cell.max.x - tr.max.x - 10.0, egui::Align2::LEFT_CENTER)
                                } else {
                                    (Pos2::new(cell.center().x, cell.max.y - 8.0), cell.width() - 2.0, egui::Align2::CENTER_CENTER)
                                };
                                let shown = elide(p, &item.name(), &font, max_w);
                                p.text(pos, align, shown, font, Color32::from_gray(205));
                                if resp.drag_started() {
                                    resp.dnd_set_drag_payload(item.clone());
                                }
                                if resp.clicked() {
                                    self.shelf.selected = Some(key.clone());
                                }
                                if resp.double_clicked() {
                                    apply = Some(item.clone());
                                }
                                let src = self.shelf.source_of(item);
                                resp.context_menu(|ui| {
                                    ui.set_min_width(220.0);
                                    // Info block, like the real application.
                                    let info = |ui: &mut egui::Ui, k: &str, v: &str| {
                                        ui.horizontal(|ui| {
                                            ui.label(RichText::new(k).size(11.0).color(Color32::from_gray(140)));
                                            ui.label(RichText::new(v).size(11.0));
                                        });
                                    };
                                    info(ui, "Name", &item.name());
                                    info(ui, "Usage", item.kind_name());
                                    info(ui, "Library", src.name());
                                    let path = match item {
                                        Item::AlphaFile(p) | Item::Texture(p) | Item::Environment(p) => Some(p.clone()),
                                        Item::SmartMaterial(_, _, Some(d), _) if d.starts_with(library::library_dir()) => Some(d.clone()),
                                        _ => None,
                                    };
                                    if let Some(p) = &path {
                                        info(ui, "Path", &p.display().to_string());
                                    }
                                    ui.separator();
                                    if ui.button("Apply").clicked() {
                                        apply = Some(item.clone());
                                        ui.close_menu();
                                    }
                                    if let Some(p) = &path {
                                        if ui.button("Show in Explorer").clicked() {
                                            let _ = std::process::Command::new("explorer").arg("/select,").arg(p).spawn();
                                            ui.close_menu();
                                        }
                                        if src == Source::User && ui.button("Delete from your assets").clicked() {
                                            let r = if p.is_dir() { std::fs::remove_dir_all(p) } else { std::fs::remove_file(p) };
                                            self.status = match r {
                                                Ok(()) => format!("Deleted {}", p.display()),
                                                Err(e) => format!("Could not delete {}: {e}", p.display()),
                                            };
                                            self.shelf.refresh();
                                            ui.close_menu();
                                        }
                                    }
                                });
                            }
                        }
                    });
                });
            }
        }
        if import {
            self.import_assets();
        }
        if save_smart {
            self.run_cmd(panels::Cmd::SaveSmartMaterial);
        }
        if let Some(item) = apply {
            self.apply_item(item);
        }
    }

    /// Bottom bar import: the import dialog asks project or library; the
    /// category picks the default type.
    fn import_assets(&mut self) {
        let kind = match self.shelf.category {
            Category::Alphas => super::ImportKind::Alpha,
            Category::Environments => super::ImportKind::Environment,
            _ => super::ImportKind::Texture,
        };
        if let Some(files) = super::file_dialog().and_then(|f| f.pick_files()) {
            self.ask_import(files, kind);
        }
    }

    /// Floating thumbnail under the pointer while an asset is dragged.
    pub(super) fn drag_preview(&mut self, ctx: &egui::Context) {
        let Some(item) = egui::DragAndDrop::payload::<Item>(ctx) else { return };
        let Some(pos) = ctx.pointer_interact_pos() else { return };
        let layer = egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("asset drag preview"));
        let p = ctx.layer_painter(layer);
        let r = Rect::from_center_size(pos + egui::vec2(30.0, 30.0), egui::vec2(48.0, 48.0));
        p.rect_filled(r.expand(3.0), 4.0, Color32::from_black_alpha(170));
        match self.shelf.thumbs.get(&item.key()).copied().filter(|t| *t != egui::TextureId::default()) {
            Some(t) => {
                p.image(t, r, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::from_white_alpha(230));
            }
            None => {
                p.rect_filled(r, 4.0, crate::style::warm(60));
            }
        }
        p.text(r.center_bottom() + egui::vec2(0.0, 11.0), egui::Align2::CENTER_CENTER, item.name(), egui::FontId::proportional(11.0), Color32::WHITE);
    }

    /// The layers a material or smart material adds (its textures imported
    /// into the project), None for other items.
    pub(super) fn item_tree(&mut self, item: Item) -> Option<LayerTree> {
        let fill_tree = |name: String, fill: FillDesc| LayerTree { desc: LayerDesc::new(0, name, LayerKind::Fill(Box::new(fill))), children: Vec::new() };
        match item {
            Item::TexMaterial(m) => {
                let fill = self.tex_material_fill(&m)?;
                Some(fill_tree(m.name.clone(), fill))
            }
            Item::Material(name, mut fill) => {
                fill.material = Some(name.clone());
                Some(fill_tree(name, fill))
            }
            Item::SmartMaterial(name, mut tree, dir, textures) => {
                let doc = self.doc.as_mut()?;
                if let Some(dir) = dir {
                    let mut map = HashMap::new();
                    for (old, file) in textures {
                        if let Ok(id) = doc.import_resource(&self.engine, &dir.join(&file), ResKind::Texture) {
                            map.insert(old, id);
                        }
                    }
                    library::remap_resources(&mut tree, &map);
                }
                tree.desc.name = name.trim_end_matches(" (user)").to_string();
                Some(tree)
            }
            _ => None,
        }
    }

    pub(super) fn apply_item(&mut self, item: Item) {
        if self.doc.is_none() && !matches!(item, Item::Brush(_) | Item::Alpha(_) | Item::Environment(_)) {
            self.status = "Open a project first.".into();
            return;
        }
        match item {
            Item::TexMaterial(m) => {
                let Some(fill) = self.tex_material_fill(&m) else { return };
                self.checkpoint("Add material");
                let doc = self.doc.as_mut().unwrap();
                let set = doc.active;
                let mut l = doc.new_layer(&self.engine, set, LayerKind::Fill(Box::new(fill)), Some(m.name.clone()));
                l.desc.name = m.name.clone();
                doc.insert_layer(set, l);
            }
            Item::Material(name, mut fill) => {
                self.checkpoint("Add material");
                fill.material = Some(name.clone());
                let doc = self.doc.as_mut().unwrap();
                let set = doc.active;
                let mut l = doc.new_layer(&self.engine, set, LayerKind::Fill(Box::new(fill)), Some(name.clone()));
                l.desc.name = name;
                doc.insert_layer(set, l);
            }
            Item::SmartMaterial(name, mut tree, dir, textures) => {
                self.checkpoint("Add smart material");
                let doc = self.doc.as_mut().unwrap();
                if let Some(dir) = dir {
                    let mut map = HashMap::new();
                    for (old, file) in textures {
                        if let Ok(id) = doc.import_resource(&self.engine, &dir.join(&file), ResKind::Texture) {
                            map.insert(old, id);
                        }
                    }
                    library::remap_resources(&mut tree, &map);
                }
                let set = doc.active;
                let mut l = doc.instantiate(&self.engine, set, &tree);
                l.desc.name = name.trim_end_matches(" (user)").to_string();
                doc.insert_layer(set, l);
            }
            Item::SmartMask(_, mask) => {
                if self.doc.as_ref().unwrap().sets[self.doc.as_ref().unwrap().active].selected.is_none() {
                    self.status = "Select a layer to apply a smart mask to.".into();
                    return;
                }
                self.run_cmd(panels::Cmd::ApplySmartMask(mask));
            }
            Item::Brush(b) => {
                self.apply_brush_preset(&b);
                if matches!(self.tool, Tool::PolygonFill | Tool::Picker) {
                    self.tool = Tool::Paint;
                }
            }
            Item::Alpha(k) => self.brush.alpha = AlphaSource::Builtin(k),
            Item::AlphaFile(p) => {
                let doc = self.doc.as_mut().unwrap();
                match doc.import_resource(&self.engine, &p, ResKind::Alpha) {
                    Ok(id) => self.brush.alpha = AlphaSource::Resource(id),
                    Err(e) => self.status = format!("Could not load alpha: {e:#}"),
                }
            }
            Item::Texture(p) => {
                let doc = self.doc.as_mut().unwrap();
                let id = match doc.import_resource(&self.engine, &p, ResKind::Texture) {
                    Ok(id) => id,
                    Err(e) => {
                        self.status = format!("Could not load texture: {e:#}");
                        return;
                    }
                };
                self.assign_texture_or_new_fill(id, p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default());
            }
            Item::Procedural(k) => {
                self.checkpoint("Apply procedural");
                let doc = self.doc.as_mut().unwrap();
                let set = doc.active;
                let s = &mut doc.sets[set];
                let into_fill = matches!(s.selected_layer().map(|l| &l.desc.kind), Some(LayerKind::Fill(_)));
                if into_fill && s.target == Target::Content {
                    if let Some(LayerKind::Fill(f)) = s.selected_layer_mut().map(|l| &mut l.desc.kind) {
                        // A normal generator goes into the normal channel.
                        let ch = &mut f.channels[if k.is_normal() { CH_NORMAL } else { CH_COLOR }];
                        ch.enabled = true;
                        ch.source = SourceKind::Procedural;
                        ch.procedural = ProcParams::of(k);
                        ch.value = [0.05; 3];
                        ch.value_b = [0.9; 3];
                        if f.projection.mode == ProjMode::Uv {
                            f.projection = Projection { mode: ProjMode::Object3D, scale: 6.0, ..Default::default() };
                        }
                    }
                    s.dirty = true;
                } else if s.selected.is_some() && s.target.is_mask() {
                    doc.add_mask_effect(&self.engine, set, MaskEffectKind::fill_procedural(k));
                } else {
                    let mut f = FillDesc::default();
                    f.channels[CH_COLOR] = FillChannel {
                        source: SourceKind::Procedural,
                        procedural: ProcParams::of(k),
                        value: [0.05; 3],
                        value_b: [0.9; 3],
                        ..Default::default()
                    };
                    f.channels[CH_ROUGH].enabled = false;
                    f.channels[CH_METAL].enabled = false;
                    f.projection = Projection { mode: ProjMode::Object3D, scale: 6.0, ..Default::default() };
                    let l = doc.new_layer(&self.engine, set, LayerKind::Fill(Box::new(f)), Some(k.name().into()));
                    doc.insert_layer(set, l);
                }
            }
            Item::Environment(p) => self.load_environment(&p),
            Item::ProjectTexture(id, name) => self.assign_texture_or_new_fill(id, name),
            Item::Filter(k) => self.run_cmd(panels::Cmd::AddLayer(LayerKind::Filter(FilterDesc::of(k)))),
        }
    }

    /// A texture dropped from the shelf: into the selected fill layer's base
    /// color, or a new fill layer.
    /// Imports the maps of a texture material into the project (once:
    /// resources already there are reused) and returns its fill.
    /// Applies a brush preset: its tip and dynamics. Size, symmetry, lazy
    /// mouse and pen settings stay as the user set them. A starter tip file
    /// is imported into the project (without one open, the preset's
    /// built-in tip stands in).
    pub(super) fn apply_brush_preset(&mut self, b: &BrushPreset) {
        let keep = self.brush.clone();
        self.brush = b.settings.clone();
        self.brush.size = keep.size;
        self.brush.symmetry = keep.symmetry;
        self.brush.lazy = keep.lazy;
        self.brush.pressure_size = keep.pressure_size;
        self.brush.pressure_flow = keep.pressure_flow;
        let mut note = String::new();
        self.brush_tip = None;
        if let Some(file) = &b.alpha_file {
            match (library::starter_alpha(file), self.doc.as_mut()) {
                (Some(p), Some(doc)) => match doc.import_resource(&self.engine, &p, ResKind::Alpha) {
                    Ok(id) => self.brush.alpha = AlphaSource::Resource(id),
                    Err(e) => note = format!(" (tip not loaded: {e:#})"),
                },
                (None, _) => note = format!(" (tip {file} not found in the starter assets)"),
                (Some(_), None) => {}
            }
            self.brush_tip = Some((file.clone(), self.brush.alpha.clone()));
        }
        self.brush_preset = b.name.clone();
        self.status = format!("Brush: {}{note}", b.name);
    }

    pub(super) fn tex_material_fill(&mut self, m: &library::TexMaterial) -> Option<FillDesc> {
        let doc = self.doc.as_mut()?;
        let mut ids = HashMap::new();
        for (stem, _) in library::TEX_MATERIAL_MAPS {
            let Some(path) = m.map(stem) else { continue };
            let ext = path.extension().map(|x| x.to_string_lossy().into_owned()).unwrap_or_default();
            let name = format!("{} {stem}.{ext}", m.name);
            if let Some(r) = doc.resources.values().find(|r| r.name == name) {
                ids.insert(stem, r.id);
                continue;
            }
            match std::fs::read(&path).map_err(anyhow::Error::from).and_then(|b| doc.add_resource(&self.engine, name, b, ResKind::Texture)) {
                Ok(id) => {
                    ids.insert(stem, id);
                }
                Err(e) => {
                    self.status = format!("Could not load {}: {e:#}", path.display());
                    return None;
                }
            }
        }
        Some(m.fill(&ids))
    }

    fn assign_texture_or_new_fill(&mut self, id: u64, name: String) {
        self.checkpoint("Apply texture");
        let doc = self.doc.as_mut().unwrap();
        let set = doc.active;
        let s = &mut doc.sets[set];
        if s.target == Target::Content {
            if let Some(LayerKind::Fill(f)) = s.selected_layer_mut().map(|l| &mut l.desc.kind) {
                let ch = &mut f.channels[CH_COLOR];
                ch.enabled = true;
                ch.source = SourceKind::Texture;
                ch.texture = Some(id);
                ch.value = [1.0; 3];
                s.dirty = true;
                return;
            }
        }
        let mut f = FillDesc::default();
        f.channels[CH_COLOR] = FillChannel { source: SourceKind::Texture, texture: Some(id), value: [1.0; 3], ..Default::default() };
        f.channels[CH_ROUGH].enabled = false;
        f.channels[CH_METAL].enabled = false;
        let l = doc.new_layer(&self.engine, set, LayerKind::Fill(Box::new(f)), Some(name));
        doc.insert_layer(set, l);
    }

    /// Makes missing thumbnails: files decode on several worker threads
    /// (every finished one shows the same frame), at most two GPU renders
    /// per frame.
    pub(super) fn generate_thumbnails(&mut self, ctx: &egui::Context) {
        // Material spheres are lit by the environment: a new one renders
        // them again.
        if self.shelf.thumb_env != self.engine.env.name {
            self.shelf.thumb_env = self.engine.env.name.clone();
            self.shelf.thumbs.retain(|k, _| !k.starts_with("mat:"));
        }
        if self.shelf.wanted.is_empty() {
            return;
        }
        // Never while the user drags (camera, strokes, sliders): those frames
        // must stay light. Otherwise one per frame.
        if ctx.input(|i| i.pointer.any_down() || i.raw_scroll_delta != egui::Vec2::ZERO) {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
            return;
        }
        let empty = HashMap::new();
        // Waiting for a decoder (or this frame's render budget): back in the queue.
        let mut later = Vec::new();
        let mut renders = 0;
        let max_dec = std::thread::available_parallelism().map_or(2, |n| n.get().saturating_sub(1).clamp(2, 8));
        for _ in 0..32 {
            let Some(item) = self.shelf.wanted.pop() else { break };
            let key = item.key();
            if self.shelf.thumbs.contains_key(&key) {
                self.shelf.wanted_keys.remove(&key);
                continue;
            }
            // Image files decode on a worker (HDR environments take a second);
            // texture materials show their pre-rendered preview.
            type Job = Box<dyn FnOnce() -> Option<image::RgbaImage> + Send>;
            let job: Option<Job> = match &item {
                Item::AlphaFile(p) | Item::Texture(p) | Item::Environment(p) => {
                    let p = p.clone();
                    Some(Box::new(move || load_image_any(&p).map(|i| image::imageops::thumbnail(&i, THUMB, THUMB))))
                }
                Item::TexMaterial(m) => m.preview().map(|p| Box::new(move || load_image_any(&p).map(|i| image::imageops::thumbnail(&i, THUMB, THUMB))) as Job),
                Item::Brush(b) => {
                    let b = b.clone();
                    Some(Box::new(move || Some(b.thumbnail(THUMB))))
                }
                _ => None,
            };
            if let Some(job) = job {
                let ready = self.shelf.decoded_img.lock().ok().and_then(|mut d| d.remove(&key));
                match ready {
                    Some(img) => {
                        self.shelf.decoding.remove(&key);
                        self.shelf.wanted_keys.remove(&key);
                        match img {
                            Some(img) => {
                                self.shelf.image_thumb(ctx, &key, img);
                            }
                            None => {
                                self.shelf.thumbs.insert(key, egui::TextureId::default());
                            }
                        }
                    }
                    None => {
                        if !self.shelf.decoding.contains(&key) && self.shelf.decoding.len() < max_dec {
                            self.shelf.decoding.insert(key.clone());
                            let (out, key) = (self.shelf.decoded_img.clone(), key.clone());
                            std::thread::spawn(move || {
                                let img = job();
                                if let Ok(mut d) = out.lock() {
                                    d.insert(key, img);
                                }
                            });
                        }
                        later.push(item);
                        continue;
                    }
                }
                continue;
            }
            if renders >= 2 {
                later.push(item);
                continue;
            }
            renders += 1;
            if let Item::TexMaterial(m) = &item {
                // Lit by the real environment: wait until it is loaded.
                if self.env_loading.is_some() {
                    later.push(item);
                    continue;
                }
                let ready = self.shelf.decoded.lock().ok().and_then(|mut d| d.remove(&key));
                let Some(maps) = ready else {
                    if !self.shelf.decoding.contains(&key) && self.shelf.decoding.len() < max_dec {
                        self.shelf.decoding.insert(key.clone());
                        let (m, out, key) = (m.clone(), self.shelf.decoded.clone(), key.clone());
                        std::thread::spawn(move || {
                            let maps: Vec<(&'static str, image::RgbaImage)> = library::TEX_MATERIAL_MAPS
                                .iter()
                                .filter_map(|(stem, _)| {
                                    let img = crate::imageio::open(&m.map(stem)?).ok()?;
                                    Some((*stem, img.to_rgba8()))
                                })
                                .collect();
                            if let Ok(mut d) = out.lock() {
                                d.insert(key, maps);
                            }
                        });
                    }
                    // Back in the queue until its maps are decoded.
                    later.push(item);
                    continue;
                };
                self.shelf.decoding.remove(&key);
                self.shelf.wanted_keys.remove(&key);
                let mut resources = HashMap::new();
                let mut ids = HashMap::new();
                for (i, (stem, img)) in maps.into_iter().enumerate() {
                    let id = i as u64 + 1;
                    let gpu = crate::engine::texture::rgba8_with_mips(&self.engine.device, &self.engine.queue, stem, &img);
                    resources.insert(
                        id,
                        Resource { id, name: stem.into(), kind: ResKind::Texture, imported: false, encoded: Default::default(), gpu: std::sync::Arc::new(gpu), image: std::sync::Arc::new(img) },
                    );
                    ids.insert(stem, id);
                }
                // On the small sphere the pattern reads at two repeats.
                let mut fd = m.fill(&ids);
                fd.projection.scale = 2.0;
                let tree = LayerTree { desc: LayerDesc::new(0, m.name.clone(), LayerKind::Fill(Box::new(fd))), children: Vec::new() };
                let pv = self.shelf.previewer_mat.get_or_insert_with(|| Previewer::new_material(&self.engine));
                let tex = pv.render(&self.engine, &tree, &resources);
                let id = self.renderer.write().register_native_texture(&self.engine.device, &tex.view, wgpu::FilterMode::Linear);
                self.shelf.gpu_thumbs.push(tex);
                self.shelf.wanted_keys.remove(&key);
                self.shelf.thumbs.insert(key, id);
                continue;
            }
            if let Item::SmartMaterial(_, tree, Some(dir), files) = &item {
                if !files.is_empty() {
                    let ready = self.shelf.decoded_files.lock().ok().and_then(|mut d| d.remove(&key));
                    let Some(maps) = ready else {
                        if !self.shelf.decoding.contains(&key) && self.shelf.decoding.len() < max_dec {
                            self.shelf.decoding.insert(key.clone());
                            let (dir, files, out, key) = (dir.clone(), files.clone(), self.shelf.decoded_files.clone(), key.clone());
                            std::thread::spawn(move || {
                                // Small copies: the thumbnail does not need more.
                                let maps: Vec<(u64, image::RgbaImage)> = files
                                    .iter()
                                    .filter_map(|(id, f)| {
                                        let img = crate::imageio::open(&dir.join(f)).ok()?.to_rgba8();
                                        Some((*id, if img.width() > 512 { image::imageops::thumbnail(&img, 512, 512) } else { img }))
                                    })
                                    .collect();
                                if let Ok(mut d) = out.lock() {
                                    d.insert(key, maps);
                                }
                            });
                        }
                        later.push(item);
                        continue;
                    };
                    self.shelf.decoding.remove(&key);
                    self.shelf.wanted_keys.remove(&key);
                    let mut resources = HashMap::new();
                    for (id, img) in maps {
                        let gpu = crate::engine::texture::rgba8_with_mips(&self.engine.device, &self.engine.queue, "smart material map", &img);
                        resources.insert(
                            id,
                            Resource { id, name: String::new(), kind: ResKind::Texture, imported: false, encoded: Default::default(), gpu: std::sync::Arc::new(gpu), image: std::sync::Arc::new(img) },
                        );
                    }
                    let pv = self.shelf.previewer_box.get_or_insert_with(|| Previewer::new_box(&self.engine));
                    let tex = pv.render(&self.engine, tree, &resources);
                    let id = self.renderer.write().register_native_texture(&self.engine.device, &tex.view, wgpu::FilterMode::Linear);
                    self.shelf.gpu_thumbs.push(tex);
                    self.shelf.thumbs.insert(key, id);
                    continue;
                }
            }
            let tree = match &item {
                Item::Material(n, f) => Some(LayerTree {
                    desc: LayerDesc::new(0, n.clone(), LayerKind::Fill(Box::new(f.clone()))),
                    children: Vec::new(),
                }),
                Item::SmartMaterial(_, t, _, _) => Some(t.clone()),
                Item::Filter(k) => {
                    let mut checker = FillDesc::material([0.8, 0.3, 0.1], 0.4, 0.0);
                    checker.channels[CH_COLOR] = FillChannel {
                        source: SourceKind::Procedural,
                        procedural: ProcParams::of(ProcKind::Checker),
                        value: [0.08; 3],
                        value_b: [0.85, 0.4, 0.12],
                        ..Default::default()
                    };
                    checker.projection = Projection { mode: ProjMode::Object3D, scale: 3.0, ..Default::default() };
                    Some(LayerTree {
                        desc: LayerDesc::new(0, k.name().into(), LayerKind::Folder),
                        children: vec![
                            LayerTree { desc: LayerDesc::new(1, "checker".into(), LayerKind::Fill(Box::new(checker))), children: Vec::new() },
                            LayerTree { desc: LayerDesc::new(2, k.name().into(), LayerKind::Filter(FilterDesc::of(*k))), children: Vec::new() },
                        ],
                    })
                }
                Item::SmartMask(n, m) => {
                    let mut d = LayerDesc::new(0, n.clone(), LayerKind::Fill(Box::new(FillDesc::material([0.9, 0.9, 0.9], 0.5, 0.0))));
                    d.mask = Some(m.clone());
                    Some(LayerTree { desc: d, children: Vec::new() })
                }
                Item::Procedural(k) if k.is_normal() => {
                    // A normal generator: shown on polished metal.
                    let mut f = FillDesc::material([0.8, 0.8, 0.82], 0.28, 1.0);
                    f.channels[CH_NORMAL] = FillChannel { enabled: true, source: SourceKind::Procedural, procedural: ProcParams::of(*k), ..Default::default() };
                    Some(LayerTree { desc: LayerDesc::new(0, k.name().into(), LayerKind::Fill(Box::new(f))), children: Vec::new() })
                }
                Item::Procedural(k) => {
                    let mut f = FillDesc::default();
                    f.channels[CH_COLOR] = FillChannel {
                        source: SourceKind::Procedural,
                        procedural: ProcParams::of(*k),
                        value: [0.03; 3],
                        value_b: [0.9; 3],
                        ..Default::default()
                    };
                    f.projection = Projection { mode: ProjMode::Object3D, scale: 3.0, ..Default::default() };
                    Some(LayerTree { desc: LayerDesc::new(0, k.name().into(), LayerKind::Fill(Box::new(f))), children: Vec::new() })
                }
                _ => None,
            };
            if let Some(tree) = tree {
                let boxed = matches!(item, Item::SmartMaterial(..) | Item::SmartMask(..));
                let flat = matches!(item, Item::Procedural(_));
                let pv = if flat {
                    self.shelf.previewer_flat.get_or_insert_with(|| Previewer::new_flat(&self.engine))
                } else if boxed {
                    self.shelf.previewer_box.get_or_insert_with(|| Previewer::new_box(&self.engine))
                } else {
                    self.shelf.previewer.get_or_insert_with(|| Previewer::new(&self.engine))
                };
                let tex = pv.render(&self.engine, &tree, &empty);
                let id = self.renderer.write().register_native_texture(&self.engine.device, &tex.view, wgpu::FilterMode::Linear);
                self.shelf.gpu_thumbs.push(tex);
                self.shelf.wanted_keys.remove(&key);
                self.shelf.thumbs.insert(key, id);
                continue;
            }
            self.shelf.wanted_keys.remove(&key);
            let img = match &item {
                Item::Alpha(k) => Some(alpha_image(*k)),
                _ => None,
            };
            if let Some(img) = img {
                self.shelf.image_thumb(ctx, &key, img);
            } else {
                self.shelf.thumbs.insert(key, egui::TextureId::default());
            }
        }
        let waiting = !later.is_empty();
        for it in later.into_iter().rev() {
            self.shelf.wanted.insert(0, it);
        }
        if waiting {
            ctx.request_repaint_after(std::time::Duration::from_millis(15));
        } else {
            ctx.request_repaint();
        }
    }
}

/// Folder of an alpha, texture or brush in the shelf's folder menu.
fn item_folder(it: &Item) -> Option<String> {
    match it {
        Item::Alpha(_) => Some("Brush tips".into()),
        Item::AlphaFile(p) | Item::Texture(p) => Some(library::file_folder(p)),
        Item::Brush(b) if b.folder.is_empty() => Some("Your brushes".into()),
        Item::Brush(b) => Some(b.folder.clone()),
        _ => None,
    }
}

/// Shortens `text` with an ellipsis until it fits `max_w`.
fn elide(p: &egui::Painter, text: &str, font: &egui::FontId, max_w: f32) -> String {
    let mut shown = text.to_string();
    while shown.chars().count() > 4 && p.layout_no_wrap(shown.clone(), font.clone(), Color32::WHITE).size().x > max_w {
        let n = shown.chars().count();
        shown = shown.chars().take(n - 4).collect::<String>() + "...";
    }
    shown
}

/// The library's filters in force as chips with a cross, in the header strip
/// `strip`; the ones that don't fit go under a "+N" chip (with Clear all).
/// Removed filters are pushed to `gone` (0 source, 1 type, 2 folder, 3 search).
fn filter_strip(ui: &mut egui::Ui, strip: Rect, chips: &[(String, u8)], gone: &mut Vec<u8>) {
    if chips.is_empty() {
        return;
    }
    let gap = 4.0;
    let widths: Vec<f32> = chips.iter().map(|(t, _)| super::widgets::filter_chip_width(ui, t)).collect();
    let total = widths.iter().sum::<f32>() + gap * (chips.len() - 1) as f32;
    let (more_w, clear_w) = (34.0, 36.0);
    let mut shown = chips.len();
    if total > strip.width() {
        shown = 0;
        let mut x = 0.0;
        while shown < chips.len() && x + widths[shown] + gap + more_w <= strip.width() {
            x += widths[shown] + gap;
            shown += 1;
        }
    }
    let mut c = ui.new_child(egui::UiBuilder::new().max_rect(strip).layout(egui::Layout::left_to_right(egui::Align::Center)));
    c.spacing_mut().item_spacing.x = gap;
    for (text, k) in &chips[..shown] {
        let r = super::widgets::filter_chip(&mut c, text);
        super::mark(c.ctx(), format!("library filter:{k}"), r.rect);
        if r.clicked() {
            gone.push(*k);
        }
    }
    if shown < chips.len() {
        let more = super::widgets::pill(&mut c, &format!("+{}", chips.len() - shown), false).on_hover_text("More filters in force");
        super::mark(c.ctx(), "library filter:more", more.rect);
        let id = c.id().with("library filters more");
        if more.clicked() {
            c.memory_mut(|m| m.toggle_popup(id));
        }
        super::widgets::popup_below(&c, id, &more, egui::PopupCloseBehavior::CloseOnClickOutside, |ui| {
            ui.set_min_width(160.0);
            for (text, k) in &chips[shown..] {
                if super::widgets::filter_chip(ui, text).clicked() {
                    gone.push(*k);
                }
            }
            ui.separator();
            if ui.button("Clear all filters").clicked() {
                gone.extend([0, 1, 2, 3]);
            }
        });
    } else if chips.len() >= 2 && total + gap + clear_w <= strip.width() {
        let r = c.add(egui::Label::new(RichText::new("Clear").size(11.0).color(crate::style::TEXT2).underline()).sense(egui::Sense::click()));
        super::mark(c.ctx(), "library filter:clear all", r.rect);
        if r.on_hover_text("Show everything: all libraries, all types, no search").clicked() {
            gone.extend([0, 1, 2, 3]);
        }
    }
}
