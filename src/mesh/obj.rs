//! Wavefront OBJ reader. Texture sets come from `usemtl` names, so the MTL
//! file does not need to be present.

use super::RawMesh;
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::Path;

pub fn load(path: &Path) -> Result<RawMesh> {
    let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let text = String::from_utf8_lossy(&bytes);

    let mut pos: Vec<[f32; 3]> = Vec::new();
    let mut vcol: Vec<[f32; 3]> = Vec::new();
    let mut has_vcol = false;
    let mut object = 0u32;
    let mut tex: Vec<[f32; 2]> = Vec::new();
    let mut nor: Vec<[f32; 3]> = Vec::new();

    let mut raw = RawMesh::default();
    let mut corner_map: HashMap<(usize, usize, usize), u32> = HashMap::new();
    let mut group_of: HashMap<String, usize> = HashMap::new();
    let mut current = "DefaultMaterial".to_string();
    let mut missing_uv = false;
    let mut missing_normal = false;

    let parse_f = |s: Option<&str>| s.and_then(|v| v.parse::<f32>().ok()).unwrap_or(0.0);

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut it = line.split_whitespace();
        let Some(tag) = it.next() else { continue };
        match tag {
            "v" => {
                pos.push([parse_f(it.next()), parse_f(it.next()), parse_f(it.next())]);
                let rest: Vec<f32> = it.filter_map(|v| v.parse().ok()).collect();
                if rest.len() >= 3 {
                    has_vcol = true;
                    vcol.push([rest[0], rest[1], rest[2]]);
                } else {
                    vcol.push([1.0; 3]);
                }
            }
            "o" | "g" => {
                let name = line[1..].trim();
                if !name.is_empty() {
                    object = raw.object(name);
                }
            }
            "vt" => tex.push([parse_f(it.next()), parse_f(it.next())]),
            "vn" => nor.push([parse_f(it.next()), parse_f(it.next()), parse_f(it.next())]),
            "usemtl" => {
                let name = line[6..].trim();
                current = if name.is_empty() { "DefaultMaterial".into() } else { name.to_string() };
            }
            "f" => {
                let resolve = |s: &str, len: usize| -> Option<usize> {
                    let i: i64 = s.parse().ok()?;
                    if i > 0 {
                        Some(i as usize - 1)
                    } else if i < 0 {
                        (len as i64 + i).try_into().ok()
                    } else {
                        None
                    }
                };
                let mut corners: Vec<u32> = Vec::new();
                for token in it {
                    let mut parts = token.split('/');
                    let Some(vi) = parts.next().and_then(|s| resolve(s, pos.len())) else { continue };
                    let ti = parts.next().filter(|s| !s.is_empty()).and_then(|s| resolve(s, tex.len()));
                    let ni = parts.next().filter(|s| !s.is_empty()).and_then(|s| resolve(s, nor.len()));
                    if vi >= pos.len() {
                        continue;
                    }
                    let ti = ti.filter(|&t| t < tex.len());
                    let ni = ni.filter(|&n| n < nor.len());
                    missing_uv |= ti.is_none();
                    missing_normal |= ni.is_none();
                    let key = (vi, ti.map_or(0, |t| t + 1), ni.map_or(0, |n| n + 1));
                    let idx = *corner_map.entry(key).or_insert_with(|| {
                        raw.positions.push(pos[vi]);
                        let t = ti.map_or([0.0, 0.0], |t| tex[t]);
                        // OBJ UVs are OpenGL style (v up); store image space.
                        raw.uvs.push([t[0], 1.0 - t[1]]);
                        raw.normals.push(ni.map_or([0.0, 1.0, 0.0], |n| nor[n]));
                        raw.normal_missing.push(ni.is_none());
                        raw.colors.push(vcol[vi]);
                        (raw.positions.len() - 1) as u32
                    });
                    corners.push(idx);
                }
                if corners.len() < 3 {
                    continue;
                }
                let g = *group_of.entry(current.clone()).or_insert_with(|| {
                    raw.groups.push((current.clone(), Vec::new(), Vec::new()));
                    raw.polys.push(Vec::new());
                    raw.groups.len() - 1
                });
                let poly = raw.next_poly;
                raw.next_poly += 1;
                for k in 1..corners.len() - 1 {
                    raw.groups[g].1.extend_from_slice(&[corners[0], corners[k], corners[k + 1]]);
                    raw.groups[g].2.push(object);
                    raw.polys[g].push(poly);
                }
            }
            _ => {}
        }
    }

    if missing_uv {
        if tex.is_empty() {
            raw.uvs.clear();
        } else {
            raw.notes.push("Some faces have no UVs; they were placed at UV (0,0).".into());
        }
    }
    if !missing_normal {
        raw.normal_missing.clear();
    }
    if !has_vcol {
        raw.colors.clear();
    }
    Ok(raw)
}
