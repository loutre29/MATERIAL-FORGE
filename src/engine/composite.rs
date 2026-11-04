//! Executes a layer-stack plan on the GPU: base clear, per-layer content
//! (paint textures or fill evaluation), mask effect stacks, filters, folders,
//! then the finalize pass and mip generation of the set's material maps.
//!
//! Scratch textures (accumulators, fill content, masks) live in per-resolution
//! arenas shared by every texture set.

use super::texture::Tex;
use super::TextureSetGpu;
use bytemuck::{Pod, Zeroable};
use std::cell::RefCell;
use std::collections::HashMap;
use wgpu::util::DeviceExt;

pub const COMMON: &str = include_str!("../shaders/common.wgsl");
pub const PROCEDURAL: &str = include_str!("../shaders/procedural.wgsl");

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct ClearU {
    pub col: [f32; 4],
    pub mrh: [f32; 4],
    pub emi: [f32; 4],
    pub nrm: [f32; 4],
    /// The optional channel groups (`document::XG`).
    pub x: [[f32; 4]; 3],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct BlendU {
    pub mode_a: [u32; 4],
    pub mode_b: [u32; 4],
    pub op_a: [f32; 4],
    pub op_b: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct FolderU {
    pub op_a: [f32; 4],
    pub op_b: [f32; 4],
    pub flags: [u32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct FilterU {
    pub k: [u32; 4],
    pub op_a: [f32; 4],
    pub op_b: [f32; 4],
    pub p0: [f32; 4],
    pub p1: [f32; 4],
    pub color: [f32; 4],
    pub color2: [f32; 4],
    /// Curves: 4 curves of 8 (x, y) points (`document::curve_uniform`).
    pub curve: [[f32; 4]; 16],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct BlurU {
    pub dir: [i32; 4],
    pub k: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct FinalU {
    pub k: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct ChanSrcU {
    pub k: [u32; 4],
    pub a: [f32; 4],
    pub b: [f32; 4],
    pub pa: [f32; 4],
    pub pb: [f32; 4],
    pub t: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct FillU {
    pub mode_a: [u32; 4],
    pub mode_b: [u32; 4],
    pub op_a: [f32; 4],
    pub op_b: [f32; 4],
    pub proj: [f32; 4],
    pub offs: [f32; 4],
    /// Position map space to projection space, normal rotation (rows).
    pub xf: [[f32; 4]; 3],
    pub nr: [[f32; 4]; 3],
    /// UV projection: stretch u, v, wrap, filter; pivot u, v.
    pub uvx: [f32; 4],
    pub uvp: [f32; 4],
    /// The seven core channels, then the optional ones.
    pub ch: [ChanSrcU; 21],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct MaskU {
    pub k: [u32; 4],
    pub o: [f32; 4],
    pub g0: [f32; 4],
    pub g1: [f32; 4],
    pub dir: [f32; 4],
    pub lv: [f32; 4],
    pub lv2: [f32; 4],
    pub sv: [f32; 4],
    pub pa: [f32; 4],
    pub pb: [f32; 4],
    pub proj: [f32; 4],
    pub offs: [f32; 4],
    pub xf: [[f32; 4]; 3],
    pub nr: [[f32; 4]; 3],
    pub colors: [[f32; 4]; 8],
    /// UV projection: per-axis scale (the 2D manipulator), pivot.
    pub uvx: [f32; 4],
    pub uvp: [f32; 4],
}

/// Paint layer textures (see `document::PaintTex`).
pub struct PaintRefs<'a> {
    pub color: &'a Tex,
    pub vals: &'a Tex,
    pub alph: &'a Tex,
    pub emis: Option<&'a Tex>,
    pub nrm: Option<&'a Tex>,
    pub opac: Option<&'a Tex>,
}

pub enum MaskStep<'a> {
    Effect { params: MaskU, paint: Option<&'a Tex>, texture: Option<&'a Tex> },
    Blur { radius: f32, opacity: f32 },
}

pub struct MaskPlan<'a> {
    pub base: f32,
    pub steps: Vec<MaskStep<'a>>,
    /// (layer id, content hash): reuse the cached evaluation when unchanged.
    pub cache: Option<(u64, u64)>,
    /// Copy the evaluated mask into the set's mask view (mask display).
    pub capture: bool,
}

pub enum Plan<'a> {
    Paint { blend: BlendU, tex: PaintRefs<'a>, mask: Option<MaskPlan<'a>> },
    Fill { fill: Box<FillU>, textures: [Option<&'a Tex>; 7], mask: Option<MaskPlan<'a>> },
    Filter { params: FilterU, blur_radius: f32, mask: Option<MaskPlan<'a>> },
    Folder { params: FolderU, children: Vec<Plan<'a>>, mask: Option<MaskPlan<'a>> },
}

impl<'a> Plan<'a> {
    fn mask(&self) -> Option<&MaskPlan<'a>> {
        match self {
            Plan::Paint { mask, .. } | Plan::Fill { mask, .. } | Plan::Filter { mask, .. } | Plan::Folder { mask, .. } => {
                mask.as_ref()
            }
        }
    }
}

/// Texel rect (x0, y0, x1, y1), max exclusive, in texture set space.
pub type Region = [u32; 4];

/// Side of the tiles a stack is evaluated in. Scratch textures are tile
/// sized (plus the halo blur-like passes need), so a 4K set needs a few tens
/// of MB of scratch instead of gigabytes.
const TILE: u32 = 1024;
/// Halo beyond which a stack runs untiled.
const MAX_HALO: u32 = 256;
/// Prefix caches are full size: only kept at moderate resolutions.
const PREFIX_MAX_RES: u32 = 2048;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
struct TileU {
    origin: [i32; 4],
    size: [i32; 4],
    scratch: [i32; 4],
    inner: [i32; 4],
}

/// Textures of one accumulator: col, mrh, emi, nrm, then the optional
/// channel groups (1x1 unless the run needs them, see `Arena::acc`).
const ACC: usize = 7;

#[derive(Default)]
struct Arena {
    acc: Vec<[Tex; ACC]>,
    acc_free: Vec<usize>,
    mask: Vec<Tex>,
    mask_free: Vec<usize>,
}

const RGBA: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
const R32: wgpu::TextureFormat = wgpu::TextureFormat::R32Float;

fn scratch_usage() -> wgpu::TextureUsages {
    wgpu::TextureUsages::STORAGE_BINDING
        | wgpu::TextureUsages::TEXTURE_BINDING
        | wgpu::TextureUsages::COPY_SRC
        | wgpu::TextureUsages::COPY_DST
}

fn copy_usage() -> wgpu::TextureUsages {
    wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::COPY_DST
}

/// Accumulated stack below the edited layer of the active set.
struct PrefixCache {
    set: u64,
    key: u64,
    res: u32,
    /// Holds the optional groups too.
    ext: bool,
    tex: [Tex; ACC],
}

impl Arena {
    /// A free accumulator; `ext`: with full size optional groups.
    fn acc(&mut self, d: &wgpu::Device, size: u32, ext: bool) -> usize {
        let x = |l, ext: bool| if ext { Tex::new(d, l, size, size, RGBA, scratch_usage()) } else { Tex::new(d, l, 1, 1, RGBA, scratch_usage()) };
        if let Some(i) = self.acc_free.pop() {
            if ext && self.acc[i][4].width == 1 {
                for k in 4..ACC {
                    self.acc[i][k] = x("acc x", true);
                }
            }
            return i;
        }
        let t = |l| Tex::new(d, l, size, size, RGBA, scratch_usage());
        self.acc.push([t("acc col"), t("acc mrh"), t("acc emi"), t("acc nrm"), x("acc x0", ext), x("acc x1", ext), x("acc x2", ext)]);
        self.acc.len() - 1
    }
    fn mask(&mut self, d: &wgpu::Device, size: u32) -> usize {
        if let Some(i) = self.mask_free.pop() {
            return i;
        }
        self.mask.push(Tex::new(d, "mask scratch", size, size, R32, scratch_usage()));
        self.mask.len() - 1
    }
}

pub struct Pipelines {
    clear: wgpu::ComputePipeline,
    blend: wgpu::ComputePipeline,
    folder: wgpu::ComputePipeline,
    filter: wgpu::ComputePipeline,
    accblur: wgpu::ComputePipeline,
    finalize: wgpu::ComputePipeline,
    mip: wgpu::ComputePipeline,
    fill: wgpu::ComputePipeline,
    mask: wgpu::ComputePipeline,
    maskblur: wgpu::ComputePipeline,
}

pub struct Compositor {
    p: Pipelines,
    d_rgba: Tex,
    d_mask: Tex,
    d_rg: Tex,
    d_img: Tex,
    sampler: wgpu::Sampler,
    /// Scratch textures by size.
    arenas: RefCell<HashMap<u32, Arena>>,
    mask_cache: RefCell<HashMap<(u64, u64), (u64, Tex)>>,
    prefix: RefCell<Option<PrefixCache>>,
    /// Tests: evaluate every stack in one tile (the reference for tiling).
    pub force_untiled: std::cell::Cell<bool>,
}

/// Splits a shader file at "//// ENTRY <name>" markers.
pub fn sections(src: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let mut name: Option<String> = None;
    let mut body = String::new();
    for line in src.lines() {
        if let Some(n) = line.strip_prefix("//// ENTRY ") {
            if let Some(prev) = name.take() {
                out.insert(prev, std::mem::take(&mut body));
            }
            name = Some(n.trim().to_string());
            continue;
        }
        if name.is_some() {
            body.push_str(line);
            body.push('\n');
        }
    }
    if let Some(prev) = name {
        out.insert(prev, body);
    }
    out
}

fn compute(device: &wgpu::Device, label: &str, code: String) -> wgpu::ComputePipeline {
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(label),
        source: wgpu::ShaderSource::Wgsl(code.into()),
    });
    device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some(label),
        layout: None,
        module: &module,
        entry_point: "main",
        compilation_options: Default::default(),
        cache: None,
    })
}

fn ubuf(device: &wgpu::Device, data: &[u8]) -> wgpu::Buffer {
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("pass params"),
        contents: data,
        usage: wgpu::BufferUsages::UNIFORM,
    })
}

fn tv(binding: u32, view: &wgpu::TextureView) -> wgpu::BindGroupEntry<'_> {
    wgpu::BindGroupEntry { binding, resource: wgpu::BindingResource::TextureView(view) }
}

fn dispatch(
    device: &wgpu::Device,
    enc: &mut wgpu::CommandEncoder,
    pipeline: &wgpu::ComputePipeline,
    entries: &[wgpu::BindGroupEntry],
    tile: &wgpu::Buffer,
    extent: (u32, u32),
) {
    if extent.0 == 0 || extent.1 == 0 {
        return;
    }
    let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &pipeline.get_bind_group_layout(0),
        entries,
    });
    let tg = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &pipeline.get_bind_group_layout(1),
        entries: &[wgpu::BindGroupEntry { binding: 0, resource: tile.as_entire_binding() }],
    });
    let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: None, timestamp_writes: None });
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, &bg, &[]);
    pass.set_bind_group(1, &tg, &[]);
    pass.dispatch_workgroups(extent.0.div_ceil(8), extent.1.div_ceil(8), 1);
}

/// Copies a rect between two textures (origins in each texture's texels).
fn copy_rect(enc: &mut wgpu::CommandEncoder, src: &Tex, so: (u32, u32), dst: &Tex, d_o: (u32, u32), w: u32, h: u32) {
    if w == 0 || h == 0 {
        return;
    }
    enc.copy_texture_to_texture(
        wgpu::ImageCopyTexture { texture: &src.texture, mip_level: 0, origin: wgpu::Origin3d { x: so.0, y: so.1, z: 0 }, aspect: wgpu::TextureAspect::All },
        wgpu::ImageCopyTexture { texture: &dst.texture, mip_level: 0, origin: wgpu::Origin3d { x: d_o.0, y: d_o.1, z: 0 }, aspect: wgpu::TextureAspect::All },
        wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
    );
}

fn tile_buf(device: &wgpu::Device, origin: (u32, u32), size: (u32, u32), res: u32, scratch: (u32, u32), inner: Region) -> wgpu::Buffer {
    let u = TileU {
        origin: [origin.0 as i32, origin.1 as i32, 0, 0],
        size: [size.0 as i32, size.1 as i32, res as i32, res as i32],
        scratch: [scratch.0 as i32, scratch.1 as i32, 0, 0],
        inner: [inner[0] as i32, inner[1] as i32, inner[2] as i32, inner[3] as i32],
    };
    ubuf(device, bytemuck::bytes_of(&u))
}

enum Content<'v> {
    Views([&'v wgpu::TextureView; 6]),
}

/// Per run: what happens to each cached mask (layer id -> (hit, store)).
type MaskPolicy = HashMap<u64, (bool, bool)>;

struct Ctx<'a> {
    device: &'a wgpu::Device,
    enc: &'a mut wgpu::CommandEncoder,
    set: &'a TextureSetGpu,
    arena: &'a mut Arena,
    /// Allocated scratch size (square).
    size: u32,
    capture: Option<&'a Tex>,
    /// Scratch rect of the current tile (global), and the rect it owns.
    scratch: Region,
    inner: Region,
    tile: wgpu::Buffer,
    masks: &'a MaskPolicy,
    /// The set uses optional channel groups: accumulators carry them.
    ext: bool,
}

impl Ctx<'_> {
    fn acc(&mut self) -> usize {
        self.arena.acc(self.device, self.size, self.ext)
    }
    /// Accumulator textures copied between tiles and caches.
    fn acc_count(&self) -> usize {
        if self.ext {
            ACC
        } else {
            4
        }
    }
    fn extent(&self) -> (u32, u32) {
        (self.scratch[2] - self.scratch[0], self.scratch[3] - self.scratch[1])
    }
    fn origin(&self) -> (u32, u32) {
        (self.scratch[0], self.scratch[1])
    }
    /// Owned rect, in scratch texels.
    fn inner_local(&self) -> ((u32, u32), u32, u32) {
        ((self.inner[0] - self.scratch[0], self.inner[1] - self.scratch[1]), self.inner[2] - self.inner[0], self.inner[3] - self.inner[1])
    }
}

/// Extra texels a tile needs around the rect it owns, for passes that read
/// neighbors (blurs, the painted-height generator ring, height to normal).
/// None: some pass reads arbitrarily far (mirror / transform), run untiled.
fn plan_halo(plan: &[Plan], res: u32) -> Option<u32> {
    fn walk(plan: &[Plan], res: u32, h: &mut u32) -> bool {
        for node in plan {
            if let Some(m) = node.mask() {
                for step in &m.steps {
                    match step {
                        MaskStep::Blur { radius, .. } => *h += radius.clamp(1.0, 64.0).ceil() as u32 + 1,
                        MaskStep::Effect { params, .. } if params.k[0] == 3 && params.g1[3] > 0.0 => *h += (res / 170).max(2) + 2,
                        _ => {}
                    }
                }
            }
            match node {
                Plan::Filter { params, blur_radius, .. } => {
                    if params.k[0] == 15 || params.k[0] == 16 {
                        return false;
                    }
                    if *blur_radius > 0.0 {
                        *h += blur_radius.clamp(1.0, 64.0).ceil() as u32 + 1;
                    }
                }
                Plan::Folder { children, .. } => {
                    if !walk(children, res, h) {
                        return false;
                    }
                }
                _ => {}
            }
        }
        true
    }
    let mut h = 2;
    (walk(plan, res, &mut h) && h <= MAX_HALO).then_some(h)
}

/// Owned rects covering `area` (aligned to the tile grid).
fn chunks(area: Region, tile: u32) -> Vec<Region> {
    let mut out = Vec::new();
    let mut y = area[1];
    while y < area[3] {
        let y1 = ((y / tile + 1) * tile).min(area[3]);
        let mut x = area[0];
        while x < area[2] {
            let x1 = ((x / tile + 1) * tile).min(area[2]);
            out.push([x, y, x1, y1]);
            x = x1;
        }
        y = y1;
    }
    out
}

fn union(a: Option<Region>, b: Region) -> Option<Region> {
    if b[2] <= b[0] || b[3] <= b[1] {
        return a;
    }
    Some(match a {
        Some(a) => [a[0].min(b[0]), a[1].min(b[1]), a[2].max(b[2]), a[3].max(b[3])],
        None => b,
    })
}

impl Compositor {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let stack = sections(include_str!("../shaders/stack.wgsl"));
        let sources = sections(include_str!("../shaders/sources.wgsl"));
        let tile = include_str!("../shaders/tile.wgsl");
        let with_common = |s: &str| format!("{COMMON}\n{tile}\n{s}");
        let with_proc = |s: &str| format!("{COMMON}\n{PROCEDURAL}\n{tile}\n{s}");
        let p = Pipelines {
            clear: compute(device, "clear", with_common(&stack["clear"])),
            blend: compute(device, "blend", with_common(&stack["blend"])),
            folder: compute(device, "folder", with_common(&stack["folder"])),
            filter: compute(device, "filter", with_common(&stack["filter"])),
            accblur: compute(device, "accblur", with_common(&stack["accblur"])),
            finalize: compute(device, "finalize", with_common(&stack["finalize"])),
            mip: compute(device, "mip", with_common(&stack["mip"])),
            fill: compute(device, "fill", with_proc(&sources["fill"])),
            mask: compute(device, "mask", with_proc(&sources["mask"])),
            maskblur: compute(device, "maskblur", with_common(&sources["maskblur"])),
        };
        let usage = wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST;
        let d_rgba = Tex::new(device, "dummy rgba", 1, 1, RGBA, usage);
        d_rgba.write_bytes(queue, &super::texture::f32_to_f16_bytes(&[0.0; 4]));
        let d_mask = Tex::new(device, "dummy mask", 1, 1, R32, usage);
        d_mask.write_bytes(queue, bytemuck::bytes_of(&1.0f32));
        let d_rg = Tex::new(device, "dummy rg", 1, 1, wgpu::TextureFormat::Rg16Float, usage);
        d_rg.write_bytes(queue, &super::texture::f32_to_f16_bytes(&[0.0; 2]));
        let d_img = Tex::new(device, "dummy image", 1, 1, wgpu::TextureFormat::Rgba8Unorm, usage);
        d_img.write_bytes(queue, &[255, 255, 255, 255]);
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("fill sampler"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Self {
            p,
            d_rgba,
            d_mask,
            d_rg,
            d_img,
            sampler,
            arenas: RefCell::new(HashMap::new()),
            mask_cache: RefCell::new(HashMap::new()),
            prefix: RefCell::new(None),
            force_untiled: std::cell::Cell::new(false),
        }
    }

    /// Runs `f` on a layer's cached mask evaluation (texture, content hash).
    pub fn with_cached_mask<R>(&self, set: u64, layer: u64, f: impl FnOnce(&Tex, u64) -> R) -> Option<R> {
        let c = self.mask_cache.borrow();
        c.get(&(set, layer)).map(|(h, t)| f(t, *h))
    }

    /// Scratch memory held: (accumulator sets, mask scratch, cached masks, MB).
    pub fn stats(&self) -> (usize, usize, usize, u64) {
        let ar = self.arenas.borrow();
        let (mut acc, mut mask, mut bytes) = (0, 0, 0u64);
        for (size, a) in ar.iter() {
            let px = (*size as u64) * (*size as u64);
            acc += a.acc.len();
            mask += a.mask.len();
            bytes += a.acc.len() as u64 * px * 8 * 4 + a.mask.len() as u64 * px * 4;
        }
        let mc = self.mask_cache.borrow();
        for (_, t) in mc.values() {
            bytes += (t.width as u64) * (t.height as u64) * 4;
        }
        if let Some(p) = self.prefix.borrow().as_ref() {
            bytes += (p.res as u64) * (p.res as u64) * 8 * 4;
        }
        (acc, mask, mc.len(), bytes / (1024 * 1024))
    }

    /// Drops every scratch texture (after resolution changes, to free VRAM).
    pub fn trim(&self) {
        self.arenas.borrow_mut().clear();
        self.mask_cache.borrow_mut().clear();
        *self.prefix.borrow_mut() = None;
    }

    /// Tile layout of a run: (tile side, scratch size, halo).
    fn layout(&self, plan: &[Plan], res: u32) -> (u32, u32, u32) {
        match plan_halo(plan, res) {
            Some(halo) if res > TILE && !self.force_untiled.get() => {
                // Scratch sizes rounded so a few halo values share an arena.
                let size = (TILE + 2 * halo).div_ceil(64) * 64;
                (TILE, size.min(res), halo)
            }
            _ => (res, res, 0),
        }
    }

    /// Composites `plan` into the set's final maps. `region`: only that rect
    /// changed since the last run (a brush stroke); everything else of the
    /// finals is kept.
    #[allow(clippy::too_many_arguments)]
    pub fn run(
        &self,
        device: &wgpu::Device,
        enc: &mut wgpu::CommandEncoder,
        set: &TextureSetGpu,
        clear: ClearU,
        plan: &[Plan],
        fin: FinalU,
        capture: Option<&Tex>,
        prefix: Option<(usize, u64)>,
        region: Option<Region>,
    ) {
        let res = set.resolution;
        // The mask display texture exists only while it is shown.
        let capture = capture.filter(|c| c.width == res);
        // Drop cached masks of layers that are gone.
        let mut live = Vec::new();
        collect_mask_layers(plan, &mut live);
        self.mask_cache.borrow_mut().retain(|(s, l), _| *s != set.uid || live.contains(l));

        let (tile, size, halo) = self.layout(plan, res);
        let cell = set.cell;
        let area = match region {
            Some(r) => {
                let a = [r[0] / cell * cell, r[1] / cell * cell, r[2].div_ceil(cell) * cell, r[3].div_ceil(cell) * cell];
                [a[0], a[1], a[2].min(res), a[3].min(res)]
            }
            None => [0, 0, res, res],
        };
        if area[2] <= area[0] || area[3] <= area[1] {
            return;
        }
        let full = area == [0, 0, res, res];
        let masks = self.mask_policy(device, set, plan, full);

        // Prefix: the stack below the edited layer, cached full size.
        let prefix = prefix.filter(|_| res <= PREFIX_MAX_RES);
        let (split, key) = prefix.unwrap_or((0, 0));
        let split = split.min(plan.len());
        let hit = prefix.is_some()
            && self.prefix.borrow().as_ref().is_some_and(|p| p.set == set.uid && p.key == key && p.res == res);
        let ext = set.ext_on();
        let hit = hit && self.prefix.borrow().as_ref().is_some_and(|p| p.ext == ext);
        let store_prefix = prefix.is_some() && !hit && full;
        if store_prefix {
            let mut pc = self.prefix.borrow_mut();
            if !pc.as_ref().is_some_and(|p| p.res == res && p.ext == ext) {
                let t = |l| Tex::new(device, l, res, res, RGBA, copy_usage());
                let x = |l| if ext { t(l) } else { Tex::new(device, l, 1, 1, RGBA, copy_usage()) };
                *pc = Some(PrefixCache {
                    set: set.uid,
                    key,
                    res,
                    ext,
                    tex: [t("prefix col"), t("prefix mrh"), t("prefix emi"), t("prefix nrm"), x("prefix x0"), x("prefix x1"), x("prefix x2")],
                });
            }
        }

        let mut arenas = self.arenas.borrow_mut();
        let arena = arenas.entry(size).or_default();
        let finals = [&set.final_base, &set.final_orm, &set.final_normal, &set.final_emissive, &set.final_ext[0], &set.final_ext[1], &set.final_ext[2]];
        let mip0: Vec<wgpu::TextureView> = finals.iter().map(|t| t.mip_view(0)).collect();
        let mut fin = fin;
        fin.k[2] = ext as u32 as f32;
        let fin_u = ubuf(device, bytemuck::bytes_of(&fin));
        let mut touched: Option<Region> = None;
        for inner in chunks(area, tile) {
            let scratch = [
                inner[0].saturating_sub(halo),
                inner[1].saturating_sub(halo),
                (inner[2] + halo).min(res),
                (inner[3] + halo).min(res),
            ];
            let (sx, sy) = (scratch[0], scratch[1]);
            let (sw, sh) = (scratch[2] - sx, scratch[3] - sy);
            let tb = tile_buf(device, (sx, sy), (sw, sh), res, (sx, sy), inner);
            let mut ctx = Ctx { device, enc: &mut *enc, set, arena: &mut *arena, size, capture, scratch, inner, tile: tb, masks: &masks, ext };
            let cur = ctx.acc();
            if hit {
                let pc = self.prefix.borrow();
                let pc = pc.as_ref().unwrap();
                for k in 0..ctx.acc_count() {
                    copy_rect(ctx.enc, &pc.tex[k], (sx, sy), &ctx.arena.acc[cur][k], (0, 0), sw, sh);
                }
            } else {
                self.clear_pass(&mut ctx, &clear, cur);
            }
            let cur = if hit {
                cur
            } else {
                let cur = self.exec_list(&mut ctx, &plan[..split], cur, false);
                if store_prefix {
                    let pc = self.prefix.borrow();
                    let p = pc.as_ref().unwrap();
                    let (lo, w, h) = ctx.inner_local();
                    for k in 0..ctx.acc_count() {
                        copy_rect(ctx.enc, &ctx.arena.acc[cur][k], lo, &p.tex[k], (inner[0], inner[1]), w, h);
                    }
                }
                cur
            };
            let cur = self.exec_list(&mut ctx, &plan[split..], cur, false);

            // Finals: every texel whose nearest covered texel this tile owns.
            if let Some(d) = set.dilation_rect(inner) {
                let ft = tile_buf(device, (d[0], d[1]), (d[2] - d[0], d[3] - d[1]), res, (sx, sy), inner);
                let a = &ctx.arena.acc[cur];
                dispatch(
                    device,
                    ctx.enc,
                    &self.p.finalize,
                    &[
                        fin_u.as_entire_binding_entry(0),
                        tv(1, &a[0].view),
                        tv(2, &a[1].view),
                        tv(3, &a[2].view),
                        tv(4, &a[3].view),
                        tv(5, &set.baked_normal.view),
                        tv(6, &set.baked_ao.view),
                        tv(7, &set.nearest.view),
                        tv(8, &mip0[0]),
                        tv(9, &mip0[1]),
                        tv(10, &mip0[2]),
                        tv(11, &mip0[3]),
                        tv(12, &a[4].view),
                        tv(13, &a[5].view),
                        tv(14, &a[6].view),
                        tv(15, &mip0[4]),
                        tv(16, &mip0[5]),
                        tv(17, &mip0[6]),
                    ],
                    &ft,
                    (d[2] - d[0], d[3] - d[1]),
                );
                touched = union(touched, d);
            }
            ctx.arena.acc_free.push(cur);
        }
        if store_prefix {
            if let Some(p) = self.prefix.borrow_mut().as_mut() {
                p.set = set.uid;
                p.key = key;
            }
        }
        self.commit_masks(set, plan, &masks);

        // Mips of the texels that changed.
        let Some(t) = touched else { return };
        for f in finals.into_iter().filter(|f| f.width == res) {
            for level in 1..f.mips {
                let src = f.mip_view(level - 1);
                let dst = f.mip_view(level);
                let lsize = (res >> level).max(1);
                let r = [t[0] >> level, t[1] >> level, (t[2].div_ceil(1 << level)).min(lsize), (t[3].div_ceil(1 << level)).min(lsize)];
                let (w, h) = (r[2].saturating_sub(r[0]).max(1), r[3].saturating_sub(r[1]).max(1));
                let tb = tile_buf(device, (r[0], r[1]), (w, h), res, (0, 0), [0, 0, 0, 0]);
                dispatch(device, enc, &self.p.mip, &[tv(0, &src), tv(1, &dst)], &tb, (w, h));
            }
        }
    }

    /// Decides, once per run, which cached masks are reused and which get
    /// (re)stored. A tiled run must not read a cache it is still writing.
    fn mask_policy(&self, device: &wgpu::Device, set: &TextureSetGpu, plan: &[Plan], full: bool) -> MaskPolicy {
        fn walk<'p>(plan: &'p [Plan<'p>], out: &mut Vec<(u64, u64)>) {
            for p in plan {
                if let Some(Some(c)) = p.mask().map(|m| m.cache) {
                    out.push(c);
                }
                if let Plan::Folder { children, .. } = p {
                    walk(children, out);
                }
            }
        }
        let mut keys = Vec::new();
        walk(plan, &mut keys);
        let res = set.resolution;
        let mut cache = self.mask_cache.borrow_mut();
        let mut out = MaskPolicy::new();
        for (lid, hash) in keys {
            let entry = cache.get(&(set.uid, lid)).filter(|(_, t)| t.width == res);
            let hit = entry.is_some_and(|(h, _)| *h == hash);
            // A partial run can only refresh an entry that is valid elsewhere.
            let store = !hit && (full || entry.is_some());
            if store && entry.is_none() {
                cache.insert((set.uid, lid), (0, Tex::new(device, "mask cache", res, res, R32, copy_usage())));
            }
            out.insert(lid, (hit, store));
        }
        out
    }

    fn commit_masks(&self, set: &TextureSetGpu, plan: &[Plan], masks: &MaskPolicy) {
        fn walk<'p>(plan: &'p [Plan<'p>], out: &mut Vec<(u64, u64)>) {
            for p in plan {
                if let Some(Some(c)) = p.mask().map(|m| m.cache) {
                    out.push(c);
                }
                if let Plan::Folder { children, .. } = p {
                    walk(children, out);
                }
            }
        }
        let mut keys = Vec::new();
        walk(plan, &mut keys);
        let mut cache = self.mask_cache.borrow_mut();
        for (lid, hash) in keys {
            if masks.get(&lid).is_some_and(|(_, store)| *store) {
                if let Some(e) = cache.get_mut(&(set.uid, lid)) {
                    e.0 = hash;
                }
            }
        }
    }

    /// Evaluates `plan` over `clear` and copies the accumulators into `out`
    /// (brush material painting).
    pub fn evaluate_into(
        &self,
        device: &wgpu::Device,
        enc: &mut wgpu::CommandEncoder,
        set: &TextureSetGpu,
        clear: ClearU,
        plan: &[Plan],
        out: &[Tex; 4],
    ) {
        let res = set.resolution;
        let (tile, size, halo) = self.layout(plan, res);
        let masks = MaskPolicy::new();
        let mut arenas = self.arenas.borrow_mut();
        let arena = arenas.entry(size).or_default();
        for inner in chunks([0, 0, res, res], tile) {
            let scratch = [inner[0].saturating_sub(halo), inner[1].saturating_sub(halo), (inner[2] + halo).min(res), (inner[3] + halo).min(res)];
            let (sx, sy) = (scratch[0], scratch[1]);
            let tb = tile_buf(device, (sx, sy), (scratch[2] - sx, scratch[3] - sy), res, (sx, sy), inner);
            let mut ctx = Ctx { device, enc: &mut *enc, set, arena: &mut *arena, size, capture: None, scratch, inner, tile: tb, masks: &masks, ext: false };
            let cur = ctx.acc();
            self.clear_pass(&mut ctx, &clear, cur);
            let cur = self.exec_list(&mut ctx, plan, cur, false);
            let (lo, w, h) = ctx.inner_local();
            for k in 0..4 {
                copy_rect(ctx.enc, &ctx.arena.acc[cur][k], lo, &out[k], (inner[0], inner[1]), w, h);
            }
            ctx.arena.acc_free.push(cur);
        }
    }

    fn clear_pass(&self, ctx: &mut Ctx, clear: &ClearU, cur: usize) {
        let u = ubuf(ctx.device, bytemuck::bytes_of(clear));
        let a = &ctx.arena.acc[cur];
        let mut e = vec![u.as_entire_binding_entry(0)];
        e.extend((0..ACC).map(|k| tv(1 + k as u32, &a[k].view)));
        dispatch(ctx.device, ctx.enc, &self.p.clear, &e, &ctx.tile, ctx.extent());
    }

    fn exec_list(&self, ctx: &mut Ctx, plan: &[Plan], mut cur: usize, keep_input: bool) -> usize {
        let mut first = true;
        for node in plan {
            let next = self.exec_node(ctx, node, cur, keep_input && first);
            if next != cur {
                first = false;
            }
            cur = next;
        }
        cur
    }

    fn exec_node(&self, ctx: &mut Ctx, node: &Plan, cur: usize, keep: bool) -> usize {
        let mask = node.mask().map(|m| self.eval_mask(ctx, m, cur));
        let out = match node {
            Plan::Paint { blend, tex, .. } => {
                let o = ctx.acc();
                let views = [
                    &tex.color.view,
                    &tex.vals.view,
                    &tex.alph.view,
                    &tex.emis.unwrap_or(&self.d_rgba).view,
                    &tex.nrm.unwrap_or(&self.d_rgba).view,
                    &tex.opac.unwrap_or(&self.d_rgba).view,
                ];
                self.blend_pass(ctx, cur, Content::Views(views), mask, blend, o);
                o
            }
            Plan::Fill { fill, textures, .. } => {
                let o = ctx.acc();
                self.fill_pass(ctx, fill, textures, cur, mask, o);
                o
            }
            Plan::Filter { params, blur_radius, .. } => {
                let blurred = if *blur_radius > 0.0 {
                    let t1 = ctx.acc();
                    self.acc_blur(ctx, cur, t1, [1, 0], *blur_radius);
                    let t2 = ctx.acc();
                    self.acc_blur(ctx, t1, t2, [0, 1], *blur_radius);
                    ctx.arena.acc_free.push(t1);
                    Some(t2)
                } else {
                    None
                };
                let o = ctx.acc();
                let mut u = *params;
                u.k[2] = mask.is_some() as u32;
                let ub = ubuf(ctx.device, bytemuck::bytes_of(&u));
                {
                    let a = &ctx.arena.acc[cur];
                    let l = &ctx.arena.acc[blurred.unwrap_or(cur)];
                    let out = &ctx.arena.acc[o];
                    let m = mask.map(|m| &ctx.arena.mask[m].view).unwrap_or(&self.d_mask.view);
                    dispatch(
                        ctx.device,
                        ctx.enc,
                        &self.p.filter,
                        &[
                            ub.as_entire_binding_entry(0),
                            tv(1, &a[0].view),
                            tv(2, &a[1].view),
                            tv(3, &a[2].view),
                            tv(4, &a[3].view),
                            tv(5, &l[0].view),
                            tv(6, &l[1].view),
                            tv(7, &l[2].view),
                            tv(8, &l[3].view),
                            tv(9, m),
                            tv(10, &out[0].view),
                            tv(11, &out[1].view),
                            tv(12, &out[2].view),
                            tv(13, &out[3].view),
                            tv(14, &a[4].view),
                            tv(15, &a[5].view),
                            tv(16, &a[6].view),
                            tv(17, &out[4].view),
                            tv(18, &out[5].view),
                            tv(19, &out[6].view),
                        ],
                        &ctx.tile,
                        ctx.extent(),
                    );
                }
                if let Some(b) = blurred {
                    ctx.arena.acc_free.push(b);
                }
                o
            }
            Plan::Folder { params, children, .. } => {
                let after = self.exec_list(ctx, children, cur, true);
                if after == cur {
                    if let Some(m) = mask {
                        ctx.arena.mask_free.push(m);
                    }
                    return cur;
                }
                let o = ctx.acc();
                let mut u = *params;
                u.flags[0] = mask.is_some() as u32;
                let ub = ubuf(ctx.device, bytemuck::bytes_of(&u));
                {
                    let b = &ctx.arena.acc[cur];
                    let f = &ctx.arena.acc[after];
                    let out = &ctx.arena.acc[o];
                    let m = mask.map(|m| &ctx.arena.mask[m].view).unwrap_or(&self.d_mask.view);
                    dispatch(
                        ctx.device,
                        ctx.enc,
                        &self.p.folder,
                        &[
                            ub.as_entire_binding_entry(0),
                            tv(1, &b[0].view),
                            tv(2, &b[1].view),
                            tv(3, &b[2].view),
                            tv(4, &b[3].view),
                            tv(5, &f[0].view),
                            tv(6, &f[1].view),
                            tv(7, &f[2].view),
                            tv(8, &f[3].view),
                            tv(9, m),
                            tv(10, &out[0].view),
                            tv(11, &out[1].view),
                            tv(12, &out[2].view),
                            tv(13, &out[3].view),
                            tv(14, &b[4].view),
                            tv(15, &b[5].view),
                            tv(16, &b[6].view),
                            tv(17, &f[4].view),
                            tv(18, &f[5].view),
                            tv(19, &f[6].view),
                            tv(20, &out[4].view),
                            tv(21, &out[5].view),
                            tv(22, &out[6].view),
                        ],
                        &ctx.tile,
                        ctx.extent(),
                    );
                }
                ctx.arena.acc_free.push(after);
                o
            }
        };
        if let Some(m) = mask {
            ctx.arena.mask_free.push(m);
        }
        if !keep {
            ctx.arena.acc_free.push(cur);
        }
        out
    }

    fn blend_pass(
        &self,
        ctx: &mut Ctx,
        cur: usize,
        content: Content,
        mask: Option<usize>,
        blend: &BlendU,
        out: usize,
    ) {
        let content: [&wgpu::TextureView; 6] = match content {
            Content::Views(v) => v,
        };
        let mut u = *blend;
        u.mode_b[3] = mask.is_some() as u32;
        let ub = ubuf(ctx.device, bytemuck::bytes_of(&u));
        let a = &ctx.arena.acc[cur];
        let o = &ctx.arena.acc[out];
        let m = mask.map(|m| &ctx.arena.mask[m].view).unwrap_or(&self.d_mask.view);
        dispatch(
            ctx.device,
            ctx.enc,
            &self.p.blend,
            &[
                ub.as_entire_binding_entry(0),
                tv(1, &a[0].view),
                tv(2, &a[1].view),
                tv(3, &a[2].view),
                tv(4, &a[3].view),
                tv(5, content[0]),
                tv(6, content[1]),
                tv(7, content[2]),
                tv(8, content[3]),
                tv(9, content[4]),
                tv(10, content[5]),
                tv(11, m),
                tv(12, &o[0].view),
                tv(13, &o[1].view),
                tv(14, &o[2].view),
                tv(15, &o[3].view),
                tv(16, &a[4].view),
                tv(17, &a[5].view),
                tv(18, &a[6].view),
                tv(19, &o[4].view),
                tv(20, &o[5].view),
                tv(21, &o[6].view),
            ],
            &ctx.tile,
            ctx.extent(),
        );
    }

    fn fill_pass(&self, ctx: &mut Ctx, fill: &FillU, textures: &[Option<&Tex>; 7], cur: usize, mask: Option<usize>, out: usize) {
        let mut u = *fill;
        u.mode_b[3] = mask.is_some() as u32;
        let ub = ubuf(ctx.device, bytemuck::bytes_of(&u));
        let a = &ctx.arena.acc[cur];
        let o = &ctx.arena.acc[out];
        let m = mask.map(|m| &ctx.arena.mask[m].view).unwrap_or(&self.d_mask.view);
        let t = |i: usize| &textures[i].unwrap_or(&self.d_img).view;
        dispatch(
            ctx.device,
            ctx.enc,
            &self.p.fill,
            &[
                ub.as_entire_binding_entry(0),
                tv(1, t(0)),
                tv(2, t(1)),
                tv(3, t(2)),
                tv(4, t(3)),
                tv(5, t(4)),
                tv(6, t(5)),
                tv(7, t(6)),
                wgpu::BindGroupEntry { binding: 8, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                tv(9, &ctx.set.baked_position.view),
                tv(10, &ctx.set.baked_wnormal.view),
                tv(11, &a[0].view),
                tv(12, &a[1].view),
                tv(13, &a[2].view),
                tv(14, &a[3].view),
                tv(15, m),
                tv(16, &o[0].view),
                tv(17, &o[1].view),
                tv(18, &o[2].view),
                tv(19, &o[3].view),
                tv(20, &a[4].view),
                tv(21, &a[5].view),
                tv(22, &a[6].view),
                tv(23, &o[4].view),
                tv(24, &o[5].view),
                tv(25, &o[6].view),
            ],
            &ctx.tile,
            ctx.extent(),
        );
    }

    fn acc_blur(&self, ctx: &mut Ctx, src: usize, dst: usize, dir: [i32; 2], radius: f32) {
        let u = BlurU { dir: [dir[0], dir[1], 0, 0], k: [radius, 0.0, 0.0, 0.0] };
        let ub = ubuf(ctx.device, bytemuck::bytes_of(&u));
        let a = &ctx.arena.acc[src];
        let o = &ctx.arena.acc[dst];
        dispatch(
            ctx.device,
            ctx.enc,
            &self.p.accblur,
            &[
                ub.as_entire_binding_entry(0),
                tv(1, &a[0].view),
                tv(2, &a[1].view),
                tv(3, &a[2].view),
                tv(4, &a[3].view),
                tv(5, &ctx.set.nearest.view),
                tv(6, &o[0].view),
                tv(7, &o[1].view),
                tv(8, &o[2].view),
                tv(9, &o[3].view),
                tv(10, &a[4].view),
                tv(11, &a[5].view),
                tv(12, &a[6].view),
                tv(13, &o[4].view),
                tv(14, &o[5].view),
                tv(15, &o[6].view),
            ],
            &ctx.tile,
            ctx.extent(),
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn mask_pass(&self, ctx: &mut Ctx, params: &MaskU, m_in: Option<usize>, paint: Option<&Tex>, texture: Option<&Tex>, out: usize, acc: Option<usize>) {
        let ub = ubuf(ctx.device, bytemuck::bytes_of(params));
        let set = ctx.set;
        let input = m_in.map(|i| &ctx.arena.mask[i].view).unwrap_or(&self.d_mask.view);
        let acc_view = acc.map(|a| &ctx.arena.acc[a][1].view).unwrap_or(&self.d_rgba.view);
        dispatch(
            ctx.device,
            ctx.enc,
            &self.p.mask,
            &[
                ub.as_entire_binding_entry(0),
                tv(1, input),
                tv(2, &paint.unwrap_or(&self.d_rg).view),
                tv(3, &texture.unwrap_or(&self.d_img).view),
                wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                tv(5, &set.baked_ao.view),
                tv(6, &set.baked_curvature.view),
                tv(7, &set.baked_position.view),
                tv(8, &set.baked_wnormal.view),
                tv(9, &set.baked_thickness.view),
                tv(10, &set.baked_id.view),
                tv(11, &ctx.arena.mask[out].view),
                tv(12, acc_view),
            ],
            &ctx.tile,
            ctx.extent(),
        );
    }

    fn eval_mask(&self, ctx: &mut Ctx, mp: &MaskPlan, acc: usize) -> usize {
        let mut m = ctx.arena.mask(ctx.device, ctx.size);
        let (sx, sy) = ctx.origin();
        let (sw, sh) = ctx.extent();
        let (lo, iw, ih) = ctx.inner_local();
        let (hit, store) = mp.cache.and_then(|(lid, _)| ctx.masks.get(&lid).copied()).unwrap_or((false, false));
        if hit {
            let cache = self.mask_cache.borrow();
            if let Some((_, tex)) = mp.cache.and_then(|(lid, _)| cache.get(&(ctx.set.uid, lid))) {
                copy_rect(ctx.enc, tex, (sx, sy), &ctx.arena.mask[m], (0, 0), sw, sh);
                if mp.capture {
                    if let Some(cap) = ctx.capture {
                        copy_rect(ctx.enc, tex, (ctx.inner[0], ctx.inner[1]), cap, (ctx.inner[0], ctx.inner[1]), iw, ih);
                    }
                }
                return m;
            }
        }
        let mut clear = MaskU::default();
        clear.o[1] = mp.base;
        self.mask_pass(ctx, &clear, None, None, None, m, None);
        for step in &mp.steps {
            match step {
                MaskStep::Effect { params, paint, texture } => {
                    let o = ctx.arena.mask(ctx.device, ctx.size);
                    self.mask_pass(ctx, params, Some(m), *paint, *texture, o, Some(acc));
                    ctx.arena.mask_free.push(m);
                    m = o;
                }
                MaskStep::Blur { radius, opacity } => {
                    let t = ctx.arena.mask(ctx.device, ctx.size);
                    let o = ctx.arena.mask(ctx.device, ctx.size);
                    for (dir, src, dst) in [([1, 0], m, t), ([0, 1], t, o)] {
                        let u = BlurU { dir: [dir[0], dir[1], 0, 0], k: [*radius, *opacity, 0.0, 0.0] };
                        let ub = ubuf(ctx.device, bytemuck::bytes_of(&u));
                        dispatch(
                            ctx.device,
                            ctx.enc,
                            &self.p.maskblur,
                            &[
                                ub.as_entire_binding_entry(0),
                                tv(1, &ctx.arena.mask[src].view),
                                tv(2, &ctx.arena.mask[m].view),
                                tv(3, &ctx.set.nearest.view),
                                tv(4, &ctx.arena.mask[dst].view),
                            ],
                            &ctx.tile,
                            ctx.extent(),
                        );
                    }
                    ctx.arena.mask_free.push(t);
                    ctx.arena.mask_free.push(m);
                    m = o;
                }
            }
        }
        if mp.capture {
            if let Some(cap) = ctx.capture {
                copy_rect(ctx.enc, &ctx.arena.mask[m], lo, cap, (ctx.inner[0], ctx.inner[1]), iw, ih);
            }
        }
        if store {
            let cache = self.mask_cache.borrow();
            if let Some((_, tex)) = mp.cache.and_then(|(lid, _)| cache.get(&(ctx.set.uid, lid))) {
                copy_rect(ctx.enc, &ctx.arena.mask[m], lo, tex, (ctx.inner[0], ctx.inner[1]), iw, ih);
            }
        }
        m
    }
}

fn collect_mask_layers(plan: &[Plan], out: &mut Vec<u64>) {
    for p in plan {
        if let Some(Some((l, _))) = p.mask().map(|m| m.cache) {
            out.push(l);
        }
        if let Plan::Folder { children, .. } = p {
            collect_mask_layers(children, out);
        }
    }
}

trait BindingEntry {
    fn as_entire_binding_entry(&self, binding: u32) -> wgpu::BindGroupEntry<'_>;
}

impl BindingEntry for wgpu::Buffer {
    fn as_entire_binding_entry(&self, binding: u32) -> wgpu::BindGroupEntry<'_> {
        wgpu::BindGroupEntry { binding, resource: self.as_entire_binding() }
    }
}
