//! Binary FBX reader (versions 7.x), written from the publicly documented
//! node-record layout. Reads mesh geometry, normals, the first UV layer,
//! per-polygon materials and the model hierarchy transforms.

use super::RawMesh;
use anyhow::{bail, Context, Result};
use glam::{EulerRot, Mat3, Mat4, Vec3};
use std::collections::HashMap;
use std::io::Read;
use std::path::Path;

#[derive(Debug, Clone)]
enum Prop {
    I16(i16),
    Bool(bool),
    I32(i32),
    F32(f32),
    F64(f64),
    I64(i64),
    Str(String),
    Raw,
    F32Arr(Vec<f32>),
    F64Arr(Vec<f64>),
    I64Arr(Vec<i64>),
    I32Arr(Vec<i32>),
    BoolArr,
}

impl Prop {
    fn as_f64(&self) -> Option<f64> {
        Some(match self {
            Prop::I16(v) => *v as f64,
            Prop::I32(v) => *v as f64,
            Prop::I64(v) => *v as f64,
            Prop::F32(v) => *v as f64,
            Prop::F64(v) => *v,
            Prop::Bool(v) => *v as i32 as f64,
            _ => return None,
        })
    }
    fn as_i64(&self) -> Option<i64> {
        match self {
            Prop::I64(v) => Some(*v),
            Prop::I32(v) => Some(*v as i64),
            _ => None,
        }
    }
    fn as_str(&self) -> Option<&str> {
        match self {
            Prop::Str(s) => Some(s),
            _ => None,
        }
    }
    fn as_f64_vec(&self) -> Option<Vec<f64>> {
        match self {
            Prop::F64Arr(v) => Some(v.clone()),
            Prop::F32Arr(v) => Some(v.iter().map(|&x| x as f64).collect()),
            _ => None,
        }
    }
    fn as_i32_vec(&self) -> Option<Vec<i32>> {
        match self {
            Prop::I32Arr(v) => Some(v.clone()),
            Prop::I64Arr(v) => Some(v.iter().map(|&x| x as i32).collect()),
            _ => None,
        }
    }
}

#[derive(Debug)]
struct Node {
    name: String,
    props: Vec<Prop>,
    children: Vec<Node>,
}

impl Node {
    fn child(&self, name: &str) -> Option<&Node> {
        self.children.iter().find(|c| c.name == name)
    }
    fn prop_str(&self, i: usize) -> Option<&str> {
        self.props.get(i).and_then(|p| p.as_str())
    }
}

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn bytes(&mut self, n: usize) -> Result<&'a [u8]> {
        if self.pos + n > self.data.len() {
            bail!("FBX file is truncated");
        }
        let s = &self.data[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8> {
        Ok(self.bytes(1)?[0])
    }
    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.bytes(4)?.try_into()?))
    }
    fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_le_bytes(self.bytes(8)?.try_into()?))
    }
}

fn parse(data: &[u8]) -> Result<Vec<Node>> {
    if data.len() < 27 || &data[0..20] != b"Kaydara FBX Binary  " {
        bail!("Only binary FBX files are supported (re-export as binary FBX, OBJ or glTF)");
    }
    let version = u32::from_le_bytes(data[23..27].try_into()?);
    let mut r = Reader { data, pos: 27 };
    let header = if version >= 7500 { 25 } else { 13 };
    let mut nodes = Vec::new();
    while r.pos + header <= data.len() {
        match read_node(&mut r, version)? {
            Some(n) => nodes.push(n),
            None => break,
        }
    }
    Ok(nodes)
}

fn read_node(r: &mut Reader, version: u32) -> Result<Option<Node>> {
    let (end, nprops) = if version >= 7500 {
        let end = r.u64()?;
        let n = r.u64()?;
        r.u64()?;
        (end, n)
    } else {
        let end = r.u32()? as u64;
        let n = r.u32()? as u64;
        r.u32()?;
        (end, n)
    };
    let name_len = r.u8()? as usize;
    if end == 0 {
        return Ok(None);
    }
    let name = String::from_utf8_lossy(r.bytes(name_len)?).into_owned();
    let mut props = Vec::with_capacity(nprops as usize);
    for _ in 0..nprops {
        props.push(read_prop(r)?);
    }
    let mut children = Vec::new();
    while (r.pos as u64) < end {
        match read_node(r, version)? {
            Some(c) => children.push(c),
            None => break,
        }
    }
    r.pos = end as usize;
    Ok(Some(Node { name, props, children }))
}

fn read_prop(r: &mut Reader) -> Result<Prop> {
    let t = r.u8()?;
    Ok(match t {
        b'Y' => Prop::I16(i16::from_le_bytes(r.bytes(2)?.try_into()?)),
        b'C' => Prop::Bool(r.u8()? != 0),
        b'I' => Prop::I32(i32::from_le_bytes(r.bytes(4)?.try_into()?)),
        b'F' => Prop::F32(f32::from_le_bytes(r.bytes(4)?.try_into()?)),
        b'D' => Prop::F64(f64::from_le_bytes(r.bytes(8)?.try_into()?)),
        b'L' => Prop::I64(i64::from_le_bytes(r.bytes(8)?.try_into()?)),
        b'S' => {
            let n = r.u32()? as usize;
            Prop::Str(String::from_utf8_lossy(r.bytes(n)?).into_owned())
        }
        b'R' => {
            let n = r.u32()? as usize;
            r.bytes(n)?;
            Prop::Raw
        }
        b'f' | b'd' | b'l' | b'i' | b'b' => {
            let count = r.u32()? as usize;
            let encoding = r.u32()?;
            let clen = r.u32()? as usize;
            let stored = r.bytes(clen)?;
            let elem = match t {
                b'f' | b'i' => 4,
                b'd' | b'l' => 8,
                _ => 1,
            };
            let bytes: Vec<u8> = if encoding == 1 {
                let mut out = Vec::with_capacity(count * elem);
                flate2::read::ZlibDecoder::new(stored).read_to_end(&mut out)?;
                out
            } else {
                stored.to_vec()
            };
            if bytes.len() < count * elem {
                bail!("FBX array is shorter than declared");
            }
            let b = &bytes[..count * elem];
            match t {
                b'f' => Prop::F32Arr(b.chunks_exact(4).map(|c| f32::from_le_bytes(c.try_into().unwrap())).collect()),
                b'd' => Prop::F64Arr(b.chunks_exact(8).map(|c| f64::from_le_bytes(c.try_into().unwrap())).collect()),
                b'l' => Prop::I64Arr(b.chunks_exact(8).map(|c| i64::from_le_bytes(c.try_into().unwrap())).collect()),
                b'i' => Prop::I32Arr(b.chunks_exact(4).map(|c| i32::from_le_bytes(c.try_into().unwrap())).collect()),
                _ => Prop::BoolArr,
            }
        }
        other => bail!("Unknown FBX property type 0x{other:02x}"),
    })
}

/// "Name\0\x01Class" -> "Name"
fn object_name(n: &Node) -> String {
    n.prop_str(1).unwrap_or("").split('\0').next().unwrap_or("").to_string()
}

fn properties70(n: &Node) -> HashMap<String, [f64; 3]> {
    let mut out = HashMap::new();
    if let Some(p70) = n.child("Properties70") {
        for p in p70.children.iter().filter(|c| c.name == "P") {
            let Some(name) = p.prop_str(0) else { continue };
            let vals: Vec<f64> = p.props.iter().skip(4).filter_map(|v| v.as_f64()).collect();
            if !vals.is_empty() {
                let mut v = [0.0; 3];
                for (i, x) in vals.iter().take(3).enumerate() {
                    v[i] = *x;
                }
                out.insert(name.to_string(), v);
            }
        }
    }
    out
}

fn euler_xyz(deg: [f64; 3]) -> Mat4 {
    Mat4::from_euler(
        EulerRot::ZYX,
        (deg[2] as f32).to_radians(),
        (deg[1] as f32).to_radians(),
        (deg[0] as f32).to_radians(),
    )
}

fn vec3(v: [f64; 3]) -> Vec3 {
    Vec3::new(v[0] as f32, v[1] as f32, v[2] as f32)
}

fn local_matrix(model: &Node) -> Mat4 {
    let p = properties70(model);
    let get = |k: &str, d: [f64; 3]| p.get(k).copied().unwrap_or(d);
    let t = vec3(get("Lcl Translation", [0.0; 3]));
    let r = euler_xyz(get("Lcl Rotation", [0.0; 3]));
    let s = vec3(get("Lcl Scaling", [1.0; 3]));
    let pre = euler_xyz(get("PreRotation", [0.0; 3]));
    let post = euler_xyz(get("PostRotation", [0.0; 3]));
    let roff = vec3(get("RotationOffset", [0.0; 3]));
    let rpiv = vec3(get("RotationPivot", [0.0; 3]));
    let soff = vec3(get("ScalingOffset", [0.0; 3]));
    let spiv = vec3(get("ScalingPivot", [0.0; 3]));
    Mat4::from_translation(t + roff + rpiv)
        * pre
        * r
        * post.inverse()
        * Mat4::from_translation(-rpiv + soff + spiv)
        * Mat4::from_scale(s)
        * Mat4::from_translation(-spiv)
}

fn geometric_matrix(model: &Node) -> Mat4 {
    let p = properties70(model);
    let get = |k: &str, d: [f64; 3]| p.get(k).copied().unwrap_or(d);
    Mat4::from_translation(vec3(get("GeometricTranslation", [0.0; 3])))
        * euler_xyz(get("GeometricRotation", [0.0; 3]))
        * Mat4::from_scale(vec3(get("GeometricScaling", [1.0; 3])))
}

#[derive(Clone, Copy, PartialEq)]
enum Mapping {
    PolygonVertex,
    Vertex,
    Polygon,
    AllSame,
}

struct Element {
    data: Vec<f64>,
    index: Option<Vec<i32>>,
    mapping: Mapping,
    stride: usize,
}

impl Element {
    fn read(geom: &Node, layer: &str, data: &str, index: &str, stride: usize) -> Option<Element> {
        let node = geom.children.iter().find(|c| c.name == layer)?;
        let values = node.child(data)?.props.first()?.as_f64_vec()?;
        let mapping = match node.child("MappingInformationType").and_then(|n| n.prop_str(0)) {
            Some("ByPolygonVertex") => Mapping::PolygonVertex,
            Some("ByVertice") | Some("ByVertex") | Some("ByControlPoint") => Mapping::Vertex,
            Some("ByPolygon") => Mapping::Polygon,
            Some("AllSame") => Mapping::AllSame,
            _ => Mapping::PolygonVertex,
        };
        let by_index = matches!(
            node.child("ReferenceInformationType").and_then(|n| n.prop_str(0)),
            Some("IndexToDirect") | Some("Index")
        );
        let index = if by_index {
            node.child(index).and_then(|n| n.props.first()).and_then(|p| p.as_i32_vec())
        } else {
            None
        };
        Some(Element { data: values, index, mapping, stride })
    }

    fn get(&self, pv: usize, cp: usize, poly: usize) -> Option<&[f64]> {
        let i = match self.mapping {
            Mapping::PolygonVertex => pv,
            Mapping::Vertex => cp,
            Mapping::Polygon => poly,
            Mapping::AllSame => 0,
        };
        let i = match &self.index {
            Some(ix) => *ix.get(i)? as usize,
            None => i,
        };
        self.data.get(i * self.stride..i * self.stride + self.stride)
    }
}

pub fn load(path: &Path) -> Result<RawMesh> {
    let data = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let nodes = parse(&data)?;
    let objects = nodes.iter().find(|n| n.name == "Objects").context("FBX has no Objects section")?;

    let mut geometries: HashMap<i64, &Node> = HashMap::new();
    let mut models: HashMap<i64, &Node> = HashMap::new();
    let mut materials: HashMap<i64, String> = HashMap::new();
    for o in &objects.children {
        let Some(id) = o.props.first().and_then(|p| p.as_i64()) else { continue };
        match o.name.as_str() {
            "Geometry" if o.prop_str(2) == Some("Mesh") => {
                geometries.insert(id, o);
            }
            "Model" => {
                models.insert(id, o);
            }
            "Material" => {
                materials.insert(id, object_name(o));
            }
            _ => {}
        }
    }

    let mut geom_model: Vec<(i64, i64)> = Vec::new();
    let mut parent_of: HashMap<i64, i64> = HashMap::new();
    let mut model_mats: HashMap<i64, Vec<i64>> = HashMap::new();
    if let Some(conns) = nodes.iter().find(|n| n.name == "Connections") {
        for c in conns.children.iter().filter(|c| c.name == "C") {
            let (Some(child), Some(parent)) =
                (c.props.get(1).and_then(|p| p.as_i64()), c.props.get(2).and_then(|p| p.as_i64()))
            else {
                continue;
            };
            if geometries.contains_key(&child) && models.contains_key(&parent) {
                geom_model.push((child, parent));
            } else if models.contains_key(&child) && models.contains_key(&parent) {
                parent_of.insert(child, parent);
            } else if materials.contains_key(&child) && models.contains_key(&parent) {
                model_mats.entry(parent).or_default().push(child);
            }
        }
    }

    let global = |mut id: i64| -> Mat4 {
        let mut m = Mat4::IDENTITY;
        for _ in 0..256 {
            let Some(model) = models.get(&id) else { break };
            m = local_matrix(model) * m;
            match parent_of.get(&id) {
                Some(&p) => id = p,
                None => break,
            }
        }
        m
    };

    let mut raw = RawMesh::default();
    let mut any_uv = true;
    let mut any_normal = true;
    let mut any_color = false;
    let mut group_of: HashMap<String, usize> = HashMap::new();

    for (gid, mid) in geom_model {
        let geom = geometries[&gid];
        let model = models[&mid];
        let world = global(mid) * geometric_matrix(model);
        let normal_m = Mat3::from_mat4(world).inverse().transpose();
        let flip = world.determinant() < 0.0;

        let Some(verts) = geom.child("Vertices").and_then(|n| n.props.first()).and_then(|p| p.as_f64_vec())
        else {
            continue;
        };
        let Some(pvi) =
            geom.child("PolygonVertexIndex").and_then(|n| n.props.first()).and_then(|p| p.as_i32_vec())
        else {
            continue;
        };
        let normals = Element::read(geom, "LayerElementNormal", "Normals", "NormalsIndex", 3);
        let uvs = Element::read(geom, "LayerElementUV", "UV", "UVIndex", 2);
        let colors = Element::read(geom, "LayerElementColor", "Colors", "ColorIndex", 4);
        any_color |= colors.is_some();
        let mats = geom.children.iter().find(|c| c.name == "LayerElementMaterial").and_then(|n| {
            let ids = n.child("Materials")?.props.first()?.as_i32_vec()?;
            let all_same = n.child("MappingInformationType").and_then(|m| m.prop_str(0)) == Some("AllSame");
            Some((ids, all_same))
        });
        any_uv &= uvs.is_some();
        any_normal &= normals.is_some();
        let mat_list = model_mats.get(&mid).cloned().unwrap_or_default();
        let model_name = object_name(model);
        let object = raw.object(if model_name.is_empty() { "Object" } else { &model_name });

        let mut poly_start = 0usize;
        let mut poly = 0usize;
        for (pv, &raw_idx) in pvi.iter().enumerate() {
            if raw_idx >= 0 {
                continue;
            }
            // pv closes polygon `poly` spanning poly_start..=pv
            let mut corners = Vec::with_capacity(pv - poly_start + 1);
            for k in poly_start..=pv {
                let ci = pvi[k];
                let cp = if ci < 0 { !ci } else { ci } as usize;
                let p = verts.get(cp * 3..cp * 3 + 3).map_or([0.0; 3], |v| [v[0], v[1], v[2]]);
                let pos = world.transform_point3(vec3(p));
                let n = normals
                    .as_ref()
                    .and_then(|e| e.get(k, cp, poly))
                    .map_or(Vec3::Y, |v| vec3([v[0], v[1], v[2]]));
                let uv = uvs.as_ref().and_then(|e| e.get(k, cp, poly)).map_or([0.0, 0.0], |v| {
                    [v[0] as f32, 1.0 - v[1] as f32]
                });
                raw.positions.push(pos.into());
                raw.normals.push((normal_m * n).normalize_or(Vec3::Y).into());
                raw.uvs.push(uv);
                raw.colors.push(
                    colors
                        .as_ref()
                        .and_then(|e| e.get(k, cp, poly))
                        .map_or([1.0; 3], |c| [c[0] as f32, c[1] as f32, c[2] as f32]),
                );
                corners.push((raw.positions.len() - 1) as u32);
            }
            let mat_slot = match &mats {
                Some((ids, true)) => ids.first().copied().unwrap_or(0),
                Some((ids, false)) => ids.get(poly).copied().unwrap_or(0),
                None => 0,
            };
            let name = mat_list
                .get(mat_slot.max(0) as usize)
                .and_then(|m| materials.get(m))
                .cloned()
                .unwrap_or_else(|| if model_name.is_empty() { "DefaultMaterial".into() } else { model_name.clone() });
            let g = *group_of.entry(name.clone()).or_insert_with(|| {
                raw.groups.push((name.clone(), Vec::new(), Vec::new()));
                raw.groups.len() - 1
            });
            for k in 1..corners.len().saturating_sub(1) {
                let (a, b, c) = (corners[0], corners[k], corners[k + 1]);
                if flip {
                    raw.groups[g].1.extend_from_slice(&[a, c, b]);
                } else {
                    raw.groups[g].1.extend_from_slice(&[a, b, c]);
                }
                raw.groups[g].2.push(object);
            }
            poly_start = pv + 1;
            poly += 1;
        }
    }

    if !any_uv {
        raw.uvs.clear();
    }
    if !any_normal {
        raw.normals.clear();
    }
    if !any_color {
        raw.colors.clear();
    }
    if raw.positions.is_empty() {
        bail!("No mesh geometry found in this FBX");
    }
    Ok(raw)
}
