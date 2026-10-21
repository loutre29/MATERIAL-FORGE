//! Mesh import. Every loader produces a `RawMesh`; `finalize` turns it into the
//! render-ready `MeshData` (tangents, bounds). The user's authored normals and
//! UVs are used as-is: nothing here rewrites or decimates the source model.

use anyhow::{bail, Result};
use glam::Vec3;
use std::collections::HashMap;
use std::path::Path;

mod fbx;
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
    /// Image space (v down). Empty when the file carries no UVs.
    pub uvs: Vec<[f32; 2]>,
    /// Per-vertex colors, empty when absent.
    pub colors: Vec<[f32; 3]>,
    /// (material name, triangle indices, object id per triangle). Names may
    /// repeat; they are merged.
    pub groups: Vec<(String, Vec<u32>, Vec<u32>)>,
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

pub fn load(path: &Path) -> Result<MeshData> {
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
    finalize(raw)
}

pub(crate) fn finalize(mut raw: RawMesh) -> Result<MeshData> {
    let n = raw.positions.len();
    if n == 0 || raw.groups.iter().all(|(_, i, _)| i.is_empty()) {
        bail!("The mesh contains no triangles");
    }
    if raw.uvs.len() != n {
        bail!("The mesh has no UV coordinates. Unwrap it in your 3D software before importing.");
    }
    if raw.normals.len() != n {
        raw.normals = smooth_normals(&raw.positions, &raw.groups);
        raw.notes.push(
            "The file has no normals: smooth display normals were computed (the source file is untouched)."
                .into(),
        );
    }

    if !raw.colors.is_empty() && raw.colors.len() != n {
        raw.colors.clear();
    }
    if raw.objects.is_empty() {
        raw.objects.push("Object".into());
    }

    // Merge groups sharing a material name, keep first-seen order.
    let mut order: Vec<String> = Vec::new();
    let mut merged: HashMap<String, (Vec<u32>, Vec<u32>)> = HashMap::new();
    for (name, idx, objs) in raw.groups.drain(..) {
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
    }
    let mut colors_out = Vec::new();

    let mut vertices: Vec<Vertex> = Vec::with_capacity(n);
    let mut submeshes = Vec::new();
    let mut weld: HashMap<(u32, [i32; 4]), u32> = HashMap::new();
    for name in order {
        let (idx, tri_objects) = merged.remove(&name).unwrap();
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
        submeshes.push(SubMesh { name, indices: out_idx, tri_objects });
    }

    let mut bbox_min = Vec3::splat(f32::MAX);
    let mut bbox_max = Vec3::splat(f32::MIN);
    for v in &vertices {
        let p = Vec3::from(v.pos);
        bbox_min = bbox_min.min(p);
        bbox_max = bbox_max.max(p);
    }

    Ok(MeshData {
        vertices,
        submeshes,
        bbox_min,
        bbox_max,
        colors: colors_out,
        objects: raw.objects,
        notes: raw.notes,
    })
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
