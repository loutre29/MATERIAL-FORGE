//! Mesh import. Every loader produces a `RawMesh`; `finalize` turns it into the
//! render-ready `MeshData` (tangents, bounds). The user's authored normals and
//! UVs are used as-is: nothing here rewrites or decimates the source model.
//! Auto UV (`AutoUv`) unwraps meshes without UVs, or every mesh when the user
//! asks for it in the New project dialog (the source file is never written;
//! `write_obj` exports the mesh with its new UVs).

use anyhow::{bail, Result};
use glam::Vec3;
use std::collections::HashMap;
use std::path::Path;

mod fbx;
mod smartuv;
pub use smartuv::{SmartOptions, UvReport};
mod gltf_load;
mod obj;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    pub pos: [f32; 3],
    pub normal: [f32; 3],
    /// Image-space UV: origin at the top-left of the texture, v grows downward.
    pub uv: [f32; 2],
    /// MikkTSpace tangent, w is the bitangent sign (OpenGL convention).
    pub tangent: [f32; 4],
}

/// One material slot of the model; each becomes a texture set.
#[derive(Clone, Debug)]
pub struct SubMesh {
    pub name: String,
    pub indices: Vec<u32>,
    /// Object (mesh part) index of each triangle, into `MeshData::objects`.
    pub tri_objects: Vec<u32>,
    /// Polygon of each triangle: quads and n-gons of the source file, or
    /// triangle pairs rebuilt into quads when the file only has triangles.
    pub tri_polys: Vec<u32>,
}

#[derive(Clone, Debug)]
pub struct MeshData {
    pub vertices: Vec<Vertex>,
    pub submeshes: Vec<SubMesh>,
    pub bbox_min: Vec3,
    pub bbox_max: Vec3,
    /// Per-vertex colors (linear), empty when the file has none.
    pub colors: Vec<[f32; 3]>,
    /// Object / part names (ID map baking by object).
    pub objects: Vec<String>,
    /// Import remarks shown to the user (missing normals, skipped primitives...).
    pub notes: Vec<String>,
}

impl MeshData {
    pub fn triangle_count(&self) -> usize {
        self.submeshes.iter().map(|s| s.indices.len() / 3).sum()
    }

    pub fn diagonal(&self) -> f32 {
        (self.bbox_max - self.bbox_min).length().max(1e-6)
    }
}

/// Geometry as the loaders read it, before tangent generation.
#[derive(Default)]
pub(crate) struct RawMesh {
    pub positions: Vec<[f32; 3]>,
    /// Empty when the file carries no normals.
    pub normals: Vec<[f32; 3]>,
    /// Corners whose normal the file does not give (aligned with
    /// `normals`; empty: none missing). Only those get computed normals.
    pub normal_missing: Vec<bool>,
    /// Image space (v down). Empty when the file carries no UVs.
    pub uvs: Vec<[f32; 2]>,
    /// Per-vertex colors, empty when absent.
    pub colors: Vec<[f32; 3]>,
    /// (material name, triangle indices, object id per triangle). Names may
    /// repeat; they are merged.
    pub groups: Vec<(String, Vec<u32>, Vec<u32>)>,
    /// Source polygon of each triangle, per group (same order as `groups`);
    /// loaders that only know triangles leave it empty.
    pub polys: Vec<Vec<u32>>,
    /// Next polygon id (loaders number polygons file-wide).
    pub next_poly: u32,
    pub objects: Vec<String>,
    pub notes: Vec<String>,
}

impl RawMesh {
    /// Object id for `name`, registering it on first use.
    pub fn object(&mut self, name: &str) -> u32 {
        match self.objects.iter().position(|o| o == name) {
            Some(i) => i as u32,
            None => {
                self.objects.push(name.to_string());
                (self.objects.len() - 1) as u32
            }
        }
    }
}

/// UVs of an imported mesh.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum AutoUv {
    /// The mesh's UVs; a mesh without UVs is unwrapped automatically.
    #[default]
    IfMissing,
    /// Always unwrap: the authored UVs are replaced (in the project only).
    Always,
    /// High poly meshes for baking: no UVs needed, none generated.
    Skip,
}

impl AutoUv {
    /// The choices of the New project dialog.
    pub const CHOICES: [AutoUv; 2] = [AutoUv::IfMissing, AutoUv::Always];
    pub fn name(self) -> &'static str {
        match self {
            AutoUv::IfMissing => "Mesh UVs (auto unwrap if missing)",
            AutoUv::Always => "Auto unwrap (replace the mesh UVs)",
            AutoUv::Skip => "None",
        }
    }
}

/// How auto UV unwraps.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum UvMethod {
    /// Seams placed by shape (hidden places, tubes cut once, closed parts
    /// opened), conformal flattening, few large islands.
    #[default]
    Smart,
    /// Charts grown by surface direction (xatlas style): fast, more islands.
    Fast,
}

impl UvMethod {
    pub const ALL: [UvMethod; 2] = [UvMethod::Smart, UvMethod::Fast];
    pub fn name(self) -> &'static str {
        match self {
            UvMethod::Smart => "Smart",
            UvMethod::Fast => "Fast",
        }
    }

    pub fn about(self) -> &'static str {
        match self {
            UvMethod::Smart => "Seams placed by shape where they show least, tubes cut once, few large islands.",
            UvMethod::Fast => "Charts grown by surface direction: quick, more small islands.",
        }
    }
}

/// Auto UV settings: the method and the smart method's options.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct UvSettings {
    pub method: UvMethod,
    pub smart: SmartOptions,
}

pub fn load(path: &Path) -> Result<MeshData> {
    load_with(path, AutoUv::IfMissing)
}

pub fn load_with(path: &Path, auto_uv: AutoUv) -> Result<MeshData> {
    load_full(path, auto_uv, &UvSettings::default())
}

pub fn load_full(path: &Path, auto_uv: AutoUv, uv: &UvSettings) -> Result<MeshData> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let raw = match ext.as_str() {
        "obj" => obj::load(path)?,
        "gltf" | "glb" => gltf_load::load(path)?,
        "fbx" => fbx::load(path)?,
        _ => bail!("Unsupported mesh format '.{ext}' (use OBJ, FBX, glTF or GLB)"),
    };
    finalize_with(raw, auto_uv, uv).map(|r| r.0)
}

pub(crate) fn finalize(raw: RawMesh, auto_uv: AutoUv) -> Result<MeshData> {
    finalize_with(raw, auto_uv, &UvSettings::default()).map(|r| r.0)
}

/// New UVs for a loaded mesh (the Auto UV button): same triangles in the
/// same order, so painted textures can be carried over.
pub fn unwrap_mesh(mesh: &MeshData, uv: &UvSettings) -> Result<(MeshData, Vec<UvReport>)> {
    let raw = RawMesh {
        positions: mesh.vertices.iter().map(|v| v.pos).collect(),
        normals: mesh.vertices.iter().map(|v| v.normal).collect(),
        normal_missing: Vec::new(),
        uvs: mesh.vertices.iter().map(|v| v.uv).collect(),
        colors: mesh.colors.clone(),
        groups: mesh.submeshes.iter().map(|s| (s.name.clone(), s.indices.clone(), s.tri_objects.clone())).collect(),
        polys: mesh.submeshes.iter().map(|s| s.tri_polys.clone()).collect(),
        next_poly: 0,
        objects: mesh.objects.clone(),
        notes: Vec::new(),
    };
    finalize_with(raw, AutoUv::Always, uv)
}

/// `unwrap_mesh` for some texture sets only (`only[i]`: submesh i gets new
/// UVs; missing entries count as chosen). The other sets keep their UVs and
/// vertices; triangles stay in the same order.
pub fn unwrap_sets(mesh: &MeshData, uv: &UvSettings, only: &[bool]) -> Result<(MeshData, Vec<UvReport>)> {
    let chosen: Vec<usize> = (0..mesh.submeshes.len()).filter(|i| only.get(*i).copied().unwrap_or(true)).collect();
    if chosen.len() == mesh.submeshes.len() {
        return unwrap_mesh(mesh, uv);
    }
    if chosen.is_empty() {
        bail!("No texture set chosen");
    }
    let mut part = mesh.clone();
    part.submeshes = chosen.iter().map(|i| mesh.submeshes[*i].clone()).collect();
    let (new, reports) = unwrap_mesh(&part, uv)?;
    anyhow::ensure!(new.submeshes.len() == chosen.len(), "the unwrap changed the texture sets");
    // The chosen sets take the new vertices, the others keep theirs; then
    // the vertices nothing uses any more go.
    let mut out = mesh.clone();
    let base = out.vertices.len() as u32;
    let colored = !out.colors.is_empty();
    out.vertices.extend_from_slice(&new.vertices);
    if colored {
        if new.colors.len() == new.vertices.len() {
            out.colors.extend_from_slice(&new.colors);
        } else {
            out.colors.resize(out.vertices.len(), [1.0; 3]);
        }
    }
    for (k, &i) in chosen.iter().enumerate() {
        anyhow::ensure!(new.submeshes[k].indices.len() == mesh.submeshes[i].indices.len(), "the unwrap changed the triangles of {}", mesh.submeshes[i].name);
        out.submeshes[i].indices = new.submeshes[k].indices.iter().map(|x| x + base).collect();
    }
    let mut map = vec![u32::MAX; out.vertices.len()];
    let (mut verts, mut colors) = (Vec::new(), Vec::new());
    for s in out.submeshes.iter_mut() {
        for x in s.indices.iter_mut() {
            let m = &mut map[*x as usize];
            if *m == u32::MAX {
                *m = verts.len() as u32;
                verts.push(out.vertices[*x as usize]);
                if colored {
                    colors.push(out.colors[*x as usize]);
                }
            }
            *x = *m;
        }
    }
    out.vertices = verts;
    out.colors = colors;
    out.notes = new.notes;
    Ok((out, reports))
}

pub(crate) fn finalize_with(mut raw: RawMesh, auto_uv: AutoUv, uv_settings: &UvSettings) -> Result<(MeshData, Vec<UvReport>)> {
    let n = raw.positions.len();
    if n == 0 || raw.groups.iter().all(|(_, i, _)| i.is_empty()) {
        bail!("The mesh contains no triangles");
    }
    let missing = raw.normal_missing.len() == n && raw.normal_missing.iter().any(|m| *m);
    if raw.normals.len() != n || (missing && raw.normal_missing.iter().all(|m| *m)) {
        raw.normals = smooth_normals(&raw.positions, &raw.groups);
        raw.notes.push(
            "The file has no normals: smooth display normals were computed (the source file is untouched)."
                .into(),
        );
    } else if missing {
        // Authored normals are kept; only the corners without one get a
        // computed normal.
        let smooth = smooth_normals(&raw.positions, &raw.groups);
        for (i, m) in raw.normal_missing.iter().enumerate() {
            if *m {
                raw.normals[i] = smooth[i];
            }
        }
        raw.notes.push("Some parts have no normals: smooth normals were computed for those parts only (authored normals kept).".into());
    }

    if !raw.colors.is_empty() && raw.colors.len() != n {
        raw.colors.clear();
    }
    let has_uvs = raw.uvs.len() == n;
    let mut reports = Vec::new();
    match auto_uv {
        AutoUv::Skip if !has_uvs => raw.uvs = vec![[0.0; 2]; n],
        AutoUv::Skip => {}
        AutoUv::IfMissing if has_uvs => {}
        AutoUv::IfMissing | AutoUv::Always => {
            reports = match uv_settings.method {
                UvMethod::Smart => smartuv::unwrap(&mut raw, &uv_settings.smart)?,
                UvMethod::Fast => auto_unwrap(&mut raw)?,
            };
            let charts: u32 = reports.iter().map(|r| r.charts).sum();
            raw.notes.push(if has_uvs {
                format!("Auto UV: the mesh UVs were replaced by {charts} generated islands (in the project only; File > Export mesh writes them out).")
            } else {
                format!("The mesh had no UVs: they were generated automatically ({charts} islands; File > Export mesh writes them out).")
            });
        }
    }
    if raw.objects.is_empty() {
        raw.objects.push("Object".into());
    }

    // Merge groups sharing a material name, keep first-seen order.
    let mut order: Vec<String> = Vec::new();
    let mut merged: HashMap<String, (Vec<u32>, Vec<u32>, Vec<u32>)> = HashMap::new();
    let polys = std::mem::take(&mut raw.polys);
    // Triangles without a source polygon get one each (paired into quads below).
    let mut next = raw.next_poly;
    for (gi, (name, idx, objs)) in raw.groups.drain(..).enumerate() {
        if idx.is_empty() {
            continue;
        }
        if !merged.contains_key(&name) {
            order.push(name.clone());
        }
        let ntri = idx.len() / 3;
        let e = merged.entry(name).or_default();
        e.0.extend(idx);
        e.1.extend((0..ntri).map(|t| objs.get(t).copied().unwrap_or(0)));
        match polys.get(gi).filter(|p| p.len() == ntri) {
            Some(p) => e.2.extend(p.iter().copied()),
            None => {
                e.2.extend(next..next + ntri as u32);
                next += ntri as u32;
            }
        }
    }
    let mut colors_out = Vec::new();

    let mut vertices: Vec<Vertex> = Vec::with_capacity(n);
    let mut submeshes = Vec::new();
    let mut weld: HashMap<(u32, [i32; 4]), u32> = HashMap::new();
    for name in order {
        let (idx, tri_objects, src_polys) = merged.remove(&name).unwrap();
        let tangents = generate_tangents(&raw, &idx);
        let mut out_idx = Vec::with_capacity(idx.len());
        for (corner, &vi) in idx.iter().enumerate() {
            let t = tangents[corner];
            let key = (
                vi,
                [
                    (t[0] * 1e4) as i32,
                    (t[1] * 1e4) as i32,
                    (t[2] * 1e4) as i32,
                    t[3].signum() as i32,
                ],
            );
            let new = *weld.entry(key).or_insert_with(|| {
                vertices.push(Vertex {
                    pos: raw.positions[vi as usize],
                    normal: raw.normals[vi as usize],
                    uv: raw.uvs[vi as usize],
                    tangent: t,
                });
                if !raw.colors.is_empty() {
                    colors_out.push(raw.colors[vi as usize]);
                }
                (vertices.len() - 1) as u32
            });
            out_idx.push(new);
        }
        let tri_polys = quad_polys(&vertices, &out_idx, &src_polys);
        submeshes.push(SubMesh { name, indices: out_idx, tri_objects, tri_polys });
    }

    let mut bbox_min = Vec3::splat(f32::MAX);
    let mut bbox_max = Vec3::splat(f32::MIN);
    for v in &vertices {
        let p = Vec3::from(v.pos);
        bbox_min = bbox_min.min(p);
        bbox_max = bbox_max.max(p);
    }

    Ok((
        MeshData {
            vertices,
            submeshes,
            bbox_min,
            bbox_max,
            colors: colors_out,
            objects: raw.objects,
            notes: raw.notes,
        },
        reports,
    ))
}

/// Unwraps every material group into its own 0-1 UV space (a texture set
/// each): charts follow the hard edges and keep each source polygon whole.
/// Seam vertices are duplicated; returns the number of charts.
fn auto_unwrap(raw: &mut RawMesh) -> Result<Vec<UvReport>> {
    let has_colors = !raw.colors.is_empty();
    let (mut positions, mut normals, mut colors, mut uvs) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let mut reports = Vec::new();
    for gi in 0..raw.groups.len() {
        let idx = raw.groups[gi].1.clone();
        if idx.is_empty() {
            continue;
        }
        let ntri = idx.len() / 3;
        let polys = raw.polys.get(gi).filter(|p| p.len() == ntri);
        let mut input = uvunwrap::MeshInput::new(&raw.positions, &idx);
        input.polygons = polys.map(|p| p.as_slice());
        let mut opts = uvunwrap::Options::default();
        opts.density = uvunwrap::Density::Auto { page_size: 2048 };
        opts.padding = 4;
        let atlas = uvunwrap::generate(&[input], &opts).map_err(|e| anyhow::anyhow!("Auto UV of \"{}\" failed: {e}", raw.groups[gi].0))?;
        let Some(m) = atlas.meshes.first() else { continue };
        let chart_count = atlas.chart_count;
        // A cropped page is wider than tall: keep the charts' proportions in
        // the square texture.
        let aspect = atlas.height as f32 / atlas.width.max(1) as f32;
        let base = positions.len() as u32;
        for v in &m.vertices {
            let x = v.xref as usize;
            positions.push(raw.positions[x]);
            normals.push(raw.normals[x]);
            if has_colors {
                colors.push(raw.colors[x]);
            }
            // The packer's v grows upward, ours downward: unflipped, every
            // chart would be mirrored.
            uvs.push([v.uv[0], (1.0 - v.uv[1]) * aspect]);
        }
        raw.groups[gi].1 = m.indices.iter().map(|i| i + base).collect();
        let (coverage, stretch_mean, stretch_p95) = smartuv::measure(&positions, &uvs, &raw.groups[gi].1);
        reports.push(UvReport { name: raw.groups[gi].0.clone(), triangles: idx.len() / 3, charts: chart_count, coverage, stretch_mean, stretch_p95, seam_length: 0.0 });
    }
    raw.positions = positions;
    raw.normals = normals;
    raw.normal_missing.clear();
    raw.colors = colors;
    raw.uvs = uvs;
    Ok(reports)
}

/// Writes the mesh as OBJ (positions, normals, UVs, one `usemtl` per texture
/// set): the way out for UVs made by auto UV.
pub fn write_obj(mesh: &MeshData, path: &Path) -> Result<()> {
    use std::fmt::Write as _;
    let mut s = String::from("# Material Forge mesh export\n");
    for v in &mesh.vertices {
        let _ = writeln!(s, "v {} {} {}", v.pos[0], v.pos[1], v.pos[2]);
    }
    for v in &mesh.vertices {
        let _ = writeln!(s, "vn {} {} {}", v.normal[0], v.normal[1], v.normal[2]);
    }
    // OBJ texture coordinates have v up.
    for v in &mesh.vertices {
        let _ = writeln!(s, "vt {} {}", v.uv[0], 1.0 - v.uv[1]);
    }
    for sm in &mesh.submeshes {
        let _ = writeln!(s, "usemtl {}", sm.name);
        for t in sm.indices.chunks_exact(3) {
            let (a, b, c) = (t[0] + 1, t[1] + 1, t[2] + 1);
            let _ = writeln!(s, "f {a}/{a}/{a} {b}/{b}/{b} {c}/{c}/{c}");
        }
    }
    std::fs::write(path, s)?;
    Ok(())
}

/// Polygon ids per triangle, numbered from 0. `given` holds the source
/// polygon of each triangle; triangles alone in their polygon are paired
/// with a neighbor into quads ("tris to quads") when they share their
/// longest edge (the diagonal), the same vertices (no UV seam or hard edge
/// between them) and lie nearly flat.
pub fn quad_polys(vertices: &[Vertex], indices: &[u32], given: &[u32]) -> Vec<u32> {
    let ntri = indices.len() / 3;
    let mut count: HashMap<u32, u32> = HashMap::new();
    for t in 0..ntri {
        *count.entry(given.get(t).copied().unwrap_or(u32::MAX - t as u32)).or_default() += 1;
    }
    let src = |t: usize| given.get(t).copied().unwrap_or(u32::MAX - t as u32);
    let single = |t: usize| count.get(&src(t)).copied().unwrap_or(1) == 1;
    let pos = |v: u32| Vec3::from(vertices[v as usize].pos);
    let tri = |t: usize| [indices[t * 3], indices[t * 3 + 1], indices[t * 3 + 2]];
    let normal = |t: usize| {
        let [a, b, c] = tri(t);
        (pos(b) - pos(a)).cross(pos(c) - pos(a)).normalize_or_zero()
    };
    // The longest edge of a triangle, as a sorted vertex pair.
    let longest = |t: usize| {
        let v = tri(t);
        let mut best = (0.0f32, (0u32, 0u32));
        for k in 0..3 {
            let (a, b) = (v[k], v[(k + 1) % 3]);
            let l = (pos(a) - pos(b)).length_squared();
            if l > best.0 {
                best = (l, (a.min(b), a.max(b)));
            }
        }
        best.1
    };
    let mut by_edge: HashMap<(u32, u32), Vec<usize>> = HashMap::new();
    for t in (0..ntri).filter(|&t| single(t)) {
        by_edge.entry(longest(t)).or_default().push(t);
    }
    let mut partner: Vec<Option<usize>> = vec![None; ntri];
    for t in 0..ntri {
        if !single(t) || partner[t].is_some() {
            continue;
        }
        let Some(list) = by_edge.get(&longest(t)) else { continue };
        if let Some(&o) = list.iter().find(|&&o| o != t && partner[o].is_none() && normal(t).dot(normal(o)) > 0.94) {
            partner[t] = Some(o);
            partner[o] = Some(t);
        }
    }
    // Number polygons compactly in triangle order.
    let mut ids: HashMap<u32, u32> = HashMap::new();
    let mut out = vec![0u32; ntri];
    let mut next = 0u32;
    for t in 0..ntri {
        let key = match partner[t] {
            Some(o) => u32::MAX / 2 + t.min(o) as u32,
            None if single(t) => u32::MAX / 4 + t as u32,
            None => src(t),
        };
        out[t] = *ids.entry(key).or_insert_with(|| {
            next += 1;
            next - 1
        });
    }
    out
}

/// Area-weighted normals, welded by position so UV seams do not facet.
fn smooth_normals(positions: &[[f32; 3]], groups: &[(String, Vec<u32>, Vec<u32>)]) -> Vec<[f32; 3]> {
    let key = |p: &[f32; 3]| [(p[0] * 1e5) as i64, (p[1] * 1e5) as i64, (p[2] * 1e5) as i64];
    let mut acc: HashMap<[i64; 3], Vec3> = HashMap::new();
    for (_, idx, _) in groups {
        for tri in idx.chunks_exact(3) {
            let a = Vec3::from(positions[tri[0] as usize]);
            let b = Vec3::from(positions[tri[1] as usize]);
            let c = Vec3::from(positions[tri[2] as usize]);
            let n = (b - a).cross(c - a);
            for &i in tri {
                *acc.entry(key(&positions[i as usize])).or_insert(Vec3::ZERO) += n;
            }
        }
    }
    positions
        .iter()
        .map(|p| acc.get(&key(p)).copied().unwrap_or(Vec3::Y).normalize_or(Vec3::Y).into())
        .collect()
}

struct MikkGeometry<'a> {
    raw: &'a RawMesh,
    idx: &'a [u32],
    out: Vec<[f32; 4]>,
}

impl mikktspace::Geometry for MikkGeometry<'_> {
    fn num_faces(&self) -> usize {
        self.idx.len() / 3
    }
    fn num_vertices_of_face(&self, _face: usize) -> usize {
        3
    }
    fn position(&self, face: usize, vert: usize) -> [f32; 3] {
        self.raw.positions[self.idx[face * 3 + vert] as usize]
    }
    fn normal(&self, face: usize, vert: usize) -> [f32; 3] {
        self.raw.normals[self.idx[face * 3 + vert] as usize]
    }
    fn tex_coord(&self, face: usize, vert: usize) -> [f32; 2] {
        // MikkTSpace expects OpenGL UVs (v up).
        let t = self.raw.uvs[self.idx[face * 3 + vert] as usize];
        [t[0], 1.0 - t[1]]
    }
    fn set_tangent_encoded(&mut self, tangent: [f32; 4], face: usize, vert: usize) {
        self.out[face * 3 + vert] = tangent;
    }
}

/// Per-corner tangents for one group.
fn generate_tangents(raw: &RawMesh, idx: &[u32]) -> Vec<[f32; 4]> {
    let mut geo = MikkGeometry { raw, idx, out: vec![[1.0, 0.0, 0.0, 1.0]; idx.len()] };
    mikktspace::generate_tangents(&mut geo);
    for t in geo.out.iter_mut() {
        if !t.iter().all(|c| c.is_finite()) {
            *t = [1.0, 0.0, 0.0, 1.0];
        }
    }
    geo.out
}
