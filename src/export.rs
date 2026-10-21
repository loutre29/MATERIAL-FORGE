//! Texture export: reads the final maps back from the GPU and packs them
//! into files following an export preset (built-in or user-defined).

use crate::document::{Document, TextureSet};
use crate::engine::{texture::f16_bytes_to_f32, Engine};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Src {
    BaseR,
    BaseG,
    BaseB,
    BaseLuma,
    Opacity,
    Roughness,
    Smoothness,
    Metallic,
    Height,
    Ao,
    NormalX,
    NormalYGl,
    NormalYDx,
    NormalZ,
    EmissiveR,
    EmissiveG,
    EmissiveB,
    SpecularLevel,
    Transmission,
    One,
    Zero,
}

impl Src {
    pub const ALL: [Src; 21] = [
        Src::BaseR,
        Src::BaseG,
        Src::BaseB,
        Src::BaseLuma,
        Src::Opacity,
        Src::Roughness,
        Src::Smoothness,
        Src::Metallic,
        Src::Height,
        Src::Ao,
        Src::NormalX,
        Src::NormalYGl,
        Src::NormalYDx,
        Src::NormalZ,
        Src::EmissiveR,
        Src::EmissiveG,
        Src::EmissiveB,
        Src::SpecularLevel,
        Src::Transmission,
        Src::One,
        Src::Zero,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Src::BaseR => "Base color R",
            Src::BaseG => "Base color G",
            Src::BaseB => "Base color B",
            Src::BaseLuma => "Base color luminance",
            Src::Opacity => "Opacity",
            Src::Roughness => "Roughness",
            Src::Smoothness => "Smoothness (1 - roughness)",
            Src::Metallic => "Metallic",
            Src::Height => "Height",
            Src::Ao => "Ambient occlusion",
            Src::NormalX => "Normal X",
            Src::NormalYGl => "Normal Y (OpenGL)",
            Src::NormalYDx => "Normal Y (DirectX)",
            Src::NormalZ => "Normal Z",
            Src::EmissiveR => "Emissive R",
            Src::EmissiveG => "Emissive G",
            Src::EmissiveB => "Emissive B",
            Src::SpecularLevel => "Specular level",
            Src::Transmission => "Transmission",
            Src::One => "White (1)",
            Src::Zero => "Black (0)",
        }
    }
    /// sRGB-encoded in 8/16-bit integer formats.
    pub fn is_color(self) -> bool {
        matches!(self, Src::BaseR | Src::BaseG | Src::BaseB | Src::EmissiveR | Src::EmissiveG | Src::EmissiveB)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OutputMap {
    pub suffix: String,
    /// 1 (gray), 3 (RGB) or 4 (RGBA) sources.
    pub channels: Vec<Src>,
    pub enabled: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExportPreset {
    pub name: String,
    pub maps: Vec<OutputMap>,
    #[serde(default)]
    pub builtin: bool,
}

fn map(suffix: &str, channels: &[Src]) -> OutputMap {
    OutputMap { suffix: suffix.into(), channels: channels.to_vec(), enabled: true }
}

pub fn builtin_presets() -> Vec<ExportPreset> {
    use Src::*;
    let p = |name: &str, maps: Vec<OutputMap>| ExportPreset { name: name.into(), maps, builtin: true };
    vec![
        p(
            "PBR Metallic Roughness",
            vec![
                map("BaseColor", &[BaseR, BaseG, BaseB]),
                map("Roughness", &[Roughness]),
                map("Metallic", &[Metallic]),
                map("Normal", &[NormalX, NormalYGl, NormalZ]),
                map("Height", &[Height]),
                map("AmbientOcclusion", &[Ao]),
                map("Emissive", &[EmissiveR, EmissiveG, EmissiveB]),
                map("Opacity", &[Opacity]),
            ],
        ),
        p(
            "Unreal Engine (packed)",
            vec![
                map("BaseColor", &[BaseR, BaseG, BaseB]),
                map("OcclusionRoughnessMetallic", &[Ao, Roughness, Metallic]),
                map("Normal", &[NormalX, NormalYDx, NormalZ]),
                map("Emissive", &[EmissiveR, EmissiveG, EmissiveB]),
                map("Height", &[Height]),
            ],
        ),
        p(
            "Unity URP Lit (metallic)",
            vec![
                map("BaseMap", &[BaseR, BaseG, BaseB, Opacity]),
                map("MetallicSmoothness", &[Metallic, Metallic, Metallic, Smoothness]),
                map("Normal", &[NormalX, NormalYGl, NormalZ]),
                map("Occlusion", &[Ao]),
                map("Emission", &[EmissiveR, EmissiveG, EmissiveB]),
                map("Height", &[Height]),
            ],
        ),
        p(
            "Unity HDRP (mask map)",
            vec![
                map("BaseMap", &[BaseR, BaseG, BaseB, Opacity]),
                map("MaskMap", &[Metallic, Ao, Zero, Smoothness]),
                map("Normal", &[NormalX, NormalYGl, NormalZ]),
                map("Emission", &[EmissiveR, EmissiveG, EmissiveB]),
            ],
        ),
        p(
            "glTF PBR",
            vec![
                map("BaseColor", &[BaseR, BaseG, BaseB, Opacity]),
                map("OcclusionRoughnessMetallic", &[Ao, Roughness, Metallic]),
                map("Normal", &[NormalX, NormalYGl, NormalZ]),
                map("Emissive", &[EmissiveR, EmissiveG, EmissiveB]),
            ],
        ),
        p(
            "Godot (ORM)",
            vec![
                map("Albedo", &[BaseR, BaseG, BaseB, Opacity]),
                map("ORM", &[Ao, Roughness, Metallic]),
                map("Normal", &[NormalX, NormalYGl, NormalZ]),
                map("Emission", &[EmissiveR, EmissiveG, EmissiveB]),
                map("Height", &[Height]),
            ],
        ),
        p(
            "Blender Principled BSDF",
            vec![
                map("BaseColor", &[BaseR, BaseG, BaseB]),
                map("Roughness", &[Roughness]),
                map("Metallic", &[Metallic]),
                map("Normal", &[NormalX, NormalYGl, NormalZ]),
                map("Height", &[Height]),
                map("Emission", &[EmissiveR, EmissiveG, EmissiveB]),
                map("Alpha", &[Opacity]),
            ],
        ),
    ]
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FileFormat {
    Png8,
    Png16,
    Tga,
    Jpeg,
    Tiff8,
    Tiff16,
    Exr32,
}

impl FileFormat {
    pub const ALL: [FileFormat; 7] =
        [FileFormat::Png8, FileFormat::Png16, FileFormat::Tga, FileFormat::Jpeg, FileFormat::Tiff8, FileFormat::Tiff16, FileFormat::Exr32];
    pub fn name(self) -> &'static str {
        match self {
            FileFormat::Png8 => "PNG 8-bit",
            FileFormat::Png16 => "PNG 16-bit",
            FileFormat::Tga => "TGA 8-bit",
            FileFormat::Jpeg => "JPEG (quality 95)",
            FileFormat::Tiff8 => "TIFF 8-bit",
            FileFormat::Tiff16 => "TIFF 16-bit",
            FileFormat::Exr32 => "OpenEXR 32-bit float",
        }
    }
    fn ext(self) -> &'static str {
        match self {
            FileFormat::Png8 | FileFormat::Png16 => "png",
            FileFormat::Tga => "tga",
            FileFormat::Jpeg => "jpg",
            FileFormat::Tiff8 | FileFormat::Tiff16 => "tif",
            FileFormat::Exr32 => "exr",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Padding {
    /// Gutters filled from the nearest island texel.
    Infinite,
    /// Dilate this many texels, then transparent / black.
    Dilate(u32),
    /// No padding: outside the UV islands is transparent / black.
    None,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExportOptions {
    pub format: FileFormat,
    /// Power-of-two shift of the output size: -2 quarter .. +1 double.
    pub size_shift: i32,
    pub padding: Padding,
    /// `$mesh`, `$set` and `$map` are replaced.
    pub pattern: String,
}

impl Default for ExportOptions {
    fn default() -> Self {
        ExportOptions { format: FileFormat::Png8, size_shift: 0, padding: Padding::Infinite, pattern: "$mesh_$set_$map".into() }
    }
}

fn linear_to_srgb(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    if x <= 0.0031308 {
        x * 12.92
    } else {
        1.055 * x.powf(1.0 / 2.4) - 0.055
    }
}

pub struct SetMaps {
    pub res: u32,
    pub base: Vec<f32>,
    pub orm: Vec<f32>,
    pub normal: Vec<f32>,
    pub emissive: Vec<f32>,
    /// Texels kept by the padding mode (None = keep all).
    pub keep: Option<Vec<bool>>,
}

pub fn read_maps(engine: &Engine, doc: &Document, set: usize) -> SetMaps {
    let s = &doc.sets[set];
    let read = |t: &crate::engine::Tex| f16_bytes_to_f32(&t.read_bytes(&engine.device, &engine.queue));
    SetMaps {
        res: s.resolution(),
        base: read(&s.gpu.final_base),
        orm: read(&s.gpu.final_orm),
        normal: read(&s.gpu.final_normal),
        emissive: read(&s.gpu.final_emissive),
        keep: None,
    }
}

/// Coverage mask for a padding mode, from the set's UV raster.
pub fn padding_mask(set: &TextureSet, padding: Padding) -> Option<Vec<bool>> {
    match padding {
        Padding::Infinite => None,
        Padding::None => Some(set.raster.tri.iter().map(|t| *t >= 0).collect()),
        Padding::Dilate(px) => {
            let nearest = crate::bake::nearest_map(&set.raster);
            let res = set.raster.res;
            Some(
                nearest
                    .iter()
                    .enumerate()
                    .map(|(i, n)| {
                        let (x, y) = (i as u32 % res, i as u32 / res);
                        (n[0] as i64 - x as i64).abs().max((n[1] as i64 - y as i64).abs()) <= px as i64
                    })
                    .collect(),
            )
        }
    }
}

fn sample(m: &SetMaps, i: usize, s: Src) -> f32 {
    let b = i * 4;
    match s {
        Src::BaseR => m.base[b],
        Src::BaseG => m.base[b + 1],
        Src::BaseB => m.base[b + 2],
        Src::BaseLuma => 0.2126 * m.base[b] + 0.7152 * m.base[b + 1] + 0.0722 * m.base[b + 2],
        Src::Opacity => m.base[b + 3],
        Src::Ao => m.orm[b],
        Src::Roughness => m.orm[b + 1],
        Src::Smoothness => 1.0 - m.orm[b + 1],
        Src::Metallic => m.orm[b + 2],
        Src::Height => (m.orm[b + 3] * 0.5 + 0.5).clamp(0.0, 1.0),
        Src::NormalX => m.normal[b] * 0.5 + 0.5,
        Src::NormalYGl => m.normal[b + 1] * 0.5 + 0.5,
        Src::NormalYDx => -m.normal[b + 1] * 0.5 + 0.5,
        Src::NormalZ => m.normal[b + 2] * 0.5 + 0.5,
        Src::EmissiveR => m.emissive[b],
        Src::EmissiveG => m.emissive[b + 1],
        Src::EmissiveB => m.emissive[b + 2],
        // Optional channels live in the spare .w of the emissive / normal finals.
        Src::SpecularLevel => m.emissive[b + 3],
        Src::Transmission => m.normal[b + 3],
        Src::One => 1.0,
        Src::Zero => 0.0,
    }
}

fn sanitize(s: &str) -> String {
    s.chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect()
}

/// One output map as interleaved f32 values at the set resolution.
fn build(maps: &SetMaps, out: &OutputMap, format: FileFormat) -> (usize, Vec<f32>) {
    let n = (maps.res * maps.res) as usize;
    let ch = match out.channels.len() {
        0 | 1 => 1,
        2 | 3 => 3,
        _ => 4,
    };
    let jpeg = format == FileFormat::Jpeg;
    let ch = if jpeg && ch == 4 { 3 } else { ch };
    let mut data = Vec::with_capacity(n * ch);
    for i in 0..n {
        let keep = maps.keep.as_ref().map(|k| k[i]).unwrap_or(true);
        for c in 0..ch {
            let src = out.channels.get(c).copied().unwrap_or(Src::Zero);
            let mut v = if keep { sample(maps, i, src) } else { 0.0 };
            if c == 3 && !keep {
                v = 0.0;
            }
            if src.is_color() && format != FileFormat::Exr32 {
                v = linear_to_srgb(v);
            }
            data.push(v);
        }
    }
    (ch, data)
}

fn resize(ch: usize, res: u32, data: Vec<f32>, shift: i32) -> (u32, Vec<f32>) {
    if shift == 0 {
        return (res, data);
    }
    let new = if shift > 0 { res << shift } else { (res >> (-shift)).max(1) };
    let img = image::ImageBuffer::<image::Rgba<f32>, Vec<f32>>::from_fn(res, res, |x, y| {
        let i = ((y * res + x) as usize) * ch;
        let g = |k: usize| if k < ch { data[i + k] } else { data[i] };
        image::Rgba([g(0), if ch > 1 { g(1) } else { g(0) }, if ch > 2 { g(2) } else { g(0) }, if ch > 3 { g(3) } else { 1.0 }])
    });
    let r = image::imageops::resize(&img, new, new, image::imageops::FilterType::Triangle);
    let mut out = Vec::with_capacity((new * new) as usize * ch);
    for p in r.pixels() {
        for k in 0..ch {
            out.push(p.0[k]);
        }
    }
    (new, out)
}

fn write(path: &Path, format: FileFormat, ch: usize, res: u32, data: &[f32]) -> Result<()> {
    let q8 = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
    let q16 = |v: f32| (v.clamp(0.0, 1.0) * 65535.0 + 0.5) as u16;
    let err = || anyhow::anyhow!("image buffer size mismatch");
    match format {
        FileFormat::Png8 | FileFormat::Tga | FileFormat::Jpeg | FileFormat::Tiff8 => {
            let d: Vec<u8> = data.iter().map(|&v| q8(v)).collect();
            if format == FileFormat::Jpeg {
                let rgb: image::RgbImage = match ch {
                    1 => image::DynamicImage::ImageLuma8(image::GrayImage::from_raw(res, res, d).ok_or_else(err)?).to_rgb8(),
                    _ => image::RgbImage::from_raw(res, res, d).ok_or_else(err)?,
                };
                let f = std::fs::File::create(path)?;
                let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(std::io::BufWriter::new(f), 95);
                enc.encode_image(&rgb)?;
                return Ok(());
            }
            match ch {
                1 => image::GrayImage::from_raw(res, res, d).ok_or_else(err)?.save(path)?,
                3 => image::RgbImage::from_raw(res, res, d).ok_or_else(err)?.save(path)?,
                _ => image::RgbaImage::from_raw(res, res, d).ok_or_else(err)?.save(path)?,
            }
        }
        FileFormat::Png16 | FileFormat::Tiff16 => {
            let d: Vec<u16> = data.iter().map(|&v| q16(v)).collect();
            match ch {
                1 => image::ImageBuffer::<image::Luma<u16>, _>::from_raw(res, res, d).ok_or_else(err)?.save(path)?,
                3 => image::ImageBuffer::<image::Rgb<u16>, _>::from_raw(res, res, d).ok_or_else(err)?.save(path)?,
                _ => image::ImageBuffer::<image::Rgba<u16>, _>::from_raw(res, res, d).ok_or_else(err)?.save(path)?,
            }
        }
        FileFormat::Exr32 => {
            if ch == 4 {
                image::Rgba32FImage::from_raw(res, res, data.to_vec()).ok_or_else(err)?.save(path)?;
            } else {
                // Gray maps are written as RGB with the value replicated.
                let d: Vec<f32> = if ch == 1 { data.iter().flat_map(|&v| [v, v, v]).collect() } else { data.to_vec() };
                image::Rgb32FImage::from_raw(res, res, d).ok_or_else(err)?.save(path)?;
            }
        }
    }
    Ok(())
}

pub fn export_set(
    maps: &SetMaps,
    dir: &Path,
    mesh_name: &str,
    set_name: &str,
    preset: &ExportPreset,
    opts: &ExportOptions,
) -> Result<Vec<PathBuf>> {
    std::fs::create_dir_all(dir)?;
    let mut written = Vec::new();
    for out in preset.maps.iter().filter(|m| m.enabled && !m.channels.is_empty()) {
        let name = opts
            .pattern
            .replace("$mesh", &sanitize(mesh_name))
            .replace("$set", &sanitize(set_name))
            .replace("$map", &sanitize(&out.suffix));
        let path = dir.join(format!("{}.{}", name, opts.format.ext()));
        let (ch, data) = build(maps, out, opts.format);
        let (res, data) = resize(ch, maps.res, data, opts.size_shift);
        write(&path, opts.format, ch, res, &data).with_context(|| format!("writing {}", path.display()))?;
        written.push(path);
    }
    Ok(written)
}

/// User presets live next to the library (JSON).
pub fn load_user_presets(dir: &Path) -> Vec<ExportPreset> {
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut out: Vec<ExportPreset> = rd
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .filter_map(|e| serde_json::from_slice::<ExportPreset>(&std::fs::read(e.path()).ok()?).ok())
        .map(|mut p| {
            p.builtin = false;
            p
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// Removes a user preset's file (a preset never saved has none).
pub fn delete_user_preset(dir: &Path, name: &str) -> Result<()> {
    let path = dir.join(format!("{}.json", sanitize(name)));
    if path.exists() {
        std::fs::remove_file(&path)?;
    }
    Ok(())
}

pub fn save_user_preset(dir: &Path, preset: &ExportPreset) -> Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join(format!("{}.json", sanitize(&preset.name)));
    std::fs::write(&path, serde_json::to_vec_pretty(preset)?)?;
    Ok(path)
}
