//! Packshot studio for perfume and cosmetics: the lights of a product
//! photographer's set (strip softboxes, softboxes, octaboxes, diffusion
//! scrims, white bounce cards, black flags), a seamless background with a
//! falloff, shadows and an optional glossy (plexi) floor, and a camera with
//! a lens in millimeters and a frame format. Saved with the project; the
//! path tracer renders it (`engine/pathtrace.rs`, `shaders/pathtrace.wgsl`).
//!
//! Sizes and distances are in object sizes (half the diagonal of the
//! model's box), so a kit lights any product the same way. A light's power
//! is the light it puts on the subject: at 1, a white surface facing it
//! shows white, wherever the light stands (falloff from there on is
//! physical: inverse square from the real position).

use glam::Vec3;
use serde::{Deserialize, Deserializer, Serialize};

/// Light slots of the path tracer.
pub const MAX_LIGHTS: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LightKind {
    /// Rectangular softbox: the main light of a product set.
    Softbox,
    /// Tall narrow softbox: the long highlights down a bottle's sides.
    Strip,
    /// Round softbox (its width is the diameter), round highlights.
    Octabox,
    /// Large diffusion panel lit from behind: the softest light, wide
    /// graded highlights on glass and metal.
    Scrim,
    /// White card: bounces the other lights back (its power is how much),
    /// for gentle fill and white edges.
    Card,
    /// Black card (negative fill): a dark shape in the reflections of
    /// glossy products, for their edges.
    Flag,
    /// A small hard source far away (a fresnel or bare head): crisp,
    /// sun-like shadows.
    Spot,
}

impl LightKind {
    pub const ALL: [LightKind; 7] = [LightKind::Strip, LightKind::Softbox, LightKind::Octabox, LightKind::Scrim, LightKind::Spot, LightKind::Card, LightKind::Flag];
    pub fn name(self) -> &'static str {
        match self {
            LightKind::Softbox => "Softbox",
            LightKind::Strip => "Strip softbox",
            LightKind::Octabox => "Octabox",
            LightKind::Scrim => "Diffusion scrim",
            LightKind::Card => "White bounce card",
            LightKind::Flag => "Black flag",
            LightKind::Spot => "Hard light (sun)",
        }
    }
    /// The kind's usual size (width, height) and edge softness.
    pub fn shape(self) -> (f32, f32, f32) {
        match self {
            LightKind::Softbox => (1.6, 1.2, 0.2),
            LightKind::Strip => (0.45, 3.2, 0.6),
            LightKind::Octabox => (2.0, 2.0, 0.3),
            LightKind::Scrim => (2.4, 2.6, 0.5),
            LightKind::Card => (1.2, 2.2, 0.0),
            LightKind::Flag => (0.6, 3.0, 0.0),
            LightKind::Spot => (0.4, 0.4, 0.0),
        }
    }
    /// Lights its own light (cards bounce, flags block).
    pub fn emits(self) -> bool {
        !matches!(self, LightKind::Card | LightKind::Flag)
    }
}

/// A light's kind, older kinds (the gobo projector) read as a softbox.
fn kind_or_softbox<'de, D: Deserializer<'de>>(d: D) -> Result<LightKind, D::Error> {
    let s = String::deserialize(d)?;
    Ok(LightKind::ALL.into_iter().find(|k| format!("{k:?}") == s).unwrap_or(LightKind::Softbox))
}

/// The picture's proportions.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Format {
    /// The whole view.
    View,
    Square,
    Portrait45,
    Portrait23,
    Story916,
    Classic43,
    Landscape32,
    Wide169,
}

impl Format {
    pub const ALL: [Format; 8] = [Format::View, Format::Square, Format::Portrait45, Format::Portrait23, Format::Story916, Format::Classic43, Format::Landscape32, Format::Wide169];
    pub fn name(self) -> &'static str {
        match self {
            Format::View => "The view",
            Format::Square => "Square 1:1",
            Format::Portrait45 => "Portrait 4:5",
            Format::Portrait23 => "Portrait 2:3",
            Format::Story916 => "Vertical 9:16",
            Format::Classic43 => "Landscape 4:3",
            Format::Landscape32 => "Landscape 3:2",
            Format::Wide169 => "Wide 16:9",
        }
    }
    /// Width / height (None: the view's).
    pub fn aspect(self) -> Option<f32> {
        Some(match self {
            Format::View => return None,
            Format::Square => 1.0,
            Format::Portrait45 => 4.0 / 5.0,
            Format::Portrait23 => 2.0 / 3.0,
            Format::Story916 => 9.0 / 16.0,
            Format::Classic43 => 4.0 / 3.0,
            Format::Landscape32 => 3.0 / 2.0,
            Format::Wide169 => 16.0 / 9.0,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StudioLight {
    pub name: String,
    pub on: bool,
    #[serde(deserialize_with = "kind_or_softbox")]
    pub kind: LightKind,
    /// sRGB color of the light (a gel; a card's own color).
    pub color: [f32; 3],
    /// Light on the subject (1: a white surface facing it shows white). A
    /// card: how much of the light reaching it it bounces (1: all).
    pub power: f32,
    /// Degrees around the subject: 0 from the camera, 90 its right, 180
    /// behind the subject.
    pub azimuth: f32,
    /// Degrees above the subject's center.
    pub elevation: f32,
    /// From the subject's center, in object sizes.
    pub distance: f32,
    /// Width and height (octabox: diameter), in object sizes.
    pub width: f32,
    pub height: f32,
    /// Honeycomb grid: the beam's angle in degrees (0: none).
    pub grid: f32,
    /// Soft edges: 0 even to the edges (crisp highlights), 1 bright in the
    /// middle fading out to the edges (graded highlights).
    pub feather: f32,
    /// Brighter at the top (+) or at the bottom (-): a graded highlight
    /// down the product.
    pub gradient: f32,
}

impl Default for StudioLight {
    fn default() -> Self {
        let (width, height, feather) = LightKind::Strip.shape();
        Self { name: "Light".into(), on: true, kind: LightKind::Strip, color: [1.0; 3], power: 1.0, azimuth: -100.0, elevation: 8.0, distance: 2.2, width, height, grid: 0.0, feather, gradient: 0.0 }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Studio {
    pub lights: Vec<StudioLight>,
    /// sRGB color of the seamless background, shown as it is behind the
    /// product (darker away from it with `falloff`).
    pub background: [f32; 3],
    /// The background darkens away from the product: 0 even, 1 to black.
    pub falloff: f32,
    /// The lit pool behind the product, in frame heights.
    pub pool: f32,
    /// The ground catches the subject's shadows.
    pub ground_shadows: bool,
    pub shadow_opacity: f32,
    /// A glossy floor (black or white plexi): how much it reflects the
    /// product (0: a matte seamless).
    pub floor_reflection: f32,
    /// The reflection fades out over this height of the product (object
    /// sizes; 0: no fade).
    pub reflection_fade: f32,
    /// Light bounced by the studio (the background's color) onto the
    /// subject, where no light points.
    pub room: f32,
    /// The environment image's light (0: the studio's lights only).
    pub environment: f32,
    /// Lights stand around the camera (azimuth 0 is the camera's side):
    /// orbiting keeps the lighting of the shot.
    pub follow_camera: bool,
    /// Lens focal length (full frame 36 x 24 mm: the frame's long side
    /// spans 36 mm).
    pub lens_mm: f32,
    pub format: Format,
    /// Rendered image: pixels on the long side, samples per pixel.
    pub out_long: u32,
    pub out_samples: u32,
    /// The camera stays where the shot was set (`shot`): orbit, pan and
    /// zoom do nothing.
    pub lock_camera: bool,
    pub shot: Option<crate::camera::OrbitCamera>,
    /// The shots saved for this product (view, lens and format).
    pub angles: Vec<CameraAngle>,
}

/// A saved shot of the studio.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CameraAngle {
    pub name: String,
    pub camera: crate::camera::OrbitCamera,
    pub lens_mm: f32,
    pub format: Format,
}

impl Default for CameraAngle {
    fn default() -> Self {
        Self { name: "Angle".into(), camera: Default::default(), lens_mm: 85.0, format: Format::Square }
    }
}

/// The user's lighting presets: `<library>/lighting/<name>.json`.
pub fn presets_dir() -> std::path::PathBuf {
    crate::library::library_dir().join("lighting")
}

/// The user's lighting presets by name.
pub fn user_presets() -> Vec<(String, std::path::PathBuf)> {
    let mut v: Vec<(String, std::path::PathBuf)> = std::fs::read_dir(presets_dir())
        .map(|r| r.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "json")).collect::<Vec<_>>())
        .unwrap_or_default()
        .into_iter()
        .filter_map(|p| Some((p.file_stem()?.to_string_lossy().into_owned(), p)))
        .collect();
    v.sort_by_key(|(n, _)| n.to_lowercase());
    v
}

/// A name usable as a file name (Windows' reserved characters removed).
pub fn file_safe(name: &str) -> String {
    let s: String = name.chars().map(|c| if "<>:\"/\\|?*".contains(c) || c.is_control() { ' ' } else { c }).collect();
    let s = s.trim().trim_end_matches('.').to_string();
    if s.is_empty() { "Untitled".into() } else { s }
}

/// Saves `st`'s lighting (lights, background, floor, room) as a preset.
pub fn save_preset(name: &str, st: &Studio) -> anyhow::Result<std::path::PathBuf> {
    let mut s = st.clone();
    (s.angles, s.shot, s.lock_camera) = (Vec::new(), None, false);
    std::fs::create_dir_all(presets_dir())?;
    let p = presets_dir().join(format!("{}.json", file_safe(name)));
    std::fs::write(&p, serde_json::to_vec_pretty(&s)?)?;
    Ok(p)
}

pub fn load_preset(p: &std::path::Path) -> Option<Studio> {
    serde_json::from_slice(&std::fs::read(p).ok()?).ok()
}

/// Perfume and cosmetics packshot setups (`Studio::setup`), each the
/// lighting of a kind of picture luxury brands publish.
pub const SETUPS: [&str; 8] = [
    "Pure white e-commerce, side strips (Acqua di Parma look)",
    "Light grey, soft side shadow (Dior beauty look)",
    "Warm stone wall, window light (Dior Collection Privee look)",
    "White, backlit glass and juice (fragrance e-commerce)",
    "Black, rim strips and dark plexi (men's fragrance look)",
    "Mediterranean hard sun, long shadow (campaign look)",
    "Skincare jars and creams, soft top light",
    "White plexi with a soft reflection",
];

impl Default for Studio {
    fn default() -> Self {
        let mut s = Self {
            lights: Vec::new(),
            background: [1.0; 3],
            falloff: 0.0,
            pool: 1.5,
            ground_shadows: true,
            shadow_opacity: 0.5,
            floor_reflection: 0.0,
            reflection_fade: 1.0,
            room: 0.1,
            environment: 0.0,
            follow_camera: true,
            lens_mm: 85.0,
            format: Format::Square,
            out_long: 2048,
            out_samples: 512,
            lock_camera: false,
            shot: None,
            angles: Vec::new(),
        };
        s.setup(0);
        s
    }
}

impl Studio {
    /// Replaces the lights, background, floor and room with setup `i` of
    /// `SETUPS`. Sizes are in object sizes: strips taller than the
    /// product, scrims near enough to wrap it, flags just out of frame.
    pub fn setup(&mut self, i: usize) {
        let mk = |kind: LightKind, name: &str, power: f32, az: f32, el: f32, d: f32, w: f32, h: f32| {
            let (_, _, feather) = kind.shape();
            StudioLight { name: name.into(), kind, power, azimuth: az, elevation: el, distance: d, width: w, height: h, feather, ..Default::default() }
        };
        // Strips a little taller than the product, graded toward the top:
        // the long highlights down each side of a bottle.
        let strip = |name: &str, p: f32, az: f32, d: f32| StudioLight { gradient: 0.25, ..mk(LightKind::Strip, name, p, az, 4.0, d, 0.45, 3.2) };
        let flag = |name: &str, az: f32, d: f32| mk(LightKind::Flag, name, 1.0, az, 0.0, d, 0.5, 3.2);
        let white = [1.0; 3];
        // lights, background, falloff, pool, shadow, reflection, fade, room
        let (lights, bg, falloff, pool, shadow, refl, fade, room) = match i {
            // Light grey seamless (about 236, 237, 240), a medium soft key
            // from high on the right drawing a soft-edged shadow to the
            // left, a big front fill keeping the product evenly lit, a
            // strip for the metal cap's long highlight, a card on the
            // right.
            1 => (
                vec![
                    StudioLight { feather: 0.1, color: [1.0, 0.99, 0.97], ..mk(LightKind::Softbox, "Key softbox right", 0.95, 70.0, 50.0, 3.5, 0.9, 0.9) },
                    StudioLight { feather: 0.6, ..mk(LightKind::Scrim, "Front fill scrim", 0.45, -10.0, 15.0, 4.5, 3.0, 3.0) },
                    strip("Strip left", 0.5, -95.0, 2.2),
                    mk(LightKind::Card, "Card right", 0.6, 100.0, 5.0, 2.2, 1.4, 2.4),
                ],
                [0.925, 0.93, 0.94],
                0.03,
                2.0,
                0.4,
                0.0,
                1.0,
                0.18,
            ),
            // Warm off-white plaster (about 232, 226, 217) darkening gently
            // to the corners, a big soft window from the front left, strips
            // for the black cap's side highlights, a gridded strip above
            // for its rim, black flags for the glass's dark edges and a
            // back scrim through the juice.
            2 => (
                vec![
                    StudioLight { color: [1.0, 0.97, 0.92], feather: 0.5, ..mk(LightKind::Scrim, "Window scrim left", 0.85, -35.0, 30.0, 3.2, 2.6, 3.0) },
                    strip("Strip left", 0.4, -100.0, 2.2),
                    StudioLight { color: [1.0, 0.97, 0.93], ..strip("Strip right", 0.3, 100.0, 2.2) },
                    StudioLight { feather: 0.8, color: [1.0, 0.97, 0.94], ..mk(LightKind::Scrim, "Back scrim", 0.5, 180.0, 5.0, 2.6, 1.4, 2.2) },
                    StudioLight { grid: 40.0, ..mk(LightKind::Strip, "Top strip", 0.12, 0.0, 75.0, 2.6, 2.2, 0.5) },
                    flag("Flag left", -28.0, 2.6),
                    flag("Flag right", 28.0, 2.6),
                ],
                [0.91, 0.885, 0.85],
                0.2,
                1.4,
                0.35,
                0.0,
                1.0,
                0.15,
            ),
            // Clear glass and colored juice glowing from a scrim behind,
            // even side strips outlining the glass, a gentle front fill,
            // flags to keep the edges drawn.
            3 => (
                vec![
                    StudioLight { feather: 0.75, ..mk(LightKind::Scrim, "Back scrim", 1.5, 180.0, 4.0, 2.4, 1.2, 2.4) },
                    strip("Strip left", 1.2, -100.0, 2.0),
                    strip("Strip right", 1.2, 100.0, 2.0),
                    StudioLight { feather: 0.7, ..mk(LightKind::Scrim, "Front fill scrim", 0.45, 0.0, 12.0, 4.5, 2.4, 2.8) },
                    mk(LightKind::Octabox, "Top octabox", 0.3, 0.0, 80.0, 2.6, 1.8, 1.8),
                    flag("Flag left", -38.0, 2.4),
                    flag("Flag right", 38.0, 2.4),
                ],
                white,
                0.08,
                1.5,
                0.3,
                0.0,
                1.0,
                0.08,
            ),
            // Near black, warm rim strips from behind graded to the top,
            // a gridded strip above for the cap, a faint front scrim to
            // read the label, a dark plexi reflection.
            4 => (
                vec![
                    StudioLight { gradient: 0.6, feather: 0.75, color: [1.0, 0.94, 0.85], ..strip("Rim strip left", 1.5, -130.0, 2.0) },
                    StudioLight { gradient: 0.6, feather: 0.75, color: [1.0, 0.94, 0.85], ..strip("Rim strip right", 1.5, 130.0, 2.0) },
                    StudioLight { grid: 45.0, ..mk(LightKind::Strip, "Top grid strip", 0.6, 0.0, 72.0, 2.6, 2.2, 0.5) },
                    StudioLight { feather: 0.8, ..mk(LightKind::Scrim, "Front scrim", 0.16, 0.0, 10.0, 4.5, 2.0, 2.6) },
                ],
                [0.04, 0.04, 0.045],
                0.7,
                1.2,
                0.8,
                0.45,
                1.0,
                0.02,
            ),
            // A warm hard sun from the back left at a low angle: a long
            // crisp shadow toward the camera's right, a cool sky fill.
            5 => (
                vec![
                    StudioLight { color: [1.0, 0.94, 0.84], ..mk(LightKind::Spot, "Sun", 1.3, -125.0, 30.0, 10.0, 0.35, 0.35) },
                    StudioLight { color: [0.93, 0.96, 1.0], feather: 0.6, ..mk(LightKind::Scrim, "Sky fill", 0.45, 20.0, 25.0, 4.5, 3.0, 3.0) },
                    mk(LightKind::Card, "Bounce card", 0.7, 30.0, 0.0, 2.4, 1.6, 2.4),
                ],
                [0.96, 0.94, 0.9],
                0.1,
                2.0,
                0.6,
                0.0,
                1.0,
                0.15,
            ),
            // Jars and creams seen from slightly above: a large scrim
            // overhead for the lid's even highlight, a front softbox, white
            // cards wrapping the sides, a small contact shadow.
            6 => (
                vec![
                    StudioLight { feather: 0.5, ..mk(LightKind::Scrim, "Top scrim", 0.85, 0.0, 72.0, 2.6, 2.8, 2.8) },
                    mk(LightKind::Softbox, "Front softbox", 0.4, -20.0, 22.0, 3.5, 2.2, 1.6),
                    mk(LightKind::Card, "Card left", 0.85, -85.0, 10.0, 2.0, 1.4, 1.8),
                    mk(LightKind::Card, "Card right", 0.85, 85.0, 10.0, 2.0, 1.4, 1.8),
                ],
                [0.95, 0.95, 0.955],
                0.06,
                1.5,
                0.45,
                0.0,
                1.0,
                0.18,
            ),
            // White acrylic: the product mirrored below, fading, side
            // strips, a top scrim, a front fill.
            7 => (
                vec![
                    strip("Strip left", 1.4, -105.0, 2.0),
                    strip("Strip right", 1.4, 105.0, 2.0),
                    mk(LightKind::Scrim, "Top scrim", 0.45, 0.0, 78.0, 2.6, 2.4, 1.6),
                    StudioLight { feather: 0.6, ..mk(LightKind::Scrim, "Front fill scrim", 0.45, -10.0, 12.0, 4.5, 2.6, 2.8) },
                    flag("Flag left", -38.0, 2.4),
                    flag("Flag right", 38.0, 2.4),
                ],
                white,
                0.05,
                1.5,
                0.25,
                0.3,
                0.8,
                0.1,
            ),
            // Pure white seamless (255), fitted on an Acqua di Parma photo
            // (the product's colors across it and the ground shadow): a
            // medium softbox low on the front left (the soft shadow trails
            // to the right), a very wide close scrim slightly behind on the
            // left whose broad sheen wraps to the edge, a front fill
            // straight on (its reflection hides behind the label, the right
            // side stays a dim saturated color), a back scrim the shoulders
            // and the top reflect, a strip far behind on the right for a
            // thin edge line, a top strip on the cap.
            _ => (
                vec![
                    StudioLight { feather: 0.3, ..mk(LightKind::Softbox, "Key softbox left", 0.83, -60.0, 12.0, 4.5, 1.4, 1.4) },
                    StudioLight { feather: 0.7, ..mk(LightKind::Scrim, "Sheen scrim left", 0.9, -100.0, 10.0, 2.0, 4.0, 3.6) },
                    StudioLight { feather: 0.6, ..mk(LightKind::Scrim, "Front fill scrim", 0.5, 5.0, 20.0, 5.0, 2.6, 2.8) },
                    StudioLight { feather: 0.8, ..mk(LightKind::Scrim, "Back scrim", 0.45, 180.0, 15.0, 4.0, 3.0, 2.0) },
                    strip("Strip right", 0.2, 147.0, 2.2),
                    StudioLight { grid: 40.0, ..mk(LightKind::Strip, "Top strip", 0.23, 0.0, 75.0, 2.6, 2.2, 0.5) },
                ],
                white,
                0.0,
                1.5,
                0.4,
                0.0,
                1.0,
                0.12,
            ),
        };
        self.lights = lights;
        self.background = bg;
        self.falloff = falloff;
        self.pool = pool;
        self.ground_shadows = true;
        self.shadow_opacity = shadow;
        self.floor_reflection = refl;
        self.reflection_fade = fade;
        self.room = room;
        self.environment = 0.0;
    }

    /// The lighting of `from` (lights, background, floor, room), the
    /// camera and output kept.
    pub fn take_lighting(&mut self, from: &Studio) {
        let keep = (self.follow_camera, self.lens_mm, self.format, self.out_long, self.out_samples, self.lock_camera, self.shot, std::mem::take(&mut self.angles));
        *self = from.clone();
        (self.follow_camera, self.lens_mm, self.format, self.out_long, self.out_samples, self.lock_camera, self.shot, self.angles) = keep;
    }
}

/// What a packshot photo shows of its light, measured: the background at
/// the corners, the ground shadow beside the product (which side, how
/// dark, how far), which side of the product is brighter. For Claude, who
/// reads it more reliably than by eye, and for its check renders.
pub fn photo_light(img: &image::DynamicImage) -> Option<String> {
    let img = img.thumbnail(512, 512).to_rgb8();
    let (w, h) = img.dimensions();
    if w < 32 || h < 32 {
        return None;
    }
    let lum = |p: [u8; 3]| 0.2126 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.0722 * p[2] as f32;
    let at = |x: u32, y: u32| img.get_pixel(x, y).0;
    let bg = backdrop(&img);
    let bgl = lum(bg).max(1.0);
    let mut out = format!("background #{:02X}{:02X}{:02X} at the corners", bg[0], bg[1], bg[2]);
    let Some((x0, x1, y0, y1)) = product_box(&img, bg) else { return Some(out) };
    let far = |p: [u8; 3]| (0..3).map(|c| (p[c] as i32 - bg[c] as i32).abs()).sum::<i32>() > 90;
    let (ow, oh) = ((x1 - x0 + 1) as f32, (y1 - y0 + 1) as f32);
    out += &format!("; the product spans {:.0}% to {:.0}% across and {:.0}% to {:.0}% down the picture", x0 as f32 / w as f32 * 100.0, x1 as f32 / w as f32 * 100.0, y0 as f32 / h as f32 * 100.0, y1 as f32 / h as f32 * 100.0);
    // The background toward the corners against beside the product.
    let beside: Vec<f32> = [x0.saturating_sub((ow * 0.4) as u32), (x1 + (ow * 0.4) as u32).min(w - 1)].iter().map(|&x| lum(at(x, (y0 + y1) / 2))).filter(|l| (l - bgl).abs() < 60.0).collect();
    if !beside.is_empty() {
        let b = beside.iter().sum::<f32>() / beside.len() as f32;
        let d = (1.0 - bgl / b.max(1.0)) * 100.0;
        if d > 2.0 {
            out += &format!("; the background is {d:.0}% darker at the corners than beside the product (falloff)");
        } else {
            out += "; the background is even";
        }
    }
    // The ground shadow: the band at the product's foot, left and right of
    // it (columns away from the product, darker than the background).
    let (b0, b1) = (y1.saturating_sub((oh * 0.08) as u32), (y1 + (oh * 0.04) as u32).min(h - 1));
    let side = |xs: Vec<u32>| -> (f32, f32) {
        let dark: Vec<f32> = xs.iter().map(|&x| (b0..=b1).map(|y| (1.0 - lum(at(x, y)) / bgl).max(0.0)).sum::<f32>() / (b1 - b0 + 1) as f32).collect();
        let peak = dark.windows(3).map(|v| v.iter().sum::<f32>() / 3.0).fold(0.0f32, f32::max);
        let reach = dark.iter().rposition(|&d| d > 0.03).map_or(0.0, |i| (i + 1) as f32 / ow);
        (peak, reach)
    };
    let span = (ow * 2.0) as u32;
    let right = side((x1 + 2..(x1 + 2 + span).min(w)).collect());
    let left = side((x0.saturating_sub(span)..x0.saturating_sub(1)).rev().collect());
    let say = |(d, r): (f32, f32), name: &str| if d > 0.03 { format!("{name} up to {:.0}% darker than the background, reaching {r:.1} product widths", d * 100.0) } else { format!("none on the {name}") };
    out += &format!("; ground shadow at its foot: {}, {}", say(right, "right"), say(left, "left"));
    // A shadow along the product's sides (on a wall behind it, or on the
    // table of a flat lay).
    let (m0, m1) = (y0 + (oh * 0.3) as u32, y0 + (oh * 0.9) as u32);
    let along = |xs: Vec<u32>| -> f32 {
        let dark: Vec<f32> = xs.iter().map(|&x| (m0..m1).map(|y| (1.0 - lum(at(x, y)) / bgl).max(0.0)).sum::<f32>() / (m1 - m0).max(1) as f32).collect();
        dark.windows(3).map(|v| v.iter().sum::<f32>() / 3.0).fold(0.0f32, f32::max)
    };
    let span = (ow * 0.6) as u32;
    let (r, l) = (along((x1 + 2..(x1 + 2 + span).min(w)).collect()), along((x0.saturating_sub(span)..x0.saturating_sub(1)).collect()));
    if r.max(l) > 0.03 {
        let (side, d) = if r >= l { ("right", r) } else { ("left", l) };
        out += &format!("; a shadow along the product's {side} side, up to {:.0}% darker than the background (cast on the surface behind or under it: the key is on the other side)", d * 100.0);
    }
    // Which side of the product the light comes from.
    let band = |a: u32, b: u32| -> Option<f32> {
        let v: Vec<f32> = (y0 + (oh * 0.3) as u32..y0 + (oh * 0.8) as u32).flat_map(|y| (a..b).map(move |x| (x, y))).map(|(x, y)| at(x, y)).filter(|p| far(*p)).map(lum).collect();
        (!v.is_empty()).then(|| v.iter().sum::<f32>() / v.len() as f32)
    };
    let q = (ow * 0.2) as u32;
    if let (Some(l), Some(r)) = (band(x0, x0 + q), band(x1 - q, x1)) {
        let d = (l / r.max(1.0) - 1.0) * 100.0;
        out += &if d.abs() < 5.0 { "; the product's left and right sides are about as bright".to_string() } else if d > 0.0 { format!("; the product's left side is {d:.0}% brighter than its right side") } else { format!("; the product's right side is {:.0}% brighter than its left side", -d) };
    }
    if let Some(p) = photo_profile_of(&img) {
        out += &format!("; {}", profile_words(&p));
    }
    Some(out)
}

/// A packshot's background: the median of its four corners.
fn backdrop(img: &image::RgbImage) -> [u8; 3] {
    let (w, h) = img.dimensions();
    let s = (w.min(h) / 20).max(2);
    let mut px = Vec::new();
    for (x0, y0) in [(0, 0), (w - s, 0), (0, h - s), (w - s, h - s)] {
        for y in y0..y0 + s {
            for x in x0..x0 + s {
                px.push(img.get_pixel(x, y).0);
            }
        }
    }
    let med = |c: usize| {
        let mut v: Vec<u8> = px.iter().map(|p| p[c]).collect();
        v.sort_unstable();
        v[v.len() / 2]
    };
    [med(0), med(1), med(2)]
}

/// The product's box in a packshot (x0, x1, y0, y1): what stands far from
/// the background, its width from its upper part (a dark shadow at the
/// foot is not it).
fn product_box(img: &image::RgbImage, bg: [u8; 3]) -> Option<(u32, u32, u32, u32)> {
    let (w, h) = img.dimensions();
    let at = |x: u32, y: u32| img.get_pixel(x, y).0;
    let far = |p: [u8; 3]| (0..3).map(|c| (p[c] as i32 - bg[c] as i32).abs()).sum::<i32>() > 90;
    let rows: Vec<u32> = (0..h).map(|y| (0..w).filter(|&x| far(at(x, y))).count() as u32).collect();
    let (y0, y1) = (rows.iter().position(|&n| n > w / 60)? as u32, rows.iter().rposition(|&n| n > w / 60)? as u32);
    let top = y0 + ((y1 - y0) as f32 * 0.8) as u32;
    let cols: Vec<u32> = (0..w).map(|x| (y0..=top).filter(|&y| far(at(x, y))).count() as u32).collect();
    let n = (top - y0) / 30;
    let (x0, x1) = (cols.iter().position(|&c| c > n)? as u32, cols.iter().rposition(|&c| c > n)? as u32);
    (x1 > x0 + 4 && y1 > y0 + 4).then_some((x0, x1, y0, y1))
}

/// The heights of the profile's bands (fractions of the product's height
/// from its top).
const PROFILE_BANDS: [(f32, f32); 3] = [(0.12, 0.35), (0.42, 0.58), (0.62, 0.88)];

/// A packshot's light as numbers, to compare a render with a photo: the
/// product's color across its width at three heights (16 steps), the
/// ground's darkening beside its foot (8 steps right, 4 left).
pub type Profile = (Vec<f32>, Vec<f32>);

pub fn photo_profile(img: &image::DynamicImage) -> Option<Profile> {
    photo_profile_of(&img.thumbnail(512, 512).to_rgb8())
}

fn photo_profile_of(img: &image::RgbImage) -> Option<Profile> {
    let (w, h) = img.dimensions();
    if w < 32 || h < 32 {
        return None;
    }
    let bg = backdrop(img);
    let (x0, x1, y0, y1) = product_box(img, bg)?;
    let (ow, oh) = ((x1 - x0) as f32, (y1 - y0) as f32);
    let mean = |xa: f32, xb: f32, ya: f32, yb: f32| -> Option<[f32; 3]> {
        let (xa, xb) = (xa.max(0.0) as u32, xb.min(w as f32 - 1.0).max(0.0) as u32);
        let (ya, yb) = (ya.max(0.0) as u32, yb.min(h as f32 - 1.0).max(0.0) as u32);
        if xb < xa || yb < ya {
            return None;
        }
        let mut s = [0.0f32; 3];
        let mut n = 0.0;
        for y in ya..=yb {
            for x in xa..=xb {
                let p = img.get_pixel(x, y).0;
                for c in 0..3 {
                    s[c] += p[c] as f32 / 255.0;
                }
                n += 1.0;
            }
        }
        Some(s.map(|v| v / n))
    };
    let mut colors = Vec::new();
    for (g0, g1) in PROFILE_BANDS {
        for k in 0..16 {
            let c = mean(x0 as f32 + (k as f32 + 0.15) / 16.0 * ow, x0 as f32 + (k as f32 + 0.85) / 16.0 * ow, y0 as f32 + g0 * oh, y0 as f32 + g1 * oh)?;
            colors.extend(c);
        }
    }
    let bgl = (bg.iter().map(|&v| v as f32).sum::<f32>() / 765.0).max(0.01);
    let (b0, b1) = (y1 as f32 - 0.06 * oh, y1 as f32 + 0.03 * oh);
    let dark = |xa: f32, xb: f32| mean(xa, xb, b0, b1).map_or(0.0, |c| (1.0 - (c[0] + c[1] + c[2]) / 3.0 / bgl).max(0.0));
    let mut ground: Vec<f32> = (0..8).map(|k| dark(x1 as f32 + 2.0 + k as f32 / 8.0 * ow, x1 as f32 + 2.0 + (k + 1) as f32 / 8.0 * ow)).collect();
    ground.extend((0..4).map(|k| dark(x0 as f32 - 2.0 - (k + 1) as f32 / 8.0 * ow, x0 as f32 - 2.0 - k as f32 / 8.0 * ow)));
    Some((colors, ground))
}

/// How far two packshots' light is apart (0: the same): the product's
/// colors across it, the ground shadow weighted three times.
pub fn profile_distance(a: &Profile, b: &Profile) -> f32 {
    let d = |x: &[f32], y: &[f32]| x.iter().zip(y).map(|(p, q)| (p - q).abs()).sum::<f32>() / x.len().max(1) as f32;
    d(&a.0, &b.0) + 3.0 * d(&a.1, &b.1)
}

/// The product's brightness across it, in words for Claude: three heights,
/// left to right in 8 steps (0 black, 255 white).
fn profile_words(p: &Profile) -> String {
    let row = |band: usize| -> String {
        (0..8)
            .map(|k| {
                let i = (band * 16 + k * 2) * 3;
                format!("{:.0}", p.0[i..i + 6].iter().sum::<f32>() / 6.0 * 255.0)
            })
            .collect::<Vec<_>>()
            .join(" ")
    };
    format!("the product's brightness left to right (8 steps, 0 to 255): upper part {}, middle {}, lower part {}", row(0), row(1), row(2))
}

/// How much of the frame's height (or width, whichever binds) the product
/// fills once centered: what perfume and cosmetics packshots show.
pub const FRAME_FILL: f32 = 0.7;

/// Centers `points` (the model) in the frame and sets the distance so
/// they fill `fill` of it, from the camera's own yaw and pitch: `tan_y` is
/// tan of half the frame's vertical angle, `aspect` its width / height.
pub fn fit_camera(cam: &mut crate::camera::OrbitCamera, points: &[Vec3], tan_y: f32, aspect: f32, fill: f32) {
    if points.is_empty() {
        return;
    }
    let (min, max) = points.iter().fold((Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)), |(a, b), p| (a.min(*p), b.max(*p)));
    let size = ((max - min).length() * 0.5).max(1e-4);
    let (sy, cy) = cam.yaw.sin_cos();
    let (sp, cp) = cam.pitch.sin_cos();
    let fwd = -Vec3::new(cp * sy, sp, cp * cy);
    let right = fwd.cross(Vec3::Y).normalize_or(Vec3::X);
    let up = right.cross(fwd);
    let (ty, tx) = (tan_y.max(1e-4) * fill, tan_y.max(1e-4) * aspect.max(1e-3) * fill);
    // Near enough that every point is inside the frame's fill.
    let fit = |target: Vec3| {
        points.iter().fold(size * 0.2, |d, p| {
            let v = *p - target;
            let z = v.dot(fwd);
            d.max(v.dot(right).abs() / tx - z).max(v.dot(up).abs() / ty - z)
        })
    };
    let mut target = (min + max) * 0.5;
    for _ in 0..6 {
        let dist = fit(target);
        // Then the picture of the model centered on the frame (the camera
        // moves with its target: a parallel shift).
        let eye = target - fwd * dist;
        let (mut x0, mut x1, mut y0, mut y1) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
        for p in points {
            let v = *p - eye;
            let depth = v.dot(fwd).max(1e-6);
            let (x, y) = (v.dot(right) / depth, v.dot(up) / depth);
            (x0, x1, y0, y1) = (x0.min(x), x1.max(x), y0.min(y), y1.max(y));
        }
        let (mx, my) = ((x0 + x1) * 0.5, (y0 + y1) * 0.5);
        if mx.abs() < 1e-5 && my.abs() < 1e-5 {
            break;
        }
        target += (right * mx + up * my) * dist;
    }
    cam.target = target.into();
    cam.distance = fit(target);
}

/// How bright a light is at a point of its face (`nx`, `ny` from -1 to 1
/// across its width and height): soft edges and the top to bottom
/// gradient (the shader's `profile`).
fn profile(disc: bool, feather: f32, gradient: f32, nx: f32, ny: f32) -> f32 {
    let edge = if disc { (1.0 - feather * (nx * nx + ny * ny)).max(0.0) } else { (1.0 - feather * nx * nx).max(0.0) * (1.0 - 0.5 * feather * ny.powi(4)) };
    edge * (1.0 + gradient * ny).max(0.0)
}

/// The light a box of radiance 1 puts on a surface facing it from `d`
/// away on its axis (rectangle `w` x `h`, or a disc of diameter `w`; its
/// profile and grid), summed over its area: what makes a large near box
/// give the power asked, not more.
fn box_irradiance(disc: bool, w: f32, h: f32, grid_cos: f32, feather: f32, gradient: f32, d: f32) -> f32 {
    const N: usize = 32;
    let mut e = 0.0f64;
    let da = (w * h) as f64 / (N * N) as f64;
    for i in 0..N {
        for j in 0..N {
            let nx = (i as f32 + 0.5) / N as f32 * 2.0 - 1.0;
            let ny = (j as f32 + 0.5) / N as f32 * 2.0 - 1.0;
            if disc && nx * nx + ny * ny > 1.0 {
                continue;
            }
            let (x, y) = (nx * w * 0.5, ny * h * 0.5);
            let r2 = x * x + y * y + d * d;
            let c = d / r2.sqrt();
            e += (grid_falloff(grid_cos, c) * profile(disc, feather, gradient, nx, ny) * c * c / r2) as f64 * da;
        }
    }
    e as f32
}

fn grid_falloff(grid_cos: f32, c: f32) -> f32 {
    if grid_cos <= 0.0 {
        return 1.0;
    }
    let t = ((c - grid_cos) / ((1.0 - grid_cos) * 0.5)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

pub fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub struct GpuLight {
    /// xyz position (the face's center), w kind (0 off, 1 rectangle, 2
    /// disc, 4 black flag).
    pub pos: [f32; 4],
    /// Emission axis (toward the subject), w width.
    pub fwd: [f32; 4],
    /// w height.
    pub right: [f32; 4],
    /// w cosine of the grid's half angle (0: no grid).
    pub up: [f32; 4],
    /// Radiance at the face's center, w area.
    pub color: [f32; 4],
    /// Soft edges, gradient.
    pub prof: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub struct StudioGpu {
    /// on, light count, ground height, shadow opacity.
    pub a: [f32; 4],
    /// Linear background (as shown, exposure taken out), room light.
    pub bg: [f32; 4],
    /// Environment light, object size, floor reflection, its fade height
    /// (world; 0 none).
    pub b: [f32; 4],
    /// Toward the lit pool of the background (from the camera), w its
    /// angular radius.
    pub pool: [f32; 4],
    /// Background falloff, ground on (1).
    pub c: [f32; 4],
    pub lights: [GpuLight; MAX_LIGHTS],
}

/// A light as placed, for the cards' bounce.
struct Emitter {
    pos: Vec3,
    fwd: Vec3,
    right: Vec3,
    up: Vec3,
    w: f32,
    h: f32,
    disc: bool,
    grid: f32,
    feather: f32,
    gradient: f32,
    rad: Vec3,
}

impl Emitter {
    /// The light it puts on a surface at `p` facing `n` (no shadows).
    fn irradiance(&self, p: Vec3, n: Vec3) -> Vec3 {
        const N: usize = 12;
        let mut e = 0.0f32;
        let da = self.w * self.h / (N * N) as f32;
        for i in 0..N {
            for j in 0..N {
                let nx = (i as f32 + 0.5) / N as f32 * 2.0 - 1.0;
                let ny = (j as f32 + 0.5) / N as f32 * 2.0 - 1.0;
                if self.disc && nx * nx + ny * ny > 1.0 {
                    continue;
                }
                let q = self.pos + self.right * (nx * self.w * 0.5) + self.up * (ny * self.h * 0.5);
                let v = q - p;
                let r2 = v.length_squared().max(1e-8);
                let dir = v / r2.sqrt();
                let cos_l = -dir.dot(self.fwd);
                let cos_r = dir.dot(n);
                if cos_l <= 0.0 || cos_r <= 0.0 {
                    continue;
                }
                e += grid_falloff(self.grid, cos_l) * profile(self.disc, self.feather, self.gradient, nx, ny) * cos_l * cos_r / r2 * da;
            }
        }
        self.rad * e
    }
}

impl Studio {
    /// The frame in a `w` x `h` view (x, y, width, height, pixels): the
    /// format fitted inside with a margin.
    pub fn frame_in(&self, w: f32, h: f32) -> (f32, f32, f32, f32) {
        let Some(a) = self.format.aspect() else { return (0.0, 0.0, w, h) };
        let (mw, mh) = (w * 0.92, h * 0.9);
        let (fw, fh) = if mw / mh > a { (mh * a, mh) } else { (mw, mw / a) };
        ((w - fw) * 0.5, (h - fh) * 0.5, fw, fh)
    }

    /// tan of half the frame's vertical angle (the lens on 36 mm along the
    /// long side).
    pub fn tan_half_y(&self, aspect: f32) -> f32 {
        let long = 18.0 / self.lens_mm.max(1.0);
        if aspect >= 1.0 {
            long / aspect
        } else {
            long
        }
    }

    /// A light's position and axes (forward toward the subject).
    pub fn place(&self, l: &StudioLight, cam_yaw: f32, center: Vec3, size: f32) -> (Vec3, Vec3, Vec3, Vec3) {
        let yaw = if self.follow_camera { cam_yaw } else { 0.0 } + l.azimuth.to_radians();
        let el = l.elevation.to_radians().clamp(-1.55, 1.55);
        let dir = Vec3::new(el.cos() * yaw.sin(), el.sin(), el.cos() * yaw.cos());
        let pos = center + dir * l.distance.max(0.2) * size;
        let fwd = (center - pos).normalize_or(Vec3::NEG_Y);
        // Upright boxes; from straight above, square to the camera.
        let side = Vec3::new(yaw.cos(), 0.0, -yaw.sin());
        let right = fwd.cross(Vec3::Y).try_normalize().filter(|_| fwd.y.abs() < 0.995).unwrap_or(side);
        let up = right.cross(fwd);
        (pos, fwd, right, up)
    }

    /// The path tracer's studio: `center`, `size` of the model, its lowest
    /// point `ground`, the camera at `eye`, the exposure multiplier the
    /// view applies.
    pub fn gpu(&self, cam_yaw: f32, eye: Vec3, center: Vec3, size: f32, ground: f32, exposure: f32) -> StudioGpu {
        let lin = |c: [f32; 3]| Vec3::from(c.map(srgb_to_linear));
        let bg = lin(self.background) / exposure.max(1e-6);
        let to_pool = (center - eye).normalize_or(Vec3::NEG_Z);
        let pool = (self.pool.max(0.05) * 12.0 / self.lens_mm.max(1.0)).atan();
        let mut g = StudioGpu {
            a: [1.0, 0.0, ground, if self.ground_shadows { self.shadow_opacity.clamp(0.0, 1.0) } else { 0.0 }],
            bg: [bg.x, bg.y, bg.z, self.room.max(0.0)],
            b: [self.environment.max(0.0), size, self.floor_reflection.clamp(0.0, 1.0), self.reflection_fade.max(0.0) * size],
            pool: [to_pool.x, to_pool.y, to_pool.z, pool],
            c: [self.falloff.clamp(0.0, 1.0), if self.ground_shadows || self.floor_reflection > 0.0 { 1.0 } else { 0.0 }, 0.0, 0.0],
            ..Default::default()
        };
        let lights: Vec<&StudioLight> = self.lights.iter().filter(|l| l.on && (l.power > 0.0 || l.kind == LightKind::Flag)).take(MAX_LIGHTS).collect();
        let shape = |l: &StudioLight| {
            let (pos, fwd, right, up) = self.place(l, cam_yaw, center, size);
            let disc = matches!(l.kind, LightKind::Octabox | LightKind::Spot);
            let w = l.width.max(0.01) * size;
            let h = if disc { w } else { l.height.max(0.01) * size };
            let grid = if l.grid > 0.5 && l.kind != LightKind::Card { (l.grid.min(170.0) * 0.5).to_radians().cos() } else { 0.0 };
            let feather = if l.kind == LightKind::Card { 0.0 } else { l.feather.clamp(0.0, 1.0) };
            let gradient = if l.kind == LightKind::Card { 0.0 } else { l.gradient.clamp(-1.0, 1.0) };
            Emitter { pos, fwd, right, up, w, h, disc, grid, feather, gradient, rad: Vec3::ZERO }
        };
        // The lights' radiance: irradiance power * pi at the subject, the
        // whole face counted.
        let mut placed: Vec<Emitter> = lights
            .iter()
            .map(|l| {
                let mut e = shape(l);
                if l.kind.emits() {
                    let d = l.distance.max(0.2) * size;
                    e.rad = lin(l.color) * (l.power * std::f32::consts::PI / box_irradiance(e.disc, e.w, e.h, e.grid, e.feather, e.gradient, d).max(1e-12));
                }
                e
            })
            .collect();
        // Cards: white diffuse faces, their radiance from what the lights
        // put on their middle.
        for (i, l) in lights.iter().enumerate() {
            if l.kind == LightKind::Card {
                let e: Vec3 = placed.iter().zip(&lights).filter(|(_, o)| o.kind.emits()).map(|(o, _)| o.irradiance(placed[i].pos, placed[i].fwd)).sum();
                placed[i].rad = lin(l.color) * l.power.clamp(0.0, 1.0) * e / std::f32::consts::PI;
            }
        }
        for (n, (l, e)) in lights.iter().zip(&placed).enumerate() {
            let kind = match l.kind {
                LightKind::Flag => 4.0,
                LightKind::Octabox | LightKind::Spot => 2.0,
                _ => 1.0,
            };
            let area = if e.disc { std::f32::consts::PI * e.w * e.w * 0.25 } else { e.w * e.h };
            let gl = &mut g.lights[n];
            gl.pos = [e.pos.x, e.pos.y, e.pos.z, kind];
            gl.fwd = [e.fwd.x, e.fwd.y, e.fwd.z, e.w];
            gl.right = [e.right.x, e.right.y, e.right.z, e.h];
            gl.up = [e.up.x, e.up.y, e.up.z, e.grid];
            gl.color = [e.rad.x, e.rad.y, e.rad.z, area];
            gl.prof = [e.feather, e.gradient, 0.0, 0.0];
        }
        g.a[1] = lights.len() as f32;
        g
    }
}
