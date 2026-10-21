//! Shelf content: built-in materials, smart materials and smart masks
//! (authored here from procedurals and generators), and the user library in
//! %APPDATA%\SP_RETRO\library.

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

pub struct SmartMaterialPreset {
    pub name: &'static str,
    pub tree: LayerTree,
}

pub fn smart_materials() -> Vec<SmartMaterialPreset> {
    use MaskBlend::*;
    let sm = |name, tree| SmartMaterialPreset { name, tree };
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
                c.texture = map.get(&id).copied().or(Some(id));
            }
        }
    }
    if let Some(m) = &mut t.desc.mask {
        for e in m.effects.iter_mut() {
            if let MaskEffectKind::Fill { texture: Some(id), .. } = &mut e.kind {
                if let Some(n) = map.get(id) {
                    *id = *n;
                }
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

/// Environment maps of an installed Adobe Substance 3D Painter (newest
/// version found), read in place: nothing is copied into this project.
pub fn painter_environments() -> Vec<PathBuf> {
    let base = Path::new(r"C:\Program Files\Adobe");
    let Ok(rd) = std::fs::read_dir(base) else { return Vec::new() };
    let mut installs: Vec<PathBuf> = rd
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("Adobe Substance 3D Painter")))
        .collect();
    installs.sort();
    let Some(dir) = installs.last().map(|p| p.join("resources").join("starter_assets").join("environments")) else { return Vec::new() };
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&dir) {
        let mut subs: Vec<PathBuf> = rd.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.is_dir()).collect();
        subs.sort();
        for s in subs {
            out.extend(files_with(&s, ENV_EXTS));
        }
    }
    out
}

/// The real application's default environment ("Panorama"), if installed.
pub fn painter_default_environment() -> Option<PathBuf> {
    painter_environments().into_iter().find(|p| p.file_stem().is_some_and(|s| s.eq_ignore_ascii_case("panorama")))
}

pub const IMAGE_EXTS: &[&str] = &["png", "jpg", "jpeg", "tga", "tif", "tiff", "bmp", "exr", "hdr"];
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
