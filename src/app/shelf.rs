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
    fn name(self) -> &'static str {
        match self {
            Category::Materials => "Materials",
            Category::SmartMaterials => "Smart materials",
            Category::SmartMasks => "Smart masks",
            Category::Brushes => "Brushes",
            Category::Alphas => "Alphas",
            Category::Textures => "Textures",
            Category::Procedurals => "Procedurals",
            Category::Environments => "Environments",
            Category::Filters => "Filters",
            Category::All => "All libraries",
            Category::Project => "Project",
        }
    }
}

/// What a shelf item applies.
#[derive(Clone)]
pub(super) enum Item {
    Material(String, FillDesc),
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
            Item::Material(..) => "Material",
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
    previewer: Option<Previewer>,
    /// Rounded box previewer (smart materials, smart masks).
    previewer_box: Option<Previewer>,
    /// Flat square previewer (procedural textures).
    previewer_flat: Option<Previewer>,
    loaded: bool,
    list_view: bool,
    pub(super) source: Source,
    builtin_brushes: std::collections::HashSet<String>,
    /// Key of the highlighted asset.
    selected: Option<String>,
    /// Procedurals: Painter's shelf folder shown (None: all).
    proc_folder: Option<&'static str>,
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
            previewer: None,
            previewer_box: None,
            previewer_flat: None,
            loaded: false,
            list_view: false,
            source: Source::All,
            builtin_brushes: Default::default(),
            selected: None,
            proc_folder: None,
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
                v.sort_by_key(|r| r.id);
                v.into_iter().map(|r| Item::ProjectTexture(r.id, r.name.clone())).collect()
            })
            .unwrap_or_default();
        let get = |c: Category| self.items.get(&c).cloned().unwrap_or_default();
        let mut items: Vec<Item> = match self.category {
            Category::All => {
                let mut v: Vec<Item> = Category::ALL.iter().flat_map(|c| get(*c)).collect();
                v.extend(project.iter().cloned());
                v
            }
            Category::Project => project.clone(),
            Category::Textures => {
                let mut v = get(Category::Textures);
                v.extend(get(Category::Procedurals));
                v.extend(project.iter().cloned());
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
                if dir.is_some() || n.ends_with("(user)") {
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
            Item::Material(..) | Item::Alpha(_) | Item::Procedural(_) | Item::Filter(_) => Source::Starter,
        }
    }

    /// Queues a thumbnail render for `item` unless it exists or is queued.
    pub(super) fn want(&mut self, item: Item) {
        let key = item.key();
        if !self.thumbs.contains_key(&key) && !self.wanted.iter().any(|w| w.key() == key) {
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

    /// Renders a material preview (layer stack fill thumbnails).
    pub(super) fn preview(&mut self, engine: &crate::engine::Engine, tree: &LayerTree, resources: &HashMap<u64, Resource>) -> crate::engine::Tex {
        let pv = self.previewer.get_or_insert_with(|| Previewer::new(engine));
        pv.render(engine, tree, resources)
    }

    pub fn refresh(&mut self) {
        self.loaded = false;
    }

    fn load(&mut self) {
        if self.loaded {
            return;
        }
        self.loaded = true;
        let mut m = HashMap::new();
        m.insert(
            Category::Materials,
            library::materials().into_iter().map(|x| Item::Material(x.name.into(), x.fill)).collect(),
        );
        let mut sm: Vec<Item> =
            library::smart_materials().into_iter().map(|x| Item::SmartMaterial(x.name.into(), x.tree, None, Vec::new())).collect();
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
        let mut alphas: Vec<Item> = AlphaKind::ALL.iter().map(|k| Item::Alpha(*k)).collect();
        alphas.extend(library::files_with(&library::sub_dir("alphas"), library::IMAGE_EXTS).into_iter().map(Item::AlphaFile));
        m.insert(Category::Alphas, alphas);
        m.insert(
            Category::Textures,
            library::files_with(&library::sub_dir("textures"), library::IMAGE_EXTS).into_iter().map(Item::Texture).collect(),
        );
        // Painter's procedurals in its folder order, the classic ones last.
        let mut procs: Vec<ProcKind> = ProcKind::ALL.to_vec();
        procs.sort_by_key(|k| crate::procedurals::FOLDERS.iter().position(|f| *f == crate::procedurals::folder_of(*k)).unwrap_or(99));
        m.insert(Category::Procedurals, procs.into_iter().map(Item::Procedural).collect());
        m.insert(
            Category::Environments,
            library::files_with(&library::sub_dir("environments"), library::ENV_EXTS)
                .into_iter()
                .chain(library::painter_environments())
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
    let img = image::open(path).ok()?;
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
        {
            {
                egui::Frame::none().inner_margin(egui::Margin::symmetric(6.0, 4.0)).show(ui, |ui| {
                    // Breadcrumb: the arrow opens the library menu.
                    ui.horizontal(|ui| {
                        let ar = icons::button(ui, Icon::Arrow, false, 18.0, "Libraries");
                        let lid = ui.id().with("library menu");
                        if ar.clicked() {
                            ui.memory_mut(|m| m.toggle_popup(lid));
                        }
                        super::widgets::popup_below(ui, lid, &ar, egui::PopupCloseBehavior::CloseOnClick, |ui| {
                            ui.set_min_width(150.0);
                            for src in Source::ALL {
                                let on = self.shelf.source == src;
                                if ui.add(egui::SelectableLabel::new(on, if on { format!("{}   \u{2713}", src.name()) } else { src.name().to_string() })).clicked() {
                                    self.shelf.source = src;
                                }
                            }
                        });
                        ui.label(RichText::new(self.shelf.source.name()).font(crate::style::bold(12.0)));
                    });
                    // Search.
                    ui.horizontal(|ui| {
                        let (r, _) = ui.allocate_exact_size(egui::vec2(16.0, 20.0), egui::Sense::hover());
                        icons::paint(ui.painter(), r, Icon::Search, Color32::from_gray(160));
                        // Fixed width (the clear icon slot is always reserved) so typing never moves the layout.
                        let w = (ui.available_width() - 28.0).max(40.0);
                        ui.add_sized([w, 20.0], egui::TextEdit::singleline(&mut self.shelf.filter).hint_text("Search").frame(false).clip_text(true));
                        if self.shelf.filter.is_empty() {
                            ui.allocate_exact_size(egui::vec2(20.0, 20.0), egui::Sense::hover());
                        } else if icons::button(ui, Icon::Filter, true, 20.0, "Clear the search").clicked() {
                            self.shelf.filter.clear();
                        }
                    });
                    // Wraps on narrow docks instead of widening the panel.
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(2.0, 2.0);
                        // Asset types (single select); clicking the active one shows all.
                        for (c, icon) in [
                            (Category::Materials, Icon::Material),
                            (Category::SmartMaterials, Icon::Sphere),
                            (Category::SmartMasks, Icon::Mask),
                            (Category::Filters, Icon::Effect),
                            (Category::Brushes, Icon::Brush),
                            (Category::Alphas, Icon::Alpha),
                            (Category::Textures, Icon::Grid),
                            (Category::Environments, Icon::Sun),
                        ] {
                            let on = self.shelf.category == c;
                            let tip = if on { format!("{} (click again to show all)", c.name()) } else { c.name().to_string() };
                            if icons::button(ui, icon, on, 22.0, &tip).clicked() {
                                self.shelf.category = if on { Category::All } else { c };
                            }
                        }
                        ui.add_space(6.0);
                        let lv = self.shelf.list_view;
                        if icons::button(ui, if lv { Icon::List } else { Icon::Grid }, false, 22.0, "Thumbnails / list").clicked() {
                            self.shelf.list_view = !lv;
                        }
                    });
                });
                // Bottom icon bar.
                egui::TopBottomPanel::bottom("assets bottom")
                    .exact_height(28.0)
                    .frame(egui::Frame::none().fill(crate::style::HEADER).inner_margin(egui::Margin::symmetric(6.0, 2.0)))
                    .show_inside(ui, |ui| {
                        ui.horizontal_centered(|ui| {
                            ui.spacing_mut().item_spacing.x = 2.0;
                            if icons::button(ui, Icon::Import, false, 22.0, "Import resources (project or library)").clicked() {
                                import = true;
                            }
                            if icons::button(ui, Icon::Refresh, false, 22.0, "Refresh the libraries").clicked() {
                                self.shelf.refresh();
                            }
                            if icons::button(ui, Icon::Plus, false, 22.0, "Save the selected layer as a smart material").clicked() {
                                save_smart = true;
                            }
                            if icons::button(ui, Icon::Folder, false, 22.0, "Open the library folder").clicked() {
                                let _ = std::process::Command::new("explorer").arg(library::library_dir()).spawn();
                            }
                        });
                    });
                // Procedurals: Painter's folders as a filter row.
                if self.shelf.category == Category::Procedurals {
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(4.0, 3.0);
                        if ui.selectable_label(self.shelf.proc_folder.is_none(), "All").clicked() {
                            self.shelf.proc_folder = None;
                        }
                        for f in crate::procedurals::FOLDERS {
                            if ui.selectable_label(self.shelf.proc_folder == Some(f), f).clicked() {
                                self.shelf.proc_folder = Some(f);
                            }
                        }
                    });
                }
                let filter = self.shelf.filter.to_lowercase();
                let folder = self.shelf.proc_folder.filter(|_| self.shelf.category == Category::Procedurals);
                let items: Vec<Item> = self
                    .shelf
                    .browse(self.doc.as_ref())
                    .into_iter()
                    .filter(|i| filter.is_empty() || i.name().to_lowercase().contains(&filter))
                    .filter(|i| match (folder, i) {
                        (Some(f), Item::Procedural(k)) => crate::procedurals::folder_of(*k) == f,
                        _ => true,
                    })
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
                        let cols = if list { 1 } else { ((w / 66.0).floor() as usize).max(2) };
                        let gap = 4.0;
                        let tile = if list { 30.0 } else { ((w - gap * (cols as f32 - 1.0)) / cols as f32).floor() };
                        let row_h = if list { 32.0 } else { tile + 14.0 };
                        for chunk in items.chunks(cols) {
                            let (row, _) = ui.allocate_exact_size(egui::vec2(w, row_h), egui::Sense::hover());
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
                                let resp = ui
                                    .interact(cell, ui.id().with(("asset", &key)), egui::Sense::click_and_drag())
                                    .on_hover_text(format!("{}\n{}", item.name(), item.kind_name()));
                                let selected = self.shelf.selected.as_deref() == Some(key.as_str());
                                let p = ui.painter();
                                if selected {
                                    p.rect_filled(cell, 3.0, crate::style::ROW_SELECTED);
                                } else if resp.hovered() {
                                    p.rect_filled(cell, 3.0, Color32::from_gray(50));
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
                                        p.rect_filled(tr, 4.0, Color32::from_gray(45));
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
                                resp.dnd_set_drag_payload(item.clone());
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
                                        Item::SmartMaterial(_, _, Some(d), _) => Some(d.clone()),
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
                                    ui.separator();
                                    if ui.button("Close").clicked() {
                                        ui.close_menu();
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
                p.rect_filled(r, 4.0, Color32::from_gray(60));
            }
        }
        p.text(r.center_bottom() + egui::vec2(0.0, 11.0), egui::Align2::CENTER_CENTER, item.name(), egui::FontId::proportional(11.0), Color32::WHITE);
    }

    pub(super) fn apply_item(&mut self, item: Item) {
        if self.doc.is_none() && !matches!(item, Item::Brush(_) | Item::Alpha(_) | Item::Environment(_)) {
            self.status = "Open a project first.".into();
            return;
        }
        match item {
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
                let size = self.brush.size;
                self.brush = b.settings;
                self.brush.size = size;
                if matches!(self.tool, Tool::PolygonFill | Tool::Picker) {
                    self.tool = Tool::Paint;
                }
                self.status = format!("Brush: {}", b.name);
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
                        let ch = &mut f.channels[CH_COLOR];
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

    /// Renders up to two missing thumbnails per frame.
    pub(super) fn generate_thumbnails(&mut self, ctx: &egui::Context) {
        if self.shelf.wanted.is_empty() {
            return;
        }
        let empty = HashMap::new();
        for _ in 0..2 {
            let Some(item) = self.shelf.wanted.pop() else { break };
            let key = item.key();
            if self.shelf.thumbs.contains_key(&key) {
                continue;
            }
            let tree = match &item {
                Item::Material(n, f) => Some(LayerTree {
                    desc: LayerDesc::new(0, n.clone(), LayerKind::Fill(Box::new(f.clone()))),
                    children: Vec::new(),
                }),
                Item::SmartMaterial(_, t, dir, _) if dir.is_none() => Some(t.clone()),
                Item::SmartMaterial(_, t, _, _) => {
                    // User smart materials may reference textures that are not
                    // loaded here: preview with them replaced by uniforms.
                    Some(t.clone())
                }
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
                self.shelf.thumbs.insert(key, id);
                continue;
            }
            let img = match &item {
                Item::Alpha(k) => Some(alpha_image(*k)),
                Item::Brush(b) => Some(match &b.settings.alpha {
                    AlphaSource::Builtin(k) => alpha_image(*k),
                    AlphaSource::Resource(_) => alpha_image(AlphaKind::Round),
                }),
                Item::AlphaFile(p) | Item::Texture(p) | Item::Environment(p) => load_image_any(p),
                _ => None,
            };
            if let Some(img) = img {
                self.shelf.image_thumb(ctx, &key, img);
            } else {
                self.shelf.thumbs.insert(key, egui::TextureId::default());
            }
        }
        ctx.request_repaint();
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
