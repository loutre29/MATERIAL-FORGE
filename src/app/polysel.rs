//! Polygon fill selections: polygon topology of a submesh (vertices welded
//! by position, so loops cross UV seams), quad loops, partial loops grown
//! into patches, and the rectangle (marquee) hit test.

use crate::mesh::MeshData;
use glam::{Vec2, Vec3};
use std::collections::{HashMap, HashSet};

/// Polygons of one submesh.
pub struct PolyTopo {
    /// Welded vertex ring of each polygon, in winding order.
    pub rings: Vec<Vec<u32>>,
    /// Triangles of each polygon.
    pub tris: Vec<Vec<u32>>,
    /// Polygon of each triangle.
    pub tri_poly: Vec<u32>,
    /// Polygons on each welded edge (sorted vertex pair).
    edges: HashMap<(u32, u32), Vec<u32>>,
    /// Welded vertex positions.
    pub pos: Vec<Vec3>,
}

fn key(a: u32, b: u32) -> (u32, u32) {
    (a.min(b), a.max(b))
}

impl PolyTopo {
    pub fn build(mesh: &MeshData, sub: usize) -> PolyTopo {
        let s = &mesh.submeshes[sub];
        let ntri = s.indices.len() / 3;
        // Weld by quantized position.
        let q = (mesh.diagonal() * 1e-5).max(1e-9);
        let mut weld: HashMap<[i64; 3], u32> = HashMap::new();
        let mut remap: HashMap<u32, u32> = HashMap::new();
        let mut pos = Vec::new();
        let mut w = |v: u32| -> u32 {
            if let Some(&x) = remap.get(&v) {
                return x;
            }
            let p = Vec3::from(mesh.vertices[v as usize].pos);
            let k = [(p.x / q).round() as i64, (p.y / q).round() as i64, (p.z / q).round() as i64];
            let id = *weld.entry(k).or_insert_with(|| {
                pos.push(p);
                pos.len() as u32 - 1
            });
            remap.insert(v, id);
            id
        };
        let tri_poly: Vec<u32> = if s.tri_polys.len() == ntri { s.tri_polys.clone() } else { (0..ntri as u32).collect() };
        let npoly = tri_poly.iter().copied().max().map_or(0, |m| m as usize + 1);
        let mut tris = vec![Vec::new(); npoly];
        for (t, p) in tri_poly.iter().enumerate() {
            tris[*p as usize].push(t as u32);
        }
        let welded: Vec<[u32; 3]> = (0..ntri).map(|t| [w(s.indices[t * 3]), w(s.indices[t * 3 + 1]), w(s.indices[t * 3 + 2])]).collect();
        let mut rings = Vec::with_capacity(npoly);
        let mut edges: HashMap<(u32, u32), Vec<u32>> = HashMap::new();
        for (p, ts) in tris.iter().enumerate() {
            // Boundary edges: directed edges whose reverse is not in the polygon.
            let mut directed: Vec<(u32, u32)> = Vec::new();
            for &t in ts {
                let v = welded[t as usize];
                for k in 0..3 {
                    directed.push((v[k], v[(k + 1) % 3]));
                }
            }
            let set: HashSet<(u32, u32)> = directed.iter().copied().collect();
            let boundary: Vec<(u32, u32)> = directed.iter().copied().filter(|(a, b)| a != b && !set.contains(&(*b, *a))).collect();
            let next: HashMap<u32, u32> = boundary.iter().copied().collect();
            let mut ring = Vec::new();
            if let Some(&(start, _)) = boundary.first() {
                let mut v = start;
                for _ in 0..boundary.len() {
                    ring.push(v);
                    match next.get(&v) {
                        Some(&n) if n != start => v = n,
                        _ => break,
                    }
                }
            }
            if ring.len() < 3 {
                ring = ts.iter().flat_map(|&t| welded[t as usize]).collect();
                ring.dedup();
            }
            for i in 0..ring.len() {
                let e = key(ring[i], ring[(i + 1) % ring.len()]);
                let list = edges.entry(e).or_default();
                if !list.contains(&(p as u32)) {
                    list.push(p as u32);
                }
            }
            rings.push(ring);
        }
        PolyTopo { rings, tris, tri_poly, edges, pos }
    }

    pub fn is_quad(&self, p: u32) -> bool {
        self.rings.get(p as usize).is_some_and(|r| r.len() == 4)
    }

    /// Edge `k` of polygon `p` as (a, b) in ring order.
    pub fn edge(&self, p: u32, k: usize) -> (u32, u32) {
        let r = &self.rings[p as usize];
        (r[k % r.len()], r[(k + 1) % r.len()])
    }

    /// The other polygon on edge (a, b).
    fn across(&self, p: u32, a: u32, b: u32) -> Option<u32> {
        self.edges.get(&key(a, b))?.iter().copied().find(|&o| o != p)
    }

    /// Index of edge (a, b) (either direction) in quad `p`.
    fn edge_index(&self, p: u32, a: u32, b: u32) -> Option<usize> {
        let r = &self.rings[p as usize];
        (0..r.len()).find(|&i| key(r[i], r[(i + 1) % r.len()]) == key(a, b))
    }

    /// Quads met walking out of `p` through its edge `k`, then straight on
    /// (through the opposite edge of each quad), at most `n`. Each step
    /// gives the quad and the index of the edge it was entered by.
    pub fn walk(&self, p: u32, k: usize, n: usize) -> Vec<(u32, usize)> {
        let mut out = Vec::new();
        let (mut a, mut b) = self.edge(p, k);
        let mut cur = p;
        let mut seen = HashSet::from([p]);
        while out.len() < n {
            let Some(next) = self.across(cur, a, b) else { break };
            if !self.is_quad(next) || !seen.insert(next) {
                break;
            }
            let Some(e) = self.edge_index(next, a, b) else { break };
            out.push((next, e));
            (a, b) = self.edge(next, e + 2);
            cur = next;
        }
        out
    }

    /// The quad loop through `p` crossing its edge `k` (and the opposite one).
    pub fn quad_loop(&self, p: u32, k: usize) -> Vec<u32> {
        if !self.is_quad(p) {
            return vec![p];
        }
        let mut out = vec![p];
        out.extend(self.walk(p, k, 100_000).into_iter().map(|(q, _)| q));
        let seen: HashSet<u32> = out.iter().copied().collect();
        out.extend(self.walk(p, k + 2, 100_000).into_iter().map(|(q, _)| q).filter(|q| !seen.contains(q)));
        out
    }

    /// Center of polygon `p`.
    pub fn center(&self, p: u32) -> Vec3 {
        let r = &self.rings[p as usize];
        r.iter().map(|&v| self.pos[v as usize]).sum::<Vec3>() / r.len().max(1) as f32
    }

    /// Edge of `p` whose midpoint is nearest to `cursor` on screen.
    pub fn nearest_edge(&self, p: u32, screen: &dyn Fn(Vec3) -> Option<Vec2>, cursor: Vec2) -> usize {
        let r = &self.rings[p as usize];
        (0..r.len())
            .min_by(|&i, &j| {
                let d = |k: usize| {
                    let (a, b) = self.edge(p, k);
                    screen((self.pos[a as usize] + self.pos[b as usize]) * 0.5).map_or(f32::MAX, |s| s.distance(cursor))
                };
                d(i).total_cmp(&d(j))
            })
            .unwrap_or(0)
    }

    /// Edge of `p` most parallel to `dir` (world space).
    pub fn parallel_edge(&self, p: u32, dir: Vec3) -> usize {
        let r = &self.rings[p as usize];
        (0..r.len())
            .max_by(|&i, &j| {
                let d = |k: usize| {
                    let (a, b) = self.edge(p, k);
                    (self.pos[b as usize] - self.pos[a as usize]).normalize_or_zero().dot(dir).abs()
                };
                d(i).total_cmp(&d(j))
            })
            .unwrap_or(0)
    }

    pub fn edge_dir(&self, p: u32, k: usize) -> Vec3 {
        let (a, b) = self.edge(p, k);
        (self.pos[b as usize] - self.pos[a as usize]).normalize_or_zero()
    }

    /// Partial loop from quad `s` out through its edge `k`, up to `target`:
    /// the strip between them, grown sideways (through edge `k + 1` or
    /// `k + 3`, `side`) into a patch unless `primary_only`. None when
    /// `target` is not in reach.
    pub fn patch(&self, s: u32, k: usize, side: usize, target: u32, primary_only: bool) -> Option<Vec<u32>> {
        if s == target {
            return Some(vec![s]);
        }
        // Primary strip with the side edge of each cell: cell s left
        // through edge k, so its "entry" is k + 2; the side edge is k + 1
        // or k + 3 (it shares a vertex with the exit edge).
        let mut cells: Vec<(u32, usize)> = vec![(s, (k + side) % 4)];
        let side_vertex = |p: u32, side_edge: usize, exit_edge: usize| {
            let (a, b) = self.edge(p, side_edge);
            let (c, d) = self.edge(p, exit_edge);
            if a == c || a == d {
                a
            } else {
                b
            }
        };
        let mut sv = side_vertex(s, (k + side) % 4, k);
        for (q, e) in self.walk(s, k, 512) {
            // Side edge of q: the edge (other than the entry) holding sv.
            let se = [(e + 1) % 4, (e + 3) % 4].into_iter().find(|&i| {
                let (a, b) = self.edge(q, i);
                a == sv || b == sv
            })?;
            cells.push((q, se));
            let exit = (e + 2) % 4;
            let (a, b) = self.edge(q, se);
            let other = if a == sv { b } else { a };
            // The side vertex moves to the exit edge.
            let (c, d) = self.edge(q, exit);
            sv = if other == c || other == d { other } else { sv };
        }
        let primary: Vec<u32> = cells.iter().map(|c| c.0).collect();
        if let Some(i) = primary.iter().position(|&q| q == target) {
            return Some(primary[..=i].to_vec());
        }
        if primary_only {
            return None;
        }
        for (i, &(q, se)) in cells.iter().enumerate() {
            let side_walk = self.walk(q, se, 256);
            if let Some(j) = side_walk.iter().position(|(x, _)| *x == target) {
                // Rectangle of cells 0..=i along the strip, 0..=j sideways.
                let mut out = Vec::new();
                for &(c, cse) in &cells[..=i] {
                    out.push(c);
                    out.extend(self.walk(c, cse, j + 1).into_iter().map(|(x, _)| x));
                }
                return Some(out);
            }
        }
        None
    }
}

/// Even-odd point in polygon test.
pub fn point_in_polygon(p: Vec2, poly: &[Vec2]) -> bool {
    let mut inside = false;
    let mut j = poly.len() - 1;
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[j]);
        if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// Screen-space triangle vs closed polygon (lasso) overlap; `bounds` is the
/// polygon's bounding box (min, max).
pub fn tri_polygon_overlap(t: [Vec2; 3], poly: &[Vec2], bounds: [Vec2; 2]) -> bool {
    let tlo = t[0].min(t[1]).min(t[2]);
    let thi = t[0].max(t[1]).max(t[2]);
    if thi.x < bounds[0].x || tlo.x > bounds[1].x || thi.y < bounds[0].y || tlo.y > bounds[1].y {
        return false;
    }
    if t.iter().any(|p| point_in_polygon(*p, poly)) {
        return true;
    }
    let sign = |a: Vec2, b: Vec2, c: Vec2| (b - a).perp_dot(c - a);
    let in_tri = |p: Vec2| {
        let (d1, d2, d3) = (sign(t[0], t[1], p), sign(t[1], t[2], p), sign(t[2], t[0], p));
        !((d1 < 0.0 || d2 < 0.0 || d3 < 0.0) && (d1 > 0.0 || d2 > 0.0 || d3 > 0.0))
    };
    if poly.iter().any(|p| in_tri(*p)) {
        return true;
    }
    let seg = |a: Vec2, b: Vec2, c: Vec2, d: Vec2| {
        let (d1, d2) = (sign(a, b, c), sign(a, b, d));
        let (d3, d4) = (sign(c, d, a), sign(c, d, b));
        d1 * d2 < 0.0 && d3 * d4 < 0.0
    };
    let n = poly.len();
    (0..3).any(|i| (0..n).any(|j| seg(t[i], t[(i + 1) % 3], poly[j], poly[(j + 1) % n])))
}

/// Screen-space triangle vs rectangle overlap (`r` = min, max).
pub fn tri_rect_overlap(t: [Vec2; 3], r: [Vec2; 2]) -> bool {
    let inside = |p: Vec2| p.x >= r[0].x && p.x <= r[1].x && p.y >= r[0].y && p.y <= r[1].y;
    if t.iter().any(|p| inside(*p)) {
        return true;
    }
    let corners = [r[0], Vec2::new(r[1].x, r[0].y), r[1], Vec2::new(r[0].x, r[1].y)];
    let sign = |a: Vec2, b: Vec2, c: Vec2| (b - a).perp_dot(c - a);
    let in_tri = |p: Vec2| {
        let (d1, d2, d3) = (sign(t[0], t[1], p), sign(t[1], t[2], p), sign(t[2], t[0], p));
        !((d1 < 0.0 || d2 < 0.0 || d3 < 0.0) && (d1 > 0.0 || d2 > 0.0 || d3 > 0.0))
    };
    if corners.iter().any(|c| in_tri(*c)) {
        return true;
    }
    let seg = |a: Vec2, b: Vec2, c: Vec2, d: Vec2| {
        let (d1, d2) = (sign(a, b, c), sign(a, b, d));
        let (d3, d4) = (sign(c, d, a), sign(c, d, b));
        d1 * d2 < 0.0 && d3 * d4 < 0.0
    };
    (0..3).any(|i| (0..4).any(|j| seg(t[i], t[(i + 1) % 3], corners[j], corners[(j + 1) % 4])))
}
