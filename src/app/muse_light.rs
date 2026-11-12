//! Claude lights the packshot, in its texturing conversation: the user
//! attaches a photo and asks ("I want this lighting for my packshot");
//! Claude reads the photo's lighting (its shadows, highlights and
//! background) and answers the studio's lights as JSON ("lighting" in its
//! answer). The app applies them, renders a small check of the product in
//! the studio and sends it back once, so Claude corrects what differs.

use crate::studio::{LightKind, Studio, StudioLight, MAX_LIGHTS};
use base64::Engine as _;
use serde_json::{json, Value};

/// The section of Claude's instructions on lighting the packshot studio.
pub(super) const LIGHTING: &str = r##"

PACKSHOT LIGHTING. The app has a packshot studio that renders the model with a path tracer. When the user asks for lighting (a photo's lighting, "I want this lighting for my packshot", a softer shadow, a darker background, a studio setup), you are a packshot photographer: read the reference photo's lighting precisely and rebuild it with the studio's lights, then add "lighting" to your answer (below) and return no looks unless the user also asks for texturing. A photo given for its lighting is never a print or a texture on the model. Match the lighting, not the product: the user's product may differ in shape and color. Each message gives the studio's lighting now (the same JSON) and the camera.

THE STUDIO.
- A seamless background, infinite, with no wall: its color shows exactly as set behind the product (the lights do not change it). Sample it from the photo where it is neither in shadow nor in a highlight: "background": "#RRGGBB". "falloff" 0 to 1: the background darkens away from the product toward the frame's corners (a vignette; 0 even, 0.2 gentle, 0.6 strong); "pool" 0.2 to 4: the size of the lit area behind the product, in frame heights.
- The ground under the product (at its lowest point) shows the background's color and catches its shadows: "ground_shadows" true or false, "shadow_opacity" 0 to 1 (how dark the shadows get where every light is blocked). A shadow seen on a wall in the photo goes on the ground here.
- "floor_reflection" 0 to 1: a glossy plexi floor mirroring the product (0 a matte seamless, 0.25 a soft reflection, 0.5 dark plexi); "reflection_fade" 0 to 4: the reflection fades out over that height (object sizes).
- "room" 0 to 1: light bounced by the studio onto the product where no light points (0.05 to 0.2).
- No props: pedestals, plants, water or fabric cannot be added. Light only.

THE LIGHTS (at most 8). Each: {"name": "Key softbox left", "kind": "...", "color": "#FFFFFF", "power": 0.8, "azimuth": -60, "elevation": 25, "distance": 3, "width": 2.4, "height": 2.4, "feather": 0.3, "gradient": 0, "grid": 0}.
- azimuth: degrees around the product, 0 from the camera's side, 90 the camera's right (the image's right), -90 its left, 180 straight behind. elevation: degrees above the product's center (90 straight above). distance, width, height: in object sizes (1 is half the diagonal of the product's box).
- power: the light it puts on the product, 1 = a white surface facing it shows pure white. A white high key packshot: key 0.6 to 1, fill 0.2 to 0.5, strips 0.6 to 1.5, a back scrim through glass 1 to 1.6. A card's power is how much of the light reaching it it bounces (0 to 1).
- feather 0 to 1: soft edges (0 even to its edges, crisp highlights; 1 bright in the middle fading out, graded highlights). gradient -1 to 1: brighter at its top (+) or bottom (-), a graded highlight down a bottle. grid 0 to 90: a honeycomb's beam angle in degrees (0 none), less light spilling around.
- kinds:
  "Strip": tall narrow softbox (width 0.3 to 0.6, height 2.5 to 3.5, distance 1.8 to 2.6): the long vertical highlights down the sides of glossy bottles and lacquered caps, usually a pair at azimuth -80 to -110 and 80 to 110.
  "Softbox": rectangular softbox (1 to 2.5 wide): a soft key or fill; its shadow is soft.
  "Octabox": round softbox, width is the diameter: round highlights, soft key.
  "Scrim": large diffusion panel lit through (2 to 3 wide, distance 2.5 to 5): the softest light, wide graded highlights; behind the product (azimuth 180) it makes glass and colored juice glow; in front (azimuth near 0, distance 4 to 5) an even fill on the label; overhead (elevation 70 to 80) a top light on caps and lids.
  "Spot": a hard light far away (diameter 0.2 to 0.6, distance 8 to 12): crisp, sun-like shadows; the smaller, the crisper.
  "Card": white bounce card (no light of its own): gentle fill and white edges in reflections.
  "Flag": black card: dark shapes in reflections, the dark edges of glass bottles (azimuth -30 to -45 and 30 to 45, distance 2.2 to 2.8).

READING THE PHOTO.
- The shadow on the ground tells the key: falling toward the image's right means the key is on the left (negative azimuth), toward the camera means it is behind; long means a low light (elevation 10 to 25), short or under the product a high one (45 to 75). Crisp edges: a Spot; soft edges: a large Softbox or Scrim near the product. How dark the shadow is against the background sets shadow_opacity (a faint e-commerce shadow 0.25 to 0.4; a strong sun shadow 0.5 to 0.7). No visible shadow: shadow_opacity low or no key from the side. A shadow only reads when the key gives most of the light on the ground: lights from above (elevation 60 to 90) light the ground the most and wash a side key's shadow out, so keep them weak (0.1 to 0.2) when the photo shows a shadow; a very large source close to the product (a scrim 2.5 wide at distance 3) makes its shadow vanish, a soft but readable shadow comes from a Softbox 1 to 1.5 wide at distance 4 to 5.
- Highlights on glossy surfaces (glass, lacquer, metal caps) are the lights' reflections: their place, shape and length give each light (vertical bands near the edges: strips at the sides; a broad soft sheen: a scrim; a round spot: an octabox; dark bands at the edges: flags; a bright juice: a back scrim).
- Where a light's reflection lands on a glossy cylinder seen from the front: across its width at the sine of half the light's azimuth (azimuth 0: the middle; 60: halfway to the edge; 100: three quarters; 135: the edge line). A broad sheen wrapping to one edge: a large Scrim close (distance about 2) slightly behind that side (azimuth 95 to 110 on that side). A side that stays a dim, saturated color with no grey band: nothing reflects there; light it with a front fill straight on (azimuth within 10 of 0), whose reflection hides behind a front label, and keep strips on that side far behind (azimuth 130 to 140) or weak. A bright shoulder or top edge: a back scrim (azimuth 180, elevation 10 to 20) it reflects.
- Compare the whole picture, not only the shadow: the brightness of the product's lit side, its shadow side and its label against the photo's.
- Contrast between the lit and shadow sides sets the fill's power; the color of the light and the background (warm, neutral, cool) sets the colors (near white: #FFF4E6 warm, #EEF3FF cool).

THE PHOTO'S MEASURES. With a photo given for its lighting, the app measures it and gives you the numbers ("light measures"): the background's exact color, the ground shadow at the product's foot (which side, how much darker than the background, how far it reaches in product widths), a shadow along its sides, which side of the product is brighter. They are exact: take the background color as measured, put the key on the side opposite the shadow, and make the shadow as dark and as long as measured. The check render comes with the same measures of your render: make them agree.

THE CAMERA. The photo's view matters as much as the lights: a ground shadow only shows when the camera looks down on the ground a little. Give the photo's view in "camera": {"pitch": 9, "lens_mm": 100}: pitch, the degrees the camera looks down (0 at eye level: the top of a cylinder is a flat line; 5 to 12 for most packshots: its top is a thin ellipse and the ground shows; 20 and more: seen clearly from above); lens_mm, 85 to 135 for packshots (little perspective), 50 for a closer, stronger perspective. The app keeps the product centered in the frame.

THE LIGHTING IN YOUR ANSWER: "lighting": {"background": "#RRGGBB", "falloff": 0, "pool": 1.5, "ground_shadows": true, "shadow_opacity": 0.35, "floor_reflection": 0, "reflection_fade": 1, "room": 0.12, "lights": [ ... ], "camera": {"pitch": 9, "lens_mm": 100}}, next to "reply" in your JSON object. Always the complete lighting (every light, the background and the floor), not only what changes. "reply" then says in one or two short sentences the lighting you read and how you set it. Leave "lighting" out when the user does not ask about the lighting."##;

/// The lighting check: a render of the product with Claude's lighting goes
/// back to it once.
pub(super) fn check_text(asked: &str, photo: &str, render: &str) -> String {
    let measures = if photo.is_empty() { String::new() } else { format!(" LIGHT MEASURES by the app: the photo: {photo}. Your render: {render}. Make them agree (the background color, the shadow's side, darkness and reach, the brighter side).") };
    format!("LIGHTING CHECK (automatic, not written by the user). The image is a render of the product in the studio with the lighting you just set, from the user's camera. Compare it with the reference photo the user gave for the lighting (\"{asked}\"): the background's tone and falloff, the shadow's direction, length, softness and darkness, each highlight on the product (where, its shape, how bright), the contrast between the lit and the shadow side, the overall brightness and the light's color. Judge the lighting, not the product's own colors or shape.{measures} Send \"lighting\" corrected and complete (with \"camera\"), and no looks; \"reply\": one short sentence on what you changed, or that it matches.")
}

/// An image as Claude gets it: a JPEG data URL, at most 1024 px.
pub(super) fn jpeg_url(img: &image::DynamicImage) -> anyhow::Result<String> {
    let small = if img.width().max(img.height()) > 1024 { img.resize(1024, 1024, image::imageops::FilterType::Triangle) } else { img.clone() };
    let mut jpg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpg, 88).encode_image(&small.to_rgb8())?;
    Ok(format!("data:image/jpeg;base64,{}", base64::engine::general_purpose::STANDARD.encode(&jpg)))
}

fn hex(v: &Value) -> Option<[f32; 3]> {
    let s = v.as_str()?.trim().trim_start_matches('#');
    let n = u32::from_str_radix(s.get(..6)?, 16).ok()?;
    Some([((n >> 16) & 255) as f32 / 255.0, ((n >> 8) & 255) as f32 / 255.0, (n & 255) as f32 / 255.0])
}

fn to_hex(c: [f32; 3]) -> String {
    let b = |x: f32| (x.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("#{:02X}{:02X}{:02X}", b(c[0]), b(c[1]), b(c[2]))
}

fn num(v: &Value, lo: f32, hi: f32) -> Option<f32> {
    v.as_f64().map(|x| (x as f32).clamp(lo, hi)).filter(|x| x.is_finite())
}

/// One light of Claude's answer (None: no kind it knows).
fn light(v: &Value) -> Option<StudioLight> {
    let k = v["kind"].as_str()?.trim().to_lowercase();
    let kind = LightKind::ALL.into_iter().find(|x| format!("{x:?}").to_lowercase() == k || x.name().to_lowercase() == k)?;
    let (width, height, feather) = kind.shape();
    let mut l = StudioLight { name: v["name"].as_str().unwrap_or(kind.name()).to_string(), kind, width, height, feather, ..Default::default() };
    if let Some(c) = hex(&v["color"]) {
        l.color = c;
    }
    let power_max = if kind == LightKind::Card { 1.0 } else { 4.0 };
    let set = |x: &mut f32, key: &str, lo: f32, hi: f32| {
        if let Some(n) = num(&v[key], lo, hi) {
            *x = n;
        }
    };
    set(&mut l.power, "power", 0.0, power_max);
    set(&mut l.azimuth, "azimuth", -180.0, 180.0);
    set(&mut l.elevation, "elevation", -60.0, 90.0);
    set(&mut l.distance, "distance", 1.0, 12.0);
    set(&mut l.width, "width", 0.1, 6.0);
    set(&mut l.height, "height", 0.1, 6.0);
    set(&mut l.feather, "feather", 0.0, 1.0);
    set(&mut l.gradient, "gradient", -1.0, 1.0);
    set(&mut l.grid, "grid", 0.0, 90.0);
    if let Some(on) = v["on"].as_bool() {
        l.on = on;
    }
    Some(l)
}

/// The camera's pitch Claude asks (radians), if any.
pub(super) fn pitch(l: &Value) -> Option<f32> {
    num(&l["camera"]["pitch"], -10.0, 60.0).map(f32::to_radians)
}

/// Whether a message is about the lighting (the photos it brings are then
/// measured for Claude).
pub(super) fn about_lighting(text: &str) -> bool {
    let t = text.to_lowercase();
    ["light", "lighting", "packshot", "pack shot", "shadow", "studio", "eclairage", "\u{e9}clairage", "lumi\u{e8}re", "ombre"].iter().any(|w| t.contains(w))
}

/// Whether a message asks for the lighting alone: about the lighting and
/// saying nothing of textures, materials or colors. Claude then keeps every
/// material as it is.
pub(super) fn lighting_only(text: &str) -> bool {
    let t = text.to_lowercase().replace("highlight", "");
    about_lighting(&t)
        && !["textur", "materia", "mat\u{e9}ria", "mati\u{e8}re", "color", "colour", "couleur", "paint", "peint", "finish", "finition", "retexture", "label", "logo", "print"].iter().any(|w| t.contains(w))
}

/// Claude's lighting onto `st` (its angles and output kept; the lens when
/// it gives the camera). The number of lights it set.
pub(super) fn apply(l: &Value, st: &mut Studio) -> usize {
    if let Some(c) = hex(&l["background"]) {
        st.background = c;
    }
    let set = |x: &mut f32, key: &str, lo: f32, hi: f32| {
        if let Some(n) = num(&l[key], lo, hi) {
            *x = n;
        }
    };
    set(&mut st.falloff, "falloff", 0.0, 1.0);
    set(&mut st.pool, "pool", 0.2, 4.0);
    set(&mut st.shadow_opacity, "shadow_opacity", 0.0, 1.0);
    set(&mut st.floor_reflection, "floor_reflection", 0.0, 1.0);
    set(&mut st.reflection_fade, "reflection_fade", 0.0, 4.0);
    set(&mut st.room, "room", 0.0, 1.0);
    set(&mut st.environment, "environment", 0.0, 2.0);
    if let Some(b) = l["ground_shadows"].as_bool() {
        st.ground_shadows = b;
    }
    if let Some(mm) = num(&l["camera"]["lens_mm"], 14.0, 300.0) {
        st.lens_mm = mm;
    }
    let lights: Vec<StudioLight> = l["lights"].as_array().into_iter().flatten().filter_map(light).take(MAX_LIGHTS).collect();
    if !lights.is_empty() {
        st.lights = lights;
    }
    st.lights.len()
}

/// The studio's lighting in Claude's words (the same JSON it answers).
pub(super) fn to_json(st: &Studio) -> Value {
    let r = |x: f32| ((x as f64) * 100.0).round() / 100.0;
    let lights: Vec<Value> = st
        .lights
        .iter()
        .map(|l| {
            json!({"name": l.name, "on": l.on, "kind": format!("{:?}", l.kind), "color": to_hex(l.color), "power": r(l.power), "azimuth": r(l.azimuth), "elevation": r(l.elevation),
                "distance": r(l.distance), "width": r(l.width), "height": r(l.height), "feather": r(l.feather), "gradient": r(l.gradient), "grid": r(l.grid)})
        })
        .collect();
    json!({"background": to_hex(st.background), "falloff": r(st.falloff), "pool": r(st.pool), "ground_shadows": st.ground_shadows, "shadow_opacity": r(st.shadow_opacity),
        "floor_reflection": r(st.floor_reflection), "reflection_fade": r(st.reflection_fade), "room": r(st.room), "environment": r(st.environment), "lights": lights})
}

/// Claude's instructions on the studio: the rules, then the app's setups as
/// tuned starting points.
pub(super) fn rules() -> String {
    let mut s = LIGHTING.to_string();
    s += "\n\nTUNED SETUPS. The app's setups, each matched on real packshots of its look (rendered on a product next to the photo and adjusted until the light, the shadow and the highlights matched): start from the one closest to the photo and change only what the photo shows differently. Their exact lighting:";
    for (i, name) in crate::studio::SETUPS.iter().enumerate() {
        let mut st = Studio::default();
        st.setup(i);
        s += &format!("\n- {name}: {}", to_json(&st));
    }
    s
}
