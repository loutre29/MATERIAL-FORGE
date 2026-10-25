//! Flood fill over a texture set's texels (magic wand, paint bucket). The
//! grid is the set's UV raster; texels are neighbors in UV space, and UV
//! seams are stitched with links between the texels on both sides of each
//! mesh edge whose two triangles have different UVs, so a region that is
//! continuous on the surface floods as one, like it looks in 3D.

use crate::bake::UvRaster;
use crate::mesh::MeshData;
use std::collections::HashMap;

/// Texel links across the UV seams of one submesh (both directions).
pub struct Seams {
    links: HashMap<u32, Vec<u32>>,
}

impl Seams {
    /// No links: floods stop at UV seams.
    #[allow(dead_code)]
    pub fn none() -> Self {
        Self { links: HashMap::new() }
    }

    pub fn link_count(&self) -> usize {
        self.links.values().map(|v| v.len()).sum()
    }

    pub fn build(mesh: &MeshData, sub: usize, raster: &UvRaster) -> Self {
        let res = raster.res;
        let idx = &mesh.submeshes[sub].indices;
        let ext = (mesh.bbox_max - mesh.bbox_min).max_element().max(1e-6);
        let q = |p: [f32; 3]| -> [i64; 3] { [0, 1, 2].map(|k| ((p[k] - mesh.bbox_min[k]) / ext * 1e6).round() as i64) };
        // Edge (by welded positions) -> the UVs of its ends on each triangle.
        let mut edges: HashMap<([i64; 3], [i64; 3]), Vec<([f32; 2], [f32; 2], u32)>> = HashMap::new();
        for t in 0..idx.len() / 3 {
            for k in 0..3 {
                let (a, b) = (&mesh.vertices[idx[t * 3 + k] as usize], &mesh.vertices[idx[t * 3 + (k + 1) % 3] as usize]);
                let (qa, qb) = (q(a.pos), q(b.pos));
                let e = if qa <= qb { ((qa, qb), (a.uv, b.uv)) } else { ((qb, qa), (b.uv, a.uv)) };
                edges.entry(e.0).or_default().push((e.1 .0, e.1 .1, t as u32));
            }
        }
        let mut links: HashMap<u32, Vec<u32>> = HashMap::new();
        let r = res as f32;
        // The covered texel of triangle `tri` nearest to `uv` (3 x 3 search).
        let texel = |uv: [f32; 2], tri: u32| -> Option<u32> {
            let (cx, cy) = ((uv[0] * r).floor() as i64, (uv[1] * r).floor() as i64);
            let mut best: Option<(f32, u32)> = None;
            for dy in -1..=1i64 {
                for dx in -1..=1i64 {
                    let (x, y) = (cx + dx, cy + dy);
                    if x < 0 || y < 0 || x >= res as i64 || y >= res as i64 {
                        continue;
                    }
                    let i = (y as u32 * res + x as u32) as usize;
                    if raster.tri[i] < 0 {
                        continue;
                    }
                    // The same triangle wins over a neighbor one.
                    let d = ((x as f32 + 0.5) / r - uv[0]).powi(2) + ((y as f32 + 0.5) / r - uv[1]).powi(2) + if raster.tri[i] as u32 == tri { 0.0 } else { 1.0 };
                    if best.is_none_or(|b| d < b.0) {
                        best = Some((d, i as u32));
                    }
                }
            }
            best.map(|b| b.1)
        };
        for sides in edges.values() {
            for i in 0..sides.len() {
                for j in i + 1..sides.len() {
                    let (a, b) = (&sides[i], &sides[j]);
                    let same = |u: [f32; 2], v: [f32; 2]| (u[0] - v[0]).abs() < 1e-6 && (u[1] - v[1]).abs() < 1e-6;
                    if same(a.0, b.0) && same(a.1, b.1) {
                        continue;
                    }
                    let len = |s: &([f32; 2], [f32; 2], u32)| ((s.1[0] - s.0[0]).powi(2) + (s.1[1] - s.0[1]).powi(2)).sqrt() * r;
                    let n = (len(a).max(len(b)) * 2.0).ceil().clamp(1.0, 8192.0) as usize;
                    for s in 0..=n {
                        let t = s as f32 / n as f32;
                        let lerp = |p: [f32; 2], q: [f32; 2]| [p[0] + (q[0] - p[0]) * t, p[1] + (q[1] - p[1]) * t];
                        if let (Some(x), Some(y)) = (texel(lerp(a.0, a.1), a.2), texel(lerp(b.0, b.1), b.2)) {
                            if x != y {
                                links.entry(x).or_default().push(y);
                                links.entry(y).or_default().push(x);
                            }
                        }
                    }
                }
            }
        }
        for v in links.values_mut() {
            v.sort_unstable();
            v.dedup();
        }
        Self { links }
    }
}

/// Texels similar to the seed: 255 selected, 0 not. `similar(i)` tells
/// whether texel `i` matches; `contiguous` floods from `seed` through UV
/// neighbors and seam links, otherwise every matching texel is taken.
pub fn flood(raster: &UvRaster, seams: &Seams, seed: usize, contiguous: bool, similar: &(dyn Fn(usize) -> bool + Sync)) -> Vec<u8> {
    let res = raster.res as usize;
    let n = res * res;
    let mut out = vec![0u8; n];
    if !contiguous {
        use rayon::prelude::*;
        out.par_iter_mut().enumerate().for_each(|(i, o)| {
            if raster.tri[i] >= 0 && similar(i) {
                *o = 255;
            }
        });
        return out;
    }
    if seed >= n || raster.tri[seed] < 0 {
        return out;
    }
    let mut stack = vec![seed as u32];
    out[seed] = 255;
    while let Some(i) = stack.pop() {
        let i = i as usize;
        let (x, y) = (i % res, i / res);
        let mut visit = |j: usize, out: &mut Vec<u8>| {
            if out[j] == 0 && raster.tri[j] >= 0 && similar(j) {
                out[j] = 255;
                stack.push(j as u32);
            }
        };
        if x > 0 {
            visit(i - 1, &mut out);
        }
        if x + 1 < res {
            visit(i + 1, &mut out);
        }
        if y > 0 {
            visit(i - res, &mut out);
        }
        if y + 1 < res {
            visit(i + res, &mut out);
        }
        if let Some(l) = seams.links.get(&(i as u32)) {
            for &j in l {
                visit(j as usize, &mut out);
            }
        }
    }
    out
}

/// 8-bit sRGB of a linear value.
pub fn to_srgb8(v: f32) -> u8 {
    let v = v.clamp(0.0, 1.0);
    let s = if v <= 0.0031308 { v * 12.92 } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 };
    (s * 255.0 + 0.5) as u8
}

/// Distance (texels, chamfer 1 / 1.4 through UV neighbors, 1 through seam
/// links) from every covered texel to the nearest `source` texel, up to
/// `max` (beyond: f32::MAX). Uncovered texels stay at f32::MAX.
pub fn distance(raster: &UvRaster, seams: &Seams, source: &[bool], max: f32) -> Vec<f32> {
    let res = raster.res as usize;
    let n = res * res;
    let mut dist = vec![f32::MAX; n];
    // Dial's buckets in tenths of a texel.
    let buckets = (max * 10.0).ceil() as usize + 16;
    let mut queue: Vec<Vec<u32>> = vec![Vec::new(); buckets];
    for (i, s) in source.iter().enumerate() {
        if *s && raster.tri[i] >= 0 {
            dist[i] = 0.0;
            queue[0].push(i as u32);
        }
    }
    for b in 0..buckets {
        while let Some(i) = queue[b].pop() {
            let i = i as usize;
            let d = dist[i];
            if (d * 10.0).round() as usize != b {
                continue;
            }
            let (x, y) = ((i % res) as i64, (i / res) as i64);
            let relax = |j: usize, w: f32, dist: &mut Vec<f32>, queue: &mut Vec<Vec<u32>>| {
                let nd = d + w;
                if nd <= max && raster.tri[j] >= 0 && nd < dist[j] {
                    dist[j] = nd;
                    let k = (nd * 10.0).round() as usize;
                    if k < buckets {
                        queue[k].push(j as u32);
                    }
                }
            };
            for (dx, dy, w) in [(-1i64, 0i64, 1.0f32), (1, 0, 1.0), (0, -1, 1.0), (0, 1, 1.0), (-1, -1, 1.4), (1, -1, 1.4), (-1, 1, 1.4), (1, 1, 1.4)] {
                let (xx, yy) = (x + dx, y + dy);
                if xx >= 0 && yy >= 0 && xx < res as i64 && yy < res as i64 {
                    relax(yy as usize * res + xx as usize, w, &mut dist, &mut queue);
                }
            }
            if let Some(l) = seams.links.get(&(i as u32)) {
                for &j in l {
                    relax(j as usize, 1.0, &mut dist, &mut queue);
                }
            }
        }
    }
    dist
}

/// Grow / shrink / feather / border of a selection (r8 bytes, set sized).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Modify {
    Grow,
    Shrink,
    Feather,
    Border,
}

impl Modify {
    pub fn name(self) -> &'static str {
        match self {
            Modify::Grow => "Grow",
            Modify::Shrink => "Shrink",
            Modify::Feather => "Feather",
            Modify::Border => "Border",
        }
    }
}

pub fn modify(raster: &UvRaster, seams: &Seams, sel: &[u8], op: Modify, radius: f32) -> Vec<u8> {
    let r = radius.max(0.5);
    let inside: Vec<bool> = sel.iter().map(|v| *v >= 128).collect();
    let outside: Vec<bool> = inside.iter().enumerate().map(|(i, s)| !s && raster.tri[i] >= 0).collect();
    let covered = |i: usize| raster.tri[i] >= 0;
    match op {
        Modify::Grow => {
            let d = distance(raster, seams, &inside, r);
            d.iter().enumerate().map(|(i, d)| if covered(i) && *d <= r { 255 } else { 0 }).collect()
        }
        Modify::Shrink => {
            let d = distance(raster, seams, &outside, r);
            d.iter().enumerate().map(|(i, d)| if inside[i] && *d > r { 255 } else { 0 }).collect()
        }
        Modify::Feather | Modify::Border => {
            let to_out = distance(raster, seams, &outside, r);
            let to_in = distance(raster, seams, &inside, r);
            (0..sel.len())
                .map(|i| {
                    if !covered(i) {
                        return 0;
                    }
                    // Signed distance to the edge, in texels (positive inside).
                    let s = if inside[i] { to_out[i].min(r) - 0.5 } else { -(to_in[i].min(r) - 0.5) };
                    let v = if op == Modify::Feather {
                        (0.5 + 0.5 * (s / r).clamp(-1.0, 1.0)).clamp(0.0, 1.0)
                    } else if s.abs() <= r * 0.5 {
                        1.0
                    } else {
                        0.0
                    };
                    (v * 255.0 + 0.5) as u8
                })
                .collect()
        }
    }
}
