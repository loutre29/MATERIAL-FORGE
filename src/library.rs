//! Shelf content: built-in materials and smart masks (authored here from
//! procedurals and generators), the starter smart materials (scanned
//! texture materials with generator and grunge masks), and the user library
//! in %APPDATA%\SP_RETRO\library.

use crate::document::*;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub fn library_dir() -> PathBuf {
    // Tests point MF_DATA_DIR elsewhere: they must not touch the user's data.
    if let Some(d) = std::env::var_os("MF_DATA_DIR") {
        return PathBuf::from(d).join("library");
    }
    let base = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    base.join("SP_RETRO").join("library")
}

/// Recently opened or saved projects, newest first (File > Open recent).
fn recent_file() -> PathBuf {
    library_dir().parent().map(|p| p.join("recent.json")).unwrap_or_else(|| PathBuf::from("recent.json"))
}

/// The recent projects that still exist, newest first (at most 10).
pub fn recent_projects() -> Vec<PathBuf> {
    let list: Vec<PathBuf> = std::fs::read(recent_file()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
    list.into_iter().filter(|p| p.is_file()).take(10).collect()
}

/// Puts `p` first in the recent projects.
pub fn add_recent(p: &Path) {
    let p = std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let mut list = recent_projects();
    list.retain(|q| std::fs::canonicalize(q).map_or(true, |q| q != p));
    list.insert(0, p);
    list.truncate(10);
    if let Ok(json) = serde_json::to_vec_pretty(&list) {
        let f = recent_file();
        if let Some(d) = f.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let _ = std::fs::write(f, json);
    }
}

pub fn clear_recent() {
    let _ = std::fs::remove_file(recent_file());
}

pub fn sub_dir(name: &str) -> PathBuf {
    let d = library_dir().join(name);
    let _ = std::fs::create_dir_all(&d);
    d
}

// ---------------------------------------------------------------- builders

fn uni(v: [f32; 3]) -> FillChannel {
    FillChannel { value: v, ..Default::default() }
}

fn gray(v: f32) -> FillChannel {
    uni([v; 3])
}

fn procc(kind: ProcKind, a: [f32; 3], b: [f32; 3]) -> FillChannel {
    FillChannel { source: SourceKind::Procedural, value: a, value_b: b, procedural: ProcParams::of(kind), ..Default::default() }
}

fn procs(kind: ProcKind, a: f32, b: f32) -> FillChannel {
    procc(kind, [a; 3], [b; 3])
}

fn off() -> FillChannel {
    FillChannel { enabled: false, ..Default::default() }
}

/// color, roughness, metallic (+ optional height, emissive, opacity).
fn fill(color: FillChannel, rough: FillChannel, metal: FillChannel) -> FillDesc {
    let mut f = FillDesc::default();
    f.channels[CH_COLOR] = color;
    f.channels[CH_ROUGH] = rough;
    f.channels[CH_METAL] = metal;
    f.channels[CH_HEIGHT] = off();
    f.channels[CH_EMISSIVE] = off();
    f.channels[CH_OPACITY] = off();
    f.channels[CH_NORMAL] = off();
    f.projection = Projection { mode: ProjMode::Object3D, scale: 8.0, ..Default::default() };
    f
}

fn with_height(mut f: FillDesc, h: FillChannel) -> FillDesc {
    f.channels[CH_HEIGHT] = h;
    f
}

fn with_proj(mut f: FillDesc, mode: ProjMode, scale: f32) -> FillDesc {
    f.projection.mode = mode;
    f.projection.scale = scale;
    f
}

fn tweak(mut c: FillChannel, f: impl Fn(&mut ProcParams)) -> FillChannel {
    f(&mut c.procedural);
    c
}

pub struct MaterialPreset {
    pub name: &'static str,
    pub fill: FillDesc,
}

pub fn materials() -> Vec<MaterialPreset> {
    use ProcKind::*;
    let m = |name, fill| MaterialPreset { name, fill };
    vec![
        m("Iron raw", fill(uni([0.42, 0.40, 0.38]), procs(Clouds, 0.35, 0.6), gray(1.0))),
        m("Steel polished", fill(uni([0.62, 0.62, 0.64]), gray(0.14), gray(1.0))),
        m("Steel brushed", fill(uni([0.6, 0.6, 0.62]), tweak(procs(Fibers, 0.18, 0.42), |p| p.aspect = 0.05), gray(1.0))),
        m("Aluminium", fill(uni([0.85, 0.86, 0.87]), gray(0.3), gray(1.0))),
        m("Chrome", fill(uni([0.55, 0.56, 0.55]), gray(0.05), gray(1.0))),
        m("Gold", fill(uni([1.0, 0.766, 0.336]), gray(0.22), gray(1.0))),
        m("Silver", fill(uni([0.97, 0.96, 0.91]), gray(0.2), gray(1.0))),
        m("Copper", fill(uni([0.955, 0.638, 0.538]), gray(0.28), gray(1.0))),
        m("Bronze", fill(uni([0.7, 0.45, 0.2]), procs(Clouds, 0.28, 0.5), gray(1.0))),
        m("Gunmetal", fill(uni([0.1, 0.1, 0.11]), gray(0.35), gray(1.0))),
        m(
            "Rust",
            with_height(
                fill(procc(Rust, [0.18, 0.06, 0.02], [0.45, 0.17, 0.05]), gray(0.85), gray(0.0)),
                procs(Grunge, -0.15, 0.1),
            ),
        ),
        m("Plastic glossy red", fill(uni([0.6, 0.03, 0.02]), gray(0.2), gray(0.0))),
        m("Plastic matte gray", with_height(fill(uni([0.3, 0.3, 0.3]), gray(0.6), gray(0.0)), procs(Grain, -0.02, 0.02))),
        m("Rubber", fill(uni([0.03, 0.03, 0.03]), gray(0.9), gray(0.0))),
        m("Paint glossy white", fill(uni([0.8, 0.8, 0.8]), gray(0.3), gray(0.0))),
        m("Car paint blue", fill(uni([0.02, 0.08, 0.4]), gray(0.1), gray(0.3))),
        m(
            "Wood oak",
            with_height(
                fill(procc(Wood, [0.25, 0.12, 0.05], [0.5, 0.3, 0.13]), procs(Wood, 0.7, 0.55), gray(0.0)),
                procs(Wood, -0.08, 0.08),
            ),
        ),
        m(
            "Leather",
            with_height(
                fill(procc(Leather, [0.12, 0.05, 0.02], [0.25, 0.11, 0.05]), procs(Leather, 0.7, 0.5), gray(0.0)),
                procs(Leather, -0.15, 0.08),
            ),
        ),
        m(
            "Fabric denim",
            with_proj(
                with_height(fill(procc(Fabric, [0.03, 0.06, 0.18], [0.1, 0.16, 0.38]), gray(0.95), gray(0.0)), procs(Fabric, -0.1, 0.1)),
                ProjMode::Uv,
                30.0,
            ),
        ),
        m(
            "Concrete",
            with_height(fill(procc(Clouds, [0.3, 0.3, 0.29], [0.45, 0.44, 0.42]), gray(0.9), gray(0.0)), procs(Grain, -0.04, 0.04)),
        ),
        m(
            "Bricks",
            with_proj(
                with_height(
                    fill(procc(Bricks, [0.15, 0.14, 0.13], [0.4, 0.15, 0.08]), gray(0.85), gray(0.0)),
                    procs(Bricks, -0.3, 0.15),
                ),
                ProjMode::Uv,
                4.0,
            ),
        ),
        m("Marble", fill(procc(Marble, [0.3, 0.3, 0.32], [0.85, 0.85, 0.83]), gray(0.15), gray(0.0))),
        m(
            "Carbon fiber",
            with_proj(
                with_height(fill(procc(Fabric, [0.01, 0.01, 0.012], [0.06, 0.06, 0.065]), gray(0.25), gray(0.0)), procs(Fabric, -0.05, 0.05)),
                ProjMode::Uv,
                40.0,
            ),
        ),
        m("Mud", with_height(fill(procc(Clouds, [0.08, 0.05, 0.03], [0.2, 0.14, 0.08]), gray(0.95), gray(0.0)), procs(Grunge, -0.1, 0.1))),
        m("Snow", with_height(fill(uni([0.9, 0.92, 0.95]), procs(Clouds, 0.4, 0.7), gray(0.0)), procs(Clouds, -0.1, 0.1))),
        m("Moss", fill(procc(Clouds, [0.05, 0.12, 0.02], [0.15, 0.25, 0.04]), gray(0.9), gray(0.0))),
        m("Glow orange", {
            let mut f = fill(uni([0.05, 0.02, 0.01]), gray(0.5), gray(0.0));
            f.channels[CH_EMISSIVE] = uni([3.0, 0.9, 0.15]);
            f
        }),
        m("Glass tinted", {
            let mut f = fill(uni([0.7, 0.85, 0.95]), gray(0.05), gray(0.0));
            f.channels[CH_OPACITY] = gray(0.25);
            f
        }),
        // Fabrics.
        m("Fabric cotton", with_proj(with_height(fill(procc(Fabric, [0.62, 0.6, 0.56], [0.78, 0.76, 0.72]), gray(0.85), gray(0.0)), procs(Fabric, -0.03, 0.03)), ProjMode::Triplanar, 18.0)),
        m("Fabric felt", with_proj(with_height(fill(procc(Grain, [0.3, 0.12, 0.1], [0.42, 0.18, 0.14]), gray(0.95), gray(0.0)), procs(Grain, -0.02, 0.02)), ProjMode::Triplanar, 10.0)),
        m("Fabric linen", with_proj(with_height(fill(procc(Fabric, [0.66, 0.6, 0.5], [0.8, 0.75, 0.64]), gray(0.8), gray(0.0)), tweak(procs(Fabric, -0.04, 0.04), |p| p.scale = 0.6)), ProjMode::Triplanar, 14.0)),
        m("Fabric nylon", with_proj(fill(procc(Fabric, [0.05, 0.08, 0.16], [0.09, 0.13, 0.25]), gray(0.42), gray(0.0)), ProjMode::Triplanar, 28.0)),
        m("Fabric wool", with_proj(with_height(fill(procc(Turbulence, [0.48, 0.44, 0.38], [0.62, 0.58, 0.5]), gray(0.92), gray(0.0)), procs(Turbulence, -0.04, 0.04)), ProjMode::Triplanar, 8.0)),
        m("Fabric spandex", with_proj(fill(procc(Fibers, [0.04, 0.04, 0.05], [0.09, 0.09, 0.1]), gray(0.35), gray(0.0)), ProjMode::Triplanar, 30.0)),
        m("Fabric tarp", with_proj(with_height(fill(procc(Fabric, [0.12, 0.2, 0.32], [0.18, 0.28, 0.42]), gray(0.6), gray(0.0)), procs(Fabric, -0.05, 0.05)), ProjMode::Triplanar, 6.0)),
        m("Fabric ripstop", with_proj(with_height(fill(procc(Checker, [0.18, 0.22, 0.12], [0.22, 0.27, 0.15]), gray(0.55), gray(0.0)), procs(Checker, -0.02, 0.02)), ProjMode::Triplanar, 24.0)),
        // Clays and ceramics.
        m("Clay earthenware", with_height(fill(procc(Clouds, [0.55, 0.42, 0.3], [0.68, 0.53, 0.38]), gray(0.8), gray(0.0)), procs(Grain, -0.02, 0.02))),
        m("Clay terracotta", with_height(fill(procc(Clouds, [0.48, 0.2, 0.1], [0.62, 0.28, 0.15]), gray(0.85), gray(0.0)), procs(Grain, -0.03, 0.03))),
        m("Ceramic glazed white", fill(uni([0.88, 0.88, 0.86]), gray(0.08), gray(0.0))),
        m("Porcelain blue", fill(procc(Marble, [0.12, 0.2, 0.5], [0.92, 0.93, 0.95]), gray(0.1), gray(0.0))),
        // Concrete and ground.
        m("Concrete smooth", with_height(fill(procc(Clouds, [0.45, 0.45, 0.44], [0.58, 0.58, 0.57]), gray(0.75), gray(0.0)), procs(Grain, -0.02, 0.02))),
        m("Asphalt", with_proj(with_height(fill(procc(Grain, [0.06, 0.06, 0.065], [0.16, 0.16, 0.17]), gray(0.9), gray(0.0)), procs(Grain, -0.04, 0.04)), ProjMode::Triplanar, 12.0)),
        m("Ground sand", with_proj(with_height(fill(procc(Grain, [0.62, 0.52, 0.38], [0.76, 0.66, 0.5]), gray(0.95), gray(0.0)), procs(Turbulence, -0.08, 0.08)), ProjMode::Triplanar, 6.0)),
        m("Ground gravel", with_proj(with_height(fill(procc(Cells, [0.32, 0.3, 0.27], [0.55, 0.52, 0.48]), gray(0.85), gray(0.0)), procs(Cells, -0.15, 0.15)), ProjMode::Triplanar, 10.0)),
        m("Ground soil", with_proj(with_height(fill(procc(Clouds, [0.14, 0.1, 0.07], [0.26, 0.19, 0.13]), gray(0.9), gray(0.0)), procs(Grunge, -0.06, 0.06)), ProjMode::Triplanar, 5.0)),
        // Organic.
        m("Cork natural", with_proj(with_height(fill(procc(Dots, [0.5, 0.36, 0.22], [0.66, 0.5, 0.32]), gray(0.85), gray(0.0)), procs(Dots, -0.04, 0.02)), ProjMode::Triplanar, 12.0)),
        m("Paper", with_proj(fill(procc(Fibers, [0.86, 0.85, 0.82], [0.93, 0.92, 0.9]), gray(0.8), gray(0.0)), ProjMode::Triplanar, 20.0)),
        m("Cardboard", with_proj(with_height(fill(procc(Fibers, [0.48, 0.36, 0.22], [0.58, 0.45, 0.3]), gray(0.85), gray(0.0)), procs(Fibers, -0.02, 0.02)), ProjMode::Triplanar, 12.0)),
        m("Leather suede", with_proj(with_height(fill(procc(Grain, [0.36, 0.22, 0.12], [0.46, 0.3, 0.17]), gray(0.95), gray(0.0)), procs(Grain, -0.01, 0.01)), ProjMode::Triplanar, 14.0)),
        m("Leather patent", with_proj(with_height(fill(uni([0.03, 0.03, 0.035]), gray(0.08), gray(0.0)), procs(Leather, -0.02, 0.02)), ProjMode::Triplanar, 6.0)),
        m("Wood walnut", with_proj(with_height(fill(procc(Wood, [0.14, 0.08, 0.05], [0.26, 0.16, 0.1]), gray(0.55), gray(0.0)), procs(Wood, -0.02, 0.02)), ProjMode::Triplanar, 2.0)),
        m("Wood pine", with_proj(with_height(fill(procc(Wood, [0.58, 0.42, 0.24], [0.74, 0.58, 0.36]), gray(0.6), gray(0.0)), procs(Wood, -0.02, 0.02)), ProjMode::Triplanar, 2.0)),
        m("Skin", fill(procc(Clouds, [0.62, 0.42, 0.34], [0.72, 0.5, 0.42]), gray(0.55), gray(0.0))),
        m("Wax", fill(uni([0.86, 0.8, 0.62]), gray(0.35), gray(0.0))),
        m("Jade", fill(procc(Marble, [0.12, 0.38, 0.22], [0.36, 0.62, 0.42]), gray(0.18), gray(0.0))),
        // Metals.
        m("Brass", fill(uni([0.91, 0.78, 0.42]), gray(0.25), gray(1.0))),
        m("Nickel", fill(uni([0.66, 0.61, 0.53]), gray(0.22), gray(1.0))),
        m("Titanium", fill(uni([0.54, 0.5, 0.45]), gray(0.3), gray(1.0))),
        m("Iron forged", with_height(fill(procc(Clouds, [0.18, 0.17, 0.16], [0.3, 0.29, 0.27]), procs(Grunge, 0.4, 0.75), gray(1.0)), procs(Turbulence, -0.04, 0.04))),
        m("Steel galvanized", with_proj(fill(procc(Cells, [0.55, 0.56, 0.57], [0.7, 0.71, 0.72]), procs(Cells, 0.25, 0.45), gray(1.0)), ProjMode::Triplanar, 4.0)),
        m("Zinc", fill(uni([0.66, 0.68, 0.7]), procs(Clouds, 0.4, 0.55), gray(1.0))),
        m("Glitter", with_proj(fill(procc(Grain, [0.4, 0.4, 0.42], [0.95, 0.95, 0.97]), procs(Grain, 0.15, 0.4), gray(1.0)), ProjMode::Triplanar, 30.0)),
        // Plastics and stone.
        m("Plastic glossy black", fill(uni([0.02, 0.02, 0.022]), gray(0.12), gray(0.0))),
        m("Silicone", fill(uni([0.7, 0.72, 0.75]), gray(0.45), gray(0.0))),
        m("Granite", with_proj(fill(procc(Dots, [0.18, 0.17, 0.17], [0.6, 0.58, 0.56]), gray(0.35), gray(0.0)), ProjMode::Triplanar, 8.0)),
        m("Slate", with_proj(with_height(fill(procc(Ridged, [0.14, 0.15, 0.17], [0.24, 0.25, 0.28]), gray(0.7), gray(0.0)), procs(Ridged, -0.05, 0.05)), ProjMode::Triplanar, 3.0)),
        m("Sandstone", with_proj(with_height(fill(procc(Turbulence, [0.6, 0.48, 0.34], [0.74, 0.6, 0.44]), gray(0.9), gray(0.0)), procs(Grain, -0.04, 0.04)), ProjMode::Triplanar, 4.0)),
    ]
}

pub fn material(name: &str) -> FillDesc {
    materials().into_iter().find(|m| m.name == name).map(|m| m.fill).unwrap_or_default()
}

fn effect(blend: MaskBlend, opacity: f32, kind: MaskEffectKind) -> MaskEffect {
    MaskEffect { id: 0, enabled: true, blend, opacity, kind }
}

fn gen(kind: Generator, f: impl Fn(&mut GeneratorDesc)) -> MaskEffectKind {
    let mut g = GeneratorDesc::of(kind);
    f(&mut g);
    MaskEffectKind::Generator(g)
}

fn procm(kind: ProcKind, scale: f32, f: impl Fn(&mut ProcParams)) -> MaskEffectKind {
    let mut k = MaskEffectKind::fill_procedural(kind);
    if let MaskEffectKind::Fill { procedural, projection, .. } = &mut k {
        projection.scale = scale;
        f(procedural);
    }
    k
}

fn mask(base: f32, effects: Vec<MaskEffect>) -> MaskDesc {
    MaskDesc { enabled: true, base, effects }
}

pub struct SmartMaskPreset {
    pub name: &'static str,
    pub mask: MaskDesc,
}

pub fn smart_masks() -> Vec<SmartMaskPreset> {
    use MaskBlend::*;
    let s = |name, mask| SmartMaskPreset { name, mask };
    vec![
        s("Edge wear", mask(0.0, vec![effect(Normal, 1.0, gen(Generator::EdgeWear, |_| {}))])),
        s("Edge wear strong", mask(0.0, vec![effect(Normal, 1.0, gen(Generator::EdgeWear, |g| {
            g.level = 0.15;
            g.noise = 0.7;
        }))])),
        s("Cavity dirt", mask(0.0, vec![effect(Normal, 1.0, gen(Generator::Cavity, |_| {}))])),
        s("Ambient dirt", mask(0.0, vec![effect(Normal, 1.0, gen(Generator::Dirt, |_| {}))])),
        s("Dust from above", mask(0.0, vec![
            effect(Normal, 1.0, gen(Generator::Light, |g| {
                g.dir = [0.0, 1.0, 0.0];
                g.level = 0.75;
                g.contrast = 3.0;
            })),
            effect(Multiply, 1.0, procm(ProcKind::Grunge, 5.0, |_| {})),
        ])),
        s("Ground dirt", mask(0.0, vec![effect(Normal, 1.0, gen(Generator::PositionGradient, |g| {
            g.axis = 1;
            g.invert = true;
            g.level = 0.75;
            g.contrast = 3.0;
            g.noise = 0.25;
        }))])),
        s("Rust spots", mask(0.0, vec![
            effect(Normal, 1.0, procm(ProcKind::Rust, 5.0, |_| {})),
            effect(Add, 0.6, gen(Generator::Dirt, |_| {})),
        ])),
        s("Grunge", mask(0.0, vec![effect(Normal, 1.0, procm(ProcKind::Grunge, 4.0, |_| {}))])),
        s("Scratches", mask(0.0, vec![effect(Normal, 1.0, procm(ProcKind::Scratches, 6.0, |_| {}))])),
        s("Vertical drips", mask(0.0, vec![
            effect(Normal, 1.0, procm(ProcKind::Fibers, 6.0, |p| {
                p.aspect = 40.0;
                p.contrast = 2.5;
            })),
            effect(Multiply, 1.0, gen(Generator::AmbientOcclusion, |g| g.level = 0.1)),
        ])),
        s("Peeling", mask(1.0, vec![
            effect(Subtract, 1.0, procm(ProcKind::Cells, 3.0, |p| {
                p.contrast = 6.0;
                p.balance = 0.7;
            })),
            effect(Subtract, 1.0, gen(Generator::EdgeWear, |_| {})),
        ])),
    ]
}

pub fn smart_mask(name: &str) -> MaskDesc {
    smart_masks().into_iter().find(|m| m.name == name).map(|m| m.mask).unwrap_or_default()
}

fn fill_layer(name: &str, f: FillDesc, m: Option<MaskDesc>) -> LayerTree {
    let mut d = LayerDesc::new(0, name.into(), LayerKind::Fill(Box::new(f)));
    d.mask = m;
    LayerTree { desc: d, children: Vec::new() }
}

fn folder(name: &str, children: Vec<LayerTree>) -> LayerTree {
    let mut d = LayerDesc::new(0, name.into(), LayerKind::Folder);
    d.expanded = false;
    LayerTree { desc: d, children }
}

fn paint_over(color: [f32; 3], rough: f32, metal: f32) -> FillDesc {
    fill(uni(color), gray(rough), gray(metal))
}

pub struct ProceduralSmartMaterial {
    pub name: &'static str,
    pub tree: LayerTree,
}

/// Procedural layer stacks (no texture files): the self test and the
/// benchmark use them; the shelf shows `smart_materials`.
pub fn procedural_smart_materials() -> Vec<ProceduralSmartMaterial> {
    use MaskBlend::*;
    let sm = |name, tree| ProceduralSmartMaterial { name, tree };
    vec![
        sm("Steel painted worn", folder("Steel painted worn", vec![
            fill_layer("Steel", material("Steel polished"), None),
            fill_layer("Paint", paint_over([0.35, 0.02, 0.01], 0.45, 0.0), Some(mask(1.0, vec![
                effect(Subtract, 1.0, gen(Generator::EdgeWear, |_| {})),
            ]))),
            fill_layer("Dirt", paint_over([0.05, 0.04, 0.03], 0.9, 0.0), Some(smart_mask("Ambient dirt"))),
        ])),
        sm("Iron rusted", folder("Iron rusted", vec![
            fill_layer("Iron", material("Iron raw"), None),
            fill_layer("Rust", material("Rust"), Some(smart_mask("Rust spots"))),
        ])),
        sm("Brass tarnished", folder("Brass tarnished", vec![
            fill_layer("Brass", material("Brass"), None),
            fill_layer("Tarnish", paint_over([0.16, 0.12, 0.06], 0.7, 0.4), Some(smart_mask("Cavity dirt"))),
            fill_layer("Polish", paint_over([0.95, 0.82, 0.5], 0.15, 1.0), Some(mask(0.0, vec![
                effect(Normal, 1.0, gen(Generator::Edges, |g| g.contrast = 8.0)),
            ]))),
        ])),
        sm("Iron forged dusty", folder("Iron forged dusty", vec![
            fill_layer("Iron", material("Iron forged"), None),
            fill_layer("Dust", paint_over([0.55, 0.5, 0.44], 0.95, 0.0), Some(smart_mask("Dust from above"))),
        ])),
        sm("Concrete dirty", folder("Concrete dirty", vec![
            fill_layer("Concrete", material("Concrete smooth"), None),
            fill_layer("Dirt", paint_over([0.12, 0.1, 0.08], 0.95, 0.0), Some(smart_mask("Ground dirt"))),
            fill_layer("Stains", paint_over([0.2, 0.19, 0.16], 0.9, 0.0), Some(smart_mask("Grunge"))),
        ])),
        sm("Wood varnished worn", folder("Wood varnished worn", vec![
            fill_layer("Wood", material("Wood walnut"), None),
            fill_layer("Varnish wear", paint_over([0.3, 0.2, 0.12], 0.75, 0.0), Some(smart_mask("Edge wear"))),
        ])),
        sm("Leather aged", folder("Leather aged", vec![
            fill_layer("Leather", material("Leather"), None),
            fill_layer("Creases", paint_over([0.08, 0.05, 0.03], 0.85, 0.0), Some(smart_mask("Cavity dirt"))),
            fill_layer("Wear", paint_over([0.45, 0.32, 0.2], 0.9, 0.0), Some(smart_mask("Edge wear strong"))),
        ])),
        sm("Plastic scuffed", folder("Plastic scuffed", vec![
            fill_layer("Plastic", material("Plastic glossy black"), None),
            fill_layer("Scuffs", paint_over([0.12, 0.12, 0.12], 0.7, 0.0), Some(smart_mask("Edge wear"))),
            fill_layer("Dust", paint_over([0.4, 0.38, 0.35], 0.95, 0.0), Some(smart_mask("Dust from above"))),
        ])),
        sm("Gold worn", folder("Gold worn", vec![
            fill_layer("Gold", material("Gold"), None),
            fill_layer("Dirt", paint_over([0.06, 0.04, 0.02], 0.8, 0.0), Some(smart_mask("Cavity dirt"))),
        ])),
        sm("Copper oxidized", folder("Copper oxidized", vec![
            fill_layer("Copper", material("Copper"), None),
            fill_layer("Patina", paint_over([0.12, 0.35, 0.27], 0.8, 0.0), Some(mask(0.0, vec![
                effect(Normal, 1.0, gen(Generator::Dirt, |g| g.noise = 0.5)),
                effect(Multiply, 1.0, procm(ProcKind::Clouds, 4.0, |p| p.contrast = 2.0)),
            ]))),
        ])),
        sm("Bronze aged", folder("Bronze aged", vec![
            fill_layer("Bronze", material("Bronze"), None),
            fill_layer("Patina", paint_over([0.1, 0.25, 0.18], 0.85, 0.0), Some(smart_mask("Cavity dirt"))),
        ])),
        sm("Aluminium scratched", folder("Aluminium scratched", vec![
            fill_layer("Aluminium", material("Aluminium"), None),
            fill_layer("Scratches", paint_over([0.95, 0.95, 0.95], 0.12, 1.0), Some(smart_mask("Scratches"))),
        ])),
        sm("Plastic dusty", folder("Plastic dusty", vec![
            fill_layer("Plastic", material("Plastic matte gray"), None),
            fill_layer("Dust", paint_over([0.4, 0.38, 0.34], 1.0, 0.0), Some(smart_mask("Dust from above"))),
        ])),
        sm("Military olive worn", folder("Military olive worn", vec![
            fill_layer("Metal", material("Gunmetal"), None),
            fill_layer("Olive paint", paint_over([0.08, 0.09, 0.03], 0.7, 0.0), Some(mask(1.0, vec![
                effect(Subtract, 1.0, gen(Generator::EdgeWear, |_| {})),
            ]))),
            fill_layer("Mud", material("Mud"), Some(smart_mask("Ground dirt"))),
        ])),
        sm("Wood painted chipped", folder("Wood painted chipped", vec![
            fill_layer("Wood", material("Wood oak"), None),
            fill_layer("White paint", paint_over([0.7, 0.7, 0.68], 0.5, 0.0), Some(smart_mask("Peeling"))),
        ])),
        sm("Leather worn", folder("Leather worn", vec![
            fill_layer("Leather", material("Leather"), None),
            fill_layer("Wear", paint_over([0.35, 0.18, 0.08], 0.45, 0.0), Some(smart_mask("Edge wear"))),
        ])),
        sm("Concrete stained", folder("Concrete stained", vec![
            fill_layer("Concrete", material("Concrete"), None),
            fill_layer("Stains", paint_over([0.12, 0.11, 0.1], 0.95, 0.0), Some(smart_mask("Vertical drips"))),
        ])),
        sm("Rubber dirty", folder("Rubber dirty", vec![
            fill_layer("Rubber", material("Rubber"), None),
            fill_layer("Dust", paint_over([0.3, 0.28, 0.25], 1.0, 0.0), Some(smart_mask("Dust from above"))),
        ])),
        sm("Car paint chipped", folder("Car paint chipped", vec![
            fill_layer("Primer", paint_over([0.35, 0.35, 0.33], 0.7, 0.0), None),
            fill_layer("Car paint", material("Car paint blue"), Some(mask(1.0, vec![
                effect(Subtract, 1.0, gen(Generator::EdgeWear, |g| g.noise = 0.7)),
            ]))),
        ])),
    ]
}

// ---------------------------------------------------------------- starter smart materials

/// A starter smart material: scanned texture materials with generator and
/// grunge masks. The tree reads `textures` (absolute paths) through
/// placeholder resource ids, replaced by project resources when applied.
pub struct SmartMaterialPreset {
    pub name: String,
    pub tree: LayerTree,
    pub textures: Vec<(u64, PathBuf)>,
}

/// First placeholder resource id of a starter smart material.
const PLACEHOLDER: u64 = 1 << 48;

struct Recipe<'a> {
    mats: &'a [TexMaterial],
    textures: Vec<(u64, PathBuf)>,
    missing: bool,
}

impl Recipe<'_> {
    fn file(&mut self, path: PathBuf) -> u64 {
        if let Some((id, _)) = self.textures.iter().find(|(_, p)| *p == path) {
            return *id;
        }
        let id = PLACEHOLDER + self.textures.len() as u64;
        self.textures.push((id, path));
        id
    }

    /// The fill of a starter texture material, by name.
    fn mat(&mut self, name: &str) -> FillDesc {
        let Some(m) = self.mats.iter().find(|m| m.name == name).cloned() else {
            self.missing = true;
            return FillDesc::default();
        };
        let mut ids = std::collections::HashMap::new();
        for (stem, _) in TEX_MATERIAL_MAPS {
            if let Some(p) = m.map(stem) {
                ids.insert(stem, self.file(p));
            }
        }
        m.fill(&ids)
    }

    /// A mask fill from a starter grayscale texture (`textures/<rel>.png`)
    /// tiled `scale` times over the UVs; a procedural grunge without it.
    fn grunge(&mut self, rel: &str, scale: f32) -> MaskEffectKind {
        let path = starter_dir().map(|d| d.join("textures").join(format!("{rel}.png"))).filter(|p| p.is_file());
        match path {
            Some(p) => MaskEffectKind::Fill {
                value: 1.0,
                source: SourceKind::Texture,
                texture: Some(self.file(p)),
                procedural: ProcParams::of(ProcKind::Grunge),
                projection: Projection { mode: ProjMode::Uv, scale, ..Default::default() },
                range: [0.0, 1.0],
            },
            None => procm(ProcKind::Grunge, 4.0, |_| {}),
        }
    }
}

/// The lowest part of the object (mud, moss from the ground).
fn low_band(level: f32) -> MaskEffectKind {
    gen(Generator::PositionGradient, |g| {
        g.axis = 1;
        g.invert = true;
        g.level = level;
        g.contrast = 4.0;
        g.noise = 0.3;
    })
}

/// Multiplies a fill's color (texture maps included).
fn tinted(mut f: FillDesc, rgb: [f32; 3]) -> FillDesc {
    f.channels[CH_COLOR].value = rgb;
    f
}

fn levels(lo: f32, hi: f32) -> MaskEffectKind {
    MaskEffectKind::Levels { in_lo: lo, in_hi: hi, gamma: 1.0, out_lo: 0.0, out_hi: 1.0 }
}

fn up_light(level: f32, contrast: f32) -> MaskEffectKind {
    gen(Generator::Light, |g| {
        g.dir = [0.0, 1.0, 0.0];
        g.level = level;
        g.contrast = contrast;
    })
}

/// An sRGB color picked by eye, as the linear value fills take.
fn lin(c: [f32; 3]) -> [f32; 3] {
    c.map(|v| v.powf(2.2))
}

/// A fill that only changes the color (hand-painted shading layers).
fn tint(rgb: [f32; 3]) -> FillDesc {
    let mut f = paint_over(rgb, 0.5, 0.0);
    f.channels[CH_ROUGH].enabled = false;
    f.channels[CH_METAL].enabled = false;
    f
}

/// A fill that only changes the height (relief).
fn relief(h: FillChannel) -> FillDesc {
    let mut f = with_height(tint([0.0; 3]), h);
    f.channels[CH_COLOR].enabled = false;
    f
}

/// Glowing paint: the color, emissive too.
fn glowing(rgb: [f32; 3]) -> FillDesc {
    let mut f = tint(rgb);
    f.channels[CH_EMISSIVE] = uni(rgb);
    f
}

/// A layer at `opacity`.
fn faded(mut l: LayerTree, opacity: f32) -> LayerTree {
    l.desc.opacity = [opacity; NCH];
    l
}

/// A procedural pattern as a mask, tri-planar (planar patterns take each
/// face's plane), `scale` across the object.
fn pat(kind: ProcKind, scale: f32, seed: f32, f: impl Fn(&mut ProcParams)) -> MaskEffectKind {
    let mut k = procm(kind, scale, |p| {
        p.seed = seed;
        f(p);
    });
    if let MaskEffectKind::Fill { projection, .. } = &mut k {
        projection.mode = ProjMode::Triplanar;
    }
    k
}

/// Brush strokes as a mask.
fn strokes(scale: f32, seed: f32) -> MaskEffectKind {
    pat(ProcKind::BrushStrokes, scale, seed, |p| {
        p.density = 0.5;
        p.roughness = 0.6;
    })
}

/// A pattern as a fill channel ramping `a` to `b` (linear colors).
fn pat_ch(kind: ProcKind, scale: f32, a: [f32; 3], b: [f32; 3], f: impl Fn(&mut ProcParams)) -> (FillChannel, Projection) {
    let mut c = procc(kind, a, b);
    f(&mut c.procedural);
    (c, Projection { mode: ProjMode::Triplanar, scale, ..Default::default() })
}

/// Levels flipped (white below `lo`, black above `hi`).
fn inv_levels(lo: f32, hi: f32) -> MaskEffectKind {
    MaskEffectKind::Levels { in_lo: lo, in_hi: hi, gamma: 1.0, out_lo: 1.0, out_hi: 0.0 }
}

/// A base painted in visible brush strokes mixing two colors (sRGB).
fn painted_base(name: &str, a: [f32; 3], b: [f32; 3], rough: f32, metal: f32, scale: f32) -> LayerTree {
    let (c, proj) = pat_ch(ProcKind::BrushStrokes, scale, lin(a), lin(b), |p| {
        p.density = 0.5;
        p.roughness = 0.6;
    });
    let mut f = fill(c, gray(rough), gray(metal));
    f.projection = proj;
    fill_layer(name, f, None)
}

/// Light and shade painted into the color: a soft cool shadow on the sides
/// turned away from the light and warm light on top, following the form,
/// their border nudged by strokes.
fn painted_light(shadow: [f32; 3], light: [f32; 3]) -> Vec<LayerTree> {
    use MaskBlend::*;
    let lit = |invert: bool| {
        gen(Generator::Light, move |g| {
            g.dir = [0.25, 1.0, 0.35];
            g.invert = invert;
            g.level = 0.5;
            g.contrast = 1.6;
        })
    };
    vec![
        faded(fill_layer("Cool shadow", tint(lin(shadow)), Some(mask(0.0, vec![
            effect(Normal, 1.0, lit(true)),
            effect(Overlay, 0.25, strokes(5.0, 3.0)),
            effect(Normal, 1.0, levels(0.4, 0.85)),
        ]))), 0.7),
        faded(fill_layer("Warm light", tint(lin(light)), Some(mask(0.0, vec![
            effect(Normal, 1.0, lit(false)),
            effect(Overlay, 0.25, strokes(5.0, 5.0)),
            effect(Normal, 1.0, levels(0.55, 0.95)),
        ]))), 0.5),
    ]
}

/// Dark color in the crevices.
fn crevices(rgb: [f32; 3], opacity: f32) -> LayerTree {
    use MaskBlend::*;
    faded(fill_layer("Crevices", tint(lin(rgb)), Some(mask(0.0, vec![
        effect(Normal, 1.0, gen(Generator::Cavity, |g| {
            g.level = 0.15;
            g.contrast = 6.0;
        })),
        effect(Add, 0.4, gen(Generator::AmbientOcclusion, |g| g.contrast = 5.0)),
    ]))), opacity)
}

/// Light caught by the edges in broken strokes.
fn edge_light(rgb: [f32; 3], opacity: f32) -> LayerTree {
    use MaskBlend::*;
    faded(fill_layer("Edge light", tint(lin(rgb)), Some(mask(0.0, vec![
        effect(Normal, 1.0, gen(Generator::Edges, |g| {
            g.level = 0.62;
            g.contrast = 10.0;
        })),
        effect(Multiply, 1.0, strokes(10.0, 7.0)),
        effect(Normal, 1.0, levels(0.12, 0.3)),
    ]))), opacity)
}

/// The "Hand-painted ..." smart materials (no files): the look of painted
/// animated films set in a brass city and its undercity. Each has its own
/// build (engraving, chipped paint over rust, graffiti, facets...), over a
/// base in brush strokes with light and shade painted in.
fn hand_painted_smart_materials(sm: &mut dyn FnMut(&str, Vec<LayerTree>)) {
    use MaskBlend::*;
    use ProcKind as K;
    let mut out = |name: &str, mut layers: Vec<LayerTree>, shadow: [f32; 3], light: [f32; 3]| {
        // Light and shade go under the crevices and edges (the last two).
        let n = layers.len().saturating_sub(2);
        let tail = layers.split_off(n);
        layers.extend(painted_light(shadow, light));
        layers.extend(tail);
        sm(name, layers);
    };

    // Polished brass with engraved hexagon ornaments, tarnish in the
    // recesses and painted glints.
    out(
        "Hand-painted gilded brass",
        vec![
            painted_base("Brass", [0.6, 0.4, 0.13], [0.86, 0.64, 0.26], 0.4, 0.6, 5.0),
            fill_layer("Engraving", with_height(tint(lin([0.3, 0.15, 0.04])), gray(-0.08)), Some(mask(0.0, vec![
                effect(Normal, 1.0, pat(K::Hexagons, 2.5, 0.0, |p| p.density = 0.25)),
                effect(Normal, 1.0, inv_levels(0.3, 0.6)),
            ]))),
            faded(fill_layer("Tarnish", tint(lin([0.22, 0.16, 0.05])), Some(mask(0.0, vec![
                effect(Normal, 1.0, gen(Generator::Dirt, |g| g.noise = 0.4)),
                effect(Multiply, 1.0, pat(K::Grunge, 3.0, 2.0, |_| {})),
            ]))), 0.8),
            fill_layer("Glints", tint(lin([1.0, 0.95, 0.72])), Some(mask(0.0, vec![
                effect(Normal, 1.0, gen(Generator::Light, |g| {
                    g.dir = [0.5, 0.8, 0.6];
                    g.level = 0.85;
                    g.contrast = 5.0;
                })),
                effect(Multiply, 1.0, strokes(8.0, 11.0)),
                effect(Normal, 1.0, levels(0.25, 0.45)),
            ]))),
            crevices([0.2, 0.08, 0.03], 0.85),
            edge_light([1.0, 0.94, 0.7], 0.9),
        ],
        [0.32, 0.2, 0.26],
        [1.0, 0.85, 0.5],
    );

    // Undercity iron: teal paint chipped off, rust blooming through it and
    // running down in streaks.
    let mut rust = paint_over(lin([0.55, 0.24, 0.08]), 0.95, 0.0);
    rust.channels[CH_HEIGHT] = gray(0.03);
    out(
        "Hand-painted rusted plate",
        vec![
            painted_base("Iron", [0.17, 0.18, 0.2], [0.3, 0.3, 0.32], 0.7, 0.4, 5.0),
            fill_layer("Teal paint", paint_over(lin([0.12, 0.42, 0.42]), 0.6, 0.0), Some(mask(1.0, vec![
                effect(Subtract, 1.0, gen(Generator::EdgeWear, |g| {
                    g.level = 0.25;
                    g.noise = 0.7;
                })),
                effect(Subtract, 0.9, pat(K::Rust, 2.5, 1.0, |_| {})),
            ]))),
            fill_layer("Rust", rust, Some(mask(0.0, vec![
                effect(Normal, 1.0, pat(K::Rust, 3.5, 4.0, |_| {})),
                effect(Add, 0.6, gen(Generator::Dirt, |_| {})),
                effect(Normal, 1.0, levels(0.45, 0.55)),
            ]))),
            faded(fill_layer("Rust streaks", tint(lin([0.42, 0.18, 0.06])), Some(mask(0.0, vec![
                effect(Normal, 1.0, pat(K::Streaks, 2.0, 2.0, |p| p.density = 0.6)),
            ]))), 0.8),
            crevices([0.06, 0.04, 0.03], 0.9),
            edge_light([0.78, 0.8, 0.78], 0.7),
        ],
        [0.1, 0.12, 0.2],
        [0.7, 0.62, 0.5],
    );

    // A chem pipe: copper, verdigris running down, glowing green pockets.
    out(
        "Hand-painted chem copper",
        vec![
            painted_base("Copper", [0.52, 0.23, 0.11], [0.8, 0.42, 0.22], 0.45, 0.5, 5.0),
            fill_layer("Verdigris", paint_over(lin([0.3, 0.66, 0.55]), 0.9, 0.0), Some(mask(0.0, vec![
                effect(Normal, 1.0, gen(Generator::Dirt, |g| g.noise = 0.5)),
                effect(Add, 0.8, pat(K::Streaks, 2.5, 6.0, |p| p.density = 0.8)),
                effect(Overlay, 0.5, strokes(6.0, 13.0)),
                effect(Normal, 1.0, levels(0.45, 0.55)),
            ]))),
            fill_layer("Chem glow", glowing(lin([0.5, 1.0, 0.2])), Some(mask(0.0, vec![
                effect(Normal, 1.0, pat(K::Rust, 3.0, 9.0, |_| {})),
                effect(Multiply, 1.0, gen(Generator::Dirt, |g| g.level = 0.2)),
                effect(Normal, 1.0, levels(0.35, 0.45)),
            ]))),
            crevices([0.12, 0.05, 0.02], 0.85),
            edge_light([1.0, 0.75, 0.5], 0.85),
        ],
        [0.2, 0.1, 0.2],
        [1.0, 0.7, 0.45],
    );

    // An undercity wall: bricks in deep mortar, neon spray paint splashed
    // over them, grime rising from the ground.
    let mut mortar = paint_over(lin([0.3, 0.28, 0.28]), 0.95, 0.0);
    mortar.channels[CH_HEIGHT] = gray(-0.1);
    out(
        "Hand-painted graffiti bricks",
        vec![
            painted_base("Brick", [0.42, 0.18, 0.12], [0.62, 0.3, 0.2], 0.85, 0.0, 6.0),
            faded(fill_layer("Brick tones", tint(lin([0.72, 0.42, 0.28])), Some(mask(0.0, vec![
                effect(Normal, 1.0, pat(K::Cells, 3.0, 1.0, |_| {})),
                effect(Normal, 1.0, levels(0.6, 0.62)),
            ]))), 0.6),
            fill_layer("Mortar", mortar, Some(mask(0.0, vec![
                effect(Normal, 1.0, pat(K::Bricks, 3.0, 0.0, |p| p.density = 0.3)),
                effect(Normal, 1.0, inv_levels(0.3, 0.6)),
            ]))),
            fill_layer("Pink spray", paint_over(lin([1.0, 0.18, 0.62]), 0.5, 0.0), Some(mask(0.0, vec![effect(Normal, 1.0, pat(K::Splatter, 2.0, 1.0, |p| p.density = 0.35))]))),
            fill_layer("Cyan spray", paint_over(lin([0.1, 0.85, 0.95]), 0.5, 0.0), Some(mask(0.0, vec![effect(Normal, 1.0, pat(K::Splatter, 2.6, 7.0, |p| p.density = 0.3))]))),
            faded(fill_layer("Grime", tint(lin([0.1, 0.08, 0.08])), Some(mask(0.0, vec![
                effect(Normal, 1.0, low_band(0.3)),
                effect(Overlay, 0.6, strokes(6.0, 17.0)),
            ]))), 0.8),
            crevices([0.08, 0.04, 0.05], 0.85),
            edge_light([0.95, 0.75, 0.6], 0.7),
        ],
        [0.22, 0.1, 0.22],
        [0.98, 0.66, 0.46],
    );

    // A magic crystal: flat facets in blues, glowing veins and rims.
    let (c, proj) = pat_ch(K::Cells, 4.0, lin([0.04, 0.12, 0.36]), lin([0.24, 0.6, 0.95]), |_| {});
    let mut facets = fill(c, gray(0.2), gray(0.0));
    facets.projection = proj;
    out(
        "Hand-painted hextech crystal",
        vec![
            fill_layer("Facets", facets, None),
            faded(fill_layer("Brushwork", tint(lin([0.5, 0.85, 1.0])), Some(mask(0.0, vec![
                effect(Normal, 1.0, strokes(6.0, 2.0)),
                effect(Normal, 1.0, levels(0.75, 0.8)),
            ]))), 0.4),
            fill_layer("Energy veins", glowing(lin([0.4, 0.95, 1.0])), Some(mask(0.0, vec![
                effect(Normal, 1.0, pat(K::Cracks, 4.0, 0.0, |p| p.density = 0.1)),
                effect(Normal, 1.0, levels(0.5, 0.7)),
            ]))),
            fill_layer("Glowing rims", glowing(lin([0.7, 1.0, 1.0])), Some(mask(0.0, vec![effect(Normal, 0.9, gen(Generator::Edges, |g| {
                g.level = 0.4;
                g.contrast = 6.0;
            }))]))),
            crevices([0.02, 0.02, 0.15], 0.9),
            edge_light([0.85, 1.0, 1.0], 0.6),
        ],
        [0.05, 0.03, 0.28],
        [0.6, 0.9, 1.0],
    );

    // A vial of glowing purple: swirls, rising bubbles.
    let (c, proj) = pat_ch(K::Marble, 2.0, lin([0.16, 0.03, 0.28]), lin([0.55, 0.12, 0.62]), |_| {});
    let mut glass = fill(c, gray(0.25), gray(0.0));
    glass.projection = proj;
    out(
        "Hand-painted shimmer glass",
        vec![
            fill_layer("Glass", glass, None),
            fill_layer("Swirl glow", glowing(lin([1.0, 0.3, 0.88])), Some(mask(0.0, vec![
                effect(Normal, 1.0, pat(K::Marble, 2.0, 4.0, |_| {})),
                effect(Normal, 1.0, levels(0.85, 0.92)),
            ]))),
            fill_layer("Bubbles", glowing(lin([1.0, 0.78, 1.0])), Some(mask(0.0, vec![effect(Normal, 1.0, pat(K::Dots, 7.0, 3.0, |p| p.density = 0.1))]))),
            crevices([0.06, 0.0, 0.12], 0.8),
            edge_light([1.0, 0.82, 1.0], 0.85),
        ],
        [0.1, 0.03, 0.25],
        [0.95, 0.6, 1.0],
    );

    // Blue enforcer armor: hammered steel, scratches, bright worn edges.
    let (h, proj) = pat_ch(K::Cells, 6.0, [-0.03; 3], [0.03; 3], |_| {});
    let mut dents = relief(h);
    dents.projection = proj;
    out(
        "Hand-painted enforcer steel",
        vec![
            painted_base("Steel", [0.28, 0.34, 0.45], [0.45, 0.52, 0.63], 0.5, 0.5, 5.0),
            fill_layer("Hammered dents", dents, None),
            faded(fill_layer("Scratches", tint(lin([0.85, 0.88, 0.93])), Some(mask(0.0, vec![effect(Normal, 1.0, pat(K::Scratches, 3.0, 5.0, |p| p.density = 0.3))]))), 0.8),
            faded(fill_layer("Grime", tint(lin([0.1, 0.1, 0.12])), Some(mask(0.0, vec![
                effect(Normal, 1.0, gen(Generator::Dirt, |_| {})),
                effect(Multiply, 1.0, pat(K::Grunge, 3.0, 8.0, |_| {})),
            ]))), 0.7),
            crevices([0.04, 0.05, 0.12], 0.9),
            edge_light([0.95, 0.98, 1.0], 1.0),
        ],
        [0.12, 0.13, 0.3],
        [0.85, 0.86, 0.82],
    );

    // A worn leather: pores, creases, scuffs, edges rubbed lighter.
    let (h, proj) = pat_ch(K::Leather, 14.0, [-0.025; 3], [0.02; 3], |_| {});
    let mut grain = relief(h);
    grain.projection = proj;
    let mut crease = tint(lin([0.14, 0.06, 0.03]));
    crease.channels[CH_HEIGHT] = gray(-0.04);
    out(
        "Hand-painted worn leather",
        vec![
            painted_base("Leather", [0.3, 0.15, 0.07], [0.46, 0.25, 0.12], 0.65, 0.0, 5.0),
            fill_layer("Grain", grain, None),
            faded(fill_layer("Creases", crease, Some(mask(0.0, vec![
                effect(Normal, 1.0, pat(K::Ridged, 2.5, 3.0, |p| p.contrast = 1.5)),
                effect(Normal, 1.0, levels(0.72, 0.82)),
            ]))), 0.75),
            fill_layer("Rubbed edges", tint(lin([0.72, 0.5, 0.3])), Some(mask(0.0, vec![effect(Normal, 0.85, gen(Generator::EdgeWear, |g| g.noise = 0.6))]))),
            faded(fill_layer("Scuffs", tint(lin([0.12, 0.05, 0.03])), Some(mask(0.0, vec![effect(Normal, 1.0, pat(K::Scratches, 2.0, 9.0, |p| p.density = 0.5))]))), 0.6),
            crevices([0.08, 0.03, 0.02], 0.85),
            edge_light([0.92, 0.7, 0.48], 0.5),
        ],
        [0.16, 0.07, 0.14],
        [0.8, 0.52, 0.3],
    );

    // A wine coat cloth: weave, faded folds, dust.
    let (h, proj) = pat_ch(K::Fabric, 40.0, [-0.02; 3], [0.02; 3], |_| {});
    let mut weave = relief(h);
    weave.projection = proj;
    out(
        "Hand-painted coat cloth",
        vec![
            painted_base("Cloth", [0.3, 0.05, 0.1], [0.46, 0.09, 0.15], 0.95, 0.0, 4.0),
            fill_layer("Weave", weave, None),
            faded(fill_layer("Weave shade", tint(lin([0.18, 0.02, 0.06])), Some(mask(0.0, vec![
                effect(Normal, 1.0, pat(K::Fabric, 40.0, 0.0, |_| {})),
                effect(Normal, 1.0, inv_levels(0.3, 0.5)),
            ]))), 0.5),
            faded(fill_layer("Faded", tint(lin([0.62, 0.3, 0.34])), Some(mask(0.0, vec![
                effect(Normal, 1.0, gen(Generator::Edges, |g| g.level = 0.3)),
                effect(Multiply, 1.0, strokes(7.0, 4.0)),
                effect(Normal, 1.0, levels(0.2, 0.4)),
            ]))), 0.7),
            faded(fill_layer("Dust", tint(lin([0.55, 0.48, 0.42])), Some(mask(0.0, vec![
                effect(Normal, 1.0, up_light(0.75, 4.0)),
                effect(Multiply, 1.0, pat(K::Grunge, 4.0, 3.0, |_| {})),
            ]))), 0.45),
            crevices([0.1, 0.0, 0.04], 0.85),
            edge_light([1.0, 0.62, 0.55], 0.4),
        ],
        [0.14, 0.02, 0.16],
        [0.95, 0.5, 0.36],
    );

    // Undercity cobblestones: stones of mixed tones in dark gaps, moss.
    let mut gaps = tint(lin([0.07, 0.06, 0.07]));
    gaps.channels[CH_HEIGHT] = gray(-0.12);
    out(
        "Hand-painted cobblestone",
        vec![
            painted_base("Stone", [0.3, 0.3, 0.33], [0.48, 0.47, 0.46], 0.85, 0.0, 5.0),
            faded(fill_layer("Warm stones", tint(lin([0.52, 0.44, 0.34])), Some(mask(0.0, vec![
                effect(Normal, 1.0, pat(K::Cells, 3.0, 0.0, |_| {})),
                effect(Normal, 1.0, levels(0.62, 0.64)),
            ]))), 0.8),
            faded(fill_layer("Cool stones", tint(lin([0.26, 0.31, 0.36])), Some(mask(0.0, vec![
                effect(Normal, 1.0, pat(K::Cells, 3.0, 0.0, |_| {})),
                effect(Normal, 1.0, inv_levels(0.3, 0.32)),
            ]))), 0.8),
            fill_layer("Gaps", gaps, Some(mask(0.0, vec![
                effect(Normal, 1.0, pat(K::Cracks, 3.0, 0.0, |p| p.density = 0.3)),
                effect(Normal, 1.0, levels(0.4, 0.6)),
            ]))),
            fill_layer("Moss", paint_over(lin([0.3, 0.48, 0.13]), 0.95, 0.0), Some(mask(0.0, vec![
                effect(Normal, 1.0, up_light(0.7, 3.0)),
                effect(Multiply, 1.0, pat(K::Grunge, 3.0, 5.0, |_| {})),
                effect(Add, 0.5, pat(K::Cracks, 3.0, 0.0, |p| p.density = 0.5)),
                effect(Multiply, 1.0, pat(K::Clouds, 1.5, 6.0, |p| p.contrast = 2.0)),
                effect(Normal, 1.0, levels(0.35, 0.45)),
            ]))),
            crevices([0.05, 0.05, 0.08], 0.85),
            edge_light([0.88, 0.86, 0.78], 0.7),
        ],
        [0.15, 0.16, 0.3],
        [0.85, 0.8, 0.66],
    );

    // Weathered planks: long boards in dark seams, grain, old blue-gray
    // paint left in patches.
    let mut seams = tint(lin([0.07, 0.035, 0.02]));
    seams.channels[CH_HEIGHT] = gray(-0.1);
    out(
        "Hand-painted weathered planks",
        vec![
            painted_base("Wood", [0.36, 0.2, 0.1], [0.55, 0.34, 0.17], 0.8, 0.0, 5.0),
            faded(fill_layer("Grain", tint(lin([0.22, 0.1, 0.05])), Some(mask(0.0, vec![
                effect(Normal, 1.0, pat(K::Fibers, 3.0, 0.0, |p| p.contrast = 2.0)),
                effect(Normal, 1.0, levels(0.58, 0.64)),
            ]))), 0.85),
            fill_layer("Old paint", paint_over(lin([0.36, 0.47, 0.52]), 0.7, 0.0), Some(mask(0.0, vec![
                effect(Normal, 1.0, pat(K::Grunge, 2.0, 4.0, |_| {})),
                effect(Subtract, 1.0, gen(Generator::EdgeWear, |g| g.noise = 0.6)),
                effect(Normal, 1.0, levels(0.45, 0.5)),
            ]))),
            fill_layer("Seams", seams, Some(mask(0.0, vec![
                effect(Normal, 1.0, pat(K::Bricks, 3.0, 0.0, |p| {
                    p.density = 0.15;
                    p.aspect = 0.25;
                })),
                effect(Normal, 1.0, inv_levels(0.3, 0.6)),
            ]))),
            crevices([0.08, 0.03, 0.02], 0.85),
            edge_light([0.95, 0.78, 0.55], 0.8),
        ],
        [0.2, 0.1, 0.16],
        [0.95, 0.7, 0.42],
    );

    // Painted skin: blush, red in the creases, freckles, cool shade.
    out(
        "Hand-painted skin",
        vec![
            painted_base("Skin", [0.78, 0.52, 0.42], [0.92, 0.68, 0.56], 0.6, 0.0, 4.0),
            faded(fill_layer("Blush", tint(lin([0.9, 0.42, 0.4])), Some(mask(0.0, vec![
                effect(Normal, 1.0, pat(K::Clouds, 1.2, 2.0, |p| p.contrast = 1.5)),
                effect(Normal, 1.0, levels(0.5, 0.8)),
                effect(Add, 0.6, gen(Generator::Cavity, |g| g.level = 0.1)),
            ]))), 0.55),
            faded(fill_layer("Freckles", tint(lin([0.6, 0.32, 0.2])), Some(mask(0.0, vec![effect(Normal, 1.0, pat(K::Dots, 22.0, 1.0, |p| p.density = 0.05))]))), 0.7),
            crevices([0.5, 0.15, 0.16], 0.7),
            edge_light([1.0, 0.92, 0.84], 0.5),
        ],
        [0.52, 0.32, 0.46],
        [1.0, 0.88, 0.74],
    );
}

/// The starter smart materials, built on the starter texture materials
/// (a recipe whose material is missing is left out).
pub fn smart_materials() -> Vec<SmartMaterialPreset> {
    use MaskBlend::*;
    let mats = texture_materials();
    let mut out = Vec::new();
    let mut sm = |name: &str, build: &dyn Fn(&mut Recipe) -> Vec<LayerTree>| {
        let mut r = Recipe { mats: &mats, textures: Vec::new(), missing: false };
        let layers = build(&mut r);
        if !r.missing {
            out.push(SmartMaterialPreset { name: name.into(), tree: folder(name, layers), textures: r.textures });
        }
    };
    // Metals.
    sm("Steel painted worn", &|r| vec![
        fill_layer("Steel", r.mat("Worn Steel"), None),
        fill_layer("Paint", paint_over([0.32, 0.04, 0.02], 0.42, 0.0), Some(mask(1.0, vec![
            effect(Subtract, 1.0, gen(Generator::EdgeWear, |g| g.noise = 0.6)),
            effect(Multiply, 1.0, r.grunge("Grunges/Grunge Edge Damage", 2.0)),
        ]))),
        fill_layer("Dirt", paint_over([0.05, 0.04, 0.03], 0.9, 0.0), Some(smart_mask("Ambient dirt"))),
    ]);
    sm("Military paint worn", &|r| vec![
        fill_layer("Steel", r.mat("Pitted Steel"), None),
        fill_layer("Olive paint", paint_over([0.09, 0.1, 0.04], 0.65, 0.0), Some(mask(1.0, vec![
            effect(Subtract, 1.0, gen(Generator::EdgeWear, |g| {
                g.level = 0.2;
                g.noise = 0.7;
            })),
            effect(Multiply, 1.0, r.grunge("Grunges/Grunge Paint Peel", 4.0)),
        ]))),
        fill_layer("Mud", r.mat("Brown Soil"), Some(mask(0.0, vec![effect(Normal, 1.0, low_band(0.3))]))),
        fill_layer("Dust", paint_over([0.45, 0.4, 0.33], 0.95, 0.0), Some(smart_mask("Dust from above"))),
    ]);
    sm("Rusty steel", &|r| vec![
        fill_layer("Steel", r.mat("Brushed Steel"), None),
        fill_layer("Rust", r.mat("Rust Coarse 01"), Some(mask(0.0, vec![
            effect(Normal, 1.0, r.grunge("Grunges/Grunge Rust", 2.0)),
            effect(Add, 0.6, gen(Generator::Dirt, |_| {})),
            effect(Add, 0.5, gen(Generator::Cavity, |_| {})),
        ]))),
    ]);
    sm("Cast iron rusty", &|r| vec![
        fill_layer("Iron", r.mat("Pitted Cast Iron"), None),
        fill_layer("Rust", r.mat("Rusted Iron"), Some(mask(0.0, vec![
            effect(Normal, 1.0, gen(Generator::Cavity, |_| {})),
            effect(Add, 0.7, gen(Generator::Dirt, |_| {})),
            effect(Multiply, 0.7, r.grunge("Grunges/Grunge Rust", 1.5)),
        ]))),
    ]);
    sm("Galvanized steel stained", &|r| vec![
        fill_layer("Galvanized", r.mat("Galvanized Steel"), None),
        fill_layer("Rust stains", r.mat("Rusty Metal 02"), Some(mask(0.0, vec![
            effect(Normal, 1.0, r.grunge("Grunges/Grunge Water Stains", 1.5)),
            effect(Add, 0.5, gen(Generator::Cavity, |_| {})),
        ]))),
        fill_layer("Grime", paint_over([0.04, 0.035, 0.03], 0.9, 0.0), Some(smart_mask("Ambient dirt"))),
    ]);
    sm("Copper patina", &|r| vec![
        fill_layer("Copper", r.mat("Polished Copper"), None),
        fill_layer("Patina", r.mat("Copper Patina"), Some(mask(0.0, vec![
            effect(Normal, 1.0, r.grunge("Grunges/Clouds", 1.5)),
            effect(Normal, 1.0, levels(0.45, 0.65)),
            effect(Add, 0.8, gen(Generator::Cavity, |_| {})),
            effect(Add, 0.6, gen(Generator::Dirt, |g| g.noise = 0.5)),
        ]))),
    ]);
    sm("Gold worn", &|r| vec![
        fill_layer("Gold", r.mat("Polished Gold"), None),
        fill_layer("Scratches", r.mat("Scratched Gold"), Some(mask(0.0, vec![effect(Normal, 1.0, r.grunge("Grunges/Grunge Scratches", 2.0))]))),
        fill_layer("Grime", paint_over([0.06, 0.045, 0.02], 0.8, 0.0), Some(smart_mask("Cavity dirt"))),
    ]);
    sm("Silver tarnished", &|r| vec![
        fill_layer("Silver", r.mat("Polished Silver"), None),
        fill_layer("Tarnish", paint_over([0.12, 0.1, 0.08], 0.6, 0.7), Some(mask(0.0, vec![
            effect(Normal, 1.0, r.grunge("Grunges/Clouds", 2.0)),
            effect(Normal, 1.0, levels(0.5, 0.75)),
            effect(Add, 1.0, gen(Generator::Cavity, |_| {})),
            effect(Add, 0.7, gen(Generator::Dirt, |_| {})),
        ]))),
    ]);
    sm("Cobalt oxidized", &|r| vec![
        fill_layer("Oxide", r.mat("Oxidized Cobalt"), None),
        fill_layer("Bare edges", r.mat("Polished Cobalt"), Some(mask(0.0, vec![effect(Normal, 1.0, gen(Generator::EdgeWear, |g| g.level = 0.25))]))),
    ]);
    // Wood, leather, fabric, plastic.
    sm("Wood painted chipped", &|r| vec![
        fill_layer("Planks", r.mat("Weathered Brown Planks"), None),
        fill_layer("Paint", paint_over([0.42, 0.55, 0.6], 0.55, 0.0), Some(mask(1.0, vec![
            effect(Subtract, 1.0, gen(Generator::EdgeWear, |g| g.noise = 0.7)),
            effect(Multiply, 1.0, r.grunge("Grunges/Grunge Paint Peel", 4.0)),
        ]))),
        fill_layer("Dirt", paint_over([0.06, 0.05, 0.035], 0.9, 0.0), Some(smart_mask("Cavity dirt"))),
    ]);
    sm("Wood varnished dusty", &|r| vec![
        fill_layer("Oak", r.mat("Honey Oak Wood"), None),
        fill_layer("Worn varnish", paint_over([0.42, 0.3, 0.18], 0.7, 0.0), Some(smart_mask("Edge wear"))),
        fill_layer("Dust", paint_over([0.5, 0.46, 0.4], 1.0, 0.0), Some(smart_mask("Dust from above"))),
    ]);
    sm("Leather worn", &|r| vec![
        fill_layer("Leather", r.mat("Dark Brown Leather"), None),
        fill_layer("Rubbed", paint_over([0.36, 0.22, 0.12], 0.5, 0.0), Some(smart_mask("Edge wear"))),
        fill_layer("Creases", paint_over([0.04, 0.025, 0.015], 0.85, 0.0), Some(smart_mask("Cavity dirt"))),
    ]);
    sm("Denim faded", &|r| vec![
        fill_layer("Denim", r.mat("Blue Denim"), None),
        fill_layer("Faded", paint_over([0.5, 0.6, 0.72], 0.95, 0.0), Some(mask(0.0, vec![effect(Normal, 0.5, gen(Generator::EdgeWear, |g| {
            g.level = 0.15;
            g.noise = 0.8;
        }))]))),
        fill_layer("Dirt", paint_over([0.12, 0.1, 0.07], 0.95, 0.0), Some(mask(0.0, vec![effect(Normal, 1.0, low_band(0.25))]))),
    ]);
    sm("Plastic scuffed", &|r| vec![
        fill_layer("Plastic", tinted(r.mat("Green Plastic"), [0.3, 0.4, 0.34]), None),
        fill_layer("Scuffs", paint_over([0.2, 0.3, 0.18], 0.7, 0.0), Some(mask(0.0, vec![
            effect(Normal, 1.0, r.grunge("Grunges/Grunge Scratches", 2.0)),
            effect(Add, 0.4, gen(Generator::EdgeWear, |_| {})),
        ]))),
        fill_layer("Dust", paint_over([0.45, 0.42, 0.38], 1.0, 0.0), Some(smart_mask("Dust from above"))),
    ]);
    // Stone and masonry.
    sm("Mossy stone", &|r| vec![
        fill_layer("Rock", r.mat("Gray Rocks"), None),
        fill_layer("Moss", r.mat("Green Moss"), Some(mask(0.0, vec![
            effect(Normal, 1.0, up_light(0.55, 3.0)),
            effect(Multiply, 1.0, r.grunge("Grunges/Clouds", 1.5)),
            effect(Add, 0.5, gen(Generator::Cavity, |_| {})),
        ]))),
    ]);
    sm("Snowy rock", &|r| vec![
        fill_layer("Rock", r.mat("Layered Grey Cliff"), None),
        fill_layer("Snow", r.mat("Snow"), Some(mask(0.0, vec![
            effect(Normal, 1.0, up_light(0.6, 6.0)),
            effect(Multiply, 1.0, r.grunge("Grunges/Clouds Soft", 1.0)),
        ]))),
    ]);
    sm("Concrete leaks dusty", &|r| vec![
        fill_layer("Concrete", r.mat("Smooth Concrete"), None),
        fill_layer("Leaks", paint_over([0.12, 0.11, 0.1], 0.95, 0.0), Some(mask(0.0, vec![
            effect(Normal, 0.8, r.grunge("Grunges/Grunge Leaks", 1.5)),
            effect(Add, 0.5, gen(Generator::Dirt, |_| {})),
        ]))),
        fill_layer("Dust", paint_over([0.6, 0.56, 0.5], 1.0, 0.0), Some(smart_mask("Dust from above"))),
    ]);
    sm("Brick wall grimy", &|r| vec![
        fill_layer("Bricks", r.mat("Red Brick Wall"), None),
        fill_layer("Grime", paint_over([0.06, 0.05, 0.04], 0.95, 0.0), Some(mask(0.0, vec![
            effect(Normal, 1.0, gen(Generator::AmbientOcclusion, |_| {})),
            effect(Add, 0.6, r.grunge("Grunges/Grunge Water Stains", 1.0)),
        ]))),
        fill_layer("Moss", r.mat("Green Moss"), Some(mask(0.0, vec![
            effect(Normal, 1.0, low_band(0.25)),
            effect(Multiply, 1.0, r.grunge("Grunges/Clouds", 1.0)),
            effect(Normal, 1.0, levels(0.2, 0.5)),
        ]))),
    ]);
    hand_painted_smart_materials(&mut |name, layers| out.push(SmartMaterialPreset { name: name.into(), tree: folder(name, layers), textures: Vec::new() }));
    out
}

// ---------------------------------------------------------------- user library

#[derive(Serialize, Deserialize)]
pub struct SmartMaterialFile {
    pub name: String,
    pub tree: LayerTree,
    /// (resource id used in the tree, image file name next to the json)
    pub textures: Vec<(u64, String)>,
}

pub struct UserSmartMaterial {
    pub file: SmartMaterialFile,
    pub dir: PathBuf,
}

fn sanitize(s: &str) -> String {
    s.chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' || c == ' ' { c } else { '_' }).collect()
}

/// Resource ids referenced by a tree (fill channels and mask fills).
pub fn tree_resources(t: &LayerTree, out: &mut Vec<u64>) {
    if let LayerKind::Fill(f) = &t.desc.kind {
        for c in &f.channels {
            if c.source == SourceKind::Texture {
                out.extend(c.texture);
            }
        }
    }
    if let Some(m) = &t.desc.mask {
        for e in &m.effects {
            if let MaskEffectKind::Fill { source: SourceKind::Texture, texture: Some(id), .. } = &e.kind {
                out.push(*id);
            }
        }
    }
    for c in &t.children {
        tree_resources(c, out);
    }
}

/// Rewrites resource ids in a tree.
pub fn remap_resources(t: &mut LayerTree, map: &std::collections::HashMap<u64, u64>) {
    if let LayerKind::Fill(f) = &mut t.desc.kind {
        for c in f.channels.iter_mut() {
            if let Some(id) = c.texture {
                c.texture = map.get(&id).copied();
                if c.texture.is_none() && c.source == SourceKind::Texture {
                    c.source = SourceKind::Uniform;
                }
            }
        }
    }
    if let Some(m) = &mut t.desc.mask {
        for e in m.effects.iter_mut() {
            if let MaskEffectKind::Fill { texture, .. } = &mut e.kind {
                *texture = texture.and_then(|id| map.get(&id).copied());
            }
        }
    }
    for c in t.children.iter_mut() {
        remap_resources(c, map);
    }
}

pub fn save_smart_material(doc: &Document, name: &str, tree: &LayerTree) -> Result<PathBuf> {
    let dir = sub_dir("smart_materials").join(sanitize(name));
    std::fs::create_dir_all(&dir)?;
    let mut ids = Vec::new();
    tree_resources(tree, &mut ids);
    ids.sort();
    ids.dedup();
    let mut textures = Vec::new();
    for id in ids {
        if let Some(r) = doc.resources.get(&id) {
            let file = format!("tex_{id}_{}", sanitize(&r.name));
            std::fs::write(dir.join(&file), &*r.encoded)?;
            textures.push((id, file));
        }
    }
    let f = SmartMaterialFile { name: name.into(), tree: tree.clone(), textures };
    std::fs::write(dir.join("smart_material.json"), serde_json::to_vec_pretty(&f)?)?;
    Ok(dir)
}

pub fn user_smart_materials() -> Vec<UserSmartMaterial> {
    let Ok(rd) = std::fs::read_dir(sub_dir("smart_materials")) else { return Vec::new() };
    let mut out: Vec<UserSmartMaterial> = rd
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let dir = e.path();
            let f: SmartMaterialFile = serde_json::from_slice(&std::fs::read(dir.join("smart_material.json")).ok()?).ok()?;
            Some(UserSmartMaterial { file: f, dir })
        })
        .collect();
    out.sort_by(|a, b| a.file.name.cmp(&b.file.name));
    out
}

#[derive(Serialize, Deserialize)]
pub struct SmartMaskFile {
    pub name: String,
    pub mask: MaskDesc,
}

pub fn save_smart_mask(name: &str, mask: &MaskDesc) -> Result<PathBuf> {
    let path = sub_dir("smart_masks").join(format!("{}.json", sanitize(name)));
    let mut m = mask.clone();
    // Painted effects are per-project data; a smart mask keeps procedural steps.
    m.effects.retain(|e| !matches!(e.kind, MaskEffectKind::Paint));
    std::fs::write(&path, serde_json::to_vec_pretty(&SmartMaskFile { name: name.into(), mask: m })?)?;
    Ok(path)
}

pub fn user_smart_masks() -> Vec<SmartMaskFile> {
    json_files::<SmartMaskFile>(&sub_dir("smart_masks"))
}

pub fn save_brush(preset: &crate::brush::BrushPreset) -> Result<PathBuf> {
    let path = sub_dir("brushes").join(format!("{}.json", sanitize(&preset.name)));
    std::fs::write(&path, serde_json::to_vec_pretty(preset)?)?;
    Ok(path)
}

pub fn user_brushes() -> Vec<crate::brush::BrushPreset> {
    json_files(&sub_dir("brushes"))
}

fn json_files<T: for<'de> Deserialize<'de>>(dir: &Path) -> Vec<T> {
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut paths: Vec<PathBuf> =
        rd.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "json")).collect();
    paths.sort();
    paths.into_iter().filter_map(|p| serde_json::from_slice(&std::fs::read(p).ok()?).ok()).collect()
}

/// Image files of a library folder (textures, alphas, environments).
pub fn files_with(dir: &Path, exts: &[&str]) -> Vec<PathBuf> {
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut v: Vec<PathBuf> = rd
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.extension().and_then(|x| x.to_str()).is_some_and(|x| exts.iter().any(|e| e.eq_ignore_ascii_case(x)))
        })
        .collect();
    v.sort();
    v
}

/// Images of a starter assets folder (`alphas`, `textures`): its own files,
/// then each sub-folder's, sorted.
pub fn starter_files(folder: &str) -> Vec<PathBuf> {
    let Some(root) = starter_dir().map(|d| d.join(folder)) else { return Vec::new() };
    let mut v = files_with(&root, IMAGE_EXTS);
    if let Ok(rd) = std::fs::read_dir(&root) {
        let mut dirs: Vec<PathBuf> = rd.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.is_dir()).collect();
        dirs.sort();
        for d in dirs {
            v.extend(files_with(&d, IMAGE_EXTS));
        }
    }
    v
}

/// Shelf folder of a library image: its starter sub-folder, or "Your
/// assets" for imported files.
pub fn file_folder(p: &Path) -> String {
    if p.starts_with(library_dir()) {
        return "Your assets".into();
    }
    p.parent().and_then(|d| d.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// A file of the starter assets or of the user's library (not one picked
/// from disk).
pub fn is_library_file(p: &Path) -> bool {
    let under = |d: &Path| p.starts_with(d) || std::fs::canonicalize(p).ok().zip(std::fs::canonicalize(d).ok()).is_some_and(|(p, d)| p.starts_with(d));
    starter_dir().is_some_and(|d| under(&d)) || under(&library_dir())
}

/// A project resource named like something the tools pull in by themselves
/// (projects saved before resources kept where they came from): a starter
/// texture or alpha, a texture material's map, a built-in alpha shape.
pub fn is_starter_name(name: &str) -> bool {
    static NAMES: std::sync::OnceLock<std::collections::HashSet<String>> = std::sync::OnceLock::new();
    let names = NAMES.get_or_init(|| {
        ["textures", "alphas"]
            .iter()
            .flat_map(|f| starter_files(f))
            .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().to_lowercase()))
            .collect()
    });
    let lower = name.to_lowercase();
    if names.contains(&lower) {
        return true;
    }
    let stem = lower.rsplit_once('.').map_or(lower.as_str(), |(s, _)| s);
    TEX_MATERIAL_MAPS.iter().any(|(m, _)| stem.ends_with(&format!(" {m}")))
        || crate::brush::AlphaKind::ALL.iter().any(|k| k.name().eq_ignore_ascii_case(stem))
}

/// A starter alpha by its path under `starter_assets/alphas`.
pub fn starter_alpha(rel: &str) -> Option<PathBuf> {
    starter_dir().map(|d| d.join("alphas").join(rel)).filter(|p| p.is_file())
}

/// Environment maps of the starter assets (CC0, Poly Haven; credits in
/// `starter_assets/environments/CREDITS.txt`).
pub fn starter_environments() -> Vec<PathBuf> {
    starter_dir().map(|d| files_with(&d.join("environments"), ENV_EXTS)).unwrap_or_default()
}

/// The default environment: the first starter one.
pub fn default_environment() -> Option<PathBuf> {
    starter_environments().into_iter().next()
}

pub const IMAGE_EXTS: &[&str] = &["png", "jpg", "jpeg", "tga", "tif", "tiff", "bmp", "exr", "hdr", "webp", "avif"];
pub const ENV_EXTS: &[&str] = &["hdr", "exr"];

/// Copies files into a library folder; returns the new paths.
pub fn import_files(folder: &str, files: &[PathBuf]) -> Result<Vec<PathBuf>> {
    let dir = sub_dir(folder);
    let mut out = Vec::new();
    for f in files {
        if let Some(name) = f.file_name() {
            let dst = dir.join(name);
            std::fs::copy(f, &dst)?;
            out.push(dst);
        }
    }
    Ok(out)
}

/// A scanned texture material: `starter_assets/materials/<Category>/<id>/`
/// with `color.jpg`, `normal.jpg` (OpenGL), `roughness.jpg`, `height.jpg`,
/// optional `metallic.jpg` and a `material.json` (name, category, source,
/// license, downloads, metal).
#[derive(Clone, Debug, PartialEq)]
pub struct TexMaterial {
    pub name: String,
    pub category: String,
    pub dir: PathBuf,
    /// Where it comes from (Poly Haven, ambientCG), shown in its preview.
    pub source: String,
    /// Metal without a metallic map: metallic is 1.
    pub metal: bool,
    downloads: u64,
}

/// Texture material maps: file stem, fill channel.
pub const TEX_MATERIAL_MAPS: [(&str, usize); 5] =
    [("color", CH_COLOR), ("roughness", CH_ROUGH), ("metallic", CH_METAL), ("height", CH_HEIGHT), ("normal", CH_NORMAL)];

impl TexMaterial {
    /// The file of one map (`color`, `normal`...), when the material has it.
    pub fn map(&self, stem: &str) -> Option<PathBuf> {
        ["jpg", "png"].iter().map(|x| self.dir.join(format!("{stem}.{x}"))).find(|p| p.is_file())
    }

    /// Its pre-rendered shelf preview (`--matpreviews`: path traced sphere).
    pub fn preview(&self) -> Option<PathBuf> {
        Some(self.dir.join("preview.png")).filter(|p| p.is_file())
    }

    /// A fill reading the maps from resources `ids` (map stem to resource):
    /// color in sRGB, the others linear, normal in OpenGL convention.
    pub fn fill(&self, ids: &std::collections::HashMap<&'static str, u64>) -> FillDesc {
        let mut f = FillDesc::default();
        for (stem, c) in TEX_MATERIAL_MAPS {
            let ch = &mut f.channels[c];
            match ids.get(stem) {
                Some(&id) => {
                    *ch = FillChannel { enabled: true, source: SourceKind::Texture, texture: Some(id), ..Default::default() };
                    if c == CH_COLOR {
                        ch.value = [1.0; 3];
                    } else if c == CH_HEIGHT {
                        // Scanned displacement is far stronger than a
                        // painted height: a quarter by default.
                        ch.value = [0.0; 3];
                        ch.value_b = [0.25; 3];
                    } else {
                        ch.value = [0.0; 3];
                        ch.value_b = [1.0; 3];
                    }
                }
                None if c == CH_METAL => *ch = FillChannel { enabled: true, value: [if self.metal { 1.0 } else { 0.0 }; 3], ..Default::default() },
                None => {}
            }
        }
        // Tiling 1: the maps fit the UV space exactly.
        f.projection.scale = 1.0;
        f.material = Some(self.name.clone());
        f
    }
}

/// `starter_assets` next to the executable (installed) or above it (a
/// cargo build in the repository). `MF_STARTER_DIR` overrides it.
pub fn starter_dir() -> Option<PathBuf> {
    if let Ok(d) = std::env::var("MF_STARTER_DIR") {
        return Some(PathBuf::from(d)).filter(|d| d.is_dir());
    }
    let exe = std::env::current_exe().ok()?;
    exe.ancestors().skip(1).take(4).map(|d| d.join("starter_assets")).find(|d| d.is_dir())
}

/// Material categories of the shelf, in display order.
pub const MATERIAL_CATEGORIES: [&str; 11] = ["Wood", "Metal", "Stone", "Concrete", "Brick", "Tiles", "Plaster", "Ground", "Fabric", "Leather", "Plastic"];

/// The texture material in `dir` (its `material.json` and maps; the
/// category defaults to the parent folder's name).
pub fn tex_material_at(dir: &Path) -> Option<TexMaterial> {
    #[derive(Deserialize)]
    struct Meta {
        name: String,
        category: Option<String>,
        source: Option<String>,
        #[serde(default)]
        downloads: u64,
        #[serde(default)]
        metal: bool,
    }
    let meta: Meta = serde_json::from_slice(&std::fs::read(dir.join("material.json")).ok()?).ok()?;
    let folder = dir.parent().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    // Rock, marble, onyx... are one Stone category.
    let category = match meta.category.unwrap_or(folder).as_str() {
        "Rock" | "Marble" | "Onyx" | "Granite" | "Terrazzo" => "Stone".to_string(),
        c => c.to_string(),
    };
    let m = TexMaterial { name: meta.name, category, dir: dir.to_path_buf(), source: meta.source.unwrap_or_default(), metal: meta.metal, downloads: meta.downloads };
    m.map("color").is_some().then_some(m)
}

/// The texture materials of the starter assets and the library, by name.
/// Names are unique (their shelf key).
/// Each texture material's average color (sRGB hex) and average roughness
/// (None without a roughness map), read from its maps the first time and
/// kept in `material_stats.json` (Muse picks the scan whose real look is
/// closest to what is asked).
pub fn material_stats(mats: &[TexMaterial]) -> Vec<(Option<String>, Option<f32>)> {
    use rayon::prelude::*;
    let file = library_dir().join("..").join("material_stats.json");
    let mut cache: std::collections::HashMap<String, (Option<String>, Option<f32>)> =
        std::fs::read(&file).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
    let stamp = |p: &Path| p.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs());
    let key = |m: &TexMaterial| {
        let c = m.map("color");
        format!("{}|{}", m.dir.display(), c.as_deref().map_or(0, stamp))
    };
    // The average of a map, on a grid of about 64k samples.
    let average = |p: Option<PathBuf>| -> Option<[f32; 3]> {
        let img = image::open(p?).ok()?.to_rgb8();
        let step = ((img.width() as u64 * img.height() as u64 / 65536) as f32).sqrt().max(1.0) as u32;
        let (mut sum, mut n) = ([0f64; 3], 0f64);
        for y in (0..img.height()).step_by(step as usize) {
            for x in (0..img.width()).step_by(step as usize) {
                let px = img.get_pixel(x, y).0;
                for c in 0..3 {
                    sum[c] += px[c] as f64;
                }
                n += 1.0;
            }
        }
        (n > 0.0).then(|| sum.map(|s| (s / n) as f32))
    };
    let missing: Vec<&TexMaterial> = mats.iter().filter(|m| !cache.contains_key(&key(m))).collect();
    if !missing.is_empty() {
        let found: Vec<(String, (Option<String>, Option<f32>))> = missing
            .par_iter()
            .map(|m| {
                let color = average(m.map("color")).map(|c| format!("#{:02x}{:02x}{:02x}", c[0] as u8, c[1] as u8, c[2] as u8));
                let rough = average(m.map("roughness")).map(|c| (c[0] / 255.0 * 100.0).round() / 100.0);
                (key(m), (color, rough))
            })
            .collect();
        cache.extend(found);
        if let Ok(b) = serde_json::to_vec(&cache) {
            let _ = std::fs::write(&file, b);
        }
    }
    mats.iter().map(|m| cache.get(&key(m)).cloned().unwrap_or((None, None))).collect()
}

/// The texture materials under `root` (`<category>/<material>`).
fn tex_materials_in(root: &Path, out: &mut Vec<TexMaterial>) {
    let Ok(cats) = std::fs::read_dir(root) else { return };
    for cat in cats.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.is_dir()) {
        let Ok(rd) = std::fs::read_dir(&cat) else { continue };
        out.extend(rd.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.is_dir()).filter_map(|d| tex_material_at(&d)));
    }
}

/// The scans Muse downloaded (ambientCG): project assets, shown only with
/// the projects that use them.
pub fn scan_materials() -> Vec<TexMaterial> {
    let mut out = Vec::new();
    tex_materials_in(&crate::ambientcg::scans_dir(), &mut out);
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    out
}

pub fn texture_materials() -> Vec<TexMaterial> {
    // The starter assets, then the user's own in the library.
    let roots = [starter_dir().map(|d| d.join("materials")), Some(library_dir().join("materials"))];
    let mut out: Vec<TexMaterial> = Vec::new();
    for root in roots.into_iter().flatten() {
        tex_materials_in(&root, &mut out);
    }
    // One list by name, like Painter's shelf.
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()).then(b.downloads.cmp(&a.downloads)));
    let mut seen = std::collections::HashSet::new();
    for m in &mut out {
        let base = m.name.clone();
        let mut k = 2;
        while !seen.insert(m.name.clone()) {
            m.name = format!("{base} {k}");
            k += 1;
        }
    }
    out
}
