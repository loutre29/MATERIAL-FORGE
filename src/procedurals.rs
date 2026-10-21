//! Procedural textures as Painter's shelf lists them (Noises, Noises
//! Advanced, Grunges, Patterns, Geometric, Gradient). Each one is authored
//! here from public descriptions of the generators: a family of the
//! seamless generator shader (`shaders/patterns.wgsl`), its variant
//! constants and the parameters shown in Properties. The "3D" noises are
//! evaluated in object space instead (`shaders/procedural.wgsl`).

use crate::document::ProcKind;

/// A parameter of a generator, stored in `ProcParams::p[slot]`.
pub struct Param {
    pub label: &'static str,
    pub slot: usize,
    pub min: f32,
    pub max: f32,
    pub default: f32,
    pub int: bool,
}

#[allow(dead_code)]
pub struct ProcDef {
    pub kind: ProcKind,
    /// Painter's shelf folder.
    pub folder: &'static str,
    /// Generator family and variant (patterns.wgsl), hidden constants.
    pub family: u32,
    pub variant: u32,
    pub v: [f32; 4],
    pub params: &'static [Param],
}

const fn int(label: &'static str, slot: usize, min: f32, max: f32, default: f32) -> Param {
    Param { label, slot, min, max, default, int: true }
}

const fn float(label: &'static str, slot: usize, min: f32, max: f32, default: f32) -> Param {
    Param { label, slot, min, max, default, int: false }
}

const fn def(kind: ProcKind, folder: &'static str, family: u32, variant: u32, v: [f32; 4], params: &'static [Param]) -> ProcDef {
    ProcDef { kind, folder, family, variant, v, params }
}

macro_rules! scale_dis {
    ($s:expr) => {
        &[int("Scale", 0, 1.0, 64.0, $s), float("Disorder", 1, 0.0, 1.0, 0.0)]
    };
}

use ProcKind as K;

pub static DEFS: &[ProcDef] = &[
    // ------------------------------------------------------------ Noises
    def(K::PerlinNoise, "Noises", 0, 0, [0.0; 4], scale_dis!(24.0)),
    def(K::Clouds1, "Noises", 1, 0, [9.0, 0.72, 1.0, 1.7], scale_dis!(4.0)),
    def(K::Clouds2, "Noises", 1, 1, [5.0, 0.45, 1.0, 1.7], scale_dis!(2.0)),
    def(K::Clouds3, "Noises", 1, 2, [8.0, 0.66, 1.0, 1.6], scale_dis!(4.0)),
    def(K::BnwSpots1, "Noises", 4, 0, [0.5, 1.7, 8.0, 0.0], &[int("Scale", 0, 1.0, 64.0, 8.0), float("Roughness", 1, 0.0, 1.0, 0.78)]),
    def(K::BnwSpots2, "Noises", 4, 1, [0.5, 1.9, 7.0, 0.0], &[int("Scale", 0, 1.0, 64.0, 16.0), float("Roughness", 1, 0.0, 1.0, 0.82)]),
    def(K::BnwSpots3, "Noises", 4, 2, [0.5, 1.6, 6.0, 0.0], &[int("Scale", 0, 1.0, 64.0, 4.0), float("Roughness", 1, 0.0, 1.0, 0.55)]),
    def(K::Cells1, "Noises", 3, 0, [1.0, 0.0, 0.0, 0.0], scale_dis!(16.0)),
    def(K::Cells2, "Noises", 3, 1, [1.0, 0.0, 0.0, 0.0], scale_dis!(24.0)),
    def(K::Cells3, "Noises", 3, 2, [1.0, 0.0, 0.0, 0.0], scale_dis!(32.0)),
    def(K::Cells4, "Noises", 3, 3, [1.0, 0.0, 0.0, 0.0], scale_dis!(24.0)),
    def(K::Crystal1, "Noises", 5, 0, [0.0; 4], scale_dis!(8.0)),
    def(K::Crystal2, "Noises", 5, 1, [0.0; 4], scale_dis!(16.0)),
    def(K::Dirt1, "Noises", 6, 0, [1.0, 1.4, 3.0, 3.0], scale_dis!(4.0)),
    def(K::Dirt2, "Noises", 6, 0, [0.3, 1.6, 9.0, 3.0], scale_dis!(4.0)),
    def(K::Dirt3, "Noises", 6, 0, [0.1, 0.9, 2.0, 2.0], scale_dis!(8.0)),
    def(K::Dirt4, "Noises", 6, 0, [1.6, 1.4, 1.6, 2.0], scale_dis!(4.0)),
    def(K::Dirt5, "Noises", 6, 0, [0.2, 1.1, 4.0, 2.0], scale_dis!(8.0)),
    def(K::DirtGradient, "Noises", 6, 1, [1.0, 1.0, 3.0, 1.0], scale_dis!(4.0)),
    def(K::FractalSum1, "Noises", 2, 0, [1.0, 0.0, 0.0, 0.0], &FS),
    def(K::FractalSum2, "Noises", 2, 0, [1.2, 0.0, 0.0, 0.0], &FS2),
    def(K::FractalSum3, "Noises", 2, 0, [1.8, 0.0, 0.0, 0.0], &FS3),
    def(K::FractalSum4, "Noises", 2, 0, [1.9, 0.0, 0.0, 0.0], &FS4),
    def(K::FractalSumBase, "Noises", 2, 0, [0.6, 0.0, 0.0, 0.0], &FSB),
    def(K::GaussianNoise, "Noises", 7, 0, [0.0; 4], &[int("Scale", 0, 1.0, 64.0, 8.0)]),
    def(K::GaussianSpots1, "Noises", 8, 0, [0.25, 2.0, 1.0, 0.0], scale_dis!(16.0)),
    def(K::GaussianSpots2, "Noises", 8, 0, [0.12, 4.0, 1.3, 0.0], scale_dis!(48.0)),
    def(K::WhiteNoise, "Noises", 7, 1, [512.0, 0.0, 0.0, 0.0], &[]),
    def(K::WhiteNoiseFast, "Noises", 7, 1, [256.0, 0.0, 0.0, 0.0], &[]),
    def(K::BlueNoise, "Noises", 7, 2, [0.0; 4], &[]),
    def(K::DirectionalNoise1, "Noises", 9, 0, [9.0, 0.82, 0.0, 0.0], &DN1),
    def(K::DirectionalNoise2, "Noises", 9, 0, [9.0, 0.84, 0.0, 0.0], &DN2),
    def(K::DirectionalNoise3, "Noises", 9, 0, [9.0, 0.8, 0.0, 0.0], &DN3),
    def(K::DirectionalNoise4, "Noises", 9, 0, [8.0, 0.75, 0.0, 0.0], &DN4),
    def(K::DirectionalScratches, "Noises", 9, 0, [6.0, 0.75, 0.0, 0.0], &DSC),
    def(K::AnisotropicNoise, "Noises", 24, 0, [0.0; 4], scale_dis!(16.0)),
    def(K::Fur1, "Noises", 11, 0, [0.04, 6.0, 0.8, 0.0], &FUR1),
    def(K::Fur2, "Noises", 11, 0, [0.03, 6.0, 0.6, 0.0], &FUR2),
    def(K::Fur3, "Noises", 11, 1, [0.035, 7.0, 0.3, 0.0], &FUR3),
    def(K::MessyFibers1, "Noises", 11, 3, [0.3, 3.0, 0.4, 0.0], &MF1),
    def(K::MessyFibers2, "Noises", 11, 0, [0.02, 4.0, 0.1, 0.0], &MF2),
    def(K::MessyFibers3, "Noises", 11, 2, [0.02, 8.0, 0.1, 0.0], &MF3),
    def(K::MoistureNoise, "Noises", 12, 0, [1.6, 0.0, 0.0, 0.0], scale_dis!(4.0)),
    def(K::Plasma, "Noises", 13, 0, [0.0; 4], scale_dis!(3.0)),
    def(K::Waveform1, "Noises", 14, 0, [0.0; 4], &WAVE),
    def(K::Voronoi, "Noises", 15, 0, [0.0; 4], scale_dis!(8.0)),
    def(K::VoronoiFractal, "Noises", 15, 2, [0.0; 4], scale_dis!(4.0)),
    def(K::Creased, "Noises", 16, 0, [1.4, 0.5, 0.2, 2.2], scale_dis!(3.0)),
    def(K::Fluid, "Noises", 17, 1, [0.0; 4], scale_dis!(16.0)),
    def(K::Liquid, "Noises", 17, 0, [1.5, 1.0, 5.0, 0.0], scale_dis!(8.0)),
    def(K::MicroscopeView, "Noises", 18, 0, [0.0; 4], scale_dis!(12.0)),
    // ------------------------------------------------------------ Noises Advanced
    def(K::Perlin3D, "Noises Advanced", 0, 0, [0.0; 4], &[]),
    def(K::PerlinFractal3D, "Noises Advanced", 0, 0, [0.0; 4], &[]),
    def(K::RidgedFractal3D, "Noises Advanced", 0, 0, [0.0; 4], &[]),
    def(K::Simplex3D, "Noises Advanced", 0, 0, [0.0; 4], &[]),
    def(K::Voronoi3D, "Noises Advanced", 0, 0, [0.0; 4], &[]),
    def(K::VoronoiFractal3D, "Noises Advanced", 0, 0, [0.0; 4], &[]),
    def(K::Worley3D, "Noises Advanced", 0, 0, [0.0; 4], &[]),
    def(K::MarbleFine, "Noises Advanced", 23, 0, [4.0, 6.0, 1.0, 0.0], scale_dis!(4.0)),
    def(K::MarbleVeins, "Noises Advanced", 23, 0, [2.0, 18.0, 1.4, 0.0], scale_dis!(2.0)),
    def(K::MarbleWide, "Noises Advanced", 23, 0, [1.0, 2.0, 0.8, 0.0], scale_dis!(1.0)),
    def(K::CreasesSoft, "Noises Advanced", 16, 0, [1.0, 0.45, 0.1, 1.3], scale_dis!(3.0)),
    // ------------------------------------------------------------ Grunges
    def(K::GrungeMap001, "Grunges", 19, 1, [0.0; 4], scale_dis!(2.0)),
    def(K::GrungeMap002, "Grunges", 19, 2, [0.0; 4], scale_dis!(2.0)),
    def(K::GrungeMap003, "Grunges", 19, 3, [0.0; 4], scale_dis!(2.0)),
    def(K::GrungeMap004, "Grunges", 19, 4, [0.0; 4], scale_dis!(4.0)),
    def(K::GrungeMap005, "Grunges", 19, 5, [0.0; 4], scale_dis!(4.0)),
    def(K::GrungeMap006, "Grunges", 19, 6, [0.0; 4], scale_dis!(2.0)),
    def(K::GrungeMap007, "Grunges", 19, 7, [0.0; 4], scale_dis!(2.0)),
    def(K::GrungeMap008, "Grunges", 19, 8, [0.0; 4], scale_dis!(2.0)),
    def(K::GrungeMap009, "Grunges", 19, 9, [0.0; 4], scale_dis!(2.0)),
    def(K::GrungeMap010, "Grunges", 19, 10, [0.0; 4], scale_dis!(2.0)),
    def(K::GrungeMap011, "Grunges", 19, 11, [0.0; 4], scale_dis!(2.0)),
    def(K::GrungeMap012, "Grunges", 19, 12, [0.0; 4], scale_dis!(2.0)),
    def(K::GrungeMap013, "Grunges", 19, 13, [0.0; 4], scale_dis!(2.0)),
    def(K::GrungeMap014, "Grunges", 19, 14, [0.0; 4], scale_dis!(2.0)),
    def(K::GrungeMap015, "Grunges", 19, 15, [0.0; 4], scale_dis!(2.0)),
    def(K::GrungeConcrete, "Grunges", 19, 20, [0.0; 4], scale_dis!(2.0)),
    def(K::GrungeDamas, "Grunges", 19, 21, [0.0; 4], scale_dis!(2.0)),
    def(K::GrungeGalvanicLarge, "Grunges", 19, 22, [0.0; 4], scale_dis!(2.0)),
    def(K::GrungeGalvanicSmall, "Grunges", 19, 23, [0.0; 4], scale_dis!(4.0)),
    def(K::GrungeLeaks, "Grunges", 19, 24, [0.0; 4], scale_dis!(2.0)),
    def(K::GrungeLeakyPaint, "Grunges", 19, 25, [0.0; 4], scale_dis!(2.0)),
    def(K::GrungeRoughDirty, "Grunges", 19, 26, [0.0; 4], scale_dis!(2.0)),
    def(K::GrungeRustFine, "Grunges", 19, 27, [0.0; 4], scale_dis!(2.0)),
    def(K::GrungeScratchesDirty, "Grunges", 19, 28, [0.0; 4], scale_dis!(4.0)),
    def(K::GrungeScratchesFine, "Grunges", 19, 29, [0.0; 4], scale_dis!(4.0)),
    def(K::GrungeScratchesRough, "Grunges", 19, 30, [0.0; 4], scale_dis!(4.0)),
    def(K::GrungeShavings, "Grunges", 19, 31, [0.0; 4], scale_dis!(4.0)),
    def(K::GrungeSplashesDusty, "Grunges", 19, 32, [0.0; 4], scale_dis!(2.0)),
    def(K::GrungeSpots, "Grunges", 19, 33, [0.0; 4], scale_dis!(4.0)),
    def(K::GrungeSpotsDirty, "Grunges", 19, 34, [0.0; 4], scale_dis!(4.0)),
    // ------------------------------------------------------------ Patterns
    def(K::BrickGenerator, "Patterns", 22, 0, [0.3, 0.0, 0.0, 0.0], &BRICKG),
    def(K::Bricks1, "Patterns", 22, 1, [0.0; 4], &BRICK1),
    def(K::TileGenerator, "Patterns", 22, 2, [0.0; 4], &TILE),
    def(K::TileRandom2, "Patterns", 22, 15, [0.0; 4], &TILE),
    def(K::TriangleGrid, "Patterns", 22, 4, [0.0; 4], &TILE),
    def(K::Wood1, "Patterns", 22, 5, [0.0; 4], &[int("Rings", 0, 1.0, 32.0, 6.0)]),
    def(K::Fence1, "Patterns", 22, 6, [0.0; 4], &GRATE),
    def(K::MetallicGrateLines, "Patterns", 22, 7, [0.0; 4], &GRATE),
    def(K::MetallicGrateRound, "Patterns", 22, 8, [0.0; 4], &GRATE),
    def(K::MetallicGrateSimple, "Patterns", 22, 9, [0.0; 4], &GRATE),
    def(K::MetallicGrateWide, "Patterns", 22, 10, [0.0; 4], &GRATE_WIDE),
    def(K::Pavement, "Patterns", 22, 16, [0.0; 4], &[int("Scale", 0, 1.0, 32.0, 6.0)]),
    def(K::CamoUcp, "Patterns", 22, 12, [0.0; 4], &[int("Scale", 0, 1.0, 32.0, 6.0)]),
    def(K::CamoWoodland, "Patterns", 22, 13, [0.0; 4], &[int("Scale", 0, 1.0, 32.0, 3.0)]),
    def(K::ScratchesGenerator, "Patterns", 22, 14, [0.0; 4], &SCRG),
    def(K::PaperPath, "Patterns", 22, 99, [0.0; 4], &[int("Scale", 0, 1.0, 32.0, 2.0)]),
    // ------------------------------------------------------------ Geometric
    def(K::Checker1, "Geometric", 20, 0, [0.0; 4], &GEO_CHECK),
    def(K::Checker2, "Geometric", 20, 16, [0.0; 4], &GEO_CHECK),
    def(K::Circles, "Geometric", 20, 1, [0.0; 4], &GEO),
    def(K::Hexagon, "Geometric", 20, 2, [0.0; 4], &GEO),
    def(K::HexagonBorder, "Geometric", 20, 3, [0.0; 4], &GEO),
    def(K::HexagonBranch, "Geometric", 20, 9, [0.0; 4], &GEO),
    def(K::HexagonDots, "Geometric", 20, 6, [0.0; 4], &GEO),
    def(K::HexagonDouble, "Geometric", 20, 10, [0.0; 4], &GEO),
    def(K::HexagonTriangle, "Geometric", 20, 12, [0.0; 4], &GEO),
    def(K::HexagonTriangleBorder, "Geometric", 20, 13, [0.0; 4], &GEO),
    def(K::HexagonWave, "Geometric", 20, 11, [0.0; 4], &GEO),
    def(K::Polygon1, "Geometric", 20, 17, [0.0; 4], &POLY1),
    def(K::Polygon2, "Geometric", 20, 14, [0.0; 4], &POLY1),
    def(K::Stripes, "Geometric", 20, 15, [0.0; 4], &GEO_STRIPES),
    // ------------------------------------------------------------ Gradient
    def(K::GradientLinear1, "Gradient", 21, 0, [0.0; 4], &GRAD),
    def(K::GradientLinear2, "Gradient", 21, 1, [0.0; 4], &GRAD),
    def(K::GradientLinear3, "Gradient", 21, 2, [0.0; 4], &GRAD),
    def(K::GradientCircular, "Gradient", 21, 3, [0.0; 4], &GRAD),
    def(K::GradientDot, "Gradient", 21, 4, [0.0; 4], &GRAD),
    def(K::GradientChecker, "Gradient", 21, 5, [0.0; 4], &GRAD4),
    def(K::GradientCheckerDouble, "Gradient", 21, 5, [0.0; 4], &GRAD8),
    def(K::GradientAlternate, "Gradient", 21, 6, [0.0; 4], &GRAD4),
    def(K::GradientFlakes, "Gradient", 21, 7, [0.0; 4], &GRAD8),
    def(K::GradientDiscOverlap, "Gradient", 21, 8, [0.0; 4], &GRAD4),
    def(K::GradientHue, "Gradient", 21, 12, [0.0; 4], &GRAD),
    def(K::GradientWeave, "Gradient", 21, 13, [0.0; 4], &GRAD4),
    def(K::GradientAxial, "Gradient", 21, 9, [0.0; 4], &GRAD),
    def(K::GradientAxialReflected, "Gradient", 21, 10, [0.0; 4], &GRAD),
    def(K::GradientRadial, "Gradient", 21, 11, [0.0; 4], &GRAD),
];

static FS: [Param; 4] = [int("Scale", 0, 1.0, 64.0, 2.0), float("Roughness", 1, 0.0, 1.0, 0.82), int("Min level", 2, 0.0, 6.0, 0.0), int("Max level", 3, 1.0, 10.0, 8.0)];
static FS2: [Param; 4] = [int("Scale", 0, 1.0, 64.0, 4.0), float("Roughness", 1, 0.0, 1.0, 0.85), int("Min level", 2, 0.0, 6.0, 0.0), int("Max level", 3, 1.0, 10.0, 8.0)];
static FS3: [Param; 4] = [int("Scale", 0, 1.0, 64.0, 32.0), float("Roughness", 1, 0.0, 1.0, 0.55), int("Min level", 2, 0.0, 6.0, 0.0), int("Max level", 3, 1.0, 10.0, 5.0)];
static FS4: [Param; 4] = [int("Scale", 0, 1.0, 64.0, 16.0), float("Roughness", 1, 0.0, 1.0, 0.5), int("Min level", 2, 0.0, 6.0, 0.0), int("Max level", 3, 1.0, 10.0, 5.0)];
static FSB: [Param; 4] = [int("Scale", 0, 1.0, 64.0, 1.0), float("Roughness", 1, 0.0, 1.0, 0.5), int("Min level", 2, 0.0, 6.0, 0.0), int("Max level", 3, 1.0, 10.0, 8.0)];
static DN1: [Param; 4] = [int("Scale", 0, 1.0, 64.0, 8.0), float("Disorder", 1, 0.0, 1.0, 0.1), float("Angle", 2, 0.0, 90.0, 0.0), int("Stretch", 3, 1.0, 32.0, 4.0)];
static DN2: [Param; 4] = [int("Scale", 0, 1.0, 64.0, 8.0), float("Disorder", 1, 0.0, 1.0, 0.2), float("Angle", 2, 0.0, 90.0, 0.0), int("Stretch", 3, 1.0, 32.0, 6.0)];
static DN3: [Param; 4] = [int("Scale", 0, 1.0, 64.0, 4.0), float("Disorder", 1, 0.0, 1.0, 0.3), float("Angle", 2, 0.0, 90.0, 0.0), int("Stretch", 3, 1.0, 32.0, 4.0)];
static DN4: [Param; 4] = [int("Scale", 0, 1.0, 64.0, 4.0), float("Disorder", 1, 0.0, 1.0, 0.1), float("Angle", 2, 0.0, 90.0, 0.0), int("Stretch", 3, 1.0, 32.0, 12.0)];
static DSC: [Param; 4] = [int("Scale", 0, 1.0, 64.0, 32.0), float("Disorder", 1, 0.0, 1.0, 0.0), float("Angle", 2, 0.0, 90.0, 0.0), int("Stretch", 3, 1.0, 32.0, 16.0)];
static FUR1: [Param; 4] = [int("Scale", 0, 1.0, 64.0, 8.0), float("Length", 1, 0.2, 4.0, 2.4), float("Angle", 2, 0.0, 360.0, 90.0), float("Spread", 3, 0.0, 360.0, 25.0)];
static FUR2: [Param; 4] = [int("Scale", 0, 1.0, 64.0, 12.0), float("Length", 1, 0.2, 4.0, 2.0), float("Angle", 2, 0.0, 360.0, 20.0), float("Spread", 3, 0.0, 360.0, 20.0)];
static FUR3: [Param; 4] = [int("Scale", 0, 1.0, 64.0, 10.0), float("Length", 1, 0.2, 4.0, 1.6), float("Angle", 2, 0.0, 360.0, 90.0), float("Spread", 3, 0.0, 360.0, 10.0)];
static MF1: [Param; 4] = [int("Scale", 0, 1.0, 64.0, 4.0), float("Length", 1, 0.2, 4.0, 2.0), float("Angle", 2, 0.0, 360.0, 0.0), float("Spread", 3, 0.0, 360.0, 360.0)];
static MF2: [Param; 4] = [int("Scale", 0, 1.0, 64.0, 4.0), float("Length", 1, 0.2, 4.0, 3.0), float("Angle", 2, 0.0, 360.0, 0.0), float("Spread", 3, 0.0, 360.0, 360.0)];
static MF3: [Param; 4] = [int("Scale", 0, 1.0, 64.0, 6.0), float("Length", 1, 0.2, 4.0, 2.0), float("Angle", 2, 0.0, 360.0, 0.0), float("Spread", 3, 0.0, 360.0, 360.0)];
static WAVE: [Param; 4] = [int("Scale", 0, 1.0, 64.0, 4.0), float("Disorder", 1, 0.0, 1.0, 0.0), int("Count", 2, 1.0, 8.0, 2.0), float("Amplitude", 3, 0.0, 0.5, 0.45)];
static BRICKG: [Param; 4] = [int("Rows", 0, 1.0, 64.0, 4.0), float("Mortar", 1, 0.0, 0.4, 0.06), int("Bricks per row", 2, 1.0, 64.0, 4.0), float("Bevel", 3, 0.0, 0.5, 0.18)];
static BRICK1: [Param; 4] = [int("Rows", 0, 1.0, 64.0, 4.0), float("Mortar", 1, 0.0, 0.4, 0.06), int("Bricks per row", 2, 1.0, 64.0, 2.0), float("Bevel", 3, 0.0, 0.5, 0.004)];
static TILE: [Param; 2] = [int("Scale", 0, 1.0, 64.0, 10.0), float("Gap", 1, 0.0, 0.5, 0.06)];
static GRATE: [Param; 2] = [int("Scale", 0, 1.0, 64.0, 8.0), float("Bar width", 1, 0.02, 0.9, 0.2)];
static GRATE_WIDE: [Param; 2] = [int("Scale", 0, 1.0, 64.0, 4.0), float("Bar width", 1, 0.02, 0.9, 0.35)];
static SCRG: [Param; 4] = [int("Scale", 0, 1.0, 64.0, 8.0), float("Disorder", 1, 0.0, 1.0, 0.0), float("Angle", 2, 0.0, 360.0, 0.0), float("Spread", 3, 0.0, 360.0, 360.0)];
static GEO_CHECK: [Param; 1] = [int("Tiling", 0, 1.0, 64.0, 4.0)];
static GEO: [Param; 2] = [int("Tiling", 0, 1.0, 64.0, 4.0), float("Size", 1, 0.0, 1.0, 0.8)];
static GEO_STRIPES: [Param; 2] = [int("Tiling", 0, 1.0, 64.0, 8.0), float("Width", 1, 0.0, 2.0, 1.0)];
static POLY1: [Param; 3] = [int("Tiling", 0, 1.0, 64.0, 1.0), float("Size", 1, 0.0, 1.0, 0.9), float("Angle", 2, 0.0, 360.0, 0.0)];
static GRAD: [Param; 1] = [int("Tiling", 0, 1.0, 64.0, 1.0)];
static GRAD4: [Param; 1] = [int("Tiling", 0, 1.0, 64.0, 4.0)];
static GRAD8: [Param; 1] = [int("Tiling", 0, 1.0, 64.0, 8.0)];

pub fn def_of(kind: ProcKind) -> Option<&'static ProcDef> {
    DEFS.iter().find(|d| d.kind == kind)
}

/// Shelf folders in Painter's order (the legacy generators last).
pub const FOLDERS: [&str; 7] = ["Noises", "Noises Advanced", "Grunges", "Patterns", "Geometric", "Gradient", "Classic"];

pub fn folder_of(kind: ProcKind) -> &'static str {
    def_of(kind).map(|d| d.folder).unwrap_or("Classic")
}
