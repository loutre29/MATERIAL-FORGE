//! glTF 2.0 / GLB reader. Node transforms are baked into world space; each
//! glTF material becomes a texture set.

use super::RawMesh;
use anyhow::{Context, Result};
use glam::{Mat3, Mat4, Vec3};
use std::path::Path;

pub fn load(path: &Path) -> Result<RawMesh> {
    let gltf = match gltf::Gltf::open(path) {
        Ok(g) => g,
        Err(e) if e.to_string().contains("draco") => anyhow::bail!(
            "This file uses Draco mesh compression, which is not supported. Re-export it without Draco."
        ),
        Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
    };
    let buffers = gltf::import_buffers(&gltf.document, path.parent(), gltf.blob.clone())
        .context("loading glTF buffers")?;
    let mut raw = RawMesh::default();
    let mut has_normals = true;
    let mut has_uvs = true;

    let scene = gltf.document.default_scene().or_else(|| gltf.document.scenes().next());
    match scene {
        Some(scene) => {
            for node in scene.nodes() {
                visit(&node, Mat4::IDENTITY, &buffers, &mut raw, &mut has_normals, &mut has_uvs);
            }
        }
        None => {
            for mesh in gltf.document.meshes() {
                add_mesh(&mesh, Mat4::IDENTITY, &buffers, &mut raw, &mut has_normals, &mut has_uvs);
            }
        }
    }
    if has_normals {
        raw.normal_missing.clear();
    }
    if !has_uvs {
        raw.uvs.clear();
    }
    Ok(raw)
}

fn visit(
    node: &gltf::Node,
    parent: Mat4,
    buffers: &[gltf::buffer::Data],
    raw: &mut RawMesh,
    has_normals: &mut bool,
    has_uvs: &mut bool,
) {
    let world = parent * Mat4::from_cols_array_2d(&node.transform().matrix());
    if let Some(mesh) = node.mesh() {
        add_mesh(&mesh, world, buffers, raw, has_normals, has_uvs);
    }
    for child in node.children() {
        visit(&child, world, buffers, raw, has_normals, has_uvs);
    }
}

fn add_mesh(
    mesh: &gltf::Mesh,
    world: Mat4,
    buffers: &[gltf::buffer::Data],
    raw: &mut RawMesh,
    has_normals: &mut bool,
    has_uvs: &mut bool,
) {
    let normal_m = Mat3::from_mat4(world).inverse().transpose();
    let flip = world.determinant() < 0.0;
    let obj_name = mesh.name().map(String::from).unwrap_or_else(|| format!("Mesh_{}", mesh.index()));
    let object = raw.object(&obj_name);
    for prim in mesh.primitives() {
        if prim.mode() != gltf::mesh::Mode::Triangles {
            raw.notes.push(format!("Skipped a non-triangle primitive in mesh '{}'", mesh.name().unwrap_or("?")));
            continue;
        }
        let reader = prim.reader(|b| Some(&buffers[b.index()].0[..]));
        let Some(positions) = reader.read_positions() else { continue };
        let base = raw.positions.len() as u32;
        let positions: Vec<[f32; 3]> = positions.collect();
        let count = positions.len();
        for p in &positions {
            raw.positions.push(world.transform_point3(Vec3::from(*p)).into());
        }
        match reader.read_normals() {
            Some(ns) => {
                raw.normals.extend(ns.map(|n| (normal_m * Vec3::from(n)).normalize_or(Vec3::Y).to_array()));
                raw.normal_missing.resize(raw.normals.len(), false);
            }
            None => {
                *has_normals = false;
                raw.normals.extend(std::iter::repeat([0.0, 1.0, 0.0]).take(count));
                raw.normal_missing.resize(raw.normals.len(), true);
            }
        }
        match reader.read_tex_coords(0) {
            // glTF UVs are already image space (v down).
            Some(uv) => raw.uvs.extend(uv.into_f32()),
            None => {
                *has_uvs = false;
                raw.uvs.extend(std::iter::repeat([0.0, 0.0]).take(count));
            }
        }
        match reader.read_colors(0) {
            Some(c) => raw.colors.extend(c.into_rgb_f32()),
            None => raw.colors.extend(std::iter::repeat([1.0; 3]).take(count)),
        }
        let indices: Vec<u32> = match reader.read_indices() {
            Some(i) => i.into_u32().collect(),
            None => (0..count as u32).collect(),
        };
        let mut tris = Vec::with_capacity(indices.len());
        for t in indices.chunks_exact(3) {
            if flip {
                tris.extend_from_slice(&[base + t[0], base + t[2], base + t[1]]);
            } else {
                tris.extend_from_slice(&[base + t[0], base + t[1], base + t[2]]);
            }
        }
        let mat = prim.material();
        let name = match (mat.name(), mat.index()) {
            (Some(n), _) => n.to_string(),
            (None, Some(i)) => format!("Material_{i}"),
            (None, None) => "DefaultMaterial".to_string(),
        };
        let ntri = tris.len() / 3;
        raw.groups.push((name, tris, vec![object; ntri]));
    }
}
