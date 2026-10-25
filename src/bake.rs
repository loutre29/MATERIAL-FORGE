//! Mesh map baking on the CPU: UV rasterization, the nearest-texel map used
//! for dilation, position, ambient occlusion, curvature and (with a high
//! poly) tangent-space normals.

use crate::mesh::{MeshData, Vertex};
use glam::{Vec2, Vec3};
use rayon::prelude::*;
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

/// Which triangle covers each texel, with barycentrics.
pub struct UvRaster {
    pub res: u32,
    /// Index into the submesh's triangle list, -1 when uncovered.
    pub tri: Vec<i32>,
    /// (w1, w2); w0 = 1 - w1 - w2.
    pub bary: Vec<[f32; 2]>,
}

impl UvRaster {
    pub fn covered(&self, i: usize) -> bool {
        self.tri[i] >= 0
    }
}

pub fn rasterize_uv(mesh: &MeshData, sub: usize, res: u32) -> UvRaster {
    let n = (res * res) as usize;
    let mut tri = vec![-1i32; n];
    let mut bary = vec![[0.0f32; 2]; n];
    let idx = &mesh.submeshes[sub].indices;
    let r = res as f32;
    for (t, c) in idx.chunks_exact(3).enumerate() {
        let p = |i: u32| {
            let uv = mesh.vertices[i as usize].uv;
            Vec2::new(uv[0] * r, uv[1] * r)
        };
        let (a, b, cc) = (p(c[0]), p(c[1]), p(c[2]));
        let area = (b - a).perp_dot(cc - a);
        if area.abs() < 1e-12 {
            continue;
        }
        let min = a.min(b).min(cc).floor().max(Vec2::ZERO);
        let max = a.max(b).max(cc).ceil().min(Vec2::splat(r - 1.0));
        if min.x > max.x || min.y > max.y {
            continue;
        }
        let eps = -1e-5 * area.abs();
        for y in min.y as u32..=max.y as u32 {
            for x in min.x as u32..=max.x as u32 {
                let q = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
                let w0 = (b - q).perp_dot(cc - q) / area;
                let w1 = (cc - q).perp_dot(a - q) / area;
                let w2 = 1.0 - w0 - w1;
                if w0 * area.abs() >= eps && w1 * area.abs() >= eps && w2 * area.abs() >= eps {
                    let i = (y * res + x) as usize;
                    tri[i] = t as i32;
                    bary[i] = [w1, w2];
                }
            }
        }
    }
    UvRaster { res, tri, bary }
}

/// For each texel, the nearest covered texel (itself when covered). Found by
/// a multi-source breadth-first flood over 8-neighbours.
pub fn nearest_map(raster: &UvRaster) -> Vec<[u32; 2]> {
    let res = raster.res as i32;
    let n = (res * res) as usize;
    let mut out = vec![[u32::MAX, u32::MAX]; n];
    let mut queue = VecDeque::new();
    for i in 0..n {
        if raster.covered(i) {
            out[i] = [(i as i32 % res) as u32, (i as i32 / res) as u32];
            queue.push_back(i);
        }
    }
    if queue.is_empty() {
        for (i, o) in out.iter_mut().enumerate() {
            *o = [(i as i32 % res) as u32, (i as i32 / res) as u32];
        }
        return out;
    }
    while let Some(i) = queue.pop_front() {
        let (x, y) = (i as i32 % res, i as i32 / res);
        let src = out[i];
        for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1), (-1, -1), (1, -1), (-1, 1), (1, 1)] {
            let (nx, ny) = (x + dx, y + dy);
            if nx < 0 || ny < 0 || nx >= res || ny >= res {
                continue;
            }
            let j = (ny * res + nx) as usize;
            if out[j][0] == u32::MAX {
                out[j] = src;
                queue.push_back(j);
            }
        }
    }
    out
}

/// Interpolated surface sample at a covered texel.
#[derive(Clone, Copy)]
pub struct Surface {
    pub pos: Vec3,
    pub normal: Vec3,
    pub tangent: Vec3,
    pub bitangent: Vec3,
}

pub fn surface(mesh: &MeshData, sub: usize, raster: &UvRaster, i: usize) -> Option<Surface> {
    let t = raster.tri[i];
    if t < 0 {
        return None;
    }
    let idx = &mesh.submeshes[sub].indices[t as usize * 3..t as usize * 3 + 3];
    let [w1, w2] = raster.bary[i];
    let w0 = 1.0 - w1 - w2;
    let v: [&Vertex; 3] = [
        &mesh.vertices[idx[0] as usize],
        &mesh.vertices[idx[1] as usize],
        &mesh.vertices[idx[2] as usize],
    ];
    let lerp3 = |f: &dyn Fn(&Vertex) -> Vec3| f(v[0]) * w0 + f(v[1]) * w1 + f(v[2]) * w2;
    let pos = lerp3(&|x| Vec3::from(x.pos));
    let normal = lerp3(&|x| Vec3::from(x.normal)).normalize_or(Vec3::Y);
    let tan = lerp3(&|x| Vec3::new(x.tangent[0], x.tangent[1], x.tangent[2]));
    let sign = if v[0].tangent[3] < 0.0 { -1.0 } else { 1.0 };
    let tangent = (tan - normal * normal.dot(tan)).normalize_or(normal.any_orthonormal_vector());
    let bitangent = normal.cross(tangent) * sign;
    Some(Surface { pos, normal, tangent, bitangent })
}

/// Object position normalized to the bounding box, used by noise and
/// gradient generators.
pub fn position_map(mesh: &MeshData, sub: usize, raster: &UvRaster) -> Vec<[f32; 4]> {
    use rayon::prelude::*;
    let size = (mesh.bbox_max - mesh.bbox_min).max(Vec3::splat(1e-6));
    let ext = size.max_element();
    (0..raster.tri.len())
        .into_par_iter()
        .map(|i| match surface(mesh, sub, raster, i) {
            Some(s) => {
                let p = (s.pos - mesh.bbox_min) / ext;
                [p.x, p.y / (size.y / ext), p.z, 1.0]
            }
            None => [0.0; 4],
        })
        .collect()
}

// ---------------------------------------------------------------- BVH

#[derive(Clone, Copy)]
struct BvhNode {
    min: Vec3,
    max: Vec3,
    /// Leaf: first triangle; inner: left child (right = left + 1).
    start: u32,
    /// Triangle count, 0 for inner nodes.
    count: u32,
}

pub struct Bvh {
    nodes: Vec<BvhNode>,
    tris: Vec<[Vec3; 3]>,
    /// Original triangle id of each entry in `tris`.
    ids: Vec<u32>,
}

impl Bvh {
    /// GPU layout: per node (min.xyz, start bits), (max.xyz, count bits); and
    /// the original triangle id of each sorted triangle slot.
    pub fn flatten(&self) -> (Vec<[f32; 8]>, &[u32]) {
        let nodes = self
            .nodes
            .iter()
            .map(|n| {
                [n.min.x, n.min.y, n.min.z, f32::from_bits(n.start), n.max.x, n.max.y, n.max.z, f32::from_bits(n.count)]
            })
            .collect();
        (nodes, &self.ids)
    }

    /// Triangle positions in sorted (BVH leaf) order, 3 x vec4 each.
    pub fn flatten_positions(&self) -> Vec<[f32; 12]> {
        self.tris
            .iter()
            .map(|t| [t[0].x, t[0].y, t[0].z, 0.0, t[1].x, t[1].y, t[1].z, 0.0, t[2].x, t[2].y, t[2].z, 0.0])
            .collect()
    }
}

/// Traces the AO and thickness rays of many surface samples at once (GPU).
pub trait RayBackend: Sync {
    /// samples: (position, normal, tangent) per covered texel; the texel
    /// index seeds the sample pattern. Returns (ao, thickness 0..1) per
    /// sample, or None when cancelled / failed.
    #[allow(clippy::too_many_arguments)]
    fn ao_thickness(
        &self,
        samples: &[[f32; 12]],
        ao_samples: u32,
        ao_dist: f32,
        th_samples: u32,
        th_dist: f32,
        eps: f32,
        cancel: &AtomicBool,
        on_progress: &dyn Fn(f32),
    ) -> Option<Vec<[f32; 2]>>;
}

pub struct Hit {
    pub t: f32,
    pub tri: u32,
    pub u: f32,
    pub v: f32,
}

impl Bvh {
    pub fn build(tris: Vec<[Vec3; 3]>) -> Self {
        let mut order: Vec<u32> = (0..tris.len() as u32).collect();
        let centroids: Vec<Vec3> = tris.iter().map(|t| (t[0] + t[1] + t[2]) / 3.0).collect();
        let mut nodes = Vec::with_capacity(tris.len() * 2);
        nodes.push(BvhNode { min: Vec3::ZERO, max: Vec3::ZERO, start: 0, count: tris.len() as u32 });
        let mut stack = vec![0usize];
        while let Some(ni) = stack.pop() {
            let (start, count) = (nodes[ni].start as usize, nodes[ni].count as usize);
            let mut min = Vec3::splat(f32::MAX);
            let mut max = Vec3::splat(f32::MIN);
            let mut cmin = Vec3::splat(f32::MAX);
            let mut cmax = Vec3::splat(f32::MIN);
            for &t in &order[start..start + count] {
                for p in tris[t as usize] {
                    min = min.min(p);
                    max = max.max(p);
                }
                cmin = cmin.min(centroids[t as usize]);
                cmax = cmax.max(centroids[t as usize]);
            }
            nodes[ni].min = min;
            nodes[ni].max = max;
            if count <= 4 {
                continue;
            }
            // Binned SAH split.
            let extent = cmax - cmin;
            let axis = if extent.x >= extent.y && extent.x >= extent.z {
                0
            } else if extent.y >= extent.z {
                1
            } else {
                2
            };
            if extent[axis] < 1e-12 {
                continue;
            }
            const BINS: usize = 12;
            let mut bin_count = [0usize; BINS];
            let mut bin_min = [Vec3::splat(f32::MAX); BINS];
            let mut bin_max = [Vec3::splat(f32::MIN); BINS];
            let bin_of = |c: Vec3| (((c[axis] - cmin[axis]) / extent[axis] * BINS as f32) as usize).min(BINS - 1);
            for &t in &order[start..start + count] {
                let b = bin_of(centroids[t as usize]);
                bin_count[b] += 1;
                for p in tris[t as usize] {
                    bin_min[b] = bin_min[b].min(p);
                    bin_max[b] = bin_max[b].max(p);
                }
            }
            let area = |mn: Vec3, mx: Vec3| {
                let d = (mx - mn).max(Vec3::ZERO);
                d.x * d.y + d.y * d.z + d.z * d.x
            };
            let mut best = (f32::MAX, 0usize);
            for split in 1..BINS {
                let (mut lmin, mut lmax, mut lc) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN), 0);
                let (mut rmin, mut rmax, mut rc) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN), 0);
                for b in 0..BINS {
                    if bin_count[b] == 0 {
                        continue;
                    }
                    if b < split {
                        lmin = lmin.min(bin_min[b]);
                        lmax = lmax.max(bin_max[b]);
                        lc += bin_count[b];
                    } else {
                        rmin = rmin.min(bin_min[b]);
                        rmax = rmax.max(bin_max[b]);
                        rc += bin_count[b];
                    }
                }
                if lc == 0 || rc == 0 {
                    continue;
                }
                let cost = area(lmin, lmax) * lc as f32 + area(rmin, rmax) * rc as f32;
                if cost < best.0 {
                    best = (cost, split);
                }
            }
            if best.0 == f32::MAX {
                continue;
            }
            let slice = &mut order[start..start + count];
            let mut left = 0;
            for i in 0..slice.len() {
                if bin_of(centroids[slice[i] as usize]) < best.1 {
                    slice.swap(i, left);
                    left += 1;
                }
            }
            let l = nodes.len();
            nodes.push(BvhNode { min: Vec3::ZERO, max: Vec3::ZERO, start: start as u32, count: left as u32 });
            nodes.push(BvhNode {
                min: Vec3::ZERO,
                max: Vec3::ZERO,
                start: (start + left) as u32,
                count: (count - left) as u32,
            });
            nodes[ni].start = l as u32;
            nodes[ni].count = 0;
            stack.push(l);
            stack.push(l + 1);
        }
        let sorted: Vec<[Vec3; 3]> = order.iter().map(|&t| tris[t as usize]).collect();
        Bvh { nodes, tris: sorted, ids: order }
    }

    fn slab(n: &BvhNode, o: Vec3, inv: Vec3, tmax: f32) -> Option<f32> {
        let t0 = (n.min - o) * inv;
        let t1 = (n.max - o) * inv;
        let tn = t0.min(t1).max_element().max(0.0);
        let tf = t0.max(t1).min_element().min(tmax);
        (tn <= tf).then_some(tn)
    }

    /// Closest hit (any_hit = false) or first hit found (any_hit = true).
    pub fn trace(&self, o: Vec3, d: Vec3, tmax: f32, any_hit: bool) -> Option<Hit> {
        self.trace_where(o, d, tmax, any_hit, None)
    }

    /// Like `trace`, ignoring triangles (original ids) the filter rejects.
    pub fn trace_where(&self, o: Vec3, d: Vec3, tmax: f32, any_hit: bool, filter: Option<&dyn Fn(u32) -> bool>) -> Option<Hit> {
        if self.tris.is_empty() {
            return None;
        }
        // Finite reciprocals: 0 * inf would turn the slab test into NaN for
        // axis-aligned rays touching a box face.
        let safe = |v: f32| if v.abs() < 1e-20 { 1e30f32.copysign(v) } else { 1.0 / v };
        let inv = Vec3::new(safe(d.x), safe(d.y), safe(d.z));
        let mut best: Option<Hit> = None;
        let mut tlimit = tmax;
        let mut stack = [0u32; 64];
        let mut sp = 1usize;
        stack[0] = 0;
        while sp > 0 {
            sp -= 1;
            let n = &self.nodes[stack[sp] as usize];
            if Self::slab(n, o, inv, tlimit).is_none() {
                continue;
            }
            if n.count > 0 {
                for k in n.start..n.start + n.count {
                    let [a, b, c] = self.tris[k as usize];
                    let e1 = b - a;
                    let e2 = c - a;
                    let p = d.cross(e2);
                    let det = e1.dot(p);
                    if det.abs() < 1e-14 {
                        continue;
                    }
                    let id = 1.0 / det;
                    let s = o - a;
                    let u = s.dot(p) * id;
                    if !(0.0..=1.0).contains(&u) {
                        continue;
                    }
                    let q = s.cross(e1);
                    let v = d.dot(q) * id;
                    if v < 0.0 || u + v > 1.0 {
                        continue;
                    }
                    let t = e2.dot(q) * id;
                    if t > 0.0 && t < tlimit && filter.map_or(true, |f| f(self.ids[k as usize])) {
                        tlimit = t;
                        best = Some(Hit { t, tri: self.ids[k as usize], u, v });
                        if any_hit {
                            return best;
                        }
                    }
                }
            } else if sp + 2 <= stack.len() {
                let (l, r) = (n.start, n.start + 1);
                let dl = Self::slab(&self.nodes[l as usize], o, inv, tlimit);
                let dr = Self::slab(&self.nodes[r as usize], o, inv, tlimit);
                // Push the far child first so the near one is visited first.
                match (dl, dr) {
                    (Some(a), Some(b)) => {
                        let (near, far) = if a <= b { (l, r) } else { (r, l) };
                        stack[sp] = far;
                        stack[sp + 1] = near;
                        sp += 2;
                    }
                    (Some(_), None) => {
                        stack[sp] = l;
                        sp += 1;
                    }
                    (None, Some(_)) => {
                        stack[sp] = r;
                        sp += 1;
                    }
                    (None, None) => {}
                }
            }
        }
        best
    }
}

/// All triangles of a mesh in world space; ids index `mesh_triangles`.
pub fn mesh_triangles(mesh: &MeshData) -> Vec<[u32; 3]> {
    mesh.submeshes.iter().flat_map(|s| s.indices.chunks_exact(3).map(|c| [c[0], c[1], c[2]])).collect()
}

pub fn build_bvh(mesh: &MeshData, tris: &[[u32; 3]]) -> Bvh {
    Bvh::build(
        tris.iter()
            .map(|t| t.map(|i| Vec3::from(mesh.vertices[i as usize].pos)))
            .collect(),
    )
}

// ---------------------------------------------------------------- baking

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum IdSource {
    Object,
    VertexColor,
    UvIsland,
    Material,
}

impl IdSource {
    pub const ALL: [IdSource; 4] = [IdSource::Object, IdSource::VertexColor, IdSource::UvIsland, IdSource::Material];
    pub fn name(self) -> &'static str {
        match self {
            IdSource::Object => "Mesh object",
            IdSource::VertexColor => "Vertex color",
            IdSource::UvIsland => "UV island",
            IdSource::Material => "Material",
        }
    }
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct BakeSettings {
    pub ao_samples: u32,
    /// Fraction of the bounding-box diagonal.
    pub ao_distance: f32,
    /// Fraction of the diagonal searched in front of / behind the low poly.
    pub cage_distance: f32,
    pub high_poly: Option<std::path::PathBuf>,
    /// Width of the sharp-edge highlight, fraction of the diagonal.
    pub edge_width: f32,
    /// Supersampling per axis (1, 2 or 4).
    pub antialias: u32,
    pub id_source: IdSource,
    /// Fraction of the diagonal.
    pub thickness_distance: f32,
    pub thickness_samples: u32,
    /// High poly: only transfer from parts whose name matches (bolt_low <- bolt_high).
    pub match_by_name: bool,
}

impl Default for BakeSettings {
    fn default() -> Self {
        Self {
            ao_samples: 64,
            ao_distance: 0.15,
            cage_distance: 0.02,
            high_poly: None,
            edge_width: 0.006,
            antialias: 1,
            id_source: IdSource::Object,
            thickness_distance: 0.1,
            thickness_samples: 16,
            match_by_name: false,
        }
    }
}

pub struct BakeResult {
    pub ao: Vec<f32>,
    pub curvature: Vec<f32>,
    /// Tangent-space normal in [-1, 1], w = 1.
    pub normal: Vec<[f32; 4]>,
    /// World-space normal in [-1, 1], w = 1.
    pub wnormal: Vec<[f32; 4]>,
    pub thickness: Vec<f32>,
    pub id: Vec<[f32; 4]>,
}

fn radical_inverse(mut bits: u32) -> f32 {
    bits = bits.rotate_right(16);
    bits = ((bits & 0x55555555) << 1) | ((bits & 0xAAAAAAAA) >> 1);
    bits = ((bits & 0x33333333) << 2) | ((bits & 0xCCCCCCCC) >> 2);
    bits = ((bits & 0x0F0F0F0F) << 4) | ((bits & 0xF0F0F0F0) >> 4);
    bits = ((bits & 0x00FF00FF) << 8) | ((bits & 0xFF00FF00) >> 8);
    bits as f32 * 2.328_306_4e-10
}

fn hash(i: u32) -> f32 {
    let mut x = i.wrapping_mul(0x9E3779B9);
    x ^= x >> 16;
    x = x.wrapping_mul(0x85EBCA6B);
    x ^= x >> 13;
    (x & 0xFFFFFF) as f32 / 16777216.0
}

/// A saturated, well separated color for an id.
pub fn id_color(i: u32) -> [f32; 3] {
    let h = (i as f32 * 0.618034).fract();
    let s = 0.65 + 0.35 * hash(i * 7 + 1);
    let v = 0.75 + 0.25 * hash(i * 13 + 5);
    let c = v * s;
    let hp = h * 6.0;
    let x = c * (1.0 - (hp % 2.0 - 1.0).abs());
    let (r, g, b) = match hp as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = v - c;
    [r + m, g + m, b + m]
}

struct UnionFind(Vec<u32>);

impl UnionFind {
    fn new(n: usize) -> Self {
        UnionFind((0..n as u32).collect())
    }
    fn find(&mut self, mut a: u32) -> u32 {
        while self.0[a as usize] != a {
            self.0[a as usize] = self.0[self.0[a as usize] as usize];
            a = self.0[a as usize];
        }
        a
    }
    fn union(&mut self, a: u32, b: u32) {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra != rb {
            self.0[ra as usize] = rb;
        }
    }
    fn labels(mut self) -> Vec<u32> {
        let n = self.0.len();
        let mut map = HashMap::new();
        (0..n as u32)
            .map(|i| {
                let r = self.find(i);
                let next = map.len() as u32;
                *map.entry(r).or_insert(next)
            })
            .collect()
    }
}

/// Connected triangle groups of a submesh. `by_uv` joins triangles sharing a
/// UV vertex (UV islands), otherwise triangles sharing a position (mesh parts).
pub fn components(mesh: &MeshData, sub: usize, by_uv: bool) -> Vec<u32> {
    let idx = &mesh.submeshes[sub].indices;
    let ntri = idx.len() / 3;
    let mut uf = UnionFind::new(ntri);
    let mut owner: HashMap<[i64; 3], u32> = HashMap::new();
    for t in 0..ntri {
        for k in 0..3 {
            let v = &mesh.vertices[idx[t * 3 + k] as usize];
            let key = if by_uv {
                [(v.uv[0] * 1e5) as i64, (v.uv[1] * 1e5) as i64, 0]
            } else {
                [(v.pos[0] * 1e4) as i64, (v.pos[1] * 1e4) as i64, (v.pos[2] * 1e4) as i64]
            };
            match owner.get(&key) {
                Some(&o) => uf.union(t as u32, o),
                None => {
                    owner.insert(key, t as u32);
                }
            }
        }
    }
    uf.labels()
}

/// Interpolated world normals per texel (default display data before a bake).
pub fn world_normal_map(mesh: &MeshData, sub: usize, raster: &UvRaster) -> Vec<[f32; 4]> {
    use rayon::prelude::*;
    (0..raster.tri.len())
        .into_par_iter()
        .map(|i| match surface(mesh, sub, raster, i) {
            Some(s) => [s.normal.x, s.normal.y, s.normal.z, 1.0],
            None => [0.0, 1.0, 0.0, 1.0],
        })
        .collect()
}

/// Sharp edges of the welded mesh: (a, b, convex).
fn sharp_edges(mesh: &MeshData) -> Vec<(Vec3, Vec3, bool)> {
    let key = |p: Vec3| [(p.x * 1e4) as i64, (p.y * 1e4) as i64, (p.z * 1e4) as i64];
    let mut faces: HashMap<([i64; 3], [i64; 3]), Vec<(Vec3, Vec3)>> = HashMap::new();
    for s in &mesh.submeshes {
        for t in s.indices.chunks_exact(3) {
            let p: Vec<Vec3> = t.iter().map(|&i| Vec3::from(mesh.vertices[i as usize].pos)).collect();
            let n = (p[1] - p[0]).cross(p[2] - p[0]).normalize_or_zero();
            if n == Vec3::ZERO {
                continue;
            }
            let c = (p[0] + p[1] + p[2]) / 3.0;
            for e in 0..3 {
                let (a, b) = (key(p[e]), key(p[(e + 1) % 3]));
                let k = if a < b { (a, b) } else { (b, a) };
                faces.entry(k).or_default().push((n, c));
            }
        }
    }
    let unkey = |k: [i64; 3]| Vec3::new(k[0] as f32, k[1] as f32, k[2] as f32) * 1e-4;
    let mut out = Vec::new();
    for ((a, b), f) in faces {
        if f.len() != 2 {
            continue;
        }
        let (n1, c1) = f[0];
        let (n2, c2) = f[1];
        if n1.dot(n2) > 0.85 {
            continue; // under ~32 degrees
        }
        let convex = n1.dot(c2 - c1) < 0.0;
        out.push((unkey(a), unkey(b), convex));
    }
    out
}

struct EdgeGrid {
    cell: f32,
    cells: HashMap<[i32; 3], Vec<u32>>,
    edges: Vec<(Vec3, Vec3, bool)>,
}

impl EdgeGrid {
    fn new(edges: Vec<(Vec3, Vec3, bool)>, cell: f32) -> Self {
        let mut cells: HashMap<[i32; 3], Vec<u32>> = HashMap::new();
        for (i, (a, b, _)) in edges.iter().enumerate() {
            let len = (*b - *a).length();
            let steps = (len / cell).ceil().max(1.0) as u32;
            for s in 0..=steps {
                let p = a.lerp(*b, s as f32 / steps as f32);
                let c = (p / cell).floor();
                let k = [c.x as i32, c.y as i32, c.z as i32];
                let list = cells.entry(k).or_default();
                if list.last() != Some(&(i as u32)) {
                    list.push(i as u32);
                }
            }
        }
        EdgeGrid { cell, cells, edges }
    }

    /// Signed falloff in [-1, 1] from the nearest sharp edge within `width`.
    fn query(&self, p: Vec3, width: f32) -> f32 {
        let c = (p / self.cell).floor();
        let mut best = (f32::MAX, false);
        for dz in -1..=1 {
            for dy in -1..=1 {
                for dx in -1..=1 {
                    let k = [c.x as i32 + dx, c.y as i32 + dy, c.z as i32 + dz];
                    let Some(list) = self.cells.get(&k) else { continue };
                    for &e in list {
                        let (a, b, convex) = self.edges[e as usize];
                        let ab = b - a;
                        let t = ((p - a).dot(ab) / ab.length_squared().max(1e-20)).clamp(0.0, 1.0);
                        let d = (a + ab * t - p).length();
                        if d < best.0 {
                            best = (d, convex);
                        }
                    }
                }
            }
        }
        if best.0 >= width {
            return 0.0;
        }
        let f = 1.0 - best.0 / width;
        let f = f * f;
        if best.1 {
            f
        } else {
            -f
        }
    }
}

pub struct HighPoly {
    pub mesh: MeshData,
    pub tris: Vec<[u32; 3]>,
    pub bvh: Bvh,
    /// Normalized object name key of each triangle (match by name).
    pub tri_key: Vec<u32>,
    pub keys: Vec<String>,
}

/// "Bolt_low", "bolt_HP", "Bolt.high" -> "bolt".
pub fn name_key(name: &str) -> String {
    let mut s = name.to_lowercase();
    for suffix in ["_low", "_high", "_lowpoly", "_highpoly", "_lo", "_hi", "_lp", "_hp", ".low", ".high", "-low", "-high"] {
        if let Some(t) = s.strip_suffix(suffix) {
            s = t.to_string();
            break;
        }
    }
    s
}

impl HighPoly {
    pub fn new(mesh: MeshData) -> Self {
        let tris = mesh_triangles(&mesh);
        let bvh = build_bvh(&mesh, &tris);
        let keys: Vec<String> = mesh.objects.iter().map(|o| name_key(o)).collect();
        let tri_key = mesh.submeshes.iter().flat_map(|s| s.tri_objects.iter().copied()).collect();
        HighPoly { mesh, tris, bvh, tri_key, keys }
    }

    fn normal(&self, h: &Hit) -> Vec3 {
        let t = self.tris[h.tri as usize];
        let n = |i: usize| Vec3::from(self.mesh.vertices[t[i] as usize].normal);
        (n(0) * (1.0 - h.u - h.v) + n(1) * h.u + n(2) * h.v).normalize_or(Vec3::Y)
    }
}

fn cosine_dir(k: u32, samples: u32, rot: f32, n: Vec3, t: Vec3, b: Vec3) -> Vec3 {
    let u1 = (k as f32 + 0.5) / samples as f32;
    let u2 = (radical_inverse(k) + rot).fract();
    let r = u1.sqrt();
    let phi = u2 * std::f32::consts::TAU;
    t * (r * phi.cos()) + b * (r * phi.sin()) + n * (1.0 - u1).max(0.0).sqrt()
}

/// Rows of output to bake for a set (progress total).
pub fn bake_rows(res: u32, settings: &BakeSettings) -> u32 {
    res * settings.antialias.clamp(1, 4)
}

/// Bakes one texture set at `res`. `progress` counts finished supersampled rows.
#[allow(clippy::too_many_arguments)]
pub fn bake(
    mesh: &MeshData,
    sub: usize,
    res: u32,
    low_bvh: &Bvh,
    high: Option<&HighPoly>,
    settings: &BakeSettings,
    progress: &AtomicU32,
    cancel: &AtomicBool,
    rays: Option<&dyn RayBackend>,
) -> BakeResult {
    let aa = settings.antialias.clamp(1, 4);
    let hres = (res * aa) as usize;
    let raster = rasterize_uv(mesh, sub, res * aa);
    let n = hres * hres;
    let diag = mesh.diagonal();
    let ao_dist = settings.ao_distance * diag;
    let thick_dist = settings.thickness_distance * diag;
    let cage = settings.cage_distance * diag;
    let eps = diag * 2e-4;
    let samples = settings.ao_samples.max(1);
    let tsamples = settings.thickness_samples.max(1);

    let islands = match settings.id_source {
        IdSource::UvIsland => Some(components(mesh, sub, true)),
        _ => None,
    };
    let objects = &mesh.submeshes[sub].tri_objects;
    let idx = &mesh.submeshes[sub].indices;

    // Per texel: shading position + world normal (from the high poly when present).
    let shading: Vec<Option<(Vec3, Vec3, Surface)>> = (0..n)
        .into_par_iter()
        .map(|i| {
            let s = surface(mesh, sub, &raster, i)?;
            if let Some(hp) = high {
                let o = s.pos + s.normal * cage;
                // Match by name: the low triangle's part key, when the high poly has it.
                let want = settings.match_by_name.then(|| {
                    let obj = mesh.submeshes[sub].tri_objects.get(raster.tri[i] as usize).copied().unwrap_or(0) as usize;
                    let key = name_key(mesh.objects.get(obj).map(String::as_str).unwrap_or(""));
                    hp.keys.iter().position(|k| *k == key)
                });
                let filter = |t: u32| match want {
                    Some(Some(k)) => hp.tri_key.get(t as usize).copied() == Some(k as u32),
                    _ => true,
                };
                if let Some(h) = hp.bvh.trace_where(o, -s.normal, cage * 2.0, false, Some(&filter)) {
                    return Some((o - s.normal * h.t, hp.normal(&h), s));
                }
            }
            Some((s.pos, s.normal, s))
        })
        .collect();

    let occluder = high.map(|h| &h.bvh).unwrap_or(low_bvh);

    // GPU ray pass when available: every covered texel at once.
    let gpu: Option<Vec<[f32; 2]>> = rays.and_then(|r| {
        let mut index = Vec::new();
        let mut samples_in = Vec::new();
        for (i, sh) in shading.iter().enumerate() {
            if let Some((p, nw, s)) = sh {
                let t = (s.tangent - *nw * nw.dot(s.tangent)).normalize_or(nw.any_orthonormal_vector());
                samples_in.push([p.x, p.y, p.z, f32::from_bits(i as u32), nw.x, nw.y, nw.z, 0.0, t.x, t.y, t.z, 0.0]);
                index.push(i);
            }
        }
        let rows = hres as f32;
        let done = std::sync::atomic::AtomicU32::new(0);
        let report = |f: f32| {
            let target = (f * rows) as u32;
            let prev = done.swap(target, Ordering::Relaxed);
            if target > prev {
                progress.fetch_add(target - prev, Ordering::Relaxed);
            }
        };
        let res = r.ao_thickness(&samples_in, samples, ao_dist, tsamples, thick_dist, eps, cancel, &report)?;
        let mut full = vec![[1.0f32, 1.0f32]; n];
        for (k, &i) in index.iter().enumerate() {
            full[i] = res[k];
        }
        let left = (hres as u32).saturating_sub(done.load(Ordering::Relaxed));
        progress.fetch_add(left, Ordering::Relaxed);
        Some(full)
    });
    struct Px {
        ao: f32,
        normal: [f32; 4],
        thick: f32,
        id: [f32; 4],
    }
    let empty = || Px { ao: 1.0, normal: [0.0, 0.0, 1.0, 1.0], thick: 0.5, id: [0.0, 0.0, 0.0, 1.0] };
    let rows: Vec<Vec<Px>> = (0..hres)
        .into_par_iter()
        .map(|y| {
            let mut row = Vec::with_capacity(hres);
            for x in 0..hres {
                let i = y * hres + x;
                if cancel.load(Ordering::Relaxed) {
                    row.push(empty());
                    continue;
                }
                let Some((p, nw, s)) = shading[i] else {
                    row.push(empty());
                    continue;
                };
                let tangent = (s.tangent - nw * nw.dot(s.tangent)).normalize_or(nw.any_orthonormal_vector());
                let bitangent = nw.cross(tangent);
                let (ao_v, thick_v) = match &gpu {
                    Some(g) => (g[i][0], g[i][1]),
                    None => {
                        let rot = hash(i as u32);
                        let mut open = 0u32;
                        for k in 0..samples {
                            let d = cosine_dir(k, samples, rot, nw, tangent, bitangent);
                            if occluder.trace(p + nw * eps, d, ao_dist, true).is_none() {
                                open += 1;
                            }
                        }
                        let mut tsum = 0.0;
                        for k in 0..tsamples {
                            let d = cosine_dir(k, tsamples, rot, -nw, tangent, -bitangent);
                            tsum += occluder.trace(p - nw * eps, d, thick_dist, false).map(|h| h.t).unwrap_or(thick_dist);
                        }
                        (open as f32 / samples as f32, (tsum / tsamples as f32 / thick_dist.max(1e-9)).clamp(0.0, 1.0))
                    }
                };
                let normal = if high.is_some() {
                    let ts = Vec3::new(nw.dot(s.tangent), nw.dot(s.bitangent), nw.dot(s.normal)).normalize_or(Vec3::Z);
                    [ts.x, ts.y, ts.z, 1.0]
                } else {
                    [0.0, 0.0, 1.0, 1.0]
                };
                let tri = raster.tri[i] as usize;
                let id = match settings.id_source {
                    IdSource::Object => id_color(objects.get(tri).copied().unwrap_or(0)),
                    IdSource::Material => id_color(sub as u32),
                    IdSource::UvIsland => id_color(islands.as_ref().map(|v| v[tri]).unwrap_or(0)),
                    IdSource::VertexColor => {
                        if mesh.colors.is_empty() {
                            [1.0; 3]
                        } else {
                            let [w1, w2] = raster.bary[i];
                            let w0 = 1.0 - w1 - w2;
                            let c = |k: usize| Vec3::from(mesh.colors[idx[tri * 3 + k] as usize]);
                            (c(0) * w0 + c(1) * w1 + c(2) * w2).to_array()
                        }
                    }
                };
                row.push(Px { ao: ao_v, normal, thick: thick_v, id: [id[0], id[1], id[2], 1.0] });
            }
            if gpu.is_none() {
                progress.fetch_add(1, Ordering::Relaxed);
            }
            row
        })
        .collect();
    let px: Vec<Px> = rows.into_iter().flatten().collect();

    // Curvature from the mesh (like Painter's "from mesh" curvature): per
    // welded vertex from the face normals across its edges, smoothed over
    // its neighbours, interpolated per texel; plus a falloff around sharp
    // geometric edges so hard-surface low polys read.
    let vcurv = vertex_curvature(mesh, sub);
    // Relative to this set's own curvature range: broad smooth areas stay
    // near mid gray, bevels and creases read (a scale tied to the object
    // size saturates every detail of a large model).
    let range = {
        let mut a: Vec<f32> = vcurv.value.iter().map(|k| k.abs()).filter(|k| k.is_finite()).collect();
        a.sort_by(|x, y| x.total_cmp(y));
        // Floor: a bend of radius 5% of the object reads, a whole sphere
        // stays near mid gray.
        a.get(((a.len() as f32 - 1.0) * 0.9) as usize).copied().unwrap_or(0.0).max(20.0 / diag.max(1e-9))
    };
    let width = settings.edge_width * diag;
    let grid = EdgeGrid::new(sharp_edges(mesh), width.max(diag * 1e-4) * 2.0);
    let curv_hi: Vec<f32> = (0..n)
        .into_par_iter()
        .map(|i| {
            let Some((_, _, s)) = shading[i] else { return 0.5 };
            let t = raster.tri[i] as usize;
            let [w1, w2] = raster.bary[i];
            let k = vcurv.at(t, [1.0 - w1 - w2, w1, w2]);
            let smooth = (k / range * 0.8).tanh();
            let edge = grid.query(s.pos, width);
            (0.5 + 0.5 * (smooth + edge).clamp(-1.0, 1.0)).clamp(0.0, 1.0)
        })
        .collect();

    // Ray-traced maps are denoised in UV space, edge-aware (world distance
    // and normal), so nothing bleeds across seams or folds.
    let ao_hi = denoise(&px.iter().map(|p| p.ao).collect::<Vec<_>>(), &shading, hres, 2 * aa as usize);
    let thick_hi = denoise(&px.iter().map(|p| p.thick).collect::<Vec<_>>(), &shading, hres, 2 * aa as usize);

    // Downsample the supersampled maps, averaging covered samples only.
    let res = res as usize;
    let a = aa as usize;
    let mut out = BakeResult {
        ao: vec![1.0; res * res],
        curvature: vec![0.5; res * res],
        normal: vec![[0.0, 0.0, 1.0, 1.0]; res * res],
        wnormal: vec![[0.0, 1.0, 0.0, 1.0]; res * res],
        thickness: vec![0.5; res * res],
        id: vec![[0.0, 0.0, 0.0, 1.0]; res * res],
    };
    for y in 0..res {
        for x in 0..res {
            let mut cnt = 0.0f32;
            let (mut ao, mut cu, mut th) = (0.0, 0.0, 0.0);
            let mut nn = Vec3::ZERO;
            let mut wn = Vec3::ZERO;
            let mut id = Vec3::ZERO;
            for sy in 0..a {
                for sx in 0..a {
                    let i = (y * a + sy) * hres + x * a + sx;
                    let Some((_, nw, _)) = shading[i] else { continue };
                    let p = &px[i];
                    cnt += 1.0;
                    ao += ao_hi[i];
                    cu += curv_hi[i];
                    th += thick_hi[i];
                    nn += Vec3::new(p.normal[0], p.normal[1], p.normal[2]);
                    wn += nw;
                    id += Vec3::new(p.id[0], p.id[1], p.id[2]);
                }
            }
            if cnt == 0.0 {
                continue;
            }
            let o = y * res + x;
            out.ao[o] = ao / cnt;
            out.curvature[o] = cu / cnt;
            out.thickness[o] = th / cnt;
            let nn = nn.normalize_or(Vec3::Z);
            out.normal[o] = [nn.x, nn.y, nn.z, 1.0];
            let wn = wn.normalize_or(Vec3::Y);
            out.wnormal[o] = [wn.x, wn.y, wn.z, 1.0];
            let id = id / cnt;
            out.id[o] = [id.x, id.y, id.z, 1.0];
        }
    }
    out
}

/// Mean curvature per welded vertex of a submesh (positive convex).
struct VertexCurvature {
    /// Welded vertex of each corner of the submesh's triangles.
    corner: Vec<u32>,
    value: Vec<f32>,
}

impl VertexCurvature {
    /// Curvature at barycentric `w` of triangle `t`.
    fn at(&self, t: usize, w: [f32; 3]) -> f32 {
        (0..3).map(|k| self.corner.get(t * 3 + k).map_or(0.0, |&v| self.value[v as usize]) * w[k]).sum()
    }
}

fn vertex_curvature(mesh: &MeshData, sub: usize) -> VertexCurvature {
    let idx = &mesh.submeshes[sub].indices;
    let q = (mesh.diagonal() * 1e-5).max(1e-9);
    let mut weld: HashMap<[i64; 3], u32> = HashMap::new();
    let mut pos: Vec<Vec3> = Vec::new();
    let corner: Vec<u32> = idx
        .iter()
        .map(|&v| {
            let p = Vec3::from(mesh.vertices[v as usize].pos);
            let k = [(p.x / q).round() as i64, (p.y / q).round() as i64, (p.z / q).round() as i64];
            *weld.entry(k).or_insert_with(|| {
                pos.push(p);
                pos.len() as u32 - 1
            })
        })
        .collect();
    let nv = pos.len();
    // Area-weighted face normals per welded vertex (geometric, so split
    // normals at hard edges still read as edges).
    let mut nrm = vec![Vec3::ZERO; nv];
    for t in corner.chunks_exact(3) {
        let (a, b, c) = (pos[t[0] as usize], pos[t[1] as usize], pos[t[2] as usize]);
        let f = (b - a).cross(c - a);
        for &v in t {
            nrm[v as usize] += f;
        }
    }
    let nrm: Vec<Vec3> = nrm.into_iter().map(|n| n.normalize_or_zero()).collect();
    // Faces on each edge: sharp edges (over ~32 degrees) are left to the
    // sharp edge falloff, so they do not bleed over whole low-poly faces.
    let mut edge_faces: HashMap<(u32, u32), Vec<Vec3>> = HashMap::new();
    for t in corner.chunks_exact(3) {
        let (a, b, c) = (pos[t[0] as usize], pos[t[1] as usize], pos[t[2] as usize]);
        let f = (b - a).cross(c - a).normalize_or_zero();
        for k in 0..3 {
            let (u, v) = (t[k], t[(k + 1) % 3]);
            edge_faces.entry((u.min(v), u.max(v))).or_default().push(f);
        }
    }
    let mut sum = vec![0.0f32; nv];
    let mut cnt = vec![0.0f32; nv];
    let mut nbr: Vec<Vec<u32>> = vec![Vec::new(); nv];
    let mut seen = std::collections::HashSet::new();
    for t in corner.chunks_exact(3) {
        for k in 0..3 {
            let (a, b) = (t[k], t[(k + 1) % 3]);
            if a == b || !seen.insert((a.min(b), a.max(b))) {
                continue;
            }
            let sharp = edge_faces.get(&(a.min(b), a.max(b))).is_some_and(|f| f.len() == 2 && f[0].dot(f[1]) < 0.85);
            if sharp {
                continue;
            }
            let d = pos[b as usize] - pos[a as usize];
            let l2 = d.length_squared();
            if l2 < 1e-20 {
                continue;
            }
            let kv = (nrm[b as usize] - nrm[a as usize]).dot(d) / l2;
            for v in [a, b] {
                sum[v as usize] += kv;
                cnt[v as usize] += 1.0;
            }
            nbr[a as usize].push(b);
            nbr[b as usize].push(a);
        }
    }
    let mut value: Vec<f32> = (0..nv).map(|v| if cnt[v] > 0.0 { sum[v] / cnt[v] } else { 0.0 }).collect();
    // Two smoothing passes over the one-ring.
    for _ in 0..2 {
        value = (0..nv)
            .map(|v| {
                let n = &nbr[v];
                if n.is_empty() {
                    return value[v];
                }
                let m = n.iter().map(|&u| value[u as usize]).sum::<f32>() / n.len() as f32;
                value[v] * 0.5 + m * 0.5
            })
            .collect();
    }
    VertexCurvature { corner, value }
}

/// Edge-aware blur of a texel map (covered texels only): neighbours within
/// `radius` texels weigh by world distance and normal agreement.
fn denoise(v: &[f32], shading: &[Option<(Vec3, Vec3, Surface)>], res: usize, radius: usize) -> Vec<f32> {
    let r = radius.max(1) as i64;
    (0..v.len())
        .into_par_iter()
        .map(|i| {
            let Some((p, n, _)) = shading[i] else { return v[i] };
            let (x, y) = ((i % res) as i64, (i / res) as i64);
            // World size of a texel here, from the nearest covered neighbour.
            let mut texel = f32::MAX;
            for (dx, dy) in [(1i64, 0i64), (-1, 0), (0, 1), (0, -1)] {
                let (sx, sy) = (x + dx, y + dy);
                if sx >= 0 && sy >= 0 && sx < res as i64 && sy < res as i64 {
                    if let Some((q, _, _)) = shading[sy as usize * res + sx as usize] {
                        texel = texel.min(q.distance(p));
                    }
                }
            }
            if texel == f32::MAX || texel <= 0.0 {
                return v[i];
            }
            let sigma = texel * r as f32 * 0.75;
            let inv = 1.0 / (2.0 * sigma * sigma);
            let (mut acc, mut wsum) = (0.0f32, 0.0f32);
            for dy in -r..=r {
                for dx in -r..=r {
                    let (sx, sy) = (x + dx, y + dy);
                    if sx < 0 || sy < 0 || sx >= res as i64 || sy >= res as i64 {
                        continue;
                    }
                    let j = sy as usize * res + sx as usize;
                    let Some((q, m, _)) = shading[j] else { continue };
                    let nd = n.dot(m).max(0.0);
                    let w = (-(q - p).length_squared() * inv).exp() * nd * nd * nd * nd;
                    acc += v[j] * w;
                    wsum += w;
                }
            }
            if wsum > 0.0 {
                acc / wsum
            } else {
                v[i]
            }
        })
        .collect()
}
