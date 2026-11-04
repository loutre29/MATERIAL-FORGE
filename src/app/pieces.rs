//! Muse's pieces: each part of the model split into its connected mesh
//! pieces (welded by position; a part made of a single piece splits into
//! its UV islands instead), alike pieces grouped (chain links, rivets), so
//! Muse gives materials to exact polygons instead of position cuts. Also the
//! texture-space masks of pieces.

use crate::mesh::MeshData;
use glam::{Vec2, Vec3};
use std::collections::HashMap;

/// At most this many pieces (the view colors stay apart); past it the
/// smallest of a part become one "small pieces" group.
pub const MAX_PIECES: usize = 36;

/// A group of alike mesh pieces of one part: what Muse calls piece N.
#[derive(Clone)]
pub struct Piece {
    pub set: usize,
    pub sub: usize,
    /// The part it belongs to (its object or texture set name).
    pub part: String,
    pub tris: Vec<u32>,
    /// Mesh pieces in the group.
    pub count: usize,
    pub area: f32,
    pub lo: Vec3,
    pub hi: Vec3,
}

struct Dsu(Vec<u32>);

impl Dsu {
    fn find(&mut self, mut a: u32) -> u32 {
        while self.0[a as usize] != a {
            let up = self.0[self.0[a as usize] as usize];
            self.0[a as usize] = up;
            a = up;
        }
        a
    }

    fn union(&mut self, a: u32, b: u32) {
        let (a, b) = (self.find(a), self.find(b));
        if a != b {
            self.0[a as usize] = b;
        }
    }
}

/// The connected triangle sets of `tris`: triangles sharing a welded vertex
/// (by position, and by UV too when `uv`: the UV islands).
pub fn components(mesh: &MeshData, sub: usize, tris: &[u32], uv: bool) -> Vec<Vec<u32>> {
    let s = &mesh.submeshes[sub];
    let q = (mesh.diagonal() * 1e-5).max(1e-9);
    let mut dsu = Dsu((0..tris.len() as u32).collect());
    let mut seen: HashMap<[i64; 5], u32> = HashMap::new();
    for (i, &t) in tris.iter().enumerate() {
        for k in 0..3 {
            let v = &mesh.vertices[s.indices[t as usize * 3 + k] as usize];
            let p = Vec3::from(v.pos) / q;
            let w = if uv { [(v.uv[0] * 1e5).round() as i64, (v.uv[1] * 1e5).round() as i64] } else { [0, 0] };
            let key = [p.x.round() as i64, p.y.round() as i64, p.z.round() as i64, w[0], w[1]];
            match seen.get(&key) {
                Some(&j) => dsu.union(i as u32, j),
                None => {
                    seen.insert(key, i as u32);
                }
            }
        }
    }
    let mut groups: HashMap<u32, Vec<u32>> = HashMap::new();
    for (i, &t) in tris.iter().enumerate() {
        groups.entry(dsu.find(i as u32)).or_default().push(t);
    }
    let mut out: Vec<Vec<u32>> = groups.into_values().collect();
    out.sort_by_key(|g| g[0]);
    out
}

/// (area, bounding box) of triangles.
fn measure(mesh: &MeshData, sub: usize, tris: &[u32]) -> (f32, Vec3, Vec3) {
    let idx = &mesh.submeshes[sub].indices;
    let (mut area, mut lo, mut hi) = (0.0f32, Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    for &t in tris {
        let v = |k: usize| Vec3::from(mesh.vertices[idx[t as usize * 3 + k] as usize].pos);
        let (a, b, c) = (v(0), v(1), v(2));
        area += (b - a).cross(c - a).length() * 0.5;
        lo = lo.min(a).min(b).min(c);
        hi = hi.max(a).max(b).max(c);
    }
    (area, lo, hi)
}

/// The pieces of `parts`: (texture set, submesh, object or the whole
/// submesh, part name). Numbered part by part, largest first.
pub fn find(mesh: &MeshData, parts: &[(usize, usize, Option<u32>, String)]) -> Vec<Piece> {
    // Alike pieces: same area and box size within about 15%.
    let bucket = |x: f32| (x.max(1e-12).ln() / 1.15f32.ln()).round() as i64;
    let mut per_part: Vec<Vec<Piece>> = Vec::new();
    for (set, sub, object, name) in parts {
        let s = &mesh.submeshes[*sub];
        let ntri = (s.indices.len() / 3) as u32;
        let tris: Vec<u32> = (0..ntri).filter(|&t| object.map_or(true, |o| s.tri_objects.get(t as usize).copied().unwrap_or(0) == o)).collect();
        if tris.is_empty() {
            per_part.push(Vec::new());
            continue;
        }
        let mut comps = components(mesh, *sub, &tris, false);
        if comps.len() == 1 {
            let islands = components(mesh, *sub, &tris, true);
            if islands.len() > 1 {
                comps = islands;
            }
        }
        let mut groups: Vec<Piece> = Vec::new();
        let mut by_sig: HashMap<[i64; 4], usize> = HashMap::new();
        for c in comps {
            let (area, lo, hi) = measure(mesh, *sub, &c);
            let mut e = (hi - lo).to_array();
            e.sort_by(|a, b| a.total_cmp(b));
            let sig = [bucket(area), bucket(e[0] + 1e-9), bucket(e[1] + 1e-9), bucket(e[2] + 1e-9)];
            match by_sig.get(&sig) {
                Some(&g) => {
                    let p = &mut groups[g];
                    p.tris.extend(c);
                    p.count += 1;
                    p.area += area;
                    p.lo = p.lo.min(lo);
                    p.hi = p.hi.max(hi);
                }
                None => {
                    by_sig.insert(sig, groups.len());
                    groups.push(Piece { set: *set, sub: *sub, part: name.clone(), tris: c, count: 1, area, lo, hi });
                }
            }
        }
        groups.sort_by(|a, b| b.area.total_cmp(&a.area));
        per_part.push(groups);
    }
    // Over the limit: the smallest groups of a part merge into one.
    let total: usize = per_part.iter().map(|g| g.len()).sum();
    if total > MAX_PIECES {
        let mut areas: Vec<f32> = per_part.iter().flatten().map(|p| p.area).collect();
        areas.sort_by(|a, b| b.total_cmp(a));
        // Room for one merged group per part that overflows.
        let mut keep = MAX_PIECES - per_part.len().min(MAX_PIECES / 2);
        loop {
            let cut = areas[keep.min(areas.len()) - 1];
            let over = per_part.iter().filter(|g| g.iter().any(|p| p.area < cut)).count();
            if keep + over <= MAX_PIECES || keep <= 1 {
                break;
            }
            keep -= 1;
        }
        let cut = areas[keep - 1];
        for g in per_part.iter_mut() {
            let small: Vec<Piece> = g.iter().filter(|p| p.area < cut).cloned().collect();
            if small.len() < 2 {
                continue;
            }
            g.retain(|p| p.area >= cut);
            let mut m = small[0].clone();
            for p in &small[1..] {
                m.tris.extend_from_slice(&p.tris);
                m.count += p.count;
                m.area += p.area;
                m.lo = m.lo.min(p.lo);
                m.hi = m.hi.max(p.hi);
            }
            g.push(m);
        }
    }
    per_part.into_iter().flatten().collect()
}

/// Which piece covers each texel of a texture set (0: none, i + 1: piece
/// i), from the pieces' UVs, grown a few texels into empty space so seams
/// stay covered.
pub fn owners(mesh: &MeshData, pieces: &[Piece], set: usize, res: u32) -> Vec<u16> {
    let n = res as usize;
    let mut own = vec![0u16; n * n];
    let r = res as f32;
    for (i, p) in pieces.iter().enumerate().filter(|(_, p)| p.set == set) {
        let idx = &mesh.submeshes[p.sub].indices;
        for &t in &p.tris {
            let uv = |k: usize| Vec2::from(mesh.vertices[idx[t as usize * 3 + k] as usize].uv) * r;
            let (a, b, c) = (uv(0), uv(1), uv(2));
            let area = (b - a).perp_dot(c - a);
            if area.abs() < 1e-12 {
                continue;
            }
            let x0 = a.x.min(b.x).min(c.x).floor().max(0.0) as usize;
            let x1 = (a.x.max(b.x).max(c.x).ceil() as usize).min(n);
            let y0 = a.y.min(b.y).min(c.y).floor().max(0.0) as usize;
            let y1 = (a.y.max(b.y).max(c.y).ceil() as usize).min(n);
            for y in y0..y1 {
                for x in x0..x1 {
                    let q = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
                    let w0 = (b - q).perp_dot(c - q) / area;
                    let w1 = (c - q).perp_dot(a - q) / area;
                    if w0 >= 0.0 && w1 >= 0.0 && w0 + w1 <= 1.0 {
                        own[y * n + x] = i as u16 + 1;
                    }
                }
            }
        }
    }
    for _ in 0..4 {
        let prev = own.clone();
        for y in 0..n {
            for x in 0..n {
                if prev[y * n + x] != 0 {
                    continue;
                }
                let near = [(x > 0).then(|| y * n + x - 1), (x + 1 < n).then(|| y * n + x + 1), (y > 0).then(|| (y - 1) * n + x), (y + 1 < n).then(|| (y + 1) * n + x)];
                if let Some(o) = near.into_iter().flatten().map(|j| prev[j]).find(|&o| o != 0) {
                    own[y * n + x] = o;
                }
            }
        }
    }
    own
}

/// A paint mask (value, coverage per texel) white on the pieces in `keep`.
pub fn mask(own: &[u16], keep: &[usize]) -> Vec<f32> {
    let mut on = vec![false; keep.iter().max().map_or(1, |m| m + 2)];
    for &k in keep {
        on[k + 1] = true;
    }
    own.iter().flat_map(|&o| [if on.get(o as usize).copied().unwrap_or(false) { 1.0 } else { 0.0 }, 1.0]).collect()
}
