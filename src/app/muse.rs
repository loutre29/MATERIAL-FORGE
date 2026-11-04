//! The Muse panel: texturing by prompt with Meta's Muse models (Meta Model
//! API, https://api.meta.ai/v1). Muse Spark sees the model split into
//! pieces (`pieces`: its connected mesh pieces, alike ones grouped), each in
//! its own color with its number, rendered here on the CPU, with each
//! piece's size and place (part names are often meaningless), and writes
//! looks: a layer stack for a list of pieces (fills with color, patterns,
//! every channel, glass transmission and IOR; library and ambientCG
//! materials; generated textures from Muse Image; masks from generators and
//! patterns; blend modes; filters). The app builds each look in a folder
//! masked to its pieces' polygons. It is a conversation: later messages
//! change only the looks asked for.

use super::dock::panel_header;
use super::icons::{self, Icon};
use super::*;
use crate::document::*;
use base64::Engine as _;
use eframe::egui::{Color32, RichText, Ui};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Duration;

const API: &str = "https://api.meta.ai/v1";
pub(super) const TEXT_MODEL: &str = "muse-spark-1.3";
const IMAGE_MODEL: &str = "muse-image-1.0";

/// Part colors in the views Muse sees (and the swatches in the panel).
const PALETTE: [([u8; 3], &str); 12] = [
    ([230, 25, 75], "red"),
    ([60, 180, 75], "green"),
    ([0, 130, 200], "blue"),
    ([255, 225, 25], "yellow"),
    ([245, 130, 48], "orange"),
    ([145, 30, 180], "purple"),
    ([70, 240, 240], "cyan"),
    ([240, 50, 230], "magenta"),
    ([210, 245, 60], "lime"),
    ([250, 190, 212], "pink"),
    ([0, 128, 128], "teal"),
    ([170, 110, 40], "brown"),
];

/// Part `i`'s color and its name (past 12: darker, then lighter shades).
fn part_color(i: usize) -> ([u8; 3], String) {
    let (c, n) = PALETTE[i % 12];
    match i / 12 {
        0 => (c, n.to_string()),
        1 => (c.map(|v| (v as f32 * 0.5) as u8), format!("dark {n}")),
        _ => (c.map(|v| v / 2 + 128), format!("light {n}")),
    }
}

/// A part of the object Muse textures: one object of a texture set, or the
/// whole set when it holds a single object.
#[derive(Clone)]
pub struct Part {
    set: usize,
    object: Option<u32>,
    name: String,
}

/// What the worker hands back.
enum Msg {
    Status(String),
    Done(Box<TurnOut>),
    Failed(String),
}

/// A reference image attached to a message: R1, R2... in the conversation.
#[derive(Clone)]
struct Ref {
    name: String,
    /// What Muse gets (a JPEG data URL, at most 1024 px).
    url: String,
    /// A copy in `muse/refs` (a layer can use it as a texture).
    path: std::path::PathBuf,
    thumb: egui::TextureHandle,
    /// An image the user imported into the project (sent by itself).
    project: bool,
}

/// A message sent and Muse's answer as it remembers it.
#[derive(Clone)]
struct Turn {
    user: String,
    /// The reference images sent with it (data URLs).
    refs: Vec<String>,
    reply: String,
}

/// What the conversation's first turn found: the instructions (with the
/// model's facts), the views and the material sheet (data URLs) and the
/// pieces.
struct First {
    system: String,
    view: String,
    sheet: String,
    surf: String,
    surfaces: Vec<Surface>,
    pieces: Vec<pieces::Piece>,
}

/// One turn's result: Muse's answer and what it needs (generated textures,
/// downloaded ambientCG materials).
struct TurnOut {
    first: Option<Box<First>>,
    user: String,
    refs: Vec<String>,
    plan: Value,
    images: HashMap<String, image::RgbaImage>,
    /// ambientCG query, (material folder, "Name (Id)").
    downloads: HashMap<String, (std::path::PathBuf, String)>,
    notes: Vec<String>,
    check: bool,
}

/// A message as the panel shows it.
struct ChatLine {
    user: bool,
    text: String,
    notes: Vec<String>,
    /// The reference images sent with it.
    refs: Vec<egui::TextureHandle>,
}

/// Pieces with one layer stack: a folder per texture set they are in.
struct Look {
    pieces: Vec<usize>,
    what: String,
    layers: Value,
    /// (texture set, folder id).
    folders: Vec<(usize, u64)>,
}

/// The conversation: what Muse saw first, what was said, and what is
/// applied now (the looks).
struct Convo {
    /// The style Muse chose (the panel's on Auto).
    style: Option<Style>,
    /// The document it belongs to (`doc_key`).
    doc: String,
    /// Numbered as in the views Muse sees.
    pieces: Vec<pieces::Piece>,
    system: String,
    /// The views, as a data URL (sent with the first message).
    view: String,
    /// The material sheet, as a data URL (sent with the first message).
    sheet: String,
    /// The SURFACES image (a data URL, sent with the first message) and
    /// the surfaces it shows.
    surf: String,
    surfaces: Vec<Surface>,
    turns: Vec<Turn>,
    looks: Vec<Look>,
    /// Every reference image attached so far (R1 first).
    refs: Vec<Ref>,
}

#[derive(Default)]
pub struct Muse {
    prompt: String,
    key: String,
    /// The key being typed in the connect card.
    key_draft: String,
    /// Connect: the key page was opened (the paste step shows).
    connect_opened: bool,
    /// Connect: why the last key was not taken.
    connect_msg: String,
    /// A key being checked with Meta, and the answer to come.
    connecting: Option<(String, mpsc::Receiver<Result<(), String>>)>,
    /// The numbered pieces shown on the model.
    pieces_on: bool,
    piece_view: Option<PieceView>,
    /// The labels' places for one camera (its hash).
    label_cache: std::cell::RefCell<Option<(u64, Vec<(usize, (f32, f32))>)>>,
    /// The texture sets Muse works on (checked in the panel; all at first).
    sets_on: Vec<bool>,
    status: String,
    chat: Vec<ChatLine>,
    convo: Option<Convo>,
    /// Reference images attached to the message being typed.
    attach: Vec<Ref>,
    /// Where the panel is (images dropped there are attached).
    rect: Option<egui::Rect>,
    job: Option<mpsc::Receiver<Msg>>,
    /// Set by Cancel: the worker stops at its next step, its answer is dropped.
    cancel: Option<Arc<AtomicBool>>,
    /// The self check waiting for the turn's layers to be composited.
    check: Option<Check>,
    /// Where each print placed on a surface went (`print_key`: its name,
    /// surface and box on the SURFACES image).
    placed: HashMap<String, (String, usize, [f32; 4])>,
    /// The project's imported images already sent (resource ids).
    files_sent: Vec<u64>,
    /// The texturing style asked in the panel.
    style: Style,
    /// While Muse checks its first pass: the view as it was before (shown
    /// instead of the live one) and Muse's answer, so the user sees one
    /// result, the final one.
    hold: Option<(crate::engine::Tex, egui::TextureId)>,
    pending: Option<ChatLine>,
    /// The studio render of the lighting Muse set, waiting to go back to
    /// it (its path, the user's request).
    light_check: Option<(std::path::PathBuf, String)>,
    /// The light measures of the photos sent with the last lighting request.
    light_facts: String,
    /// The texture sets to prepare (2048 px, baked mesh maps) once Muse
    /// answers with looks: a lighting request alone needs neither.
    prepare: Option<Vec<usize>>,
    /// The request in flight asks for the lighting alone: looks in the
    /// answer are dropped, the materials kept.
    light_only: bool,
    /// The light profile of the photo given for the lighting.
    light_photo: Option<crate::studio::Profile>,
    /// Muse's first lighting, before its check corrects it: the studio, the
    /// camera's pitch and how far its render is from the photo.
    light_first: Option<(crate::studio::Studio, f32, f32)>,
    /// The render of the corrected lighting, compared with the first one.
    light_verify: Option<std::path::PathBuf>,
    /// What the app chose for a print layer of the turn (by layer name):
    /// a face's photo and turn, a wrap's band and arc. Written into the
    /// stored looks, so Muse sees it and the same layer gives the same.
    resolved: HashMap<String, serde_json::Map<String, Value>>,
}

/// The kind of texturing: a product as in its pack shots (flawless, true
/// to its photos), a real object with its life's wear, or a stylized look.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
enum Style {
    #[default]
    Auto,
    Product,
    Realistic,
    Stylized,
}

impl Style {
    const ALL: [Style; 4] = [Style::Auto, Style::Product, Style::Realistic, Style::Stylized];

    fn name(self) -> &'static str {
        match self {
            Style::Auto => "Auto",
            Style::Product => "Product",
            Style::Realistic => "Realistic",
            Style::Stylized => "Stylized",
        }
    }

    fn hint(self) -> &'static str {
        match self {
            Style::Auto => "Muse chooses the style from the object and your request",
            Style::Product => "A product as in its official photos: brand new, flawless, every color and print exactly as your images show",
            Style::Realistic => "A real object in use: photoreal, with the wear of its age and life",
            Style::Stylized => "A game or cartoon look: clean or hand-painted colors",
        }
    }

    fn from_word(s: &str) -> Option<Style> {
        Style::ALL.into_iter().find(|st| st.name().eq_ignore_ascii_case(s.trim()))
    }
}

/// The pieces on the model: each texture set's texels in their piece's
/// color, and every triangle of the model with its piece (u16::MAX: none).
struct PieceView {
    key: String,
    tex: Vec<crate::engine::Tex>,
    tris: Vec<([glam::Vec3; 3], u16)>,
}

impl Muse {
    /// The texture showing set `set` in piece colors, while the pieces show.
    pub(super) fn piece_tex(&self, set: usize) -> Option<&crate::engine::Tex> {
        self.piece_view.as_ref().filter(|_| self.pieces_on).and_then(|v| v.tex.get(set))
    }
}

/// Where each piece's number goes in the view (0..1 across and down): the
/// model rendered small with a depth test, each piece's label on its
/// visible pixel nearest the middle of what it shows (smallest first, so
/// small pieces keep their place). `tris`: every triangle of the model
/// with its piece (u16::MAX: none, it only hides what is behind).
fn piece_label_spots(tris: &[([glam::Vec3; 3], u16)], m: glam::Mat4, aspect: f32) -> Vec<(usize, (f32, f32))> {
    const W: usize = 240;
    let h = ((W as f32 / aspect).round() as usize).clamp(16, 960);
    let mut depth = vec![f32::MAX; W * h];
    let mut seen = vec![u16::MAX; W * h];
    for (v, piece) in tris {
        let c = v.map(|p| m * p.extend(1.0));
        if c.iter().any(|q| q.w <= 1e-6) {
            continue;
        }
        let s = c.map(|q| ((q.x / q.w + 1.0) * 0.5 * W as f32, (1.0 - q.y / q.w) * 0.5 * h as f32, q.z / q.w));
        let area = (s[1].0 - s[0].0) * (s[2].1 - s[0].1) - (s[2].0 - s[0].0) * (s[1].1 - s[0].1);
        if area.abs() < 1e-9 {
            continue;
        }
        let x0 = s.iter().map(|q| q.0).fold(f32::MAX, f32::min).floor().max(0.0) as usize;
        let x1 = (s.iter().map(|q| q.0).fold(f32::MIN, f32::max).ceil().max(0.0) as usize).min(W);
        let y0 = s.iter().map(|q| q.1).fold(f32::MAX, f32::min).floor().max(0.0) as usize;
        let y1 = (s.iter().map(|q| q.1).fold(f32::MIN, f32::max).ceil().max(0.0) as usize).min(h);
        for y in y0..y1 {
            for x in x0..x1 {
                let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                let w0 = ((s[1].0 - px) * (s[2].1 - py) - (s[2].0 - px) * (s[1].1 - py)) / area;
                let w1 = ((s[2].0 - px) * (s[0].1 - py) - (s[0].0 - px) * (s[2].1 - py)) / area;
                let w2 = 1.0 - w0 - w1;
                if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                    continue;
                }
                let z = w0 * s[0].2 + w1 * s[1].2 + w2 * s[2].2;
                let at = y * W + x;
                if z < depth[at] {
                    depth[at] = z;
                    seen[at] = *piece;
                }
            }
        }
    }
    let mut px_of: HashMap<u16, Vec<(f32, f32)>> = HashMap::new();
    for (at, &k) in seen.iter().enumerate().filter(|(_, k)| **k != u16::MAX) {
        px_of.entry(k).or_default().push(((at % W) as f32 + 0.5, (at / W) as f32 + 0.5));
    }
    let mut order: Vec<(u16, Vec<(f32, f32)>)> = px_of.into_iter().filter(|(_, v)| v.len() >= 3).collect();
    order.sort_by_key(|(k, v)| (v.len(), *k));
    order
        .into_iter()
        .map(|(k, pts)| {
            let n = pts.len() as f32;
            let (cx, cy) = (pts.iter().map(|p| p.0).sum::<f32>() / n, pts.iter().map(|p| p.1).sum::<f32>() / n);
            let p = pts.iter().copied().min_by(|a, b| ((a.0 - cx).powi(2) + (a.1 - cy).powi(2)).total_cmp(&((b.0 - cx).powi(2) + (b.1 - cy).powi(2)))).unwrap();
            (k as usize, (p.0 / W as f32, p.1 / h as f32))
        })
        .collect()
}

/// Where Meta gives Model API keys (the connect card opens it).
const KEYS_URL: &str = "https://dev.meta.ai/";

/// Prompts the empty conversation offers.
const EXAMPLES: [&str; 4] = ["An old brass street lantern, clear glass, slightly aged", "Weathered dark wood and black wrought iron", "Match the attached photo as closely as possible", "I want the lighting of the attached photo for my packshot"];

/// A self check to send once the result is on screen.
struct Check {
    frames: u32,
    /// Frames in a row with nothing left to composite.
    quiet: u32,
    /// The user's message it checks.
    user: String,
}

/// Where the API key is kept (this computer only, next to the library).
fn key_path() -> std::path::PathBuf {
    crate::library::library_dir().join("..").join("muse_key.txt")
}

impl Muse {
    pub fn new() -> Self {
        let key = std::env::var("MODEL_API_KEY").ok().filter(|k| !k.trim().is_empty()).or_else(|| std::fs::read_to_string(key_path()).ok()).map(|k| k.trim().to_string()).unwrap_or_default();
        Muse { key, ..Default::default() }
    }
}

/// The parts of the object: the objects of each texture set (several in a
/// set: one part each, told apart by the ID map), or the set itself.
fn parts(doc: &Document) -> Vec<Part> {
    let mut out = Vec::new();
    for (s, set) in doc.sets.iter().enumerate() {
        let mut objs: Vec<u32> = doc.mesh.submeshes.get(set.submesh).map(|m| m.tri_objects.clone()).unwrap_or_default();
        objs.sort_unstable();
        objs.dedup();
        let object_name = |o: u32| doc.mesh.objects.get(o as usize).cloned().unwrap_or_else(|| format!("Part {o}"));
        if objs.len() > 1 {
            out.extend(objs.into_iter().map(|o| Part { set: s, object: Some(o), name: object_name(o) }));
        } else {
            out.push(Part { set: s, object: None, name: set.name.clone() });
        }
    }
    out
}

/// The parts in scope as `pieces::find` takes them.
fn part_list(doc: &Document, all: &[Part], scope: &[usize]) -> Vec<(usize, usize, Option<u32>, String)> {
    scope.iter().map(|&i| &all[i]).map(|p| (p.set, doc.sets[p.set].submesh, p.object, p.name.clone())).collect()
}

// ------------------------------------------------------------------ the views

/// Digits 0 to 9 in a 3x5 pixel font (rows top first, 3 bits each).
const DIGITS: [[u8; 5]; 10] = [
    [7, 5, 5, 5, 7],
    [2, 6, 2, 2, 7],
    [7, 1, 7, 4, 7],
    [7, 1, 7, 1, 7],
    [5, 5, 7, 1, 1],
    [7, 4, 7, 1, 7],
    [7, 4, 7, 5, 7],
    [7, 1, 1, 1, 1],
    [7, 5, 7, 5, 7],
    [7, 5, 7, 1, 7],
];

/// Writes `n` centered on (cx, cy): white digits on a dark box.
fn draw_number(img: &mut image::RgbaImage, cx: i32, cy: i32, n: usize) {
    const K: i32 = 3;
    let text = n.to_string();
    let w = text.len() as i32 * 4 * K + K;
    let h = 7 * K;
    let (x0, y0) = (cx - w / 2, cy - h / 2);
    let mut put = |x: i32, y: i32, c: [u8; 3]| {
        if x >= 0 && y >= 0 && (x as u32) < img.width() && (y as u32) < img.height() {
            img.put_pixel(x as u32, y as u32, image::Rgba([c[0], c[1], c[2], 255]));
        }
    };
    for y in 0..h {
        for x in 0..w {
            put(x0 + x, y0 + y, [12, 12, 14]);
        }
    }
    for (i, ch) in text.bytes().enumerate() {
        let rows = DIGITS[(ch - b'0') as usize];
        for (r, bits) in rows.iter().enumerate() {
            for col in 0..3 {
                if bits & (4 >> col) != 0 {
                    for dy in 0..K {
                        for dx in 0..K {
                            put(x0 + K + (i as i32 * 4 + col) * K + dx, y0 + K + r as i32 * K + dy, [255, 255, 255]);
                        }
                    }
                }
            }
        }
    }
}

/// What Muse sees: the model from four sides, each piece flat in its
/// palette color with its number on it (lit from the upper left), the rest
/// gray; and a line per piece on what it is, its size and place.
fn look_at(mesh: &crate::mesh::MeshData, pieces: &[pieces::Piece]) -> (image::RgbaImage, String) {
    use glam::Vec3;
    // Every triangle, with its piece when it has one.
    let mut of: HashMap<(usize, u32), usize> = HashMap::new();
    for (i, p) in pieces.iter().enumerate() {
        for &t in &p.tris {
            of.insert((p.sub, t), i);
        }
    }
    let mut tris: Vec<([Vec3; 3], Option<usize>)> = Vec::new();
    for (sm, sub) in mesh.submeshes.iter().enumerate() {
        for (t, idx) in sub.indices.chunks_exact(3).enumerate() {
            let v = |k: usize| Vec3::from(mesh.vertices[idx[k] as usize].pos);
            tris.push(([v(0), v(1), v(2)], of.get(&(sm, t as u32)).copied()));
        }
    }
    let (min, max) = (mesh.bbox_min, mesh.bbox_max);
    let center = (min + max) * 0.5;
    let radius = ((max - min).length() * 0.5).max(1e-6);
    let ext = (max - min).max(Vec3::splat(1e-6));
    // Coordinates as the Position gradient generator reads them
    // (`bake::position_map`): y 0..1 over the height, x and z over the
    // longest side.
    let long = ext.max_element();
    let gpos = |p: Vec3| {
        let q = (p - min) / long;
        Vec3::new(q.x, q.y * long / ext.y, q.z)
    };
    let span = gpos(max);
    let mut info = format!("Model: {} pieces, numbered 0 to {}; ", pieces.len(), pieces.len().saturating_sub(1)) + &format!("y is up. Position gradient coordinates span x 0 to {:.2}, y 0 to 1, z 0 to {:.2}.\n", span.x, span.z);
    let total_area: f32 = pieces.iter().map(|p| p.area).sum::<f32>().max(1e-9);
    for (i, p) in pieces.iter().enumerate() {
        let size = (p.hi - p.lo) / ext * 100.0;
        let c = gpos((p.lo + p.hi) * 0.5);
        let alike = if p.count > 1 { format!("{} alike mesh pieces", p.count) } else { "one mesh piece".to_string() };
        info += &format!(
            "Piece {i}: {} in the views; part \"{}\"; {alike}; {} triangles; size {:.0}% x {:.0}% y {:.0}% z of the model; center at x {:.2} y {:.2} z {:.2}; spans y {:.2} to {:.2}; {:.0}% of the surface.\n",
            part_color(i).1,
            p.part,
            p.tris.len(),
            size.x,
            size.y,
            size.z,
            c.x,
            c.y,
            c.z,
            gpos(p.lo).y,
            gpos(p.hi).y,
            p.area / total_area * 100.0,
        );
    }
    let views = views_of(mesh);
    let (cols, cw, ch) = (views.cols, views.cw, views.ch);
    let rows = 4 / cols;
    info += &format!("The numbered views image is {} x {} pixels: four views of {cw} x {ch} pixels, {cols} per row (decal boxes are in these pixels).\n", cw * cols, ch * rows);
    let mut img = image::RgbaImage::from_pixel(cw * cols, ch * rows, image::Rgba([28, 28, 30, 255]));
    for f in &views.frames {
        let (rot, ox, oy, mid, fit) = (f.rot, f.ox, f.oy, f.mid, f.fit);
        let n_px = (cw * ch) as usize;
        let mut depth = vec![f32::MAX; n_px];
        // The piece seen at each pixel (u16::MAX: none).
        let mut seen = vec![u16::MAX; n_px];
        let light = Vec3::new(-0.4, 0.7, 0.6).normalize();
        for (v, piece) in &tris {
            let p = v.map(|q| rot * ((q - center) / radius));
            let n = (p[1] - p[0]).cross(p[2] - p[0]);
            if n.length_squared() < 1e-20 {
                continue;
            }
            let shade = 0.35 + 0.65 * n.normalize().dot(light).abs();
            let base = match piece {
                Some(k) => part_color(*k).0.map(|c| c as f32),
                None => [120.0; 3],
            };
            let col = image::Rgba([(base[0] * shade) as u8, (base[1] * shade) as u8, (base[2] * shade) as u8, 255]);
            let sx = |q: Vec3| ((q.x - mid.x) * fit + cw as f32 * 0.5, ch as f32 * 0.5 - (q.y - mid.y) * fit);
            let s = p.map(sx);
            let (x0, x1) = (s.iter().map(|q| q.0).fold(f32::MAX, f32::min).max(0.0) as i32, s.iter().map(|q| q.0).fold(f32::MIN, f32::max).min(cw as f32 - 1.0) as i32);
            let (y0, y1) = (s.iter().map(|q| q.1).fold(f32::MAX, f32::min).max(0.0) as i32, s.iter().map(|q| q.1).fold(f32::MIN, f32::max).min(ch as f32 - 1.0) as i32);
            let area = (s[1].0 - s[0].0) * (s[2].1 - s[0].1) - (s[2].0 - s[0].0) * (s[1].1 - s[0].1);
            if area.abs() < 1e-8 {
                continue;
            }
            for y in y0..=y1 {
                for x in x0..=x1 {
                    let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                    let w0 = ((s[1].0 - px) * (s[2].1 - py) - (s[2].0 - px) * (s[1].1 - py)) / area;
                    let w1 = ((s[2].0 - px) * (s[0].1 - py) - (s[0].0 - px) * (s[2].1 - py)) / area;
                    let w2 = 1.0 - w0 - w1;
                    if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                        continue;
                    }
                    // Toward the viewer is -z after the rotation.
                    let z = w0 * p[0].z + w1 * p[1].z + w2 * p[2].z;
                    let at = (y as u32 * cw + x as u32) as usize;
                    if -z < depth[at] {
                        depth[at] = -z;
                        seen[at] = piece.map_or(u16::MAX, |k| k as u16);
                        img.put_pixel(ox + x as u32, oy + y as u32, col);
                    }
                }
            }
        }
        // Each visible piece's number on one of its pixels, as near the
        // middle of what is visible as the labels already placed allow
        // (the smallest pieces choose first).
        let mut px_of: HashMap<u16, Vec<(f32, f32)>> = HashMap::new();
        for (at, &k) in seen.iter().enumerate().filter(|(_, k)| **k != u16::MAX) {
            px_of.entry(k).or_default().push(((at as u32 % cw) as f32, (at as u32 / cw) as f32));
        }
        let mut order: Vec<(u16, Vec<(f32, f32)>)> = px_of.into_iter().filter(|(_, v)| v.len() >= 25).collect();
        order.sort_by_key(|(k, v)| (v.len(), *k));
        let mut placed: Vec<egui::Rect> = Vec::new();
        for (k, mut pts) in order {
            let n = pts.len() as f32;
            let (cx, cy) = (pts.iter().map(|p| p.0).sum::<f32>() / n, pts.iter().map(|p| p.1).sum::<f32>() / n);
            pts.sort_by(|a, b| ((a.0 - cx).powi(2) + (a.1 - cy).powi(2)).total_cmp(&((b.0 - cx).powi(2) + (b.1 - cy).powi(2))));
            let w = k.to_string().len() as f32 * 12.0 + 3.0;
            let free = pts.iter().step_by(3).map(|&(x, y)| egui::Rect::from_center_size(egui::pos2(x, y), egui::vec2(w + 2.0, 23.0))).find(|r| !placed.iter().any(|q| q.intersects(*r)));
            if let Some(r) = free {
                placed.push(r);
                draw_number(&mut img, ox as i32 + r.center().x as i32, oy as i32 + r.center().y as i32, k as usize);
            }
        }
    }
    (img, info)
}

// ----------------------------------------------------------------- prints

/// The framing of the four views of the numbered image (`look_at`): each
/// view's rotation (the viewer is on +z after it), the middle of what it
/// frames, its pixels per unit and its cell's corner.
struct ViewFrame {
    rot: glam::Mat3,
    mid: glam::Vec2,
    fit: f32,
    ox: u32,
    oy: u32,
}

struct Views {
    frames: Vec<ViewFrame>,
    /// Cell width and height, columns.
    cw: u32,
    ch: u32,
    cols: u32,
    center: glam::Vec3,
    radius: f32,
}

/// The four orthographic views: a 2x2 grid, or a row of four tall ones for
/// a tall model (it would fill a third of a square view).
fn views_of(mesh: &crate::mesh::MeshData) -> Views {
    use glam::{Mat3, Vec2, Vec3};
    let (min, max) = (mesh.bbox_min, mesh.bbox_max);
    let center = (min + max) * 0.5;
    let radius = ((max - min).length() * 0.5).max(1e-6);
    let views = [(30.0f32, 20.0f32), (120.0, 15.0), (210.0, 20.0), (300.0, 55.0)];
    let rots: Vec<Mat3> = views.iter().map(|(yaw, pitch)| Mat3::from_rotation_x(pitch.to_radians()) * Mat3::from_rotation_y(yaw.to_radians())).collect();
    let bounds = |rot: Mat3| {
        let (mut l, mut h) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
        for sub in &mesh.submeshes {
            for &i in &sub.indices {
                let r = rot * ((Vec3::from(mesh.vertices[i as usize].pos) - center) / radius);
                l = l.min(r.truncate());
                h = h.max(r.truncate());
            }
        }
        (l, h)
    };
    let (l0, h0) = bounds(rots[0]);
    let tall = (h0.y - l0.y) > 1.6 * (h0.x - l0.x);
    let (cols, cw, ch) = if tall { (4u32, 400u32, 800u32) } else { (2, 512, 512) };
    let frames = rots
        .iter()
        .enumerate()
        .map(|(vi, &rot)| {
            // Fit the model to the view.
            let (l, h) = bounds(rot);
            let fit = (cw as f32 * 0.92 / (h.x - l.x).max(1e-6)).min(ch as f32 * 0.92 / (h.y - l.y).max(1e-6));
            ViewFrame { rot, mid: (l + h) * 0.5, fit, ox: (vi as u32 % cols) * cw, oy: (vi as u32 / cols) * ch }
        })
        .collect();
    Views { frames, cw, ch, cols, center, radius }
}

impl Views {
    /// The view a pixel of the image is in.
    fn view_at(&self, x: f32, y: f32) -> Option<usize> {
        if x < 0.0 || y < 0.0 {
            return None;
        }
        let (c, r) = ((x / self.cw as f32) as u32, (y / self.ch as f32) as u32);
        let vi = (r * self.cols + c) as usize;
        (c < self.cols && vi < self.frames.len()).then_some(vi)
    }

    /// The ray of a pixel in view `vi`, in mesh space: a point on it and
    /// its direction toward the viewer.
    fn ray(&self, vi: usize, x: f32, y: f32) -> (glam::Vec3, glam::Vec3) {
        let f = &self.frames[vi];
        let (px, py) = (x - f.ox as f32, y - f.oy as f32);
        let q = glam::Vec3::new((px - self.cw as f32 * 0.5) / f.fit + f.mid.x, (self.ch as f32 * 0.5 - py) / f.fit + f.mid.y, 0.0);
        let back = f.rot.transpose();
        (self.center + back * q * self.radius, back * glam::Vec3::Z)
    }

    /// The visible point of the model under a pixel of view `vi`, when it
    /// is on one of the pieces flagged in `tris` (every triangle, flagged
    /// when it is on them): the point and its face's normal, turned toward
    /// the viewer.
    fn hit(&self, vi: usize, tris: &[([glam::Vec3; 3], glam::Vec3, bool)], x: f32, y: f32) -> Option<(glam::Vec3, glam::Vec3)> {
        let (o, d) = self.ray(vi, x, y);
        let mut best: Option<(f32, glam::Vec3, glam::Vec3, bool)> = None;
        for (v, n, on) in tris {
            // Moller-Trumbore, the ray both ways (it starts inside the model).
            let (e1, e2) = (v[1] - v[0], v[2] - v[0]);
            let p = d.cross(e2);
            let det = e1.dot(p);
            if det.abs() < 1e-12 {
                continue;
            }
            let s = o - v[0];
            let u = s.dot(p) / det;
            let q = s.cross(e1);
            let w = d.dot(q) / det;
            if u < 0.0 || w < 0.0 || u + w > 1.0 {
                continue;
            }
            let t = e2.dot(q) / det;
            if best.as_ref().map_or(true, |b| t > b.0) {
                best = Some((t, o + d * t, *n, *on));
            }
        }
        best.filter(|b| b.3).map(|(_, p, n, _)| (p, if n.dot(d) < 0.0 { -n } else { n }))
    }
}

/// The triangles of some pieces with their normals (the face's, turned the
/// way its vertex normals point).
fn piece_tris(mesh: &crate::mesh::MeshData, pieces: &[pieces::Piece], ids: &[usize]) -> Vec<([glam::Vec3; 3], glam::Vec3)> {
    use glam::Vec3;
    let mut out = Vec::new();
    for &i in ids {
        let Some(p) = pieces.get(i) else { continue };
        let Some(sm) = mesh.submeshes.get(p.sub) else { continue };
        for &t in &p.tris {
            let k = |j: usize| sm.indices.get(t as usize * 3 + j).map(|&x| &mesh.vertices[x as usize]);
            let (Some(a), Some(b), Some(c)) = (k(0), k(1), k(2)) else { continue };
            let v = [Vec3::from(a.pos), Vec3::from(b.pos), Vec3::from(c.pos)];
            let n = (v[1] - v[0]).cross(v[2] - v[0]);
            let vn = Vec3::from(a.normal) + Vec3::from(b.normal) + Vec3::from(c.normal);
            out.push((v, if n.dot(vn) < 0.0 { -n } else { n }));
        }
    }
    out
}

/// Every triangle of the model with its normal, flagged when it is on the
/// pieces `ids`.
fn model_tris(mesh: &crate::mesh::MeshData, pieces: &[pieces::Piece], ids: &[usize]) -> Vec<([glam::Vec3; 3], glam::Vec3, bool)> {
    use glam::Vec3;
    let on: std::collections::HashSet<(usize, u32)> = ids.iter().filter_map(|&i| pieces.get(i)).flat_map(|p| p.tris.iter().map(move |&t| (p.sub, t))).collect();
    let mut out = Vec::new();
    for (si, sm) in mesh.submeshes.iter().enumerate() {
        for (t, idx) in sm.indices.chunks_exact(3).enumerate() {
            let k = |j: usize| &mesh.vertices[idx[j] as usize];
            let v = [Vec3::from(k(0).pos), Vec3::from(k(1).pos), Vec3::from(k(2).pos)];
            let n = (v[1] - v[0]).cross(v[2] - v[0]);
            out.push((v, n, on.contains(&(si, t as u32))));
        }
    }
    out
}

/// The look's own axes and its bounds on them: the world axes, unless
/// most of its surface faces other perpendicular ways (a box turned on
/// the table): then the two ways most of it faces, named after the world
/// axes nearest them.
fn look_frame(mesh: &crate::mesh::MeshData, pieces: &[pieces::Piece], ids: &[usize]) -> Option<([glam::Vec3; 3], glam::Vec3, glam::Vec3)> {
    use glam::Vec3;
    let tris = piece_tris(mesh, pieces, ids);
    let mut ns: Vec<(Vec3, f32)> = tris.iter().map(|t| (t.1.normalize_or_zero(), t.1.length() * 0.5)).filter(|t| t.1 > 0.0).collect();
    if ns.is_empty() {
        return None;
    }
    let total: f32 = ns.iter().map(|t| t.1).sum();
    let along = |d: Vec3, list: &[(Vec3, f32)]| list.iter().filter(|t| t.0.dot(d).abs() > 0.99).map(|t| t.1).sum::<f32>();
    let world = [Vec3::X, Vec3::Y, Vec3::Z];
    let axes = if world.iter().map(|&a| along(a, &ns)).sum::<f32>() >= 0.6 * total {
        world
    } else {
        ns.sort_by(|a, b| b.1.total_cmp(&a.1));
        let big: Vec<(Vec3, f32)> = ns.iter().take(20000).copied().collect();
        let cand: Vec<Vec3> = big.iter().take(300).map(|t| t.0).collect();
        let a = cand.iter().copied().max_by(|&x, &y| along(x, &big).total_cmp(&along(y, &big)))?;
        let b = cand
            .iter()
            .copied()
            .filter(|c| c.dot(a).abs() < 0.1)
            .map(|c| (c - a * c.dot(a)).normalize_or_zero())
            .max_by(|&x, &y| along(x, &big).total_cmp(&along(y, &big)))
            .unwrap_or_else(|| a.any_orthonormal_vector());
        let mut left = vec![a, b, a.cross(b)];
        let mut out = [Vec3::ZERO; 3];
        for (i, w) in world.iter().enumerate().take(2) {
            let j = (0..left.len()).max_by(|&x, &y| left[x].dot(*w).abs().total_cmp(&left[y].dot(*w).abs()))?;
            let v = left.remove(j);
            out[i] = if v.dot(*w) < 0.0 { -v } else { v };
        }
        // Right handed, so prints are never mirrored.
        out[2] = out[0].cross(out[1]);
        out
    };
    let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    for (v, _) in &tris {
        for p in v {
            let q = Vec3::new(p.dot(axes[0]), p.dot(axes[1]), p.dot(axes[2]));
            lo = lo.min(q);
            hi = hi.max(q);
        }
    }
    Some((axes, lo, hi))
}

const SIDE_NAMES: [&str; 6] = ["+x", "-x", "+y (top)", "-y (bottom)", "+z", "-z"];

/// A side named as in the SIDES facts ("+x", "-y", "top", "bottom").
fn side_index(v: &Value) -> Option<usize> {
    let s = v.as_str()?.trim().to_lowercase();
    match s.split_whitespace().next()? {
        "+x" | "x" | "x+" => Some(0),
        "-x" | "x-" => Some(1),
        "+y" | "y" | "y+" | "top" => Some(2),
        "-y" | "y-" | "bottom" => Some(3),
        "+z" | "z" | "z+" => Some(4),
        "-z" | "z-" => Some(5),
        _ => None,
    }
}

/// A print over side `side` of the look (the face of its bounding box
/// that way), on every piece seen there: all of it, or `area` (its share
/// of the side, [x0, y0, x1, y1] from the top left, 0 to 1). `aspect` 0
/// fills the area; otherwise the print keeps it inside.
fn side_decal(mesh: &crate::mesh::MeshData, pieces: &[pieces::Piece], ids: &[usize], side: usize, area: Option<[f32; 4]>, aspect: f32) -> Option<Projection> {
    let (axes, lo, hi) = look_frame(mesh, pieces, ids)?;
    let s = photo::sides(axes, lo, hi)[side];
    let size = s.hi - s.lo;
    let a = area.unwrap_or([0.0, 0.0, 1.0, 1.0]).map(|x| x.clamp(0.0, 1.0));
    let (x0, x1, y0, y1) = (a[0].min(a[2]), a[0].max(a[2]), a[1].min(a[3]), a[1].max(a[3]));
    let (mut w, mut h) = ((x1 - x0) * size.x, (y1 - y0) * size.y);
    if w < 1e-6 || h < 1e-6 {
        return None;
    }
    if aspect <= 0.0 {
    } else if w / h > aspect {
        w = h * aspect;
    } else {
        h = w / aspect.max(1e-3);
    }
    let rc = s.lo.x + (x0 + x1) * 0.5 * size.x;
    let uc = s.hi.y - (y0 + y1) * 0.5 * size.y;
    // From just outside the side to 15% of the look's depth that way:
    // the outer face, with its folds and flaps, not the inside.
    let long = (mesh.bbox_max - mesh.bbox_min).max_element().max(1e-6);
    let deep = (hi - lo)[side / 2];
    let (din, dout) = (0.15 * deep + 0.005 * long, 0.02 * long);
    let dc = s.plane + (dout - din) * 0.5;
    Some(decal_projection(mesh, s.right * rc + s.up * uc + s.out * dc, s.right, s.up, s.out, [w, h, din + dout]))
}

/// The eigenvector of the smallest eigenvalue of a symmetric 3 x 3 matrix
/// (Jacobi rotations).
fn least_axis(m: [[f32; 3]; 3]) -> glam::Vec3 {
    let mut a = m.map(|r| r.map(|x| x as f64));
    let mut v = [[1.0f64, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    for _ in 0..32 {
        let (mut p, mut q, mut big) = (0, 1, 0.0);
        for i in 0..3 {
            for j in i + 1..3 {
                if a[i][j].abs() > big {
                    (p, q, big) = (i, j, a[i][j].abs());
                }
            }
        }
        if big < 1e-12 {
            break;
        }
        let theta = 0.5 * (2.0 * a[p][q]).atan2(a[q][q] - a[p][p]);
        let (c, sn) = (theta.cos(), theta.sin());
        for k in 0..3 {
            let (akp, akq) = (a[k][p], a[k][q]);
            a[k][p] = c * akp - sn * akq;
            a[k][q] = sn * akp + c * akq;
        }
        for k in 0..3 {
            let (apk, aqk) = (a[p][k], a[q][k]);
            a[p][k] = c * apk - sn * aqk;
            a[q][k] = sn * apk + c * aqk;
        }
        for row in v.iter_mut() {
            let (vp, vq) = (row[p], row[q]);
            row[p] = c * vp - sn * vq;
            row[q] = sn * vp + c * vq;
        }
    }
    let k = (0..3).min_by(|&i, &j| a[i][i].total_cmp(&a[j][j])).unwrap_or(0);
    glam::Vec3::new(v[0][k] as f32, v[1][k] as f32, v[2][k] as f32).normalize_or_zero()
}

/// A print going around a round look (a label on a bottle, a jar, a can):
/// around the axis its pieces are round about (the way their surface
/// never faces), its middle facing side `facing`, between `from` and `to`
/// (0 the bottom, 1 the top along the axis), over `arc` degrees. Left
/// out: the band is the straight part of the body (where it is widest);
/// a piece that only partly goes around (a label of its own) gives its
/// own facing and arc; else "to" and "arc" follow the print's proportions
/// (`aspect`, width / height). Returns the projection and what was taken.
fn wrap_decal(mesh: &crate::mesh::MeshData, pieces: &[pieces::Piece], ids: &[usize], w: &Value, aspect: f32) -> Option<(Projection, String, Value)> {
    use glam::{Vec2, Vec3};
    let tris = piece_tris(mesh, pieces, ids);
    if tris.is_empty() {
        return None;
    }
    let world = [Vec3::X, Vec3::Y, Vec3::Z];
    // The axis: given, or the way the surface never faces (snapped to a
    // world axis near it), pointing up.
    let axis = match w["axis"].as_str().map(|s| s.trim().trim_start_matches(['+', '-']).to_lowercase()) {
        Some(s) if s == "x" => Vec3::X,
        Some(s) if s == "y" => Vec3::Y,
        Some(s) if s == "z" => Vec3::Z,
        _ => {
            let mut m = [[0.0f32; 3]; 3];
            for (_, n) in &tris {
                let area = n.length();
                let u = n.normalize_or_zero();
                for i in 0..3 {
                    for j in 0..3 {
                        m[i][j] += area * u[i] * u[j];
                    }
                }
            }
            let ax = least_axis(m);
            let near = world.iter().copied().max_by(|a, b| a.dot(ax).abs().total_cmp(&b.dot(ax).abs())).unwrap_or(Vec3::Y);
            let ax = if near.dot(ax).abs() > 0.985 { near } else { ax };
            if ax.dot(Vec3::Y) < 0.0 || (ax.dot(Vec3::Y).abs() < 1e-3 && ax.dot(Vec3::X) + ax.dot(Vec3::Z) < 0.0) {
                -ax
            } else {
                ax
            }
        }
    };
    let e1 = axis.any_orthonormal_vector();
    let e2 = axis.cross(e1);
    let pts: Vec<(Vec2, f32)> = tris.iter().flat_map(|t| t.0).map(|p| (Vec2::new(p.dot(e1), p.dot(e2)), p.dot(axis))).collect();
    let (h0, h1) = pts.iter().fold((f32::MAX, f32::MIN), |(a, b), p| (a.min(p.1), b.max(p.1)));
    let height = (h1 - h0).max(1e-6);
    // The circle the points lie on (least squares: x^2 + y^2 + D x + E y
    // + F = 0).
    let mut ata = [[0.0f64; 3]; 3];
    let mut atb = [0.0f64; 3];
    for (q, _) in &pts {
        let row = [q.x as f64, q.y as f64, 1.0];
        let rhs = -((q.x * q.x + q.y * q.y) as f64);
        for i in 0..3 {
            for j in 0..3 {
                ata[i][j] += row[i] * row[j];
            }
            atb[i] += row[i] * rhs;
        }
    }
    let det = |m: [[f64; 3]; 3]| m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1]) - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0]) + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
    let d = det(ata);
    if d.abs() < 1e-18 {
        return None;
    }
    let col = |k: usize| {
        let mut m = ata;
        for i in 0..3 {
            m[i][k] = atb[i];
        }
        det(m) / d
    };
    let (dd, ee) = (col(0), col(1));
    let center = Vec2::new((-dd / 2.0) as f32, (-ee / 2.0) as f32);
    // The radius along the axis (64 slices: the farthest point), to find
    // the straight body.
    const N: usize = 64;
    let mut slab = vec![-1.0f32; N];
    for (q, h) in &pts {
        let k = (((h - h0) / height * N as f32) as usize).min(N - 1);
        slab[k] = slab[k].max((*q - center).length());
    }
    // Slices with no vertex (a low poly body has few rings): between the
    // filled ones around them.
    let filled: Vec<usize> = (0..N).filter(|&k| slab[k] >= 0.0).collect();
    for k in 0..N {
        if slab[k] < 0.0 {
            let below = filled.iter().rev().find(|&&f| f < k).copied();
            let above = filled.iter().find(|&&f| f > k).copied();
            slab[k] = match (below, above) {
                (Some(b), Some(a)) => slab[b] + (slab[a] - slab[b]) * (k - b) as f32 / (a - b) as f32,
                (Some(b), None) => slab[b],
                (None, Some(a)) => slab[a],
                (None, None) => 0.0,
            };
        }
    }
    let widest = slab.iter().copied().fold(0.0f32, f32::max).max(1e-6);
    if std::env::var("MF_MUSE_DUMP").is_ok() {
        eprintln!("wrap: axis {axis:?} center {center:?} height {height} slabs {:?}", slab.iter().map(|r| (r / widest * 100.0).round() as i32).collect::<Vec<_>>());
    }
    let (mut s0, mut s1, mut best) = (0, N - 1, 0);
    let mut k = 0;
    while k < N {
        if slab[k] >= 0.95 * widest {
            let start = k;
            while k < N && slab[k] >= 0.95 * widest {
                k += 1;
            }
            if k - start > best {
                (s0, s1, best) = (start, k - 1, k - start);
            }
        } else {
            k += 1;
        }
    }
    // Facing: a side across the axis; else the way a partial piece faces;
    // else the first view's side.
    let mean_n: Vec3 = tris.iter().map(|t| t.1).sum::<Vec3>();
    let total: f32 = tris.iter().map(|t| t.1.length()).sum::<f32>().max(1e-9);
    let across = |v: Vec3| (v - axis * v.dot(axis)).normalize_or_zero();
    let facing = side_index(&w["facing"]).map(|i| world[i / 2] * if i % 2 == 0 { 1.0 } else { -1.0 }).map(across).filter(|v| *v != Vec3::ZERO);
    let partial = (across(mean_n).length() > 0.0 && (mean_n - axis * mean_n.dot(axis)).length() > 0.3 * total).then(|| across(mean_n));
    let views = views_of(mesh);
    let first = views.frames.first().map(|fr| fr.rot.transpose() * Vec3::Z).unwrap_or(Vec3::Z);
    let out = facing.or(partial).unwrap_or_else(|| across(first));
    if out == Vec3::ZERO {
        return None;
    }
    let right = axis.cross(out);
    // How far around the pieces go, seen from the axis (a partial piece).
    let o2 = Vec2::new(out.dot(e1), out.dot(e2));
    let r2 = Vec2::new(right.dot(e1), right.dot(e2));
    let (a0, a1) = pts.iter().fold((f32::MAX, f32::MIN), |(lo, hi), (q, _)| {
        let v = *q - center;
        let ang = v.dot(r2).atan2(v.dot(o2));
        (lo.min(ang), hi.max(ang))
    });
    let span = (a1 - a0).to_degrees();
    let f = |key: &str| w[key].as_f64().map(|v| (v as f32).clamp(0.0, 1.0));
    let (mut from, mut to) = match (f("from"), f("to")) {
        (Some(a0), Some(a1)) => (a0.min(a1), a0.max(a1)),
        (Some(a0), None) => (a0, -1.0),
        (None, Some(a1)) => (-1.0, a1),
        // A label piece of its own: all of it; a body: its straight part,
        // a little in from its ends.
        (None, None) if partial.is_some() => (0.0, 1.0),
        (None, None) => {
            let (b0, b1) = (s0 as f32 / N as f32, (s1 + 1) as f32 / N as f32);
            let m = (b1 - b0) * 0.04;
            (b0 + m, b1 - m)
        }
    };
    let arc_given = w["arc"].as_f64().map(|v| (v as f32).clamp(5.0, 360.0)).or((partial.is_some() && span < 300.0).then_some(span));
    // The band's radius: its farthest points (the label lies on them).
    let band_r = |f0: f32, f1: f32| {
        let (k0, k1) = (((f0.max(0.0) * N as f32) as usize).min(N - 1), ((f1.max(f0).min(1.0) * N as f32).ceil() as usize).clamp(1, N));
        slab[k0..k1.max(k0 + 1)].iter().copied().fold(0.0f32, f32::max)
    };
    let mut r = band_r(from.max(0.0), if to < 0.0 { 1.0 } else { to }).max(1e-6);
    let mut arc = arc_given.unwrap_or(0.0);
    if from < 0.0 || to < 0.0 {
        let deg = arc_given.unwrap_or(120.0);
        arc = deg;
        let h = r * deg.to_radians() / aspect.max(0.05) / height;
        if from < 0.0 {
            from = (to - h).max(0.0);
        } else {
            to = (from + h).min(1.0);
        }
        r = band_r(from, to).max(1e-6);
    } else if arc_given.is_none() {
        arc = (aspect * (to - from) * height / r).to_degrees().clamp(5.0, 360.0);
    }
    let mid = e1 * center.x + e2 * center.y + axis * (h0 + (from + to) * 0.5 * height);
    let size = [2.0 * r * 1.08, (to - from).max(1e-3) * height, 2.0 * r * 1.08];
    let q = glam::Quat::from_mat3(&glam::Mat3::from_cols(right, axis, out)).normalize();
    let proj = Projection { mode: ProjMode::Wrap, scale: arc, position: (mid - WorldMap::of(mesh).center).to_array(), orient: q.to_array(), axes: size, ..Default::default() };
    let mut got = serde_json::json!({"from": (from * 100.0).round() / 100.0, "to": (to * 100.0).round() / 100.0, "arc": arc.round()});
    if let Some(i) = (0..6).find(|&i| (world[i / 2] * if i % 2 == 0 { 1.0 } else { -1.0 }).dot(out) > 0.97) {
        got["facing"] = Value::String(SIDE_NAMES[i].split(' ').next().unwrap_or("").to_string());
    }
    if let Some(a) = w["axis"].as_str() {
        got["axis"] = Value::String(a.to_string());
    }
    Some((proj, format!("from {from:.2} to {to:.2} of the body's height, over {arc:.0} degrees"), got))
}

/// The SIDES facts: each face of the model's bounding box, its size and
/// proportions as a print on it sees them, the view that shows it best
/// and the pieces seen there (a 96 x 96 depth raster of the side).
fn sides_facts(mesh: &crate::mesh::MeshData, pieces: &[pieces::Piece]) -> String {
    use glam::Vec3;
    let ids: Vec<usize> = (0..pieces.len()).collect();
    let Some((axes, lo, hi)) = look_frame(mesh, pieces, &ids) else { return String::new() };
    let all = photo::sides(axes, lo, hi);
    let long = (mesh.bbox_max - mesh.bbox_min).max_element().max(1e-6);
    let views = views_of(mesh);
    let toward = |vi: usize| views.frames[vi].rot.transpose() * Vec3::Z;
    let tris: Vec<([Vec3; 3], Vec3, usize)> = (0..pieces.len()).flat_map(|i| piece_tris(mesh, pieces, &[i]).into_iter().map(move |(v, n)| (v, n.normalize_or_zero(), i))).collect();
    let world = axes == [Vec3::X, Vec3::Y, Vec3::Z];
    let mut out = format!(
        "SIDES of the model (the faces of its bounding box{}, for prints covering a whole face of a product: the panels of a box, a carton, a book, a flat device; walls are upright, the top and bottom are seen from outside with -z at their top):\n",
        if world { "" } else { ", along the model's own axes: it stands turned" }
    );
    const N: usize = 96;
    for (i, s) in all.iter().enumerate() {
        let size = s.hi - s.lo;
        let mut z = vec![f32::MIN; N * N];
        let mut owner = vec![usize::MAX; N * N];
        for (v, n, piece) in &tris {
            if n.dot(s.out) <= 0.05 {
                continue;
            }
            let q = v.map(|p| (((p.dot(s.right) - s.lo.x) / size.x.max(1e-6)) * N as f32, ((s.hi.y - p.dot(s.up)) / size.y.max(1e-6)) * N as f32, p.dot(s.out)));
            let area = (q[1].0 - q[0].0) * (q[2].1 - q[0].1) - (q[2].0 - q[0].0) * (q[1].1 - q[0].1);
            if area.abs() < 1e-9 {
                continue;
            }
            let x0 = q.iter().map(|p| p.0).fold(f32::MAX, f32::min).floor().max(0.0) as usize;
            let x1 = (q.iter().map(|p| p.0).fold(f32::MIN, f32::max).ceil().max(0.0) as usize).min(N);
            let y0 = q.iter().map(|p| p.1).fold(f32::MAX, f32::min).floor().max(0.0) as usize;
            let y1 = (q.iter().map(|p| p.1).fold(f32::MIN, f32::max).ceil().max(0.0) as usize).min(N);
            for y in y0..y1 {
                for x in x0..x1 {
                    let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                    let w0 = ((q[1].0 - px) * (q[2].1 - py) - (q[2].0 - px) * (q[1].1 - py)) / area;
                    let w1 = ((q[2].0 - px) * (q[0].1 - py) - (q[0].0 - px) * (q[2].1 - py)) / area;
                    let w2 = 1.0 - w0 - w1;
                    if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                        continue;
                    }
                    let d = w0 * q[0].2 + w1 * q[1].2 + w2 * q[2].2;
                    if d > z[y * N + x] {
                        z[y * N + x] = d;
                        owner[y * N + x] = *piece;
                    }
                }
            }
        }
        let mut count: HashMap<usize, usize> = HashMap::new();
        for &o in owner.iter().filter(|&&o| o != usize::MAX) {
            *count.entry(o).or_default() += 1;
        }
        let mut seen: Vec<(usize, usize)> = count.into_iter().filter(|c| c.1 * 33 >= N * N).collect();
        seen.sort_by(|a, b| b.1.cmp(&a.1));
        let list: Vec<String> = seen.iter().map(|(p, c)| format!("{p} ({:.0}%)", *c as f32 * 100.0 / (N * N) as f32)).collect();
        let best = (0..views.frames.len()).max_by(|&a, &b| s.out.dot(toward(a)).total_cmp(&s.out.dot(toward(b)))).unwrap_or(0);
        out += &format!(
            "Side {}: {:.0}% x {:.0}% of the model's longest side (width x height; proportions {:.2}), seen best in view {}, shows pieces {}.\n",
            SIDE_NAMES[i],
            size.x / long * 100.0,
            size.y / long * 100.0,
            s.aspect(),
            best + 1,
            if list.is_empty() { "none".to_string() } else { list.join(", ") }
        );
    }
    out
}

/// A decal frame: center, right, up, out of the surface, width, height,
/// depth (mesh space) as a projection.
fn decal_projection(mesh: &crate::mesh::MeshData, c: glam::Vec3, right: glam::Vec3, up: glam::Vec3, out: glam::Vec3, size: [f32; 3]) -> Projection {
    let q = glam::Quat::from_mat3(&glam::Mat3::from_cols(right, up, out)).normalize();
    Projection {
        mode: ProjMode::Decal,
        position: (c - WorldMap::of(mesh).center).to_array(),
        orient: q.to_array(),
        axes: size,
        ..Default::default()
    }
}

/// An image placed once in a box of the numbered views image, on the
/// surface of `tris` seen there: upright as in that view, its proportions
/// (`aspect` = width / height) kept inside the box.
fn decal_in_box(mesh: &crate::mesh::MeshData, tris: &[([glam::Vec3; 3], glam::Vec3, bool)], b: [f32; 4], aspect: f32) -> Option<Projection> {
    use glam::Vec3;
    let v = views_of(mesh);
    let (x0, x1) = (b[0].min(b[2]), b[0].max(b[2]));
    let (y0, y1) = (b[1].min(b[3]), b[1].max(b[3]));
    let (xc, yc) = ((x0 + x1) * 0.5, (y0 + y1) * 0.5);
    let vi = v.view_at(xc, yc)?;
    // The surface under the box: where it is and which way it faces.
    let mut hits = Vec::new();
    for j in 0..5 {
        for i in 0..5 {
            let (x, y) = (x0 + (x1 - x0) * (i as f32 + 0.5) / 5.0, y0 + (y1 - y0) * (j as f32 + 0.5) / 5.0);
            hits.extend(v.hit(vi, tris, x, y));
        }
    }
    if hits.is_empty() {
        return None;
    }
    let out = hits.iter().map(|h| h.1.normalize_or_zero()).sum::<Vec3>().normalize_or_zero();
    let at = v.hit(vi, tris, xc, yc).map(|h| h.0).unwrap_or_else(|| hits.iter().map(|h| h.0).sum::<Vec3>() / hits.len() as f32);
    let (_, toward) = v.ray(vi, xc, yc);
    if out == Vec3::ZERO || out.dot(toward) < 0.05 {
        return None;
    }
    // Upright as the view shows it.
    let view_up = v.frames[vi].rot.transpose() * Vec3::Y;
    let up = (view_up - out * view_up.dot(out)).normalize_or_zero();
    if up == Vec3::ZERO {
        return None;
    }
    let right = up.cross(out);
    // The box's edges on the surface's plane.
    let on_plane = |x: f32, y: f32| {
        let (o, d) = v.ray(vi, x, y);
        o + d * ((at - o).dot(out) / d.dot(out))
    };
    let mut w = (on_plane(x1, yc) - on_plane(x0, yc)).dot(right).abs();
    let mut h = (on_plane(xc, y0) - on_plane(xc, y1)).dot(up).abs();
    if w < 1e-6 || h < 1e-6 {
        return None;
    }
    if w / h > aspect {
        w = h * aspect;
    } else {
        h = w / aspect.max(1e-3);
    }
    Some(decal_projection(mesh, on_plane(xc, yc), right, up, out, [w, h, w.max(h)]))
}

/// An image stretched over pieces as seen from the way they face (a
/// screen, a label covering a flat panel). None when they do not face one
/// way (a box, a cylinder).
fn decal_fit(mesh: &crate::mesh::MeshData, tris: &[([glam::Vec3; 3], glam::Vec3)]) -> Option<Projection> {
    use glam::Vec3;
    let sum: Vec3 = tris.iter().map(|t| t.1).sum();
    let total: f32 = tris.iter().map(|t| t.1.length()).sum();
    if total <= 0.0 || sum.length() < 0.5 * total {
        return None;
    }
    let out = sum.normalize();
    let up = if out.y.abs() > 0.9 { Vec3::NEG_Z } else { Vec3::Y };
    let up = (up - out * up.dot(out)).normalize();
    let right = up.cross(out);
    let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    for (v, _) in tris {
        for p in v {
            let q = Vec3::new(p.dot(right), p.dot(up), p.dot(out));
            lo = lo.min(q);
            hi = hi.max(q);
        }
    }
    let mid = (lo + hi) * 0.5;
    let size = hi - lo;
    let depth = size.z + 0.05 * size.x.max(size.y);
    Some(decal_projection(mesh, right * mid.x + up * mid.y + out * mid.z, right, up, out, [size.x.max(1e-5), size.y.max(1e-5), depth]))
}

/// Part of a photo as flat artwork (a label, a logo, a printed panel):
/// "crop" [x0, y0, x1, y1] cuts a box; "quad" [[x, y] x 4] (top left, top
/// right, bottom right, bottom left) cuts a part seen at an angle and
/// straightens it; "arc" (degrees of the circumference that part shows)
/// unrolls a part wrapped around a round surface (a label on a bottle,
/// squeezed toward its sides in the photo). Coordinates are the image's
/// pixels as Muse sees it (longest side 1024). None when the source asks
/// for none of these.
fn cut_print(path: &std::path::Path, src: &Value) -> Option<anyhow::Result<std::path::PathBuf>> {
    let pt = |v: &Value| Some([v[0].as_f64()? as f32, v[1].as_f64()? as f32]);
    let quad: Option<[[f32; 2]; 4]> = match (src["quad"].as_array(), src["crop"].as_array()) {
        (Some(q), _) if q.len() == 4 => Some([pt(&q[0])?, pt(&q[1])?, pt(&q[2])?, pt(&q[3])?]),
        (_, Some(c)) if c.len() == 4 => {
            let v: Vec<f32> = c.iter().filter_map(|x| x.as_f64()).map(|x| x as f32).collect();
            (v.len() == 4).then(|| [[v[0], v[1]], [v[2], v[1]], [v[2], v[3]], [v[0], v[3]]])
        }
        _ => None,
    };
    let quad = quad?;
    let arc = src["arc"].as_f64().map(|a| (a as f32).clamp(1.0, 179.0).to_radians());
    Some((|| {
        let img = image::open(path)?.to_rgba8();
        let (w, h) = img.dimensions();
        let k = (w.max(h) as f32 / 1024.0).max(1.0);
        let q = quad.map(|p| [p[0] * k, p[1] * k]);
        // The corners Muse gives are a few pixels off: each side moves onto
        // the strongest edge near it (the label's border).
        let q = if src["snap"].as_bool() == Some(false) { q } else { snap_quad(&img, q, 0.25) };
        let out = cut_quad(&img, q, arc)?;
        let dir = muse_dir().join("prints");
        std::fs::create_dir_all(&dir)?;
        let stem = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        let key: f32 = quad.iter().map(|p| p[0] * 3.0 + p[1] * 7.0).sum::<f32>() + arc.unwrap_or(0.0) * 1000.0;
        let file = dir.join(format!("cut_{stem}_{:.0}.png", key * 10.0));
        out.save(&file)?;
        Ok(file)
    })())
}

/// The part of `img` inside the quad (its pixels: top left, top right,
/// bottom right, bottom left), straightened to a rectangle at its real
/// proportions (the mean of its opposite sides); `arc` (radians) unrolls
/// a part wrapped around a round surface.
fn cut_quad(img: &image::RgbaImage, q: [[f32; 2]; 4], arc: Option<f32>) -> anyhow::Result<image::RgbaImage> {
    let len = |a: [f32; 2], b: [f32; 2]| ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt();
    let unroll = arc.map_or(1.0, |t| (t * 0.5) / (t * 0.5).sin());
    let ow = ((len(q[0], q[1]) + len(q[3], q[2])) * 0.5 * unroll).round().clamp(8.0, 4096.0) as u32;
    let oh = ((len(q[0], q[3]) + len(q[1], q[2])) * 0.5).round().clamp(8.0, 4096.0) as u32;
    anyhow::ensure!(ow >= 8 && oh >= 8, "the part is too small");
    let hm = square_to_quad(q).ok_or_else(|| anyhow::anyhow!("the four corners do not make a shape"))?;
    Ok(image::RgbaImage::from_fn(ow, oh, |x, y| {
        let mut u = (x as f32 + 0.5) / ow as f32;
        let v = (y as f32 + 0.5) / oh as f32;
        // Even steps around the surface: the photo shows sin of them.
        if let Some(t) = arc {
            let phi = (u - 0.5) * t;
            u = 0.5 + 0.5 * phi.sin() / (t * 0.5).sin();
        }
        let d = hm[6] * u + hm[7] * v + 1.0;
        let sx = (hm[0] * u + hm[1] * v + hm[2]) / d;
        let sy = (hm[3] * u + hm[4] * v + hm[5]) / d;
        bilinear(img, sx - 0.5, sy - 0.5)
    }))
}

/// Each side of the quad moved (along its normal, up to `reach` of the
/// quad's short side) onto the line where the photo changes most across
/// it, then the corners where the moved sides meet.
fn snap_quad(img: &image::RgbaImage, q: [[f32; 2]; 4], reach: f32) -> [[f32; 2]; 4] {
    use glam::Vec2;
    let p: [Vec2; 4] = q.map(Vec2::from);
    let c = (p[0] + p[1] + p[2] + p[3]) / 4.0;
    let size = (p[1] - p[0]).length().min((p[3] - p[0]).length());
    let reach = (size * reach).max(3.0);
    let at = |v: Vec2| bilinear(img, v.x, v.y).0;
    let side = |s: usize| {
        let (a, b) = (p[s], p[(s + 1) % 4]);
        let dir = (b - a).normalize_or_zero();
        let n = Vec2::new(-dir.y, dir.x);
        (a, b, dir, if n.dot(c - a) < 0.0 { -n } else { n })
    };
    // The contrast across side s shifted by d (inside minus outside), and
    // the mean color a little inside it (past the edge's blur).
    let score = |s: usize, d: f32| {
        let (a, b, _, n) = side(s);
        let (mut sum, mut inside) = (0.0, [0.0f32; 3]);
        for i in 1..20 {
            let m = a + (b - a) * (i as f32 / 20.0) + n * d;
            let (pin, pout) = (at(m + n * 2.0), at(m - n * 2.0));
            sum += (0..3).map(|k| (pin[k] as f32 - pout[k] as f32).abs()).sum::<f32>();
            let deep = at(m + n * 6.0);
            for k in 0..3 {
                inside[k] += deep[k] as f32 / 19.0;
            }
        }
        (sum, inside)
    };
    // `like`: the part's own main color (a label's paper); an edge with
    // something else just inside it (the bottle beyond a label) scores less.
    let best = |s: usize, like: [f32; 3]| {
        let weigh = |(sc, inside): (f32, [f32; 3])| sc * (1.0 - ((0..3).map(|k| (inside[k] - like[k]).powi(2)).sum::<f32>().sqrt() / 250.0).min(0.7));
        let (mut top, mut at_d) = (weigh(score(s, 0.0)) * 1.05, 0.0);
        let mut d = -reach;
        while d <= reach {
            let sc = weigh(score(s, d));
            if sc > top {
                (top, at_d) = (sc, d);
            }
            d += 0.5;
        }
        at_d
    };
    // The most common color inside the quad (its middle, coarse bins).
    let mut bins: HashMap<[u8; 3], (usize, [f32; 3])> = HashMap::new();
    for i in 0..24 {
        for j in 0..24 {
            let (u, v) = (0.15 + 0.7 * i as f32 / 23.0, 0.15 + 0.7 * j as f32 / 23.0);
            let m = p[0].lerp(p[1], u).lerp(p[3].lerp(p[2], u), v);
            let px = at(m);
            let e = bins.entry([px[0] / 32, px[1] / 32, px[2] / 32]).or_insert((0, [0.0; 3]));
            e.0 += 1;
            for k in 0..3 {
                e.1[k] += px[k] as f32;
            }
        }
    }
    let like = bins.values().max_by_key(|b| b.0).map(|b| b.1.map(|x| x / b.0 as f32)).unwrap_or([128.0; 3]);
    let mut lines: [(Vec2, Vec2); 4] = [(Vec2::ZERO, Vec2::ZERO); 4];
    for s in 0..4 {
        let (a, _, dir, n) = side(s);
        lines[s] = (a + n * best(s, like), dir);
    }
    // Corner s is where side s-1 and side s meet.
    let meet = |(p1, d1): (Vec2, Vec2), (p2, d2): (Vec2, Vec2), fallback: Vec2| {
        let den = d1.x * d2.y - d1.y * d2.x;
        if den.abs() < 1e-4 {
            return fallback;
        }
        let t = ((p2.x - p1.x) * d2.y - (p2.y - p1.y) * d2.x) / den;
        p1 + d1 * t
    };
    std::array::from_fn(|s| meet(lines[(s + 3) % 4], lines[s], p[s]).to_array())
}

/// The projective map from the unit square (u right, v down) to the quad
/// (top left, top right, bottom right, bottom left): x = (h0 u + h1 v +
/// h2) / (h6 u + h7 v + 1), y likewise with h3 h4 h5.
fn square_to_quad(q: [[f32; 2]; 4]) -> Option<[f32; 8]> {
    let [p0, p1, p2, p3] = q.map(|p| [p[0] as f64, p[1] as f64]);
    let (sx, sy) = (p0[0] - p1[0] + p2[0] - p3[0], p0[1] - p1[1] + p2[1] - p3[1]);
    let (dx1, dy1, dx2, dy2) = (p1[0] - p2[0], p1[1] - p2[1], p3[0] - p2[0], p3[1] - p2[1]);
    let (g, hh) = if sx.abs() < 1e-9 && sy.abs() < 1e-9 {
        (0.0, 0.0)
    } else {
        let den = dx1 * dy2 - dx2 * dy1;
        if den.abs() < 1e-9 {
            return None;
        }
        ((sx * dy2 - dx2 * sy) / den, (dx1 * sy - sx * dy1) / den)
    };
    let a = p1[0] - p0[0] + g * p1[0];
    let b = p3[0] - p0[0] + hh * p3[0];
    let d = p1[1] - p0[1] + g * p1[1];
    let e = p3[1] - p0[1] + hh * p3[1];
    Some([a, b, p0[0], d, e, p0[1], g, hh].map(|x| x as f32))
}

/// A pixel between pixels (edges clamped).
fn bilinear(img: &image::RgbaImage, x: f32, y: f32) -> image::Rgba<u8> {
    let (w, h) = img.dimensions();
    let x = x.clamp(0.0, (w - 1) as f32);
    let y = y.clamp(0.0, (h - 1) as f32);
    let (x0, y0) = (x.floor() as u32, y.floor() as u32);
    let (x1, y1) = ((x0 + 1).min(w - 1), (y0 + 1).min(h - 1));
    let (fx, fy) = (x - x0 as f32, y - y0 as f32);
    let p = |x, y| img.get_pixel(x, y).0.map(|c| c as f32);
    let (a, b, c, d) = (p(x0, y0), p(x1, y0), p(x0, y1), p(x1, y1));
    image::Rgba(std::array::from_fn(|i| {
        let top = a[i] + (b[i] - a[i]) * fx;
        let bot = c[i] + (d[i] - c[i]) * fx;
        (top + (bot - top) * fy).round() as u8
    }))
}

/// A print image cropped to what it shows (its opaque pixels, or those
/// unlike its corners: a label on a plain background), saved once next to
/// Muse's files. The original when there is nothing to crop.
fn trimmed(path: &std::path::Path) -> std::path::PathBuf {
    let stem = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let dest = muse_dir().join("prints").join(format!("{stem}.png"));
    if dest.exists() {
        return dest;
    }
    let Ok(img) = image::open(path).map(|i| i.to_rgba8()) else { return path.to_path_buf() };
    let (w, h) = img.dimensions();
    let alpha = img.pixels().any(|p| p[3] < 250);
    let corners = [(0, 0), (w - 1, 0), (0, h - 1), (w - 1, h - 1)].map(|(x, y)| img.get_pixel(x, y).0);
    let bg = [0, 1, 2].map(|c| corners.iter().map(|p| p[c] as i32).sum::<i32>() / 4);
    let shows = |p: &image::Rgba<u8>| if alpha { p[3] > 12 } else { (0..3).map(|c| (p[c] as i32 - bg[c]).abs()).sum::<i32>() > 36 };
    let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0, 0);
    for (x, y, p) in img.enumerate_pixels() {
        if shows(p) {
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
        }
    }
    if x1 < x0 || y1 < y0 || (x1 - x0 + 1 == w && y1 - y0 + 1 == h) {
        return path.to_path_buf();
    }
    let pad = ((x1 - x0).max(y1 - y0) / 40).max(2);
    let (x0, y0) = (x0.saturating_sub(pad), y0.saturating_sub(pad));
    let (x1, y1) = ((x1 + pad).min(w - 1), (y1 + pad).min(h - 1));
    let crop = image::imageops::crop_imm(&img, x0, y0, x1 - x0 + 1, y1 - y0 + 1).to_image();
    match std::fs::create_dir_all(dest.parent().unwrap()).map_err(anyhow::Error::from).and_then(|_| crop.save(&dest).map_err(anyhow::Error::from)) {
        Ok(()) => dest,
        Err(_) => path.to_path_buf(),
    }
}

/// What a print layer prints (a reference or a text), None for any other
/// layer.
fn print_key(l: &Value) -> Option<String> {
    let src = source(l);
    src["reference"].as_u64().map(|r| format!("R{r}")).or_else(|| src["text"].as_str().map(|t| format!("T{t}")))
}

/// What the self check may change: a look only for a defect it names
/// ("problem"), and a look on a scan or image keeps one as its base (a
/// wrong scan is fixed with another scan that was found, not plain
/// colors).
fn guard_check(looks: &[Look], plan: &mut Value, downloads: &HashMap<String, (std::path::PathBuf, String)>, notes: &mut Vec<String>) {
    let Some(new) = plan["looks"].as_array_mut() else { return };
    // `fresh`: the check's own layers, whose scans are only those it found now.
    let textured = |layers: &Value, fresh: bool| {
        let base = layers.as_array().into_iter().flatten().find(|l| print_key(l).is_none() && !l["name"].as_str().is_some_and(|n| n.starts_with("Finish:")));
        base.is_some_and(|l| {
            let s = source(l);
            match s["ambientcg"].as_str() {
                Some(q) => (!fresh && s["got"].is_string()) || downloads.contains_key(q),
                None => ["material", "smart_material", "texture_prompt", "sample", "reference"].iter().any(|k| !s[*k].is_null()),
            }
        })
    };
    new.retain(|look| {
        let what = look["what"].as_str().unwrap_or("a look").to_string();
        if look["problem"].as_str().map_or(true, |s| s.trim().is_empty()) {
            notes.push(format!("The check changed {what} without naming a defect: kept as it was."));
            return false;
        }
        let ids: Vec<usize> = look["pieces"].as_array().into_iter().flatten().filter_map(|v| v.as_u64()).map(|v| v as usize).collect();
        let had = looks.iter().filter(|l| l.pieces.iter().any(|i| ids.contains(i))).any(|l| textured(&l.layers, false));
        if had && !textured(&look["layers"], true) {
            notes.push(format!("The check found the scan of {what} wrong but had no better scan for it: kept the look as it was."));
            return false;
        }
        true
    });
}

/// A self check may move a print but not drop it: the prints (reference
/// images) of the looks it redoes come back when it left them out.
fn keep_prints(looks: &[Look], plan: &mut Value, notes: &mut Vec<String>) {
    let Some(new) = plan["looks"].as_array_mut() else { return };
    // The prints the check kept, in any look (it may move one to another).
    let mut have: std::collections::HashSet<String> = new.iter().flat_map(|l| l["layers"].as_array().into_iter().flatten().filter_map(print_key)).collect();
    let mut back = 0;
    for look in new.iter_mut() {
        let ids: Vec<usize> = look["pieces"].as_array().into_iter().flatten().filter_map(|v| v.as_u64()).map(|v| v as usize).collect();
        let Some(layers) = look["layers"].as_array_mut() else { continue };
        for old in looks.iter().filter(|l| l.pieces.iter().any(|i| ids.contains(i))) {
            for l in old.layers.as_array().into_iter().flatten() {
                let Some(r) = print_key(l) else { continue };
                if have.insert(r) {
                    layers.push(l.clone());
                    back += 1;
                }
            }
        }
    }
    if back > 0 {
        notes.push(format!("Kept {back} print(s) the check had left out."));
    }
}

/// Glass in front of a screen: the view shows the environment through
/// see-through glass, not the screen behind it. Such glass (a look with
/// transmission whose pieces cover a piece with a glowing print) becomes
/// dark glossy glass carrying the screen's image itself.
fn screen_glass(pieces: &[pieces::Piece], plan: &mut Value, notes: &mut Vec<String>) {
    let Some(looks) = plan["looks"].as_array_mut() else { return };
    let ids = |l: &Value| -> Vec<usize> { l["pieces"].as_array().into_iter().flatten().filter_map(|v| v.as_u64()).map(|v| v as usize).filter(|&i| i < pieces.len()).collect() };
    // Screens: looks with a glowing print, and that print.
    let screens: Vec<(Vec<usize>, Value)> = looks
        .iter()
        .filter_map(|l| {
            let glow = l["layers"].as_array()?.iter().find(|x| print_key(x).is_some() && hex(&x["emissive"]).is_some_and(|c| c.iter().any(|&v| v > 0.0)))?;
            Some((ids(l), glow.clone()))
        })
        .collect();
    // b's box mostly covers a's on two axes.
    let covers = |b: &pieces::Piece, a: &pieces::Piece| {
        let ext = a.hi - a.lo;
        let lap = b.hi.min(a.hi) - b.lo.max(a.lo);
        (0..3).filter(|&k| lap[k] >= 0.8 * ext[k].max(1e-6)).count() >= 2
    };
    for l in looks.iter_mut() {
        let mine = ids(l);
        let clear = l["layers"].as_array().is_some_and(|a| a.iter().any(|x| x["transmission"].as_f64().unwrap_or(0.0) > 0.0));
        if !clear {
            continue;
        }
        let Some((_, glow)) = screens.iter().find(|(s, _)| !s.iter().any(|i| mine.contains(i)) && mine.iter().any(|&g| s.iter().any(|&t| covers(&pieces[g], &pieces[t])))) else { continue };
        let mut print = glow.clone();
        print["source"]["fit"] = Value::Bool(true);
        if let Some(src) = print["source"].as_object_mut() {
            src.remove("box");
            src.remove("surface");
            src.remove("decal");
        }
        l["layers"] = serde_json::json!([{"name": "Screen glass", "source": {"color": "#050505"}, "metallic": 0.0, "roughness": 0.06}, print]);
        notes.push(format!("{}: see-through glass would hide the screen in the view, so it shows the screen's image itself.", l["what"].as_str().unwrap_or("The cover glass")));
    }
}

// ----------------------------------------------------------------- finish

/// The finishes a look can ask for ("finish") and what they do.
const FINISHES: &str = "plastic, glossy plastic, rubber, bare metal, painted metal, wood, leather, fabric, ceramic, stone, glass, none";

/// A look's realism finish: subtle, physically motivated layers on top of
/// its material (roughness variation, fine scratches or smudges, grime in
/// the crevices from the baked mesh maps, dust on the faces looking up,
/// rubbed or chipped edges), their strength from `age` (0 new, 1 old and
/// used) and capped, so they never change what the material reads as.
fn finish_layers(kind: &str, age: f32) -> Vec<Value> {
    use serde_json::json;
    let a = age.clamp(0.0, 1.0);
    let k = kind.trim().to_lowercase();
    let op = |lo: f32, hi: f32| ((lo + (hi - lo) * a) * 100.0).round() / 100.0;
    let rough_var = |opacity: f32, scale: f32| json!({"name": "Finish: roughness variation", "blend": "Overlay", "layer_opacity": opacity, "roughness_pattern": {"pattern": "Grunge Map 003", "scale": scale, "min": 0.38, "max": 0.62, "contrast": 1, "seed": 3}});
    let scratches = |opacity: f32| json!({"name": "Finish: fine scratches", "blend": "Overlay", "layer_opacity": opacity, "roughness_pattern": {"pattern": "Grunge Scratches Fine", "scale": 3, "min": 0.4, "max": 0.65, "contrast": 1, "seed": 5}});
    let smudges = |opacity: f32| json!({"name": "Finish: smudges", "blend": "Overlay", "layer_opacity": opacity, "roughness_pattern": {"pattern": "Grunge Spots Dirty", "scale": 2, "min": 0.42, "max": 0.62, "contrast": 1, "seed": 7}});
    let grime = |opacity: f32, color: &str| json!({"name": "Finish: grime in crevices", "color": color, "roughness": 0.85, "metallic": 0.0, "layer_opacity": opacity, "mask": {"base": "black", "steps": [{"generator": "Dirt", "level": 0.55, "contrast": 3, "noise": 0.25, "noise_scale": 14}, {"pattern": "Grunge Map 007", "scale": 6, "contrast": 1.5, "blend": "Multiply"}]}});
    let dust = |opacity: f32| json!({"name": "Finish: dust", "color": "#8F887E", "roughness": 0.92, "metallic": 0.0, "layer_opacity": opacity, "mask": {"base": "black", "steps": [{"generator": "Directional light", "direction": [0, 1, 0], "level": 0.6, "contrast": 3}, {"pattern": "Grunge Splashes Dusty", "scale": 4, "blend": "Multiply"}]}});
    let rubbed = |opacity: f32, roughness: f32| json!({"name": "Finish: rubbed edges", "color": "#242424", "roughness": roughness, "blend": {"color": "Screen", "roughness": "Normal"}, "layer_opacity": opacity, "mask": {"base": "black", "steps": [{"generator": "Edge wear", "level": 0.62 - 0.15 * a, "contrast": 4, "noise": 0.3, "noise_scale": 18}]}});
    let polished = |opacity: f32| json!({"name": "Finish: polished edges", "roughness": 0.16, "layer_opacity": opacity, "mask": {"base": "black", "steps": [{"generator": "Edges", "level": 0.55, "contrast": 4, "noise": 0.2}]}});
    let chipped = |opacity: f32| json!({"name": "Finish: chipped paint", "color": "#8C8C8A", "metallic": 1.0, "roughness": 0.4, "layer_opacity": opacity, "mask": {"base": "black", "steps": [{"generator": "Edge wear", "level": 0.8 - 0.3 * a, "contrast": 6, "noise": 0.35, "noise_scale": 16}]}});
    match k.as_str() {
        "none" => Vec::new(),
        "glossy plastic" => vec![smudges(op(0.25, 0.5)), scratches(op(0.15, 0.45)), grime(op(0.12, 0.45), "#55504A"), dust(op(0.06, 0.35)), rubbed(op(0.1, 0.4), 0.25)],
        "plastic" => vec![rough_var(0.35, 10.0), smudges(op(0.15, 0.35)), grime(op(0.12, 0.45), "#55504A"), dust(op(0.06, 0.35)), rubbed(op(0.1, 0.4), 0.35)],
        "rubber" => vec![rough_var(0.4, 8.0), grime(op(0.15, 0.5), "#5A544C"), dust(op(0.1, 0.45))],
        "bare metal" => vec![rough_var(0.3, 8.0), scratches(op(0.2, 0.55)), smudges(op(0.15, 0.4)), grime(op(0.15, 0.5), "#3A342E"), polished(op(0.2, 0.5))],
        "painted metal" => vec![rough_var(0.3, 8.0), grime(op(0.12, 0.45), "#4A443C"), dust(op(0.05, 0.3)), chipped(op(0.0, 0.9))],
        "wood" => vec![grime(op(0.15, 0.5), "#3E3328"), dust(op(0.05, 0.3)), rubbed(op(0.1, 0.4), 0.45)],
        "leather" => vec![rough_var(0.3, 8.0), grime(op(0.15, 0.5), "#3A3028"), rubbed(op(0.15, 0.5), 0.35)],
        "fabric" => vec![grime(op(0.12, 0.45), "#5A5248"), dust(op(0.06, 0.35))],
        "ceramic" | "stone" => vec![rough_var(0.3, 10.0), grime(op(0.12, 0.5), "#5A5248"), dust(op(0.05, 0.3))],
        "glass" => vec![smudges(op(0.2, 0.5)), dust(op(0.03, 0.2))],
        _ => vec![rough_var(0.3, 10.0), grime(op(0.12, 0.45), "#55504A"), dust(op(0.05, 0.3))],
    }
}

/// Puts each look's finish on top of its layers (rebuilt from "finish" and
/// "age" each time; the "Finish:" layers it had are replaced).
/// A product is flawless (brand new, no finish wear); a stylized look has
/// no photoreal wear either.
fn apply_style(style: Style, plan: &mut Value) {
    if !matches!(style, Style::Product | Style::Stylized) {
        return;
    }
    for l in plan["looks"].as_array_mut().into_iter().flatten() {
        l["finish"] = Value::String("none".into());
        if style == Style::Product {
            l["age"] = serde_json::json!(0.0);
        }
    }
}

fn apply_finishes(plan: &mut Value) {
    let Some(looks) = plan["looks"].as_array_mut() else { return };
    for l in looks.iter_mut() {
        let Some(kind) = l["finish"].as_str().map(str::to_string) else { continue };
        let age = l["age"].as_f64().unwrap_or(0.3) as f32;
        let Some(layers) = l["layers"].as_array_mut() else { continue };
        layers.retain(|x| !x["name"].as_str().is_some_and(|n| n.starts_with("Finish:")));
        layers.extend(finish_layers(&kind, age));
    }
}

/// Where the prints placed on a surface are, and which are hidden behind
/// what sits in front of their surface, run off it or overlap another.
fn placement_facts(c: &Convo, placed: &HashMap<String, (String, usize, [f32; 4])>) -> String {
    let live: std::collections::HashSet<String> = c.looks.iter().flat_map(|l| l.layers.as_array().cloned().unwrap_or_default()).filter_map(|x| print_key(&x)).collect();
    let mut list: Vec<&(String, usize, [f32; 4])> = placed.iter().filter(|(k, _)| live.contains(*k)).map(|(_, v)| v).collect();
    list.sort_by(|a, b| a.1.cmp(&b.1).then(a.2[1].total_cmp(&b.2[1])).then(a.2[0].total_cmp(&b.2[0])));
    let area = |b: [f32; 4]| (b[2] - b[0]).max(0.0) * (b[3] - b[1]).max(0.0);
    let inter = |a: [f32; 4], b: [f32; 4]| area([a[0].max(b[0]), a[1].max(b[1]), a[2].min(b[2]), a[3].min(b[3])]);
    let mut out = String::new();
    for (i, (name, k, b)) in list.iter().enumerate() {
        let Some(s) = c.surfaces.get(*k) else { continue };
        let a = area(*b).max(1e-3);
        out += &format!("- {name}: surface {k}, box [{:.0}, {:.0}, {:.0}, {:.0}]", b[0], b[1], b[2], b[3]);
        // The pieces in front of the surface inside the box, pixel by pixel.
        let mut by: HashMap<u16, usize> = HashMap::new();
        let ch = s.cover.len() / s.cover_w.max(1);
        let (x0, y0) = ((b[0] - s.ox).max(0.0) as usize, (b[1] - s.oy).max(0.0) as usize);
        let (x1, y1) = (((b[2] - s.ox).max(0.0) as usize).min(s.cover_w), ((b[3] - s.oy).max(0.0) as usize).min(ch));
        for y in y0..y1 {
            for x in x0..x1 {
                let o = s.cover[y * s.cover_w + x];
                if o != u16::MAX {
                    *by.entry(o).or_default() += 1;
                }
            }
        }
        let mut behind: Vec<(u16, f32)> = by.into_iter().map(|(o, n)| (o, n as f32 / a)).filter(|f| f.1 > 0.15).collect();
        behind.sort_by(|x, y| y.1.total_cmp(&x.1));
        if !behind.is_empty() {
            let names: Vec<String> = behind.iter().map(|(o, f)| format!("{:.0}% behind piece {o}", f * 100.0)).collect();
            out += &format!(". HIDDEN: {} (sits in front of the surface there)", names.join(", "));
        }
        let off = 1.0 - inter(*b, s.rect()) / a;
        if off > 0.3 {
            out += &format!(". {:.0}% of it is off surface {k}", off * 100.0);
        }
        for (other, k2, b2) in &list[i + 1..] {
            if k2 == k && inter(*b, *b2) > 0.1 * a.min(area(*b2)) {
                out += &format!(". OVERLAPS {other}");
            }
        }
        out += ".\n";
    }
    out
}

/// A lit screen is flat: relief on a look with a glowing print crumples
/// the image, so those height layers are left out.
fn flat_screens(plan: &mut Value, notes: &mut Vec<String>) {
    let Some(looks) = plan["looks"].as_array_mut() else { return };
    for l in looks.iter_mut() {
        let Some(layers) = l["layers"].as_array_mut() else { continue };
        if !layers.iter().any(|x| print_key(x).is_some() && hex(&x["emissive"]).is_some_and(|c| c.iter().any(|&v| v > 0.0))) {
            continue;
        }
        let before = layers.len();
        layers.retain(|x| print_key(x).is_some() || !(x["height_pattern"].is_object() || x["height"].is_object() || x["height"].is_number()));
        if layers.len() < before {
            notes.push(format!("{}: relief left out (it crumples the lit image).", l["what"].as_str().unwrap_or("The screen")));
        }
    }
}

// ---------------------------------------------------------------- uv
/// How a group of pieces is unwrapped, measured on its triangles.
struct UvFit {
    /// 3D and UV area (UV square units, stacked islands counted again).
    area: f32,
    uv_area: f32,
    /// Share of the area whose triangles have no usable UVs.
    degenerate: f32,
    /// Spread of the texel size (area-weighted deviation of log ratio).
    uneven: f32,
    /// How much longer a texel is along U than along V (1: square).
    squash: f32,
    /// Share of the UV cells used twice or more (stacked, mirrored).
    overlap: f32,
    /// The elongated islands' length direction in the UV square (degrees
    /// from +U, 0 to 180) when they agree, and the share of the area they
    /// cover.
    angle: Option<f32>,
    long: f32,
    /// Every island: its piece, triangles, length direction in the UV
    /// (degrees from +U, none when round or bent) and 3D area.
    islands: Vec<(usize, Vec<u32>, Option<f32>, f32)>,
}

impl UvFit {
    /// Model units per UV unit.
    fn unit(&self) -> f32 {
        (self.area / self.uv_area.max(1e-12)).sqrt()
    }

    /// The UV rotation (degrees) that turns a texture's grain (`grain`:
    /// its direction in the texture, degrees from +U) along the length, or
    /// across it.
    fn turn(&self, grain: f32, across: bool) -> Option<f32> {
        Some(uv_turn(grain, self.angle?, across))
    }
}

fn uv_turn(grain: f32, island: f32, across: bool) -> f32 {
    grain - island + if across { 90.0 } else { 0.0 }
}

/// The grain of a gray image (row by row, `w` wide): the direction its
/// streaks run (degrees from +U, image x right and y down, like the UVs),
/// from the structure tensor of its gradients; none when it has no clear
/// direction.
fn grain_of(w: usize, h: usize, v: &[f32]) -> Option<f32> {
    let (mut xx, mut xy, mut yy) = (0.0f64, 0.0f64, 0.0f64);
    for y in 1..h.saturating_sub(1) {
        for x in 1..w.saturating_sub(1) {
            let gx = (v[y * w + x + 1] - v[y * w + x - 1]) as f64;
            let gy = (v[(y + 1) * w + x] - v[(y - 1) * w + x]) as f64;
            xx += gx * gx;
            xy += gx * gy;
            yy += gy * gy;
        }
    }
    let tr = xx + yy;
    let disc = ((xx - yy).powi(2) + 4.0 * xy * xy).sqrt();
    if tr <= 1e-12 || disc < 0.15 * tr {
        return None;
    }
    // Gradients cross the streaks: the grain is a quarter turn from them.
    let across = 0.5 * (2.0 * xy).atan2(xx - yy);
    Some((across.to_degrees() as f32 + 90.0).rem_euclid(180.0))
}

fn grain_of_image(img: &image::RgbaImage) -> Option<f32> {
    let small = image::imageops::resize(img, 256, 256, image::imageops::FilterType::Triangle);
    let v: Vec<f32> = small.pixels().map(|p| (p[0] as f32 + p[1] as f32 + p[2] as f32) / 765.0).collect();
    grain_of(256, 256, &v)
}

fn uv_fit(mesh: &crate::mesh::MeshData, pieces: &[pieces::Piece], ids: &[usize]) -> Option<UvFit> {
    use glam::{Vec2, Vec3};
    const GRID: usize = 256;
    let mut f = UvFit { area: 0.0, uv_area: 0.0, degenerate: 0.0, uneven: 0.0, squash: 1.0, overlap: 0.0, angle: None, long: 0.0, islands: Vec::new() };
    let (mut logs, mut squash, mut long_area) = (Vec::new(), Vec::new(), 0.0f32);
    // Axial mean of the islands' directions (doubled angles).
    let (mut c2, mut s2) = (0.0f32, 0.0f32);
    let (mut used, mut twice) = (0usize, 0usize);
    for &i in ids.iter().filter(|&&i| i < pieces.len()) {
        let p = &pieces[i];
        let sm = &mesh.submeshes[p.sub];
        let corner = |t: u32, j: usize| {
            let v = &mesh.vertices[sm.indices[t as usize * 3 + j] as usize];
            (Vec3::from(v.pos), Vec2::from(v.uv))
        };
        // Per triangle: areas, dP/du and dP/dv.
        let tri = |t: u32| {
            let ((p0, t0), (p1, t1), (p2, t2)) = (corner(t, 0), corner(t, 1), corner(t, 2));
            let (e1, e2, d1, d2) = (p1 - p0, p2 - p0, t1 - t0, t2 - t0);
            let det = d1.x * d2.y - d2.x * d1.y;
            let a3 = e1.cross(e2).length() * 0.5;
            if det.abs() < 1e-12 {
                return (a3, 0.0, Vec3::ZERO, Vec3::ZERO, [t0, t1, t2]);
            }
            ((a3), det.abs() * 0.5, (e1 * d2.y - e2 * d1.y) / det, (e2 * d1.x - e1 * d2.x) / det, [t0, t1, t2])
        };
        // Cells of the UV square this piece covers (overlap).
        let mut grid = vec![0u8; GRID * GRID];
        for &t in &p.tris {
            let (a3, auv, du, dv, uv) = tri(t);
            f.area += a3;
            if auv <= 0.0 {
                f.degenerate += a3;
                continue;
            }
            f.uv_area += auv;
            logs.push(((auv / a3.max(1e-12)).ln(), a3));
            squash.push(((du.length() / dv.length().max(1e-12)).max(1e-6).ln(), a3));
            let g = |v: Vec2| v * GRID as f32;
            let (q0, q1, q2) = (g(uv[0]), g(uv[1]), g(uv[2]));
            let (x0, x1) = (q0.x.min(q1.x).min(q2.x).floor() as i32, q0.x.max(q1.x).max(q2.x).ceil() as i32);
            let (y0, y1) = (q0.y.min(q1.y).min(q2.y).floor() as i32, q0.y.max(q1.y).max(q2.y).ceil() as i32);
            let den = (q1.x - q0.x) * (q2.y - q0.y) - (q2.x - q0.x) * (q1.y - q0.y);
            for y in y0..y1 {
                for x in x0..x1 {
                    let c = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
                    let w1 = ((c.x - q0.x) * (q2.y - q0.y) - (q2.x - q0.x) * (c.y - q0.y)) / den;
                    let w2 = ((q1.x - q0.x) * (c.y - q0.y) - (c.x - q0.x) * (q1.y - q0.y)) / den;
                    if w1 < 0.0 || w2 < 0.0 || w1 + w2 > 1.0 {
                        continue;
                    }
                    let (gx, gy) = (x.rem_euclid(GRID as i32) as usize, y.rem_euclid(GRID as i32) as usize);
                    let c = &mut grid[gy * GRID + gx];
                    *c = c.saturating_add(1);
                }
            }
        }
        used += grid.iter().filter(|&&c| c > 0).count();
        twice += grid.iter().filter(|&&c| c > 1).count();
        // Each UV island: its length direction and the UV axis along it.
        // Flat in the UV, an island shows its length there (a curled leaf
        // too); otherwise its 3D length is carried into the UV.
        for isl in pieces::components(mesh, p.sub, &p.tris, true) {
            let mut uvp: Vec<(Vec2, f32)> = Vec::new();
            for &t in &isl {
                let (a3, auv, ..) = tri(t);
                if auv > 0.0 {
                    uvp.push(((corner(t, 0).1 + corner(t, 1).1 + corner(t, 2).1) / 3.0, a3));
                }
            }
            let uw: f32 = uvp.iter().map(|q| q.1).sum::<f32>();
            if uw > 0.0 {
                let m = uvp.iter().map(|q| q.0 * q.1).sum::<Vec2>() / uw;
                let (mut xx, mut xy, mut yy) = (0.0f32, 0.0f32, 0.0f32);
                for (q, a) in &uvp {
                    let d = *q - m;
                    xx += d.x * d.x * a;
                    xy += d.x * d.y * a;
                    yy += d.y * d.y * a;
                }
                let tr = xx + yy;
                let disc = ((xx - yy).powi(2) + 4.0 * xy * xy).sqrt();
                let (l1, l2) = ((tr + disc) * 0.5, ((tr - disc) * 0.5).max(1e-20));
                if l1 >= 2.25 * l2 {
                    let mut phi = 0.5 * (2.0 * xy).atan2(xx - yy);
                    // A strip wrapped around a tube: its two ends meet in
                    // 3D, so its long UV axis goes around and the length
                    // is the other one.
                    let dir = Vec2::new(phi.cos(), phi.sin());
                    let mut ends: Vec<(f32, Vec3)> = isl.iter().flat_map(|&t| (0..3).map(move |j| (t, j))).map(|(t, j)| {
                        let (p3, q) = corner(t, j);
                        (q.dot(dir), p3)
                    }).collect();
                    ends.sort_by(|a, b| a.0.total_cmp(&b.0));
                    let k = (ends.len() / 20).max(1);
                    let mean = |s: &[(f32, Vec3)]| s.iter().map(|e| e.1).sum::<Vec3>() / s.len().max(1) as f32;
                    let (lo3, hi3) = (mean(&ends[..k]), mean(&ends[ends.len() - k..]));
                    let (bmin, bmax) = ends.iter().fold((Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)), |(a, b), e| (a.min(e.1), b.max(e.1)));
                    if lo3.distance(hi3) < 0.25 * (bmax - bmin).length() {
                        phi += std::f32::consts::FRAC_PI_2;
                    }
                    c2 += uw * (2.0 * phi).cos();
                    s2 += uw * (2.0 * phi).sin();
                    long_area += uw;
                    f.islands.push((i, isl.clone(), Some(phi.to_degrees().rem_euclid(180.0)), uw));
                    continue;
                }
            }
            let mut pts: Vec<(Vec3, f32)> = Vec::new();
            for &t in &isl {
                let (a3, ..) = tri(t);
                let c = (corner(t, 0).0 + corner(t, 1).0 + corner(t, 2).0) / 3.0;
                pts.push((c, a3));
            }
            let w: f32 = pts.iter().map(|p| p.1).sum::<f32>().max(1e-12);
            let mean = pts.iter().map(|p| p.0 * p.1).sum::<Vec3>() / w;
            let mut cov = glam::Mat3::ZERO;
            for (c, a) in &pts {
                let d = *c - mean;
                cov += glam::Mat3::from_cols(d * d.x, d * d.y, d * d.z) * *a;
            }
            // The main axis by power iteration; its spread against the rest.
            let mut axis = Vec3::new(0.577, 0.577, 0.577);
            for _ in 0..24 {
                axis = (cov * axis).normalize_or_zero();
                if axis == Vec3::ZERO {
                    break;
                }
            }
            let l1 = axis.dot(cov * axis);
            let l_rest = (cov.x_axis.x + cov.y_axis.y + cov.z_axis.z - l1).max(1e-12);
            if axis == Vec3::ZERO || l1 < 2.25 * l_rest {
                f.islands.push((i, isl.clone(), None, w));
                continue;
            }
            // The axis in UV, triangle by triangle (least squares on dP/du,
            // dP/dv), averaged as an axis.
            let (mut ic, mut is, mut ia) = (0.0f32, 0.0f32, 0.0f32);
            for &t in &isl {
                let (a3, _, du, dv, _) = tri(t);
                let (a, b, c) = (du.dot(du), du.dot(dv), dv.dot(dv));
                let det = a * c - b * b;
                if det <= 1e-20 {
                    continue;
                }
                let (x, y) = (du.dot(axis), dv.dot(axis));
                let (u, v) = ((c * x - b * y) / det, (a * y - b * x) / det);
                let phi = v.atan2(u);
                ic += a3 * (2.0 * phi).cos();
                is += a3 * (2.0 * phi).sin();
                ia += a3;
            }
            // An island bent in the UV (a curved strip) has no one direction.
            if ia <= 0.0 || (ic * ic + is * is).sqrt() < 0.8 * ia {
                long_area += ia;
                f.islands.push((i, isl.clone(), None, ia));
                continue;
            }
            let r = (ic * ic + is * is).sqrt();
            c2 += ia * ic / r;
            s2 += ia * is / r;
            long_area += ia;
            f.islands.push((i, isl.clone(), Some((is.atan2(ic) * 0.5).to_degrees().rem_euclid(180.0)), ia));
        }
    }
    if f.area <= 0.0 {
        return None;
    }
    f.degenerate /= f.area;
    let wsum: f32 = logs.iter().map(|l| l.1).sum::<f32>().max(1e-12);
    let m = logs.iter().map(|l| l.0 * l.1).sum::<f32>() / wsum;
    f.uneven = (logs.iter().map(|l| (l.0 - m).powi(2) * l.1).sum::<f32>() / wsum).sqrt();
    let sm = squash.iter().map(|l| l.0 * l.1).sum::<f32>() / wsum;
    f.squash = sm.exp();
    f.overlap = twice as f32 / used.max(1) as f32;
    f.long = long_area / f.area;
    // The islands agree on one direction.
    if long_area > 0.0 && (c2 * c2 + s2 * s2).sqrt() >= 0.85 * long_area {
        f.angle = Some((s2.atan2(c2) * 0.5).to_degrees().rem_euclid(180.0));
    }
    Some(f)
}

/// The islands grouped by length direction (within 15 degrees), largest
/// group first; round islands join the largest group. One rotation each.
fn uv_groups(fit: &UvFit) -> Vec<(f32, Vec<(usize, Vec<u32>)>)> {
    let apart = |a: f32, b: f32| {
        let d = (a - b).rem_euclid(180.0);
        d.min(180.0 - d)
    };
    let mut order: Vec<&(usize, Vec<u32>, Option<f32>, f32)> = fit.islands.iter().filter(|x| x.2.is_some()).collect();
    order.sort_by(|a, b| b.3.total_cmp(&a.3));
    let mut groups: Vec<(f32, f32, Vec<(usize, Vec<u32>)>)> = Vec::new();
    for x in order {
        let a = x.2.unwrap_or(0.0);
        match groups.iter_mut().find(|g| apart(g.0, a) <= 15.0) {
            Some(g) => {
                g.1 += x.3;
                g.2.push((x.0, x.1.clone()));
            }
            None => groups.push((a, x.3, vec![(x.0, x.1.clone())])),
        }
    }
    groups.sort_by(|a, b| b.1.total_cmp(&a.1));
    // At most 6 rotations: the small groups join the closest big one.
    while groups.len() > 6 {
        let last = groups.pop().unwrap();
        let k = (0..groups.len()).min_by(|&i, &j| apart(groups[i].0, last.0).total_cmp(&apart(groups[j].0, last.0))).unwrap_or(0);
        groups[k].2.extend(last.2);
    }
    if let Some(g) = groups.first_mut() {
        g.2.extend(fit.islands.iter().filter(|x| x.2.is_none()).map(|x| (x.0, x.1.clone())));
    }
    groups.into_iter().map(|g| (g.0, g.2)).collect()
}

/// The UV facts of every piece for Muse, and what they allow.
fn uv_facts(mesh: &crate::mesh::MeshData, pieces: &[pieces::Piece]) -> String {
    let long = (mesh.bbox_max - mesh.bbox_min).max_element().max(1e-6);
    let mut out = String::from("UV LAYOUT of each piece (measured):\n");
    for i in 0..pieces.len() {
        let Some(f) = uv_fit(mesh, pieces, &[i]) else { continue };
        if f.degenerate > 0.5 || f.uv_area < 1e-6 {
            out += &format!("Piece {i}: no usable UVs (Tri-planar only).\n");
            continue;
        }
        let mut s = vec![format!("covers {:.0}% of the UV square", f.uv_area.min(9.99) * 100.0)];
        s.push(if f.uneven < 0.3 { "even texel size".into() } else if f.uneven < 0.7 { "texel size varies (somewhat stretched)".into() } else { "very uneven texel size (badly stretched)".into() });
        if !(0.67..=1.5).contains(&f.squash) {
            s.push(format!("texels {:.1}x longer along {}", f.squash.max(1.0 / f.squash), if f.squash > 1.0 { "U" } else { "V" }));
        }
        if f.overlap > 0.1 {
            s.push(format!("{:.0}% of its UV area is stacked (islands share texels)", f.overlap * 100.0));
        }
        s.push(match f.angle {
            Some(a) if (a - 90.0).abs() <= 12.0 => "its length runs along V (up and down the texture)".into(),
            Some(a) if !(12.0..=168.0).contains(&a) => "its length runs along U (across the texture)".into(),
            Some(a) => format!("its length runs at {a:.0} degrees in the UV square (\"along\" turns the texture to follow it)"),
            None if f.long > 0.3 => "its islands run in different directions (no one rotation fits them all)".into(),
            None => "no long direction (round or square islands)".into(),
        });
        s.push(format!("1 UV unit = {:.0}% of the model's longest side", f.unit() / long * 100.0));
        out += &format!("Piece {i}: {}.\n", s.join(", "));
    }
    out
}

// --------------------------------------------------------------- surfaces

/// A UV island as the SURFACES image shows it: flattened along the way it
/// faces, upright (the model's up, or its back for faces looking up or
/// down), every island at the same scale.
#[derive(Clone)]
struct Surface {
    piece: usize,
    right: glam::Vec3,
    up: glam::Vec3,
    out: glam::Vec3,
    /// Bounds on (right, up), mesh units; depth range along `out`.
    lo: glam::Vec2,
    hi: glam::Vec2,
    depth: (f32, f32),
    /// Its corner on the image and the pixels per mesh unit.
    ox: f32,
    oy: f32,
    s: f32,
    /// What sits in front of it (buttons in their holes, a door): its name
    /// (the piece, plus a letter when the piece shows several parts), its
    /// piece and its box on the SURFACES image.
    front: Vec<(String, usize, [f32; 4])>,
    /// Per pixel of its rect, the piece in front there (u16::MAX: none).
    cover: std::sync::Arc<Vec<u16>>,
    cover_w: usize,
}

impl Surface {
    /// A pixel of the SURFACES image in this surface's (right, up) units.
    fn to_ru(&self, x: f32, y: f32) -> glam::Vec2 {
        glam::Vec2::new(self.lo.x + (x - self.ox) / self.s, self.hi.y - (y - self.oy) / self.s)
    }

    /// The box of a print `size` pixels tall (its width from `aspect`)
    /// beside what sits in front of this surface (`near`: its name, or a
    /// piece number for all its parts), on `side`, `gap` pixels away.
    fn near_box(&self, near: &Value, side: &str, size: Option<f32>, gap: Option<f32>, aspect: f32) -> Option<[f32; 4]> {
        let name = match near {
            Value::String(s) => s.trim().to_string(),
            Value::Number(n) => n.to_string(),
            _ => return None,
        };
        let mine = |f: &(String, usize, [f32; 4])| f.0 == name || f.1.to_string() == name;
        let a = self.front.iter().filter(|f| mine(f)).map(|f| f.2).reduce(|a, b| [a[0].min(b[0]), a[1].min(b[1]), a[2].max(b[2]), a[3].max(b[3])])?;
        let mut h = size.filter(|s| *s > 0.0).unwrap_or(((a[3] - a[1]) * 0.5).max(4.0));
        let (cx, cy) = ((a[0] + a[2]) * 0.5, (a[1] + a[3]) * 0.5);
        // Above or below, a label stays clear of its neighbors' labels: no
        // wider than the space to the next part on its row (smaller text).
        if side != "left" && side != "right" {
            let room = self.front.iter().filter(|f| !mine(f) && f.2[1] < a[3] && f.2[3] > a[1]).map(|f| ((f.2[0] + f.2[2]) * 0.5 - cx).abs()).fold(f32::MAX, f32::min);
            h = h.min(room * 0.92 / aspect.max(0.05));
        }
        let w = h * aspect.max(0.05);
        let g = gap.unwrap_or(h * 0.5);
        Some(match side {
            "below" => [cx - w * 0.5, a[3] + g, cx + w * 0.5, a[3] + g + h],
            "left" => [a[0] - g - w, cy - h * 0.5, a[0] - g, cy + h * 0.5],
            "right" => [a[2] + g, cy - h * 0.5, a[2] + g + w, cy + h * 0.5],
            _ => [cx - w * 0.5, a[1] - g - h, cx + w * 0.5, a[1] - g],
        })
    }

    fn rect(&self) -> [f32; 4] {
        let size = (self.hi - self.lo) * self.s;
        [self.ox, self.oy, self.ox + size.x, self.oy + size.y]
    }

    /// An image placed in a box of the SURFACES image (`aspect` = width /
    /// height kept inside the box), as a decal on this surface.
    fn place(&self, mesh: &crate::mesh::MeshData, b: [f32; 4], aspect: f32) -> Option<Projection> {
        let (x0, x1) = (b[0].min(b[2]), b[0].max(b[2]));
        let (y0, y1) = (b[1].min(b[3]), b[1].max(b[3]));
        let r = |x: f32| self.lo.x + (x - self.ox) / self.s;
        let u = |y: f32| self.hi.y - (y - self.oy) / self.s;
        let (mut w, mut h) = (r(x1) - r(x0), u(y0) - u(y1));
        if w < 1e-6 || h < 1e-6 {
            return None;
        }
        // aspect 0: the print fills the box ("stretch").
        if aspect <= 0.0 {
        } else if w / h > aspect {
            w = h * aspect;
        } else {
            h = w / aspect.max(1e-3);
        }
        let (rc, uc) = ((r(x0) + r(x1)) * 0.5, (u(y0) + u(y1)) * 0.5);
        let dc = (self.depth.0 + self.depth.1) * 0.5;
        let depth = (self.depth.1 - self.depth.0) + 0.1 * w.max(h);
        Some(decal_projection(mesh, self.right * rc + self.up * uc + self.out * dc, self.right, self.up, self.out, [w, h, depth]))
    }
}

/// The SURFACES image: the main UV islands of the pieces, each flattened
/// and upright, at one scale, with what sits in front of it (buttons in
/// their holes) dimmed, a grid every 5% of the model's longest side and
/// its number. Returns the image, the surfaces and their facts for Muse.
fn surfaces_of(mesh: &crate::mesh::MeshData, pieces: &[pieces::Piece]) -> (image::RgbaImage, Vec<Surface>, String) {
    use glam::{Vec2, Vec3};
    const W: f32 = 1536.0;
    const PAD: f32 = 14.0;
    const LABEL: f32 = 26.0;
    let long = (mesh.bbox_max - mesh.bbox_min).max_element().max(1e-6);
    // Every triangle in scope: corners, normal (the face's, turned as its
    // vertex normals), piece.
    let mut all: Vec<([Vec3; 3], Vec3, usize)> = Vec::new();
    for i in 0..pieces.len() {
        for (v, n) in piece_tris(mesh, pieces, &[i]) {
            all.push((v, n.normalize_or_zero(), i));
        }
    }
    // The islands, largest first.
    let total: f32 = pieces.iter().map(|p| p.area).sum::<f32>().max(1e-9);
    let mut islands: Vec<(usize, f32, Vec<([Vec3; 3], Vec3)>)> = Vec::new();
    for (i, p) in pieces.iter().enumerate() {
        for isl in pieces::components(mesh, p.sub, &p.tris, true) {
            let sm = &mesh.submeshes[p.sub];
            let tris: Vec<([Vec3; 3], Vec3)> = isl
                .iter()
                .map(|&t| {
                    let k = |j: usize| &mesh.vertices[sm.indices[t as usize * 3 + j] as usize];
                    let v = [Vec3::from(k(0).pos), Vec3::from(k(1).pos), Vec3::from(k(2).pos)];
                    let n = (v[1] - v[0]).cross(v[2] - v[0]);
                    let vn = Vec3::from(k(0).normal) + Vec3::from(k(1).normal) + Vec3::from(k(2).normal);
                    (v, if n.dot(vn) < 0.0 { -n } else { n })
                })
                .collect();
            let area: f32 = tris.iter().map(|t| t.1.length() * 0.5).sum();
            if area >= 0.004 * total {
                islands.push((i, area, tris));
            }
        }
    }
    islands.sort_by(|a, b| b.1.total_cmp(&a.1));
    islands.truncate(24);
    // Each island's frame and bounds.
    struct Frame {
        piece: usize,
        right: Vec3,
        up: Vec3,
        out: Vec3,
        lo: Vec2,
        hi: Vec2,
        depth: (f32, f32),
        tris: Vec<([Vec3; 3], Vec3)>,
    }
    let mut frames: Vec<Frame> = islands
        .into_iter()
        .filter_map(|(piece, _, tris)| {
            let out = tris.iter().map(|t| t.1).sum::<Vec3>().normalize_or_zero();
            if out == Vec3::ZERO {
                return None;
            }
            let up = if out.y.abs() > 0.9 { Vec3::NEG_Z } else { Vec3::Y };
            let up = (up - out * up.dot(out)).normalize();
            let right = up.cross(out);
            let (mut lo, mut hi, mut depth) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN), (f32::MAX, f32::MIN));
            for (v, _) in &tris {
                for p in v {
                    let q = Vec2::new(p.dot(right), p.dot(up));
                    lo = lo.min(q);
                    hi = hi.max(q);
                    depth = (depth.0.min(p.dot(out)), depth.1.max(p.dot(out)));
                }
            }
            Some(Frame { piece, right, up, out, lo, hi, depth, tris })
        })
        .collect();
    // One scale for all: the largest that packs them in rows into W wide
    // and at most W high; islands that come out tiny are left out.
    let pack = |frames: &[Frame], s: f32| -> (Vec<(f32, f32)>, f32) {
        let (mut x, mut y, mut row) = (PAD, PAD, 0.0f32);
        let mut at = Vec::new();
        for f in frames {
            let size = (f.hi - f.lo) * s;
            if x + size.x + PAD > W && x > PAD {
                x = PAD;
                y += row + PAD;
                row = 0.0;
            }
            at.push((x, y + LABEL));
            x += size.x.max(LABEL * 2.0) + PAD;
            row = row.max(size.y + LABEL);
        }
        (at, y + row + PAD)
    };
    let fit = |frames: &[Frame]| {
        let widest = frames.iter().map(|f| (f.hi - f.lo).x).fold(1e-6f32, f32::max);
        let (mut a, mut b) = (0.0f32, (W - 2.0 * PAD) / widest);
        for _ in 0..30 {
            let m = (a + b) * 0.5;
            if pack(frames, m).1 <= W {
                a = m;
            } else {
                b = m;
            }
        }
        a
    };
    let mut s = fit(&frames);
    let before = frames.len();
    frames.retain(|f| ((f.hi - f.lo) * s).max_element() >= 40.0);
    if frames.len() < before && !frames.is_empty() {
        s = fit(&frames);
    }
    let (at, height) = pack(&frames, s);
    let mut img = image::RgbaImage::from_pixel(W as u32, height.max(64.0).min(W) as u32, image::Rgba([28, 28, 30, 255]));
    let views = views_of(mesh);
    let mut info = format!(
        "SURFACES image (the third image of the first message, {} x {} pixels): the main UV islands of the pieces, each flattened along the way it faces and upright, all at the same scale, its number above its top left corner; what sits in front of it (buttons in their holes, a door) is drawn dimmed and listed under the surface with its box (named by its piece, plus a letter when that piece shows several parts there); the grid squares are 5% of the model's longest side.\n",
        img.width(),
        img.height()
    );
    let mut out = Vec::new();
    for (k, (f, &(ox, oy))) in frames.iter().zip(&at).enumerate() {
        let size = (f.hi - f.lo) * s;
        let (w, h) = (size.x.ceil() as usize + 1, size.y.ceil() as usize + 1);
        let mut zbuf = vec![f32::MIN; w * h];
        let mut owner = vec![usize::MAX; w * h];
        let own: std::collections::HashSet<[u32; 3]> = f.tris.iter().map(|t| t.0[0].to_array().map(f32::to_bits)).collect();
        let slack = 0.15 * size.max_element() / s;
        for (v, n, piece) in &all {
            let facing = n.dot(f.out);
            if facing <= 0.05 {
                continue;
            }
            let d = v.map(|p| p.dot(f.out));
            if d.iter().all(|&z| z < f.depth.0 - slack) || d.iter().all(|&z| z > f.depth.1 + slack) {
                continue;
            }
            let q = v.map(|p| ((p.dot(f.right) - f.lo.x) * s, (f.hi.y - p.dot(f.up)) * s));
            let mine = *piece == f.piece && own.contains(&v[0].to_array().map(f32::to_bits));
            let base = part_color(*piece).0.map(|c| c as f32);
            let shade = 0.35 + 0.65 * facing;
            let col = if mine { base.map(|c| c * shade) } else { base.map(|c| (c * 0.3 + 40.0) * shade) };
            let area = (q[1].0 - q[0].0) * (q[2].1 - q[0].1) - (q[2].0 - q[0].0) * (q[1].1 - q[0].1);
            if area.abs() < 1e-9 {
                continue;
            }
            let x0 = q.iter().map(|p| p.0).fold(f32::MAX, f32::min).floor().max(0.0) as usize;
            let x1 = (q.iter().map(|p| p.0).fold(f32::MIN, f32::max).ceil().max(0.0) as usize).min(w);
            let y0 = q.iter().map(|p| p.1).fold(f32::MAX, f32::min).floor().max(0.0) as usize;
            let y1 = (q.iter().map(|p| p.1).fold(f32::MIN, f32::max).ceil().max(0.0) as usize).min(h);
            for y in y0..y1 {
                for x in x0..x1 {
                    let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                    let w0 = ((q[1].0 - px) * (q[2].1 - py) - (q[2].0 - px) * (q[1].1 - py)) / area;
                    let w1 = ((q[2].0 - px) * (q[0].1 - py) - (q[0].0 - px) * (q[2].1 - py)) / area;
                    let w2 = 1.0 - w0 - w1;
                    if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                        continue;
                    }
                    let z = w0 * d[0] + w1 * d[1] + w2 * d[2];
                    if z > zbuf[y * w + x] {
                        zbuf[y * w + x] = z;
                        owner[y * w + x] = if *piece == f.piece { usize::MAX } else { *piece };
                        let (ix, iy) = (ox as u32 + x as u32, oy as u32 + y as u32);
                        if ix < img.width() && iy < img.height() {
                            img.put_pixel(ix, iy, image::Rgba([col[0] as u8, col[1] as u8, col[2] as u8, 255]));
                        }
                    }
                }
            }
        }
        // The parts of other pieces in front of it, each with its box.
        let mut parts: Vec<(usize, [f32; 4])> = Vec::new();
        let mut seen = vec![false; w * h];
        for i0 in 0..w * h {
            let pc = owner[i0];
            if pc == usize::MAX || seen[i0] {
                continue;
            }
            let (mut b, mut n, mut stack) = ([usize::MAX, usize::MAX, 0, 0], 0, vec![i0]);
            seen[i0] = true;
            while let Some(i) = stack.pop() {
                let (x, y) = (i % w, i / w);
                n += 1;
                b = [b[0].min(x), b[1].min(y), b[2].max(x + 1), b[3].max(y + 1)];
                for (dx, dy) in [(-1i32, 0i32), (1, 0), (0, -1), (0, 1)] {
                    let (nx, ny) = (x as i32 + dx, y as i32 + dy);
                    if nx >= 0 && ny >= 0 && (nx as usize) < w && (ny as usize) < h {
                        let j = ny as usize * w + nx as usize;
                        if !seen[j] && owner[j] == pc {
                            seen[j] = true;
                            stack.push(j);
                        }
                    }
                }
            }
            // Compact parts only: a frame around the surface is no place.
            if n >= 6 && n * 4 >= (b[2] - b[0]) * (b[3] - b[1]) {
                parts.push((pc, [ox + b[0] as f32, oy + b[1] as f32, ox + b[2] as f32, oy + b[3] as f32]));
            }
        }
        parts.sort_by(|a, b| a.0.cmp(&b.0).then(a.1[0].total_cmp(&b.1[0])));
        let mut front = Vec::new();
        for (i, &(pc, b)) in parts.iter().enumerate() {
            let many = parts.iter().filter(|q| q.0 == pc).count() > 1;
            let nth = parts[..i].iter().filter(|q| q.0 == pc).count();
            let name = if many { format!("{pc}{}", (b'a' + nth.min(25) as u8) as char) } else { pc.to_string() };
            front.push((name, pc, b));
        }
        // The grid, every 5% of the model's longest side.
        let step = 0.05 * long * s;
        if step >= 6.0 {
            let mut g = |x: u32, y: u32| {
                if x < img.width() && y < img.height() {
                    let p = img.get_pixel_mut(x, y);
                    let c = p.0;
                    p.0 = [((c[0] as u16 * 3 + 200) / 4) as u8, ((c[1] as u16 * 3 + 200) / 4) as u8, ((c[2] as u16 * 3 + 200) / 4) as u8, 255];
                }
            };
            let mut gx = 0.0;
            while gx <= size.x {
                for y in 0..h as u32 {
                    g(ox as u32 + gx as u32, oy as u32 + y);
                }
                gx += step;
            }
            let mut gy = 0.0;
            while gy <= size.y {
                for x in 0..w as u32 {
                    g(ox as u32 + x, oy as u32 + gy as u32);
                }
                gy += step;
            }
        }
        draw_number(&mut img, ox as i32 + (k.to_string().len() as i32 * 12 + 3) / 2, oy as i32 - 14, k);
        let toward = |vi: usize| views.frames[vi].rot.transpose() * Vec3::Z;
        let best = (0..views.frames.len()).max_by(|&a, &b| f.out.dot(toward(a)).total_cmp(&f.out.dot(toward(b)))).unwrap_or(0);
        let dir: Vec<String> = [("+x", Vec3::X), ("-x", Vec3::NEG_X), ("+y (up)", Vec3::Y), ("-y (down)", Vec3::NEG_Y), ("+z", Vec3::Z), ("-z", Vec3::NEG_Z)]
            .iter()
            .filter(|(_, a)| f.out.dot(*a) > 0.4)
            .map(|(n, _)| n.to_string())
            .collect();
        let pct = (f.hi - f.lo) / long * 100.0;
        info += &format!(
            "Surface {k}: on piece {}, faces {} (seen best in view {}), {:.0}% x {:.0}% of the model's longest side, at [{:.0}, {:.0}, {:.0}, {:.0}] on the SURFACES image.\n",
            f.piece,
            if dir.is_empty() { "several ways".to_string() } else { dir.join(" and ") },
            best + 1,
            pct.x,
            pct.y,
            ox,
            oy,
            ox + size.x,
            oy + size.y
        );
        if !front.is_empty() {
            let list: Vec<String> = front.iter().take(40).map(|(n, _, b)| format!("{n} [{:.0}, {:.0}, {:.0}, {:.0}]", b[0], b[1], b[2], b[3])).collect();
            info += &format!("  In front of surface {k} (pixel boxes on the SURFACES image): {}.\n", list.join(", "));
        }
        let cover = owner.iter().map(|&o| if o == usize::MAX { u16::MAX } else { o as u16 }).collect();
        out.push(Surface { piece: f.piece, right: f.right, up: f.up, out: f.out, lo: f.lo, hi: f.hi, depth: f.depth, ox, oy, s, front, cover: std::sync::Arc::new(cover), cover_w: w });
    }
    (img, out, info)
}

/// The SURFACES image as the model looks now: the same layout and pixels,
/// each surface (and what sits in front of it) in its texture's colors and
/// glow instead of its piece color. `maps`: per submesh, the texture size,
/// its final base color and emissive (linear RGBA) and emissive intensity.
#[allow(clippy::type_complexity)]
fn surfaces_now(mesh: &crate::mesh::MeshData, pieces: &[pieces::Piece], surfaces: &[Surface], maps: &HashMap<usize, (u32, Vec<f32>, Vec<f32>, f32)>) -> image::RgbaImage {
    use glam::{Vec2, Vec3};
    let height = surfaces.iter().map(|s| s.rect()[3]).fold(64.0f32, f32::max) + 14.0;
    let mut img = image::RgbaImage::from_pixel(1536, height.min(1536.0) as u32, image::Rgba([28, 28, 30, 255]));
    // Every triangle in scope with its corners' UVs and its submesh.
    let mut all: Vec<([Vec3; 3], Vec3, [Vec2; 3], usize)> = Vec::new();
    for p in pieces {
        let sm = &mesh.submeshes[p.sub];
        for &t in &p.tris {
            let k = |j: usize| &mesh.vertices[sm.indices[t as usize * 3 + j] as usize];
            let v = [Vec3::from(k(0).pos), Vec3::from(k(1).pos), Vec3::from(k(2).pos)];
            let n = (v[1] - v[0]).cross(v[2] - v[0]);
            let vn = Vec3::from(k(0).normal) + Vec3::from(k(1).normal) + Vec3::from(k(2).normal);
            let n = if n.dot(vn) < 0.0 { -n } else { n };
            all.push((v, n.normalize_or_zero(), [Vec2::from(k(0).uv), Vec2::from(k(1).uv), Vec2::from(k(2).uv)], p.sub));
        }
    }
    for f in surfaces {
        let size = (f.hi - f.lo) * f.s;
        let (w, h) = (size.x.ceil() as usize + 1, size.y.ceil() as usize + 1);
        let mut zbuf = vec![f32::MIN; w * h];
        let slack = 0.15 * size.max_element() / f.s;
        for (v, n, uv, sub) in &all {
            let facing = n.dot(f.out);
            let Some((res, base, emi, ei)) = maps.get(sub) else { continue };
            if facing <= 0.05 {
                continue;
            }
            let d = v.map(|p| p.dot(f.out));
            if d.iter().all(|&z| z < f.depth.0 - slack) || d.iter().all(|&z| z > f.depth.1 + slack) {
                continue;
            }
            let q = v.map(|p| ((p.dot(f.right) - f.lo.x) * f.s, (f.hi.y - p.dot(f.up)) * f.s));
            let area = (q[1].0 - q[0].0) * (q[2].1 - q[0].1) - (q[2].0 - q[0].0) * (q[1].1 - q[0].1);
            if area.abs() < 1e-9 {
                continue;
            }
            let x0 = q.iter().map(|p| p.0).fold(f32::MAX, f32::min).floor().max(0.0) as usize;
            let x1 = (q.iter().map(|p| p.0).fold(f32::MIN, f32::max).ceil().max(0.0) as usize).min(w);
            let y0 = q.iter().map(|p| p.1).fold(f32::MAX, f32::min).floor().max(0.0) as usize;
            let y1 = (q.iter().map(|p| p.1).fold(f32::MIN, f32::max).ceil().max(0.0) as usize).min(h);
            let shade = 0.8 + 0.2 * facing;
            for y in y0..y1 {
                for x in x0..x1 {
                    let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                    let w0 = ((q[1].0 - px) * (q[2].1 - py) - (q[2].0 - px) * (q[1].1 - py)) / area;
                    let w1 = ((q[2].0 - px) * (q[0].1 - py) - (q[0].0 - px) * (q[2].1 - py)) / area;
                    let w2 = 1.0 - w0 - w1;
                    if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                        continue;
                    }
                    let z = w0 * d[0] + w1 * d[1] + w2 * d[2];
                    if z <= zbuf[y * w + x] {
                        continue;
                    }
                    zbuf[y * w + x] = z;
                    let t = uv[0] * w0 + uv[1] * w1 + uv[2] * w2;
                    let r = *res as f32;
                    let (tx, ty) = ((t.x.rem_euclid(1.0) * r) as usize % *res as usize, (t.y.rem_euclid(1.0) * r) as usize % *res as usize);
                    let at = (ty * *res as usize + tx) * 4;
                    let c = [0, 1, 2].map(|i| {
                        let lin = base.get(at + i).copied().unwrap_or(0.0) * shade + emi.get(at + i).copied().unwrap_or(0.0) * ei;
                        (lin.clamp(0.0, 1.0).powf(1.0 / 2.2) * 255.0).round() as u8
                    });
                    let (ix, iy) = (f.ox as u32 + x as u32, f.oy as u32 + y as u32);
                    if ix < img.width() && iy < img.height() {
                        img.put_pixel(ix, iy, image::Rgba([c[0], c[1], c[2], 255]));
                    }
                }
            }
        }
    }
    for (k, f) in surfaces.iter().enumerate() {
        draw_number(&mut img, f.ox as i32 + (k.to_string().len() as i32 * 12 + 3) / 2, f.oy as i32 - 14, k);
    }
    img
}

/// The texture set an image is laid out on (a UV sheet made for this
/// model): what it shows (its opaque pixels, or those unlike its corners)
/// falls inside that set's UV islands, which leave room around them.
/// Also whether it is that set's UV layout itself (it fills the islands
/// with a few flat colors: a guide, not a texture).
fn uv_sheet_of(path: &std::path::Path, sets: &[(String, &crate::bake::UvRaster)]) -> Option<(String, bool)> {
    let img = image::open(path).ok()?.to_rgba8();
    let (w, h) = img.dimensions();
    let alpha = img.pixels().any(|p| p[3] < 250);
    let corners = [(0, 0), (w - 1, 0), (0, h - 1), (w - 1, h - 1)].map(|(x, y)| img.get_pixel(x, y).0);
    let bg = [0, 1, 2].map(|c| corners.iter().map(|p| p[c] as i32).sum::<i32>() / 4);
    let shows = |p: &image::Rgba<u8>| if alpha { p[3] > 12 } else { (0..3).map(|c| (p[c] as i32 - bg[c]).abs()).sum::<i32>() > 36 };
    let step = ((w as u64 * h as u64 / 250_000) as f64).sqrt().max(1.0) as u32;
    let mut content = Vec::new();
    for y in (0..h).step_by(step as usize) {
        for x in (0..w).step_by(step as usize) {
            if shows(img.get_pixel(x, y)) {
                content.push((x as f32 / w as f32, y as f32 / h as f32));
            }
        }
    }
    let samples = ((w / step) * (h / step)).max(1) as f32;
    if (content.len() as f32) < 0.005 * samples {
        return None;
    }
    // Spread over the layout (a single word or logo in the middle of its
    // image would fit inside any big island).
    let (lo, hi) = content.iter().fold(((1.0f32, 1.0f32), (0.0f32, 0.0f32)), |(l, h), &(u, v)| ((l.0.min(u), l.1.min(v)), (h.0.max(u), h.1.max(v))));
    if hi.0 - lo.0 < 0.25 || hi.1 - lo.1 < 0.25 {
        return None;
    }
    let mut best: Option<(String, f32)> = None;
    for (name, r) in sets {
        let res = r.res as i32;
        let covered = r.tri.iter().filter(|&&t| t >= 0).count() as f32 / r.tri.len().max(1) as f32;
        if covered > 0.85 {
            continue;
        }
        // Inside an island, or within 2 texels of one (soft edges).
        let inside = |u: f32, v: f32| {
            let (cx, cy) = ((u * res as f32) as i32, (v * res as f32) as i32);
            (-2..=2).any(|dy| (-2..=2).any(|dx| {
                let (x, y) = (cx + dx, cy + dy);
                x >= 0 && y >= 0 && x < res && y < res && r.tri[(y * res + x) as usize] >= 0
            }))
        };
        let hit = content.iter().filter(|&&(u, v)| inside(u, v)).count() as f32 / content.len() as f32;
        if hit >= 0.97 && best.as_ref().map_or(true, |b| hit > b.1) {
            best = Some((name.clone(), hit));
        }
    }
    let (name, _) = best?;
    let r = sets.iter().find(|s| s.0 == name)?.1;
    let res = r.res as i32;
    let (mut isl, mut cov) = (0usize, 0usize);
    let mut colors: HashMap<[u8; 3], usize> = HashMap::new();
    for y in (0..h).step_by(step as usize) {
        for x in (0..w).step_by(step as usize) {
            let (tx, ty) = ((x as f32 / w as f32 * res as f32) as i32, (y as f32 / h as f32 * res as f32) as i32);
            if tx < res && ty < res && r.tri[(ty * res + tx) as usize] >= 0 {
                isl += 1;
                let p = img.get_pixel(x, y);
                if shows(p) {
                    cov += 1;
                    *colors.entry([p[0] / 32, p[1] / 32, p[2] / 32]).or_default() += 1;
                }
            }
        }
    }
    let mut counts: Vec<usize> = colors.into_values().collect();
    counts.sort_unstable_by(|a, b| b.cmp(a));
    let flat = counts.iter().take(3).sum::<usize>() as f32 >= 0.95 * cov.max(1) as f32;
    let layout = cov as f32 >= 0.8 * isl.max(1) as f32 && flat;
    Some((name, layout))
}

/// Text rendered as a print image (its color, transparent around it),
/// saved once next to Muse's files. `font`: a family name from
/// `text_fonts`, `bold` its bold face when it has one.
fn text_image(text: &str, font: &str, bold: bool, color: [f32; 3]) -> anyhow::Result<std::path::PathBuf> {
    use ab_glyph::{Font, FontVec, PxScale, ScaleFont};
    let file = text_fonts()
        .into_iter()
        .find(|(n, _, _)| n.eq_ignore_ascii_case(font.trim()))
        .or_else(|| text_fonts().into_iter().next())
        .map(|(_, r, b)| if bold { b.unwrap_or(r) } else { r })
        .ok_or_else(|| anyhow::anyhow!("no font found"))?;
    let key = {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (text, &file, color.map(f32::to_bits)).hash(&mut h);
        h.finish()
    };
    let dest = muse_dir().join("prints").join(format!("text_{key:016x}.png"));
    if dest.exists() {
        return Ok(dest);
    }
    let font = FontVec::try_from_vec(std::fs::read(&file)?)?;
    let sf = font.as_scaled(PxScale::from(160.0));
    let lines: Vec<&str> = text.lines().collect();
    let line_h = sf.height() + sf.line_gap();
    let widths: Vec<f32> = lines
        .iter()
        .map(|l| {
            let mut x = 0.0;
            let mut prev = None;
            for c in l.chars() {
                let g = sf.glyph_id(c);
                if let Some(p) = prev {
                    x += sf.kern(p, g);
                }
                x += sf.h_advance(g);
                prev = Some(g);
            }
            x
        })
        .collect();
    let width = widths.iter().copied().fold(1.0f32, f32::max);
    let pad = 12.0;
    let (iw, ih) = ((width + 2.0 * pad).ceil() as u32, (line_h * lines.len() as f32 + 2.0 * pad).ceil() as u32);
    let mut cover = vec![0.0f32; (iw * ih) as usize];
    for (li, l) in lines.iter().enumerate() {
        let mut x = pad + (width - widths[li]) * 0.5;
        let base = pad + li as f32 * line_h + sf.ascent();
        let mut prev = None;
        for c in l.chars() {
            let g = sf.glyph_id(c);
            if let Some(p) = prev {
                x += sf.kern(p, g);
            }
            if let Some(o) = font.outline_glyph(g.with_scale_and_position(sf.scale(), ab_glyph::point(x, base))) {
                let bb = o.px_bounds();
                o.draw(|gx, gy, a| {
                    let (px, py) = (bb.min.x as i32 + gx as i32, bb.min.y as i32 + gy as i32);
                    if px >= 0 && py >= 0 && (px as u32) < iw && (py as u32) < ih {
                        let at = (py as u32 * iw + px as u32) as usize;
                        cover[at] = (cover[at] + a).min(1.0);
                    }
                });
            }
            x += sf.h_advance(g);
            prev = Some(g);
        }
    }
    let rgb = color.map(|c| (c.clamp(0.0, 1.0).powf(1.0 / 2.2) * 255.0).round() as u8);
    let img = image::RgbaImage::from_fn(iw, ih, |x, y| {
        let a = cover[(y * iw + x) as usize];
        image::Rgba([rgb[0], rgb[1], rgb[2], (a * 255.0).round() as u8])
    });
    std::fs::create_dir_all(dest.parent().unwrap())?;
    img.save(&dest)?;
    Ok(dest)
}

/// Fonts text prints can use (Windows' own): family, regular file, bold file.
fn text_fonts() -> Vec<(String, std::path::PathBuf, Option<std::path::PathBuf>)> {
    let dir = std::path::Path::new(r"C:\Windows\Fonts");
    let list = [
        ("Arial", "arial.ttf", "arialbd.ttf"),
        ("Arial Narrow", "ARIALN.TTF", "ARIALNB.TTF"),
        ("Segoe UI", "segoeui.ttf", "segoeuib.ttf"),
        ("Bahnschrift", "bahnschrift.ttf", ""),
        ("Verdana", "verdana.ttf", "verdanab.ttf"),
        ("Tahoma", "tahoma.ttf", "tahomabd.ttf"),
        ("Trebuchet MS", "trebuc.ttf", "trebucbd.ttf"),
        ("Calibri", "calibri.ttf", "calibrib.ttf"),
        ("Impact", "impact.ttf", ""),
        ("Times New Roman", "times.ttf", "timesbd.ttf"),
        ("Georgia", "georgia.ttf", "georgiab.ttf"),
        ("Courier New", "cour.ttf", "courbd.ttf"),
        ("Consolas", "consola.ttf", "consolab.ttf"),
    ];
    list.iter()
        .filter(|(_, r, _)| dir.join(r).exists())
        .map(|(n, r, b)| (n.to_string(), dir.join(r), Some(dir.join(b)).filter(|p| !b.is_empty() && p.exists())))
        .collect()
}

// ------------------------------------------------------------------ the API

/// A JSON request to the Meta Model API.
pub(super) fn post(key: &str, path: &str, body: Value) -> anyhow::Result<Value> {
    let r = ureq::post(&format!("{API}{path}"))
        .set("Authorization", &format!("Bearer {key}"))
        .set("Content-Type", "application/json")
        .timeout(Duration::from_secs(300))
        .send_string(&body.to_string());
    let read = |resp: ureq::Response| {
        let mut s = String::new();
        std::io::Read::read_to_string(&mut resp.into_reader(), &mut s).map(|_| s)
    };
    match r {
        Ok(resp) => Ok(serde_json::from_str(&read(resp)?)?),
        Err(ureq::Error::Status(code, resp)) => {
            let text = read(resp).unwrap_or_default();
            let msg = serde_json::from_str::<Value>(&text).ok().and_then(|v| v["error"]["message"].as_str().map(String::from)).unwrap_or(text);
            anyhow::bail!("Meta Model API error {code}: {}", msg.chars().take(300).collect::<String>())
        }
        Err(e) => Err(e.into()),
    }
}

/// The patterns Muse may use: all but the normal map generators.
fn pattern_names() -> Vec<&'static str> {
    ProcKind::ALL.iter().filter(|k| !k.is_normal()).map(|k| k.name()).collect()
}

fn instructions(info: &str, sets: &str, mats: &[String], smart: &[String], acg: Option<&[String]>) -> String {
    // ambientCG, when its catalog could be read.
    let (acg_source, acg_text) = match acg {
        Some(tags) => (
            "
        or {\"ambientcg\": \"3 to 6 tag words\", \"scale\": 0.5, \"tint\": \"#RRGGBB\", \"roughness_min\": 0.2, \"roughness_max\": 0.5, \"relief\": 0.5}",
            format!(
                "
ambientCG (about 2000 more scanned materials, CC0, downloaded when used): {{\"ambientcg\": \"tag words\"}} picks the material whose tags match your words best (category first: metal, wood, fabric, leather, tiles, rock, ground, paint, plastic...; then color, finish, condition: \"metal gold scratched\", \"wood planks oak dark\", \"leather brown worn\"; a word with a minus leaves out the scans with that tag: \"leaf green -ground -fallen\"). Use it when no local scan fits; it takes the same keys as a scanned material. The scan it found is named in that layer's \"got\" (name, category, tags) in your stacks. Tags it knows: {}.
",
                tags.join(", ")
            ),
        ),
        None => ("", String::new()),
    };
    let gens = Generator::ALL.iter().map(|g| g.name()).collect::<Vec<_>>().join(", ");
    let mblends = MaskBlend::ALL.iter().map(|b| b.name()).collect::<Vec<_>>().join(", ");
    format!(
        r##"You are the best texture artist alive, a master of Material Forge, a layer-based PBR texture painting app like Substance 3D Painter, which you know better than anyone: every channel, blend mode, generator, pattern and filter below, and exactly how the stack computes. You texture the user's 3D model the way they ask. Unless they ask for a stylized look, the result must be indistinguishable from a photograph of the real object. Realism comes from CHOOSING THE RIGHT MATERIAL for each piece: the scanned material that already looks like the real one (same kind, same color and darkness, same roughness), at the right scale. Extra layers are a small finishing touch, never the look itself: too many layers wash out a good scan and make it look fake.

THE MODEL. It is split into PIECES: each piece is one connected mesh piece, or several alike ones grouped (the links of a chain, the rivets of a door); a part made of one single mesh is split into its UV islands. The image shows the model from four sides; each piece has its own flat color and its number written on it; gray is out of scope. The user sees the same image with the same numbers. Part names usually mean nothing (Cylinder.004, Material.002): identify each piece by its number, color, shape, size and place.
{info}
TEXTURE SETS (current settings): {sets}

MATERIALS GO BY PIECES. A look is one material with its layers, applied to a list of pieces, and masked exactly to their polygons. Give every piece in scope to exactly one look on the first answer; pieces of the same material share one look (list them together). Separate materials by pieces, never with masks: masks inside a look are for the rare effect on that material (grime in crevices). Only when one piece clearly holds two materials (a guard and grip modeled as one mesh) may a layer inside its look cover a zone of it, with a sharp Position gradient cut (contrast 60 or more) placed from the piece's spans.

THIS IS A CONVERSATION. The first message asks for a look of the whole object. Later messages ask for changes or ask questions; the current looks come with them. Change only what is asked: return in "looks" only the looks you change or add, each with its complete layer list (copy the layers you keep exactly as they are, then make the asked change). A look takes its pieces away from the looks that had them, so to give piece 3 another material, return a new look with "pieces": [3] and leave the others alone. To change a look, return it with the same pieces. Return no looks at all for a question or a remark. When the user names a piece by its number, use that piece. Never start over unless the user asks for it.
REFERENCE IMAGES. The user may attach photos or images, numbered R1, R2... in the order they arrive. Study them closely: what each material really is (dark oiled oak, black hammered wrought iron), its color and darkness (sample it as hex where it is neither in shadow nor in a highlight), its roughness and shine. Then pick, for each, the scan whose own color and roughness are closest, and tint it toward the sampled color. Add wear, dirt or dust only if the reference clearly shows it. Say in your reply which scans you chose and why. A reference can also be put on the model itself as a PRINT (labels, logos, printed text, a screen image), see PRINTS below.

PRINTS (labels, logos, printed text, markings, a screen image). A print layer's source is its image: {{"reference": n}}; or {{"text": "VOLUME", "font": "Arial", "bold": false, "color": "#FFFFFF"}}, text you write that the app draws (use it for the printed words the object has and no image gives, in the font closest to the real one: {fonts}; "\n" starts a new line); or a texture_prompt. Plus ONE placement in the source:
- "surface": k, "box": [x0, y0, x1, y1]: the box the print fills on surface k of the SURFACES image (pixels of that image). The precise way, use it whenever the face is on that image: each surface is shown flat, upright and to scale, with its holes, edges and what sits in front of it, so put each print exactly where it is on the real object relative to those features (under that slot, left of those buttons), at its real size (the grid gives the scale). Never over what sits in front of the surface (its list gives the boxes): a print there is hidden. The print goes into the texture there once, upright, its blank border cropped, its proportions kept inside the box; it may run onto neighboring surfaces that continue the same face.
- "surface": k, "near": "4b", "side": "above", "size": 6: a print beside something listed in front of surface k (a button, knob, switch, port, handle, hinge, slot or door: its name from that list, or a piece number for all its parts); the app puts it centered "above", "below", "left" or "right" of it, "size" pixels of the SURFACES image tall (its width follows), "gap" pixels away (default half the size). The exact way for the label of a control: every label naming a control or part (button, knob, switch, port, slot, gauge) is placed with near, one print per control as the reference shows them (not one line of words for several controls).
- "side": "+x" (a side of the SIDES facts): the print covers that whole face of the product, on every piece seen there with its folds and flaps (the panels of a box or a carton, the covers of a book, the back of a phone): THE way for a product photographed face by face, one print per face, never surface boxes for those. With a reference and no crop or quad, the app finds the face on the photo's background itself, straightens it, takes out the photo's color cast ("balance": false keeps its colors), and turns it (or trades it with the photo you gave the opposite side) so the prints meet across the product's edges as on the real one; Say which way each face stands: "up": "top", "right", "bottom" or "left", the edge of the photo that is at the top of that face on the real product as it stands: the edge the tops of its letters point to (text running down the photo, read from its top to its bottom, has its letters' tops toward the right edge; text running up the photo, toward the left); for the top and bottom sides also "toward": the side that edge touches ("+z"). Your stacks then give "turned": the turn the app applied (clockwise, from the photo); change "up" to fix a face, or force a turn with "rotate": 0, 90, 180 or 270. Give each photo to the side with its proportions (the photo facts give each face's proportions, the SIDES facts each side's; opposite sides share them): a face 0.41 wide for 1 tall goes on a side of 2.41 or 0.41. Photos of the whole object at an angle show which faces touch which: use them to choose the sides, not as prints. "area": [x0, y0, x1, y1] (0 to 1 of the side, from its top left) puts a print on part of a side instead, its proportions kept ("stretch": true fills the area).
- "wrap": {{"facing": "-z", "from": 0.3, "to": 0.75, "arc": 140}}: the print goes around a round body (a label on a bottle, a jar, a can, a tube, a candle, a cup), on every piece of the look there, whatever their UVs: its middle facing that side, between "from" and "to" along the body's axis (0 its bottom, 1 its top, read on the views), over "arc" degrees of its circumference (360 all around). Leave out "to" or "arc" and the app takes it from the print's proportions; leave out "from" and "to" and it covers the straight part of the body. A label cut from a photo of the product: "crop" or "quad" plus the source's "arc" (unrolled first), then "wrap" with the same arc. Prefer it to surface boxes for labels on round bodies.
- "decal": [x0, y0, x1, y1]: a box on the NUMBERED VIEWS image instead (its pixels; never the self check render), only for a face no surface shows. Drawn tight around where the print is, in the view that shows that face best; put there once, upright as that view shows it.
- "uv_sheet": true: only for artwork the message says fits inside the UV islands of a texture set, made for that UV map (never a photo or a logo): it goes on exactly as it is.
- Part of an image as the print, when no artwork file gives it (a label, a logo or a printed panel taken from a product photo): add to the source "crop": [x0, y0, x1, y1] (a box on Rn, its pixels as you see it), or "quad": [[x, y], [x, y], [x, y], [x, y]], its four corners top left, top right, bottom right, bottom left, exactly on the part's edges, for a part seen at an angle: the app straightens it to a flat rectangle at its real proportions. Add "arc": degrees when that part wraps around a round surface in the photo (a label on a bottle or a can: the share of the circumference you see of it, often 100 to 150): the app unrolls it so it is not squeezed at its sides. The app moves each side onto the part's edge in the photo when it is a few pixels off ("snap": false keeps your corners exactly). Then place it like any print: a whole label goes in a surface box covering the label's surface with "stretch": true (it fills the box exactly, as the real label covers its piece). Take the whole label in one print, its corners on its edges, never one print per word.
- "fit": true: the image stretched over the look's pieces as seen from the way they face: a TV or phone screen, a label covering a whole flat panel. Only for a piece that faces one way. A lit screen: the print layer with "emissive": "#FFFFFF" glows with the image's own colors (a dimmer gray for a dimmer screen); no other emissive layer over it.
A print layer gives only color (plus roughness if the print is glossier than the surface), blend Normal for an image with transparency, Screen for a light print on a black background (white text), Multiply for a dark print on a white background. Text drawn by the app comes in its color with transparency: blend Normal. One layer per print; prints do not count in the 1 to 3 layers of a look, and many small labels each get their own box. Never place a print with a Planar or UV projection, a Transform filter or a position mask: those repeat it all over the piece.

IMAGES AS TOOLS (references, and the project files the user imported, are numbered R1, R2...; each comes with its size as you see it):
- A material sampled from a photo: a layer source {{"sample": n, "region": [x0, y0, x1, y1], "flatten": true, "scale": 2}}: that region of Rn (its pixels as you see it) is cut out, made seamless and tiled over the pieces like a scan, giving color (add the layer's own roughness, metallic). The way to get a surface no scan or pattern gives: hand painted, sponged, marbled or mottled glaze, a precise wood, stone or fabric, a printed pattern. Pick the largest region that shows only that surface, seen flat on, inside its edges, away from reflections and highlights; "flatten": true takes out the light falloff across the region (a smooth gradient of the photo's lighting), keeping the pattern itself: true for a photo of a lit object. "mean": "#RRGGBB" gives the sample that average color, its variations kept: a photo is lit and reads lighter and paler than the surface's own color, so set it to the real color of that surface (the base color you would give it). Size it with the projection's "repeat": the repeats across the look's own pieces (1: the region shows the whole part once, right when it is a photo of that very part), instead of "scale".
- An image as a mask step: {{"reference": n, "uv_sheet": true}} (laid out on the UV map: its light areas let the layer through) or {{"reference": n, "scale": 2}} (tiled). The way to use artwork masks: one layer per ink, foil, varnish or embossing, its color and roughness (and a little height for embossing), with the artwork image as its mask.
- An image as a channel map: "height_pattern", "roughness_pattern" or "metallic_pattern": {{"reference": n, "uv_sheet": true, "min": 0.4, "max": 0.6}}: the image's lightness drives that channel from min to max (paper grain, a ribbed texture, an embossing map). With "uv_sheet" the whole layer goes on the UV map.
- Project files: images the user imported into the project, made for this model. Look at every one and use each for what it is: a black and white artwork laid out on the UV map is a mask (uv_sheet); a full color sheet on the UV map is a print (uv_sheet); a close photo of a surface is a sample; a logo is a print.

HOW THE STACK COMPUTES (exactly what Material Forge does):
- Channels: color, roughness, metallic, height, emissive, opacity, normal, and the optional ones (specular, transmission, ior, absorption, scattering, coat, coat_roughness, sheen, sheen_roughness, anisotropy, anisotropy_angle). Every channel is computed on its own, layer by layer from the bottom, over the texture set's base values.
- A layer writes ONLY the channels it gives. A layer giving only "roughness" changes only roughness; a layer meant for relief must give only height; a dirt layer gives color and roughness (and metallic 0 over metal). Never give color to a layer that should not change the color: it would paint over everything below.
- For each channel a layer gives: result = mix(below, blend(below, layer value), weight), weight = mask x layer_opacity x that channel's opacity ("channel_opacity").
- Blend modes per channel ("blend": one mode for every channel the layer gives, or an object with a mode per channel {{"color": "Multiply", "roughness": "Normal"}}, needed as soon as its channels must blend differently, like grime: color Multiply but roughness Normal; the default is Normal for every channel and Normal map combine for the normal):
  Normal: the layer's value replaces what is below (by the weight); the relief of the normal maps below stays. Replace: the same, but on height it also flattens all the relief below (normals included): use it for a smooth coat of paint or a polished patch over a rough surface.
  Multiply: below x value (darken color: grime, stains, wetness; scale roughness down). Divide, Inverse divide: below / value, value / below.
  Linear dodge (add): below + value (stack relief: bumps added on the base relief; extra roughness). Subtract: below - value (dents, scratches and engravings carved into the height; less roughness). Inverse subtract: value - below. Signed addition: below + 2 x value - 1 (0.5 changes nothing: a pattern centered on 0.5 raises and lowers).
  Overlay, Soft light, Hard light, Vivid light, Pin light, Linear burn, Color burn, Color dodge, Screen, Darken (min), Lighten (max), Difference, Exclusion: as in Photoshop. Overlay and Soft light with a pattern centered on 0.5 (or a mid gray color) add contrast detail without shifting the average: the best tool for color variation, roughness breakup and fine height detail.
  Hue, Saturation, Color, Value: color only (recolor while keeping the value below, or the reverse).
  Normal map combine (default) and Normal map detail: the normal channel only.
  Optional channels always blend Normal.
- Recipes: base material, all Normal; color variation: a pattern layer giving only color, Overlay or Soft light, layer_opacity 0.1 to 0.3; grime: color Multiply with a warm gray (#8A8070 to #B0A898) plus roughness Normal; roughness breakup: a layer giving only roughness_pattern, Overlay, opacity 0.3 to 0.6; height detail over a scan: a layer giving only height_pattern, Linear dodge or Subtract (min 0, max 0.2 to 0.6) or Overlay centered on 0.5; wet or polished spots: roughness only, Darken (min) or Normal with a low value; dust: color Normal at low opacity plus roughness 0.9.

HEIGHT AND RELIEF. Relief comes from height that VARIES (a pattern, a scan): a uniform height shows nothing. The slope of the relief, so how strong it reads, grows with three things multiplied: the texture set's "height_scale", the height range of the layer (max - min of its height_pattern, times its opacity) and how fine the pattern is (its scale). So: broad shapes (pattern scale 1 to 4: waviness, dents, warping) can use the whole range 0 to 1; medium detail (scale 5 to 15: hammer marks, grain) 0.1 to 0.3; fine detail (scale above 15: bubbles, pores, scratches, fibers) only 0.03 to 0.1, more looks like crackle or noise. height_scale (0 to 0.2) is per texture set: 0.02 (default) suits most objects with the ranges above; 0.03 to 0.05 for deep relief (stone, bark, embossing, heavy hammering); above that only for very coarse surfaces. Raise it with "texture_sets" only when a set's main relief is too weak. Scans bring their own height and normal map ("relief" scales them). Glossy surfaces (roughness 0.3 or less: glaze, lacquer, glossy paint and plastic, polished metal and stone, glass) mirror their surroundings and the reflection shows every bump: keep them smooth, with no waviness layer; only a surface that is visibly uneven in real life (hand blown glass, hand thrown pottery, hammered metal) gets height, within a range of 0.03 (the app caps it there).

WHAT YOU WRITE. One JSON object, nothing else:
{{"reply": "your answer to the user, 1 to 3 short plain sentences without dashes: what you did (name the pieces by number), or the answer to their question",
 "analysis": "(first message only) what the object is, its real size, and what each piece is and what real material it is made of",
 "style": "product, realistic or stylized (when the message leaves it to you)",
 "texture_sets": [{{"name": "<set name>", "height_scale": 0.05, "alpha_mode": "Auto", "emissive_intensity": 1.0}}],  (only the sets and settings you change; alpha_mode: Auto, Opaque, Alpha test, Alpha blend)
 "looks": [{{"pieces": [0, 2, 5], "what": "what this material is on the object", "finish": "plastic", "age": 0.3, "layers": [ ...layers, bottom first... ]}}]}}

TEXTURING STYLE. Each message says the style, or leaves it to you: then choose it from the object and the request, write it in "style" ("product", "realistic" or "stylized") in your answer, and keep it after.
- product: a product as in its official photos and pack shots (packaging, cosmetics, a bottle, a box, electronics, a toy in its box): brand new and flawless, no wear, dust, grime, scratches, smudges, fingerprints, bumps or waviness; every color, finish and print exactly as the reference images show (take the colors from them, the prints from them). Every look "age": 0 and "finish": "none" (the app enforces it). Surfaces perfectly smooth unless the material itself is textured (paper grain, fabric weave, leather grain).
- realistic: a real object in use, photoreal, with the wear its age and life give (finish and age as below).
- stylized: a game or cartoon look: clean flat or hand-painted colors, painted light and edges, no scans unless asked; every look "finish": "none".

REALISM FINISH. Every look gets "finish": one of {finishes}; and "age": 0 (brand new, showroom) to 1 (old, heavily used), 0.2 to 0.4 for an object in normal use (older for an antique or a worn tool). On top of the look's layers the app then adds subtle, physically based detail tuned for that material: roughness variation, smudges or fine scratches, grime in the crevices (from the baked mesh maps), dust on the faces looking up, rubbed or chipped edges. It is what makes a surface read as real instead of CG, so never leave it out; do not make your own grime, dust, smudge or edge wear layers. The "Finish:" layers are rebuilt from finish and age each time: change those to change them. "none" only for what is perfectly clean (a lit screen, a light, a liquid).

A layer:
{{"name": "Brass base",
 "source": {{"material": "<exact scanned material name>", "scale": 0.5, "tint": "#RRGGBB", "roughness_min": 0.2, "roughness_max": 0.5, "relief": 0.5}}
        or {{"color": "#RRGGBB"}}
        or {{"pattern": "<pattern>", "color": "#RRGGBB", "color2": "#RRGGBB", "scale": 4, "contrast": 1.0, "balance": 0.5, "warp": 0.0, "seed": 0, "invert": false}}
        or {{"smart_material": "<exact library name>"}}
        or {{"texture_prompt": "a seamless texture to generate"}}
        or {{"reference": 1}}{acg_source},
 "roughness": 0.4, "metallic": 1.0, "height": 0.5, "emissive": "#RRGGBB", "opacity": 1.0,
 "roughness_pattern": {{"pattern": "<pattern>", "scale": 8, "min": 0.3, "max": 0.45, "contrast": 1, "seed": 0}}, (also metallic_pattern, height_pattern)
 "transmission": 1.0, "ior": 1.5, "absorption": 0.1, "scattering": 0.0, "coat": 0.0, "coat_roughness": 0.05, "sheen": 0.0, "sheen_roughness": 0.3, "specular": 0.5, "anisotropy": 0.0, "anisotropy_angle": 0,
 "blend": "Normal" or {{"<channel>": "<mode>", ...}}, "layer_opacity": 1.0, "channel_opacity": {{"height": 0.5}},
 "projection": {{"mode": "Tri-planar", "scale": 2, "rotation": 0, "offset": [0, 0], "along": "length", "repeat": 1}},  (modes: Tri-planar (default), UV, Planar X, Planar Y, Planar Z; see PROJECTION; prints use a decal box or fit instead)
 "mask": {{"base": "black", "steps": [ ...mask steps in order... ]}},
 "filters": [ ...filters... ]}}
PROJECTION, chosen per layer from the UV LAYOUT facts and the material: Tri-planar is seamless on any shape but ignores the shape's direction: fibers, grain, veins, planks, weave, stripes, scales and brushed metal come out in fixed world directions and smear or turn wrong on curved, slanted or thin parts (a bent trunk, leaves, a plank at an angle, a strap). UV follows the shape through its UV layout: use it for every directional material (and for leaves, petals, cloth, straps, thin cards) on pieces whose UVs are usable and even, with "along": "length" (the texture's grain runs along each piece's length: bark fibers along the trunk, veins along the leaf, grain along the board) or "across" (rings around a trunk, stripes across a strap); the app measures the grain of the texture or pattern itself and the direction of each UV island, and turns it to match. In UV mode "scale" keeps the same meaning (repeats across the model's longest side): the app converts it to the UV tiling from the measured texel size. Pieces or islands pointing different ways in the UV are fine: the app splits such a layer into one copy per direction, each turned for its islands. Tri-planar for non-directional materials (stone, plaster, rust, concrete, plain plastic) and for pieces with no usable UVs or badly stretched UVs. UV seams can show a cut in a tiled texture where islands meet: acceptable for organic materials, avoid UV on hard surfaces with many small islands. Stacked islands share texels: fine for tiled materials, never for unique prints.
A scanned material brings its own color, roughness, metallic, height and normal maps. "scale" is how many times its texture repeats across the model's longest side: never below 1 (a stretched scan is blurry and low quality); 2 to 4 for most objects, up to 6 to 8 for fine grain on a large object. "tint" is the average color it should end up with (the scan's detail stays around it; leave it out to keep the scan's own color). A tint can only darken a scan or shift its hue, never brighten it: choose a scan at least as light as the color you want; roughness_min/max remap its roughness map. "relief" (0 to 1, default 0.5) is the strength of its height and normal detail: 0 to 0.15 for polished or smooth finishes, 0.3 to 0.6 for most surfaces, 0.8 to 1 for deep texture; 0 also drops its normal map. Patterns: "contrast" sharpens, "balance" shifts toward color2 (0 to 1), "warp" distorts (0 to 1), "invert" swaps the ends.

Mask steps (each blends into the mask built so far, "blend" one of: {mblends}):
{{"generator": "<generator>", "level": 0.5, "contrast": 4, "invert": false, "noise": 0.3, "noise_scale": 12, "seed": 0, "axis": "y", "direction": [0, 1, 0], "height_reactive": 0, "blend": "Multiply", "opacity": 1}}
{{"pattern": "<pattern>", "scale": 6, "contrast": 1, "balance": 0.5, "warp": 0, "seed": 0, "invert": false, "blend": "Multiply", "opacity": 1}}
{{"levels": [0.4, 0.6], "gamma": 1}}  {{"threshold": 0.5}}  {{"invert": true}}  {{"blur": 4}}
Generators (from the baked mesh maps): {gens}. A generator's value v becomes clamp((v - level) x contrast + 0.5); invert flips it; "noise" (0 to 1, default 0.1 for wear and dirt generators, 0 for the others) adds fbm noise of "noise_scale" (default 12) to v first, breaking up the border (0.2 to 0.3 for natural wear). "height_reactive" (0 to 1) makes edge and cavity generators follow the height painted in the layers below (wear on the bumps of a scan or a height pattern, not only on the mesh edges). Edges: convex edges and corners; Cavity: crevices; Ambient occlusion: occluded areas; Dirt: occlusion and crevices; Edge wear: chipped edges; Curvature (raw): 0.5 flat, above convex, below concave; Position gradient: v is the coordinate on the axis (x, y or z) in the units given above (level 0.62 with contrast 60 is a sharp cut, white above; invert: white below); Directional light: faces turned toward a direction ([0, 1, 0]: faces looking up, where dust settles; [0, -1, 0]: undersides, drips and soot); Thickness: thin areas (where light passes through).

Patterns (procedural, seamless; scale = repetitions across the model's longest side; the Grunge maps and Dirt, Fractal Sum, Moisture, Creased and Leaks ones make the most natural wear masks and breakups): {patterns}.
Filters (on the layer's own content; "channels" picks the channels they change, default color, or color and height for blur, sharpen, levels, invert, highpass):
{{"filter": "HSL", "hue": 0.0, "saturation": 1.0, "lightness": 0.0}}, {{"filter": "Brightness / contrast", "brightness": 0, "contrast": 1}}, {{"filter": "Levels", "low": 0, "high": 1, "gamma": 1}}, {{"filter": "Blur", "radius": 4}}, {{"filter": "Sharpen", "amount": 1, "radius": 2}}, {{"filter": "Tint", "color": "#RRGGBB", "amount": 0.5}}, {{"filter": "Gradient map", "dark": "#RRGGBB", "bright": "#RRGGBB"}}, {{"filter": "Color correct", "brightness": 0, "contrast": 1, "gamma": 1, "saturation": 1, "hue": 0}}, {{"filter": "Desaturate"}}, {{"filter": "Invert"}}, {{"filter": "Posterize", "steps": 6}}, {{"filter": "Clamp", "low": 0, "high": 1}}, {{"filter": "Height adjust", "scale": 1, "offset": 0}}, {{"filter": "Histogram scan", "position": 0.5, "contrast": 0.5}}, {{"filter": "Highpass", "radius": 8}}, {{"filter": "Transform", "offset_u": 0, "offset_v": 0, "rotation": 0, "scale": 1}}; each may add "channels": ["color", "roughness", "height", ...].

Scanned materials (photographed, the most realistic choice), by category, each with its number on the MATERIAL SHEET (the second image of the first message: every scan rendered on a sphere, numbered in its corner; look at it and choose by eye), its own average color and average roughness (write the material by its exact name, not its number; choose by these: the scan already closest in color and roughness to the real material needs the least tint and looks the most real; one far from it never looks right, however you tint it):
{mats}
{acg_text}Smart materials (layered looks; the "Hand-painted" ones are stylized, only for a stylized request): {smart}.

HOW TO WORK, for a photoreal result.
1. In "analysis", say what the object is in real life, its real size, and what each piece is and what real material it is made of, with its finish and age (polished brass, handled for decades; clear soda-lime glass, mouth blown; blackened wrought iron, outdoors).
2. CHOOSE THE MATERIAL, the step that decides the result. For metal, wood, leather, fabric, stone, concrete, brick and plaster, pick the scanned material (local or ambientCG) of the same kind and finish whose average color and roughness are closest to the real one (or to the reference): dark wood needs a dark wood scan, not a pale one tinted dark. Scale 2 to 4, never below 1. Tint only to nudge its color toward the real one; leave the tint out when the scan is already right. Keep the material's hue and warmth: wood is a warm brown (red clearly above green, green above blue), never grayed. Pick a scan whose surface matches the finish: Polished Silver or Smooth Grey Metal for polished steel, Brushed Steel only for visibly brushed metal. A plain color base only for paint, plastic, rubber, ceramic, glass and liquids.
3. Physically based values. Metals: metallic 1, color from real reflectance: clean polished iron and steel #C4C7C7, aged or forged iron #5A5C5E to #3C3C3C (roughness 0.55 to 0.75), aluminium #F5F6F6, chrome #C8CACB, silver #FBF8F2, gold #FFD58A, copper #F7C1A6, brass #E3C27C, bronze #C49A6C, blackened iron #3C3C3C; roughness 0.15 to 0.3 polished, 0.35 to 0.6 worn. Non-metals: metallic 0, color never darker than #282828 or brighter than #F0F0F0, real-world colors are muted; roughness 0.4 to 0.9 for most surfaces.
4. Glass, crystal, liquids, gems: a base giving color near white (a faint tint), metallic 0, roughness 0.02 to 0.08, transmission 1, ior (glass 1.5, water 1.33, crystal 1.6, diamond 2.4), absorption 0.05 to 0.3 for a colored thickness tint. Transmission shows the environment through the glass, never the pieces behind it: glass covering a screen, a dial or a print is not see-through here; put the screen's image on that glass itself ("fit": true, glowing) over a dark glossy base (roughness 0.05). Made glass is smooth: no height layer. Only hand blown glass gets one layer giving ONLY height: seeds (Gaussian Spots, scale 25 to 40, range 0 to 0.03, Linear dodge); the set's height_scale left at 0.02.
5. KEEP IT SIMPLE: 1 to 3 layers per look. The base material, then at most one or two subtle layers when they clearly help: a roughness or height breakup on a plain color base, a subtle color variation. Everyday wear (grime in crevices, dust, smudges, fine scratches, rubbed edges) comes from the look's finish, never from your own layers; add your own only for heavier damage the user asks for or the reference clearly shows (rust, stains, bleaching, deep scratches), thin and subtle (layer_opacity 0.2 to 0.5). No layer may change a material's overall color or value: the piece must still read as the material you chose. Never stack layers that only repeat what the scan already has (grain, pores, roughness variation). Never make hammer marks, dents or pores with Cells or Voronoi patterns: their cell borders read as a network of cracks; choose a scan that already has that surface (ambientCG: "metal hammered", "metal forged").
6. The looks of one object share one coherent story (same age, same environment). Do not invent bright colors, glow or patterns the user did not ask for; emissive only for real light sources.
7. texture_prompt only for printed graphics, labels or ornaments that nothing above can make."##,
        patterns = pattern_names().join(", "),
        fonts = text_fonts().iter().map(|f| f.0.clone()).collect::<Vec<_>>().join(", "),
        finishes = FINISHES,
        smart = smart.join(", "),
        mats = mats.join("\n"),
    )
}

/// A plan layer's source (Muse sometimes writes its keys on the layer itself).
fn source(l: &Value) -> &Value {
    if l["source"].is_object() {
        &l["source"]
    } else {
        l
    }
}

/// Muse's text without long dashes (the panel never shows them).
fn no_dashes(s: &str) -> String {
    s.replace(" \u{2014} ", ", ").replace(['\u{2014}', '\u{2013}'], ", ")
}

/// Where Muse keeps its files (views, plans, generated textures).
pub(super) fn muse_dir() -> std::path::PathBuf {
    crate::library::library_dir().join("..").join("muse")
}

/// What the model is, for the conversation's first turn.
struct FirstTurn {
    mesh: Arc<crate::mesh::MeshData>,
    /// (texture set, submesh, object, name) of each part in scope.
    parts: Vec<(usize, usize, Option<u32>, String)>,
    /// The scanned materials and the plain presets.
    tex: Vec<crate::library::TexMaterial>,
    presets: Vec<String>,
    smart: Vec<String>,
}

/// The material list Muse reads: one line per category, each scan with
/// its number on the material sheet, its average color and roughness
/// ("Metal: 12 Brushed Steel (#8a8986, rough 0.35, metal), ..."), then the
/// plain presets; and the sheet itself: every scan's preview sphere with
/// its number, so Muse chooses by eye.
fn material_lines(tex: &[crate::library::TexMaterial], presets: &[String]) -> (Vec<String>, image::RgbaImage) {
    let stats = crate::library::material_stats(tex);
    let mut lines = Vec::new();
    let mut order: Vec<&crate::library::TexMaterial> = Vec::new();
    for cat in crate::library::MATERIAL_CATEGORIES {
        let mut items: Vec<String> = Vec::new();
        for (m, (color, rough)) in tex.iter().zip(&stats).filter(|(m, _)| m.category == cat) {
            order.push(m);
            let mut facts: Vec<String> = color.iter().cloned().collect();
            facts.extend(rough.map(|r| format!("rough {r:.2}")));
            if m.metal || m.map("metallic").is_some() {
                facts.push("metal".into());
            }
            let n = order.len();
            items.push(if facts.is_empty() { format!("{n} {}", m.name) } else { format!("{n} {} ({})", m.name, facts.join(", ")) });
        }
        if !items.is_empty() {
            lines.push(format!("{cat}: {}", items.join(", ")));
        }
    }
    if !presets.is_empty() {
        lines.push(format!("Plain presets: {}", presets.join(", ")));
    }
    // The sheet: tiles in the list's order, numbered in their corner.
    const TILE: u32 = 112;
    const COLS: u32 = 12;
    let rows = (order.len() as u32).div_ceil(COLS).max(1);
    let mut sheet = image::RgbaImage::from_pixel(TILE * COLS, TILE * rows, image::Rgba([28, 28, 30, 255]));
    for (i, m) in order.iter().enumerate() {
        let (x, y) = ((i as u32 % COLS) * TILE, (i as u32 / COLS) * TILE);
        if let Some(img) = m.preview().and_then(|p| image::open(p).ok()) {
            let t = img.resize_exact(TILE, TILE, image::imageops::FilterType::Triangle).to_rgba8();
            image::imageops::overlay(&mut sheet, &t, x as i64, y as i64);
        }
        let w = (i + 1).to_string().len() as i32 * 12 + 3;
        draw_number(&mut sheet, x as i32 + w / 2 + 2, y as i32 + 12, i + 1);
    }
    (lines, sheet)
}

struct Request {
    key: String,
    first: Option<FirstTurn>,
    system: String,
    view: String,
    /// The material sheet (a data URL), sent with the first message.
    sheet: String,
    /// The SURFACES image (a data URL), sent with the first message.
    surf: String,
    turns: Vec<Turn>,
    /// The stacks applied now (JSON), empty before the first answer.
    stacks: String,
    /// The texture sets' settings now.
    sets: String,
    user: String,
    /// Reference images sent with it (data URLs).
    refs: Vec<String>,
    /// A self check (Muse looking at its result), not a user message.
    check: bool,
    /// The ambientCG scans of the looks applied now: (query, asset id).
    scans: Vec<(String, String)>,
}

/// The work off the UI thread: (first turn) the views and instructions,
/// Muse Spark's answer, then Muse Image's textures and ambientCG's
/// materials.
fn run(mut req: Request, tx: &mpsc::Sender<Msg>, stop: &AtomicBool) -> anyhow::Result<TurnOut> {
    let b64 = &base64::engine::general_purpose::STANDARD;
    let mut notes = Vec::new();
    let _ = tx.send(Msg::Status("Muse Spark is looking at the model...".into()));
    // ambientCG's catalog (on disk after the first time; MF_MUSE_OFFLINE:
    // none, for checks).
    let catalog = match std::env::var_os("MF_MUSE_OFFLINE").map_or_else(crate::ambientcg::catalog, |_| Err(anyhow::anyhow!("offline"))) {
        Ok(c) => Some(c),
        Err(e) => {
            notes.push(format!("ambientCG is not reachable ({e:#}): only the local library is used."));
            None
        }
    };
    let dir = muse_dir();
    let _ = std::fs::create_dir_all(&dir);
    let mut first = None;
    if let Some(f) = req.first.take() {
        let pieces = pieces::find(&f.mesh, &f.parts);
        let (view, info) = look_at(&f.mesh, &pieces);
        let _ = view.save(dir.join("last_view.png"));
        let (surf, surfaces, surf_info) = surfaces_of(&f.mesh, &pieces);
        let _ = surf.save(dir.join("last_surfaces.png"));
        let _ = std::fs::write(dir.join("last_surfaces.txt"), &surf_info);
        let uv_info = uv_facts(&f.mesh, &pieces);
        let _ = std::fs::write(dir.join("last_uv.txt"), &uv_info);
        let side_info = sides_facts(&f.mesh, &pieces);
        let _ = std::fs::write(dir.join("last_sides.txt"), &side_info);
        let info = info + &surf_info + &side_info + &uv_info;
        let mut png = Vec::new();
        surf.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)?;
        req.surf = format!("data:image/png;base64,{}", b64.encode(&png));
        let mut png = Vec::new();
        view.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)?;
        req.view = format!("data:image/png;base64,{}", b64.encode(&png));
        let vocab = catalog.as_ref().map(|c| crate::ambientcg::vocabulary(c, 300));
        let _ = tx.send(Msg::Status("Muse Spark is reading the materials...".into()));
        let (mats, sheet) = material_lines(&f.tex, &f.presets);
        let _ = sheet.save(dir.join("material_sheet.png"));
        let mut jpg = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpg, 85).encode_image(&image::DynamicImage::ImageRgba8(sheet).to_rgb8())?;
        req.sheet = format!("data:image/jpeg;base64,{}", b64.encode(&jpg));
        req.system = instructions(&info, &req.sets, &mats, &f.smart, vocab.as_deref()) + &super::muse_light::rules();
        first = Some(Box::new(First { system: req.system.clone(), view: req.view.clone(), sheet: req.sheet.clone(), surf: req.surf.clone(), surfaces, pieces }));
    }
    anyhow::ensure!(!stop.load(Ordering::Relaxed), "Cancelled.");
    // MF_MUSE_PLAN=<json>: replays a saved plan (`muse/last_plan.json`)
    // instead of asking Muse Spark (texturing work without API calls);
    // "a.json;b.json": one per turn.
    let plan: Value = if let Some(p) = std::env::var("MF_MUSE_PLAN").ok() {
        let files: Vec<&str> = p.split(';').filter(|f| !f.is_empty()).collect();
        let f = files.get(req.turns.len()).or(files.last()).copied().unwrap_or_default();
        parse_answer(&std::fs::read_to_string(f)?)?
    } else {
        let _ = tx.send(Msg::Status("Muse Spark is thinking...".into()));
        let mut msgs = vec![serde_json::json!({"role": "system", "content": req.system})];
        let n = req.turns.len();
        for i in 0..=n {
            let (text, refs) = if i < n {
                (req.turns[i].user.clone(), &req.turns[i].refs)
            } else if req.stacks.is_empty() {
                (req.user.clone(), &req.refs)
            } else {
                (format!("{}\n\nCURRENT LOOKS (applied now; each look's pieces, what it is and its layers, bottom first):\n{}\n\nTEXTURE SETS NOW: {}", req.user, req.stacks, req.sets), &req.refs)
            };
            // The views go with the first message, references with theirs.
            let mut content = vec![serde_json::json!({"type": "text", "text": text})];
            if i == 0 {
                content.push(serde_json::json!({"type": "image_url", "image_url": {"url": req.view}}));
                if !req.sheet.is_empty() {
                    content.push(serde_json::json!({"type": "image_url", "image_url": {"url": req.sheet}}));
                }
                if !req.surf.is_empty() {
                    content.push(serde_json::json!({"type": "image_url", "image_url": {"url": req.surf}}));
                }
            }
            for r in refs {
                content.push(serde_json::json!({"type": "image_url", "image_url": {"url": r}}));
            }
            msgs.push(serde_json::json!({"role": "user", "content": content}));
            if i < n {
                msgs.push(serde_json::json!({"role": "assistant", "content": req.turns[i].reply}));
            }
        }
        let v = post(&req.key, "/chat/completions", serde_json::json!({"model": TEXT_MODEL, "messages": msgs}))?;
        anyhow::ensure!(!stop.load(Ordering::Relaxed), "Cancelled.");
        let text = v["choices"][0]["message"]["content"].as_str().unwrap_or_default().to_string();
        // Muse's answer as written (muse/last_answer.txt), before reading it.
        let _ = std::fs::write(dir.join("last_answer.txt"), &text);
        match parse_answer(&text) {
            Ok(plan) => plan,
            // Not readable even repaired: Muse is asked once to send it again.
            Err(e) => {
                let _ = tx.send(Msg::Status("Muse's answer was not valid JSON: asking it again...".into()));
                msgs.push(serde_json::json!({"role": "assistant", "content": text}));
                msgs.push(serde_json::json!({"role": "user", "content": format!("Your answer is not valid JSON ({e}). Send the same answer again as one valid JSON object, nothing else.")}));
                let v = post(&req.key, "/chat/completions", serde_json::json!({"model": TEXT_MODEL, "messages": msgs}))?;
                anyhow::ensure!(!stop.load(Ordering::Relaxed), "Cancelled.");
                let text = v["choices"][0]["message"]["content"].as_str().unwrap_or_default().to_string();
                let _ = std::fs::write(dir.join("last_answer.txt"), &text);
                parse_answer(&text).map_err(|e| anyhow::anyhow!("Muse's answer was not valid JSON, even sent again ({e}); it is in muse/last_answer.txt"))?
            }
        }
    };
    // The last plan, and every turn's in `plans` (turn_1 starts a conversation).
    let text = serde_json::to_string_pretty(&plan).unwrap_or_default();
    let _ = std::fs::write(dir.join("last_plan.json"), &text);
    let file = if req.check { format!("turn_{}_check.json", req.turns.len()) } else { format!("turn_{}.json", req.turns.len() + 1) };
    let _ = std::fs::create_dir_all(dir.join("plans")).and_then(|_| std::fs::write(dir.join("plans").join(file), &text));
    let layers: Vec<Value> = plan["looks"].as_array().or_else(|| plan["parts"].as_array()).into_iter().flatten().flat_map(|p| p["layers"].as_array().cloned().unwrap_or_default()).collect();
    // The textures it asked for.
    let mut images = HashMap::new();
    let prompts: Vec<String> = layers.iter().filter_map(|l| source(l)["texture_prompt"].as_str().map(String::from)).collect();
    for (i, t) in prompts.iter().enumerate() {
        anyhow::ensure!(!stop.load(Ordering::Relaxed), "Cancelled.");
        if images.contains_key(t) {
            continue;
        }
        let _ = tx.send(Msg::Status(format!("Muse Image is painting texture {} of {}...", i + 1, prompts.len())));
        let img = post(
            &req.key,
            "/images/generations",
            serde_json::json!({
                "model": IMAGE_MODEL,
                "prompt": format!("{t}. Seamless tileable texture filling the whole frame, seen straight on, flat even lighting, no shadows, no perspective, no objects, no text."),
                "n": 1,
                "size": "1024x1024",
                "response_format": "b64_json",
                "output_format": "png",
            }),
        )?;
        let data = img["data"][0]["b64_json"].as_str().ok_or_else(|| anyhow::anyhow!("Muse Image returned no image"))?;
        images.insert(t.clone(), image::load_from_memory(&b64.decode(data)?)?.to_rgba8());
    }
    // The ambientCG materials it asked for.
    let mut downloads = HashMap::new();
    for q in layers.iter().filter_map(|l| source(l)["ambientcg"].as_str().map(String::from)) {
        anyhow::ensure!(!stop.load(Ordering::Relaxed), "Cancelled.");
        let Some(cat) = &catalog else { break };
        if downloads.contains_key(&q) {
            continue;
        }
        // A new query for a scan already on the model asks for another
        // one: the scans got so far are not picked again.
        let avoid: Vec<String> = if req.scans.iter().any(|(q0, _)| *q0 == q) { Vec::new() } else { req.scans.iter().map(|s| s.1.clone()).collect() };
        match crate::ambientcg::best(cat, &q, &avoid) {
            Some(a) => {
                let _ = tx.send(Msg::Status(format!("Getting {} from ambientCG...", a.name)));
                match crate::ambientcg::download(a, stop) {
                    Ok(d) => {
                        let tags: Vec<&str> = a.tags.iter().take(8).map(String::as_str).collect();
                        downloads.insert(q, (d, format!("{} ({}), category {}, tags: {}", a.name, a.id, a.category, tags.join(", "))));
                    }
                    Err(e) => notes.push(format!("ambientCG {}: {e:#}", a.id)),
                }
            }
            None => notes.push(format!("ambientCG has nothing for \"{q}\".")),
        }
    }
    Ok(TurnOut { first, user: req.user, refs: req.refs, plan, images, downloads, notes, check: req.check })
}

// ------------------------------------------------------------------ the plan

/// Muse's answer as a plan: its JSON object (a plain answer is a reply
/// without changes). The slips a model makes in long JSON are repaired
/// when it does not read as it is: comments, missing or trailing commas,
/// a bracket closed with the wrong kind, an answer cut short.
pub(super) fn parse_answer(text: &str) -> anyhow::Result<Value> {
    let (Some(a), Some(b)) = (text.find('{'), text.rfind('}')) else {
        return Ok(serde_json::json!({"reply": text.trim()}));
    };
    let body = if b > a { &text[a..=b] } else { &text[a..] };
    match serde_json::from_str::<Value>(body) {
        Ok(v) => Ok(v),
        // A bracket of the wrong kind: a missing closer, or one too many.
        // The reading whose looks hold the most layers (with their pieces)
        // is the one Muse meant.
        Err(e) => {
            let score = |v: &Value| -> usize {
                v["looks"].as_array().into_iter().flatten().filter(|l| l["pieces"].is_array()).map(|l| l["layers"].as_array().map_or(0, |a| a.len()) + 1).sum()
            };
            [false, true]
                .into_iter()
                .filter_map(|drop| serde_json::from_str::<Value>(&repair_json(&text[a..], drop)).ok())
                .max_by_key(|v| score(v))
                .ok_or_else(|| anyhow::anyhow!("{e}"))
        }
    }
}

/// `drop`: a closing bracket of the wrong kind is one too many (dropped),
/// instead of closing the open ones first.
fn repair_json(s: &str, drop: bool) -> String {
    let mut out = String::with_capacity(s.len() + 16);
    let mut stack: Vec<char> = Vec::new();
    let chars: Vec<char> = s.chars().collect();
    // The last thing written outside strings: a value just ended (a comma
    // is missing before the next one), or a ',' ':' '[' '{'.
    let mut after_value = false;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            '"' => {
                if after_value {
                    out.push(',');
                }
                // The whole string, escapes kept, raw newlines escaped.
                out.push('"');
                i += 1;
                while i < chars.len() && chars[i] != '"' {
                    match chars[i] {
                        '\\' if i + 1 < chars.len() => {
                            out.push('\\');
                            out.push(chars[i + 1]);
                            i += 1;
                        }
                        '\n' => out.push_str("\\n"),
                        '\r' => {}
                        '\t' => out.push_str("\\t"),
                        ch => out.push(ch),
                    }
                    i += 1;
                }
                out.push('"');
                // A key: its ':' follows; a value otherwise.
                let next = chars[(i + 1).min(chars.len())..].iter().find(|c| !c.is_whitespace());
                after_value = next != Some(&':');
            }
            '/' if chars.get(i + 1) == Some(&'/') => {
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
                continue;
            }
            '/' if chars.get(i + 1) == Some(&'*') => {
                i += 2;
                while i + 1 < chars.len() && !(chars[i] == '*' && chars[i + 1] == '/') {
                    i += 1;
                }
                i += 2;
                continue;
            }
            '{' | '[' => {
                if after_value {
                    out.push(',');
                }
                stack.push(c);
                out.push(c);
                after_value = false;
            }
            '}' | ']' => {
                // A trailing comma goes; a bracket of the wrong kind closes
                // the open one(s) first.
                while out.ends_with(|ch: char| ch.is_whitespace()) {
                    out.pop();
                }
                if out.ends_with(',') {
                    out.pop();
                }
                let want = if c == '}' { '{' } else { '[' };
                if stack.last() == Some(&want) || (!drop && stack.contains(&want)) {
                    while let Some(open) = stack.pop() {
                        out.push(if open == '{' { '}' } else { ']' });
                        if open == want {
                            break;
                        }
                    }
                }
                after_value = true;
            }
            ',' => {
                if !out.trim_end().ends_with([',', '[', '{']) {
                    out.push(',');
                }
                after_value = false;
            }
            ':' => {
                out.push(':');
                after_value = false;
            }
            c if c.is_whitespace() => out.push(c),
            _ => {
                // A number, true, false or null.
                if after_value {
                    out.push(',');
                }
                while i < chars.len() && !(chars[i].is_whitespace() || matches!(chars[i], ',' | ']' | '}' | ':' | '"')) {
                    out.push(chars[i]);
                    i += 1;
                }
                after_value = true;
                continue;
            }
        }
        i += 1;
    }
    // Cut short: an unfinished last member goes (back to the last comma
    // or opening bracket), then what is still open gets closed.
    if !stack.is_empty() {
        let t = out.trim_end();
        if !t.ends_with(['}', ']']) && !t.ends_with(['"', 'e', 'l']) && !t.ends_with(|c: char| c.is_ascii_digit()) || t.ends_with(':') {
            let cut = out.rfind([',', '{', '[']).unwrap_or(0);
            out.truncate(cut + usize::from(!out[cut..].starts_with(',')));
        } else if stack.last() == Some(&'{') && out.trim_end().ends_with('"') && out[..out.trim_end().len() - 1].rfind('"').is_some_and(|q| out[..q].trim_end().ends_with([',', '{'])) {
            // A key without its value.
            let cut = out.rfind([',', '{']).unwrap_or(0);
            out.truncate(cut + usize::from(!out[cut..].starts_with(',')));
        }
    }
    while out.ends_with(|ch: char| ch.is_whitespace() || ch == ',') {
        out.pop();
    }
    while let Some(open) = stack.pop() {
        out.push(if open == '{' { '}' } else { ']' });
    }
    out
}

fn hex(v: &Value) -> Option<[f32; 3]> {
    let s = v.as_str()?.trim().trim_start_matches('#');
    if s.len() != 6 {
        return None;
    }
    let c = |i: usize| u8::from_str_radix(&s[i..i + 2], 16).ok().map(|v| (v as f32 / 255.0).powf(2.2));
    Some([c(0)?, c(2)?, c(4)?])
}

/// The words of a material name that say what it is, not its color,
/// finish or number ("Dark Espresso Wood": espresso, wood).
fn kind_words(name: &str) -> Vec<String> {
    const SKIP: [&str; 24] = [
        "dark", "light", "pale", "brown", "grey", "gray", "black", "white", "red", "green", "orange", "blue", "yellow", "polished", "clean", "dirty", "worn",
        "old", "smooth", "rough", "weathered", "spotted", "coarse", "fine",
    ];
    name.split(|c: char| !c.is_alphabetic()).map(str::to_lowercase).filter(|w| w.len() > 2 && !SKIP.contains(&w.as_str())).collect()
}

/// A linear color in CIELAB (D65), for color distances.
fn lab(c: [f32; 3]) -> [f32; 3] {
    let x = (0.4124 * c[0] + 0.3576 * c[1] + 0.1805 * c[2]) / 0.9505;
    let y = 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
    let z = (0.0193 * c[0] + 0.1192 * c[1] + 0.9505 * c[2]) / 1.089;
    let f = |t: f32| if t > 0.008856 { t.cbrt() } else { 7.787 * t + 16.0 / 116.0 };
    [116.0 * f(y) - 16.0, 500.0 * (f(x) - f(y)), 200.0 * (f(y) - f(z))]
}

fn lab_dist(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

/// The average linear color of an image file (a scanned material's color map).
fn average_color(path: &std::path::Path) -> Option<[f32; 3]> {
    let img = image::open(path).ok()?.thumbnail(64, 64).to_rgb8();
    let n = (img.width() * img.height()).max(1) as f32;
    let mut sum = [0.0f32; 3];
    for p in img.pixels() {
        for i in 0..3 {
            sum[i] += (p[i] as f32 / 255.0).powf(2.2);
        }
    }
    Some(sum.map(|s| s / n))
}

/// A channel by the name Muse uses.
fn channel_index(name: &str) -> Option<usize> {
    match name.trim().to_lowercase().as_str() {
        "color" | "base color" | "basecolor" | "albedo" => Some(CH_COLOR),
        "roughness" => Some(CH_ROUGH),
        "metallic" | "metalness" => Some(CH_METAL),
        "height" => Some(CH_HEIGHT),
        "emissive" => Some(CH_EMISSIVE),
        "opacity" => Some(CH_OPACITY),
        "normal" => Some(CH_NORMAL),
        _ => None,
    }
}

/// A blend mode by its name (or a common short one: add, min, max).
fn blend_mode(v: &Value) -> Option<BlendMode> {
    let alias = match v.as_str()?.trim().to_lowercase().as_str() {
        "add" | "linear dodge" | "additive" => Some(BlendMode::LinearDodge),
        "min" | "darken" => Some(BlendMode::Darken),
        "max" | "lighten" => Some(BlendMode::Lighten),
        "normal map combine" | "combine" => Some(BlendMode::NormalCombine),
        _ => None,
    };
    alias.or_else(|| by_name(BlendMode::ALL, |b| b.name(), v))
}

fn num(v: &Value, k: &str) -> Option<f32> {
    v[k].as_f64().map(|x| x as f32)
}

fn by_name<T: Copy>(all: &[T], name: fn(&T) -> &'static str, v: &Value) -> Option<T> {
    let s = v.as_str()?.trim().to_lowercase();
    all.iter().find(|k| name(k).to_lowercase() == s).or_else(|| all.iter().find(|k| name(k).to_lowercase().starts_with(&s))).copied()
}

fn proc_kind(v: &Value) -> Option<ProcKind> {
    by_name(ProcKind::ALL, |k| k.name(), v)
}

/// Pattern parameters common to fills and masks.
fn proc_params(kind: ProcKind, v: &Value) -> ProcParams {
    let mut p = ProcParams::of(kind);
    if let Some(c) = num(v, "contrast") {
        p.contrast = c.clamp(0.1, 8.0);
    }
    if let Some(s) = num(v, "seed") {
        p.seed = s;
    }
    if let Some(b) = num(v, "balance") {
        p.balance = b.clamp(0.0, 1.0);
    }
    if let Some(w) = num(v, "warp") {
        p.warp = w.clamp(0.0, 1.0);
    }
    p.invert = v["invert"].as_bool().unwrap_or(false);
    if kind == ProcKind::BrushStrokes {
        p.density = 0.5;
        p.roughness = 0.6;
    }
    p
}

fn mask_step(v: &Value, first: bool) -> Option<MaskEffect> {
    let blend = by_name(MaskBlend::ALL, |b| b.name(), &v["blend"]).unwrap_or(if first { MaskBlend::Normal } else { MaskBlend::Multiply });
    let opacity = num(v, "opacity").unwrap_or(1.0).clamp(0.0, 1.0);
    let kind = if let Some(g) = by_name(Generator::ALL, |g| g.name(), &v["generator"]) {
        let mut d = GeneratorDesc::of(g);
        d.level = num(v, "level").unwrap_or(d.level);
        d.contrast = num(v, "contrast").unwrap_or(d.contrast);
        d.invert = v["invert"].as_bool().unwrap_or(false);
        // (The library defaults reach 0.5, which spreads wear and dirt
        // far past edges and cavities; Muse sets it when it wants more.
        // Position cuts keep none.)
        d.noise = num(v, "noise").unwrap_or(d.noise.min(0.1)).clamp(0.0, 1.0);
        d.noise_scale = num(v, "noise_scale").unwrap_or(d.noise_scale).clamp(0.5, 200.0);
        d.seed = num(v, "seed").unwrap_or(d.seed);
        d.height_reactive = num(v, "height_reactive").unwrap_or(0.0).clamp(0.0, 1.0);
        if let Some(a) = v["axis"].as_str() {
            d.axis = match a.trim().to_lowercase().as_str() {
                "x" => 0,
                "z" => 2,
                _ => 1,
            };
        }
        if let Some(d3) = v["direction"].as_array().filter(|a| a.len() == 3) {
            d.dir = [0, 1, 2].map(|i| d3[i].as_f64().unwrap_or(0.0) as f32);
        }
        MaskEffectKind::Generator(d)
    } else if let Some(k) = proc_kind(&v["pattern"]) {
        let mut e = MaskEffectKind::fill_procedural(k);
        if let MaskEffectKind::Fill { procedural, projection, .. } = &mut e {
            *procedural = proc_params(k, v);
            projection.mode = ProjMode::Triplanar;
            projection.scale = num(v, "scale").unwrap_or(4.0).clamp(0.05, 200.0);
        }
        e
    } else if let Some(id) = v["texture_id"].as_u64() {
        // An image: on the UV map as it is, or tiled.
        let mut e = MaskEffectKind::fill_procedural(ProcKind::Clouds1);
        if let MaskEffectKind::Fill { source, texture, projection, .. } = &mut e {
            *source = SourceKind::Texture;
            *texture = Some(id);
            *projection = if v["uv_sheet"].as_bool() == Some(true) {
                Projection { mode: ProjMode::Uv, scale: 1.0, ..Default::default() }
            } else {
                Projection { mode: ProjMode::Triplanar, scale: num(v, "scale").unwrap_or(2.0).clamp(0.05, 200.0), ..Default::default() }
            };
        }
        e
    } else if let Some(l) = v["levels"].as_array().filter(|a| a.len() == 2) {
        let (lo, hi) = (l[0].as_f64().unwrap_or(0.0) as f32, l[1].as_f64().unwrap_or(1.0) as f32);
        MaskEffectKind::Levels { in_lo: lo, in_hi: hi.max(lo + 0.001), gamma: num(v, "gamma").unwrap_or(1.0).clamp(0.1, 10.0), out_lo: 0.0, out_hi: 1.0 }
    } else if let Some(t) = num(v, "threshold") {
        MaskEffectKind::Threshold { value: t.clamp(0.0, 1.0) }
    } else if v["invert"].as_bool() == Some(true) {
        MaskEffectKind::Invert
    } else if let Some(r) = num(v, "blur") {
        MaskEffectKind::Blur { radius: r.clamp(0.5, 64.0) }
    } else {
        return None;
    };
    // Levels, invert and blur work on the mask so far.
    let blend = if matches!(kind, MaskEffectKind::Generator(_) | MaskEffectKind::Fill { .. }) { blend } else { MaskBlend::Normal };
    Some(MaskEffect { id: 0, enabled: true, blend, opacity, kind })
}

fn mask(v: &Value) -> Option<MaskDesc> {
    let (base, steps) = match v {
        Value::Array(a) => (0.0, a.clone()),
        Value::Object(_) => (if v["base"].as_str() == Some("white") { 1.0 } else { 0.0 }, v["steps"].as_array().cloned().unwrap_or_default()),
        _ => return None,
    };
    let effects: Vec<MaskEffect> = steps.iter().enumerate().filter_map(|(i, s)| mask_step(s, i == 0)).collect();
    (!effects.is_empty() || base > 0.0).then_some(MaskDesc { enabled: true, base, effects })
}

fn filter(v: &Value) -> Option<FilterDesc> {
    let k = by_name(FilterKind::ALL, |k| k.name(), &v["filter"])?;
    let mut f = FilterDesc::of(k);
    let mut set = |i: usize, key: &str| {
        if let Some(x) = num(v, key) {
            f.p[i] = x;
        }
    };
    match k {
        FilterKind::Hsl => {
            set(0, "hue");
            set(1, "saturation");
            set(2, "lightness");
        }
        FilterKind::BrightnessContrast => {
            set(0, "brightness");
            set(1, "contrast");
        }
        FilterKind::Levels => {
            set(0, "low");
            set(1, "high");
            set(2, "gamma");
        }
        FilterKind::Posterize => set(0, "steps"),
        FilterKind::Blur | FilterKind::Highpass => set(0, "radius"),
        FilterKind::Clamp => {
            set(0, "low");
            set(1, "high");
        }
        FilterKind::HeightAdjust => {
            set(0, "scale");
            set(1, "offset");
        }
        FilterKind::HistogramScan => {
            set(0, "position");
            set(1, "contrast");
        }
        FilterKind::Transform => {
            set(0, "offset_u");
            set(1, "offset_v");
            set(2, "rotation");
            set(3, "scale");
        }
        FilterKind::Sharpen => {
            set(0, "amount");
            set(1, "radius");
        }
        FilterKind::Tint => set(0, "amount"),
        FilterKind::ColorCorrect => {
            set(0, "brightness");
            set(1, "contrast");
            set(2, "gamma");
            set(3, "saturation");
            set(4, "hue");
        }
        _ => {}
    }
    if let Some(c) = hex(&v["color"]).or_else(|| hex(&v["dark"])) {
        f.color = c;
    }
    if let Some(c) = hex(&v["bright"]) {
        f.color2 = c;
    }
    if let Some(list) = v["channels"].as_array() {
        let chosen: Vec<usize> = list.iter().filter_map(|c| c.as_str()).filter_map(channel_index).collect();
        if !chosen.is_empty() {
            f.channels = [false; NCH];
            for c in chosen {
                f.channels[c] = true;
            }
        }
    }
    Some(f)
}

/// Optional channels a layer may write: (key, index, stored value).
const EXT_KEYS: [(&str, usize); 11] = [
    ("specular", 0),
    ("transmission", 1),
    ("scattering", 5),
    ("coat", 6),
    ("coat_roughness", 7),
    ("sheen", 8),
    ("sheen_roughness", 9),
    ("anisotropy", 10),
    ("anisotropy_angle", 11),
    ("ior", 12),
    ("absorption", 13),
];

/// A fill from a plan layer whose source is a color, a pattern or a
/// generated texture (`tex`); the optional channels it uses.
fn fill(v: &Value, tex: Option<u64>) -> (FillDesc, Vec<usize>) {
    let src = source(v);
    let mut f = FillDesc::default();
    for c in f.channels.iter_mut() {
        c.enabled = false;
    }
    let mut scale = 4.0f32;
    if let Some(id) = tex {
        f.channels[CH_COLOR] = FillChannel { source: SourceKind::Texture, texture: Some(id), value: [1.0; 3], ..Default::default() };
        scale = num(src, "scale").unwrap_or(2.0).clamp(0.05, 200.0);
    } else if let Some(k) = proc_kind(&src["pattern"]) {
        let a = hex(&src["color"]).unwrap_or([0.2; 3]);
        let b = hex(&src["color2"]).unwrap_or([0.6; 3]);
        f.channels[CH_COLOR] = FillChannel { source: SourceKind::Procedural, value: a, value_b: b, procedural: proc_params(k, src), ..Default::default() };
        scale = num(src, "scale").unwrap_or(4.0).clamp(0.05, 200.0);
    } else if let Some(c) = hex(&src["color"]) {
        f.channels[CH_COLOR] = FillChannel { value: c, ..Default::default() };
    }
    f.projection = Projection { mode: ProjMode::Triplanar, scale, ..Default::default() };
    if v["uv_maps"].as_bool() == Some(true) {
        f.projection = Projection { mode: ProjMode::Uv, scale: 1.0, ..Default::default() };
    }
    for (key, ch) in [("roughness", CH_ROUGH), ("metallic", CH_METAL), ("height", CH_HEIGHT)] {
        if let Some(p) = v.get(format!("{key}_pattern")).filter(|p| p.is_object()) {
            // An image: its lightness drives the channel from min to max.
            if let Some(id) = p["texture_id"].as_u64() {
                let (lo, hi) = (num(p, "min").unwrap_or(0.0), num(p, "max").unwrap_or(1.0));
                f.channels[ch] = FillChannel { source: SourceKind::Texture, texture: Some(id), value: [lo; 3], value_b: [hi; 3], ..Default::default() };
                continue;
            }
            if let Some(k) = proc_kind(&p["pattern"]) {
                let mut pp = proc_params(k, p);
                pp.scale = num(p, "scale").unwrap_or(scale) / scale.max(1e-3);
                let (lo, hi) = (num(p, "min").unwrap_or(0.0), num(p, "max").unwrap_or(1.0));
                f.channels[ch] = FillChannel { source: SourceKind::Procedural, value: [lo; 3], value_b: [hi; 3], procedural: pp, ..Default::default() };
                continue;
            }
        }
        if let Some(x) = num(v, key) {
            let x = if ch == CH_HEIGHT { x.clamp(-1.0, 1.0) } else { x.clamp(0.0, 1.0) };
            f.channels[ch] = FillChannel { value: [x; 3], ..Default::default() };
        }
    }
    if let Some(c) = hex(&v["emissive"]) {
        // An image glows with its own colors (a lit screen).
        f.channels[CH_EMISSIVE] = match tex {
            Some(id) => FillChannel { source: SourceKind::Texture, texture: Some(id), value: c, ..Default::default() },
            None => FillChannel { value: c, ..Default::default() },
        };
    }
    if let Some(o) = num(v, "opacity") {
        f.channels[CH_OPACITY] = FillChannel { value: [o.clamp(0.0, 1.0); 3], ..Default::default() };
    }
    let mut ext = Vec::new();
    for (key, i) in EXT_KEYS {
        if let Some(x) = num(v, key) {
            // Stored 0..1: IOR from 1..3, the angle from degrees.
            let x = match i {
                12 => (x - 1.0) / 2.0,
                11 => x / 360.0,
                _ => x,
            };
            f.ext[i] = FillChannel { enabled: true, value: [x.clamp(0.0, 1.0); 3], value_b: [1.0; 3], ..Default::default() };
            ext.push(i);
        }
    }
    (f, ext)
}

/// Content effects need ids, like mask effects (`instantiate` gives those).
fn give_ids(doc: &mut Document, l: &mut Layer) {
    for e in &mut l.desc.effects {
        e.id = doc.alloc_id();
    }
    for c in &mut l.children {
        give_ids(doc, c);
    }
}

/// The settings of texture sets Muse can change, as it reads them.
fn sets_text(doc: &Document, sets: &[usize]) -> String {
    sets.iter()
        .map(|&s| {
            let st = &doc.sets[s].settings;
            let on: Vec<&str> = (0..NEXT).filter(|&i| st.ext_channels[i]).map(|i| EXT_NAMES[i]).collect();
            let alpha = match st.alpha_mode {
                AlphaMode::Auto => "Auto",
                AlphaMode::Opaque => "Opaque",
                AlphaMode::AlphaTest => "Alpha test",
                AlphaMode::AlphaBlend => "Alpha blend",
            };
            format!("{}: height_scale {:.3}, alpha_mode {alpha}, emissive_intensity {}, optional channels on: {}", doc.sets[s].name, st.height_scale, st.emissive_intensity, on.join(", "))
        })
        .collect::<Vec<_>>()
        .join("; ")
}

/// An image file as a reference: copied to `muse/refs`, sent as a JPEG of
/// at most 1024 px, with a thumbnail for the panel.
fn load_ref(ctx: &egui::Context, path: &std::path::Path) -> anyhow::Result<Ref> {
    let img = image::open(path)?;
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "image".into());
    let dir = muse_dir().join("refs");
    std::fs::create_dir_all(&dir)?;
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let copy = dir.join(format!("{stamp}_{name}"));
    std::fs::copy(path, &copy)?;
    let small = if img.width().max(img.height()) > 1024 { img.resize(1024, 1024, image::imageops::FilterType::Triangle) } else { img.clone() };
    let mut jpg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpg, 85).encode_image(&small.to_rgb8())?;
    let url = format!("data:image/jpeg;base64,{}", base64::engine::general_purpose::STANDARD.encode(&jpg));
    let t = img.thumbnail(128, 128).to_rgba8();
    let thumb = ctx.load_texture(format!("muse ref {stamp}"), egui::ColorImage::from_rgba_unmultiplied([t.width() as usize, t.height() as usize], t.as_raw()), Default::default());
    Ok(Ref { name, url, path: copy, thumb, project: false })
}

/// What Muse should know of an attached image: its size as Muse sees it
/// (at most 1024 px), where it comes from, black and white, transparency.
fn image_facts(path: &std::path::Path, project: bool) -> String {
    let Ok(img) = image::open(path) else { return String::new() };
    let (w, h) = (img.width(), img.height());
    let k = (w.max(h) as f32 / 1024.0).max(1.0);
    let mut out = format!(", {:.0} x {:.0} px", w as f32 / k, h as f32 / k);
    if project {
        out += ", project file";
    }
    // A photo of a product: the object on its background (its corners in
    // the pixels Muse sees), and a color cast on its white areas.
    if !project {
        let seen = img.thumbnail(1024, 1024).to_rgba8();
        if let Some(f) = photo::find_object(&seen) {
            let q = f.quad;
            let len = |a: [f32; 2], b: [f32; 2]| ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt();
            let (w, h) = ((len(q[0], q[1]) + len(q[3], q[2])) * 0.5, (len(q[0], q[3]) + len(q[1], q[2])) * 0.5);
            let corners: Vec<String> = q.iter().map(|p| format!("[{:.0}, {:.0}]", p[0], p[1])).collect();
            if f.fill >= 0.8 && f.share >= 0.1 {
                out += &format!(
                    ", shows one flat face on a background: corners [{}] (it fills {:.0}% of the photo; proportions {:.2} wide for 1 tall)",
                    corners.join(", "),
                    f.share * 100.0,
                    w / h.max(1.0)
                );
            } else {
                out += &format!(", shows an object on a background over {:.0}% of the photo (not one flat face: a view of the object)", f.share * 100.0);
            }
        }
        if let Some((_, l)) = photo::paper_of(&seen) {
            if (l[1] * l[1] + l[2] * l[2]).sqrt() > 6.0 {
                let way = if l[2].abs() >= l[1].abs() { if l[2] < 0.0 { "blue" } else { "yellow" } } else if l[1] > 0.0 { "magenta" } else { "green" };
                out += &format!(", its main light color is {way} tinted (paper under a {way} light, or a {way} material)");
            }
        }
    }
    let img = img.thumbnail(256, 256).to_rgba8();
    let n = img.pixels().count().max(1) as f32;
    if img.pixels().filter(|p| p[3] < 250).count() as f32 > 0.01 * n {
        out += ", with transparency";
    }
    let gray = img.pixels().all(|p| (p[0] as i32 - p[1] as i32).abs() < 12 && (p[1] as i32 - p[2] as i32).abs() < 12);
    let two = img.pixels().filter(|p| p[0] < 40 || p[0] > 215).count() as f32 > 0.9 * n;
    if gray && two {
        out += ", black and white (a mask)";
    } else if gray {
        out += ", grayscale";
    }
    out
}

/// A cut-out object on a flat background (a single leaf, a decal): a
/// fifth to nine tenths of the image, corners included, is one flat color
/// and the rest stands far from it (a plain scan is near its color).
fn is_cutout(path: &std::path::Path) -> bool {
    let Ok(img) = image::open(path) else { return false };
    let img = img.thumbnail(128, 128).to_rgba8();
    let (w, h) = img.dimensions();
    if w < 4 || h < 4 {
        return false;
    }
    let corners = [(0, 0), (w - 1, 0), (0, h - 1), (w - 1, h - 1)].map(|(x, y)| img.get_pixel(x, y).0);
    let near = |a: [u8; 4], b: [u8; 4]| (0..3).all(|c| (a[c] as i32 - b[c] as i32).abs() <= 10) || (a[3] < 16 && b[3] < 16);
    if !corners.iter().all(|&c| near(c, corners[0])) {
        return false;
    }
    let n = (w * h) as usize;
    let flat = img.pixels().filter(|p| near(p.0, corners[0])).count();
    let rest: Vec<i32> = img.pixels().filter(|p| !near(p.0, corners[0])).map(|p| (0..3).map(|c| (p[c] as i32 - corners[0][c] as i32).abs()).max().unwrap_or(0)).collect();
    let apart = rest.iter().sum::<i32>() as f32 / rest.len().max(1) as f32;
    flat * 5 >= n && flat * 10 <= n * 9 && apart >= 60.0
}

/// An image as a gray map (lightness times opacity) for masks and channel
/// maps, cached in `muse/prints`.
fn gray_image(path: &std::path::Path) -> anyhow::Result<std::path::PathBuf> {
    let dir = muse_dir().join("prints");
    std::fs::create_dir_all(&dir)?;
    let stem = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let out = dir.join(format!("gray_{stem}.png"));
    if out.exists() {
        return Ok(out);
    }
    let img = image::open(path)?.to_rgba8();
    let g = image::GrayImage::from_fn(img.width(), img.height(), |x, y| {
        let p = img.get_pixel(x, y).0;
        let l = 0.2126 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.0722 * p[2] as f32;
        image::Luma([(l * p[3] as f32 / 255.0).round() as u8])
    });
    g.save(&out)?;
    Ok(out)
}

/// A material from a region of an image (pixels as Muse sees it, at most
/// 1024 px): cut out, its light falloff evened out (`flatten`), made
/// seamless (a copy shifted by half cross-fades over its borders), cached
/// in `muse/prints`.
fn sample_texture(path: &std::path::Path, region: [f32; 4], flatten: bool, mean: Option<[f32; 3]>) -> anyhow::Result<std::path::PathBuf> {
    let img = image::open(path)?.to_rgb8();
    let (w, h) = img.dimensions();
    let k = (w.max(h) as f32 / 1024.0).max(1.0);
    let r = region.map(|x| x * k);
    let (x0, x1) = (r[0].min(r[2]).max(0.0), r[0].max(r[2]).min(w as f32));
    let (y0, y1) = (r[1].min(r[3]).max(0.0), r[1].max(r[3]).min(h as f32));
    anyhow::ensure!(x1 - x0 >= 16.0 && y1 - y0 >= 16.0, "the region is too small or outside the image");
    let mut crop = image::imageops::crop_imm(&img, x0 as u32, y0 as u32, (x1 - x0) as u32, (y1 - y0) as u32).to_image();
    let (cw, ch) = crop.dimensions();
    if flatten {
        // The light falloff over the region: a smooth quadratic fit of its
        // lightness, divided out (the same for all channels: hues kept).
        // Only the gradient goes; blotches, washes and grain stay.
        let lum = |p: &image::Rgb<u8>| (p[0] as f64 * 0.2126 + p[1] as f64 * 0.7152 + p[2] as f64 * 0.0722).max(1.0);
        let terms = |x: f64, y: f64| [1.0, x, y, x * x, x * y, y * y];
        let mut ata = [[0.0f64; 6]; 6];
        let mut atb = [0.0f64; 6];
        let step = ((cw.max(ch) / 128).max(1)) as usize;
        for y in (0..ch).step_by(step) {
            for x in (0..cw).step_by(step) {
                let t = terms(x as f64 / cw as f64 - 0.5, y as f64 / ch as f64 - 0.5);
                let l = lum(crop.get_pixel(x, y));
                for i in 0..6 {
                    atb[i] += t[i] * l;
                    for j in 0..6 {
                        ata[i][j] += t[i] * t[j];
                    }
                }
            }
        }
        if let Some(k) = solve6(ata, atb) {
            let n = (cw * ch) as f64;
            let mean = crop.pixels().map(lum).sum::<f64>() / n;
            for (x, y, p) in crop.enumerate_pixels_mut() {
                let t = terms(x as f64 / cw as f64 - 0.5, y as f64 / ch as f64 - 0.5);
                let fit = (0..6).map(|i| k[i] * t[i]).sum::<f64>().max(mean * 0.2);
                for c in 0..3 {
                    p[c] = (p[c] as f64 * mean / fit).round().clamp(0.0, 255.0) as u8;
                }
            }
        }
    }
    let out = image::RgbImage::from_fn(cw, ch, |x, y| {
        let a = crop.get_pixel(x, y).0;
        let b = crop.get_pixel((x + cw / 2) % cw, (y + ch / 2) % ch).0;
        let wx = 1.0 - (2.0 * (x as f32 + 0.5) / cw as f32 - 1.0).abs();
        let wy = 1.0 - (2.0 * (y as f32 + 0.5) / ch as f32 - 1.0).abs();
        let t = (wx.min(wy) * 2.0).min(1.0);
        let t = t * t * (3.0 - 2.0 * t);
        image::Rgb([0, 1, 2].map(|c| (b[c] as f32 + (a[c] as f32 - b[c] as f32) * t).round() as u8))
    });
    // A photo is lit: its pixels are lighter than the surface's own color.
    // "mean" moves the sample's average (linear) to that color, its
    // variations kept in proportion.
    let mut out = out;
    if let Some(m) = mean {
        let lin = |v: u8| (v as f32 / 255.0).powf(2.2);
        let n = (out.width() * out.height()).max(1) as f32;
        let avg = [0, 1, 2].map(|c| out.pixels().map(|p| lin(p[c])).sum::<f32>() / n);
        let k = [0, 1, 2].map(|c| m[c] / avg[c].max(1e-4));
        for p in out.pixels_mut() {
            for c in 0..3 {
                p[c] = ((lin(p[c]) * k[c]).clamp(0.0, 1.0).powf(1.0 / 2.2) * 255.0).round() as u8;
            }
        }
    }
    let dir = muse_dir().join("prints");
    std::fs::create_dir_all(&dir)?;
    let stem = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let tone = mean.map(|m| format!("_{:.0}_{:.0}_{:.0}", m[0] * 1000.0, m[1] * 1000.0, m[2] * 1000.0)).unwrap_or_default();
    let file = dir.join(format!("sample_{stem}_{:.0}_{:.0}_{:.0}_{:.0}{}{tone}.png", region[0], region[1], region[2], region[3], if flatten { "_flat" } else { "" }));
    out.save(&file)?;
    Ok(file)
}

/// Solves a 6 x 6 linear system (Gaussian elimination, partial pivoting).
fn solve6(mut a: [[f64; 6]; 6], mut b: [f64; 6]) -> Option<[f64; 6]> {
    for c in 0..6 {
        let r = (c..6).max_by(|&i, &j| a[i][c].abs().total_cmp(&a[j][c].abs()))?;
        if a[r][c].abs() < 1e-12 {
            return None;
        }
        a.swap(c, r);
        b.swap(c, r);
        for i in c + 1..6 {
            let f = a[i][c] / a[c][c];
            for j in c..6 {
                a[i][j] -= f * a[c][j];
            }
            b[i] -= f * b[c];
        }
    }
    let mut x = [0.0; 6];
    for i in (0..6).rev() {
        x[i] = (b[i] - (i + 1..6).map(|j| a[i][j] * x[j]).sum::<f64>()) / a[i][i];
    }
    Some(x)
}

/// Image files Muse takes as references.
pub(super) fn is_image(path: &std::path::Path) -> bool {
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "webp" | "bmp" | "tga" | "tif" | "tiff")
}

/// Which document a conversation belongs to (its sets and mesh).
fn doc_key(doc: &Document) -> String {
    format!("{}|{}", doc.sets.iter().map(|s| s.name.as_str()).collect::<Vec<_>>().join(","), doc.mesh.vertices.len())
}

/// The ambientCG scans on the model: (query, asset id), from each layer's
/// "got" ("Name (Id), category ...").
fn scans_of(c: &Convo) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for l in c.looks.iter().flat_map(|l| l.layers.as_array().cloned().unwrap_or_default()) {
        let s = source(&l);
        let (Some(q), Some(got)) = (s["ambientcg"].as_str(), s["got"].as_str()) else { continue };
        if let Some(id) = got.split_once(" (").and_then(|(_, r)| r.split_once(')')).map(|(id, _)| id.to_string()) {
            out.push((q.to_string(), id));
        }
    }
    out
}

/// The looks applied now, as Muse reads them (JSON).
fn stacks_text(c: &Convo) -> String {
    let v: Vec<Value> = c.looks.iter().map(|l| serde_json::json!({"pieces": l.pieces, "what": l.what, "layers": l.layers})).collect();
    if v.is_empty() {
        String::new()
    } else {
        Value::Array(v).to_string()
    }
}

/// A chat message: the user's on the right, Muse's on the left.
fn bubble(ui: &mut Ui, line: &ChatLine) {
    let w = ui.available_width();
    let layout = egui::Layout::top_down(if line.user { egui::Align::Max } else { egui::Align::Min });
    ui.with_layout(layout, |ui| {
        let fill = if line.user { crate::style::ACCENT.gamma_multiply(0.3) } else { Color32::from_gray(40) };
        egui::Frame::none().fill(fill).rounding(8.0).inner_margin(egui::Margin::symmetric(10.0, 7.0)).show(ui, |ui| {
            ui.set_max_width(w * 0.86);
            ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                ui.label(&line.text);
                if !line.refs.is_empty() {
                    ui.add_space(4.0);
                    ref_row(ui, &line.refs, 64.0, "Reference sent with this message");
                }
                for n in &line.notes {
                    ui.label(RichText::new(n).small().weak());
                }
            });
        });
    });
    ui.add_space(6.0);
}

/// A small rounded toggle: tinted with the accent when on.
fn chip(ui: &mut Ui, text: &str, on: bool) -> egui::Response {
    let (fill, stroke, color) = if on {
        (crate::style::ACCENT.gamma_multiply(0.25), egui::Stroke::new(1.0f32, crate::style::ACCENT), Color32::WHITE)
    } else {
        (crate::style::FIELD, egui::Stroke::NONE, Color32::from_gray(185))
    };
    ui.add(egui::Button::new(RichText::new(text).small().color(color)).fill(fill).stroke(stroke).rounding(10.0))
}

/// Reference thumbnails in rows of fixed height (a bigger one on hover);
/// the index of the one clicked.
fn ref_row(ui: &mut Ui, thumbs: &[egui::TextureHandle], size: f32, tip: &str) -> Option<usize> {
    let per_row = ((ui.available_width() + 6.0) / (size + 6.0)).floor().max(1.0) as usize;
    let mut clicked = None;
    for (r, chunk) in thumbs.chunks(per_row).enumerate() {
        ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), size), egui::Layout::left_to_right(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            for (k, t) in chunk.iter().enumerate() {
                let resp = ui.add(egui::Image::new(t).fit_to_exact_size(egui::vec2(size, size)).maintain_aspect_ratio(true).rounding(4.0).sense(egui::Sense::click()));
                let resp = resp.on_hover_ui(|ui| {
                    ui.label(tip);
                    ui.add(egui::Image::new(t).max_size(egui::vec2(320.0, 320.0)).rounding(4.0));
                });
                if resp.clicked() {
                    clicked = Some(r * per_row + k);
                }
            }
        });
    }
    clicked
}

impl RetroApp {
    /// The Muse panel (top right by default): a conversation with Muse
    /// about the texturing.
    pub(super) fn muse_panel(&mut self, ui: &mut Ui) {
        self.muse_poll(ui.ctx());
        self.muse_connect_poll(ui.ctx());
        self.muse.rect = Some(ui.max_rect());
        // MF_MUSE_ATTACH=<img;img>: attached at start (UI checks).
        static DEMO: std::sync::Once = std::sync::Once::new();
        DEMO.call_once(|| {
            for p in std::env::var("MF_MUSE_ATTACH").unwrap_or_default().split(';').filter(|p| !p.is_empty()) {
                self.muse_attach(ui.ctx(), std::path::Path::new(p));
            }
            if std::env::var_os("MF_MUSE_PIECES").is_some() {
                self.muse.pieces_on = true;
            }
        });
        // Images dragged over the window go to Muse while its panel is open.
        let hovering = ui.ctx().input(|i| i.raw.hovered_files.iter().any(|f| f.path.as_deref().map_or(true, is_image)));
        if hovering && !self.muse.key.is_empty() {
            let r = ui.max_rect().shrink(4.0);
            let painter = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("muse drop")));
            painter.rect(r, 8.0f32, Color32::from_black_alpha(170), egui::Stroke::new(2.0f32, crate::style::ACCENT));
            painter.text(r.center(), egui::Align2::CENTER_CENTER, "Drop images for Muse", egui::FontId::proportional(18.0), Color32::WHITE);
        }
        let mut close = false;
        panel_header(ui, "Muse", "Texture by conversation", |ui| close = icons::button(ui, Icon::Close, false, 24.0, "Close").clicked());
        if close {
            self.dock.get_or_insert_with(Default::default).muse = false;
            self.muse.pieces_on = false;
        }
        egui::Frame::none().inner_margin(egui::Margin::symmetric(12.0, 4.0)).show(ui, |ui| {
            if self.muse.key.is_empty() {
                self.muse_connect_card(ui);
            } else {
                self.muse_body(ui);
            }
        });
        self.muse_update_pieces();
    }

    /// Not connected yet: one button to get a key, one to paste it.
    fn muse_connect_card(&mut self, ui: &mut Ui) {
        let checking = self.muse.connecting.is_some();
        ui.add_space(24.0);
        ui.vertical_centered(|ui| {
            ui.label(RichText::new("Connect Muse").size(18.0).strong());
            ui.add_space(4.0);
            ui.label(RichText::new("Muse textures your model from a description or a photo. It runs on your Meta Model API account.").weak());
            ui.add_space(14.0);
            if !self.muse.connect_opened {
                let b = egui::Button::new(RichText::new("Connect Muse").color(Color32::WHITE).strong()).fill(crate::style::ACCENT).min_size(egui::vec2(200.0, 34.0)).rounding(8.0);
                if ui.add(b).on_hover_text("Opens Meta's API key page in your browser").clicked() {
                    let _ = std::process::Command::new("explorer").arg(KEYS_URL).spawn();
                    self.muse.connect_opened = true;
                }
                ui.add_space(8.0);
                if ui.small_button("I already have a key").clicked() {
                    self.muse.connect_opened = true;
                }
            } else {
                ui.label("In the page that opened: sign in, open API keys,\ncreate a key and copy it. Then:");
                ui.add_space(10.0);
                let b = egui::Button::new(RichText::new("Paste key and connect").color(Color32::WHITE).strong()).fill(crate::style::ACCENT).min_size(egui::vec2(220.0, 34.0)).rounding(8.0);
                if ui.add_enabled(!checking, b).clicked() {
                    match arboard::Clipboard::new().and_then(|mut c| c.get_text()) {
                        Ok(t) if !t.trim().is_empty() && !t.trim().contains(char::is_whitespace) => self.muse_connect(t.trim().to_string()),
                        _ => self.muse.connect_msg = "Nothing that looks like a key is copied. Copy the key, then click again.".into(),
                    }
                }
                ui.add_space(10.0);
                ui.label(RichText::new("or type it").small().weak());
                ui.horizontal(|ui| {
                    let r = ui.add_enabled(!checking, egui::TextEdit::singleline(&mut self.muse.key_draft).password(true).hint_text("Key").desired_width(ui.available_width() - 80.0));
                    let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    let draft = self.muse.key_draft.trim().to_string();
                    if (ui.add_enabled(!checking && !draft.is_empty(), egui::Button::new("Connect")).clicked() || enter) && !draft.is_empty() {
                        self.muse_connect(draft);
                    }
                });
                ui.add_space(6.0);
                if ui.small_button("Open the key page again").clicked() {
                    let _ = std::process::Command::new("explorer").arg(KEYS_URL).spawn();
                }
            }
            ui.add_space(10.0);
            if checking {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Checking the key...");
                });
            } else if !self.muse.connect_msg.is_empty() {
                ui.label(RichText::new(&self.muse.connect_msg).color(Color32::from_rgb(230, 140, 120)));
            }
            ui.add_space(10.0);
            ui.label(RichText::new("The key stays on this computer.").small().weak());
        });
    }

    /// Checks a key with Meta (a free call) before keeping it.
    fn muse_connect(&mut self, key: String) {
        let (tx, rx) = mpsc::channel();
        self.muse.connect_msg.clear();
        let k = key.clone();
        std::thread::spawn(move || {
            let r = match ureq::get(&format!("{API}/models")).set("Authorization", &format!("Bearer {k}")).timeout(Duration::from_secs(20)).call() {
                Ok(_) => Ok(()),
                Err(ureq::Error::Status(401 | 403, _)) => Err("Meta refused this key. Copy it again from the API keys page.".to_string()),
                Err(ureq::Error::Status(code, _)) => Err(format!("Meta answered with an error ({code}). Try again in a moment.")),
                Err(e) => Err(format!("Meta could not be reached ({e}).")),
            };
            let _ = tx.send(r);
        });
        self.muse.connecting = Some((key, rx));
    }

    fn muse_connect_poll(&mut self, ctx: &egui::Context) {
        let Some((key, rx)) = &self.muse.connecting else { return };
        ctx.request_repaint_after(Duration::from_millis(100));
        let Ok(r) = rx.try_recv() else { return };
        let key = key.clone();
        self.muse.connecting = None;
        match r {
            Ok(()) => {
                let path = key_path();
                if let Err(e) = std::fs::create_dir_all(path.parent().unwrap_or(&path)).and_then(|_| std::fs::write(&path, &key)) {
                    self.toast(format!("Muse is connected, but the key could not be saved ({e}): it is kept until you close the app."));
                }
                self.muse.key = key;
                self.muse.key_draft.clear();
                self.muse.connect_opened = false;
            }
            Err(m) => self.muse.connect_msg = m,
        }
    }

    fn muse_body(&mut self, ui: &mut Ui) {
        let Some(doc) = self.doc.as_ref() else {
            ui.label(RichText::new("Open a project to texture it with Muse.").weak());
            return;
        };
        // Another document: a new conversation.
        let key = doc_key(doc);
        if self.muse.convo.as_ref().is_some_and(|c| c.doc != key) && self.muse.job.is_none() {
            self.muse.convo = None;
            self.muse.chat.clear();
        }
        let set_names: Vec<String> = doc.sets.iter().map(|s| s.name.clone()).collect();
        if self.muse.sets_on.len() != set_names.len() {
            self.muse.sets_on = vec![true; set_names.len()];
        }
        let all = parts(doc);
        let busy = self.muse.job.is_some() || self.muse.check.is_some() || self.muse.light_check.is_some() || self.muse.light_verify.is_some();
        let started = self.muse.convo.is_some();

        // The bar: pieces on the model, a new conversation, the connection.
        ui.horizontal(|ui| {
            if chip(ui, "Pieces", self.muse.pieces_on).on_hover_text("Show the numbered pieces on the model (name them in your messages: \"make piece 3 dark wood\")").clicked() {
                self.muse.pieces_on = !self.muse.pieces_on;
            }
            let chosen = self.muse.convo.as_ref().and_then(|c| c.style).filter(|_| self.muse.style == Style::Auto);
            let shown = match chosen {
                Some(s) => format!("Style: Auto ({})", s.name()),
                None => format!("Style: {}", self.muse.style.name()),
            };
            egui::ComboBox::from_id_salt("muse style").selected_text(RichText::new(shown).small()).width(110.0).show_ui(ui, |ui| {
                for s in Style::ALL {
                    ui.selectable_value(&mut self.muse.style, s, s.name()).on_hover_text(s.hint());
                }
            })
            .response
            .on_hover_text(self.muse.style.hint());
            if ui.add_enabled(!busy && !self.muse.chat.is_empty(), egui::Button::new(RichText::new("New conversation").small()).small()).on_hover_text("Start over (the layers stay)").clicked() {
                self.muse.convo = None;
                self.muse.chat.clear();
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let chip = ui.add(egui::Button::new(RichText::new("\u{25CF} Connected").small().color(Color32::from_rgb(120, 200, 140))).frame(false)).on_hover_text("Muse account");
                let pid = ui.id().with("muse account");
                if chip.clicked() {
                    ui.memory_mut(|m| m.toggle_popup(pid));
                }
                super::widgets::popup_below(ui, pid, &chip, egui::PopupCloseBehavior::CloseOnClick, |ui| {
                    ui.set_min_width(150.0);
                    if ui.button("Disconnect Muse").clicked() {
                        let _ = std::fs::remove_file(key_path());
                        self.muse.key.clear();
                        self.muse.pieces_on = false;
                    }
                });
            });
        });
        if self.muse.key.is_empty() {
            return;
        }
        // The texture sets to texture, chosen before the conversation starts.
        if set_names.len() > 1 {
            ui.add_space(2.0);
            if started {
                let on: Vec<&str> = set_names.iter().zip(&self.muse.sets_on).filter(|(_, b)| **b).map(|(n, _)| n.as_str()).collect();
                ui.label(RichText::new(format!("Texturing {}", on.join(", "))).small().weak());
            } else {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new("Texture").small().weak());
                    for (i, n) in set_names.iter().enumerate() {
                        let on = self.muse.sets_on[i];
                        if ui.add_enabled_ui(!busy, |ui| chip(ui, n, on)).inner.on_hover_text("Click to include or leave out this texture set").clicked() {
                            self.muse.sets_on[i] = !on;
                        }
                    }
                });
            }
        }
        let on = self.muse.sets_on.clone();
        let scope: Vec<usize> = (0..all.len()).filter(|&i| on.get(all[i].set).copied().unwrap_or(true)).collect();
        ui.add_space(6.0);
        // The message box at the bottom (as tall as its content), the
        // conversation scrolling in the room above it.
        let mut send = false;
        let can_send = !busy && !self.muse.prompt.trim().is_empty() && (started || !scope.is_empty());
        egui::TopBottomPanel::bottom(ui.id().with("muse composer")).frame(egui::Frame::none().inner_margin(egui::Margin { top: 6.0, bottom: 8.0, ..Default::default() })).show_separator_line(false).resizable(false).show_inside(ui, |ui| {
            egui::Frame::none().fill(crate::style::FIELD).rounding(10.0).inner_margin(egui::Margin::symmetric(8.0, 6.0)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                // The images going with the message (a click removes one).
                if !self.muse.attach.is_empty() {
                    let thumbs: Vec<egui::TextureHandle> = self.muse.attach.iter().map(|r| r.thumb.clone()).collect();
                    if let Some(k) = ref_row(ui, &thumbs, 48.0, "Sent with your message (click to remove)") {
                        self.muse.attach.remove(k);
                    }
                    ui.add_space(4.0);
                }
                let hint = if started { "Ask for a change: make piece 3 dark wood, frosted glass..." } else { "Describe the look, or drop a reference photo" };
                let r = ui.add(
                    egui::TextEdit::multiline(&mut self.muse.prompt)
                        .hint_text(hint)
                        .frame(false)
                        .desired_rows(2)
                        .desired_width(f32::INFINITY)
                        .return_key(Some(egui::KeyboardShortcut::new(egui::Modifiers::SHIFT, egui::Key::Enter))),
                );
                send |= r.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter) && !i.modifiers.shift) && can_send;
                ui.horizontal(|ui| {
                    if ui.add_enabled(!busy, egui::Button::new(RichText::new("+ Image").small()).small()).on_hover_text("Attach reference photos (or drop them anywhere on the window)").clicked() {
                        let files = file_dialog().and_then(|d| d.add_filter("Images", &["png", "jpg", "jpeg", "webp", "bmp", "tga", "tif", "tiff"]).pick_files());
                        for f in files.unwrap_or_default() {
                            self.muse_attach(ui.ctx(), &f);
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let b = egui::Button::new(RichText::new("Send").color(Color32::WHITE).strong()).fill(if can_send { crate::style::ACCENT } else { Color32::from_gray(70) }).rounding(6.0);
                        send |= ui.add_enabled(can_send, b).on_hover_text("Enter (Shift+Enter: new line)").clicked();
                    });
                });
            });
        });
        egui::CentralPanel::default().frame(egui::Frame::none()).show_inside(ui, |ui| {
            egui::ScrollArea::vertical().id_salt("muse chat").auto_shrink([false, false]).stick_to_bottom(!self.muse.chat.is_empty()).show(ui, |ui| {
                if self.muse.chat.is_empty() {
                    ui.add_space(4.0);
                    ui.label(RichText::new("What should it look like?").strong());
                    ui.label(RichText::new("Describe it, or attach a photo of the real thing. Then keep talking to change any piece; only what you ask for changes.").small().weak());
                    ui.add_space(8.0);
                    for ex in EXAMPLES {
                        if chip(ui, ex, false).on_hover_text("Use this").clicked() {
                            self.muse.prompt = ex.to_string();
                        }
                        ui.add_space(2.0);
                    }
                }
                for line in &self.muse.chat {
                    bubble(ui, line);
                }
                if busy {
                    egui::Frame::none().fill(Color32::from_gray(36)).rounding(8.0).inner_margin(egui::Margin::symmetric(10.0, 8.0)).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(RichText::new(&self.muse.status).small());
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.small_button("Cancel").clicked() {
                                    self.muse_cancel();
                                }
                            });
                        });
                    });
                }
            });
        });
        if send {
            let ctx = ui.ctx().clone();
            self.muse_project_files(&ctx);
            self.muse_send(all, scope);
        }
    }

    /// Keeps the pieces shown on the model up to date (the conversation's
    /// pieces, or before it starts the ones the texture sets in scope give).
    fn muse_update_pieces(&mut self) {
        if !self.muse.pieces_on {
            return;
        }
        let Some(doc) = self.doc.as_ref() else { return };
        let key = match self.muse.convo.as_ref().filter(|c| !c.pieces.is_empty()) {
            Some(c) => format!("{}|convo|{}", doc_key(doc), c.pieces.len()),
            None => format!("{}|{:?}", doc_key(doc), self.muse.sets_on),
        };
        if self.muse.piece_view.as_ref().is_some_and(|v| v.key == key) {
            return;
        }
        let pieces = match self.muse.convo.as_ref().filter(|c| !c.pieces.is_empty()) {
            Some(c) => c.pieces.clone(),
            None => {
                let all = parts(doc);
                let on = &self.muse.sets_on;
                let scope: Vec<usize> = (0..all.len()).filter(|&i| on.get(all[i].set).copied().unwrap_or(true)).collect();
                pieces::find(&doc.mesh, &part_list(doc, &all, &scope))
            }
        };
        // Each set's texels in their piece's color (gray: no piece).
        const RES: u32 = 1024;
        let tex = (0..doc.sets.len())
            .map(|s| {
                let own = pieces::owners(&doc.mesh, &pieces, s, RES);
                let data: Vec<f32> = own
                    .iter()
                    .flat_map(|&o| {
                        let c = if o == 0 { [0.42; 3] } else { part_color(o as usize - 1).0.map(|v| v as f32 / 255.0) };
                        [c[0], c[1], c[2], 1.0]
                    })
                    .collect();
                let t = crate::engine::Tex::new(&self.engine.device, "muse pieces", RES, RES, wgpu::TextureFormat::Rgba16Float, wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST);
                t.write_bytes(&self.engine.queue, &crate::engine::texture::f32_to_f16_bytes(&data));
                t
            })
            .collect();
        // Every triangle with its piece, for the labels' depth test.
        let mesh = &doc.mesh;
        let mut piece_of: HashMap<(usize, u32), u16> = HashMap::new();
        for (i, p) in pieces.iter().enumerate() {
            for &t in &p.tris {
                piece_of.insert((p.sub, t), i as u16);
            }
        }
        let tris = mesh
            .submeshes
            .iter()
            .enumerate()
            .flat_map(|(si, sm)| {
                let piece_of = &piece_of;
                (0..sm.indices.len() / 3).map(move |t| {
                    let v = |k: usize| glam::Vec3::from(mesh.vertices[sm.indices[t * 3 + k] as usize].pos);
                    ([v(0), v(1), v(2)], piece_of.get(&(si, t as u32)).copied().unwrap_or(u16::MAX))
                })
            })
            .collect();
        self.muse.piece_view = Some(PieceView { key, tex, tris });
    }

    /// The pieces' numbers over the 3D view: a small depth render of the
    /// model (redone when the camera moves) finds what each piece shows of
    /// itself, and its number goes in the middle of that.
    pub(super) fn muse_piece_labels(&self, painter: &egui::Painter, rect: egui::Rect) {
        let Some(view) = self.muse.piece_view.as_ref().filter(|_| self.muse.pieces_on) else { return };
        if self.view.display != Display::Material || rect.width() < 8.0 || rect.height() < 8.0 {
            return;
        }
        let aspect = rect.width() / rect.height();
        let m = self.camera.view_proj(aspect);
        let key = {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            m.to_cols_array().map(f32::to_bits).hash(&mut h);
            view.key.hash(&mut h);
            h.finish()
        };
        let mut cache = self.muse.label_cache.borrow_mut();
        if cache.as_ref().map(|c| c.0) != Some(key) {
            *cache = Some((key, piece_label_spots(&view.tris, m, aspect)));
        }
        let font = egui::FontId::proportional(13.0);
        let mut placed: Vec<egui::Rect> = Vec::new();
        for &(i, (u, v)) in &cache.as_ref().unwrap().1 {
            let at = egui::pos2(rect.min.x + u * rect.width(), rect.min.y + v * rect.height());
            let text = i.to_string();
            let r = egui::Rect::from_center_size(at, egui::vec2(10.0 + 7.5 * text.len() as f32, 18.0));
            if placed.iter().any(|q| q.intersects(r)) {
                continue;
            }
            placed.push(r);
            let col = part_color(i).0;
            painter.rect(r, 9.0, Color32::from_black_alpha(200), egui::Stroke::new(1.5f32, Color32::from_rgb(col[0], col[1], col[2])));
            painter.text(r.center(), egui::Align2::CENTER_CENTER, text, font.clone(), Color32::WHITE);
        }
    }

    /// Sends the message being typed (the first one starts the conversation
    /// on the parts in scope).
    fn muse_send(&mut self, all: Vec<Part>, scope: Vec<usize>) {
        let text = self.muse.prompt.trim().to_string();
        let Some(doc) = self.doc.as_ref() else { return };
        if text.is_empty() {
            return;
        }
        let first = match &self.muse.convo {
            Some(_) => None,
            None => {
                self.muse.placed.clear();
                self.muse.convo = Some(Convo { style: None, doc: doc_key(doc), pieces: Vec::new(), system: String::new(), view: String::new(), sheet: String::new(), surf: String::new(), surfaces: Vec::new(), turns: Vec::new(), looks: Vec::new(), refs: Vec::new() });
                let parts = part_list(doc, &all, &scope);
                let mesh = doc.mesh.clone();
                let (tex, presets, smart) = self.shelf.material_names();
                Some(FirstTurn { mesh, parts, tex, presets, smart })
            }
        };
        // The texture sets in the conversation.
        let mut sets: Vec<usize> = match &first {
            Some(f) => f.parts.iter().map(|p| p.0).collect(),
            None => self.muse.convo.as_ref().map(|c| c.pieces.iter().map(|p| p.set).collect()).unwrap_or_default(),
        };
        sets.sort_unstable();
        sets.dedup();
        if first.is_some() {
            self.muse.prepare = Some(sets.clone());
        }
        let Some(doc) = self.doc.as_ref() else { return };
        let sets = sets_text(doc, &sets);
        let studio = format!("\n[PACKSHOT STUDIO now: {} Its lighting: {}]", self.studio_facts(), super::muse_light::to_json(&doc.studio));
        // Attached images become R1, R2... of the conversation.
        let attach = std::mem::take(&mut self.muse.attach);
        let c = self.muse.convo.as_mut().unwrap();
        let mut user = text.clone();
        if !attach.is_empty() {
            let names: Vec<String> = attach.iter().enumerate().map(|(k, r)| format!("R{} ({}{})", c.refs.len() + k + 1, r.name, image_facts(&r.path, r.project))).collect();
            user = format!("{text}\n[Reference images attached: {}]", names.join(", "));
            if attach.iter().any(|r| r.project) {
                user += "\n[The project files were imported into this project by the user: made for this model (artwork masks, UV sheets, logos, textures, photos). Look at each one and use every one that fits, for what it is.]";
            }
            // Images laid out on a texture set's UV map.
            let rasters: Vec<(String, &crate::bake::UvRaster)> = doc.sets.iter().map(|s| (s.name.clone(), &s.raster)).collect();
            for (k, r) in attach.iter().enumerate() {
                let n = c.refs.len() + k + 1;
                match uv_sheet_of(&r.path, &rasters) {
                    Some((set, true)) => user += &format!("\n[R{n} is the UV layout of texture set {set} (its UV islands drawn flat): a guide to where things are on that UV map, never a texture or a mask itself.]"),
                    Some((set, false)) => user += &format!("\n[R{n} fits inside the UV islands of texture set {set}. If it is artwork made for that UV map (not a photo of the object), it goes on as it is with uv_sheet (a print {{\"reference\": {n}, \"uv_sheet\": true}}, or a mask step when it is black and white). A photo or a logo is never a uv_sheet.]"),
                    None => {}
                }
            }
        }
        // MF_MUSE_STYLE: the style for headless runs.
        if let Some(s) = std::env::var("MF_MUSE_STYLE").ok().and_then(|s| Style::from_word(&s)) {
            self.muse.style = s;
        }
        user += &match self.muse.style {
            Style::Auto => match c.style {
                Some(s) => format!("\n[Texturing style: {} (your choice; keep it unless the user asks otherwise).]", s.name().to_lowercase()),
                None => "\n[Texturing style: not set by the user: choose it from the object and the request, write it in \"style\", and follow it.]".to_string(),
            },
            s => format!("\n[Texturing style chosen by the user: {}.]", s.name().to_lowercase()),
        };
        user += &studio;
        // A request for the lighting alone: the materials stay.
        self.muse.light_only = super::muse_light::lighting_only(&text);
        if self.muse.light_only {
            user += "\n[This message asks only for the packshot's lighting: return \"lighting\" and no looks; every material stays as it is.]";
        }
        // A lighting request: its photos measured.
        if super::muse_light::about_lighting(&text) {
            self.muse.light_photo = attach.iter().filter(|r| !r.project).find_map(|r| crate::studio::photo_profile(&image::open(&r.path).ok()?));
            let n0 = c.refs.len();
            let facts: Vec<String> = attach.iter().enumerate().filter(|(_, r)| !r.project).filter_map(|(k, r)| Some(format!("R{} {}", n0 + k + 1, crate::studio::photo_light(&image::open(&r.path).ok()?)?))).collect();
            if !facts.is_empty() {
                user += &format!("\n[Light measures by the app: {}]", facts.join(". "));
                self.muse.light_facts = facts.join(". ");
            }
        }
        let _ = std::fs::write(muse_dir().join("last_message.txt"), &user);
        let urls: Vec<String> = attach.iter().map(|r| r.url.clone()).collect();
        let thumbs: Vec<egui::TextureHandle> = attach.iter().map(|r| r.thumb.clone()).collect();
        c.refs.extend(attach);
        let req = Request { key: self.muse.key.clone(), first, system: c.system.clone(), view: c.view.clone(), sheet: c.sheet.clone(), surf: c.surf.clone(), turns: c.turns.clone(), stacks: stacks_text(c), sets, user, refs: urls, check: false, scans: scans_of(c) };
        self.muse.chat.push(ChatLine { user: true, text, notes: Vec::new(), refs: thumbs });
        self.muse.prompt.clear();
        self.muse.status = "Muse Spark is looking at the model...".into();
        self.muse_spawn(req);
    }

    /// The images the user imported into the project go to Muse with the
    /// next message, once each, as references it can use like any other
    /// (made for this model: masks, UV sheets, logos, photos).
    pub(super) fn muse_project_files(&mut self, ctx: &egui::Context) {
        if self.muse.convo.is_none() {
            self.muse.files_sent.clear();
        }
        let Some(doc) = self.doc.as_ref() else { return };
        let had: Vec<String> = self.muse.convo.iter().flat_map(|c| c.refs.iter().map(|r| r.name.to_lowercase())).chain(self.muse.attach.iter().map(|r| r.name.to_lowercase())).collect();
        let mut res: Vec<&Resource> = doc.resources.values().filter(|r| r.imported && !self.muse.files_sent.contains(&r.id)).collect();
        res.sort_by_key(|r| r.id);
        let dir = muse_dir().join("files");
        let _ = std::fs::create_dir_all(&dir);
        let mut new = Vec::new();
        for r in res.into_iter().take(16) {
            let ext = image::guess_format(&r.encoded).ok().and_then(|f| f.extensions_str().first().copied()).unwrap_or("png");
            let stem: String = std::path::Path::new(&r.name).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| r.name.clone());
            let name = format!("{stem}.{ext}");
            if had.contains(&name.to_lowercase()) || had.contains(&r.name.to_lowercase()) {
                new.push((r.id, None));
                continue;
            }
            let path = dir.join(format!("{}_{name}", r.id));
            if std::fs::write(&path, r.encoded.as_slice()).is_err() {
                continue;
            }
            if let Ok(mut x) = load_ref(ctx, &path) {
                x.name = name;
                x.project = true;
                new.push((r.id, Some(x)));
            }
        }
        for (id, x) in new {
            self.muse.files_sent.push(id);
            self.muse.attach.extend(x);
        }
    }

    /// Before Muse's first look at the model: its texture sets get at least
    /// 2048 texels (small prints and details stay sharp) and their mesh
    /// maps baked (the finishes' grime, dust and edges read them).
    fn muse_prepare(&mut self, sets: &[usize]) {
        let Some(doc) = self.doc.as_mut() else { return };
        let mut raised = Vec::new();
        // Only as far as the GPU memory allows (many sets stay as they are).
        let mut res: Vec<u32> = doc.sets.iter().map(|s| s.resolution()).collect();
        for &s in sets {
            res[s] = res[s].max(2048);
        }
        let fits = Document::sets_bytes(res) <= crate::document::SET_BUDGET;
        for &s in sets.iter().filter(|_| fits) {
            if doc.sets[s].resolution() < 2048 {
                doc.change_resolution(&self.engine, s, 2048);
                raised.push(doc.sets[s].name.clone());
            }
        }
        if !raised.is_empty() {
            self.history.clear();
            self.engine.compositor.trim();
            self.engine.procgen.trim();
            self.message(format!("Muse: {} raised to 2048 px for sharper details.", raised.join(", ")));
        }
        let Some(doc) = self.doc.as_ref() else { return };
        let which: Vec<bool> = (0..doc.sets.len()).map(|i| sets.contains(&i) && !doc.sets[i].baked).collect();
        if which.iter().any(|&b| b) && self.bake_job.is_none() {
            let st = doc.bake_settings.clone();
            self.start_bake(st, which);
            self.message("Muse: baking the mesh maps (grime, dust and worn edges read them).".to_string());
        }
    }

    /// `--musetest`: starts Muse on the texture sets named (all when none),
    /// or (`next`) continues the conversation.
    pub(super) fn muse_test_send(&mut self, ctx: &egui::Context, prompt: &str, sets: &[String]) -> anyhow::Result<()> {
        let doc = self.doc.as_ref().ok_or_else(|| anyhow::anyhow!("no project"))?;
        let all = parts(doc);
        let on: Vec<bool> = doc.sets.iter().map(|s| sets.is_empty() || sets.iter().any(|n| n.eq_ignore_ascii_case(&s.name))).collect();
        let scope: Vec<usize> = (0..all.len()).filter(|&i| on[all[i].set]).collect();
        anyhow::ensure!(!scope.is_empty(), "no texture set named {sets:?}");
        anyhow::ensure!(!self.muse.key.is_empty(), "no Meta Model API key (MODEL_API_KEY)");
        self.muse.prompt = prompt.to_string();
        self.muse_project_files(ctx);
        self.muse_send(all, scope);
        Ok(())
    }

    /// The conversation's turns so far (UI checks).
    pub(super) fn muse_test_turns(&self) -> usize {
        self.muse.convo.as_ref().map_or(0, |c| c.turns.len())
    }

    /// A key for checks that replay Muse's answers (no API call).
    pub(super) fn muse_test_key(&mut self) {
        if self.muse.key.is_empty() {
            self.muse.key = "test".into();
        }
    }

    /// `--musetest`: (still working, the last message's text and notes).
    pub(super) fn muse_test_state(&self) -> (bool, Vec<String>) {
        // Muse's lines since the last message (the answer and its check).
        let from = self.muse.chat.iter().rposition(|l| l.user).map_or(0, |i| i + 1);
        let last = self.muse.chat[from..].iter().flat_map(|l| std::iter::once(l.text.clone()).chain(l.notes.iter().cloned())).collect();
        (self.muse.job.is_some() || self.muse.check.is_some() || self.muse.light_check.is_some() || self.muse.light_verify.is_some(), last)
    }

    /// Stops the current answer: nothing is applied (a request already sent
    /// finishes in the background and is dropped).
    fn muse_cancel(&mut self) {
        self.muse.check = None;
        if let Some(c) = self.muse.cancel.take() {
            c.store(true, Ordering::Relaxed);
        }
        self.muse.job = None;
        self.muse_turn_failed("Cancelled.".into());
    }

    /// A turn that gave nothing: said in the chat; a conversation that never
    /// got its first answer starts over.
    /// The view as it is now, shown instead of the live one while Muse
    /// works on its result (`muse_release` lets the live view back).
    fn muse_hold(&mut self) {
        let out = self.engine.viewport.output();
        let (dev, q) = (&self.engine.device, &self.engine.queue);
        let t = crate::engine::Tex::new(dev, "muse hold", out.width, out.height, out.format, wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST);
        let mut enc = dev.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("muse hold") });
        enc.copy_texture_to_texture(out.texture.as_image_copy(), t.texture.as_image_copy(), wgpu::Extent3d { width: out.width, height: out.height, depth_or_array_layers: 1 });
        q.submit([enc.finish()]);
        let id = self.renderer.write().register_native_texture(dev, &t.view, wgpu::FilterMode::Linear);
        self.muse.hold = Some((t, id));
    }

    /// The final result shows: the live view back, Muse's answer in the chat.
    fn muse_release(&mut self) {
        if let Some(line) = self.muse.pending.take() {
            self.muse.chat.push(line);
        }
        if let Some((_, id)) = self.muse.hold.take() {
            self.renderer.write().free_texture(&id);
        }
    }

    /// The held view, while Muse works on its result.
    pub(super) fn muse_hold_tex(&self) -> Option<egui::TextureId> {
        self.muse.hold.as_ref().map(|h| h.1)
    }

    fn muse_turn_failed(&mut self, why: String) {
        self.muse_release();
        if self.muse.convo.as_ref().is_some_and(|c| c.system.is_empty()) {
            // Its images go back with the message box.
            if let Some(c) = self.muse.convo.take() {
                self.muse.attach = c.refs;
            }
        }
        self.muse.chat.push(ChatLine { user: false, text: why, notes: Vec::new(), refs: Vec::new() });
    }

    pub(super) fn muse_poll(&mut self, ctx: &egui::Context) {
        if self.muse.check.is_some() && self.muse.job.is_none() {
            self.muse_check_tick(ctx);
        }
        let Some(rx) = &self.muse.job else { return };
        ctx.request_repaint_after(Duration::from_millis(100));
        let msgs: Vec<Msg> = rx.try_iter().collect();
        for m in msgs {
            match m {
                Msg::Status(s) => self.muse.status = s,
                Msg::Failed(e) => {
                    self.muse.job = None;
                    self.muse.cancel = None;
                    self.muse_turn_failed(e);
                }
                Msg::Done(t) => {
                    self.muse.job = None;
                    self.muse.cancel = None;
                    self.muse_answer(*t);
                }
            }
        }
    }

    /// Fixes what is known to look wrong in a plan before it is applied
    /// (the plan is changed, so Muse remembers what is really there): a
    /// scan tinted far from its own color gives way to the closest scan of
    /// its category; cell patterns carved as height on a scanned look (they
    /// read as a network of cracks) are dropped; a look keeps at most
    /// `MAX_LAYERS` layers.
    fn guard_plan(&mut self, plan: &mut Value, notes: &mut Vec<String>) {
        const MAX_LAYERS: usize = 4;
        let Some(looks) = plan["looks"].as_array_mut() else { return };
        let (tex, _, _) = self.shelf.material_names();
        let stats = crate::library::material_stats(&tex);
        let lab_of = |i: usize| stats[i].0.as_ref().and_then(|h| hex(&Value::String(h.clone()))).map(lab);
        for look in looks.iter_mut() {
            let Some(layers) = look["layers"].as_array_mut() else { continue };
            let scanned = layers.first().is_some_and(|l| source(l)["material"].is_string() || source(l)["ambientcg"].is_string());
            // Cell patterns as height on a scan.
            let before = layers.len();
            layers.retain(|l| {
                let p = l["height_pattern"]["pattern"].as_str().unwrap_or("").to_lowercase();
                !(scanned && (p.starts_with("cells") || p.starts_with("voronoi")))
            });
            if layers.len() < before {
                notes.push("A cell pattern used as height was left out (it draws cracks on a scan).".into());
            }
            // A glossy surface mirrors its surroundings and the reflection
            // shows every bump: its height patterns stay within a range of
            // 0.03 (more reads as a lumpy, crumpled surface).
            let base_rough = layers.iter().find(|l| print_key(l).is_none()).and_then(|l| l["roughness"].as_f64().or_else(|| source(l)["roughness_max"].as_f64()));
            if base_rough.is_some_and(|r| r <= 0.3) {
                let mut tamed = Vec::new();
                for l in layers.iter_mut().filter(|l| l["height_pattern"].is_object()) {
                    let hp = &l["height_pattern"];
                    let range = (hp["max"].as_f64().unwrap_or(1.0) - hp["min"].as_f64().unwrap_or(0.0)).abs();
                    let op = l["layer_opacity"].as_f64().unwrap_or(1.0);
                    if range * op > 0.03 {
                        l["layer_opacity"] = serde_json::json!(0.03 / range.max(1e-6));
                        tamed.push(l["name"].as_str().unwrap_or("height").to_string());
                    }
                }
                if !tamed.is_empty() {
                    notes.push(format!("{}: made very gentle (a glossy surface shows every bump).", tamed.join(", ")));
                }
            }
            // Prints (reference images) and the finish do not count.
            let mut count = 0;
            let mut names = Vec::new();
            layers.retain(|l| {
                if print_key(l).is_some() || l["name"].as_str().is_some_and(|n| n.starts_with("Finish:")) {
                    return true;
                }
                count += 1;
                if count > MAX_LAYERS {
                    names.push(l["name"].as_str().unwrap_or("layer").to_string());
                }
                count <= MAX_LAYERS
            });
            if !names.is_empty() {
                notes.push(format!("Kept {MAX_LAYERS} layers, left out: {}.", names.join(", ")));
            }
            // The scans' tints.
            for l in layers.iter_mut() {
                let layer = l["name"].as_str().unwrap_or("Layer").to_string();
                let src = if l["source"].is_object() { &mut l["source"] } else { &mut *l };
                let (Some(name), Some(tint)) = (src["material"].as_str().map(str::to_string), hex(&src["tint"])) else { continue };
                let Some(i) = tex.iter().position(|m| m.name.eq_ignore_ascii_case(name.trim())) else { continue };
                let want = lab(tint);
                let Some(own) = lab_of(i) else { continue };
                let d0 = lab_dist(want, own);
                if d0 < 20.0 {
                    continue;
                }
                // Same kind of surface: the names share a material word
                // (wood, oak, iron...), not only a category (wicker is wood).
                let kind = kind_words(&tex[i].name);
                let best = (0..tex.len())
                    .filter(|&k| k != i && tex[k].category == tex[i].category)
                    .filter(|&k| kind_words(&tex[k].name).iter().any(|w| kind.contains(w)))
                    .filter_map(|k| lab_of(k).map(|c| (k, lab_dist(want, c))))
                    .min_by(|a, b| a.1.total_cmp(&b.1));
                if let Some((k, _)) = best.filter(|b| b.1 < d0 * 0.6) {
                    notes.push(format!("{layer}: {} instead of {name} (its own color is closer to {}).", tex[k].name, src["tint"].as_str().unwrap_or("")));
                    src["material"] = Value::String(tex[k].name.clone());
                }
            }
        }
    }

    /// The self check's render of the model now (musetest).
    pub(super) fn muse_test_shot(&mut self) -> Option<image::RgbaImage> {
        self.muse_shot()
    }

    /// The model as it looks now (the viewport's look and lighting, the
    /// material display) from two sides, rendered off screen: what Muse's
    /// self check sees.

    fn muse_shot(&mut self) -> Option<image::RgbaImage> {
        use crate::engine::viewport::{CameraUniform, DrawSet, Frame, PostUniform, SetUniform, Viewport};
        let doc = self.doc.as_ref()?;
        let (min, max) = (doc.mesh.bbox_min, doc.mesh.bbox_max);
        let diag = doc.mesh.diagonal();
        // Tall models get tall views.
        let size = max - min;
        let (w, h) = if size.y > 1.4 * size.x.max(size.z) { (560u32, 840u32) } else { (720, 720) };
        let engine = &self.engine;
        let env = &engine.env;
        let v = &self.view;
        let mut vp = Viewport::new(&engine.device, &engine.queue, env);
        vp.resize(&engine.device, w, h);
        let (light_dir, light_vp) = self.key_light();
        // The whole model from the front and the back, then close-ups of
        // its most detailed areas.
        let mut cams = Vec::new();
        for yaw in [0.6f32, 0.6 + std::f32::consts::PI] {
            let mut cam = crate::camera::OrbitCamera { yaw, pitch: 0.3, ..Default::default() };
            cam.frame(min, max);
            cams.push(cam);
        }
        cams.extend(self.muse_closeups());
        let mut out = image::RgbaImage::new(w * 2, h * cams.len().div_ceil(2) as u32);
        for (k, cam) in cams.into_iter().enumerate() {
            let view_proj = cam.view_proj(w as f32 / h as f32);
            let frame = Frame {
                camera: CameraUniform {
                    view_proj: view_proj.to_cols_array_2d(),
                    inv_view_proj: view_proj.inverse().to_cols_array_2d(),
                    eye: cam.eye().extend(diag).into(),
                    env: [v.env_rotation.to_radians(), 2f32.powf(v.exposure), v.bg_blur, (env.mip_count as f32 - 3.0).max(1.0)],
                    display: [0.0, 1.0, if v.shadows { v.shadow_strength } else { 0.0 }, v.bg_intensity],
                    sh: env.sh,
                    light_view_proj: light_vp.to_cols_array_2d(),
                    light: [light_dir.x, light_dir.y, light_dir.z, v.light_intensity],
                    wire: [0.0; 4],
                },
                post: PostUniform { a: [2f32.powf(v.exposure), v.tonemap as f32, 0.0, 1.5], b: [0.0, v.saturation, v.contrast, 0.0], texel: [0.0; 4] },
                mesh: Some(&doc.gpu_mesh),
                sets: doc
                    .sets
                    .iter()
                    .map(|s| DrawSet {
                        submesh: s.submesh,
                        base: &s.gpu.final_base,
                        orm: &s.gpu.final_orm,
                        normal: &s.gpu.final_normal,
                        emissive: &s.gpu.final_emissive,
                        display: &s.gpu.final_base,
                        params: SetUniform { k: [s.shader_alpha(), s.settings.alpha_cutoff, s.settings.emissive_intensity, 0.0] },
                        visible: s.settings.visible,
                        transmissive: s.uses_ext(1),
                    })
                    .collect(),
                shadows: v.shadows,
                wireframe: false,
                wire_only: None,
                selection: None,
                path_traced: false,
            };
            let mut enc = engine.device.create_command_encoder(&Default::default());
            vp.render(&engine.device, &engine.queue, &mut enc, &frame);
            engine.queue.submit(Some(enc.finish()));
            let bytes = vp.output().read_bytes(&engine.device, &engine.queue);
            let img = image::RgbaImage::from_raw(w, h, bytes)?;
            image::imageops::overlay(&mut out, &img, ((k as u32 % 2) * w) as i64, ((k as u32 / 2) * h) as i64);
        }
        Some(out)
    }

    /// Cameras for close-ups of the two most detailed areas: where the
    /// prints are (their boxes on their surface), else the largest surfaces
    /// not facing down; seen a little from the side, where shine and
    /// relief show.
    fn muse_closeups(&self) -> Vec<crate::camera::OrbitCamera> {
        use glam::Vec2;
        let (Some(c), Some(doc)) = (self.muse.convo.as_ref(), self.doc.as_ref()) else { return Vec::new() };
        let long = (doc.mesh.bbox_max - doc.mesh.bbox_min).max_element().max(1e-6);
        let mut boxes: HashMap<usize, (Vec2, Vec2)> = HashMap::new();
        for x in c.looks.iter().flat_map(|l| l.layers.as_array().cloned().unwrap_or_default()) {
            let src = source(&x);
            let (Some(k), Some(b)) = (src["surface"].as_u64(), src["box"].as_array().filter(|a| a.len() == 4)) else { continue };
            let Some(s) = c.surfaces.get(k as usize) else { continue };
            let b = [0, 1, 2, 3].map(|i| b[i].as_f64().unwrap_or(0.0) as f32);
            let (p0, p1) = (s.to_ru(b[0], b[1]), s.to_ru(b[2], b[3]));
            let e = boxes.entry(k as usize).or_insert((p0.min(p1), p0.max(p1)));
            *e = (e.0.min(p0.min(p1)), e.1.max(p0.max(p1)));
        }
        let mut picks: Vec<(usize, Vec2, Vec2)> = boxes.into_iter().map(|(k, (lo, hi))| (k, lo, hi)).collect();
        picks.sort_by(|a, b| ((b.2 - b.1).x * (b.2 - b.1).y).total_cmp(&((a.2 - a.1).x * (a.2 - a.1).y)));
        for (k, s) in c.surfaces.iter().enumerate() {
            if picks.len() >= 2 {
                break;
            }
            if s.out.y > -0.5 && !picks.iter().any(|p| p.0 == k) {
                // A detail: the middle of the surface, not all of it.
                let mid = (s.lo + s.hi) * 0.5;
                let half = (s.hi - s.lo) * 0.25;
                picks.push((k, mid - half, mid + half));
            }
        }
        picks.truncate(2);
        picks
            .into_iter()
            .map(|(k, lo, hi)| {
                let s = &c.surfaces[k];
                let mid = (lo + hi) * 0.5;
                let ext = (hi - lo).max_element().max(0.12 * long) * 1.25;
                let dir = (s.out + s.right * 0.35 + s.up * 0.2).normalize();
                let mut cam = crate::camera::OrbitCamera::default();
                cam.target = (s.right * mid.x + s.up * mid.y + s.out * s.depth.1).into();
                cam.yaw = dir.x.atan2(dir.z);
                cam.pitch = dir.y.clamp(-1.0, 1.0).asin().clamp(-1.5, 1.5);
                cam.distance = ext * 0.5 / (cam.fov_y * 0.5).tan();
                cam
            })
            .collect()
    }

    /// Waits until the turn's layers are composited, then sends Muse a
    /// render of the result to check against the request and references.
    fn muse_check_tick(&mut self, ctx: &egui::Context) {
        ctx.request_repaint();
        let Some(ch) = self.muse.check.as_mut() else { return };
        ch.frames += 1;
        let busy = self.doc.as_ref().is_some_and(|d| d.sets.iter().any(|s| s.dirty || s.dirty_region.is_some()));
        ch.quiet = if busy { 0 } else { ch.quiet + 1 };
        if ch.frames < 6 || ch.quiet < 3 {
            return;
        }
        let ch = self.muse.check.take().unwrap();
        let Some(shot) = self.muse_shot() else {
            self.muse_release();
            return;
        };
        let _ = shot.save(muse_dir().join("last_check.png"));
        let now = self.muse_surfaces_now();
        let now_url = now.and_then(|img| {
            let _ = img.save(muse_dir().join("last_check_surfaces.png"));
            let mut png = Vec::new();
            img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).ok()?;
            Some(format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(&png)))
        });
        let mut jpg = Vec::new();
        if image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpg, 85).encode_image(&image::DynamicImage::ImageRgba8(shot).to_rgb8()).is_err() {
            self.muse_release();
            return;
        }
        let url = format!("data:image/jpeg;base64,{}", base64::engine::general_purpose::STANDARD.encode(&jpg));
        let Some(c) = self.muse.convo.as_ref() else {
            self.muse_release();
            return;
        };
        let sets: Vec<usize> = {
            let mut s: Vec<usize> = c.pieces.iter().map(|p| p.set).collect();
            s.sort_unstable();
            s.dedup();
            s
        };
        let sets = self.doc.as_ref().map(|d| sets_text(d, &sets)).unwrap_or_default();
        let refs = if c.refs.is_empty() { String::new() } else { " and with the reference images".to_string() };
        let user = format!(
            "SELF CHECK (automatic, not written by the user). The first image is a render of the model as it looks now in the app (its lighting) with the looks you just applied: the top row the whole model from the front and the back, the bottom row close-ups of its most detailed areas at full texture resolution. Compare it with what the user asked (\"{}\"){refs}. Your answer stays as it is unless the render shows a clear defect: the wrong material (each ambientCG layer of your stacks says in \"got\" which scan the app found for your query: is that scan really this material? leaves scattered on the ground are not a leaf, a wall is not a floor), a color or darkness clearly off, the wrong shine, a texture far too big or too small for the object, visible tiling, stretching or seams, a print broken or misplaced. Choices that work are not defects: never retune a tint, scale, age or pattern that already looks fine, and never redo a look to try a variation. For each look you return, write in \"problem\" the defect you see in the render, and give its complete layers: every layer that is fine exactly as it was, only the layers that fix the problem changed. A scan or image is never replaced by plain colors: when a scan is the wrong material, ask for another scan (a different ambientcg query, with minus words for the tags of the wrong one, or a library material), or a \"texture_prompt\" when no scan library has this material (a living leaf, skin, a painted pattern). Never drop a print (a reference image layer) the user asked for: if one is misplaced, keep it and correct its box (surface boxes are in the SURFACES image, decal boxes in the numbered views image). A face print (\"side\") upside down, sideways or on the wrong face (compare with the photos of the whole object at an angle): correct its \"up\" (the photo edge its letters' tops point to; your stacks give in \"turned\" the turn the app applied) or its side. A wrapped label too high, low, wide or narrow: correct its \"from\", \"to\" or \"arc\" (your stacks give what the app took). The app took the color cast of the light out of face photos (paper made neutral): that is not a defect. Most of the time nothing is clearly wrong: then return no looks. \"reply\": one short sentence on what you corrected, or that it looks right.",
            ch.user
        );
        let style = if self.muse.style == Style::Auto { c.style } else { Some(self.muse.style) };
        let user = if style == Some(Style::Product) {
            format!("{user}\nThis is a PRODUCT: it must look flawless and exactly like the reference images. Any wear, dust, grime, smudge, bump, waviness or blur is a defect, and so is any color, finish or print that differs from the references.")
        } else {
            user
        };
        let facts = placement_facts(c, &self.muse.placed);
        let user = if facts.is_empty() { user } else { format!("{user}\nPRINT PLACEMENT, measured by the app on the SURFACES image:\n{facts}Fix every problem listed: move that print's box, or place it beside the thing it names with \"near\".") };
        let user = if now_url.is_some() {
            format!("{user} The second image is the SURFACES image as the model looks now: the same layout and pixels as the SURFACES image of the first message, in the texture's colors. Check every print there: is it where it is on the real object relative to the holes, edges and buttons, at its real size, readable, not overlapping another? If one is off, return its look with that print's box corrected (same layout, so move the box by what you see).")
        } else {
            user
        };
        let refs: Vec<String> = std::iter::once(url).chain(now_url).collect();
        let req = Request { key: self.muse.key.clone(), first: None, system: c.system.clone(), view: c.view.clone(), sheet: c.sheet.clone(), surf: c.surf.clone(), turns: c.turns.clone(), stacks: stacks_text(c), sets, user, refs, check: true, scans: scans_of(c) };
        self.muse.status = "Muse is checking the result...".into();
        self.muse_spawn(req);
    }

    /// The SURFACES image as the model looks now, when the looks have
    /// prints (`surfaces_now`).
    fn muse_surfaces_now(&self) -> Option<image::RgbaImage> {
        let c = self.muse.convo.as_ref()?;
        let prints = c.looks.iter().any(|l| l.layers.as_array().is_some_and(|a| a.iter().any(|x| print_key(x).is_some())));
        if c.surfaces.is_empty() || !prints {
            return None;
        }
        let doc = self.doc.as_ref()?;
        let (dev, q) = (&self.engine.device, &self.engine.queue);
        let mut maps = HashMap::new();
        for s in &doc.sets {
            let res = s.gpu.final_base.width;
            let base = crate::engine::texture::f16_bytes_to_f32(&s.gpu.final_base.read_bytes(dev, q));
            let emi = if s.gpu.final_emissive.width == res { crate::engine::texture::f16_bytes_to_f32(&s.gpu.final_emissive.read_bytes(dev, q)) } else { Vec::new() };
            maps.insert(s.submesh, (res, base, emi, s.settings.emissive_intensity));
        }
        Some(surfaces_now(&doc.mesh, &c.pieces, &c.surfaces, &maps))
    }

    /// Runs a turn on a worker thread.
    fn muse_spawn(&mut self, req: Request) {
        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let stop = cancel.clone();
        std::thread::spawn(move || {
            let msg = match run(req, &tx, &stop) {
                Ok(t) => Msg::Done(Box::new(t)),
                Err(e) => Msg::Failed(format!("{e:#}")),
            };
            if !stop.load(Ordering::Relaxed) {
                let _ = tx.send(msg);
            }
        });
        self.muse.job = Some(rx);
        self.muse.cancel = Some(cancel);
    }

    /// Applies an answer and shows it.
    fn muse_answer(&mut self, t: TurnOut) {
        let Some(mut c) = self.muse.convo.take() else { return };
        if let Some(f) = t.first {
            let f = *f;
            c.system = f.system;
            c.view = f.view;
            c.sheet = f.sheet;
            c.surf = f.surf;
            c.surfaces = f.surfaces;
            c.pieces = f.pieces;
        }
        let reply = no_dashes(t.plan["reply"].as_str().or_else(|| t.plan["summary"].as_str()).unwrap_or("Done."));
        let mut notes = Vec::new();
        if let Some(a) = t.plan["analysis"].as_str().filter(|_| c.turns.is_empty()) {
            notes.push(no_dashes(a));
        }
        let mut plan = t.plan;
        // A first pass that Muse will check: the view stays as it was until
        // the final result.
        // Replays skip it, unless MF_MUSE_CHECK asks (its plan: the next
        // file of MF_MUSE_PLAN).
        let replay = std::env::var_os("MF_MUSE_PLAN").is_some() && std::env::var_os("MF_MUSE_CHECK").is_none();
        let will_check = !t.check && !replay && std::env::var_os("MF_MUSE_NOCHECK").is_none();
        if will_check && self.muse.hold.is_none() {
            self.muse_hold();
        }
        if t.check {
            guard_check(&c.looks, &mut plan, &t.downloads, &mut notes);
            keep_prints(&c.looks, &mut plan, &mut notes);
        }
        if c.style.is_none() {
            c.style = plan["style"].as_str().and_then(Style::from_word);
        }
        let style = if self.muse.style == Style::Auto { c.style.unwrap_or(Style::Realistic) } else { self.muse.style };
        apply_style(style, &mut plan);
        screen_glass(&c.pieces, &mut plan, &mut notes);
        flat_screens(&mut plan, &mut notes);
        apply_finishes(&mut plan);
        self.guard_plan(&mut plan, &mut notes);
        // A request for the lighting alone keeps the materials, whatever
        // Muse sent.
        if self.muse.light_only {
            let mut dropped = false;
            for k in ["looks", "parts"] {
                if let Some(o) = plan.as_object_mut() {
                    dropped |= o.remove(k).and_then(|v| v.as_array().map(|a| !a.is_empty())).unwrap_or(false);
                }
            }
            if dropped && !t.check {
                notes.push("Your materials were kept: this request was about the lighting only.".to_string());
            }
        }
        // Texturing: the sets prepared first (not for a lighting answer).
        if plan["looks"].as_array().is_some_and(|l| !l.is_empty()) {
            if let Some(sets) = self.muse.prepare.take() {
                self.muse_prepare(&sets);
            }
        }
        let changed = self.muse_apply(&mut c, &plan, &t.images, &t.downloads, &t.user, &mut notes);
        // The packshot lighting it set.
        let lit = plan.get("lighting").filter(|l| l.is_object()).cloned();
        if let Some(l) = &lit {
            let n = self.muse_lighting(l, t.check);
            if !t.check {
                notes.push(format!("Studio lighting set: {n} lights (\"Back to my lights\" in the studio undoes it)."));
            }
        }
        notes.extend(t.notes);
        // What Muse remembers of its answer: the reply and what it changed
        // (the stacks themselves come with the next message).
        let memory = if changed.is_empty() { reply.clone() } else { format!("{reply} [Changed: {}]", changed.join(", ")) };
        // A self check is remembered without its render (it is not resent).
        let (user, refs) = if t.check { ("[Self check of your previous answer, with a render of the result]".to_string(), Vec::new()) } else { (t.user.clone(), t.refs) };
        c.turns.push(Turn { user, refs, reply: memory });
        self.muse.convo = Some(c);
        if t.check {
            // One message for the turn: the answer, and what the check
            // refined in it.
            let mut line = self.muse.pending.take().unwrap_or(ChatLine { user: false, text: reply.clone(), notes: Vec::new(), refs: Vec::new() });
            if !changed.is_empty() {
                line.notes.push(format!("Refined after looking at the result: {reply}"));
            } else if lit.is_some() {
                line.notes.push(format!("Lighting refined after a render: {reply}"));
            }
            line.notes.extend(notes);
            // The corrected lighting rendered too: the one closer to the
            // photo stays.
            if lit.is_some() && self.muse.light_first.is_some() && self.studio_job_idle() {
                self.muse.pending = Some(line);
                let path = muse_dir().join("lighting_verify.png");
                let mut s = self.doc.as_ref().map(|d| d.studio.clone()).unwrap_or_default();
                (s.out_long, s.out_samples) = (640, 96);
                self.studio_queue.clear();
                self.render_studio_shot(self.camera, &s, path.clone());
                self.muse.light_verify = Some(path);
                self.muse.status = "Comparing the corrected lighting with your photo...".into();
                return;
            }
            self.muse.light_first = None;
            self.muse.chat.push(line);
            self.muse_release();
            return;
        }
        let line = ChatLine { user: false, text: reply, notes, refs: Vec::new() };
        // After a turn that changed the model, Muse looks at the result once
        // (not after a check, nor when replaying a saved plan).
        if will_check && !changed.is_empty() {
            self.muse.pending = Some(line);
            let user = t.user.lines().next().unwrap_or_default().to_string();
            self.muse.check = Some(Check { frames: 0, quiet: 0, user });
            self.muse.status = "Muse is checking its result...".into();
        } else if will_check && lit.is_some() && self.studio_job_idle() {
            // The lighting alone: a render of it goes back to Muse.
            self.muse.pending = Some(line);
            let path = muse_dir().join("lighting_check.png");
            let mut s = self.doc.as_ref().map(|d| d.studio.clone()).unwrap_or_default();
            (s.out_long, s.out_samples) = (640, 96);
            self.studio_queue.clear();
            self.render_studio_shot(self.camera, &s, path.clone());
            self.muse.light_check = Some((path, t.user.lines().next().unwrap_or_default().to_string()));
            self.muse.status = "Muse is rendering its lighting to check it...".into();
        } else {
            self.muse.chat.push(line);
            self.muse_release();
        }
    }

    /// Muse's lighting onto the studio (entered, the model centered, when it
    /// is off); before its first answer of a turn the lighting is kept for
    /// "Back to my lights". The number of lights.
    fn muse_lighting(&mut self, l: &Value, check: bool) -> usize {
        let Some(doc) = self.doc.as_mut() else { return 0 };
        let before = doc.studio.clone();
        let n = super::muse_light::apply(l, &mut doc.studio);
        doc.modified = true;
        if !check {
            self.studio_prev = Some(before);
        }
        let locked = doc.studio.lock_camera;
        if !self.view.studio {
            self.toggle_studio();
        }
        // The photo's view, the product centered in it.
        if let Some(p) = super::muse_light::pitch(l).filter(|_| !locked) {
            self.camera.pitch = p;
        }
        self.studio_camera();
        if !locked {
            self.studio_recenter_now();
        }
        n
    }

    /// The studio render of Muse's lighting is saved (`path`): it goes back
    /// to Muse, who corrects the lighting. False: not that render.
    pub(super) fn muse_light_saved(&mut self, path: &std::path::Path) -> bool {
        if self.muse.light_verify.as_deref() == Some(path) {
            self.muse.light_verify = None;
            self.muse_light_verified(path);
            return true;
        }
        if self.muse.light_check.as_ref().map(|(p, _)| p.as_path()) != Some(path) {
            return false;
        }
        let (_, asked) = self.muse.light_check.take().unwrap();
        let img = image::open(path).ok();
        let render = img.as_ref().and_then(crate::studio::photo_light).unwrap_or_default();
        // How far the first lighting is from the photo, to judge Muse's
        // correction against.
        let d = img.as_ref().and_then(crate::studio::photo_profile).zip(self.muse.light_photo.as_ref()).map(|(r, p)| crate::studio::profile_distance(p, &r));
        self.muse.light_first = d.and_then(|d| Some((self.doc.as_ref()?.studio.clone(), self.camera.pitch, d)));
        let url = img.and_then(|i| super::muse_light::jpeg_url(&i).ok());
        let (Some(url), Some(c)) = (url, self.muse.convo.as_ref()) else {
            self.muse_release();
            return true;
        };
        let sets: Vec<usize> = {
            let mut s: Vec<usize> = c.pieces.iter().map(|p| p.set).collect();
            s.sort_unstable();
            s.dedup();
            s
        };
        let sets = self.doc.as_ref().map(|d| sets_text(d, &sets)).unwrap_or_default();
        let req = Request { key: self.muse.key.clone(), first: None, system: c.system.clone(), view: c.view.clone(), sheet: c.sheet.clone(), surf: c.surf.clone(), turns: c.turns.clone(), stacks: stacks_text(c), sets, user: super::muse_light::check_text(&asked, &self.muse.light_facts, &render), refs: vec![url], check: true, scans: scans_of(c) };
        self.muse.status = "Muse is comparing the render with your photo...".into();
        self.muse_spawn(req);
        true
    }

    /// The corrected lighting's render is saved: the lighting closer to the
    /// photo stays (Muse's first one back when the correction moved away).
    fn muse_light_verified(&mut self, path: &std::path::Path) {
        let first = self.muse.light_first.take();
        let now = image::open(path).ok().as_ref().and_then(crate::studio::photo_profile);
        let note = match (first, now, self.muse.light_photo.as_ref()) {
            (Some((studio, pitch, d1)), Some(r), Some(p)) => {
                let d2 = crate::studio::profile_distance(p, &r);
                if d2 > d1 {
                    let locked = studio.lock_camera;
                    if let Some(doc) = self.doc.as_mut() {
                        doc.studio = studio;
                        doc.modified = true;
                    }
                    if !locked {
                        self.camera.pitch = pitch;
                    }
                    self.studio_camera();
                    if !locked {
                        self.studio_recenter_now();
                    }
                    Some(format!("Its correction moved away from your photo (difference {:.0}% against {:.0}%), so its first lighting was kept.", d2 * 100.0, d1 * 100.0))
                } else {
                    Some(format!("The correction is closer to your photo (difference {:.0}%, was {:.0}%).", d2 * 100.0, d1 * 100.0))
                }
            }
            _ => None,
        };
        if let (Some(n), Some(line)) = (note, self.muse.pending.as_mut()) {
            line.notes.push(n);
        }
        self.muse.status.clear();
        self.muse_release();
    }

    /// The studio render was cancelled: Muse's answer shows as it was.
    pub(super) fn muse_light_cancelled(&mut self) {
        if self.muse.light_verify.take().is_some() {
            self.muse.light_first = None;
            self.muse.status.clear();
            self.muse_release();
        }
        if self.muse.light_check.take().is_some() {
            self.muse.status.clear();
            self.muse_release();
        }
    }

    /// Attaches an image file to the message being typed.
    pub(super) fn muse_attach(&mut self, ctx: &egui::Context, path: &std::path::Path) {
        match load_ref(ctx, path) {
            Ok(r) => self.muse.attach.push(r),
            Err(e) => self.toast(format!("{} could not be attached: {e:#}", path.display())),
        }
    }

    /// A file dropped while the Muse panel is open: an image becomes a
    /// reference (anywhere on the window: Windows gives no pointer position
    /// during a file drag).
    pub(super) fn muse_drop(&mut self, ctx: &egui::Context, path: &std::path::Path) -> bool {
        let open = self.dock.as_ref().is_some_and(|d| d.muse);
        if !(open && is_image(path)) {
            return false;
        }
        self.muse_attach(ctx, path);
        true
    }

    /// The layer with the reference images its mask steps and channel maps
    /// name ({"reference": n}) imported as gray maps ("texture_id"); a UV
    /// sheet map puts the layer on the UV map ("uv_maps").
    fn image_maps(&mut self, v: &Value, refs: &[std::path::PathBuf], name: &str, notes: &mut Vec<String>) -> Value {
        let mut v = v.clone();
        let mut uv = false;
        for key in ["roughness_pattern", "height_pattern", "metallic_pattern"] {
            if v[key].is_object() {
                uv |= self.gray_map(&mut v[key], refs, name, notes);
            }
        }
        if v["mask"].is_object() {
            let n = v["mask"]["steps"].as_array().map_or(0, |a| a.len());
            for i in 0..n {
                self.gray_map(&mut v["mask"]["steps"][i], refs, name, notes);
            }
        }
        let n = v["mask"].as_array().map_or(0, |a| a.len());
        for i in 0..n {
            self.gray_map(&mut v["mask"][i], refs, name, notes);
        }
        if uv {
            v["uv_maps"] = Value::Bool(true);
        }
        v
    }

    /// One {"reference": n} map as a gray texture; true when it is a UV sheet.
    fn gray_map(&mut self, x: &mut Value, refs: &[std::path::PathBuf], name: &str, notes: &mut Vec<String>) -> bool {
        let Some(n) = x["reference"].as_u64() else { return false };
        let Some(path) = (n as usize).checked_sub(1).and_then(|i| refs.get(i)) else {
            notes.push(format!("{name}: there is no reference R{n} for its map, left out."));
            return false;
        };
        let made = gray_image(path).and_then(|g| {
            let doc = self.doc.as_mut().ok_or_else(|| anyhow::anyhow!("no project"))?;
            doc.import_resource(&self.engine, &g, ResKind::Texture)
        });
        match made {
            Ok(id) => {
                x["texture_id"] = Value::from(id);
                x["uv_sheet"].as_bool() == Some(true)
            }
            Err(e) => {
                notes.push(format!("{name}: R{n} could not be used as a map ({e:#}), left out."));
                false
            }
        }
    }

    /// One plan layer as a layer tree (None: nothing usable).
    #[allow(clippy::too_many_arguments)]
    #[allow(clippy::too_many_arguments)]
    fn muse_layer(&mut self, v: &Value, images: &HashMap<String, image::RgbaImage>, downloads: &HashMap<String, (std::path::PathBuf, String)>, refs: &[std::path::PathBuf], pieces: &[pieces::Piece], surfaces: &[Surface], ids: &[usize], notes: &mut Vec<String>) -> Option<(LayerTree, Vec<usize>, Vec<Vec<(usize, Vec<u32>)>>)> {
        let name = v["name"].as_str().unwrap_or("Layer").to_string();
        let owned = self.image_maps(v, refs, &name, notes);
        let v = &owned;
        let src = source(v);
        // A library material, smart material or downloaded ambientCG material.
        let item = if let Some(q) = src["ambientcg"].as_str() {
            match downloads.get(q).and_then(|(dir, label)| crate::library::tex_material_at(dir).map(|m| (m, label))) {
                Some((m, _)) if m.map("color").as_deref().is_some_and(is_cutout) => {
                    notes.push(format!("{name}: {} is a single cut-out object on a background, not a surface that tiles, skipped.", m.name));
                    return None;
                }
                Some((m, label)) => {
                    notes.push(format!("{name}: ambientCG {label}"));
                    Some(super::shelf::Item::TexMaterial(Box::new(m)))
                }
                None => {
                    notes.push(format!("{name}: no ambientCG material for \"{q}\", skipped."));
                    return None;
                }
            }
        } else if let Some(lib) = src["smart_material"].as_str().or_else(|| src["material"].as_str()) {
            match self.shelf.material_item(lib) {
                Some(it) => Some(it),
                None => {
                    notes.push(format!("{name}: \"{lib}\" is not in the library, skipped."));
                    return None;
                }
            }
        } else {
            None
        };
        // A print's placement (decal or fit), set after the projection.
        let mut placed: Option<Projection> = None;
        // UV along islands that point different ways: (groups, rotation
        // asked, across).
        let mut split: Option<(Vec<(f32, Vec<(usize, Vec<u32>)>)>, f32, f32, bool)> = None;
        let (mut tree, ext) = if let Some(item) = item {
            let color_map = match &item {
                super::shelf::Item::TexMaterial(m) => m.map("color"),
                _ => None,
            };
            let Some(mut t) = self.item_tree(item) else {
                notes.push(format!("{name}: the material could not be loaded, skipped."));
                return None;
            };
            // Scanned materials: tri-planar at the object's real size, not
            // stretched over its UV layout.
            if let LayerKind::Fill(f) = &mut t.desc.kind {
                f.projection.mode = ProjMode::Triplanar;
                // Never under 1: a scan stretched over more than its size
                // looks blurry.
                f.projection.scale = num(src, "scale").unwrap_or(2.0).clamp(1.0, 200.0);
                // "tint" is the average color it should have: the scan
                // keeps its detail around that color. A tint is a plain
                // color multiplier (0 to 1, what the tint bar shows: white
                // gives the scan back), so it darkens and shifts the hue
                // but never brightens.
                if let Some(c) = hex(&src["tint"]) {
                    let avg = color_map.as_deref().and_then(average_color).unwrap_or([1.0; 3]);
                    let ratio = [0, 1, 2].map(|i| c[i] / avg[i].max(0.01));
                    let top = ratio.iter().copied().fold(1.0f32, f32::max);
                    f.channels[CH_COLOR].value = ratio.map(|r| (r / top).clamp(0.0, 1.0));
                }
                // Relief: the scan's height (and normal map) strength, 1 = as
                // the library applies it.
                let relief = num(src, "relief").unwrap_or(0.5).clamp(0.0, 1.5);
                let h = &mut f.channels[CH_HEIGHT];
                if h.source == SourceKind::Texture {
                    h.value_b = h.value_b.map(|v| v * relief);
                }
                if relief < 0.05 {
                    f.channels[CH_NORMAL].enabled = false;
                }
                // Roughness maps are rescaled into [min, max].
                if let (Some(lo), Some(hi)) = (num(src, "roughness_min"), num(src, "roughness_max")) {
                    let ch = &mut f.channels[CH_ROUGH];
                    if ch.source == SourceKind::Texture {
                        ch.value = [lo.clamp(0.0, 1.0); 3];
                        ch.value_b = [hi.clamp(0.0, 1.0); 3];
                    }
                }
            }
            (t, Vec::new())
        } else {
            let place_box = src["decal"].as_array().filter(|a| a.len() == 4).map(|a| [0, 1, 2, 3].map(|i| a[i].as_f64().unwrap_or(0.0) as f32));
            let place_fit = src["fit"].as_bool() == Some(true);
            let place_surf = src["box"].as_array().filter(|a| a.len() == 4).map(|a| [0, 1, 2, 3].map(|i| a[i].as_f64().unwrap_or(0.0) as f32));
            let place_near = src["near"].is_string() || src["near"].is_number();
            let place_side = side_index(&src["side"]);
            let place_wrap = src["wrap"].is_object() || src["wrap"].as_bool() == Some(true);
            let placing = place_surf.is_some() || place_near || place_box.is_some() || place_fit || place_side.is_some() || place_wrap;
            // The image: a reference (cropped to its print when it goes in a
            // box), or a generated texture.
            let path = if let Some(f) = src["cut_file"].as_str() {
                // A face print the side pass already cut, balanced and turned.
                Some(std::path::PathBuf::from(f))
            } else if let Some(n) = src["reference"].as_u64() {
                let Some(path) = (n as usize).checked_sub(1).and_then(|i| refs.get(i)) else {
                    notes.push(format!("{name}: there is no reference R{n}, skipped."));
                    return None;
                };
                match cut_print(path, src) {
                    // A part of the photo, straightened or unrolled.
                    Some(Ok(p)) => Some(p),
                    Some(Err(e)) => {
                        notes.push(format!("{name}: that part of R{n} could not be cut ({e:#}), skipped."));
                        return None;
                    }
                    None => Some(if place_box.is_some() || place_surf.is_some() || place_near || place_side.is_some() || place_wrap { trimmed(path) } else { path.clone() }),
                }
            } else if let Some(t) = src["text"].as_str() {
                if !placing {
                    notes.push(format!("{name}: text needs a place (a surface box), skipped."));
                    return None;
                }
                match text_image(t, src["font"].as_str().unwrap_or("Arial"), src["bold"].as_bool().unwrap_or(false), hex(&src["color"]).unwrap_or([1.0; 3])) {
                    Ok(p) => Some(p),
                    Err(e) => {
                        notes.push(format!("{name}: the text could not be drawn ({e:#}), skipped."));
                        return None;
                    }
                }
            } else if let Some(n) = src["sample"].as_u64() {
                let Some(path) = (n as usize).checked_sub(1).and_then(|i| refs.get(i)) else {
                    notes.push(format!("{name}: there is no reference R{n} to sample, skipped."));
                    return None;
                };
                let Some(r) = src["region"].as_array().filter(|a| a.len() == 4).map(|a| [0, 1, 2, 3].map(|i| a[i].as_f64().unwrap_or(0.0) as f32)) else {
                    notes.push(format!("{name}: a sample needs its \"region\" [x0, y0, x1, y1] on R{n}, skipped."));
                    return None;
                };
                match sample_texture(path, r, src["flatten"].as_bool() == Some(true), hex(&src["mean"])) {
                    Ok(p) => Some(p),
                    Err(e) => {
                        notes.push(format!("{name}: R{n} could not be sampled ({e:#}), skipped."));
                        return None;
                    }
                }
            } else {
                match src["texture_prompt"].as_str().and_then(|t| images.get(t)) {
                    Some(img) => {
                        let dir = muse_dir();
                        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
                        let slug: String = name.chars().map(|ch| if ch.is_alphanumeric() { ch } else { '_' }).collect();
                        let path = dir.join(format!("{slug}_{stamp}.png"));
                        if let Err(e) = std::fs::create_dir_all(&dir).map_err(anyhow::Error::from).and_then(|_| img.save(&path).map_err(anyhow::Error::from)) {
                            notes.push(format!("{name}: the generated texture could not be saved ({e:#})."));
                            return None;
                        }
                        Some(path)
                    }
                    None => None,
                }
            };
            if let Some(path) = path.as_ref().filter(|_| placing) {
                let doc = self.doc.as_ref()?;
                let aspect = image::image_dimensions(path).map(|(w, h)| w as f32 / h.max(1) as f32).unwrap_or(1.0);
                placed = if place_wrap {
                    match wrap_decal(&doc.mesh, pieces, ids, &src["wrap"], aspect) {
                        Some((p, how, got)) => {
                            notes.push(format!("{name}: wrapped {how}."));
                            let mut m = serde_json::Map::new();
                            m.insert("wrap".into(), got);
                            self.muse.resolved.insert(name.clone(), m);
                            Some(p)
                        }
                        None => None,
                    }
                } else if let Some(side) = place_side {
                    // A whole side unless an area says where on it: filled
                    // (the face print covers its face), or as "stretch" says.
                    let area = src["area"].as_array().filter(|a| a.len() == 4).map(|a| [0, 1, 2, 3].map(|i| a[i].as_f64().unwrap_or(0.0) as f32));
                    let fill = src["stretch"].as_bool().unwrap_or(area.is_none());
                    side_decal(&doc.mesh, pieces, ids, side, area, if fill { 0.0 } else { aspect })
                } else if place_surf.is_some() || place_near {
                    let k = src["surface"].as_u64().map(|k| k as usize).filter(|&k| k < surfaces.len()).or_else(|| {
                        let b = place_surf?;
                        let (cx, cy) = ((b[0] + b[2]) * 0.5, (b[1] + b[3]) * 0.5);
                        surfaces.iter().position(|s| {
                            let r = s.rect();
                            cx >= r[0] && cx <= r[2] && cy >= r[1] && cy <= r[3]
                        })
                    });
                    match k.map(|k| (k, &surfaces[k])) {
                        Some((_, s)) if !ids.contains(&s.piece) => {
                            notes.push(format!("{name}: its surface is on piece {}, not in this look, skipped.", s.piece));
                            return None;
                        }
                        Some((k, s)) => {
                            let f = |key: &str| src[key].as_f64().map(|v| v as f32);
                            let b = place_surf.or_else(|| s.near_box(&src["near"], src["side"].as_str().unwrap_or("above"), f("size"), f("gap"), aspect));
                            // A stretched print over nearly all its surface (a
                            // whole label): all of it, edge to edge.
                            let b = b.map(|b| {
                                let r = s.rect();
                                let cover = |a0: f32, a1: f32, r0: f32, r1: f32| (a1.min(r1) - a0.max(r0)).max(0.0) / (r1 - r0).max(1e-3);
                                let full = src["stretch"].as_bool() == Some(true) && cover(b[0], b[2], r[0], r[2]) > 0.85 && cover(b[1], b[3], r[1], r[3]) > 0.85;
                                if full {
                                    r
                                } else {
                                    b
                                }
                            });
                            let Some(b) = b else {
                                notes.push(format!("{name}: {} is not in front of surface {k} (see its list), skipped.", src["near"]));
                                return None;
                            };
                            if let Some(key) = print_key(v) {
                                self.muse.placed.insert(key, (name.clone(), k, b));
                            }
                            s.place(&doc.mesh, b, if src["stretch"].as_bool() == Some(true) { 0.0 } else { aspect })
                        }
                        None => None,
                    }
                } else {
                    match place_box {
                        Some(b) => decal_in_box(&doc.mesh, &model_tris(&doc.mesh, pieces, ids), b, aspect),
                        None => decal_fit(&doc.mesh, &piece_tris(&doc.mesh, pieces, ids)),
                    }
                };
                if placed.is_none() {
                    let why = if place_wrap {
                        "its pieces give no round body to wrap it around (or its facing runs along their axis)"
                    } else if place_side.is_some() {
                        "its side is not one of the SIDES"
                    } else if place_surf.is_some() || place_near {
                        "its surface box is on no surface of the SURFACES image"
                    } else if place_box.is_some() { "its decal box is not on this look's pieces in the views" } else { "its pieces do not face one way (place it with a decal box)" };
                    notes.push(format!("{name}: {why}, skipped."));
                    return None;
                }
            }
            let tex = match path {
                Some(path) => {
                    let doc = self.doc.as_mut()?;
                    match doc.import_resource(&self.engine, &path, ResKind::Texture) {
                        Ok(id) => Some(id),
                        Err(e) => {
                            notes.push(format!("{name}: its image could not be added ({e:#})."));
                            return None;
                        }
                    }
                }
                None => None,
            };
            let (f, ext) = fill(v, tex);
            (LayerTree { desc: LayerDesc::new(0, name.clone(), LayerKind::Fill(Box::new(f))), children: Vec::new() }, ext)
        };
        tree.desc.name = name;
        // Blend: one mode for every channel the layer gives (the normal keeps
        // its own), or a mode per channel.
        match &v["blend"] {
            Value::Object(m) => {
                for (k, b) in m {
                    if let (Some(ch), Some(b)) = (channel_index(k), blend_mode(b)) {
                        tree.desc.blend[ch] = b;
                    }
                }
            }
            b => {
                if let Some(b) = blend_mode(b) {
                    for ch in (0..NCH).filter(|&ch| ch != CH_NORMAL) {
                        tree.desc.blend[ch] = b;
                    }
                }
            }
        }
        let o = num(v, "layer_opacity").unwrap_or(1.0).clamp(0.0, 1.0);
        tree.desc.opacity = [o; NCH];
        tree.desc.ext_opacity = o;
        if let Some(m) = v["channel_opacity"].as_object() {
            for (k, x) in m {
                if let (Some(ch), Some(x)) = (channel_index(k), x.as_f64()) {
                    tree.desc.opacity[ch] = o * (x as f32).clamp(0.0, 1.0);
                }
            }
        }
        if let (Some(pj), LayerKind::Fill(f)) = (v["projection"].as_object(), &mut tree.desc.kind) {
            let pv = Value::Object(pj.clone());
            let mode = match pv["mode"].as_str().map(|m| m.trim().to_lowercase()) {
                Some(m) if m.contains("tri") => Some(ProjMode::Triplanar),
                Some(m) => by_name(ProjMode::ALL, |p| p.name(), &Value::String(m)),
                None => None,
            };
            if let Some(m) = mode {
                f.projection.mode = m;
            }
            if let Some(x) = num(&pv, "scale") {
                // Scans never stretched past their size (blurry).
                let scan = src["material"].is_string() || src["ambientcg"].is_string();
                f.projection.scale = x.clamp(if scan { 1.0 } else { 0.05 }, 200.0);
            }
            // "repeat": n repeats across the look's own pieces (their
            // largest side), whatever the object's size: 1 shows the whole
            // texture once across them (a sample of that very part).
            if let (Some(k), Some(doc)) = (num(&pv, "repeat"), self.doc.as_ref()) {
                let (lo, hi) = ids.iter().filter_map(|&i| pieces.get(i)).fold((glam::Vec3::splat(f32::MAX), glam::Vec3::splat(f32::MIN)), |(a, b), p| (a.min(p.lo), b.max(p.hi)));
                let long = (doc.mesh.bbox_max - doc.mesh.bbox_min).max_element().max(1e-6);
                let ext = (hi - lo).max_element();
                if ext > 0.0 {
                    f.projection.scale = (k * long / ext).clamp(0.05, 200.0);
                }
            }
            if let Some(x) = num(&pv, "rotation") {
                f.projection.rotation = x;
            }
            if let Some(o) = pv["offset"].as_array().filter(|a| a.len() == 2) {
                f.projection.offset = [o[0].as_f64().unwrap_or(0.0) as f32, o[1].as_f64().unwrap_or(0.0) as f32];
            }
            // UV: "scale" keeps its meaning (repeats across the model's
            // longest side) at the look's measured texel size; "along"
            // turns the texture's vertical along the pieces' length.
            if f.projection.mode == ProjMode::Uv && src["uv_sheet"].as_bool() != Some(true) {
                if let Some(doc) = self.doc.as_ref() {
                    if let Some(fit) = uv_fit(&doc.mesh, pieces, ids) {
                        let long = (doc.mesh.bbox_max - doc.mesh.bbox_min).max_element().max(1e-6);
                        f.projection.scale = (f.projection.scale * fit.unit() / long).clamp(0.05, 200.0);
                        let across = pv["along"].as_str() == Some("across");
                        // The texture's own grain (vertical when it has none).
                        let grain = if pv["along"].is_string() { self.fill_grain(f).unwrap_or(90.0) } else { 90.0 };
                        let turn = match pv["along"].as_str() {
                            Some("length") | Some("across") => fit.turn(grain, across),
                            _ => Some(0.0),
                        };
                        match turn {
                            Some(t) => f.projection.rotation += t,
                            None => {
                                let groups = uv_groups(&fit);
                                if groups.len() >= 2 {
                                    split = Some((groups, f.projection.rotation, grain, across));
                                } else if let Some(g) = groups.first() {
                                    f.projection.rotation += uv_turn(grain, g.0, across);
                                }
                            }
                        }
                    }
                }
            }
        }
        // A UV sheet goes on as it is, over the UV map.
        if let (true, LayerKind::Fill(f)) = (src["uv_sheet"].as_bool() == Some(true), &mut tree.desc.kind) {
            f.projection = Projection { mode: ProjMode::Uv, scale: 1.0, ..Default::default() };
        }
        // A placed print: once, where it was put (a Transform would tile it).
        let is_placed = placed.is_some() || src["uv_sheet"].as_bool() == Some(true);
        if let (Some(pr), LayerKind::Fill(f)) = (placed, &mut tree.desc.kind) {
            f.projection = pr;
        }
        if let Some(m) = mask(&v["mask"]) {
            tree.desc.mask = Some(m);
        }
        let filters: Vec<FilterDesc> = v["filters"].as_array().into_iter().flatten().filter_map(filter).filter(|f| !(is_placed && f.kind == FilterKind::Transform)).collect();
        if matches!(tree.desc.kind, LayerKind::Folder) {
            tree.children.extend(filters.into_iter().map(|f| LayerTree { desc: LayerDesc::new(0, f.kind.name().into(), LayerKind::Filter(f)), children: Vec::new() }));
        } else {
            tree.desc.effects = filters.into_iter().map(|f| ContentEffect { id: 0, enabled: true, opacity: 1.0, filter: f }).collect();
        }
        // Islands pointing different ways: a copy per direction, turned for
        // it and masked to its islands (painted once the ids exist).
        let mut islands = Vec::new();
        if let Some((groups, rot, grain, across)) = split {
            let mut folder = LayerTree { desc: LayerDesc::new(0, tree.desc.name.clone(), LayerKind::Folder), children: Vec::new() };
            for (angle, isl) in groups {
                let mut copy = tree.clone();
                if let LayerKind::Fill(f) = &mut copy.desc.kind {
                    f.projection.rotation = rot + uv_turn(grain, angle, across);
                }
                let paint = |blend| MaskEffect { id: 0, enabled: true, blend, opacity: 1.0, kind: MaskEffectKind::Paint };
                match &mut copy.desc.mask {
                    Some(m) => m.effects.push(paint(MaskBlend::Multiply)),
                    None => copy.desc.mask = Some(MaskDesc { enabled: true, base: 0.0, effects: vec![paint(MaskBlend::Normal)] }),
                }
                copy.desc.name = format!("{} ({angle:.0} deg)", tree.desc.name);
                folder.children.push(copy);
                islands.push(isl);
            }
            tree = folder;
        }
        Some((tree, ext, islands))
    }

    /// Face prints ("side" with a reference): each photo's face cut out
    /// (found on its background when no crop or quad says where), its color
    /// cast taken out, then turned, or traded with the print of the
    /// opposite side, so the prints meet best across the product's edges.
    /// Each such layer gets its print file ("cut_file").
    fn muse_sides(&mut self, layers: &mut [Value], refs: &[std::path::PathBuf], pieces: &[pieces::Piece], ids: &[usize], notes: &mut Vec<String>) {
        let Some(doc) = self.doc.as_ref() else { return };
        let Some((axes, lo, hi)) = look_frame(&doc.mesh, pieces, ids) else { return };
        let all = photo::sides(axes, lo, hi);
        struct Job {
            layer: usize,
            n: usize,
            side: usize,
            img: image::RgbaImage,
            how: &'static str,
        }
        let mut jobs = Vec::new();
        for (li, l) in layers.iter().enumerate() {
            let src = source(l);
            let (Some(side), Some(n)) = (side_index(&src["side"]), src["reference"].as_u64()) else { continue };
            let Some(path) = (n as usize).checked_sub(1).and_then(|i| refs.get(i)) else { continue };
            let name = l["name"].as_str().unwrap_or("Print");
            let cut = match cut_print(path, src) {
                Some(Ok(p)) => image::open(&p).ok().map(|i| (i.to_rgba8(), "cut where you said")),
                Some(Err(e)) => {
                    notes.push(format!("{name}: that part of R{n} could not be cut ({e:#}), skipped."));
                    continue;
                }
                // The face found on the photo's background, its sides on
                // the face's edges.
                None => image::open(path).ok().map(|i| i.to_rgba8()).map(|img| match photo::find_object(&img).filter(|f| f.fill >= 0.8) {
                    Some(f) => {
                        let q = snap_quad(&img, f.quad, 0.03);
                        match cut_quad(&img, q, None) {
                            Ok(c) => (c, "found on the photo"),
                            Err(_) => (img, "the whole photo"),
                        }
                    }
                    None => (img, "the whole photo (no single flat face found on it)"),
                }),
            };
            if let Some((img, how)) = cut {
                jobs.push(Job { layer: li, n: n as usize, side, img, how });
            }
        }
        if jobs.is_empty() {
            return;
        }
        // The light's color cast: the face photos of a product are taken
        // together, so one cast for all, from the papers most of them agree
        // on (a face printed in a light color, peach or mint, stays out).
        let own: Vec<Option<([f32; 3], [f32; 3])>> = jobs.iter().map(|j| photo::paper_of(&j.img)).collect();
        let papers: Vec<([f32; 3], [f32; 3])> = own.iter().flatten().copied().collect();
        let near = |x: &([f32; 3], [f32; 3]), y: &([f32; 3], [f32; 3])| ((x.1[1] - y.1[1]).powi(2) + (x.1[2] - y.1[2]).powi(2)).sqrt() < 12.0;
        let group: Vec<&([f32; 3], [f32; 3])> = papers
            .iter()
            .map(|x| papers.iter().filter(|y| near(x, y)).collect::<Vec<_>>())
            .max_by(|g, h| g.len().cmp(&h.len()).then_with(|| {
                let chroma = |g: &Vec<&([f32; 3], [f32; 3])>| g.iter().map(|p| (p.1[1].powi(2) + p.1[2].powi(2)).sqrt()).sum::<f32>() / g.len() as f32;
                chroma(h).total_cmp(&chroma(g))
            }))
            .unwrap_or_default();
        if !group.is_empty() {
            let mean = |k: usize, lab: bool| group.iter().map(|p| if lab { p.1[k] } else { p.0[k] }).sum::<f32>() / group.len() as f32;
            let (a, b) = (mean(1, true), mean(2, true));
            if (a * a + b * b).sqrt() > 4.0 {
                let gains = photo::neutral_gains([mean(0, false), mean(1, false), mean(2, false)]);
                let mut count = 0;
                for (j, paper) in jobs.iter_mut().zip(&own) {
                    if source(&layers[j.layer])["balance"].as_bool() == Some(false) {
                        continue;
                    }
                    // A photo whose paper is the cast: its own paper neutral;
                    // the others (a face of another color) the shared gains.
                    let g = match paper {
                        Some(p) if group.iter().any(|q| near(p, q)) => photo::neutral_gains(p.0),
                        _ => gains,
                    };
                    j.img = photo::balanced(&j.img, g);
                    count += 1;
                }
                let way = if b.abs() >= a.abs() { if b < 0.0 { "blue" } else { "yellow" } } else if a > 0.0 { "magenta" } else { "green" };
                notes.push(format!("{count} face photo(s) had a {way} cast from the light, taken out (their paper made neutral; \"balance\": false keeps a photo's colors)."));
            }
        }
        let prints: Vec<photo::SidePrint> = jobs
            .iter()
            .map(|j| {
                let src = source(&layers[j.layer]);
                let turn = src["rotate"].as_f64().map(|r| ((r / 90.0).round() as i64).rem_euclid(4) as u32 * 90);
                // "up": the edge of the photo at the top of the face as the
                // product stands; "toward": the side that edge touches (up,
                // +y, for a wall; say it for the top and bottom).
                let up = match src["up"].as_str().map(|s| s.trim().to_lowercase()) {
                    Some(s) if s == "top" => Some(0),
                    Some(s) if s == "right" => Some(270),
                    Some(s) if s == "bottom" => Some(180),
                    Some(s) if s == "left" => Some(90),
                    _ => None,
                };
                let toward = side_index(&src["toward"]).map(|t| all[t].out).or_else(|| (j.side / 2 != 1).then_some(all[j.side].up));
                let up = up.zip(toward).and_then(|(u, t)| {
                    let s = &all[j.side];
                    let extra = [(s.up, 0), (s.right, 90), (-s.up, 180), (-s.right, 270)].into_iter().find(|(d, _)| d.dot(t) > 0.9)?.1;
                    Some((u + extra) % 360)
                });
                photo::SidePrint {
                    side: j.side,
                    img: image::imageops::thumbnail(&j.img, 192, 192),
                    aspect: j.img.width() as f32 / j.img.height().max(1) as f32,
                    turn,
                    up,
                    free: turn.is_none() && src["lock"].as_bool() != Some(true),
                }
            })
            .collect();
        let (choice, _) = photo::solve_sides(&all, &prints);
        let dir = muse_dir().join("prints");
        let _ = std::fs::create_dir_all(&dir);
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
        for (i, &(from, turn)) in choice.iter().enumerate() {
            let (j, f) = (&jobs[i], &jobs[from]);
            let img = photo::turned(&f.img, turn);
            let file = dir.join(format!("side_{}_r{}_{stamp}.png", j.side, f.n));
            if let Err(e) = img.save(&file) {
                notes.push(format!("A face print could not be saved ({e:#})."));
                continue;
            }
            let name = layers[j.layer]["name"].as_str().unwrap_or("Print").to_string();
            let l = &mut layers[j.layer];
            let s = if l["source"].is_object() { &mut l["source"] } else { l };
            s["cut_file"] = Value::String(file.to_string_lossy().to_string());
            s["reference"] = serde_json::json!(f.n);
            let mut got = serde_json::Map::new();
            got.insert("reference".into(), serde_json::json!(f.n));
            got.insert("turned".into(), serde_json::json!(turn));
            self.muse.resolved.insert(name.clone(), got);
            let mut note = format!("{name}: R{} on side {}, {}, turned {turn} deg", f.n, SIDE_NAMES[j.side], f.how);
            if from != i {
                note += &format!(" (moved from side {}: its proportions and edges fit here)", SIDE_NAMES[jobs[from].side]);
            }
            let a = img.width() as f32 / img.height().max(1) as f32;
            if (a / all[j.side].aspect()).ln().abs() > 1.3f32.ln() {
                note += &format!("; its proportions ({a:.2}) are not the side's ({:.2}): it may belong on another side", all[j.side].aspect());
            }
            notes.push(note + ".");
        }
    }

    /// A layer's Paint mask: the given islands' polygons only.
    fn muse_mask_islands(&mut self, set: usize, layer: u64, pieces: &[pieces::Piece], islands: &[(usize, Vec<u32>)]) {
        let Some(doc) = self.doc.as_mut() else { return };
        let res = doc.sets[set].resolution();
        let some: Vec<pieces::Piece> = islands.iter().filter(|(p, _)| pieces.get(*p).is_some_and(|q| q.set == set)).map(|(p, tris)| pieces::Piece { tris: tris.clone(), ..pieces[*p].clone() }).collect();
        let own = pieces::owners(&doc.mesh, &some, set, res);
        let keep: Vec<usize> = (0..some.len()).collect();
        let data = crate::engine::texture::f32_to_f16_bytes(&pieces::mask(&own, &keep));
        let Some(l) = find_mut(&mut doc.sets[set].layers, layer) else { return };
        let Some(eid) = l.desc.mask.as_ref().and_then(|m| m.effects.iter().find(|e| matches!(e.kind, MaskEffectKind::Paint))).map(|e| e.id) else { return };
        let tex = full_mask_paint(&self.engine.device, res);
        tex.write_bytes(&self.engine.queue, &data);
        l.mask_paint.insert(eid, tex);
        l.rev = next_rev();
        doc.sets[set].dirty = true;
    }

    /// The grain of a fill's main map (color, else height, else
    /// roughness): an image measured as it is, a pattern rendered small.
    fn fill_grain(&self, f: &FillDesc) -> Option<f32> {
        let doc = self.doc.as_ref()?;
        for ch in [CH_COLOR, CH_HEIGHT, CH_ROUGH] {
            let c = &f.channels[ch];
            if !c.enabled {
                continue;
            }
            match c.source {
                SourceKind::Texture => {
                    if let Some(r) = c.texture.and_then(|id| doc.resources.get(&id)) {
                        return grain_of_image(&r.image);
                    }
                }
                SourceKind::Procedural => {
                    if let Some(mut u) = c.procedural.gen_u(256) {
                        u.k[2] = 256;
                        let (dev, q) = (&self.engine.device, &self.engine.queue);
                        let mut enc = dev.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("muse grain") });
                        let t = self.engine.procgen.get(dev, &mut enc, &u);
                        q.submit([enc.finish()]);
                        let px = crate::engine::texture::f16_bytes_to_f32(&t.read_bytes(dev, q));
                        let v: Vec<f32> = px.chunks(4).map(|c| c[0]).collect();
                        let n = (v.len() as f32).sqrt() as usize;
                        return grain_of(n, n, &v);
                    }
                }
                _ => {}
            }
        }
        None
    }

    /// Builds the looks of an answer: each look's layers in a folder per
    /// texture set, masked to its pieces' polygons. The pieces it takes leave
    /// the looks they were in (their folders' masks shrink; a look left with
    /// no piece is removed, its folder's place reused). Returns what changed.
    fn muse_apply(&mut self, c: &mut Convo, plan: &Value, images: &HashMap<String, image::RgbaImage>, downloads: &HashMap<String, (std::path::PathBuf, String)>, user: &str, notes: &mut Vec<String>) -> Vec<String> {
        let mut changed = Vec::new();
        let answer: Vec<Value> = plan["looks"].as_array().or_else(|| plan["parts"].as_array()).cloned().unwrap_or_default();
        let settings: Vec<Value> = plan["texture_sets"].as_array().cloned().unwrap_or_default();
        if self.doc.is_none() || (answer.is_empty() && settings.is_empty()) {
            return changed;
        }
        // The self check refines the turn: one undo step for both.
        if !user.starts_with("SELF CHECK") {
            let short: String = user.chars().take(40).collect();
            self.checkpoint(&format!("Muse: {short}"));
        }
        // Texture set settings (the sets of the conversation only).
        for t in settings {
            let Some(name) = t["name"].as_str().map(str::trim) else { continue };
            let doc = self.doc.as_mut().unwrap();
            let Some(set) = doc.sets.iter().position(|s| s.name.eq_ignore_ascii_case(name)).filter(|&s| c.pieces.iter().any(|p| p.set == s)) else {
                notes.push(format!("No texture set \"{name}\" in this conversation."));
                continue;
            };
            let st = &mut doc.sets[set].settings;
            if let Some(h) = num(&t, "height_scale") {
                st.height_scale = h.clamp(0.0, 0.2);
            }
            if let Some(e) = num(&t, "emissive_intensity") {
                st.emissive_intensity = e.clamp(0.0, 100.0);
            }
            if let Some(a) = t["alpha_mode"].as_str() {
                st.alpha_mode = match a.trim().to_lowercase().as_str() {
                    "opaque" => AlphaMode::Opaque,
                    "alpha test" => AlphaMode::AlphaTest,
                    "alpha blend" => AlphaMode::AlphaBlend,
                    _ => AlphaMode::Auto,
                };
            }
            doc.sets[set].dirty = true;
            doc.modified = true;
            changed.push(format!("{name} settings"));
        }
        let refs: Vec<std::path::PathBuf> = c.refs.iter().map(|r| r.path.clone()).collect();
        let n = c.pieces.len();
        let mut owners: HashMap<usize, Vec<u16>> = HashMap::new();
        self.muse.resolved.clear();
        for p in answer {
            let mut ids: Vec<usize> = match p["pieces"].as_array() {
                Some(a) => a.iter().filter_map(|v| v.as_u64()).map(|v| v as usize).collect(),
                None => p["piece"].as_u64().map(|v| vec![v as usize]).unwrap_or_default(),
            };
            let missing: Vec<String> = ids.iter().filter(|&&i| i >= n).map(|i| i.to_string()).collect();
            if !missing.is_empty() {
                notes.push(format!("{}: there is no piece {} (pieces go from 0 to {}), left out.", p["what"].as_str().unwrap_or("A look"), missing.join(", "), n.saturating_sub(1)));
            }
            ids.retain(|&i| i < n);
            ids.sort_unstable();
            ids.dedup();
            if ids.is_empty() {
                continue;
            }
            let what = p["what"].as_str().unwrap_or("Look").to_string();
            let mut children = Vec::new();
            let mut only = Vec::new();
            let mut splits = Vec::new();
            let mut ext_used = Vec::new();
            let mut layers = p["layers"].as_array().cloned().unwrap_or_default();
            self.muse_sides(&mut layers, &refs, &c.pieces, &ids, notes);
            for l in layers {
                if let Some((mut t, ext, split)) = self.muse_layer(&l, images, downloads, &refs, &c.pieces, &c.surfaces, &ids, notes) {
                    splits.push(split);
                    // A print placed on a surface stays on that surface's
                    // piece (its box also reaches what sits in front of it).
                    let piece = source(&l)["surface"].as_u64().and_then(|k| c.surfaces.get(k as usize)).map(|s| s.piece).filter(|_| t.desc.mask.is_none());
                    if piece.is_some() {
                        let paint = MaskEffect { id: 0, enabled: true, blend: MaskBlend::Normal, opacity: 1.0, kind: MaskEffectKind::Paint };
                        t.desc.mask = Some(MaskDesc { enabled: true, base: 0.0, effects: vec![paint] });
                    }
                    only.push(piece);
                    children.push(t);
                    ext_used.extend(ext);
                }
            }
            if children.is_empty() {
                continue;
            }
            // The pieces leave their looks; emptied folders are freed.
            let mut freed: Vec<(usize, u64)> = Vec::new();
            for look in c.looks.iter_mut() {
                let before = look.pieces.len();
                look.pieces.retain(|i| !ids.contains(i));
                if look.pieces.len() == before {
                    continue;
                }
                let mut kept = Vec::new();
                for &(set, fid) in &look.folders {
                    let keep: Vec<usize> = look.pieces.iter().copied().filter(|&i| c.pieces[i].set == set).collect();
                    if keep.is_empty() {
                        freed.push((set, fid));
                    } else {
                        self.muse_mask(set, fid, &c.pieces, &keep, &mut owners);
                        kept.push((set, fid));
                    }
                }
                look.folders = kept;
            }
            c.looks.retain(|l| !l.pieces.is_empty());
            // The new look, a folder per texture set its pieces are in.
            let mut sets: Vec<usize> = ids.iter().map(|&i| c.pieces[i].set).collect();
            sets.sort_unstable();
            sets.dedup();
            let mut folders = Vec::new();
            for set in sets {
                let paint = MaskEffect { id: 0, enabled: true, blend: MaskBlend::Normal, opacity: 1.0, kind: MaskEffectKind::Paint };
                let mut folder = LayerTree { desc: LayerDesc::new(0, format!("Muse: {what}"), LayerKind::Folder), children: children.clone() };
                folder.desc.mask = Some(MaskDesc { enabled: true, base: 0.0, effects: vec![paint] });
                let doc = self.doc.as_mut().unwrap();
                let mut l = doc.instantiate(&self.engine, set, &folder);
                give_ids(doc, &mut l);
                let id = l.desc.id;
                let print_ids: Vec<(u64, usize)> = l.children.iter().zip(&only).filter_map(|(ch, p)| p.map(|p| (ch.desc.id, p))).collect();
                let split_ids: Vec<(u64, Vec<(usize, Vec<u32>)>)> = l.children.iter().zip(&splits).flat_map(|(ch, sp)| ch.children.iter().zip(sp.iter()).map(|(cc, isl)| (cc.desc.id, isl.clone()))).collect();
                // In the place of a freed folder of this set; otherwise on top
                // of the whole stack, never inside the selected folder (layers
                // above it would cover Muse's work).
                let mut l = Some(l);
                let s = &mut doc.sets[set];
                if let Some(k) = freed.iter().position(|f| f.0 == set) {
                    let (_, old) = freed.remove(k);
                    if let Some((list, at)) = locate_mut(&mut s.layers, old) {
                        list[at] = l.take().unwrap();
                        s.selected = Some(id);
                        s.extra.clear();
                    }
                }
                if let Some(l) = l {
                    s.selected = None;
                    doc.insert_layer(set, l);
                }
                let s = &mut doc.sets[set];
                for &e in &ext_used {
                    s.settings.ext_channels[e] = true;
                }
                let keep: Vec<usize> = ids.iter().copied().filter(|&i| c.pieces[i].set == set).collect();
                self.muse_mask(set, id, &c.pieces, &keep, &mut owners);
                for (pid, piece) in print_ids {
                    self.muse_mask(set, pid, &c.pieces, &[piece], &mut owners);
                }
                for (sid, isl) in split_ids {
                    self.muse_mask_islands(set, sid, &c.pieces, &isl);
                }
                folders.push((set, id));
            }
            // Freed folders nothing took.
            if let Some(doc) = self.doc.as_mut() {
                for (set, old) in freed {
                    let s = &mut doc.sets[set];
                    if let Some((list, at)) = locate_mut(&mut s.layers, old) {
                        list.remove(at);
                    }
                    if s.selected == Some(old) {
                        s.selected = None;
                    }
                    s.dirty = true;
                }
            }
            // Each ambientCG layer remembers the scan it got ("got"): Muse
            // sees its stacks with the next message and can judge the scan.
            let mut layers = p["layers"].clone();
            for l in layers.as_array_mut().into_iter().flatten() {
                if let Some(got) = l["name"].as_str().and_then(|n| self.muse.resolved.get(n)).cloned() {
                    let s = if l["source"].is_object() { &mut l["source"] } else { &mut *l };
                    for (k, v) in got {
                        s[k] = v;
                    }
                }
                if let Some(m) = l["source"].as_object_mut() {
                    m.remove("got");
                }
                if let Some(m) = l.as_object_mut() {
                    m.remove("got");
                }
                let Some((_, label)) = source(l)["ambientcg"].as_str().and_then(|q| downloads.get(q)) else { continue };
                let label = Value::String(label.clone());
                if l["source"].is_object() {
                    l["source"]["got"] = label;
                } else {
                    l["got"] = label;
                }
            }
            c.looks.push(Look { pieces: ids, what: what.clone(), layers, folders });
            changed.push(what);
        }
        let unbaked = c.pieces.iter().any(|p| self.doc.as_ref().is_some_and(|d| !d.sets[p.set].baked));
        if unbaked && self.bake_job.is_none() {
            notes.push("Bake the mesh maps (Bake, top right): the wear and dirt masks read them.".into());
        }
        changed
    }

    /// Masks a Muse folder to `keep` (pieces of `set`): its paint mask gets
    /// a new texture (the undo history keeps the old one).
    fn muse_mask(&mut self, set: usize, folder: u64, pieces: &[pieces::Piece], keep: &[usize], owners: &mut HashMap<usize, Vec<u16>>) {
        let Some(doc) = self.doc.as_mut() else { return };
        let res = doc.sets[set].resolution();
        let own = owners.entry(set).or_insert_with(|| pieces::owners(&doc.mesh, pieces, set, res));
        let data = crate::engine::texture::f32_to_f16_bytes(&pieces::mask(own, keep));
        let Some(layer) = find_mut(&mut doc.sets[set].layers, folder) else { return };
        let Some(eid) = layer.desc.mask.as_ref().and_then(|m| m.effects.iter().find(|e| matches!(e.kind, MaskEffectKind::Paint))).map(|e| e.id) else { return };
        let tex = full_mask_paint(&self.engine.device, res);
        tex.write_bytes(&self.engine.queue, &data);
        layer.mask_paint.insert(eid, tex);
        layer.rev = next_rev();
        doc.sets[set].dirty = true;
        doc.modified = true;
    }
}

/// `--museview <project> <out.png>`: saves the views Muse gets of a
/// project (all its parts) and prints the part facts.
pub fn view_shot(project: &std::path::Path, out: &std::path::Path) -> anyhow::Result<()> {
    let engine = crate::engine::Engine::headless()?;
    let (doc, _) = crate::project::load(&engine, project)?;
    let all = parts(&doc);
    let scope: Vec<usize> = (0..all.len()).collect();
    let pieces = pieces::find(&doc.mesh, &part_list(&doc, &all, &scope));
    let (img, info) = look_at(&doc.mesh, &pieces);
    img.save(out)?;
    println!("{info}");
    Ok(())
}
