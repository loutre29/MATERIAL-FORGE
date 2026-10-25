//! Smart auto UV: few islands, seams where they show least.
//!
//! 1. Corners are welded by position. Seams start at the hard edges (sharper
//!    than the hard edge angle, or normals split in the file), at the
//!    borders between parts and at open borders. Quads and n-gons are never
//!    cut. Seams that end in the middle of the surface (slits) are taken
//!    back.
//! 2. Every chart becomes a topological disk so it can be flattened in one
//!    piece: a tube (several borders: arms, straps, fingers) gets one cut
//!    joining its borders, a closed part (a head, a ball) a cut between its
//!    two most distant points. Cuts take the cheapest path, where an edge
//!    costs its length times how visible it is: occluded (ambient
//!    occlusion), creased and concave places are cheap, so the seams run
//!    under the arms, inside the legs, in folds and along edges. Turning
//!    costs extra, so seams follow edge loops instead of zigzagging.
//! 3. Least squares conformal maps (Levy et al. 2002) flatten each disk,
//!    pinned at its two most distant border points (sparse Cholesky, faer).
//!    When the chart is mostly quads whose edge directions agree (a grid, a
//!    strip, a hair lock), its quads are also straightened: a least squares
//!    solve that turns every quad edge toward one of two axes, as a grid, or
//!    only its rows or only its columns. A straightened layout wins when it
//!    folds nothing and stretches little more than the conformal one.
//!    Charts are flattened in parallel.
//! 4. A chart with flipped or strongly stretched triangles is first opened
//!    like a pelt (a seam from its border to its farthest tip), else split
//!    in two: along an edge loop of its quads when one parts it (around a
//!    narrow, hidden neck, through the overlap), else between overlapping
//!    triangles, by surface direction or by position. Those last three only
//!    say which triangles are clearly on which side; the seam itself is a
//!    minimum cut through the band between (edges cost their length times
//!    how visible they are), so it runs short and straight, not stepped.
//!    Polygons are never cut.
//! 5. Small islands go back into a bigger neighbor when the union still
//!    flattens cleanly (no fold, no overlap, little stretch), a few passes,
//!    every candidate tried in parallel; part borders stay seams. Each
//!    island is then turned upright (the model's up points up, long islands
//!    snap to an axis, straightened ones keep their rows on the axes) when
//!    `upright` is on.
//! 6. uvunwrap (vendored: per-face allowed turns, packing orders and
//!    non-manifold edges kept together added) packs the islands
//!    (`existing_uvs`): uniform texel density, shape-aware packing, tried
//!    in several orders and by rows in parallel, the fullest page kept;
//!    upright islands only turn as `upright` allows. An island the packer
//!    finds invalid is unwrapped its own way, so the result is always
//!    usable.

use super::RawMesh;
use anyhow::{anyhow, Result};
use glam::{DVec2, Vec3};
use std::collections::{BinaryHeap, HashMap};

const NONE: u32 = u32::MAX;

/// Settings of the smart unwrap.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SmartOptions {
    /// Edges sharper than this angle (degrees) are seams.
    pub hard_angle: f32,
    /// Cuts follow occluded, creased places (ambient occlusion rays).
    pub hide_seams: bool,
    /// Area stretch accepted before a chart is split (1 = none).
    pub max_stretch: f32,
    /// Islands stand as on the model (up is up, limbs straight) and the
    /// packer may not turn them: easier to paint by hand, a little less
    /// texture used.
    pub upright: bool,
    /// Atlas page size in texels and the gap around islands.
    pub page: u32,
    pub padding: u32,
}

impl Default for SmartOptions {
    fn default() -> Self {
        SmartOptions { hard_angle: 60.0, hide_seams: true, max_stretch: 2.0, upright: true, page: 2048, padding: 4 }
    }
}

/// What an unwrap made, per texture set.
#[derive(Clone, Debug, Default)]
pub struct UvReport {
    pub name: String,
    pub triangles: usize,
    pub charts: u32,
    /// Fraction of the UV square the islands cover.
    pub coverage: f32,
    /// Area stretch: area-weighted mean and worst 5 % (1 = none).
    pub stretch_mean: f32,
    pub stretch_p95: f32,
    /// Seam length relative to the square root of the surface area.
    pub seam_length: f32,
}

/// Disjoint sets.
struct Dsu(Vec<u32>);

impl Dsu {
    fn new(n: usize) -> Self {
        Dsu((0..n as u32).collect())
    }
    fn find(&mut self, mut x: u32) -> u32 {
        while self.0[x as usize] != x {
            let p = self.0[x as usize];
            self.0[x as usize] = self.0[p as usize];
            x = p;
        }
        x
    }
    fn union(&mut self, a: u32, b: u32) {
        let (a, b) = (self.find(a), self.find(b));
        if a != b {
            self.0[a as usize] = b;
        }
    }
}

/// Dijkstra entry (min-heap by cost).
#[derive(PartialEq)]
struct Node(f64, u32);
impl Eq for Node {}
impl PartialOrd for Node {
    fn partial_cmp(&self, o: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for Node {
    fn cmp(&self, o: &Self) -> std::cmp::Ordering {
        o.0.total_cmp(&self.0).then(o.1.cmp(&self.1))
    }
}

/// The geometry of one material group (a texture set) being unwrapped.
struct Geo {
    /// Welded positions.
    pos: Vec<Vec3>,
    /// Triangles: welded corners, raw corners, polygon.
    tris: Vec<[u32; 3]>,
    raw: Vec<[u32; 3]>,
    poly: Vec<u32>,
    /// Neighbor across local edge k (corner k to k + 1) and its edge index.
    adj: Vec<[u32; 3]>,
    adj_k: Vec<[u8; 3]>,
    fnormal: Vec<Vec3>,
    area: Vec<f32>,
    /// Cost factor of cutting each edge (visibility, crease).
    cost: Vec<[f32; 3]>,
    /// No area (repeated corner or a sliver): in no chart, its corners
    /// collapse onto a neighbor's UVs.
    degen: Vec<bool>,
    /// Edges between parts of the model: always seams.
    part: Vec<[bool; 3]>,
}

impl Geo {
    fn edge(&self, t: usize, k: usize) -> (u32, u32) {
        (self.tris[t][k], self.tris[t][(k + 1) % 3])
    }

    fn edge_len(&self, t: usize, k: usize) -> f32 {
        let (a, b) = self.edge(t, k);
        self.pos[a as usize].distance(self.pos[b as usize])
    }
}

/// A group and its seams. A copy shares the geometry and copies only the
/// seams, so charts can be worked on in copies (in parallel, or to try a
/// merge) and their seams kept or dropped.
#[derive(Clone)]
struct Group {
    geo: std::sync::Arc<Geo>,
    seam: Vec<[bool; 3]>,
}

impl std::ops::Deref for Group {
    type Target = Geo;
    fn deref(&self) -> &Geo {
        &self.geo
    }
}

impl Group {
    /// Takes the seams of `tris` from a copy that worked on them.
    fn take(&mut self, from: &Group, tris: &[u32]) {
        for &t in tris {
            self.seam[t as usize] = from.seam[t as usize];
        }
    }

    /// Marks the edge (t, k) a seam on both of its sides.
    fn cut(&mut self, t: usize, k: usize) {
        self.seam[t][k] = true;
        let n = self.adj[t][k];
        if n != NONE {
            let kn = self.adj_k[t][k] as usize;
            self.seam[n as usize][kn] = true;
        }
    }

    /// Whether edge (t, k) joins two triangles of `chart` without a seam.
    fn inner(&self, t: usize, k: usize, chart_of: &[u32], c: u32) -> bool {
        let n = self.adj[t][k];
        n != NONE && !self.seam[t][k] && chart_of[n as usize] == c
    }
}

/// The cut mesh of one chart: its corners grouped into vertices across the
/// edges that are not seams.
struct Cut {
    /// Local vertex of each corner (3 per chart triangle).
    vert: Vec<u32>,
    nverts: usize,
    /// Welded vertex of each local vertex.
    welded: Vec<u32>,
    /// Boundary half-edges (local from, local to).
    border: Vec<(u32, u32)>,
}

fn cut_mesh(g: &Group, tris: &[u32], chart_of: &[u32], c: u32) -> Cut {
    let local: HashMap<u32, usize> = tris.iter().enumerate().map(|(i, t)| (*t, i)).collect();
    let mut dsu = Dsu::new(tris.len() * 3);
    let mut border = Vec::new();
    for (i, &t) in tris.iter().enumerate() {
        let t = t as usize;
        for k in 0..3 {
            if g.inner(t, k, chart_of, c) {
                let n = g.adj[t][k] as usize;
                let kn = g.adj_k[t][k] as usize;
                let j = local[&(n as u32)];
                // The neighbor runs the edge the other way: its corner kn is
                // our corner k + 1, its corner kn + 1 our corner k.
                dsu.union((i * 3 + k) as u32, (j * 3 + (kn + 1) % 3) as u32);
                dsu.union((i * 3 + (k + 1) % 3) as u32, (j * 3 + kn) as u32);
            }
        }
    }
    let mut ids: HashMap<u32, u32> = HashMap::new();
    let mut vert = vec![0u32; tris.len() * 3];
    let mut welded = Vec::new();
    for (i, &t) in tris.iter().enumerate() {
        for k in 0..3 {
            let r = dsu.find((i * 3 + k) as u32);
            let n = ids.len() as u32;
            let v = *ids.entry(r).or_insert_with(|| {
                welded.push(g.tris[t as usize][k]);
                n
            });
            vert[i * 3 + k] = v;
        }
    }
    for (i, &t) in tris.iter().enumerate() {
        for k in 0..3 {
            if !g.inner(t as usize, k, chart_of, c) {
                border.push((vert[i * 3 + k], vert[i * 3 + (k + 1) % 3]));
            }
        }
    }
    Cut { vert, nverts: ids.len(), welded, border }
}

/// Border loops and Euler characteristic of a cut mesh.
fn topology(cut: &Cut, ntris: usize) -> (usize, i64) {
    let e = (3 * ntris + cut.border.len()) / 2;
    let chi = cut.nverts as i64 - e as i64 + ntris as i64;
    // Loops: follow each border half-edge to the one leaving its end.
    let mut next: HashMap<u32, Vec<usize>> = HashMap::new();
    for (i, (a, _)) in cut.border.iter().enumerate() {
        next.entry(*a).or_default().push(i);
    }
    let mut seen = vec![false; cut.border.len()];
    let mut loops = 0;
    for s in 0..cut.border.len() {
        if seen[s] {
            continue;
        }
        loops += 1;
        let mut h = s;
        while !seen[h] {
            seen[h] = true;
            let to = cut.border[h].1;
            match next.get(&to).and_then(|v| v.iter().find(|i| !seen[**i])) {
                Some(&n) => h = n,
                None => break,
            }
        }
    }
    (loops, chi)
}

/// Interior edges of a chart's cut mesh as a graph over local vertices:
/// (other vertex, weight, triangle, edge).
fn edge_graph(g: &Group, tris: &[u32], chart_of: &[u32], c: u32, cut: &Cut, weighted: bool) -> Vec<Vec<(u32, f64, u32, u8)>> {
    let mut graph = vec![Vec::new(); cut.nverts];
    for (i, &t) in tris.iter().enumerate() {
        let t = t as usize;
        for k in 0..3 {
            if !g.inner(t, k, chart_of, c) {
                continue;
            }
            let n = g.adj[t][k] as usize;
            // Each interior edge once (from its lower triangle).
            if n < t {
                continue;
            }
            let (a, b) = (cut.vert[i * 3 + k], cut.vert[i * 3 + (k + 1) % 3]);
            let mut w = g.edge_len(t, k) as f64;
            if weighted {
                w *= g.cost[t][k] as f64;
                // Splitting a polygon is the last resort.
                if g.poly[t] == g.poly[n] {
                    w *= 50.0;
                }
            }
            graph[a as usize].push((b, w, t as u32, k as u8));
            graph[b as usize].push((a, w, t as u32, k as u8));
        }
    }
    graph
}

/// Shortest paths from `sources`; stops at the first vertex `stop` accepts.
/// Returns the distance and predecessor (vertex, triangle, edge) arrays and
/// the vertex reached.
#[allow(clippy::type_complexity)]
fn dijkstra(graph: &[Vec<(u32, f64, u32, u8)>], sources: &[u32], stop: impl Fn(u32) -> bool) -> (Vec<f64>, Vec<(u32, u32, u8)>, Option<u32>) {
    let n = graph.len();
    let mut dist = vec![f64::INFINITY; n];
    let mut prev = vec![(NONE, NONE, 0u8); n];
    let mut heap = BinaryHeap::new();
    for &s in sources {
        dist[s as usize] = 0.0;
        heap.push(Node(0.0, s));
    }
    while let Some(Node(d, v)) = heap.pop() {
        if d > dist[v as usize] {
            continue;
        }
        if d > 0.0 && stop(v) {
            return (dist, prev, Some(v));
        }
        for &(u, w, t, k) in &graph[v as usize] {
            let nd = d + w;
            if nd < dist[u as usize] {
                dist[u as usize] = nd;
                prev[u as usize] = (v, t, k);
                heap.push(Node(nd, u));
            }
        }
    }
    (dist, prev, None)
}

/// How much a seam pays to turn: an edge costs its weight times
/// 1 + TURN * (1 - cos of the turn), so 90 degree turns cost 2.5 times more.
const TURN: f64 = 1.5;

/// Shortest seam from `sources` to the first vertex `stop` accepts that
/// prefers going straight: the state is the edge a path arrives by and a
/// turn costs extra, so seams run along edge loops instead of zigzagging
/// through the grid. Returns the path's edges (triangle, edge).
fn dijkstra_straight(graph: &[Vec<(u32, f64, u32, u8)>], at: &dyn Fn(u32) -> Vec3, sources: &[u32], stop: impl Fn(u32) -> bool) -> Option<Vec<(u32, u8)>> {
    let mut off = vec![0usize; graph.len() + 1];
    for v in 0..graph.len() {
        off[v + 1] = off[v] + graph[v].len();
    }
    let m = off[graph.len()];
    let mut from = vec![0u32; m];
    for v in 0..graph.len() {
        for j in 0..graph[v].len() {
            from[off[v] + j] = v as u32;
        }
    }
    let mut dist = vec![f64::INFINITY; m];
    let mut prev = vec![usize::MAX; m];
    let mut heap = BinaryHeap::new();
    for &s in sources {
        for (j, e) in graph[s as usize].iter().enumerate() {
            let id = off[s as usize] + j;
            if e.1 < dist[id] {
                dist[id] = e.1;
                heap.push(Node(e.1, id as u32));
            }
        }
    }
    while let Some(Node(d, id)) = heap.pop() {
        let id = id as usize;
        if d > dist[id] {
            continue;
        }
        let u = from[id];
        let (v, ..) = graph[u as usize][id - off[u as usize]];
        if stop(v) {
            let mut path = Vec::new();
            let mut e = id;
            while e != usize::MAX && path.len() <= m {
                let fu = from[e];
                let (_, _, t, k) = graph[fu as usize][e - off[fu as usize]];
                path.push((t, k));
                e = prev[e];
            }
            return Some(path);
        }
        let din = (at(v) - at(u)).normalize_or_zero();
        for (j, &(w2, w, ..)) in graph[v as usize].iter().enumerate() {
            if w2 == u {
                continue;
            }
            let dout = (at(w2) - at(v)).normalize_or_zero();
            let turn = (1.0 - din.dot(dout).clamp(-1.0, 1.0)) as f64;
            let nd = d + w * (1.0 + TURN * turn);
            let nid = off[v as usize] + j;
            if nd < dist[nid] {
                dist[nid] = nd;
                prev[nid] = id;
                heap.push(Node(nd, nid as u32));
            }
        }
    }
    None
}

/// One cut toward a disk: joins two borders, or opens a closed chart.
/// False when no cut helps (the chart is split instead).
fn cut_toward_disk(g: &mut Group, tris: &[u32], chart_of: &[u32], c: u32, cut: &Cut, loops: usize) -> bool {
    let graph = edge_graph(g, tris, chart_of, c, cut, true);
    if loops == 0 {
        // Closed: from one end to the other (most distant pair).
        let plain = edge_graph(g, tris, chart_of, c, cut, false);
        let far = |from: u32| -> u32 {
            let (d, _, _) = dijkstra(&plain, &[from], |_| false);
            d.iter().enumerate().filter(|(_, x)| x.is_finite()).max_by(|a, b| a.1.total_cmp(b.1)).map(|(i, _)| i as u32).unwrap_or(from)
        };
        let a = far(0);
        let b = far(a);
        if a == b {
            return false;
        }
        let at = |v: u32| g.pos[cut.welded[v as usize] as usize];
        let Some(path) = dijkstra_straight(&graph, &at, &[a], |v| v == b) else { return false };
        for (t, k) in &path {
            g.cut(*t as usize, *k as usize);
        }
        return !path.is_empty();
    }
    // Several borders: from the first border to the nearest other one.
    let mut dsu = Dsu::new(cut.nverts);
    for (a, b) in &cut.border {
        dsu.union(*a, *b);
    }
    let first = dsu.find(cut.border[0].0);
    let on_border: Vec<bool> = {
        let mut v = vec![false; cut.nverts];
        for (a, _) in &cut.border {
            v[*a as usize] = true;
        }
        v
    };
    let sources: Vec<u32> = (0..cut.nverts as u32).filter(|v| on_border[*v as usize] && dsu.find(*v) == first).collect();
    let mut roots = vec![0u32; cut.nverts];
    for (v, r) in roots.iter_mut().enumerate() {
        *r = dsu.find(v as u32);
    }
    let at = |v: u32| g.pos[cut.welded[v as usize] as usize];
    let Some(path) = dijkstra_straight(&graph, &at, &sources, |v| on_border[v as usize] && roots[v as usize] != first) else { return false };
    for (t, k) in &path {
        g.cut(*t as usize, *k as usize);
    }
    !path.is_empty()
}

/// Smallest cut of a graph between sources and sinks (Edmonds-Karp:
/// shortest augmenting paths). `edges` are (node, node, capacity), both
/// ways; node 0 is the source, 1 the sink. Returns which nodes stay on the
/// source side.
fn min_cut(n: usize, edges: &[(usize, usize, f64)]) -> Vec<bool> {
    // Residual graph: per node, (to, edge id); paired ids e and e ^ 1.
    let mut adj: Vec<Vec<(usize, usize)>> = vec![Vec::new(); n];
    let mut cap = Vec::with_capacity(edges.len() * 2);
    for &(a, b, c) in edges {
        adj[a].push((b, cap.len()));
        cap.push(c);
        adj[b].push((a, cap.len()));
        cap.push(c);
    }
    let eps = 1e-12;
    let mut prev = vec![usize::MAX; n];
    for _ in 0..10_000 {
        prev.iter_mut().for_each(|p| *p = usize::MAX);
        prev[0] = 0;
        let mut queue = std::collections::VecDeque::from([0usize]);
        while let Some(v) = queue.pop_front() {
            if v == 1 {
                break;
            }
            for &(u, e) in &adj[v] {
                if prev[u] == usize::MAX && cap[e] > eps {
                    prev[u] = e;
                    queue.push_back(u);
                }
            }
        }
        if prev[1] == usize::MAX {
            break;
        }
        // The path's bottleneck, then push it.
        let mut push = f64::INFINITY;
        let mut v = 1;
        while v != 0 {
            let e = prev[v];
            push = push.min(cap[e]);
            v = adj_from(&adj, e, v);
        }
        let mut v = 1;
        while v != 0 {
            let e = prev[v];
            cap[e] -= push;
            cap[e ^ 1] += push;
            v = adj_from(&adj, e, v);
        }
    }
    // Source side: still reachable.
    let mut side = vec![false; n];
    side[0] = true;
    let mut stack = vec![0usize];
    while let Some(v) = stack.pop() {
        for &(u, e) in &adj[v] {
            if !side[u] && cap[e] > eps {
                side[u] = true;
                stack.push(u);
            }
        }
    }
    side
}

/// The node an edge of `min_cut`'s residual graph leaves from, given the
/// node it reaches.
fn adj_from(adj: &[Vec<(usize, usize)>], e: usize, to: usize) -> usize {
    // Edge e ^ 1 runs back from `to`.
    adj[to].iter().find(|(_, id)| *id == e ^ 1).map(|(u, _)| *u).unwrap_or(0)
}

/// Distance over a chart's triangles (by local index) from `seeds`, a step
/// between neighbors costing `step(from, to)`, nothing inside a polygon.
fn grow(g: &Group, tris: &[u32], chart_of: &[u32], c: u32, seeds: &[usize], step: impl Fn(u32, u32) -> f64) -> Vec<f64> {
    let local: HashMap<u32, usize> = tris.iter().enumerate().map(|(i, t)| (*t, i)).collect();
    let mut dist = vec![f64::INFINITY; tris.len()];
    let mut heap = BinaryHeap::new();
    for &s in seeds {
        dist[s] = 0.0;
        heap.push(Node(0.0, s as u32));
    }
    while let Some(Node(d, i)) = heap.pop() {
        if d > dist[i as usize] {
            continue;
        }
        let t = tris[i as usize];
        for k in 0..3 {
            if !g.inner(t as usize, k, chart_of, c) {
                continue;
            }
            let n = g.adj[t as usize][k];
            let j = local[&n];
            let nd = d + if g.poly[n as usize] == g.poly[t as usize] { 0.0 } else { step(t, n) };
            if nd < dist[j] {
                dist[j] = nd;
                heap.push(Node(nd, j as u32));
            }
        }
    }
    dist
}

/// Cuts a chart in two along the cheapest seam between where `side` (per
/// triangle by local index, 0 to 1) is clearly low and clearly high: the
/// band in between is free, and a minimum cut through it (each edge
/// costing its length times how visible it is) finds the shortest, most
/// hidden way across. On a grid that is an edge loop, never a staircase.
/// Polygons stay whole.
fn cut_by_field(g: &mut Group, tris: &[u32], chart_of: &[u32], c: u32, side: &[f64]) -> bool {
    // Nodes: source, sink, then the chart's polygons.
    let mut node: HashMap<u32, usize> = HashMap::new();
    let mut weight: Vec<(f64, f64)> = Vec::new();
    for (i, &t) in tris.iter().enumerate() {
        let p = g.poly[t as usize];
        let n = *node.entry(p).or_insert_with(|| {
            weight.push((0.0, 0.0));
            weight.len() + 1
        });
        let a = g.area[t as usize] as f64 + 1e-30;
        let f = if side[i].is_finite() { side[i] } else { 0.5 };
        weight[n - 2].0 += f * a;
        weight[n - 2].1 += a;
    }
    let f: Vec<f64> = weight.iter().map(|(fa, a)| fa / a).collect();
    // Clearly one side or the other: the outer 30 % of the field's range.
    let (lo, hi) = f.iter().fold((f64::MAX, f64::MIN), |m, x| (m.0.min(*x), m.1.max(*x)));
    if !(hi - lo > 1e-9) {
        return false;
    }
    let (a_max, b_min) = (lo + 0.3 * (hi - lo), hi - 0.3 * (hi - lo));
    let mut edges: Vec<(usize, usize, f64)> = Vec::new();
    for (i, fi) in f.iter().enumerate() {
        if *fi <= a_max {
            edges.push((0, i + 2, f64::INFINITY));
        } else if *fi >= b_min {
            edges.push((i + 2, 1, f64::INFINITY));
        }
    }
    let mut pair: HashMap<(usize, usize), f64> = HashMap::new();
    for &t in tris {
        for k in 0..3 {
            if !g.inner(t as usize, k, chart_of, c) {
                continue;
            }
            let n = g.adj[t as usize][k];
            let (p, q) = (node[&g.poly[t as usize]], node[&g.poly[n as usize]]);
            if p < q {
                *pair.entry((p, q)).or_insert(0.0) += g.edge_len(t as usize, k) as f64 * g.cost[t as usize][k] as f64 + 1e-12;
            }
        }
    }
    let mut pairs: Vec<((usize, usize), f64)> = pair.into_iter().collect();
    pairs.sort_by(|a, b| a.0.cmp(&b.0));
    edges.extend(pairs.into_iter().map(|((p, q), w)| (p, q, w)));
    let src = min_cut(weight.len() + 2, &edges);
    let mut cuts = 0;
    for &t in tris {
        for k in 0..3 {
            if g.inner(t as usize, k, chart_of, c) {
                let n = g.adj[t as usize][k];
                if src[node[&g.poly[t as usize]]] != src[node[&g.poly[n as usize]]] {
                    g.cut(t as usize, k);
                    cuts += 1;
                }
            }
        }
    }
    cuts > 0
}

/// A disk that flattens badly (a finger, a limb or a whole body closed at
/// its ends) is opened like a pelt: one seam from its border to the point
/// farthest from it (a tip), along the most hidden, straightest way. The
/// tube becomes a strip instead of a crushed cap. False when the chart
/// has no tip worth it.
fn tip_cut(g: &mut Group, tris: &[u32], chart_of: &[u32], c: u32, cut: &Cut) -> bool {
    if cut.border.is_empty() {
        return false;
    }
    let plain = edge_graph(g, tris, chart_of, c, cut, false);
    let mut on_border = vec![false; cut.nverts];
    for (a, b) in &cut.border {
        on_border[*a as usize] = true;
        on_border[*b as usize] = true;
    }
    let sources: Vec<u32> = (0..cut.nverts as u32).filter(|v| on_border[*v as usize]).collect();
    let (d, _, _) = dijkstra(&plain, &sources, |_| false);
    let Some((far, depth)) = d.iter().enumerate().filter(|(_, x)| x.is_finite()).max_by(|a, b| a.1.total_cmp(b.1)).map(|(i, x)| (i as u32, *x)) else { return false };
    // A shallow chart (a patch, not a tube) gains nothing from a slit.
    let area: f64 = tris.iter().map(|t| g.area[*t as usize] as f64).sum();
    let rim: f64 = cut.border.iter().map(|(a, b)| g.pos[cut.welded[*a as usize] as usize].distance(g.pos[cut.welded[*b as usize] as usize]) as f64).sum();
    if depth <= 0.0 || depth < 0.25 * area.sqrt().min(rim / 4.0) {
        return false;
    }
    let graph = edge_graph(g, tris, chart_of, c, cut, true);
    let at = |v: u32| g.pos[cut.welded[v as usize] as usize];
    let Some(path) = dijkstra_straight(&graph, &at, &[far], |v| on_border[v as usize]) else { return false };
    for (t, k) in &path {
        g.cut(*t as usize, *k as usize);
    }
    !path.is_empty()
}

/// Splits a chart in two by surface direction: the two most different
/// normals seed regions grown over the surface (cheap toward triangles
/// facing the seed's way); the seam runs where they meet.
fn split_by_normals(g: &mut Group, tris: &[u32], chart_of: &[u32], c: u32) -> bool {
    if tris.len() < 2 {
        return false;
    }
    let avg: Vec3 = tris.iter().map(|t| g.fnormal[*t as usize] * g.area[*t as usize]).sum::<Vec3>().normalize_or_zero();
    let s1 = (0..tris.len()).min_by(|a, b| g.fnormal[tris[*a] as usize].dot(avg).total_cmp(&g.fnormal[tris[*b] as usize].dot(avg))).unwrap();
    let n1 = g.fnormal[tris[s1] as usize];
    let s2 = (0..tris.len()).min_by(|a, b| g.fnormal[tris[*a] as usize].dot(n1).total_cmp(&g.fnormal[tris[*b] as usize].dot(n1))).unwrap();
    if g.poly[tris[s1] as usize] == g.poly[tris[s2] as usize] {
        return false;
    }
    let n2 = g.fnormal[tris[s2] as usize];
    let fnormal = &g.fnormal;
    let step = |n: Vec3| move |_: u32, to: u32| 0.01 + (1.0 - fnormal[to as usize].dot(n)) as f64;
    let d1 = grow(g, tris, chart_of, c, &[s1], step(n1));
    let d2 = grow(g, tris, chart_of, c, &[s2], step(n2));
    let side: Vec<f64> = d1.iter().zip(&d2).map(|(a, b)| a / (a + b).max(1e-30)).collect();
    cut_by_field(g, tris, chart_of, c, &side)
}

/// Splits a chart between two of its triangles (an overlap in UV, local
/// indices): the seam lands about halfway over the surface, where the
/// flattened chart folded onto itself.
fn split_between(g: &mut Group, tris: &[u32], chart_of: &[u32], c: u32, a: usize, b: usize) -> bool {
    if a == b || g.poly[tris[a] as usize] == g.poly[tris[b] as usize] {
        return false;
    }
    let center = |t: u32| {
        let w = g.tris[t as usize];
        (g.pos[w[0] as usize] + g.pos[w[1] as usize] + g.pos[w[2] as usize]) / 3.0
    };
    let step = |t: u32, n: u32| center(t).distance(center(n)) as f64;
    let da = grow(g, tris, chart_of, c, &[a], step);
    let db = grow(g, tris, chart_of, c, &[b], step);
    let side: Vec<f64> = da.iter().zip(&db).map(|(x, y)| x / (x + y).max(1e-30)).collect();
    cut_by_field(g, tris, chart_of, c, &side)
}

/// Splits a chart along an edge loop of its quads (the row of edges a
/// regular grid continues straight through, as an artist selects one): a
/// closed loop around a limb, or a loop running from border to border. The
/// loop must part triangles `pair` (local indices: two overlapping
/// triangles) when given; the cheapest wins, by its length times how
/// visible it is over the square root of the smaller side's area, so cuts
/// go round narrow, hidden necks (wrists, the base of a strap) with real
/// parts on both sides. Seams cut this way are straight. False when no loop
/// parts the chart.
fn split_by_loop(g: &mut Group, tris: &[u32], chart_of: &[u32], c: u32, cut: &Cut, pair: Option<(usize, usize)>) -> bool {
    let nv = cut.nverts;
    let mut on_border = vec![false; nv];
    for (a, b) in &cut.border {
        on_border[*a as usize] = true;
        on_border[*b as usize] = true;
    }
    // Polygons of two triangles (quads) and how many polygons meet at each
    // vertex.
    let mut poly_tris: HashMap<u32, u32> = HashMap::new();
    for &t in tris {
        *poly_tris.entry(g.poly[t as usize]).or_insert(0) += 1;
    }
    let mut vert_polys: Vec<Vec<u32>> = vec![Vec::new(); nv];
    for (i, &t) in tris.iter().enumerate() {
        for k in 0..3 {
            let v = &mut vert_polys[cut.vert[i * 3 + k] as usize];
            let p = g.poly[t as usize];
            if !v.contains(&p) {
                v.push(p);
            }
        }
    }
    // Edges between polygons inside the chart: local vertex pair -> (one
    // side as (triangle, edge), the two polygons).
    let mut edges: HashMap<(u32, u32), ((u32, u8), [u32; 2])> = HashMap::new();
    let mut at: Vec<Vec<(u32, u32)>> = vec![Vec::new(); nv];
    for (i, &t) in tris.iter().enumerate() {
        for k in 0..3 {
            if !g.inner(t as usize, k, chart_of, c) {
                continue;
            }
            let nb = g.adj[t as usize][k];
            let (p, q) = (g.poly[t as usize], g.poly[nb as usize]);
            if p == q {
                continue;
            }
            let (a, b) = (cut.vert[i * 3 + k], cut.vert[i * 3 + (k + 1) % 3]);
            let key = (a.min(b), a.max(b));
            if edges.insert(key, ((t, k as u8), [p, q])).is_none() {
                at[a as usize].push(key);
                at[b as usize].push(key);
            }
        }
    }
    // The edge a loop continues by at vertex v, coming by edge e: the one
    // sharing no polygon with it, where four quads meet.
    let regular = |v: u32| !on_border[v as usize] && at[v as usize].len() == 4 && vert_polys[v as usize].len() == 4 && vert_polys[v as usize].iter().all(|p| poly_tris.get(p) == Some(&2));
    let next = |v: u32, e: (u32, u32)| -> Option<(u32, u32)> {
        if !regular(v) {
            return None;
        }
        let ps = edges[&e].1;
        let mut it = at[v as usize].iter().filter(|o| **o != e && !edges[o].1.iter().any(|p| ps.contains(p)));
        let o = *it.next()?;
        it.next().is_none().then_some(o)
    };
    // Every loop once: (edges, closed); open ones count only when both ends
    // reach the border (a loop stopping at an irregular vertex parts
    // nothing).
    let mut seen: std::collections::HashSet<(u32, u32)> = std::collections::HashSet::new();
    let mut keys: Vec<(u32, u32)> = edges.keys().copied().collect();
    keys.sort_unstable();
    let mut loops: Vec<Vec<(u32, u32)>> = Vec::new();
    for e0 in keys {
        if seen.contains(&e0) {
            continue;
        }
        seen.insert(e0);
        let mut path = vec![e0];
        let mut closed = false;
        let mut ends_ok = true;
        for (start, dir_end) in [(e0.1, true), (e0.0, false)] {
            let (mut v, mut e) = (start, e0);
            loop {
                match next(v, e) {
                    Some(o) if o == e0 => {
                        closed = true;
                        break;
                    }
                    Some(o) => {
                        if !seen.insert(o) {
                            break;
                        }
                        if dir_end {
                            path.push(o);
                        } else {
                            path.insert(0, o);
                        }
                        v = if o.0 == v { o.1 } else { o.0 };
                        e = o;
                    }
                    None => {
                        ends_ok &= on_border[v as usize];
                        break;
                    }
                }
            }
            if closed {
                break;
            }
        }
        if (closed || ends_ok) && path.len() >= 2 {
            loops.push(path);
        }
    }
    if loops.is_empty() {
        return false;
    }
    // Triangle adjacency inside the chart by local index.
    let local: HashMap<u32, usize> = tris.iter().enumerate().map(|(i, t)| (*t, i)).collect();
    let area: Vec<f64> = tris.iter().map(|t| g.area[*t as usize] as f64).collect();
    let total: f64 = area.iter().sum();
    let mut best: Option<(f64, usize)> = None;
    let mut reach = vec![u32::MAX; tris.len()];
    let start = pair.map_or(0, |p| p.0);
    for (li, path) in loops.iter().enumerate() {
        let wall: std::collections::HashSet<(u32, u32)> = path.iter().copied().collect();
        // The side of `start`.
        let mut side = 0.0;
        let mut stack = vec![start];
        reach[start] = li as u32;
        while let Some(i) = stack.pop() {
            side += area[i];
            let t = tris[i] as usize;
            for k in 0..3 {
                if !g.inner(t, k, chart_of, c) {
                    continue;
                }
                let (a, b) = (cut.vert[i * 3 + k], cut.vert[i * 3 + (k + 1) % 3]);
                if wall.contains(&(a.min(b), a.max(b))) {
                    continue;
                }
                let j = local[&g.adj[t][k]];
                if reach[j] != li as u32 {
                    reach[j] = li as u32;
                    stack.push(j);
                }
            }
        }
        let parts = match pair {
            Some((_, b)) => reach[b] != li as u32,
            None => side < total * 0.999,
        };
        let small = side.min(total - side);
        if !parts || small < total * 0.02 {
            continue;
        }
        let cost: f64 = path
            .iter()
            .map(|e| {
                let ((t, k), _) = edges[e];
                g.edge_len(t as usize, k as usize) as f64 * g.cost[t as usize][k as usize] as f64
            })
            .sum();
        let score = cost / small.sqrt();
        if best.is_none_or(|b| score < b.0) {
            best = Some((score, li));
        }
    }
    let Some((_, li)) = best else { return false };
    for e in &loops[li] {
        let ((t, k), _) = edges[e];
        g.cut(t as usize, k as usize);
    }
    true
}

/// Splits a chart in two halves along its longest extent (by triangle
/// center): for charts all facing one way.
fn split_by_position(g: &mut Group, tris: &[u32], chart_of: &[u32], c: u32) -> bool {
    if tris.len() < 2 {
        return false;
    }
    let center = |t: u32| {
        let w = g.tris[t as usize];
        (g.pos[w[0] as usize] + g.pos[w[1] as usize] + g.pos[w[2] as usize]) / 3.0
    };
    let lo = tris.iter().fold(Vec3::splat(f32::MAX), |m, t| m.min(center(*t)));
    let hi = tris.iter().fold(Vec3::splat(f32::MIN), |m, t| m.max(center(*t)));
    let ext = hi - lo;
    let axis = if ext.x >= ext.y && ext.x >= ext.z { 0 } else if ext.y >= ext.z { 1 } else { 2 };
    // By rank, so the halves hold about as many triangles.
    let mut order: Vec<usize> = (0..tris.len()).collect();
    order.sort_by(|a, b| center(tris[*a])[axis].total_cmp(&center(tris[*b])[axis]).then(a.cmp(b)));
    let mut side = vec![0.0; tris.len()];
    for (r, i) in order.into_iter().enumerate() {
        side[i] = r as f64 / tris.len() as f64;
    }
    cut_by_field(g, tris, chart_of, c, &side)
}

/// Seams that end in the middle of the surface (a hard edge fading out, a
/// few split normals) are slits: they cut nothing apart, and joining them
/// up later zigzags. Their dead ends are taken back, edge by edge, until
/// every seam closes a loop or runs between borders. Returns the edges
/// taken back.
fn prune_dead_ends(g: &mut Group) -> usize {
    // Each seam edge once (from its lower triangle), by welded vertex; open
    // borders count as seams that never end.
    let mut at: HashMap<u32, Vec<(u32, u8)>> = HashMap::new();
    let mut border: std::collections::HashSet<u32> = std::collections::HashSet::new();
    for t in 0..g.tris.len() {
        if g.degen[t] {
            continue;
        }
        for k in 0..3 {
            let n = g.adj[t][k];
            let (a, b) = g.edge(t, k);
            if n == NONE {
                border.insert(a);
                border.insert(b);
            } else if g.seam[t][k] && (n as usize) > t {
                at.entry(a).or_default().push((t as u32, k as u8));
                at.entry(b).or_default().push((t as u32, k as u8));
            }
        }
    }
    let live = |g: &Group, e: &(u32, u8)| g.seam[e.0 as usize][e.1 as usize];
    let mut stack: Vec<u32> = at.keys().copied().filter(|v| !border.contains(v)).collect();
    stack.sort_unstable();
    let mut n = 0;
    while let Some(v) = stack.pop() {
        if border.contains(&v) {
            continue;
        }
        let Some(list) = at.get(&v) else { continue };
        let alive: Vec<(u32, u8)> = list.iter().copied().filter(|e| live(g, e)).collect();
        if alive.len() != 1 {
            continue;
        }
        let (t, k) = (alive[0].0 as usize, alive[0].1 as usize);
        g.seam[t][k] = false;
        let nb = g.adj[t][k];
        if nb != NONE {
            let kn = g.adj_k[t][k] as usize;
            g.seam[nb as usize][kn] = false;
        }
        n += 1;
        let (a, b) = g.edge(t, k);
        stack.push(if a == v { b } else { a });
    }
    n
}

/// Turns a flattened chart upright, as an artist lays islands out: the
/// model's up (+Y) points up in the texture (V decreasing, image rows
/// grow downward). A chart that barely climbs (a sole, a shoulder top)
/// lies along its longest side instead. A long chart (a limb, a strap)
/// then snaps straight to the nearest axis, an arm held out included. A
/// straightened chart only turns by quarter turns, so its rows stay
/// level. Returns the quarter turns the packer may still give it (bit k:
/// k turns): none for a chart that climbs, a half turn for one that lies.
fn upright(g: &Group, tris: &[u32], verts: &[u32], uv: &mut [DVec2], straight: bool) -> u8 {
    let quarter = |a: f64| (a / std::f64::consts::FRAC_PI_2).round() * std::f64::consts::FRAC_PI_2;
    let turn = |uv: &mut [DVec2], angle: f64| {
        let (s, co) = angle.sin_cos();
        for p in uv.iter_mut() {
            *p = DVec2::new(p.x * co - p.y * s, p.x * s + p.y * co);
        }
    };
    // Area weighted principal axis (angle from U) and elongation.
    let axis = |uv: &[DVec2]| {
        let (mut sum, mut c) = (0.0f64, DVec2::ZERO);
        let tri = |i: usize| [uv[verts[i * 3] as usize], uv[verts[i * 3 + 1] as usize], uv[verts[i * 3 + 2] as usize]];
        for i in 0..tris.len() {
            let u = tri(i);
            let a = (u[1] - u[0]).perp_dot(u[2] - u[0]).abs() * 0.5;
            sum += a;
            c += (u[0] + u[1] + u[2]) / 3.0 * a;
        }
        if sum <= 0.0 {
            return (0.0, 1.0);
        }
        c /= sum;
        let (mut xx, mut xy, mut yy) = (0.0, 0.0, 0.0);
        for i in 0..tris.len() {
            let u = tri(i);
            let a = (u[1] - u[0]).perp_dot(u[2] - u[0]).abs() * 0.5;
            // Second moment of the triangle about the chart center.
            let d = [u[0] - c, u[1] - c, u[2] - c];
            let s = d[0] + d[1] + d[2];
            let sq = d.iter().fold(DVec2::ZERO, |m, p| m + *p * *p);
            let sxy: f64 = d.iter().map(|p| p.x * p.y).sum();
            xx += a / 12.0 * (sq.x + s.x * s.x);
            yy += a / 12.0 * (sq.y + s.y * s.y);
            xy += a / 12.0 * (sxy + s.x * s.y);
        }
        let tr = 0.5 * (xx + yy);
        let r = (0.25 * (xx - yy) * (xx - yy) + xy * xy).sqrt();
        let ratio = ((tr + r) / (tr - r).max(1e-30)).sqrt();
        (0.5 * (2.0 * xy).atan2(xx - yy), ratio)
    };
    let mut grad = DVec2::ZERO;
    let mut mag = 0.0f64;
    for (i, &t) in tris.iter().enumerate() {
        let u = [uv[verts[i * 3] as usize], uv[verts[i * 3 + 1] as usize], uv[verts[i * 3 + 2] as usize]];
        let w = g.tris[t as usize];
        let y = [g.pos[w[0] as usize].y as f64, g.pos[w[1] as usize].y as f64, g.pos[w[2] as usize].y as f64];
        let (e1, e2) = (u[1] - u[0], u[2] - u[0]);
        let det = e1.perp_dot(e2);
        if det.abs() < 1e-18 {
            continue;
        }
        // World height as a function of UV: its gradient.
        let (d1, d2) = (y[1] - y[0], y[2] - y[0]);
        let gr = DVec2::new(d1 * e2.y - d2 * e1.y, e1.x * d2 - e2.x * d1) / det;
        let a = det.abs() * 0.5;
        grad += gr * a;
        mag += gr.length() * a;
    }
    if grad.length() > 0.5 * mag && mag > 0.0 {
        // Rotate the gradient onto (0, -1).
        let to_up = -std::f64::consts::FRAC_PI_2 - grad.y.atan2(grad.x);
        if straight {
            turn(uv, quarter(to_up));
            return 1;
        }
        turn(uv, to_up);
        let (a, ratio) = axis(uv);
        if ratio > 1.4 {
            // Straight up unless clearly lying (an arm held out stands up).
            let q = std::f64::consts::FRAC_PI_2;
            let to = if (q - a.abs()) < 0.96 { q.copysign(a) } else { 0.0 };
            turn(uv, to - a);
        }
        1
    } else {
        // Principal axis along U.
        let (a, _) = axis(uv);
        turn(uv, if straight { quarter(-a) } else { -a });
        0b0101
    }
}

/// A flattened chart: UV per local vertex, the local vertex of each corner,
/// and whether its quads were straightened (rows and columns on the axes).
struct Flattened {
    uv: Vec<DVec2>,
    vert: Vec<u32>,
    straight: bool,
    /// Worst 5 % area stretch.
    p95: f64,
}

/// Flattened charts by (first triangle, size).
type Flat = HashMap<(u32, usize), Flattened>;

/// How much more stretch (worst 5 %) a straightened chart may have than its
/// conformal map: straight rows and columns are worth some (a tapered limb
/// becomes a rectangle), up to STRAIGHT_FREE whatever the conformal map has.
/// Straight rows alone are worth less.
const STRAIGHT_STRETCH: f64 = 1.3;
/// And how much more mean stretch (a grid, rows or columns alone).
const STRAIGHT_MEAN: f64 = 1.12;
const LEVEL_MEAN: f64 = 1.06;
const STRAIGHT_FREE: f64 = 1.6;
const LEVEL_STRETCH: f64 = 1.15;

/// Flattens a disk: the conformal map, or better its quads straightened
/// (a grid, else straight rows or straight columns) when that folds nothing
/// and stretches little more, and no more than `max_stretch`. Returns the
/// UVs and whether they are straightened (rows or columns on the axes, so
/// only quarter turns keep them so).
fn flatten(g: &Group, tris: &[u32], cut: &Cut, max_stretch: f64) -> Option<(Vec<DVec2>, bool)> {
    let conf = lscm(g, tris, cut);
    // Quads that disagree (an irregular vertex inside) can not all line up:
    // trying costs time and seldom pays.
    let Some(frame) = quad_frame(g, tris, cut).filter(|f| f.clash.is_empty()) else { return conf.map(|uv| (uv, false)) };
    // Worst 5 % stretch of a valid layout, else None.
    let judge = |uv: &[DVec2]| {
        let (mean, p95, flips) = stretch(g, tris, cut, uv);
        (!flips && layout_check(cut, uv, tris.len()).is_none()).then_some((mean, p95))
    };
    let (cmean, cp95) = conf.as_deref().and_then(judge).unwrap_or((f64::INFINITY, f64::INFINITY));
    let mut best: Option<(Vec<DVec2>, f64)> = None;
    for (level, allow) in [([true, true], (cp95 * STRAIGHT_STRETCH).max(STRAIGHT_FREE)), ([true, false], cp95 * LEVEL_STRETCH), ([false, true], cp95 * LEVEL_STRETCH)] {
        if best.is_some() && level[0] != level[1] {
            break;
        }
        let allow = allow.max(1.3).min(max_stretch);
        // The mean counts too: a big organic patch pulled square stretches
        // everywhere a little.
        let allow_mean = cmean * if level[0] && level[1] { STRAIGHT_MEAN } else { LEVEL_MEAN };
        let Some(uv) = straighten(g, tris, cut, &frame, level) else { continue };
        let p95 = judge(&uv).filter(|(m, _)| *m <= allow_mean).map(|(_, p)| p);
        if let Some(p95) = p95.filter(|p| *p <= allow) {
            if best.as_ref().is_none_or(|b| p95 < b.1) {
                best = Some((uv, p95));
            }
        }
    }
    match best {
        Some((uv, _)) => Some((uv, true)),
        None => conf.map(|uv| (uv, false)),
    }
}

/// Makes a chart a disk with as few cuts as it takes (tubes joined across,
/// closed parts opened). False when it can not be (handles).
fn make_disk(g: &mut Group, tris: &[u32], chart_of: &[u32], c: u32, cuts: &mut usize) -> bool {
    for _ in 0..12 {
        let cut = cut_mesh(g, tris, chart_of, c);
        let (loops, chi) = topology(&cut, tris.len());
        if loops == 1 && chi == 1 {
            return true;
        }
        if (loops == 1 && chi < 1) || !cut_toward_disk(g, tris, chart_of, c, &cut, loops) {
            return false;
        }
        *cuts += 1;
    }
    false
}

/// Charts bigger than this are not joined (their flattening is slow and
/// rarely clean once that big).
const MERGE_MAX_TRIS: usize = 6000;

/// Joins two charts in a copy of the group: the seams between them go (not
/// part borders), so do the cuts that made each a disk (not natural
/// seams), and the union is made a disk and flattened. The flattening when
/// it folds nothing and stretches no more than `max_stretch`.
fn try_merge(g: &Group, chart_of: &[u32], a: &[u32], b: &[u32], natural: &[[bool; 3]], max_stretch: f64) -> Option<(Group, Vec<u32>, Flattened)> {
    if a.len() + b.len() > MERGE_MAX_TRIS {
        return None;
    }
    let mut h = g.clone();
    let c = chart_of[a[0] as usize];
    let other = chart_of[b[0] as usize];
    let mut ids = chart_of.to_vec();
    for &t in b {
        ids[t as usize] = c;
    }
    let mut tris: Vec<u32> = a.iter().chain(b).copied().collect();
    tris.sort_unstable();
    for &t in &tris {
        for k in 0..3 {
            let n = h.adj[t as usize][k];
            if n == NONE || ids[n as usize] != c || h.part[t as usize][k] || !h.seam[t as usize][k] {
                continue;
            }
            let (ct, cn) = (chart_of[t as usize], chart_of[n as usize]);
            let between = (ct == c && cn == other) || (ct == other && cn == c);
            if between || !natural[t as usize][k] {
                h.seam[t as usize][k] = false;
            }
        }
    }
    let mut cuts = 0;
    if !make_disk(&mut h, &tris, &ids, c, &mut cuts) {
        return None;
    }
    let cut = cut_mesh(&h, &tris, &ids, c);
    let (uv, straight) = flatten(&h, &tris, &cut, max_stretch)?;
    let (_, p95, flips) = stretch(&h, &tris, &cut, &uv);
    if flips || p95 > max_stretch || layout_check(&cut, &uv, tris.len()).is_some() {
        return None;
    }
    Some((h, tris, Flattened { uv, vert: cut.vert, straight, p95 }))
}

/// Stretch (worst 5 %) a union of islands may always have.
const MERGE_STRETCH: f64 = 1.45;

/// Islands go back together when the union still flattens cleanly (no
/// fold, no overlap, little stretch): fewer, bigger islands, as an artist
/// leaves them. A small island (a bevel, a rim, a cap) may join a neighbor
/// across a hard edge, and an island much smaller than its neighbor too;
/// part borders stay. Each pass tries every island against its longest
/// shared borders in parallel, then joins the best pairs that do not
/// overlap. Returns the merges made.
fn merge_charts(g: &mut Group, natural: &[[bool; 3]], max_stretch: f64, flat: &mut Flat) -> usize {
    use rayon::prelude::*;
    let total: f64 = (0..g.tris.len()).filter(|t| !g.degen[*t]).map(|t| g.area[t] as f64).sum();
    let mut merged = 0;
    for _pass in 0..10 {
        let (chart_of, list) = charts(g);
        let area: Vec<f64> = list.iter().map(|tris| tris.iter().map(|t| g.area[*t as usize] as f64).sum()).collect();
        // Per chart, the bigger neighbors it may join, by shared length.
        let cands: Vec<(usize, Vec<usize>)> = (0..list.len())
            .filter_map(|c| {
                let mut shared: HashMap<usize, f64> = HashMap::new();
                for &t in &list[c] {
                    for k in 0..3 {
                        let n = g.adj[t as usize][k];
                        if n == NONE || !g.seam[t as usize][k] || g.part[t as usize][k] {
                            continue;
                        }
                        let nc = chart_of[n as usize];
                        if nc == NONE || nc as usize == c {
                            continue;
                        }
                        *shared.entry(nc as usize).or_default() += g.edge_len(t as usize, k) as f64;
                    }
                }
                let small = area[c] < total * 0.03;
                let mut v: Vec<(usize, f64)> = shared
                    .into_iter()
                    .filter(|(nc, _)| (area[c], c) < (area[*nc], *nc) && (small || area[c] <= 0.5 * area[*nc]))
                    .collect();
                v.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
                (!v.is_empty()).then(|| (c, v.into_iter().take(3).map(|(nc, _)| nc).collect()))
            })
            .collect();
        // The union may stretch a little more than the worse of the two (the
        // first rounds allow up to `max_stretch`), never past it.
        let p95 = |c: usize| flat.get(&(list[c][0], list[c].len())).map_or(f64::INFINITY, |f| f.p95);
        let tries: Vec<(usize, usize, Group, Vec<u32>, Flattened)> = cands
            .par_iter()
            .filter_map(|(c, ns)| {
                ns.iter().find_map(|&nc| {
                    let limit = (p95(*c).max(p95(nc)) * 1.05).clamp(MERGE_STRETCH, max_stretch);
                    try_merge(g, &chart_of, &list[*c], &list[nc], natural, limit).map(|(h, tris, f)| (*c, nc, h, tris, f))
                })
            })
            .collect();
        // Smallest first; each chart joins once per pass.
        let mut order: Vec<usize> = (0..tries.len()).collect();
        order.sort_by(|a, b| area[tries[*a].0].total_cmp(&area[tries[*b].0]).then(a.cmp(b)));
        let mut touched = vec![false; list.len()];
        let mut slots: Vec<Option<(usize, usize, Group, Vec<u32>, Flattened)>> = tries.into_iter().map(Some).collect();
        let mut any = false;
        for i in order {
            let (c, nc) = (slots[i].as_ref().unwrap().0, slots[i].as_ref().unwrap().1);
            if touched[c] || touched[nc] {
                continue;
            }
            let (_, _, h, tris, f) = slots[i].take().unwrap();
            g.take(&h, &tris);
            flat.remove(&(list[c][0], list[c].len()));
            flat.remove(&(list[nc][0], list[nc].len()));
            flat.insert((tris[0], tris.len()), f);
            touched[c] = true;
            touched[nc] = true;
            merged += 1;
            any = true;
        }
        if !any {
            break;
        }
    }
    merged
}

/// Counts of what the chart rounds did (debug log).
#[derive(Default)]
struct Tally {
    cuts: usize,
    tips: usize,
    loops: usize,
    splits: usize,
    invalid: usize,
    nodisk: usize,
    nolscm: usize,
    reasons: HashMap<&'static str, usize>,
}

impl Tally {
    fn add(&mut self, o: Tally) {
        self.cuts += o.cuts;
        self.tips += o.tips;
        self.loops += o.loops;
        self.splits += o.splits;
        self.invalid += o.invalid;
        self.nodisk += o.nodisk;
        self.nolscm += o.nolscm;
        for (k, v) in o.reasons {
            *self.reasons.entry(k).or_insert(0) += v;
        }
    }
}

/// One round's work on one chart (in a copy of the group): made a disk and
/// flattened. A chart that folds, overlaps or stretches too much is opened
/// at a tip (at most 8 per chart, `tips` done so far) or else split: along
/// the cheapest edge loop between the overlapping triangles, between them,
/// by surface direction or by position. Returns the flattening when it is
/// kept, whether a tip was opened, and the counts.
fn work_chart(g: &mut Group, tris: &[u32], chart_of: &[u32], c: u32, tips: u32, max_stretch: f64) -> (Option<Flattened>, bool, Tally) {
    let mut n = Tally::default();
    if !make_disk(g, tris, chart_of, c, &mut n.cuts) {
        n.nodisk += 1;
        // Handles or worse: halve the chart and look again.
        n.splits += split_by_normals(g, tris, chart_of, c) as usize;
        return (None, false, n);
    }
    let cut = cut_mesh(g, tris, chart_of, c);
    let Some((uv, straight)) = flatten(g, tris, &cut, max_stretch) else {
        n.nolscm += 1;
        if tris.len() > 1 {
            n.splits += split_by_normals(g, tris, chart_of, c) as usize;
        }
        return (None, false, n);
    };
    let (_, p95, flips) = stretch(g, tris, &cut, &uv);
    let checked = if flips { Some(("flipped", None)) } else { layout_check(&cut, &uv, tris.len()) };
    let problem = checked.map(|(p, _)| p);
    let pair = checked.and_then(|(_, pair)| pair);
    let valid = problem.is_none();
    if let Some(p) = problem {
        n.invalid += 1;
        *n.reasons.entry(p).or_insert(0usize) += 1;
    }
    if valid && (p95 <= max_stretch || tris.len() <= 8) {
        return (Some(Flattened { uv, vert: cut.vert, straight, p95 }), false, n);
    }
    // First a pelt seam to a tip; splitting is the last resort.
    if tips < 8 && tip_cut(g, tris, chart_of, c, &cut) {
        n.tips += 1;
        return (None, true, n);
    }
    let split = if pair.is_some_and(|(i, j)| split_by_loop(g, tris, chart_of, c, &cut, Some((i, j)))) {
        n.loops += 1;
        true
    } else {
        pair.is_some_and(|(i, j)| split_between(g, tris, chart_of, c, i, j))
            || (!valid && pair.is_none() && split_by_loop(g, tris, chart_of, c, &cut, None))
            || split_by_normals(g, tris, chart_of, c)
            || (!valid && split_by_position(g, tris, chart_of, c))
    };
    if split {
        n.splits += 1;
        (None, false, n)
    } else {
        (Some(Flattened { uv, vert: cut.vert, straight, p95 }), false, n)
    }
}

/// Charts: triangles connected across edges that are not seams.
fn charts(g: &Group) -> (Vec<u32>, Vec<Vec<u32>>) {
    let n = g.tris.len();
    let mut chart_of = vec![NONE; n];
    let mut list: Vec<Vec<u32>> = Vec::new();
    for s in 0..n {
        if chart_of[s] != NONE || g.degen[s] {
            continue;
        }
        let c = list.len() as u32;
        let mut stack = vec![s as u32];
        chart_of[s] = c;
        let mut tris = Vec::new();
        while let Some(t) = stack.pop() {
            tris.push(t);
            for k in 0..3 {
                let nb = g.adj[t as usize][k];
                if nb != NONE && !g.seam[t as usize][k] && chart_of[nb as usize] == NONE {
                    chart_of[nb as usize] = c;
                    stack.push(nb);
                }
            }
        }
        // Sorted: a chart's order (and so its flattening's vertex numbering)
        // depends on its triangles only, not on the walk.
        tris.sort_unstable();
        list.push(tris);
    }
    (chart_of, list)
}

/// Least squares conformal map of a disk: UV per local vertex (world
/// units), or None when the solve fails.
fn lscm(g: &Group, tris: &[u32], cut: &Cut) -> Option<Vec<DVec2>> {
    let nv = cut.nverts;
    if nv < 3 {
        return None;
    }
    // Pins: the two most distant border vertices.
    let border: Vec<u32> = {
        let mut v: Vec<u32> = cut.border.iter().map(|(a, _)| *a).collect();
        v.sort_unstable();
        v.dedup();
        v
    };
    let p = |v: u32| g.pos[cut.welded[v as usize] as usize];
    let far = |from: u32, set: &[u32]| *set.iter().max_by(|a, b| p(**a).distance(p(from)).total_cmp(&p(**b).distance(p(from)))).unwrap();
    let cand: Vec<u32> = if border.len() >= 2 { border } else { (0..nv as u32).collect() };
    let a = far(cand[0], &cand);
    let b = far(a, &cand);
    if a == b {
        return None;
    }
    let d = p(a).distance(p(b)) as f64;
    // Unknowns: u then v of every free vertex.
    let mut col = vec![usize::MAX; nv];
    let mut nfree = 0;
    for (v, c) in col.iter_mut().enumerate() {
        if v as u32 != a && v as u32 != b {
            *c = nfree;
            nfree += 1;
        }
    }
    let pinned = |v: u32| -> Option<DVec2> {
        if v == a {
            Some(DVec2::ZERO)
        } else if v == b {
            Some(DVec2::new(d, 0.0))
        } else {
            None
        }
    };
    let n = nfree * 2;
    let mut rows = SparseRows::new(n);
    let mut rhs = Vec::new();
    for (i, &t) in tris.iter().enumerate() {
        let Some(ws) = conformal(g, t) else { continue };
        // Real row: a u - b v; imaginary row: b u + a v.
        for imag in [false, true] {
            let mut r = 0.0;
            for (j, wj) in ws.iter().enumerate() {
                let v = cut.vert[i * 3 + j];
                let (cu, cv) = if imag { (wj.y, wj.x) } else { (wj.x, -wj.y) };
                match pinned(v) {
                    Some(uv) => r -= cu * uv.x + cv * uv.y,
                    None => {
                        let c = col[v as usize];
                        rows.push(c, cu);
                        rows.push(c + nfree, cv);
                    }
                }
            }
            rows.end_row();
            rhs.push(r);
        }
    }
    let x = solve(&rows, &rhs)?;
    Some((0..nv).map(|v| pinned(v as u32).unwrap_or_else(|| DVec2::new(x[col[v]], x[col[v] + nfree]))).collect())
}

/// The conformal (LSCM) weights of a triangle, per corner: with
/// U_j = u_j + i v_j, sum_j W_j U_j = 0 when the triangle maps to UV by a
/// similarity (Levy et al.), scaled by one over the square root of its
/// doubled area so every triangle counts by its size. None without area.
fn conformal(g: &Group, t: u32) -> Option<[DVec2; 3]> {
    let w = g.tris[t as usize];
    let (p0, p1, p2) = (g.pos[w[0] as usize], g.pos[w[1] as usize], g.pos[w[2] as usize]);
    let e1 = p1 - p0;
    let nrm = e1.cross(p2 - p0);
    let area2 = nrm.length() as f64;
    if area2 < 1e-12 || e1.length() < 1e-9 {
        return None;
    }
    let x = e1.normalize();
    let y = nrm.normalize().cross(x);
    let q = [DVec2::ZERO, DVec2::new(e1.length() as f64, 0.0), DVec2::new((p2 - p0).dot(x) as f64, (p2 - p0).dot(y) as f64)];
    let s = 1.0 / area2.sqrt();
    // W_j = q_{j+2} - q_{j+1}.
    Some([(q[2] - q[1]) * s, (q[0] - q[2]) * s, (q[1] - q[0]) * s])
}

/// Quads of a chart: polygons of exactly two chart triangles joined across
/// their diagonal, as four local vertices in winding order, with the local
/// indices of the two triangles.
fn chart_quads(g: &Group, tris: &[u32], cut: &Cut) -> Vec<([u32; 4], [usize; 2])> {
    let mut by_poly: HashMap<u32, Vec<usize>> = HashMap::new();
    for (i, &t) in tris.iter().enumerate() {
        by_poly.entry(g.poly[t as usize]).or_default().push(i);
    }
    let mut polys: Vec<(&u32, &Vec<usize>)> = by_poly.iter().filter(|(_, v)| v.len() == 2).collect();
    polys.sort_unstable_by_key(|(p, _)| **p);
    let mut out = Vec::new();
    for (_, list) in polys {
        let (i, j) = (list[0], list[1]);
        let a = [cut.vert[i * 3], cut.vert[i * 3 + 1], cut.vert[i * 3 + 2]];
        let b = [cut.vert[j * 3], cut.vert[j * 3 + 1], cut.vert[j * 3 + 2]];
        // The corner of each triangle the other lacks.
        let (Some(ka), Some(kb)) = ((0..3).find(|k| !b.contains(&a[*k])), (0..3).find(|k| !a.contains(&b[*k]))) else { continue };
        let (x, p, q) = (a[ka], a[(ka + 1) % 3], a[(ka + 2) % 3]);
        // The second triangle runs the diagonal backward: q, p, y.
        if b[(kb + 1) % 3] != q || b[(kb + 2) % 3] != p {
            continue;
        }
        let quad = [x, p, b[kb], q];
        if (0..4).any(|m| (m + 1..4).any(|n| quad[m] == quad[n])) {
            continue;
        }
        // Nearly flat: the two halves face the same way.
        if g.fnormal[tris[i] as usize].dot(g.fnormal[tris[j] as usize]) < 0.5 {
            continue;
        }
        // A quad closing to a point (the tip of a lock, a pole) is a
        // triangle in all but name: its sides can not run parallel.
        let len = |k: usize| g.pos[cut.welded[quad[k] as usize] as usize].distance(g.pos[cut.welded[quad[(k + 1) % 4] as usize] as usize]);
        let ratio = |a: f32, b: f32| a.min(b) / a.max(b).max(1e-30);
        if ratio(len(0), len(2)) < 0.25 || ratio(len(1), len(3)) < 0.25 {
            continue;
        }
        out.push((quad, [i, j]));
    }
    out
}

/// The quads of a chart with the direction of each of their edges.
struct QuadFrame {
    quads: Vec<([u32; 4], [usize; 2])>,
    /// Per quad, how far its corners' edges turn: the edge leaving corner k
    /// runs (k + turn) % 4, with 0 = +u, 1 = +v, 2 = -u, 3 = -v
    /// (counter-clockwise like the triangles).
    turn: Vec<u8>,
    /// Edges (local vertex pairs) whose two quads disagree.
    clash: std::collections::HashSet<(u32, u32)>,
}

/// Which way the quad edges of a chart run, as "follow quads" finds it: the
/// directions spread from quad to quad across shared edges (a neighbor runs
/// the shared edge backward), from the biggest quad. Around an irregular
/// vertex inside the chart they disagree. None when quads cover too little
/// of the chart.
fn quad_frame(g: &Group, tris: &[u32], cut: &Cut) -> Option<QuadFrame> {
    let quads = chart_quads(g, tris, cut);
    let area: f64 = tris.iter().map(|t| g.area[*t as usize] as f64).sum();
    let qarea = |q: &([u32; 4], [usize; 2])| (g.area[tris[q.1[0]] as usize] + g.area[tris[q.1[1]] as usize]) as f64;
    if quads.len() < 2 || quads.iter().map(qarea).sum::<f64>() < 0.6 * area {
        return None;
    }
    // Quads by edge (local vertex pair): (quad, corner the edge leaves).
    let mut by_edge: HashMap<(u32, u32), Vec<(usize, usize)>> = HashMap::new();
    for (qi, (c, _)) in quads.iter().enumerate() {
        for k in 0..4 {
            let (a, b) = (c[k], c[(k + 1) % 4]);
            by_edge.entry((a.min(b), a.max(b))).or_default().push((qi, k));
        }
    }
    let mut turn = vec![u8::MAX; quads.len()];
    let mut roots: Vec<usize> = (0..quads.len()).collect();
    roots.sort_by(|a, b| qarea(&quads[*b]).total_cmp(&qarea(&quads[*a])).then(a.cmp(b)));
    for root in roots {
        if turn[root] != u8::MAX {
            continue;
        }
        turn[root] = 0;
        let mut queue = std::collections::VecDeque::from([root]);
        while let Some(q) = queue.pop_front() {
            let c = quads[q].0;
            for k in 0..4 {
                let (a, b) = (c[k], c[(k + 1) % 4]);
                let d = (k as u8 + turn[q]) % 4;
                for &(o, ko) in &by_edge[&(a.min(b), a.max(b))] {
                    if o != q && turn[o] == u8::MAX {
                        turn[o] = (d + 2 + 4 - ko as u8) % 4;
                        queue.push_back(o);
                    }
                }
            }
        }
    }
    let dir = |q: usize, k: usize| (k as u8 + turn[q]) % 4;
    let clash = by_edge
        .iter()
        .filter(|(_, l)| l.len() == 2 && dir(l[0].0, l[0].1) != (dir(l[1].0, l[1].1) + 2) % 4)
        .map(|(e, _)| *e)
        .collect();
    Some(QuadFrame { quads, turn, clash })
}

/// Straightens a chart's quads: with `level[0]`, the quad edges running
/// along u (rows) become horizontal; with `level[1]`, those along v
/// (columns) vertical. Both make a strip (a limb, a belt) a rectangle and a
/// patch a grid; one alone keeps a tapering strip (a hair lock) straight
/// but lets it narrow. Every edge keeps its surface length as well as it
/// can (least squares averages a row's lengths) and every triangle its
/// shape (conformal terms, lighter on quads), so nothing is left loose.
/// Edges whose quads disagree get no direction.
fn straighten(g: &Group, tris: &[u32], cut: &Cut, frame: &QuadFrame, level: [bool; 2]) -> Option<Vec<DVec2>> {
    // Unknowns: u then v of every vertex but the first (pinned at 0).
    let nv = cut.nverts;
    let nfree = nv - 1;
    let col = |v: u32| (v as usize).checked_sub(1);
    let mut rows = SparseRows::new(nfree * 2);
    let mut rhs = Vec::new();
    let mut diff = |rows: &mut SparseRows, a: u32, b: u32, axis: usize, target: f64, w: f64| {
        // w (x_b - x_a - target) on one axis.
        if let Some(c) = col(b) {
            rows.push(c + axis * nfree, w);
        }
        if let Some(c) = col(a) {
            rows.push(c + axis * nfree, -w);
        }
        rows.end_row();
        rhs.push(w * target);
    };
    // Along an edge: its length; across: nothing, firmly (straight lines).
    const ALONG: f64 = 1.0;
    const ACROSS: f64 = 10.0;
    let both = level[0] && level[1];
    let mut in_quad = vec![false; tris.len()];
    for (qi, (c, [i, j])) in frame.quads.iter().enumerate() {
        in_quad[*i] = true;
        in_quad[*j] = true;
        for k in 0..4 {
            let (a, b) = (c[k], c[(k + 1) % 4]);
            if frame.clash.contains(&(a.min(b), a.max(b))) {
                continue;
            }
            let len = g.pos[cut.welded[a as usize] as usize].distance(g.pos[cut.welded[b as usize] as usize]) as f64;
            let d = (k as u8 + frame.turn[qi]) % 4;
            let (along, across) = if d % 2 == 0 { (0, 1) } else { (1, 0) };
            let sign = if d < 2 { 1.0 } else { -1.0 };
            diff(&mut rows, a, b, along, sign * len, if both { ALONG } else { 0.3 * ALONG });
            if level[(d % 2) as usize] {
                diff(&mut rows, a, b, across, 0.0, ACROSS);
            }
        }
    }
    for (i, &t) in tris.iter().enumerate() {
        let w = if !in_quad[i] { 1.0 } else if both && frame.clash.is_empty() { 0.3 } else { 1.0 };
        let Some(ws) = conformal(g, t) else { continue };
        for imag in [false, true] {
            for (j, wj) in ws.iter().enumerate() {
                if let Some(c) = col(cut.vert[i * 3 + j]) {
                    let (cu, cv) = if imag { (wj.y, wj.x) } else { (wj.x, -wj.y) };
                    rows.push(c, cu * w);
                    rows.push(c + nfree, cv * w);
                }
            }
            rows.end_row();
            rhs.push(0.0);
        }
    }
    let x = solve(&rows, &rhs)?;
    Some((0..nv as u32).map(|v| col(v).map_or(DVec2::ZERO, |c| DVec2::new(x[c], x[c + nfree]))).collect())
}

/// Whether two UV triangles overlap (separating axis test, with a small
/// tolerance so triangles that only touch do not count).
fn tri_overlap(a: &[DVec2; 3], b: &[DVec2; 3], eps: f64) -> bool {
    for (p, q) in [(a, b), (b, a)] {
        for k in 0..3 {
            let e = p[(k + 1) % 3] - p[k];
            let n = DVec2::new(-e.y, e.x);
            let (pa, pb) = p.iter().fold((f64::MAX, f64::MIN), |(lo, hi), v| (lo.min(n.dot(*v)), hi.max(n.dot(*v))));
            let (qa, qb) = q.iter().fold((f64::MAX, f64::MIN), |(lo, hi), v| (lo.min(n.dot(*v)), hi.max(n.dot(*v))));
            let tol = eps * n.length();
            if pb <= qa + tol || qb <= pa + tol {
                return false;
            }
        }
    }
    true
}

/// The packer keeps an island as it is only when no triangle is flipped or
/// without area and no two overlap: the same test here, so a chart that
/// fails is split our way instead of being unwrapped again in pieces.
fn valid_layout(cut: &Cut, uv: &[DVec2], ntris: usize) -> bool {
    layout_problem(cut, uv, ntris).is_none()
}

/// Why a flattened chart is not a valid island: "degenerate", "thin"
/// (a triangle without UV area) or "overlap".
fn layout_problem(cut: &Cut, uv: &[DVec2], ntris: usize) -> Option<&'static str> {
    layout_check(cut, uv, ntris).map(|(p, _)| p)
}

/// `layout_problem` with the two overlapping triangles (local indices).
fn layout_check(cut: &Cut, uv: &[DVec2], ntris: usize) -> Option<(&'static str, Option<(usize, usize)>)> {
    let tri = |i: usize| [uv[cut.vert[i * 3] as usize], uv[cut.vert[i * 3 + 1] as usize], uv[cut.vert[i * 3 + 2] as usize]];
    let areas: Vec<f64> = (0..ntris)
        .map(|i| {
            let t = tri(i);
            (t[1] - t[0]).perp_dot(t[2] - t[0]) * 0.5
        })
        .collect();
    let total: f64 = areas.iter().sum();
    if !(total.abs() > 0.0) || uv.iter().any(|p| !p.is_finite()) {
        return Some(("degenerate", None));
    }
    let sign = total.signum();
    if areas.iter().any(|a| a * sign <= total.abs() * 1e-10) {
        return Some(("thin", None));
    }
    if ntris < 2 {
        return None;
    }
    // Grid of triangle boxes; pairs sharing no vertex are tested.
    let lo = uv.iter().fold(DVec2::splat(f64::MAX), |m, p| m.min(*p));
    let hi = uv.iter().fold(DVec2::splat(f64::MIN), |m, p| m.max(*p));
    let cell = (2.0 * (total.abs() / ntris as f64).sqrt()).max((hi - lo).max_element() / 1024.0);
    if !(cell > 0.0) {
        return Some(("degenerate", None));
    }
    let mut grid: HashMap<(i32, i32), Vec<u32>> = HashMap::new();
    let eps = cell * 1e-6;
    for i in 0..ntris {
        let t = tri(i);
        let bl = t.iter().fold(DVec2::splat(f64::MAX), |m, p| m.min(*p));
        let bh = t.iter().fold(DVec2::splat(f64::MIN), |m, p| m.max(*p));
        let (x0, y0) = (((bl.x - lo.x) / cell) as i32, ((bl.y - lo.y) / cell) as i32);
        let (x1, y1) = (((bh.x - lo.x) / cell) as i32, ((bh.y - lo.y) / cell) as i32);
        for y in y0..=y1 {
            for x in x0..=x1 {
                let list = grid.entry((x, y)).or_default();
                for &j in list.iter() {
                    let j = j as usize;
                    let shared = (0..3).any(|a| (0..3).any(|b| cut.vert[i * 3 + a] == cut.vert[j * 3 + b]));
                    if !shared && tri_overlap(&t, &tri(j), eps) {
                        return Some(("overlap", Some((i, j))));
                    }
                }
                list.push(i as u32);
            }
        }
    }
    None
}

/// Area stretch of a flattened chart: (mean, worst 5 %), area weighted, and
/// whether triangles flip or are crushed (CRUSHED times off the chart's
/// average: a foot or a fingertip squeezed to a dot by a long chart, which
/// the worst 5 % misses when it is small). Infinity when degenerate.
fn stretch(g: &Group, tris: &[u32], cut: &Cut, uv: &[DVec2]) -> (f64, f64, bool) {
    let mut a3 = Vec::with_capacity(tris.len());
    let mut a2 = Vec::with_capacity(tris.len());
    for (i, &t) in tris.iter().enumerate() {
        let u = [uv[cut.vert[i * 3] as usize], uv[cut.vert[i * 3 + 1] as usize], uv[cut.vert[i * 3 + 2] as usize]];
        a2.push((u[1] - u[0]).perp_dot(u[2] - u[0]) * 0.5);
        a3.push(g.area[t as usize] as f64);
    }
    let (s3, s2): (f64, f64) = (a3.iter().sum(), a2.iter().sum());
    if s3 <= 0.0 || s2.abs() <= 1e-18 {
        return (f64::INFINITY, f64::INFINITY, true);
    }
    let sign = s2.signum();
    let flips = a2.iter().zip(&a3).any(|(x, y)| *y > s3 * 1e-7 && x * sign <= 0.0);
    let mut v: Vec<(f64, f64)> = a2
        .iter()
        .zip(&a3)
        .filter(|(_, y)| **y > 0.0)
        .map(|(x, y)| {
            let r = ((x * sign) / s2.abs()) / (y / s3);
            (if r > 0.0 { r.max(1.0 / r) } else { f64::INFINITY }, *y)
        })
        .collect();
    let mean = v.iter().map(|(s, w)| s.min(1e3) * w).sum::<f64>() / s3;
    let crushed = v.iter().any(|(s, w)| *s > CRUSHED && *w > s3 * 1e-5);
    v.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut acc = 0.0;
    let mut p95 = 1.0;
    for (s, w) in &v {
        acc += w;
        p95 = *s;
        if acc >= s3 * 0.95 {
            break;
        }
    }
    (mean, p95, flips || crushed)
}

/// Area stretch no triangle of a chart may have.
const CRUSHED: f64 = 10.0;

// ------------------------------------------------------------ solver

/// Row-compressed sparse matrix built row by row.
struct SparseRows {
    ptr: Vec<usize>,
    cols: Vec<usize>,
    vals: Vec<f64>,
    ncols: usize,
}

impl SparseRows {
    fn new(ncols: usize) -> Self {
        SparseRows { ptr: vec![0], cols: Vec::new(), vals: Vec::new(), ncols }
    }
    fn push(&mut self, c: usize, v: f64) {
        self.cols.push(c);
        self.vals.push(v);
    }
    fn end_row(&mut self) {
        self.ptr.push(self.cols.len());
    }
}

/// Least squares `min |A x - b|` through the normal equations and a sparse
/// Cholesky factorization.
fn solve(a: &SparseRows, b: &[f64]) -> Option<Vec<f64>> {
    use faer::dyn_stack::{MemBuffer, MemStack};
    use faer::sparse::linalg::cholesky::{factorize_symbolic_cholesky, CholeskySymbolicParams, SymmetricOrdering};
    use faer::sparse::{SparseColMat, SymbolicSparseColMat};
    use faer::{Conj, MatMut, Par, Side};
    let n = a.ncols;
    if n == 0 {
        return Some(Vec::new());
    }
    // A^T b and the lower triangle of A^T A, column by column.
    let mut atb = vec![0.0; n];
    let mut by_col: Vec<Vec<(usize, f64)>> = vec![Vec::new(); n];
    for r in 0..a.ptr.len() - 1 {
        for k in a.ptr[r]..a.ptr[r + 1] {
            by_col[a.cols[k]].push((r, a.vals[k]));
            atb[a.cols[k]] += a.vals[k] * b[r];
        }
    }
    let mut col_ptr = vec![0usize];
    let mut row_idx = Vec::new();
    let mut vals = Vec::new();
    let mut acc = vec![0.0; n];
    let mut mark = vec![usize::MAX; n];
    let mut touched = Vec::new();
    for j in 0..n {
        touched.clear();
        for &(r, arj) in &by_col[j] {
            for k in a.ptr[r]..a.ptr[r + 1] {
                let i = a.cols[k];
                if i < j {
                    continue;
                }
                if mark[i] != j {
                    mark[i] = j;
                    acc[i] = 0.0;
                    touched.push(i);
                }
                acc[i] += arj * a.vals[k];
            }
        }
        touched.sort_unstable();
        for &i in &touched {
            row_idx.push(i);
            vals.push(acc[i]);
        }
        col_ptr.push(row_idx.len());
    }
    let symbolic = SymbolicSparseColMat::<usize>::new_checked(n, n, col_ptr, None, row_idx);
    let m = SparseColMat::<usize, f64>::new(symbolic, vals);
    let sym = factorize_symbolic_cholesky(m.symbolic(), Side::Lower, SymmetricOrdering::Amd, CholeskySymbolicParams::default()).ok()?;
    let par = Par::Seq;
    let mut lvals = vec![0.0; sym.len_val()];
    let scratch = sym.factorize_numeric_llt_scratch::<f64>(par, Default::default()).or(sym.solve_in_place_scratch::<f64>(1, par));
    let mut buf = MemBuffer::new(scratch);
    let llt = sym.factorize_numeric_llt(&mut lvals, m.as_ref(), Side::Lower, Default::default(), par, MemStack::new(&mut buf), Default::default()).ok()?;
    llt.solve_in_place_with_conj(Conj::No, MatMut::from_column_major_slice_mut(&mut atb, n, 1), par, MemStack::new(&mut buf));
    atb.iter().all(|v| v.is_finite()).then_some(atb)
}

// ------------------------------------------------------------ driver

/// Quality of a UV layout: covered fraction of the UV square, and area
/// stretch (area weighted mean, worst 5 %; 1 = none).
pub(crate) fn measure(positions: &[[f32; 3]], uvs: &[[f32; 2]], idx: &[u32]) -> (f32, f32, f32) {
    let (mut a2s, mut a3s) = (0.0f64, 0.0f64);
    let mut per: Vec<(f64, f64)> = Vec::with_capacity(idx.len() / 3);
    for t in idx.chunks_exact(3) {
        let (a, b, c) = (uvs[t[0] as usize], uvs[t[1] as usize], uvs[t[2] as usize]);
        let a2 = (((b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1])) as f64 * 0.5).abs();
        let (p0, p1, p2) = (Vec3::from(positions[t[0] as usize]), Vec3::from(positions[t[1] as usize]), Vec3::from(positions[t[2] as usize]));
        let a3 = ((p1 - p0).cross(p2 - p0).length() * 0.5) as f64;
        a2s += a2;
        a3s += a3;
        per.push((a2, a3));
    }
    if a2s <= 0.0 || a3s <= 0.0 {
        return (0.0, 0.0, 0.0);
    }
    let mut v: Vec<(f64, f64)> = per
        .iter()
        .filter(|(_, a3)| *a3 > 0.0)
        .map(|(a2, a3)| {
            let r = (a2 / a2s) / (a3 / a3s);
            (if r > 0.0 { r.max(1.0 / r).min(100.0) } else { 100.0 }, *a3)
        })
        .collect();
    let mean = v.iter().map(|(s, w)| s * w).sum::<f64>() / a3s;
    v.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut acc = 0.0;
    let mut p95 = 1.0;
    for (s, w) in &v {
        acc += w;
        p95 = *s;
        if acc >= a3s * 0.95 {
            break;
        }
    }
    (a2s as f32, mean as f32, p95 as f32)
}

/// Packs the islands several ways and keeps the layout that fills the most
/// of the UV square (the packer never overlaps islands): shape packing with
/// each placement order, free turning both from the islands' own direction
/// and from their tightest bounding box when turning is allowed, and the
/// row folding packer. Upright islands keep their direction (only those
/// that lie flat may turn over). Returns the atlas and what won.
fn pack_best(input: &uvunwrap::MeshInput, opts: &SmartOptions) -> Result<(uvunwrap::Atlas, String), uvunwrap::Error> {
    use rayon::prelude::*;
    use uvunwrap::{PackOrder, Packing};
    let mut base = uvunwrap::Options::default();
    base.density = uvunwrap::Density::Auto { page_size: opts.page };
    base.padding = opts.padding;
    base.allow_rotation = !opts.upright;
    base.keep_orientation = opts.upright;
    let mut tries = Vec::new();
    let keeps: &[bool] = if opts.upright { &[true] } else { &[false, true] };
    for &keep in keeps {
        for order in [PackOrder::Perimeter, PackOrder::Area, PackOrder::Height, PackOrder::LongSide] {
            let mut o = base;
            o.keep_orientation = keep;
            o.order = order;
            tries.push((o, format!("shape, {order:?} first{}", if keep || opts.upright { "" } else { ", boxed" })));
        }
        let mut o = base;
        o.keep_orientation = keep;
        o.packing = Packing::Fast;
        tries.push((o, format!("rows{}", if keep || opts.upright { "" } else { ", boxed" })));
    }
    // Share of the square covered: the page is cropped to its used height.
    let fill = |a: &uvunwrap::Atlas| a.utilization as f64 * a.height as f64 / a.width.max(1) as f64;
    let results: Vec<(Result<uvunwrap::Atlas, uvunwrap::Error>, String)> = tries.into_par_iter().map(|(o, how)| (uvunwrap::generate(&[*input], &o), how)).collect();
    let mut best: Option<(uvunwrap::Atlas, String)> = None;
    let mut error = None;
    for (r, how) in results {
        match r {
            Ok(a) if a.page_count <= 1 && best.as_ref().is_none_or(|(b, _)| fill(&a) > fill(b)) => best = Some((a, how)),
            Ok(_) => {}
            Err(e) => error = Some(e),
        }
    }
    match (best, error) {
        (Some((a, how)), _) => {
            let pct = fill(&a) * 100.0;
            Ok((a, format!("{how} ({pct:.1} %)")))
        }
        (None, Some(e)) => Err(e),
        (None, None) => uvunwrap::generate(&[*input], &base).map(|a| (a, "default".into())),
    }
}

/// Openness of each welded vertex (fraction of hemisphere rays that leave
/// the model): 0 in deep folds, 1 on open convex areas.
fn openness(pos: &[Vec3], normal: &[Vec3], all_tris: Vec<[Vec3; 3]>, diag: f32) -> Vec<f32> {
    use rayon::prelude::*;
    let bvh = crate::bake::Bvh::build(all_tris);
    const N: u32 = 24;
    let reach = diag * 0.3;
    pos.par_iter()
        .zip(normal.par_iter())
        .map(|(p, n)| {
            let n = n.normalize_or(Vec3::Y);
            let t = n.any_orthonormal_vector();
            let b = n.cross(t);
            let mut open = 0;
            for k in 0..N {
                // Cosine-weighted directions on a fixed spiral.
                let u1 = (k as f32 + 0.5) / N as f32;
                let phi = k as f32 * 2.399_963;
                let r = u1.sqrt();
                let d = t * (r * phi.cos()) + b * (r * phi.sin()) + n * (1.0 - u1).sqrt();
                if bvh.trace(*p + n * diag * 1e-4, d, reach, true).is_none() {
                    open += 1;
                }
            }
            open as f32 / N as f32
        })
        .collect()
}

/// Unwraps every group of `raw` with smart seams. Returns per group reports.
pub(crate) fn unwrap(raw: &mut RawMesh, opts: &SmartOptions) -> Result<Vec<UvReport>> {
    let bmin = raw.positions.iter().fold(Vec3::splat(f32::MAX), |m, p| m.min(Vec3::from(*p)));
    let bmax = raw.positions.iter().fold(Vec3::splat(f32::MIN), |m, p| m.max(Vec3::from(*p)));
    let diag = (bmax - bmin).length().max(1e-6);
    // Weld the whole mesh by exact position, as the packer does (a looser
    // weld would join corners it keeps apart).
    let mut weld: HashMap<[u32; 3], u32> = HashMap::new();
    let mut wid = vec![0u32; raw.positions.len()];
    let mut wpos: Vec<Vec3> = Vec::new();
    for (i, p) in raw.positions.iter().enumerate() {
        let key = [(p[0] + 0.0).to_bits(), (p[1] + 0.0).to_bits(), (p[2] + 0.0).to_bits()];
        let n = wpos.len() as u32;
        wid[i] = *weld.entry(key).or_insert_with(|| {
            wpos.push(Vec3::from(*p));
            n
        });
    }
    // Vertex normals (welded) and, for hidden seams, their openness.
    let mut wnorm = vec![Vec3::ZERO; wpos.len()];
    let mut all_tris = Vec::new();
    for (_, idx, _) in &raw.groups {
        for t in idx.chunks_exact(3) {
            let p = [wpos[wid[t[0] as usize] as usize], wpos[wid[t[1] as usize] as usize], wpos[wid[t[2] as usize] as usize]];
            let n = (p[1] - p[0]).cross(p[2] - p[0]);
            for c in t {
                wnorm[wid[*c as usize] as usize] += n;
            }
            all_tris.push(p);
        }
    }
    let open = if opts.hide_seams { openness(&wpos, &wnorm, all_tris, diag) } else { vec![1.0; wpos.len()] };
    let hard = opts.hard_angle.to_radians().cos();

    let mut reports = Vec::new();
    let has_colors = !raw.colors.is_empty();
    let (mut positions, mut normals, mut colors, mut uvs) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for gi in 0..raw.groups.len() {
        let idx = raw.groups[gi].1.clone();
        let objs = raw.groups[gi].2.clone();
        let ntri = idx.len() / 3;
        if ntri == 0 {
            continue;
        }
        let polys: Vec<u32> = raw.polys.get(gi).filter(|p| p.len() == ntri).cloned().unwrap_or_else(|| (0..ntri as u32).collect());
        // ---- group topology
        let mut g = Geo {
            pos: wpos.clone(),
            tris: Vec::with_capacity(ntri),
            raw: Vec::with_capacity(ntri),
            poly: polys.clone(),
            adj: vec![[NONE; 3]; ntri],
            adj_k: vec![[0; 3]; ntri],
            fnormal: Vec::with_capacity(ntri),
            area: Vec::with_capacity(ntri),
            cost: vec![[1.0; 3]; ntri],
            degen: vec![false; ntri],
            part: vec![[false; 3]; ntri],
        };
        let mut seam = vec![[false; 3]; ntri];
        for t in idx.chunks_exact(3) {
            let w = [wid[t[0] as usize], wid[t[1] as usize], wid[t[2] as usize]];
            let (p0, p1, p2) = (wpos[w[0] as usize], wpos[w[1] as usize], wpos[w[2] as usize]);
            let n = (p1 - p0).cross(p2 - p0);
            g.area.push(n.length() * 0.5);
            g.fnormal.push(n.normalize_or_zero());
            g.tris.push(w);
            g.raw.push([t[0], t[1], t[2]]);
        }
        // The packer's rule: a repeated corner, or a height under a millionth
        // of the model's size (it drops those; any other triangle needs room).
        for t in 0..ntri {
            let w = g.tris[t];
            let (a, b, c) = (wpos[w[0] as usize], wpos[w[1] as usize], wpos[w[2] as usize]);
            let longest = (b - a).length().max((c - b).length()).max((a - c).length());
            g.degen[t] = w[0] == w[1] || w[1] == w[2] || w[0] == w[2] || !(longest > 0.0) || 2.0 * g.area[t] / longest <= diag * 1e-6;
        }
        let mut edges: HashMap<(u32, u32), Vec<(u32, u8)>> = HashMap::new();
        for (t, w) in g.tris.iter().enumerate() {
            if g.degen[t] {
                continue;
            }
            for k in 0..3 {
                let (a, b) = (w[k], w[(k + 1) % 3]);
                if a != b {
                    edges.entry((a.min(b), a.max(b))).or_default().push((t as u32, k as u8));
                }
            }
        }
        let mut keys: Vec<(u32, u32)> = edges.keys().copied().collect();
        keys.sort_unstable();
        for key in keys {
            let list = &edges[&key];
            // Consistent orientation only: a neighbor runs the edge backward.
            // Where more than two triangles meet (a fin, a doubled surface),
            // each pairs with the backward one it continues most smoothly.
            let (fwd, back): (Vec<(u32, u8)>, Vec<(u32, u8)>) = list.iter().partition(|(t, k)| g.edge(*t as usize, *k as usize).0 == key.0);
            let mut used = vec![false; back.len()];
            for &(t1, k1) in &fwd {
                let n1 = g.fnormal[t1 as usize];
                let Some(j) = (0..back.len()).filter(|j| !used[*j]).max_by(|a, b| n1.dot(g.fnormal[back[*a].0 as usize]).total_cmp(&n1.dot(g.fnormal[back[*b].0 as usize])).then(b.cmp(a))) else { break };
                used[j] = true;
                let (t2, k2) = back[j];
                g.adj[t1 as usize][k1 as usize] = t2;
                g.adj_k[t1 as usize][k1 as usize] = k2;
                g.adj[t2 as usize][k2 as usize] = t1;
                g.adj_k[t2 as usize][k2 as usize] = k1;
            }
        }
        // ---- natural seams and cut costs
        let mut why_seam = [0usize; 8];
        let normal_of = |r: u32| Vec3::from(raw.normals.get(r as usize).copied().unwrap_or([0.0, 1.0, 0.0]));
        for t in 0..ntri {
            for k in 0..3 {
                let n = g.adj[t][k];
                if n == NONE || (n as usize) < t {
                    continue;
                }
                let n = n as usize;
                let kn = g.adj_k[t][k] as usize;
                let cosd = g.fnormal[t].dot(g.fnormal[n]);
                let crease = ((1.0 - cosd) / (1.0 - 0.5)).clamp(0.0, 1.0);
                let (a, b) = g.edge(t, k);
                let o = 0.5 * (open[a as usize] + open[b as usize]);
                // Hidden (occluded) and creased edges are cheap to cut.
                let c = (0.05 + o * o) * (1.0 - 0.75 * crease);
                g.cost[t][k] = c;
                g.cost[n][kn] = c;
                if g.poly[t] == g.poly[n] {
                    continue;
                }
                // Split normals in the file: corners of one welded vertex with
                // different normals on the two sides.
                let split = [(k, (kn + 1) % 3), ((k + 1) % 3, kn)].iter().any(|&(ct, cn)| {
                    let (rt, rn) = (g.raw[t][ct], g.raw[n][cn]);
                    rt != rn && normal_of(rt).dot(normal_of(rn)) < 0.995
                });
                let part = objs.get(t) != objs.get(n);
                if part {
                    g.part[t][k] = true;
                    g.part[n][kn] = true;
                }
                if cosd < hard || split || part {
                    seam[t][k] = true;
                    seam[n][kn] = true;
                    if std::env::var("MF_UV_DEBUG").is_ok() {
                        why_seam[(cosd < hard) as usize + 2 * split as usize + 4 * part as usize] += 1;
                    }
                }
            }
        }
        let mut g = Group { geo: std::sync::Arc::new(g), seam };
        let pruned = prune_dead_ends(&mut g);
        let natural = g.seam.clone();
        let debug = std::env::var("MF_UV_DEBUG").is_ok();
        if debug {
            eprintln!("  {pruned} dead-end seam edges pruned");
        }
        let t0 = std::time::Instant::now();
        if debug {
            eprintln!("  natural seams by (hard, split, part) bits: {why_seam:?}");
            let hard_n = g.seam.iter().flatten().filter(|x| **x).count() / 2;
            let interior = g.adj.iter().flatten().filter(|x| **x != NONE).count() / 2;
            eprintln!("[{}] {} tris, {} interior edges, {} natural seams, {} natural charts", raw.groups[gi].0, ntri, interior, hard_n, charts(&g).1.len());
        }
        let mut tally = Tally::default();
        let mut n_rounds = 0;
        // ---- disks, flattening, splits
        // Flattened charts by (first triangle, size): a chart found again
        // unchanged (same triangles, same seams) keeps its map.
        let mut flat: Flat = HashMap::new();
        // Pelt seams already opened per chart (a hand gets one per finger).
        let mut tips: HashMap<(u32, usize), u32> = HashMap::new();
        for _round in 0..64 {
            n_rounds += 1;
            let (chart_of, list) = charts(&g);
            // Every chart not flattened yet, each in its own copy of the
            // group (charts share no triangle, so their seams merge back).
            let todo: Vec<usize> = (0..list.len()).filter(|c| !flat.contains_key(&(list[*c][0], list[*c].len()))).collect();
            if todo.is_empty() {
                break;
            }
            use rayon::prelude::*;
            let done: Vec<(usize, Group, Option<Flattened>, bool, Tally)> = todo
                .into_par_iter()
                .map(|c| {
                    let tris = &list[c];
                    let mut h = g.clone();
                    let used = tips.get(&(tris[0], tris.len())).copied().unwrap_or(0);
                    let (f, tip, n) = work_chart(&mut h, tris, &chart_of, c as u32, used, opts.max_stretch as f64);
                    (c, h, f, tip, n)
                })
                .collect();
            let mut changed = false;
            for (c, h, f, tip, n) in done {
                let tris = &list[c];
                let key = (tris[0], tris.len());
                tally.add(n);
                g.take(&h, tris);
                if tip {
                    *tips.entry(key).or_insert(0) += 1;
                }
                match f {
                    Some(f) => {
                        flat.insert(key, f);
                    }
                    None => changed = true,
                }
            }
            if !changed {
                break;
            }
        }
        // ---- merging islands back
        let n_merged = merge_charts(&mut g, &natural, opts.max_stretch as f64, &mut flat);
        if debug {
            eprintln!(
                "  {} rounds, {} cuts, {} tip seams, {} splits ({} along loops; {} invalid, {} not disks, {} solves failed), {} merges, {:.1} s",
                n_rounds, tally.cuts, tally.tips, tally.splits, tally.loops, tally.invalid, tally.nodisk, tally.nolscm, n_merged, t0.elapsed().as_secs_f32()
            );
            eprintln!("  invalid because: {:?}", tally.reasons);
        }
        // ---- output: a vertex per (chart vertex, file vertex), charts apart
        let (chart_of, list) = charts(&g);
        if debug {
            let unflat = list.iter().filter(|t| !flat.contains_key(&(t[0], t.len()))).count();
            eprintln!("  -> {} charts ({} not flattened)", list.len(), unflat);
        }
        let mut seam_len = 0.0f64;
        for t in 0..ntri {
            for k in 0..3 {
                let n = g.adj[t][k];
                if n == NONE || g.seam[t][k] && (n as usize) > t {
                    seam_len += g.edge_len(t, k) as f64;
                }
            }
        }
        let total_area: f64 = g.area.iter().map(|a| *a as f64).sum();
        let mut out_pos: Vec<[f32; 3]> = Vec::new();
        let mut out_raw: Vec<u32> = Vec::new();
        let mut out_uv: Vec<[f32; 2]> = Vec::new();
        let mut out_idx = vec![0u32; ntri * 3];
        // Quarter turns the packer may give each triangle's island.
        let mut turns = vec![if opts.upright { 1u8 } else { 15 }; ntri];
        let mut x_off = 0.0f64;
        for (c, tris) in list.iter().enumerate() {
            let uv_cut = flat.get(&(tris[0], tris.len()));
            let cut_fallback;
            let (uv, verts): (Vec<DVec2>, &Vec<u32>) = match uv_cut {
                Some(f) => (f.uv.clone(), &f.vert),
                None => {
                    // Not flattened: degenerate UVs, uvunwrap unwraps it.
                    cut_fallback = cut_mesh(&g, tris, &chart_of, c as u32);
                    (vec![DVec2::ZERO; cut_fallback.nverts], &cut_fallback.vert)
                }
            };
            let mut uv = uv;
            if uv_cut.is_some() && opts.upright {
                let allowed = upright(&g, tris, verts, &mut uv, uv_cut.is_some_and(|f| f.straight));
                for t in tris {
                    turns[*t as usize] = allowed;
                }
            }
            let lo = uv.iter().fold(DVec2::splat(f64::MAX), |m, p| m.min(*p));
            let hi = uv.iter().fold(DVec2::splat(f64::MIN), |m, p| m.max(*p));
            let mut map: HashMap<(u32, u32), u32> = HashMap::new();
            for (i, &t) in tris.iter().enumerate() {
                for k in 0..3 {
                    let lv = verts[i * 3 + k];
                    let r = g.raw[t as usize][k];
                    let v = *map.entry((lv, r)).or_insert_with(|| {
                        out_pos.push(raw.positions[r as usize]);
                        out_raw.push(r);
                        let p = uv[lv as usize] - lo;
                        out_uv.push([(p.x + x_off) as f32, p.y as f32]);
                        (out_pos.len() - 1) as u32
                    });
                    out_idx[t as usize * 3 + k] = v;
                }
            }
            // Charts side by side: no two share a UV by chance.
            x_off += (hi.x - lo.x).max(0.0) + diag as f64;
        }
        // Triangles without area: their corners reuse a vertex made for the
        // same file vertex (or a new one), they add no island.
        {
            let mut by_raw: HashMap<u32, u32> = HashMap::new();
            for (i, r) in out_raw.iter().enumerate() {
                by_raw.entry(*r).or_insert(i as u32);
            }
            for t in 0..ntri {
                if !g.degen[t] {
                    continue;
                }
                // One UV point for the whole triangle: a corner's existing
                // vertex if any, its UV shared by the others.
                let anchor = (0..3).find_map(|k| by_raw.get(&g.raw[t][k]).map(|v| out_uv[*v as usize])).unwrap_or([0.0, 0.0]);
                for k in 0..3 {
                    let r = g.raw[t][k];
                    let v = match by_raw.get(&r) {
                        Some(&v) if out_uv[v as usize] == anchor => v,
                        _ => {
                            out_pos.push(raw.positions[r as usize]);
                            out_raw.push(r);
                            out_uv.push(anchor);
                            (out_pos.len() - 1) as u32
                        }
                    };
                    out_idx[t * 3 + k] = v;
                }
            }
        }
        // ---- packing
        // A triangle without area leaves its polygon (the packer wants a
        // polygon's triangles all ignored or none).
        let pack_polys: Vec<u32> = polys.iter().enumerate().map(|(t, p)| if g.degen[t] { u32::MAX - t as u32 } else { *p }).collect();
        let mut input = uvunwrap::MeshInput::new(&out_pos, &out_idx);
        input.polygons = Some(&pack_polys);
        // The packer's v grows upward (it turns mirrored islands to wind
        // counter-clockwise that way); ours grows downward: flipped going in
        // and coming out, or every island comes out mirrored.
        let pack_uv: Vec<[f32; 2]> = out_uv.iter().map(|p| [p[0], -p[1]]).collect();
        input.existing_uvs = Some(&pack_uv);
        // Triangles without area get no island (their corners snap to a UV).
        input.ignored_faces = Some(&g.degen);
        input.face_turns = Some(&turns);
        let t_pack = std::time::Instant::now();
        let (atlas, how) = pack_best(&input, opts).map_err(|e| anyhow!("Auto UV of \"{}\" failed: {e}", raw.groups[gi].0))?;
        if debug {
            eprintln!("  packed by {how} in {:.1} s", t_pack.elapsed().as_secs_f32());
        }
        let Some(m) = atlas.meshes.first() else { continue };
        if debug {
            // Islands of the packed result (triangles sharing output vertices).
            // Corners at one place with one UV are one vertex.
            let mut key: HashMap<([u32; 3], [u32; 2]), u32> = HashMap::new();
            let ids: Vec<u32> = m
                .vertices
                .iter()
                .enumerate()
                .map(|(i, v)| {
                    let p = out_pos[v.xref as usize];
                    *key.entry(([p[0].to_bits(), p[1].to_bits(), p[2].to_bits()], [v.uv[0].to_bits(), v.uv[1].to_bits()])).or_insert(i as u32)
                })
                .collect();
            let mut dsu = Dsu::new(m.vertices.len());
            for t in m.indices.chunks_exact(3) {
                dsu.union(ids[t[0] as usize], ids[t[1] as usize]);
                dsu.union(ids[t[1] as usize], ids[t[2] as usize]);
            }
            let mut per_chart: HashMap<u32, std::collections::HashSet<u32>> = HashMap::new();
            for (t, tri) in m.indices.chunks_exact(3).enumerate() {
                if chart_of[t] != NONE {
                    per_chart.entry(chart_of[t]).or_default().insert(dsu.find(ids[tri[0] as usize]));
                }
            }
            let mut v: Vec<(usize, u32)> = per_chart.iter().map(|(c, s)| (s.len(), *c)).filter(|(n, _)| *n > 1).collect();
            v.sort_unstable_by(|a, b| b.cmp(a));
            let all: std::collections::HashSet<u32> = m.indices.chunks_exact(3).map(|t| dsu.find(ids[t[0] as usize])).collect();
            let degen_n = g.degen.iter().filter(|d| **d).count();
            eprintln!("  output islands {} (packer counts {}), degenerate triangles {degen_n}", all.len(), atlas.chart_count);
            eprintln!("  packer split {} of {} charts; worst:", v.len(), list.len());
            for (n, c) in v.iter().take(6) {
                let tris = &list[*c as usize];
                let cut = cut_mesh(&g, tris, &chart_of, *c);
                let fl = flat.get(&(tris[0], tris.len()));
                let ok = fl.map(|f| valid_layout(&cut, &f.uv, tris.len()));
                let thin = tris.iter().filter(|t| {
                    let w = g.tris[**t as usize];
                    let (a, b, cc) = (g.pos[w[0] as usize], g.pos[w[1] as usize], g.pos[w[2] as usize]);
                    let longest = (b - a).length().max((cc - b).length()).max((a - cc).length());
                    2.0 * g.area[**t as usize] / longest <= diag * 1e-6
                }).count();
                let why = fl.and_then(|f| layout_problem(&cut, &f.uv, tris.len()));
                let npoly = tris.iter().map(|t| g.poly[*t as usize]).collect::<std::collections::HashSet<_>>().len();
                if let Some(Flattened { uv, .. }) = fl {
                    let a2: Vec<f64> = (0..tris.len()).map(|i| { let u = [uv[cut.vert[i*3] as usize], uv[cut.vert[i*3+1] as usize], uv[cut.vert[i*3+2] as usize]]; (u[1]-u[0]).perp_dot(u[2]-u[0]) * 0.5 }).collect();
                    let tot: f64 = a2.iter().sum();
                    let (mi, mv) = a2.iter().enumerate().map(|(i, a)| (i, a / tot)).min_by(|a, b| a.1.total_cmp(&b.1)).unwrap();
                    let t = tris[mi] as usize;
                    let a3tot: f32 = tris.iter().map(|t| g.area[*t as usize]).sum();
                    let w = g.tris[t];
                    let cv = [cut.vert[mi*3], cut.vert[mi*3+1], cut.vert[mi*3+2]];
                    eprintln!("      polys {npoly}, worst uv area {mv:.2e} (3D {:.2e}), welded {:?}, cut verts {:?}", g.area[t] / a3tot, w, cv);
                }
                eprintln!("    chart {c}: {} tris -> {n} islands, our check {:?} ({:?}), slivers {thin}", tris.len(), ok, why);
            }
        }
        let aspect = atlas.height as f32 / atlas.width.max(1) as f32;
        let base = positions.len() as u32;
        for v in &m.vertices {
            let r = out_raw[v.xref as usize] as usize;
            positions.push(raw.positions[r]);
            normals.push(raw.normals[r]);
            if has_colors {
                colors.push(raw.colors[r]);
            }
            uvs.push([v.uv[0], (1.0 - v.uv[1]) * aspect]);
        }
        let new_idx: Vec<u32> = m.indices.iter().map(|i| i + base).collect();
        let (_, sm, s95) = measure(&positions, &uvs, &new_idx);
        raw.groups[gi].1 = new_idx;
        reports.push(UvReport {
            name: raw.groups[gi].0.clone(),
            triangles: ntri,
            charts: atlas.chart_count,
            coverage: measure(&positions, &uvs, &raw.groups[gi].1).0,
            stretch_mean: sm,
            stretch_p95: s95,
            seam_length: (seam_len / total_area.max(1e-12).sqrt()) as f32,
        });
    }
    raw.positions = positions;
    raw.normals = normals;
    raw.normal_missing.clear();
    raw.colors = colors;
    raw.uvs = uvs;
    Ok(reports)
}
