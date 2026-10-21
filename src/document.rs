//! The open project: mesh, resources, texture sets and their layer trees.

use crate::bake;
use crate::engine::composite::{
    BlendU, ChanSrcU, ClearU, FillU, FilterU, FinalU, FolderU, MaskPlan, MaskStep, MaskU, PaintRefs, Plan,
};
use crate::engine::paint::{LAYER_FORMAT, MASK_PAINT_FORMAT, OPAC_FORMAT};
use crate::engine::{Engine, GpuMesh, Tex, TextureSetGpu};
use crate::mesh::MeshData;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

pub const NCH: usize = 7;
pub const CHANNELS: [&str; NCH] = ["Base color", "Roughness", "Metallic", "Height", "Emissive", "Opacity", "Normal"];
pub const CH_COLOR: usize = 0;
pub const CH_ROUGH: usize = 1;
pub const CH_METAL: usize = 2;
pub const CH_HEIGHT: usize = 3;
pub const CH_EMISSIVE: usize = 4;
pub const CH_OPACITY: usize = 5;
pub const CH_NORMAL: usize = 6;

/// Optional channels a texture set can add (Texture Set Settings > Channels).
/// They live in spare accumulator / final slots: specular level in the
/// roughness-metal-height .w and emissive final .w, transmission in the
/// emissive accumulator .w and normal final .w.
pub const NEXT: usize = 2;
pub const EXT_NAMES: [&str; NEXT] = ["Specular level", "Transmission"];
pub const EXT_SHORT: [&str; NEXT] = ["spec", "trans"];
/// Values when a layer does not write the channel.
pub const EXT_DEFAULTS: [f32; NEXT] = [0.5, 0.0];

fn ext_default() -> [FillChannel; NEXT] {
    std::array::from_fn(|i| FillChannel { enabled: false, value: [EXT_DEFAULTS[i]; 3], value_b: [1.0; 3], ..Default::default() })
}

/// True for channels holding a color (rgb), false for scalars and the normal.
pub fn is_color_channel(c: usize) -> bool {
    c == CH_COLOR || c == CH_EMISSIVE
}

macro_rules! named_enum {
    ($name:ident { $($variant:ident => $label:expr),* $(,)? }) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
        pub enum $name { $($variant),* }
        #[allow(dead_code)]
        impl $name {
            pub const ALL: &'static [$name] = &[$($name::$variant),*];
            pub fn name(self) -> &'static str {
                match self { $($name::$variant => $label),* }
            }
            pub fn code(self) -> u32 {
                Self::ALL.iter().position(|v| *v == self).unwrap() as u32
            }
        }
    };
}

named_enum!(BlendMode {
    Normal => "Normal",
    Replace => "Replace",
    Multiply => "Multiply",
    Divide => "Divide",
    InverseDivide => "Inverse divide",
    Darken => "Darken (min)",
    Lighten => "Lighten (max)",
    LinearDodge => "Linear dodge (add)",
    Subtract => "Subtract",
    InverseSubtract => "Inverse subtract",
    Difference => "Difference",
    Exclusion => "Exclusion",
    SignedAddition => "Signed addition",
    Overlay => "Overlay",
    Screen => "Screen",
    LinearBurn => "Linear burn",
    ColorBurn => "Color burn",
    ColorDodge => "Color dodge",
    SoftLight => "Soft light",
    HardLight => "Hard light",
    VividLight => "Vivid light",
    PinLight => "Pin light",
    Saturation => "Saturation",
    Color => "Color",
    Value => "Value",
    Hue => "Hue",
    NormalCombine => "Normal map combine",
    NormalDetail => "Normal map detail",
});

named_enum!(MaskBlend {
    Normal => "Normal",
    Multiply => "Multiply",
    Add => "Add",
    Subtract => "Subtract",
    Lighten => "Lighten (max)",
    Darken => "Darken (min)",
    Screen => "Screen",
    Overlay => "Overlay",
    Difference => "Difference",
});

named_enum!(ProcKind {
    Clouds => "Clouds",
    Perlin => "Perlin noise",
    Ridged => "Ridged noise",
    Cells => "Cells",
    Cracks => "Cracks",
    Grunge => "Grunge",
    Scratches => "Scratches",
    Dots => "Dots",
    Fibers => "Fibers",
    Wood => "Wood rings",
    Bricks => "Bricks",
    Checker => "Checker",
    Gradient => "Linear gradient",
    Turbulence => "Turbulence",
    Marble => "Marble",
    Leather => "Leather",
    Rust => "Rust spots",
    Fabric => "Fabric weave",
    Grain => "Grain",
    Hexagons => "Hexagons",
    // Painter's shelf procedurals (`procedurals.rs`), after the classic ones
    // so saved codes keep their meaning.
    PerlinNoise => "Perlin Noise",
    Clouds1 => "Clouds 1",
    Clouds2 => "Clouds 2",
    Clouds3 => "Clouds 3",
    BnwSpots1 => "BnW Spots 1",
    BnwSpots2 => "BnW Spots 2",
    BnwSpots3 => "BnW Spots 3",
    Cells1 => "Cells 1",
    Cells2 => "Cells 2",
    Cells3 => "Cells 3",
    Cells4 => "Cells 4",
    Crystal1 => "Crystal 1",
    Crystal2 => "Crystal 2",
    Dirt1 => "Dirt 1",
    Dirt2 => "Dirt 2",
    Dirt3 => "Dirt 3",
    Dirt4 => "Dirt 4",
    Dirt5 => "Dirt 5",
    DirtGradient => "Dirt Gradient",
    FractalSum1 => "Fractal Sum 1",
    FractalSum2 => "Fractal Sum 2",
    FractalSum3 => "Fractal Sum 3",
    FractalSum4 => "Fractal Sum 4",
    FractalSumBase => "Fractal Sum Base",
    GaussianNoise => "Gaussian Noise",
    GaussianSpots1 => "Gaussian Spots 1",
    GaussianSpots2 => "Gaussian Spots 2",
    WhiteNoise => "White Noise",
    WhiteNoiseFast => "White Noise Fast",
    BlueNoise => "Blue Noise",
    DirectionalNoise1 => "Directional Noise 1",
    DirectionalNoise2 => "Directional Noise 2",
    DirectionalNoise3 => "Directional Noise 3",
    DirectionalNoise4 => "Directional Noise 4",
    DirectionalScratches => "Directional Scratches",
    AnisotropicNoise => "Anisotropic Noise",
    Fur1 => "Fur 1",
    Fur2 => "Fur 2",
    Fur3 => "Fur 3",
    MessyFibers1 => "Messy Fibers 1",
    MessyFibers2 => "Messy Fibers 2",
    MessyFibers3 => "Messy Fibers 3",
    MoistureNoise => "Moisture Noise",
    Plasma => "Plasma",
    Waveform1 => "Waveform 1",
    Voronoi => "Voronoi",
    VoronoiFractal => "Voronoi Fractal",
    Creased => "Creased",
    Fluid => "Fluid",
    Liquid => "Liquid",
    MicroscopeView => "Microscope View",
    Perlin3D => "3D Perlin Noise",
    PerlinFractal3D => "3D Perlin Noise Fractal",
    RidgedFractal3D => "3D Ridged Noise Fractal",
    Simplex3D => "3D Simplex Noise",
    Voronoi3D => "3D Voronoi",
    VoronoiFractal3D => "3D Voronoi Fractal",
    Worley3D => "3D Worley Noise",
    MarbleFine => "Marble Fine",
    MarbleVeins => "Marble Veins",
    MarbleWide => "Marble Wide",
    CreasesSoft => "Creases Soft",
    GrungeMap001 => "Grunge Map 001",
    GrungeMap002 => "Grunge Map 002",
    GrungeMap003 => "Grunge Map 003",
    GrungeMap004 => "Grunge Map 004",
    GrungeMap005 => "Grunge Map 005",
    GrungeMap006 => "Grunge Map 006",
    GrungeMap007 => "Grunge Map 007",
    GrungeMap008 => "Grunge Map 008",
    GrungeMap009 => "Grunge Map 009",
    GrungeMap010 => "Grunge Map 010",
    GrungeMap011 => "Grunge Map 011",
    GrungeMap012 => "Grunge Map 012",
    GrungeMap013 => "Grunge Map 013",
    GrungeMap014 => "Grunge Map 014",
    GrungeMap015 => "Grunge Map 015",
    GrungeConcrete => "Grunge Concrete",
    GrungeDamas => "Grunge Damas",
    GrungeGalvanicLarge => "Grunge Galvanic Large",
    GrungeGalvanicSmall => "Grunge Galvanic Small",
    GrungeLeaks => "Grunge Leaks",
    GrungeLeakyPaint => "Grunge Leaky Paint",
    GrungeRoughDirty => "Grunge Rough Dirty",
    GrungeRustFine => "Grunge Rust Fine",
    GrungeScratchesDirty => "Grunge Scratches Dirty",
    GrungeScratchesFine => "Grunge Scratches Fine",
    GrungeScratchesRough => "Grunge Scratches Rough",
    GrungeShavings => "Grunge Shavings",
    GrungeSplashesDusty => "Grunge Splashes Dusty",
    GrungeSpots => "Grunge Spots",
    GrungeSpotsDirty => "Grunge Spots Dirty",
    BrickGenerator => "Brick Generator",
    Bricks1 => "Bricks 1",
    TileGenerator => "Tile Generator",
    TileRandom2 => "Tile Random 2",
    TriangleGrid => "Triangle Grid",
    Wood1 => "Wood 1",
    Fence1 => "Fence 1",
    MetallicGrateLines => "Metallic Grate Lines",
    MetallicGrateRound => "Metallic Grate Round",
    MetallicGrateSimple => "Metallic Grate Simple",
    MetallicGrateWide => "Metallic Grate Wide",
    Pavement => "Pavement",
    CamoUcp => "Camo UCP",
    CamoWoodland => "Camo Woodland",
    ScratchesGenerator => "Scratches Generator",
    PaperPath => "Paper Path",
    Checker1 => "Checker 1",
    Checker2 => "Checker 2",
    Circles => "Circles",
    Hexagon => "Hexagon",
    HexagonBorder => "Hexagon Border",
    HexagonBranch => "Hexagon Branch",
    HexagonDots => "Hexagon Dots",
    HexagonDouble => "Hexagon Double",
    HexagonTriangle => "Hexagon Triangle",
    HexagonTriangleBorder => "Hexagon Triangle Border",
    HexagonWave => "Hexagon Wave",
    Polygon1 => "Polygon 1",
    Polygon2 => "Polygon 2",
    Stripes => "Stripes",
    GradientLinear1 => "Gradient Linear 1",
    GradientLinear2 => "Gradient Linear 2",
    GradientLinear3 => "Gradient Linear 3",
    GradientCircular => "Gradient Circular",
    GradientDot => "Gradient Dot",
    GradientChecker => "Gradient Checker",
    GradientCheckerDouble => "Gradient Checker Double",
    GradientAlternate => "Gradient Alternate",
    GradientFlakes => "Gradient Flakes",
    GradientDiscOverlap => "Gradient Disc Overlap",
    GradientHue => "Gradient Hue",
    GradientWeave => "Gradient Weave",
    GradientAxial => "Gradient Axial",
    GradientAxialReflected => "Gradient Axial Reflected",
    GradientRadial => "Gradient Radial",
});

impl ProcKind {
    /// Classic generators: evaluated per texel in 3D (procedural.wgsl).
    pub fn is_classic(self) -> bool {
        self.code() < 20
    }

    /// Painter's position-based "3D" noises (also evaluated per texel).
    pub fn is_3d(self) -> bool {
        matches!(
            self,
            ProcKind::Perlin3D | ProcKind::PerlinFractal3D | ProcKind::RidgedFractal3D | ProcKind::Simplex3D | ProcKind::Voronoi3D | ProcKind::VoronoiFractal3D | ProcKind::Worley3D
        )
    }

    /// Rendered once into a seamless texture (engine/procgen.rs).
    pub fn is_bitmap(self) -> bool {
        !self.is_classic() && !self.is_3d()
    }

    /// Case of `proc_raw` in procedural.wgsl (classic codes, then the 3D noises).
    pub fn analytic_id(self) -> u32 {
        match self {
            ProcKind::Perlin3D => 20,
            ProcKind::PerlinFractal3D => 21,
            ProcKind::RidgedFractal3D => 22,
            ProcKind::Simplex3D => 23,
            ProcKind::Voronoi3D => 24,
            ProcKind::VoronoiFractal3D => 25,
            ProcKind::Worley3D => 26,
            k if k.is_classic() => k.code(),
            _ => 0,
        }
    }
}

named_enum!(ProjMode {
    Uv => "UV",
    Triplanar => "Tri-planar",
    PlanarX => "Planar X",
    PlanarY => "Planar Y",
    PlanarZ => "Planar Z",
    Object3D => "Object 3D (procedurals)",
});

named_enum!(SourceKind {
    Uniform => "Uniform",
    Texture => "Texture",
    Procedural => "Procedural",
});

named_enum!(FilterKind {
    Hsl => "HSL",
    BrightnessContrast => "Brightness / contrast",
    Levels => "Levels",
    Invert => "Invert",
    Posterize => "Posterize",
    Blur => "Blur",
    Sharpen => "Sharpen",
    Tint => "Tint",
    GradientMap => "Gradient map",
    Desaturate => "Desaturate",
    Clamp => "Clamp",
    HeightAdjust => "Height adjust",
    HistogramScan => "Histogram scan",
    Highpass => "Highpass",
    ColorCorrect => "Color correct",
    Mirror => "Mirror",
    Transform => "Transform",
});

named_enum!(Generator {
    Edges => "Edges (curvature)",
    Cavity => "Cavity",
    AmbientOcclusion => "Ambient occlusion",
    Dirt => "Dirt",
    EdgeWear => "Edge wear",
    PositionGradient => "Position gradient",
    Light => "Directional light",
    Thickness => "Thickness",
    Curvature => "Curvature (raw)",
});

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct ProcParams {
    pub kind: ProcKind,
    pub seed: f32,
    pub contrast: f32,
    pub balance: f32,
    pub warp: f32,
    pub detail: f32,
    pub aspect: f32,
    pub density: f32,
    pub roughness: f32,
    /// Pattern frequency multiplier (the "Scale" of the pattern's own
    /// parameters; the projection tiling stays separate).
    pub scale: f32,
    /// Swaps the low / high ends (colors or range).
    pub invert: bool,
    /// Parameters of Painter's shelf procedurals (`procedurals::DEFS`).
    pub p: [f32; 8],
}

impl Default for ProcParams {
    fn default() -> Self {
        ProcParams {
            kind: ProcKind::Clouds,
            seed: 0.0,
            contrast: 1.0,
            balance: 0.5,
            warp: 0.0,
            detail: 5.0,
            aspect: 1.0,
            density: 0.5,
            roughness: 0.5,
            scale: 1.0,
            invert: false,
            p: [0.0; 8],
        }
    }
}

impl ProcParams {
    pub fn of(kind: ProcKind) -> Self {
        let mut p = ProcParams { kind, ..Default::default() };
        if let Some(d) = crate::procedurals::def_of(kind) {
            for q in d.params {
                p.p[q.slot] = q.default;
            }
        }
        p
    }

    /// Inputs of the seamless generator (None for per-texel generators).
    /// `set_res`: resolution of the texture set using it (the size follows).
    pub fn gen_u(&self, set_res: u32) -> Option<crate::engine::procgen::GenU> {
        let d = crate::procedurals::def_of(self.kind).filter(|_| self.kind.is_bitmap())?;
        Some(crate::engine::procgen::GenU {
            k: [d.family, d.variant, crate::engine::procgen::ProcGen::res_for(set_res), 0],
            a: [self.seed, self.balance, self.contrast, 0.0],
            p0: [self.p[0], self.p[1], self.p[2], self.p[3]],
            p1: [self.p[4], self.p[5], self.p[6], self.p[7]],
            v: d.v,
        })
    }
    fn pa(&self) -> [f32; 4] {
        [self.seed, self.contrast, self.balance, self.warp]
    }
    fn pb(&self) -> [f32; 4] {
        [self.detail, self.aspect, self.density, self.roughness]
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Projection {
    pub mode: ProjMode,
    pub scale: f32,
    pub offset: [f32; 2],
    /// Degrees.
    pub rotation: f32,
    pub sharpness: f32,
    /// Mirror U / V (context toolbar).
    pub mirror: [bool; 2],
    /// 3D placement of planar / tri-planar / object projections (the
    /// viewport manipulator): offset from the mesh center in mesh units,
    /// orientation (quaternion x, y, z, w), scale per axis.
    pub position: [f32; 3],
    pub orient: [f32; 4],
    pub axes: [f32; 3],
}

impl Default for Projection {
    fn default() -> Self {
        Projection {
            mode: ProjMode::Uv,
            scale: 1.0,
            offset: [0.0; 2],
            rotation: 0.0,
            sharpness: 4.0,
            mirror: [false; 2],
            position: [0.0; 3],
            orient: [0.0, 0.0, 0.0, 1.0],
            axes: [1.0; 3],
        }
    }
}

/// Mesh space of the object position map (`bake::position_map`):
/// mesh = min + pos * k (x and z share the largest extent, y uses its own).
#[derive(Clone, Copy, Debug)]
pub struct WorldMap {
    pub min: glam::Vec3,
    pub k: glam::Vec3,
    pub center: glam::Vec3,
}

impl WorldMap {
    pub fn of(mesh: &MeshData) -> Self {
        let size = (mesh.bbox_max - mesh.bbox_min).max(glam::Vec3::splat(1e-6));
        let ext = size.max_element();
        WorldMap { min: mesh.bbox_min, k: glam::Vec3::new(ext, size.y, ext), center: (mesh.bbox_min + mesh.bbox_max) * 0.5 }
    }
}

/// Projection uniforms: mode / scale / rotation / sharpness, offsets and
/// mirror, then position-map space to projection space (3 affine rows) and
/// the rotation of normals into it (3 rows).
pub struct ProjU {
    pub proj: [f32; 4],
    pub offs: [f32; 4],
    pub xf: [[f32; 4]; 3],
    pub nr: [[f32; 4]; 3],
}

impl Projection {
    pub fn rotation3(&self) -> glam::Quat {
        let q = glam::Quat::from_array(self.orient);
        if q.length_squared() < 1e-8 {
            glam::Quat::IDENTITY
        } else {
            q.normalize()
        }
    }

    /// Projection frame in mesh space: pivot, orientation, scale.
    pub fn frame(&self, w: &WorldMap) -> (glam::Vec3, glam::Quat, glam::Vec3) {
        (w.center + glam::Vec3::from(self.position), self.rotation3(), glam::Vec3::from(self.axes))
    }

    fn uniform(&self, w: &WorldMap) -> ProjU {
        use glam::{Mat3, Vec3};
        let rinv = Mat3::from_quat(self.rotation3().inverse());
        let s = Vec3::from(self.axes).abs().max(Vec3::splat(1e-4));
        // Mesh point x to projection space: c + S^-1 R^-1 (x - c - t);
        // in position-map space (x = min + k p) that is L p + b.
        let m = Mat3::from_diagonal(1.0 / s) * rinv;
        let d = w.k;
        let l = Mat3::from_diagonal(1.0 / d) * m * Mat3::from_diagonal(d);
        let t = Vec3::from(self.position);
        let b = (w.center - w.min + m * (w.min - w.center - t)) / d;
        let row = |m: &Mat3, i: usize, e: f32| {
            let r = m.row(i);
            [r.x, r.y, r.z, e]
        };
        ProjU {
            proj: [self.mode.code() as f32, self.scale, self.rotation.to_radians(), self.sharpness],
            offs: [self.offset[0], self.offset[1], if self.mirror[0] { -1.0 } else { 1.0 }, if self.mirror[1] { -1.0 } else { 1.0 }],
            xf: [row(&l, 0, b.x), row(&l, 1, b.y), row(&l, 2, b.z)],
            nr: [row(&rinv, 0, 0.0), row(&rinv, 1, 0.0), row(&rinv, 2, 0.0)],
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct FillChannel {
    pub enabled: bool,
    pub source: SourceKind,
    /// Uniform value, texture tint / range min, procedural color A.
    pub value: [f32; 3],
    /// Texture range max, procedural color B.
    pub value_b: [f32; 3],
    pub texture: Option<u64>,
    pub procedural: ProcParams,
    pub flip_green: bool,
}

impl Default for FillChannel {
    fn default() -> Self {
        FillChannel {
            enabled: true,
            source: SourceKind::Uniform,
            value: [0.5; 3],
            value_b: [1.0; 3],
            texture: None,
            procedural: ProcParams::default(),
            flip_green: false,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct FillDesc {
    pub channels: [FillChannel; NCH],
    pub projection: Projection,
    /// Optional channels (specular level, transmission): uniform or procedural.
    #[serde(default = "ext_default")]
    pub ext: [FillChannel; NEXT],
    /// Material the fill was made from (shown in its Material mode slot).
    pub material: Option<String>,
}

impl Default for FillDesc {
    fn default() -> Self {
        let ch = |enabled: bool, v: f32| FillChannel { enabled, value: [v; 3], ..Default::default() };
        let mut channels = [ch(true, 0.5), ch(true, 0.5), ch(true, 0.0), ch(false, 0.0), ch(false, 0.0), ch(false, 1.0), ch(false, 0.0)];
        channels[CH_NORMAL].value = [0.0, 0.0, 1.0];
        channels[CH_NORMAL].value_b = [1.0, 1.0, 1.0];
        channels[CH_HEIGHT].value_b = [1.0; 3];
        FillDesc { ext: ext_default(), channels, projection: Projection::default(), material: None }
    }
}

impl FillDesc {
    /// A fill with base color, roughness and metallic set.
    pub fn material(color: [f32; 3], rough: f32, metal: f32) -> Self {
        let mut f = FillDesc::default();
        f.channels[CH_COLOR].value = color;
        f.channels[CH_ROUGH].value = [rough; 3];
        f.channels[CH_METAL].value = [metal; 3];
        f
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct FilterDesc {
    pub kind: FilterKind,
    pub channels: [bool; NCH],
    pub p: [f32; 8],
    pub color: [f32; 3],
    pub color2: [f32; 3],
}

impl Default for FilterDesc {
    fn default() -> Self {
        Self::of(FilterKind::Hsl)
    }
}

impl FilterDesc {
    pub fn of(kind: FilterKind) -> Self {
        let mut channels = [false; NCH];
        let p = match kind {
            FilterKind::Hsl => [0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            FilterKind::BrightnessContrast => [0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            FilterKind::Levels => [0.0, 1.0, 1.0, 0.0, 1.0, 0.0, 0.0, 0.0],
            FilterKind::Posterize => [6.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            FilterKind::Blur => [4.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            FilterKind::Sharpen => [1.0, 2.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            FilterKind::Tint => [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            FilterKind::Clamp => [0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            // scale, offset
            FilterKind::HeightAdjust => [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            // position, contrast
            FilterKind::HistogramScan => [0.5, 0.5, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            // radius
            FilterKind::Highpass => [8.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            // brightness, contrast, gamma, saturation, hue shift
            FilterKind::ColorCorrect => [0.0, 1.0, 1.0, 1.0, 0.0, 0.0, 0.0, 0.0],
            // axis: 0 horizontal, 1 vertical, 2 both
            FilterKind::Mirror => [0.0; 8],
            // offset u, offset v, rotation (degrees), scale
            FilterKind::Transform => [0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0],
            _ => [0.0; 8],
        };
        channels[CH_COLOR] = !matches!(kind, FilterKind::HeightAdjust);
        if matches!(kind, FilterKind::Blur | FilterKind::Sharpen | FilterKind::Levels | FilterKind::Invert | FilterKind::HeightAdjust | FilterKind::Highpass) {
            channels[CH_HEIGHT] = true;
        }
        if matches!(kind, FilterKind::Mirror | FilterKind::Transform) {
            channels = [true; NCH];
        }
        FilterDesc { kind, channels, p, color: [1.0, 0.8, 0.6], color2: [1.0, 1.0, 1.0] }
    }

    fn blur_radius(&self) -> f32 {
        match self.kind {
            FilterKind::Blur => self.p[0].max(0.5),
            FilterKind::Sharpen => self.p[1].max(0.5),
            FilterKind::Highpass => self.p[0].max(0.5),
            _ => 0.0,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct GeneratorDesc {
    pub kind: Generator,
    pub level: f32,
    pub contrast: f32,
    pub invert: bool,
    pub noise: f32,
    pub noise_scale: f32,
    pub seed: f32,
    pub axis: u32,
    pub dir: [f32; 3],
    /// How much painted height in the layers below feeds the curvature
    /// (0 = baked curvature only).
    pub height_reactive: f32,
}

impl Default for GeneratorDesc {
    fn default() -> Self {
        Self::of(Generator::Edges)
    }
}

impl GeneratorDesc {
    pub fn of(kind: Generator) -> Self {
        let (level, contrast, noise) = match kind {
            Generator::Edges => (0.2, 4.0, 0.0),
            Generator::EdgeWear => (0.3, 5.0, 0.5),
            Generator::Cavity => (0.15, 4.0, 0.0),
            Generator::AmbientOcclusion => (0.3, 3.0, 0.0),
            Generator::Dirt => (0.3, 3.0, 0.3),
            Generator::PositionGradient => (0.5, 1.0, 0.0),
            Generator::Light => (0.5, 2.0, 0.0),
            Generator::Thickness => (0.5, 2.0, 0.0),
            Generator::Curvature => (0.5, 1.0, 0.0),
        };
        GeneratorDesc {
            kind,
            level,
            contrast,
            invert: false,
            noise,
            noise_scale: 12.0,
            seed: 0.0,
            axis: 1,
            dir: [0.3, 1.0, 0.2],
            height_reactive: 0.0,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum MaskEffectKind {
    Paint,
    Fill {
        value: f32,
        source: SourceKind,
        texture: Option<u64>,
        procedural: ProcParams,
        projection: Projection,
        range: [f32; 2],
    },
    Generator(GeneratorDesc),
    Levels { in_lo: f32, in_hi: f32, gamma: f32, out_lo: f32, out_hi: f32 },
    Blur { radius: f32 },
    Invert,
    ColorSelect { colors: Vec<[f32; 3]>, tolerance: f32 },
    Threshold { value: f32 },
}

impl MaskEffectKind {
    pub fn label(&self) -> String {
        match self {
            MaskEffectKind::Paint => "Paint".into(),
            MaskEffectKind::Fill { source, procedural, .. } => match source {
                SourceKind::Procedural => format!("Fill: {}", procedural.kind.name()),
                SourceKind::Texture => "Fill: texture".into(),
                SourceKind::Uniform => "Fill: uniform".into(),
            },
            MaskEffectKind::Generator(g) => format!("Generator: {}", g.kind.name()),
            MaskEffectKind::Levels { .. } => "Levels".into(),
            MaskEffectKind::Blur { .. } => "Blur".into(),
            MaskEffectKind::Invert => "Invert".into(),
            MaskEffectKind::ColorSelect { .. } => "Color selection".into(),
            MaskEffectKind::Threshold { .. } => "Threshold".into(),
        }
    }

    pub fn fill_procedural(kind: ProcKind) -> Self {
        MaskEffectKind::Fill {
            value: 1.0,
            source: SourceKind::Procedural,
            texture: None,
            procedural: ProcParams::of(kind),
            projection: Projection { mode: ProjMode::Object3D, scale: 6.0, ..Default::default() },
            range: [0.0, 1.0],
        }
    }

    pub fn levels() -> Self {
        MaskEffectKind::Levels { in_lo: 0.0, in_hi: 1.0, gamma: 1.0, out_lo: 0.0, out_hi: 1.0 }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MaskEffect {
    pub id: u64,
    pub enabled: bool,
    pub blend: MaskBlend,
    pub opacity: f32,
    pub kind: MaskEffectKind,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct MaskDesc {
    pub enabled: bool,
    /// Value before the first effect (white or black mask).
    pub base: f32,
    /// Bottom first.
    pub effects: Vec<MaskEffect>,
}

impl Default for MaskDesc {
    fn default() -> Self {
        MaskDesc { enabled: true, base: 1.0, effects: Vec::new() }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum LayerKind {
    Paint,
    Fill(Box<FillDesc>),
    Filter(FilterDesc),
    Folder,
}

impl LayerKind {
    pub fn tag(&self) -> &'static str {
        match self {
            LayerKind::Paint => "PAINT",
            LayerKind::Fill(_) => "FILL",
            LayerKind::Filter(_) => "FILTER",
            LayerKind::Folder => "FOLDER",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LayerDesc {
    pub id: u64,
    pub name: String,
    pub visible: bool,
    pub kind: LayerKind,
    pub blend: [BlendMode; NCH],
    pub opacity: [f32; NCH],
    pub mask: Option<MaskDesc>,
    #[serde(default = "yes")]
    pub expanded: bool,
    /// Effects stacked on the layer content (filters), bottom first.
    #[serde(default)]
    pub effects: Vec<ContentEffect>,
    /// Opacity of the optional channels (normal blending).
    #[serde(default = "one_f32")]
    pub ext_opacity: f32,
    /// Color tag shown on the row (0 = none).
    #[serde(default)]
    pub tag: u8,
    /// A brush stroke painted the opacity channel on this paint layer
    /// (Auto shader alpha then blends the set).
    #[serde(default)]
    pub opacity_painted: bool,
}

/// An effect on a layer's content. The layer and its effects composite as
/// an implicit group, then the layer mask applies to the result.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ContentEffect {
    pub id: u64,
    pub enabled: bool,
    pub opacity: f32,
    pub filter: FilterDesc,
}

fn yes() -> bool {
    true
}

fn one_f32() -> f32 {
    1.0
}

impl LayerDesc {
    pub fn new(id: u64, name: String, kind: LayerKind) -> Self {
        let mut blend = [BlendMode::Normal; NCH];
        blend[CH_NORMAL] = BlendMode::NormalCombine;
        LayerDesc { id, name, visible: true, kind, blend, opacity: [1.0; NCH], mask: None, expanded: true, effects: Vec::new(), tag: 0, ext_opacity: 1.0, opacity_painted: false }
    }

    fn blend_u(&self) -> BlendU {
        let b = |i: usize| self.blend[i].code();
        BlendU {
            mode_a: [b(0), b(1), b(2), b(3)],
            mode_b: [b(4), b(5), b(6), 0],
            op_a: [self.opacity[0], self.opacity[1], self.opacity[2], self.opacity[3]],
            op_b: [self.opacity[4], self.opacity[5], self.opacity[6], self.ext_opacity],
        }
    }
}

/// Serializable layer tree (smart materials, project files).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LayerTree {
    pub desc: LayerDesc,
    #[serde(default)]
    pub children: Vec<LayerTree>,
}

#[derive(Clone)]
pub struct PaintTex {
    pub color: Arc<Tex>,
    pub vals: Arc<Tex>,
    pub alph: Arc<Tex>,
    pub emis: Option<Arc<Tex>>,
    pub nrm: Option<Arc<Tex>>,
    pub opac: Option<Arc<Tex>>,
}

const LAYER_USAGE: wgpu::TextureUsages = wgpu::TextureUsages::RENDER_ATTACHMENT
    .union(wgpu::TextureUsages::TEXTURE_BINDING)
    .union(wgpu::TextureUsages::COPY_SRC)
    .union(wgpu::TextureUsages::COPY_DST);

impl PaintTex {
    pub fn new(device: &wgpu::Device, res: u32) -> Self {
        let t = |l| Arc::new(Tex::new(device, l, res, res, LAYER_FORMAT, LAYER_USAGE));
        PaintTex { color: t("layer color"), vals: t("layer values"), alph: t("layer alphas"), emis: None, nrm: None, opac: None }
    }

    /// A layer nothing was painted on yet: 1x1 transparent textures, so an
    /// empty 4K paint layer costs no video memory. `materialize` before the
    /// first stroke.
    pub fn empty(device: &wgpu::Device) -> Self {
        Self::new(device, 1)
    }

    pub fn is_empty(&self) -> bool {
        self.color.width == 1
    }

    /// Full-size (still transparent) textures for painting. Undo entries
    /// are always taken after this, so they never point at the placeholder.
    pub fn materialize(&mut self, device: &wgpu::Device, res: u32) -> bool {
        if !self.is_empty() || res == 1 {
            return false;
        }
        *self = Self::new(device, res);
        true
    }

    /// Allocates the emissive / normal / opacity textures on first use.
    pub fn ensure_b(&mut self, device: &wgpu::Device) {
        let res = self.color.width;
        if self.emis.is_none() {
            self.emis = Some(Arc::new(Tex::new(device, "layer emissive", res, res, LAYER_FORMAT, LAYER_USAGE)));
            self.nrm = Some(Arc::new(Tex::new(device, "layer normal", res, res, LAYER_FORMAT, LAYER_USAGE)));
            self.opac = Some(Arc::new(Tex::new(device, "layer opacity", res, res, OPAC_FORMAT, LAYER_USAGE)));
        }
    }

    pub fn all(&self) -> Vec<&Arc<Tex>> {
        let mut v = vec![&self.color, &self.vals, &self.alph];
        v.extend(self.emis.iter());
        v.extend(self.nrm.iter());
        v.extend(self.opac.iter());
        v
    }
}

/// Globally unique revision numbers: a value always names one texture content.
pub fn next_rev() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

/// Paint effect of a mask: a 1x1 placeholder (no coverage) until something
/// is painted; `full_mask_paint` gives the real texture.
pub fn new_mask_paint(device: &wgpu::Device, _res: u32) -> Arc<Tex> {
    full_mask_paint(device, 1)
}

pub fn full_mask_paint(device: &wgpu::Device, res: u32) -> Arc<Tex> {
    Arc::new(Tex::new(device, "mask paint", res, res, MASK_PAINT_FORMAT, LAYER_USAGE))
}

impl Layer {
    /// Gives a paint target its full-size texture before it is painted.
    pub fn materialize(&mut self, device: &wgpu::Device, res: u32, mask_effect: Option<u64>) {
        let grew = match mask_effect {
            None => self.paint.as_mut().is_some_and(|p| p.materialize(device, res)),
            Some(eid) => match self.mask_paint.get_mut(&eid) {
                Some(t) if t.width == 1 && res > 1 => {
                    *t = full_mask_paint(device, res);
                    true
                }
                _ => false,
            },
        };
        if grew {
            self.rev = next_rev();
        }
    }
}

#[derive(Clone)]
pub struct Layer {
    pub desc: LayerDesc,
    /// Bumped whenever this layer's painted textures change (cache keys).
    pub rev: u64,
    pub paint: Option<PaintTex>,
    /// Paint mask effects' textures by effect id.
    pub mask_paint: HashMap<u64, Arc<Tex>>,
    /// Folder children, bottom first.
    pub children: Vec<Layer>,
}

impl Layer {
    pub fn is_folder(&self) -> bool {
        matches!(self.desc.kind, LayerKind::Folder)
    }

    pub fn tree(&self) -> LayerTree {
        LayerTree { desc: self.desc.clone(), children: self.children.iter().map(|c| c.tree()).collect() }
    }

    pub fn byte_size(&self) -> u64 {
        let p: u64 = self.paint.as_ref().map(|p| p.all().iter().map(|t| t.byte_size()).sum()).unwrap_or(0);
        let m: u64 = self.mask_paint.values().map(|t| t.byte_size()).sum();
        p + m + self.children.iter().map(|c| c.byte_size()).sum::<u64>()
    }
}

// ---------------------------------------------------------------- tree helpers

pub fn find<'a>(layers: &'a [Layer], id: u64) -> Option<&'a Layer> {
    for l in layers {
        if l.desc.id == id {
            return Some(l);
        }
        if let Some(f) = find(&l.children, id) {
            return Some(f);
        }
    }
    None
}

pub fn find_mut<'a>(layers: &'a mut [Layer], id: u64) -> Option<&'a mut Layer> {
    for l in layers.iter_mut() {
        if l.desc.id == id {
            return Some(l);
        }
        if let Some(f) = find_mut(&mut l.children, id) {
            return Some(f);
        }
    }
    None
}

/// The list containing `id` and its index there.
pub fn locate_mut<'a>(layers: &'a mut Vec<Layer>, id: u64) -> Option<(&'a mut Vec<Layer>, usize)> {
    if let Some(i) = layers.iter().position(|l| l.desc.id == id) {
        return Some((layers, i));
    }
    for l in layers.iter_mut() {
        if let Some(r) = locate_mut(&mut l.children, id) {
            return Some(r);
        }
    }
    None
}

/// The layers of `ids` that are not inside another of them, bottom first
/// (stack order): moving or deleting a folder takes its children along.
pub fn selection_roots(layers: &[Layer], ids: &[u64]) -> Vec<u64> {
    fn walk(layers: &[Layer], ids: &[u64], out: &mut Vec<u64>) {
        for l in layers {
            if ids.contains(&l.desc.id) {
                out.push(l.desc.id);
            } else {
                walk(&l.children, ids, out);
            }
        }
    }
    let mut out = Vec::new();
    walk(layers, ids, &mut out);
    out
}

/// Id of the folder containing `id` (None at the root).
pub fn parent_of(layers: &[Layer], id: u64) -> Option<u64> {
    for l in layers {
        if l.children.iter().any(|c| c.desc.id == id) {
            return Some(l.desc.id);
        }
        if let Some(p) = parent_of(&l.children, id) {
            return Some(p);
        }
    }
    None
}

pub fn visit<'a>(layers: &'a [Layer], f: &mut dyn FnMut(&'a Layer)) {
    for l in layers {
        f(l);
        visit(&l.children, f);
    }
}

pub fn visit_mut(layers: &mut [Layer], f: &mut dyn FnMut(&mut Layer)) {
    for l in layers.iter_mut() {
        f(l);
        visit_mut(&mut l.children, f);
    }
}

// ---------------------------------------------------------------- resources

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResKind {
    Texture,
    Alpha,
    Stencil,
}

#[derive(Clone)]
pub struct Resource {
    pub id: u64,
    pub name: String,
    pub kind: ResKind,
    /// Original file bytes, kept for saving the project.
    pub encoded: Arc<Vec<u8>>,
    pub gpu: Arc<Tex>,
    pub image: Arc<image::RgbaImage>,
}

pub fn decode_image(bytes: &[u8]) -> anyhow::Result<image::RgbaImage> {
    let img = image::load_from_memory(bytes)?;
    // HDR sources are tonemapped into 8 bits: resources are material inputs.
    Ok(img.into_rgba8())
}

// ---------------------------------------------------------------- texture sets

/// Where a dragged layer lands relative to the row it is dropped on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    Above,
    Below,
    Into,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Content,
    /// The mask as a whole (paints into its top paint effect).
    Mask,
    MaskEffect(u64),
    /// An effect of the layer content.
    ContentEffect(u64),
}

impl Target {
    /// The mask or one of its effects is selected.
    pub fn is_mask(self) -> bool {
        matches!(self, Target::Mask | Target::MaskEffect(_))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AlphaMode {
    /// Alpha blend when the layer stack uses the opacity channel.
    Auto,
    Opaque,
    AlphaTest,
    AlphaBlend,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct SetSettings {
    /// Normal strength of painted height.
    pub height_scale: f32,
    pub ao_strength: f32,
    pub base_color: [f32; 3],
    /// roughness, metallic, height
    pub base_values: [f32; 3],
    pub alpha_mode: AlphaMode,
    pub alpha_cutoff: f32,
    pub emissive_intensity: f32,
    pub visible: bool,
    /// Optional channels enabled on this texture set.
    pub ext_channels: [bool; NEXT],
}

impl Default for SetSettings {
    fn default() -> Self {
        SetSettings {
            height_scale: 0.02,
            ao_strength: 1.0,
            base_color: [0.5, 0.5, 0.5],
            base_values: [0.5, 0.0, 0.0],
            alpha_mode: AlphaMode::Auto,
            alpha_cutoff: 0.5,
            emissive_intensity: 1.0,
            visible: true,
            ext_channels: [false; NEXT],
        }
    }
}

/// Everything history needs to restore one texture set's stack.
#[derive(Clone)]
pub struct SetState {
    pub layers: Vec<Layer>,
    pub selected: Option<u64>,
    pub target: Target,
    pub settings: SetSettings,
}

pub struct TextureSet {
    pub name: String,
    pub submesh: usize,
    /// Bottom first.
    pub layers: Vec<Layer>,
    /// Position map space to mesh space (projection placement).
    pub world: WorldMap,
    pub selected: Option<u64>,
    /// Other selected layers (Ctrl / Shift + click); `selected` is the one
    /// whose properties show.
    pub extra: Vec<u64>,
    pub target: Target,
    pub settings: SetSettings,
    pub gpu: TextureSetGpu,
    pub raster: bake::UvRaster,
    pub baked: bool,
    pub dirty: bool,
    /// Bumped when the mesh maps change (generators depend on them).
    pub bake_rev: u64,
    /// Layer whose top-level ancestor splits the cached prefix (the edited one).
    pub focus: Option<u64>,
    /// Texels painted since the last composite when nothing else changed
    /// (`dirty` wins): only that rect gets recomposited.
    pub dirty_region: Option<crate::engine::composite::Region>,
}

impl TextureSet {
    pub fn resolution(&self) -> u32 {
        self.gpu.resolution
    }

    pub fn selected_layer(&self) -> Option<&Layer> {
        self.selected.and_then(|id| find(&self.layers, id))
    }

    pub fn selected_layer_mut(&mut self) -> Option<&mut Layer> {
        let id = self.selected?;
        find_mut(&mut self.layers, id)
    }

    pub fn state(&self) -> SetState {
        SetState {
            layers: self.layers.clone(),
            selected: self.selected,
            target: self.target,
            settings: self.settings.clone(),
        }
    }

    /// Some visible layer writes the opacity channel.
    pub fn uses_opacity(&self) -> bool {
        fn any(layers: &[Layer]) -> bool {
            layers.iter().filter(|l| l.desc.visible).any(|l| match &l.desc.kind {
                LayerKind::Fill(f) => f.channels[CH_OPACITY].enabled,
                // Painted opacity only: the eraser and emissive / normal
                // strokes allocate the opacity texture too.
                LayerKind::Paint => l.desc.opacity_painted && l.paint.as_ref().is_some_and(|p| p.opac.is_some()),
                LayerKind::Folder => any(&l.children),
                LayerKind::Filter(_) => false,
            })
        }
        any(&self.layers)
    }

    /// Viewport shader alpha: 0 opaque, 1 alpha test, 2 alpha blend.
    pub fn shader_alpha(&self) -> f32 {
        match self.settings.alpha_mode {
            AlphaMode::Auto => {
                if self.uses_opacity() {
                    2.0
                } else {
                    0.0
                }
            }
            AlphaMode::Opaque => 0.0,
            AlphaMode::AlphaTest => 1.0,
            AlphaMode::AlphaBlend => 2.0,
        }
    }

    pub fn restore(&mut self, s: SetState) -> SetState {
        let old = self.state();
        self.layers = s.layers;
        self.selected = s.selected;
        self.extra.clear();
        self.target = s.target;
        self.settings = s.settings;
        self.dirty = true;
        old
    }

    /// Renders (or finds in the cache) the seamless procedural textures the
    /// visible stack uses.
    fn proc_textures(&self, engine: &Engine, enc: &mut wgpu::CommandEncoder) -> HashMap<u64, Arc<Tex>> {
        let res = self.resolution();
        let mut out = HashMap::new();
        let mut want = |p: &ProcParams| {
            if let Some(u) = p.gen_u(res) {
                out.entry(u.key()).or_insert_with(|| engine.procgen.get(&engine.device, enc, &u));
            }
        };
        fn walk(layers: &[Layer], f: &mut dyn FnMut(&ProcParams)) {
            for l in layers.iter().filter(|l| l.desc.visible) {
                if let LayerKind::Fill(fd) = &l.desc.kind {
                    for ch in fd.channels.iter().filter(|c| c.enabled && c.source == SourceKind::Procedural) {
                        f(&ch.procedural);
                    }
                }
                if let Some(m) = l.desc.mask.as_ref().filter(|m| m.enabled) {
                    for e in m.effects.iter().filter(|e| e.enabled) {
                        if let MaskEffectKind::Fill { source: SourceKind::Procedural, procedural, .. } = &e.kind {
                            f(procedural);
                        }
                    }
                }
                walk(&l.children, f);
            }
        }
        walk(&self.layers, &mut want);
        out
    }

    /// Every selected layer: `selected` plus the others.
    pub fn selection(&self) -> Vec<u64> {
        let mut v: Vec<u64> = self.extra.clone();
        v.extend(self.selected);
        v
    }

    /// Drops selected ids that no longer exist (or repeat the primary one).
    pub fn prune_selection(&mut self) {
        if self.extra.is_empty() {
            return;
        }
        let sel = self.selected;
        let layers = &self.layers;
        let mut seen = std::collections::HashSet::new();
        self.extra.retain(|id| Some(*id) != sel && find(layers, *id).is_some() && seen.insert(*id));
        if self.selected.is_none() {
            self.selected = self.extra.pop();
        }
    }

    /// Rows as the layer stack shows them, top to bottom (open folders only).
    pub fn visible_rows(&self) -> Vec<u64> {
        fn walk(layers: &[Layer], out: &mut Vec<u64>) {
            for l in layers.iter().rev() {
                out.push(l.desc.id);
                if l.is_folder() && l.desc.expanded {
                    walk(&l.children, out);
                }
            }
        }
        let mut out = Vec::new();
        walk(&self.layers, &mut out);
        out
    }

    /// Hash of what an undo step restores (layer descriptions and settings),
    /// serialized straight into a hasher. Selection and folder expansion are
    /// left out: like in Painter, clicking around is not an edit.
    pub fn fingerprint_hash(&self) -> u64 {
        use std::hash::Hasher;
        struct W(std::collections::hash_map::DefaultHasher);
        impl std::io::Write for W {
            fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
                self.0.write(b);
                Ok(b.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        fn walk(w: &mut W, layers: &[Layer]) {
            for l in layers {
                if l.desc.expanded {
                    let mut d = l.desc.clone();
                    d.expanded = false;
                    let _ = serde_json::to_writer(&mut *w, &d);
                } else {
                    let _ = serde_json::to_writer(&mut *w, &l.desc);
                }
                w.0.write_u8(b'(');
                walk(w, &l.children);
                w.0.write_u8(b')');
            }
        }
        let mut w = W(std::collections::hash_map::DefaultHasher::new());
        walk(&mut w, &self.layers);
        let _ = serde_json::to_writer(&mut w, &self.settings);
        w.0.finish()
    }

    /// Selection, target and folder expansion (what `fingerprint_hash` skips).
    pub fn view_hash(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.selected.hash(&mut h);
        format!("{:?}", self.target).hash(&mut h);
        visit(&self.layers, &mut |l| (l.desc.id, l.desc.expanded).hash(&mut h));
        h.finish()
    }

    #[allow(dead_code)]
    pub fn fingerprint(&self) -> String {
        let trees: Vec<LayerTree> = self.layers.iter().map(|l| l.tree()).collect();
        format!(
            "{}|{}|{:?}|{:?}",
            serde_json::to_string(&trees).unwrap_or_default(),
            serde_json::to_string(&self.settings).unwrap_or_default(),
            self.selected,
            self.target
        )
    }

    pub fn composite(
        &mut self,
        engine: &Engine,
        encoder: &mut wgpu::CommandEncoder,
        resources: &HashMap<u64, Resource>,
        capture_mask: Option<u64>,
    ) {
        let s = &self.settings;
        let clear = ClearU {
            col: [s.base_color[0], s.base_color[1], s.base_color[2], 1.0],
            mrh: [s.base_values[0], s.base_values[1], s.base_values[2], EXT_DEFAULTS[0]],
            emi: [0.0, 0.0, 0.0, EXT_DEFAULTS[1]],
            nrm: [0.0, 0.0, 1.0, 0.0],
        };
        let res = self.resolution();
        let procs = self.proc_textures(engine, encoder);
        let ctx = PlanCtx { resources, procs: &procs, res, capture: capture_mask, bake_rev: self.bake_rev, world: self.world };
        // Cache everything below the top-level ancestor of the edited layer.
        let k = self.focus.and_then(|f| self.layers.iter().position(|l| l.desc.id == f || find(&l.children, f).is_some()));
        // Planned in two parts: layers that add nothing have no plan entry,
        // so the split is the length of the lower part's plan.
        let mut plan = plan_list(&self.layers[..k.unwrap_or(0)], &ctx);
        let split = plan.len();
        plan.extend(plan_list(&self.layers[k.unwrap_or(0)..], &ctx));
        let prefix = k.map(|k| {
            let mut h = std::collections::hash_map::DefaultHasher::new();
            use std::hash::{Hash, Hasher};
            for l in self.layers[..k].iter().filter(|l| l.desc.visible) {
                hash_layer(l, &mut h);
            }
            serde_json::to_string(&self.settings).unwrap_or_default().hash(&mut h);
            (self.bake_rev, res).hash(&mut h);
            (split, h.finish())
        });
        engine.compositor.run(
            &engine.device,
            encoder,
            &self.gpu,
            clear,
            &plan,
            FinalU { k: [s.height_scale, s.ao_strength, 0.0, 0.0] },
            Some(&self.gpu.mask_view),
            prefix,
            if self.dirty { None } else { self.dirty_region },
        );
        self.dirty = false;
        self.dirty_region = None;
    }
}

impl TextureSet {
    /// Evaluates a fill material over this set into `out` (col, mrh, emi, nrm).
    pub fn evaluate_material(
        &self,
        engine: &Engine,
        encoder: &mut wgpu::CommandEncoder,
        resources: &HashMap<u64, Resource>,
        fill: &FillDesc,
        out: &[Tex; 4],
    ) {
        let s = &self.settings;
        let clear = ClearU {
            col: [s.base_color[0], s.base_color[1], s.base_color[2], 1.0],
            mrh: [s.base_values[0], s.base_values[1], 0.0, EXT_DEFAULTS[0]],
            emi: [0.0, 0.0, 0.0, EXT_DEFAULTS[1]],
            nrm: [0.0, 0.0, 1.0, 0.0],
        };
        let layer = Layer {
            desc: LayerDesc::new(0, String::new(), LayerKind::Fill(Box::new(fill.clone()))),
            rev: 0,
            paint: None,
            mask_paint: HashMap::new(),
            children: Vec::new(),
        };
        let mut procs = HashMap::new();
        for ch in fill.channels.iter().filter(|c| c.enabled && c.source == SourceKind::Procedural) {
            if let Some(u) = ch.procedural.gen_u(self.resolution()) {
                procs.insert(u.key(), engine.procgen.get(&engine.device, encoder, &u));
            }
        }
        let ctx = PlanCtx { resources, procs: &procs, res: self.resolution(), capture: None, bake_rev: self.bake_rev, world: self.world };
        if let Some(p) = plan_layer(&layer, &ctx) {
            engine.compositor.evaluate_into(&engine.device, encoder, &self.gpu, clear, &[p], out);
        }
    }
}

fn lod_for(tex: &Tex, scale: f32, res: u32) -> f32 {
    ((tex.width as f32 * scale.abs().max(1e-3)) / res as f32).log2().max(0.0)
}

struct PlanCtx<'a> {
    resources: &'a HashMap<u64, Resource>,
    /// Seamless procedural textures by generator key.
    procs: &'a HashMap<u64, Arc<Tex>>,
    res: u32,
    capture: Option<u64>,
    bake_rev: u64,
    world: WorldMap,
}

/// Description + painted revision of a layer subtree (cache keys).
fn hash_layer(l: &Layer, h: &mut std::collections::hash_map::DefaultHasher) {
    use std::hash::Hash;
    serde_json::to_string(&l.desc).unwrap_or_default().hash(h);
    l.rev.hash(h);
    for c in &l.children {
        hash_layer(c, h);
    }
}

fn plan_list<'a>(layers: &'a [Layer], ctx: &PlanCtx<'a>) -> Vec<Plan<'a>> {
    layers.iter().filter(|l| l.desc.visible).filter_map(|l| plan_layer(l, ctx)).collect()
}

fn plan_mask<'a>(l: &'a Layer, ctx: &PlanCtx<'a>) -> Option<MaskPlan<'a>> {
    let (resources, res, capture) = (ctx.resources, ctx.res, ctx.capture);
    let m = l.desc.mask.as_ref()?;
    if !m.enabled {
        return None;
    }
    let mut steps = Vec::new();
    for e in m.effects.iter().filter(|e| e.enabled) {
        let mut u = MaskU { o: [e.opacity, 0.0, 0.0, 0.0], ..Default::default() };
        let blend = e.blend.code();
        let step = match &e.kind {
            MaskEffectKind::Paint => {
                u.k = [1, blend, 0, 0];
                MaskStep::Effect { params: u, paint: l.mask_paint.get(&e.id).map(|t| &**t), texture: None }
            }
            MaskEffectKind::Fill { value, source, texture, procedural, projection, range } => {
                let generated = (*source == SourceKind::Procedural)
                    .then(|| procedural.gen_u(ctx.res))
                    .flatten()
                    .and_then(|g| ctx.procs.get(&g.key()))
                    .map(|t| &**t);
                let tex = if *source == SourceKind::Texture { texture.and_then(|id| resources.get(&id)).map(|r| &*r.gpu) } else { generated };
                let source = if *source == SourceKind::Texture && tex.is_none() { SourceKind::Uniform } else { *source };
                // Painter's procedurals: a texture mapped onto the range.
                let code = if generated.is_some() { SourceKind::Texture.code() } else { source.code() };
                u.k = [2, blend, 0, code];
                u.o[1] = *value;
                u.o[2] = tex.map(|t| lod_for(t, projection.scale, res)).unwrap_or(0.0);
                let (lo, hi) = if source == SourceKind::Procedural && procedural.invert { (range[1], range[0]) } else { (range[0], range[1]) };
                u.sv = [lo, hi, procedural.kind.analytic_id() as f32, procedural.scale.max(1e-4)];
                u.pa = procedural.pa();
                u.pb = procedural.pb();
                let pu = projection.uniform(&ctx.world);
                u.proj = pu.proj;
                u.offs = pu.offs;
                u.xf = pu.xf;
                u.nr = pu.nr;
                MaskStep::Effect { params: u, paint: None, texture: tex }
            }
            MaskEffectKind::Generator(g) => {
                u.k = [3, blend, g.kind.code(), 0];
                u.g0 = [g.level, g.contrast, if g.invert { 1.0 } else { 0.0 }, g.noise];
                u.g1 = [g.noise_scale, g.seed, g.axis as f32, g.height_reactive];
                u.dir = [g.dir[0], g.dir[1], g.dir[2], 0.0];
                MaskStep::Effect { params: u, paint: None, texture: None }
            }
            MaskEffectKind::Levels { in_lo, in_hi, gamma, out_lo, out_hi } => {
                u.k = [4, 0, 0, 0];
                u.lv = [*in_lo, *in_hi, *gamma, *out_lo];
                u.lv2 = [*out_hi, 0.0, 0.0, 0.0];
                MaskStep::Effect { params: u, paint: None, texture: None }
            }
            MaskEffectKind::Invert => {
                u.k = [5, 0, 0, 0];
                MaskStep::Effect { params: u, paint: None, texture: None }
            }
            MaskEffectKind::ColorSelect { colors, tolerance } => {
                u.k = [6, blend, 0, 0];
                u.dir[3] = *tolerance;
                u.lv2[3] = colors.len().min(8) as f32;
                for (i, c) in colors.iter().take(8).enumerate() {
                    u.colors[i] = [c[0], c[1], c[2], 1.0];
                }
                MaskStep::Effect { params: u, paint: None, texture: None }
            }
            MaskEffectKind::Threshold { value } => {
                u.k = [7, 0, 0, 0];
                u.lv2[1] = *value;
                MaskStep::Effect { params: u, paint: None, texture: None }
            }
            MaskEffectKind::Blur { radius } => MaskStep::Blur { radius: *radius, opacity: e.opacity },
        };
        steps.push(step);
    }
    let hash = {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        serde_json::to_string(m).unwrap_or_default().hash(&mut h);
        (l.rev, ctx.bake_rev, res).hash(&mut h);
        h.finish()
    };
    // Masks reading the stack below cannot be cached on their own inputs.
    let reads_stack = m.effects.iter().any(|e| matches!(&e.kind, MaskEffectKind::Generator(g) if e.enabled && g.height_reactive > 0.0));
    let cache = (!reads_stack).then_some((l.desc.id, hash));
    Some(MaskPlan { base: m.base, steps, capture: capture == Some(l.desc.id), cache })
}

fn plan_layer<'a>(l: &'a Layer, ctx: &PlanCtx<'a>) -> Option<Plan<'a>> {
    let fx: Vec<&ContentEffect> = l.desc.effects.iter().filter(|e| e.enabled).collect();
    if fx.is_empty() || l.is_folder() {
        return plan_layer_inner(l, ctx, plan_mask(l, ctx));
    }
    // Content effects: [layer, effects...] as a group, the mask on the group.
    let mut children = vec![plan_layer_inner(l, ctx, None)?];
    for e in fx {
        children.push(filter_plan(&e.filter, [e.opacity; 4], [e.opacity, e.opacity, e.opacity, 0.0], None));
    }
    Some(Plan::Folder {
        params: FolderU { op_a: [1.0; 4], op_b: [1.0; 4], flags: [0; 4] },
        children,
        mask: plan_mask(l, ctx),
    })
}

fn filter_plan<'a>(f: &FilterDesc, op_a: [f32; 4], op_b: [f32; 4], mask: Option<MaskPlan<'a>>) -> Plan<'a> {
    let bits = f.channels.iter().enumerate().fold(0u32, |acc, (i, &c)| acc | ((c as u32) << i));
    Plan::Filter {
        params: FilterU {
            k: [f.kind.code(), bits, 0, 0],
            op_a,
            op_b,
            p0: [f.p[0], f.p[1], f.p[2], f.p[3]],
            p1: [f.p[4], f.p[5], f.p[6], f.p[7]],
            color: [f.color[0], f.color[1], f.color[2], 1.0],
            color2: [f.color2[0], f.color2[1], f.color2[2], 1.0],
        },
        blur_radius: f.blur_radius(),
        mask,
    }
}

fn plan_layer_inner<'a>(l: &'a Layer, ctx: &PlanCtx<'a>, mask: Option<MaskPlan<'a>>) -> Option<Plan<'a>> {
    let (resources, res) = (ctx.resources, ctx.res);
    let d = &l.desc;
    Some(match &d.kind {
        LayerKind::Paint => {
            let p = l.paint.as_ref()?;
            // Nothing painted yet: the layer adds nothing (its content
            // effects still filter the stack, so keep it then).
            if p.is_empty() && d.effects.is_empty() {
                return None;
            }
            Plan::Paint {
                blend: d.blend_u(),
                tex: PaintRefs {
                    color: &p.color,
                    vals: &p.vals,
                    alph: &p.alph,
                    emis: p.emis.as_deref(),
                    nrm: p.nrm.as_deref(),
                    opac: p.opac.as_deref(),
                },
                mask,
            }
        }
        LayerKind::Fill(f) => {
            let pu = f.projection.uniform(&ctx.world);
            let b = d.blend_u();
            let mut u = FillU { mode_a: b.mode_a, mode_b: b.mode_b, op_a: b.op_a, op_b: b.op_b, proj: pu.proj, offs: pu.offs, xf: pu.xf, nr: pu.nr, ..Default::default() };
            let mut textures: [Option<&Tex>; NCH] = [None; NCH];
            for (c, ch) in f.channels.iter().enumerate() {
                let generated = (ch.source == SourceKind::Procedural)
                    .then(|| ch.procedural.gen_u(ctx.res))
                    .flatten()
                    .and_then(|u| ctx.procs.get(&u.key()))
                    .map(|t| &**t);
                let tex = if ch.source == SourceKind::Texture {
                    ch.texture.and_then(|id| resources.get(&id)).map(|r| &*r.gpu)
                } else {
                    generated
                };
                let source = if ch.source == SourceKind::Texture && tex.is_none() { SourceKind::Uniform } else { ch.source };
                textures[c] = tex;
                let mut flags = 0;
                if source == SourceKind::Texture && is_color_channel(c) {
                    flags |= 1;
                }
                // Painter's procedurals: sampled like a bitmap, mapped onto
                // the channel's two values.
                let source = if generated.is_some() {
                    flags |= 4;
                    SourceKind::Texture
                } else {
                    source
                };
                if ch.flip_green {
                    flags |= 2;
                }
                let mut a = [ch.value[0], ch.value[1], ch.value[2], 0.0];
                if source == SourceKind::Texture && c == CH_NORMAL {
                    a[0] = ch.value_b[0]; // normal texture strength
                }
                let mut b = [ch.value_b[0], ch.value_b[1], ch.value_b[2], 0.0];
                if (source == SourceKind::Procedural || generated.is_some()) && ch.procedural.invert {
                    std::mem::swap(&mut a, &mut b);
                }
                u.ch[c] = ChanSrcU {
                    k: [source.code(), ch.procedural.kind.code(), flags, ch.enabled as u32],
                    a,
                    b,
                    pa: ch.procedural.pa(),
                    pb: ch.procedural.pb(),
                    t: [tex.map(|t| lod_for(t, f.projection.scale, res)).unwrap_or(0.0), c as f32, 0.0, ch.procedural.scale.max(1e-4)],
                };
                u.ch[c].k[1] = ch.procedural.kind.analytic_id();
            }
            for (i, ch) in f.ext.iter().enumerate() {
                // Texture sources are not available on optional channels yet.
                let source = if ch.source == SourceKind::Texture { SourceKind::Uniform } else { ch.source };
                let (mut a, mut b) = ([ch.value[0], ch.value[1], ch.value[2], 0.0], [ch.value_b[0], ch.value_b[1], ch.value_b[2], 0.0]);
                if source == SourceKind::Procedural && ch.procedural.invert {
                    std::mem::swap(&mut a, &mut b);
                }
                u.ch[NCH + i] = ChanSrcU {
                    k: [source.code(), ch.procedural.kind.code(), 0, ch.enabled as u32],
                    a,
                    b,
                    pa: ch.procedural.pa(),
                    pb: ch.procedural.pb(),
                    t: [0.0, (NCH + i) as f32, 0.0, ch.procedural.scale.max(1e-4)],
                };
            }
            Plan::Fill { fill: Box::new(u), textures, mask }
        }
        LayerKind::Filter(f) => {
            let b = d.blend_u();
            filter_plan(f, b.op_a, b.op_b, mask)
        }
        LayerKind::Folder => {
            let b = d.blend_u();
            Plan::Folder {
                params: FolderU { op_a: b.op_a, op_b: b.op_b, flags: [0; 4] },
                children: plan_list(&l.children, ctx),
                mask,
            }
        }
    })
}

// ---------------------------------------------------------------- document

pub struct Document {
    pub mesh: Arc<MeshData>,
    pub gpu_mesh: GpuMesh,
    pub mesh_name: String,
    pub sets: Vec<TextureSet>,
    pub active: usize,
    pub next_id: u64,
    pub path: Option<std::path::PathBuf>,
    pub bake_settings: bake::BakeSettings,
    pub modified: bool,
    pub resources: HashMap<u64, Resource>,
    /// Picking acceleration over every triangle of the mesh.
    pub bvh: Arc<bake::Bvh>,
    /// BVH triangle id -> (submesh, triangle index inside it).
    pub pick_map: Vec<(u32, u32)>,
    /// Geometry mask: hidden triangles per submesh.
    pub hidden: Vec<Vec<bool>>,
}

pub struct Hit {
    pub set: usize,
    pub tri: u32,
    pub pos: glam::Vec3,
    pub normal: glam::Vec3,
    pub uv: [f32; 2],
    pub dist: f32,
}

impl Document {
    pub fn new(engine: &Engine, mesh: MeshData, mesh_name: String, res: u32) -> Self {
        let gpu_mesh = GpuMesh::new(&engine.device, &mesh);
        let mesh = Arc::new(mesh);
        let subs: Vec<(usize, u32)> = (0..mesh.submeshes.len()).map(|i| (i, res)).collect();
        let sets = Self::make_sets(engine, &mesh, &subs);
        let (bvh, pick_map) = Self::picking(&mesh);
        let mut doc = Document {
            mesh,
            gpu_mesh,
            mesh_name,
            sets,
            active: 0,
            next_id: 1,
            path: None,
            bake_settings: bake::BakeSettings::default(),
            modified: false,
            resources: HashMap::new(),
            bvh,
            pick_map,
            hidden: Vec::new(),
        };
        doc.reset_hidden();
        for s in 0..doc.sets.len() {
            doc.add_layer(engine, s, LayerKind::Paint);
        }
        doc.modified = false;
        doc
    }

    pub fn picking(mesh: &MeshData) -> (Arc<bake::Bvh>, Vec<(u32, u32)>) {
        let tris = bake::mesh_triangles(mesh);
        let mut map = Vec::with_capacity(tris.len());
        for (si, s) in mesh.submeshes.iter().enumerate() {
            for t in 0..s.indices.len() / 3 {
                map.push((si as u32, t as u32));
            }
        }
        (Arc::new(bake::build_bvh(mesh, &tris)), map)
    }

    pub fn make_set(engine: &Engine, mesh: &MeshData, sub: usize, res: u32) -> TextureSet {
        let cpu = SetMaps::new(mesh, sub, res);
        Self::finish_set(engine, mesh, sub, res, cpu)
    }

    /// Several texture sets: the CPU maps of all of them in parallel.
    pub fn make_sets(engine: &Engine, mesh: &MeshData, sets: &[(usize, u32)]) -> Vec<TextureSet> {
        use rayon::prelude::*;
        let maps: Vec<SetMaps> = sets.par_iter().map(|&(sub, res)| SetMaps::new(mesh, sub, res)).collect();
        sets.iter().zip(maps).map(|(&(sub, res), m)| Self::finish_set(engine, mesh, sub, res, m)).collect()
    }

    fn finish_set(engine: &Engine, mesh: &MeshData, sub: usize, res: u32, maps: SetMaps) -> TextureSet {
        let SetMaps { raster, nearest, position, wnormal } = maps;
        let gpu = TextureSetGpu::new(engine, res, &nearest, &position, &wnormal);
        TextureSet {
            name: mesh.submeshes[sub].name.clone(),
            submesh: sub,
            layers: Vec::new(),
            world: WorldMap::of(mesh),
            selected: None,
            extra: Vec::new(),
            target: Target::Content,
            settings: SetSettings::default(),
            gpu,
            raster,
            baked: false,
            dirty: true,
            bake_rev: 0,
            focus: None,
            dirty_region: None,
        }
    }

    pub fn alloc_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    pub fn reset_hidden(&mut self) {
        self.hidden = self.mesh.submeshes.iter().map(|s| vec![false; s.indices.len() / 3]).collect();
    }

    pub fn any_hidden(&self) -> bool {
        self.hidden.iter().any(|v| v.iter().any(|h| *h))
    }

    /// Hides or shows triangles of a submesh and rebuilds its draw buffer.
    pub fn set_hidden(&mut self, engine: &Engine, sub: usize, tris: &[u32], hide: bool) {
        if self.hidden.len() != self.mesh.submeshes.len() {
            self.reset_hidden();
        }
        for &t in tris {
            if let Some(h) = self.hidden[sub].get_mut(t as usize) {
                *h = hide;
            }
        }
        self.refresh_visibility(engine);
    }

    /// Rebuilds every submesh's visible index buffer from `hidden`.
    pub fn refresh_visibility(&mut self, engine: &Engine) {
        use wgpu::util::DeviceExt;
        for (si, s) in self.mesh.submeshes.iter().enumerate() {
            let hidden = self.hidden.get(si);
            let gs = &mut self.gpu_mesh.submeshes[si];
            if !hidden.is_some_and(|h| h.iter().any(|x| *x)) {
                gs.visible = None;
                continue;
            }
            let h = hidden.unwrap();
            let idx: Vec<u32> = s
                .indices
                .chunks_exact(3)
                .enumerate()
                .filter(|(t, _)| !h[*t])
                .flat_map(|(_, c)| c.iter().copied())
                .collect();
            let n = idx.len() as u32;
            let data = if idx.is_empty() { vec![0u32; 3] } else { idx };
            let buf = engine.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("visible indices"),
                contents: bytemuck::cast_slice(&data),
                usage: wgpu::BufferUsages::INDEX,
            });
            gs.visible = Some((buf, n));
        }
    }

    /// Ray cast against the mesh: closest visible hit with interpolated data.
    pub fn pick(&self, origin: glam::Vec3, dir: glam::Vec3) -> Option<Hit> {
        let mut o = origin;
        let mut travelled = 0.0;
        let eps = self.mesh.diagonal() * 1e-5;
        let mut found = None;
        for _ in 0..64 {
            let h = self.bvh.trace(o, dir, f32::MAX, false)?;
            let (sub, tri) = self.pick_map[h.tri as usize];
            let hidden = self.hidden.get(sub as usize).and_then(|v| v.get(tri as usize)).copied().unwrap_or(false);
            if !hidden {
                found = Some((h, travelled));
                break;
            }
            travelled += h.t + eps;
            o += dir * (h.t + eps);
        }
        let (h, travelled) = found?;
        let origin = o;
        let (sub, tri) = self.pick_map[h.tri as usize];
        let idx = &self.mesh.submeshes[sub as usize].indices[tri as usize * 3..tri as usize * 3 + 3];
        let v = |i: usize| &self.mesh.vertices[idx[i] as usize];
        let (w0, w1, w2) = (1.0 - h.u - h.v, h.u, h.v);
        let n = (glam::Vec3::from(v(0).normal) * w0 + glam::Vec3::from(v(1).normal) * w1 + glam::Vec3::from(v(2).normal) * w2)
            .normalize_or(glam::Vec3::Y);
        let uv = [
            v(0).uv[0] * w0 + v(1).uv[0] * w1 + v(2).uv[0] * w2,
            v(0).uv[1] * w0 + v(1).uv[1] * w1 + v(2).uv[1] * w2,
        ];
        let set = self.sets.iter().position(|s| s.submesh == sub as usize)?;
        Some(Hit { set, tri, pos: origin + dir * h.t, normal: n, uv, dist: h.t + travelled })
    }

    pub fn new_layer(&mut self, engine: &Engine, set: usize, kind: LayerKind, name: Option<String>) -> Layer {
        let id = self.alloc_id();
        let res = self.sets[set].resolution();
        let mut count = 0;
        visit(&self.sets[set].layers, &mut |l| {
            if std::mem::discriminant(&l.desc.kind) == std::mem::discriminant(&kind) {
                count += 1;
            }
        });
        let name = name.unwrap_or_else(|| match &kind {
            LayerKind::Paint => format!("Layer {}", count + 1),
            LayerKind::Fill(_) => format!("Fill layer {}", count + 1),
            LayerKind::Filter(f) => format!("{} {}", f.kind.name(), count + 1),
            LayerKind::Folder => format!("Folder {}", count + 1),
        });
        let _ = res;
        let paint = matches!(kind, LayerKind::Paint).then(|| PaintTex::empty(&engine.device));
        Layer { desc: LayerDesc::new(id, name, kind), rev: 0, paint, mask_paint: HashMap::new(), children: Vec::new() }
    }

    /// Inserts above the selection (inside the selected folder if one is
    /// selected and expanded) and selects the new layer.
    pub fn insert_layer(&mut self, set: usize, layer: Layer) {
        let id = layer.desc.id;
        let s = &mut self.sets[set];
        let sel = s.selected;
        let mut layer = Some(layer);
        if let Some(sel) = sel {
            if let Some(f) = find_mut(&mut s.layers, sel) {
                if f.is_folder() && f.desc.expanded {
                    f.children.push(layer.take().unwrap());
                }
            }
            if let Some(l) = layer.take() {
                match locate_mut(&mut s.layers, sel) {
                    Some((list, i)) => list.insert(i + 1, l),
                    None => s.layers.push(l),
                }
            }
        } else {
            s.layers.push(layer.take().unwrap());
        }
        s.selected = Some(id);
        s.target = Target::Content;
        s.dirty = true;
        self.modified = true;
    }

    pub fn add_layer(&mut self, engine: &Engine, set: usize, kind: LayerKind) -> u64 {
        let l = self.new_layer(engine, set, kind, None);
        let id = l.desc.id;
        self.insert_layer(set, l);
        id
    }

    /// Builds layers from a saved tree with fresh ids (smart materials).
    pub fn instantiate(&mut self, engine: &Engine, set: usize, tree: &LayerTree) -> Layer {
        let res = self.sets[set].resolution();
        let mut desc = tree.desc.clone();
        desc.id = self.alloc_id();
        let mut mask_paint = HashMap::new();
        if let Some(m) = &mut desc.mask {
            for e in &mut m.effects {
                e.id = self.alloc_id();
                if matches!(e.kind, MaskEffectKind::Paint) {
                    mask_paint.insert(e.id, new_mask_paint(&engine.device, res));
                }
            }
        }
        let paint = matches!(desc.kind, LayerKind::Paint).then(|| PaintTex::empty(&engine.device));
        let children = tree.children.iter().map(|c| self.instantiate(engine, set, c)).collect();
        Layer { desc, rev: 0, paint, mask_paint, children }
    }

    pub fn duplicate_layer(&mut self, engine: &Engine, set: usize) {
        let Some(sel) = self.sets[set].selected else { return };
        let Some(src) = find(&self.sets[set].layers, sel).cloned() else { return };
        let mut enc = engine.device.create_command_encoder(&Default::default());
        let copy = self.deep_copy(engine, &src, &mut enc);
        engine.queue.submit(Some(enc.finish()));
        let s = &mut self.sets[set];
        let id = copy.desc.id;
        if let Some((list, i)) = locate_mut(&mut s.layers, sel) {
            list.insert(i + 1, copy);
        }
        s.selected = Some(id);
        s.target = Target::Content;
        s.dirty = true;
        self.modified = true;
    }

    fn deep_copy(&mut self, engine: &Engine, src: &Layer, enc: &mut wgpu::CommandEncoder) -> Layer {
        let dup = |t: &Arc<Tex>, enc: &mut wgpu::CommandEncoder| Arc::new(t.duplicate(&engine.device, enc, "layer copy"));
        let mut desc = src.desc.clone();
        desc.id = self.alloc_id();
        desc.name = format!("{} copy", desc.name);
        let paint = src.paint.as_ref().map(|p| PaintTex {
            color: dup(&p.color, enc),
            vals: dup(&p.vals, enc),
            alph: dup(&p.alph, enc),
            emis: p.emis.as_ref().map(|t| dup(t, enc)),
            nrm: p.nrm.as_ref().map(|t| dup(t, enc)),
            opac: p.opac.as_ref().map(|t| dup(t, enc)),
        });
        let mut mask_paint = HashMap::new();
        if let Some(m) = &mut desc.mask {
            for e in &mut m.effects {
                let new_id = self.alloc_id();
                if let Some(t) = src.mask_paint.get(&e.id) {
                    mask_paint.insert(new_id, dup(t, enc));
                }
                e.id = new_id;
            }
        }
        let children = src.children.iter().map(|c| self.deep_copy(engine, c, enc)).collect();
        Layer { desc, rev: 0, paint, mask_paint, children }
    }

    pub fn delete_layer(&mut self, set: usize) {
        let s = &mut self.sets[set];
        let Some(sel) = s.selected else { return };
        let parent = parent_of(&s.layers, sel);
        if let Some((list, i)) = locate_mut(&mut s.layers, sel) {
            list.remove(i);
            let next = if list.is_empty() { None } else { Some(list[i.saturating_sub(1).min(list.len() - 1)].desc.id) };
            s.selected = next.or(parent);
        }
        if s.selected.is_none() {
            s.selected = s.layers.last().map(|l| l.desc.id);
        }
        s.target = Target::Content;
        s.dirty = true;
        self.modified = true;
    }

    /// dir > 0 moves toward the top of its list.
    pub fn move_layer(&mut self, set: usize, dir: i32) {
        let s = &mut self.sets[set];
        let Some(sel) = s.selected else { return };
        if let Some((list, i)) = locate_mut(&mut s.layers, sel) {
            let j = i as i32 + dir;
            if j >= 0 && (j as usize) < list.len() {
                list.swap(i, j as usize);
                s.dirty = true;
                self.modified = true;
            }
        }
    }

    /// Drag and drop: moves `src` just above `target` (into `target` when it
    /// is an expanded folder).
    /// Moves `src` next to `target`: above, below or into it (a folder).
    /// None keeps the old rule: into an expanded folder, above anything else.
    pub fn move_layer_onto(&mut self, set: usize, src: u64, target: u64, place: Option<Place>) {
        self.move_layers_onto(set, &[src], target, place);
    }

    /// Moves several layers next to `target` (above, below or into a
    /// folder), keeping their stack order; a selected folder brings its
    /// children. They stay selected.
    pub fn move_layers_onto(&mut self, set: usize, ids: &[u64], target: u64, place: Option<Place>) {
        let s = &mut self.sets[set];
        let roots = selection_roots(&s.layers, ids);
        if roots.is_empty() || roots.contains(&target) {
            return;
        }
        // Never move a folder into itself.
        if roots.iter().any(|id| find(&s.layers, *id).is_some_and(|l| find(&l.children, target).is_some())) {
            return;
        }
        let place = match place {
            Some(p) => p,
            None if find(&s.layers, target).is_some_and(|t| t.is_folder() && t.desc.expanded) => Place::Into,
            None => Place::Above,
        };
        let into = place == Place::Into && find(&s.layers, target).is_some_and(|t| t.is_folder());
        // Bottom first, like the lists.
        let mut moved = Vec::new();
        for id in &roots {
            if let Some((list, i)) = locate_mut(&mut s.layers, *id) {
                moved.push(list.remove(i));
            }
        }
        if into {
            if let Some(t) = find_mut(&mut s.layers, target) {
                t.children.extend(moved);
                t.desc.expanded = true;
            }
        } else if let Some((list, i)) = locate_mut(&mut s.layers, target) {
            let at = if place == Place::Below { i } else { i + 1 };
            for (k, l) in moved.into_iter().enumerate() {
                list.insert(at + k, l);
            }
        } else {
            s.layers.extend(moved);
        }
        let primary = s.selected.filter(|p| roots.contains(p)).or(roots.last().copied());
        s.selected = primary;
        s.extra = roots.into_iter().filter(|id| Some(*id) != primary).collect();
        s.target = Target::Content;
        s.dirty = true;
        self.modified = true;
    }

    /// Wraps several layers in a new folder placed where the topmost was.
    pub fn group_layers(&mut self, engine: &Engine, set: usize, ids: &[u64]) {
        let roots = selection_roots(&self.sets[set].layers, ids);
        let Some(&top) = roots.last() else { return };
        let folder = self.new_layer(engine, set, LayerKind::Folder, None);
        let fid = folder.desc.id;
        let s = &mut self.sets[set];
        match locate_mut(&mut s.layers, top) {
            Some((list, i)) => list.insert(i + 1, folder),
            None => return,
        }
        let mut moved = Vec::new();
        for id in &roots {
            if let Some((list, i)) = locate_mut(&mut s.layers, *id) {
                moved.push(list.remove(i));
            }
        }
        if let Some(f) = find_mut(&mut s.layers, fid) {
            f.children = moved;
            f.desc.expanded = true;
        }
        s.selected = Some(fid);
        s.extra.clear();
        s.target = Target::Content;
        s.dirty = true;
        self.modified = true;
    }

    /// Deletes several layers (a folder with its children).
    pub fn delete_layers(&mut self, set: usize, ids: &[u64]) {
        let s = &mut self.sets[set];
        let roots = selection_roots(&s.layers, ids);
        // The row just below the lowest deleted one keeps the focus.
        let rows = s.visible_rows();
        let doomed = |id: u64, layers: &[Layer]| roots.iter().any(|r| *r == id || find(layers, *r).is_some_and(|l| find(&l.children, id).is_some()));
        let last = rows.iter().rposition(|id| doomed(*id, &s.layers));
        let next = last.and_then(|k| rows[k + 1..].iter().chain(rows[..k].iter().rev()).find(|id| !doomed(**id, &s.layers)).copied());
        for id in &roots {
            if let Some((list, i)) = locate_mut(&mut s.layers, *id) {
                list.remove(i);
            }
        }
        s.selected = next;
        s.extra.clear();
        s.target = Target::Content;
        s.dirty = true;
        self.modified = true;
    }

    /// Duplicates several layers, each copy above its source; the copies
    /// end up selected.
    pub fn duplicate_layers(&mut self, engine: &Engine, set: usize, ids: &[u64]) {
        let roots = selection_roots(&self.sets[set].layers, ids);
        let mut copies = Vec::new();
        for id in roots {
            self.sets[set].selected = Some(id);
            self.duplicate_layer(engine, set);
            copies.extend(self.sets[set].selected);
        }
        let s = &mut self.sets[set];
        s.selected = copies.pop();
        s.extra = copies;
    }

    /// Wraps the selected layer in a new folder.
    pub fn group_selected(&mut self, engine: &Engine, set: usize) {
        let Some(sel) = self.sets[set].selected else { return };
        let folder = self.new_layer(engine, set, LayerKind::Folder, None);
        let fid = folder.desc.id;
        let s = &mut self.sets[set];
        if let Some((list, i)) = locate_mut(&mut s.layers, sel) {
            let l = list.remove(i);
            let mut folder = folder;
            folder.children.push(l);
            list.insert(i, folder);
            s.selected = Some(fid);
            s.target = Target::Content;
            s.dirty = true;
            self.modified = true;
        }
    }

    /// Moves the selected layer out of its folder, just above it.
    pub fn ungroup_selected(&mut self, set: usize) {
        let s = &mut self.sets[set];
        let Some(sel) = s.selected else { return };
        let Some(parent) = parent_of(&s.layers, sel) else { return };
        let layer = match locate_mut(&mut s.layers, sel) {
            Some((list, i)) => list.remove(i),
            None => return,
        };
        if let Some((list, i)) = locate_mut(&mut s.layers, parent) {
            list.insert(i + 1, layer);
        }
        s.dirty = true;
        self.modified = true;
    }

    /// Moves the selected layer into the folder just below it in its list.
    pub fn move_into_folder_below(&mut self, set: usize) {
        let s = &mut self.sets[set];
        let Some(sel) = s.selected else { return };
        if let Some((list, i)) = locate_mut(&mut s.layers, sel) {
            if i == 0 || !list[i - 1].is_folder() {
                return;
            }
            let l = list.remove(i);
            list[i - 1].children.push(l);
            list[i - 1].desc.expanded = true;
            s.dirty = true;
            self.modified = true;
        }
    }

    pub fn add_mask(&mut self, engine: &Engine, set: usize, white: bool) {
        let res = self.sets[set].resolution();
        let eid = self.alloc_id();
        let s = &mut self.sets[set];
        let Some(l) = s.selected_layer_mut() else { return };
        // Like Painter, the new mask has no effects: painting on it creates
        // its paint effect on demand.
        let _ = (engine, res, eid);
        let m = MaskDesc { base: if white { 1.0 } else { 0.0 }, ..Default::default() };
        l.mask_paint.clear();
        l.desc.mask = Some(m);
        s.target = Target::Mask;
        s.dirty = true;
        self.modified = true;
    }

    /// Adds an effect on top of the selected layer's mask (creating a black mask
    /// first if needed) and selects it.
    pub fn add_mask_effect(&mut self, engine: &Engine, set: usize, kind: MaskEffectKind) -> Option<u64> {
        let res = self.sets[set].resolution();
        let eid = self.alloc_id();
        let s = &mut self.sets[set];
        let l = s.selected_layer_mut()?;
        let m = l.desc.mask.get_or_insert_with(|| MaskDesc { base: 0.0, ..Default::default() });
        let paint = matches!(kind, MaskEffectKind::Paint);
        m.effects.push(MaskEffect { id: eid, enabled: true, blend: MaskBlend::Normal, opacity: 1.0, kind });
        if paint {
            l.mask_paint.insert(eid, new_mask_paint(&engine.device, res));
        }
        s.target = Target::MaskEffect(eid);
        s.dirty = true;
        self.modified = true;
        Some(eid)
    }

    /// Adds a filter effect on the selected layer's content.
    pub fn add_content_effect(&mut self, set: usize, filter: FilterDesc) -> Option<u64> {
        let id = self.alloc_id();
        let s = &mut self.sets[set];
        let l = s.selected_layer_mut()?;
        if l.is_folder() {
            return None;
        }
        l.desc.effects.push(ContentEffect { id, enabled: true, opacity: 1.0, filter });
        s.target = Target::ContentEffect(id);
        s.dirty = true;
        self.modified = true;
        Some(id)
    }

    pub fn remove_content_effect(&mut self, set: usize, id: u64) {
        let s = &mut self.sets[set];
        if let Some(l) = s.selected_layer_mut() {
            l.desc.effects.retain(|e| e.id != id);
        }
        s.target = Target::Content;
        s.dirty = true;
        self.modified = true;
    }

    /// Moves a content effect up (+1, toward the top of the list) or down.
    pub fn move_content_effect(&mut self, set: usize, id: u64, d: i32) {
        let s = &mut self.sets[set];
        let Some(l) = s.selected_layer_mut() else { return };
        let Some(i) = l.desc.effects.iter().position(|e| e.id == id) else { return };
        let j = i as i32 + d;
        if j >= 0 && (j as usize) < l.desc.effects.len() {
            l.desc.effects.swap(i, j as usize);
            s.dirty = true;
            self.modified = true;
        }
    }

    pub fn remove_mask(&mut self, set: usize) {
        let s = &mut self.sets[set];
        let Some(l) = s.selected_layer_mut() else { return };
        l.desc.mask = None;
        l.mask_paint.clear();
        s.target = Target::Content;
        s.dirty = true;
        self.modified = true;
    }

    pub fn remove_mask_effect(&mut self, set: usize, eid: u64) {
        let s = &mut self.sets[set];
        let Some(l) = s.selected_layer_mut() else { return };
        if let Some(m) = &mut l.desc.mask {
            m.effects.retain(|e| e.id != eid);
        }
        l.mask_paint.remove(&eid);
        s.target = Target::Mask;
        s.dirty = true;
        self.modified = true;
    }

    pub fn move_mask_effect(&mut self, set: usize, eid: u64, dir: i32) {
        let s = &mut self.sets[set];
        let Some(l) = s.selected_layer_mut() else { return };
        if let Some(m) = &mut l.desc.mask {
            if let Some(i) = m.effects.iter().position(|e| e.id == eid) {
                let j = i as i32 + dir;
                if j >= 0 && (j as usize) < m.effects.len() {
                    m.effects.swap(i, j as usize);
                    s.dirty = true;
                    self.modified = true;
                }
            }
        }
    }

    pub fn import_resource(&mut self, engine: &Engine, path: &std::path::Path, kind: ResKind) -> anyhow::Result<u64> {
        let bytes = std::fs::read(path)?;
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        self.add_resource(engine, name, bytes, kind)
    }

    pub fn add_resource(&mut self, engine: &Engine, name: String, bytes: Vec<u8>, kind: ResKind) -> anyhow::Result<u64> {
        let img = decode_image(&bytes)?;
        let gpu = crate::engine::texture::rgba8_with_mips(&engine.device, &engine.queue, &name, &img);
        let id = self.alloc_id();
        self.resources.insert(
            id,
            Resource { id, name, kind, encoded: Arc::new(bytes), gpu: Arc::new(gpu), image: Arc::new(img) },
        );
        self.modified = true;
        Ok(id)
    }

    pub fn vram_bytes(&self) -> u64 {
        let sets: u64 = self
            .sets
            .iter()
            .map(|s| s.gpu.byte_size() + s.layers.iter().map(|l| l.byte_size()).sum::<u64>())
            .sum();
        sets + self.resources.values().map(|r| r.gpu.byte_size()).sum::<u64>()
    }
}

impl Document {
    /// Resamples every layer and mesh map of a texture set to `res`.
    pub fn change_resolution(&mut self, engine: &Engine, set: usize, res: u32) {
        let mesh = self.mesh.clone();
        let s = &mut self.sets[set];
        if s.resolution() == res {
            return;
        }
        let dev = &engine.device;
        let b = &engine.blitter;
        let mut enc = dev.create_command_encoder(&Default::default());
        visit_mut(&mut s.layers, &mut |l| {
            if let Some(p) = l.paint.as_mut().filter(|p| !p.is_empty()) {
                p.color = Arc::new(b.resample(dev, &mut enc, &p.color, res));
                p.vals = Arc::new(b.resample(dev, &mut enc, &p.vals, res));
                p.alph = Arc::new(b.resample(dev, &mut enc, &p.alph, res));
                for t in [&mut p.emis, &mut p.nrm, &mut p.opac].into_iter().flatten() {
                    *t = Arc::new(b.resample(dev, &mut enc, t, res));
                }
            }
            for t in l.mask_paint.values_mut().filter(|t| t.width > 1) {
                *t = Arc::new(b.resample(dev, &mut enc, t, res));
            }
        });
        let mut fresh = Self::make_set(engine, &mesh, s.submesh, res);
        if s.baked {
            let g = &s.gpu;
            let f = &mut fresh.gpu;
            f.baked_ao = b.resample(dev, &mut enc, &g.baked_ao, res);
            f.baked_curvature = b.resample(dev, &mut enc, &g.baked_curvature, res);
            f.baked_normal = b.resample(dev, &mut enc, &g.baked_normal, res);
            f.baked_wnormal = b.resample(dev, &mut enc, &g.baked_wnormal, res);
            f.baked_thickness = b.resample(dev, &mut enc, &g.baked_thickness, res);
            f.baked_id = b.resample(dev, &mut enc, &g.baked_id, res);
        }
        engine.queue.submit(Some(enc.finish()));
        s.gpu = fresh.gpu;
        s.raster = fresh.raster;
        s.dirty = true;
        self.modified = true;
    }

    /// Replaces the mesh, keeping the layers of texture sets whose material
    /// name still exists. Returns remarks for the user.
    pub fn reimport(&mut self, engine: &Engine, mesh: MeshData) -> Vec<String> {
        let mut notes = mesh.notes.clone();
        let gpu_mesh = GpuMesh::new(&engine.device, &mesh);
        let mesh = Arc::new(mesh);
        let mut old: Vec<TextureSet> = std::mem::take(&mut self.sets);
        let default_res = old.first().map(|s| s.resolution()).unwrap_or(2048);
        let mut sets = Vec::new();
        for (i, sub) in mesh.submeshes.iter().enumerate() {
            match old.iter().position(|s| s.name == sub.name) {
                Some(p) => {
                    let o = old.remove(p);
                    let mut fresh = Self::make_set(engine, &mesh, i, o.resolution());
                    fresh.layers = o.layers;
                    fresh.selected = o.selected;
                    fresh.settings = o.settings;
                    sets.push(fresh);
                }
                None => {
                    notes.push(format!("New texture set '{}'", sub.name));
                    sets.push(Self::make_set(engine, &mesh, i, default_res));
                }
            }
        }
        for o in old {
            notes.push(format!("Texture set '{}' is not in the new mesh and was removed", o.name));
        }
        let (bvh, pick_map) = Self::picking(&mesh);
        self.sets = sets;
        self.mesh = mesh;
        self.gpu_mesh = gpu_mesh;
        self.bvh = bvh;
        self.pick_map = pick_map;
        self.reset_hidden();
        self.active = 0;
        for s in 0..self.sets.len() {
            if self.sets[s].layers.is_empty() {
                self.add_layer(engine, s, LayerKind::Paint);
            }
        }
        self.modified = true;
        notes
    }
}

/// CPU side of a texture set's mesh data: UV raster, nearest covered texel
/// (gutter dilation), object position and world normal per texel.
pub struct SetMaps {
    raster: bake::UvRaster,
    nearest: Vec<[u32; 2]>,
    position: Vec<[f32; 4]>,
    wnormal: Vec<[f32; 4]>,
}

impl SetMaps {
    pub fn new(mesh: &MeshData, sub: usize, res: u32) -> Self {
        let raster = bake::rasterize_uv(mesh, sub, res);
        let (nearest, (position, wnormal)) = rayon::join(
            || bake::nearest_map(&raster),
            || (bake::position_map(mesh, sub, &raster), bake::world_normal_map(mesh, sub, &raster)),
        );
        SetMaps { raster, nearest, position, wnormal }
    }
}
