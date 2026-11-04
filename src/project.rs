//! `.sprp` project files: a JSON header followed by deflated binary blobs
//! (mesh arrays, layer textures, baked maps, resource files). Self-contained:
//! the original mesh file is not needed to reopen a project.

use crate::bake::BakeSettings;
use crate::camera::OrbitCamera;
use crate::document::{decode_image, Document, Layer, LayerDesc, PaintTex, ResKind, Resource, SetSettings, Target};
use crate::engine::{Engine, GpuMesh, Tex};
use crate::mesh::{MeshData, SubMesh, Vertex};
use anyhow::{bail, Context, Result};
use flate2::{read::DeflateDecoder, write::DeflateEncoder, Compression};
use glam::Vec3;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::Arc;

const MAGIC: &[u8; 8] = b"SPRETRO\x02";

#[derive(Serialize, Deserialize)]
struct Header {
    version: u32,
    mesh_name: String,
    vertex_blob: usize,
    colors_blob: Option<usize>,
    submeshes: Vec<SubHeader>,
    objects: Vec<String>,
    bbox_min: [f32; 3],
    bbox_max: [f32; 3],
    next_id: u64,
    active: usize,
    bake_settings: BakeSettings,
    camera: OrbitCamera,
    /// The packshot studio (older projects: the default kit).
    #[serde(default)]
    studio: crate::studio::Studio,
    sets: Vec<SetHeader>,
    resources: Vec<ResHeader>,
}

#[derive(Serialize, Deserialize)]
struct SubHeader {
    name: String,
    indices: usize,
    objects: usize,
    /// Polygon of each triangle (older projects: rebuilt on load).
    #[serde(default)]
    polys: Option<usize>,
}

#[derive(Serialize, Deserialize)]
struct ResHeader {
    id: u64,
    name: String,
    kind: ResKind,
    blob: usize,
    /// Imported by the user (older projects: told by the name).
    #[serde(default)]
    imported: Option<bool>,
}

#[derive(Serialize, Deserialize)]
struct SetHeader {
    name: String,
    submesh: usize,
    res: u32,
    settings: SetSettings,
    baked: bool,
    /// Mesh maps the user removed (bits in `MESH_MAPS` order).
    #[serde(default)]
    removed_maps: u8,
    /// ao, curvature, normal, world normal, thickness, id
    maps: [usize; 6],
    selected: Option<u64>,
    layers: Vec<LayerSave>,
}

#[derive(Serialize, Deserialize)]
struct LayerSave {
    desc: LayerDesc,
    children: Vec<LayerSave>,
    /// color, vals, alph, emis, nrm, opac
    paint: Option<[Option<usize>; 6]>,
    mask_paint: Vec<(u64, usize)>,
}

struct Blobs(Vec<Vec<u8>>);

impl Blobs {
    fn push(&mut self, data: &[u8]) -> usize {
        let mut enc = DeflateEncoder::new(Vec::new(), Compression::fast());
        enc.write_all(data).unwrap();
        self.0.push(enc.finish().unwrap());
        self.0.len() - 1
    }
    /// Already-compressed file data (images) stored as-is.
    fn push_raw(&mut self, data: &[u8]) -> usize {
        self.push(data)
    }
}

fn save_layer(engine: &Engine, blobs: &mut Blobs, l: &Layer) -> LayerSave {
    let read = |t: &Tex| t.read_bytes(&engine.device, &engine.queue);
    let paint = l.paint.as_ref().map(|p| {
        [
            Some(blobs.push(&read(&p.color))),
            Some(blobs.push(&read(&p.vals))),
            Some(blobs.push(&read(&p.alph))),
            p.emis.as_ref().map(|t| blobs.push(&read(t))),
            p.nrm.as_ref().map(|t| blobs.push(&read(t))),
            p.opac.as_ref().map(|t| blobs.push(&read(t))),
        ]
    });
    let mask_paint = l.mask_paint.iter().map(|(id, t)| (*id, blobs.push(&read(t)))).collect();
    LayerSave {
        desc: l.desc.clone(),
        children: l.children.iter().map(|c| save_layer(engine, blobs, c)).collect(),
        paint,
        mask_paint,
    }
}

pub fn save(engine: &Engine, doc: &Document, camera: &OrbitCamera, path: &Path) -> Result<()> {
    let mut blobs = Blobs(Vec::new());
    let read = |t: &Tex| t.read_bytes(&engine.device, &engine.queue);
    let vertex_blob = blobs.push(bytemuck::cast_slice(&doc.mesh.vertices));
    let colors_blob = (!doc.mesh.colors.is_empty()).then(|| blobs.push(bytemuck::cast_slice(&doc.mesh.colors)));
    let submeshes = doc
        .mesh
        .submeshes
        .iter()
        .map(|s| SubHeader {
            name: s.name.clone(),
            indices: blobs.push(bytemuck::cast_slice(&s.indices)),
            objects: blobs.push(bytemuck::cast_slice(&s.tri_objects)),
            polys: Some(blobs.push(bytemuck::cast_slice(&s.tri_polys))),
        })
        .collect();
    let mut sets = Vec::new();
    for s in &doc.sets {
        let g = &s.gpu;
        let maps = [
            blobs.push(&read(&g.baked_ao)),
            blobs.push(&read(&g.baked_curvature)),
            blobs.push(&read(&g.baked_normal)),
            blobs.push(&read(&g.baked_wnormal)),
            blobs.push(&read(&g.baked_thickness)),
            blobs.push(&read(&g.baked_id)),
        ];
        let layers = s.layers.iter().map(|l| save_layer(engine, &mut blobs, l)).collect();
        sets.push(SetHeader {
            name: s.name.clone(),
            submesh: s.submesh,
            res: s.resolution(),
            settings: s.settings.clone(),
            baked: s.baked,
            removed_maps: s.removed_maps,
            maps,
            selected: s.selected,
            layers,
        });
    }
    let mut resources: Vec<ResHeader> = doc
        .resources
        .values()
        .map(|r| ResHeader { id: r.id, name: r.name.clone(), kind: r.kind, blob: blobs.push_raw(&r.encoded), imported: Some(r.imported) })
        .collect();
    resources.sort_by_key(|r| r.id);
    let header = Header {
        version: 3,
        mesh_name: doc.mesh_name.clone(),
        vertex_blob,
        colors_blob,
        submeshes,
        objects: doc.mesh.objects.clone(),
        bbox_min: doc.mesh.bbox_min.into(),
        bbox_max: doc.mesh.bbox_max.into(),
        next_id: doc.next_id,
        active: doc.active,
        bake_settings: doc.bake_settings.clone(),
        camera: *camera,
        studio: doc.studio.clone(),
        sets,
        resources,
    };
    let json = serde_json::to_vec(&header)?;
    let tmp = path.with_extension("sprp.tmp");
    {
        let mut f = std::io::BufWriter::new(
            std::fs::File::create(&tmp).with_context(|| format!("creating {}", tmp.display()))?,
        );
        f.write_all(MAGIC)?;
        f.write_all(&(json.len() as u64).to_le_bytes())?;
        f.write_all(&json)?;
        f.write_all(&(blobs.0.len() as u64).to_le_bytes())?;
        for b in &blobs.0 {
            f.write_all(&(b.len() as u64).to_le_bytes())?;
            f.write_all(b)?;
        }
        f.flush()?;
    }
    std::fs::rename(&tmp, path).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

struct Reader<'a> {
    data: &'a [u8],
    ranges: Vec<std::ops::Range<usize>>,
}

impl Reader<'_> {
    fn blob(&self, i: usize) -> Result<Vec<u8>> {
        let r = self.ranges.get(i).context("missing blob")?.clone();
        let mut out = Vec::new();
        DeflateDecoder::new(&self.data[r]).read_to_end(&mut out)?;
        Ok(out)
    }
}

fn load_layer(engine: &Engine, rd: &Reader, ls: &LayerSave, res: u32) -> Result<Layer> {
    let paint = match &ls.paint {
        Some(b) => {
            let color = rd.blob(b[0].context("paint color")?)?;
            let full = color.len() as u64 == res as u64 * res as u64 * 8;
            let mut p = if full { PaintTex::new(&engine.device, res) } else { PaintTex::empty(&engine.device) };
            p.color.write_bytes(&engine.queue, &color);
            p.vals.write_bytes(&engine.queue, &rd.blob(b[1].context("paint values")?)?);
            p.alph.write_bytes(&engine.queue, &rd.blob(b[2].context("paint alphas")?)?);
            if b[3].is_some() {
                p.ensure_b(&engine.device);
                for (slot, t) in [(b[3], &p.emis), (b[4], &p.nrm), (b[5], &p.opac)] {
                    if let (Some(i), Some(t)) = (slot, t) {
                        t.write_bytes(&engine.queue, &rd.blob(i)?);
                    }
                }
            }
            Some(p)
        }
        None => None,
    };
    let mut mask_paint = HashMap::new();
    for (id, b) in &ls.mask_paint {
        let bytes = rd.blob(*b)?;
        let full = bytes.len() as u64 == res as u64 * res as u64 * 4;
        let t = crate::document::full_mask_paint(&engine.device, if full { res } else { 1 });
        t.write_bytes(&engine.queue, &bytes);
        mask_paint.insert(*id, t);
    }
    let mut children = Vec::new();
    for c in &ls.children {
        children.push(load_layer(engine, rd, c, res)?);
    }
    Ok(Layer { desc: ls.desc.clone(), rev: 0, paint, mask_paint, children })
}

pub fn load(engine: &Engine, path: &Path) -> Result<(Document, OrbitCamera)> {
    let data = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    if data.len() < 16 || &data[..7] != b"SPRETRO" {
        bail!("Not a Material Forge project");
    }
    if data[7] != MAGIC[7] {
        bail!("This project was saved by an older version (format {}); it cannot be opened.", data[7]);
    }
    let mut pos = 8;
    let take_u64 = |pos: &mut usize| -> Result<u64> {
        let v = u64::from_le_bytes(data.get(*pos..*pos + 8).context("truncated project")?.try_into()?);
        *pos += 8;
        Ok(v)
    };
    let jlen = take_u64(&mut pos)? as usize;
    let header: Header = serde_json::from_slice(data.get(pos..pos + jlen).context("truncated project")?)?;
    pos += jlen;
    let count = take_u64(&mut pos)? as usize;
    if count > data.len() / 8 {
        bail!("corrupt project (blob count)");
    }
    let mut ranges = Vec::with_capacity(count);
    for _ in 0..count {
        let len = take_u64(&mut pos)? as usize;
        let end = pos.checked_add(len).filter(|e| *e <= data.len()).context("truncated project")?;
        ranges.push(pos..end);
        pos = end;
    }
    if pos > data.len() {
        bail!("truncated project");
    }
    let rd = Reader { data: &data, ranges };

    let vertices: Vec<Vertex> = bytemuck::pod_collect_to_vec(&rd.blob(header.vertex_blob)?);
    let colors: Vec<[f32; 3]> = match header.colors_blob {
        Some(b) => bytemuck::pod_collect_to_vec(&rd.blob(b)?),
        None => Vec::new(),
    };
    let mut submeshes = Vec::new();
    for s in &header.submeshes {
        let indices: Vec<u32> = bytemuck::pod_collect_to_vec(&rd.blob(s.indices)?);
        // Polygons of older projects (or a damaged blob) are rebuilt.
        let stored: Option<Vec<u32>> = s.polys.and_then(|b| rd.blob(b).ok()).map(|b| bytemuck::pod_collect_to_vec(&b));
        let tri_polys = match stored {
            Some(p) if p.len() * 3 == indices.len() => p,
            _ => crate::mesh::quad_polys(&vertices, &indices, &[]),
        };
        submeshes.push(SubMesh {
            name: s.name.clone(),
            indices,
            tri_objects: bytemuck::pod_collect_to_vec(&rd.blob(s.objects)?),
            tri_polys,
        });
    }
    let mesh = MeshData {
        vertices,
        submeshes,
        bbox_min: Vec3::from(header.bbox_min),
        bbox_max: Vec3::from(header.bbox_max),
        colors,
        objects: header.objects.clone(),
        notes: Vec::new(),
    };
    let gpu_mesh = GpuMesh::new(&engine.device, &mesh);
    let mut resources = HashMap::new();
    for r in &header.resources {
        let bytes = rd.blob(r.blob)?;
        let img = decode_image(&bytes).with_context(|| format!("resource {}", r.name))?;
        let gpu = crate::engine::texture::rgba8_with_mips(&engine.device, &engine.queue, &r.name, &img);
        resources.insert(
            r.id,
            Resource {
                id: r.id,
                name: r.name.clone(),
                kind: r.kind,
                imported: r.imported.unwrap_or_else(|| !crate::library::is_starter_name(&r.name)),
                encoded: Arc::new(bytes),
                gpu: Arc::new(gpu),
                image: Arc::new(img),
            },
        );
    }
    let mut sets = Vec::new();
    let subs: Vec<(usize, u32)> = header.sets.iter().map(|sh| (sh.submesh, sh.res)).collect();
    for (sh, mut set) in header.sets.iter().zip(Document::make_sets(engine, &mesh, &subs)) {
        set.name = sh.name.clone();
        set.settings = sh.settings.clone();
        if header.version < 3 && set.settings.alpha_mode == crate::document::AlphaMode::Opaque {
            set.settings.alpha_mode = crate::document::AlphaMode::Auto;
        }
        set.baked = sh.baked;
        set.removed_maps = sh.removed_maps;
        if set.baked {
            set.gpu.alloc_baked(&engine.device, None);
        }
        let g = &set.gpu;
        for (tex, b) in [
            (&g.baked_ao, sh.maps[0]),
            (&g.baked_curvature, sh.maps[1]),
            (&g.baked_normal, sh.maps[2]),
            (&g.baked_wnormal, sh.maps[3]),
            (&g.baked_thickness, sh.maps[4]),
            (&g.baked_id, sh.maps[5]),
        ] {
            // Unbaked maps are constants (older files stored them full size).
            let bytes = rd.blob(b)?;
            if bytes.len() as u64 == tex.width as u64 * tex.height as u64 * tex.bytes_per_pixel() as u64 {
                tex.write_bytes(&engine.queue, &bytes);
            }
        }
        for ls in &sh.layers {
            set.layers.push(load_layer(engine, &rd, ls, sh.res)?);
        }
        set.selected = sh.selected;
        set.target = Target::Content;
        set.dirty = true;
        sets.push(set);
    }
    let (bvh, pick_map) = Document::picking(&mesh);
    let doc = Document {
        mesh: Arc::new(mesh),
        gpu_mesh,
        mesh_name: header.mesh_name,
        active: header.active.min(sets.len().saturating_sub(1)),
        sets,
        next_id: header.next_id,
        path: Some(path.to_path_buf()),
        bake_settings: header.bake_settings,
        modified: false,
        resources,
        bvh,
        pick_map,
        hidden: Vec::new(),
        studio: header.studio,
    };
    let mut doc = doc;
    doc.reset_hidden();
    Ok((doc, header.camera))
}
