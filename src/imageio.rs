//! Image files: what the `image` crate reads (PNG, JPEG, TGA, TIFF, BMP,
//! EXR, HDR, WebP), plus AVIF, decoded by rav1d (a Rust port of the dav1d
//! AV1 decoder) through `vendor/avif-decode` (built without assembly, so no
//! NASM is needed).

use anyhow::{Context, Result};
use image::DynamicImage;
use std::path::Path;

/// An ISO BMFF file whose `ftyp` box names an AVIF brand.
fn is_avif(b: &[u8]) -> bool {
    if b.len() < 16 || &b[4..8] != b"ftyp" {
        return false;
    }
    let size = (u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as usize).clamp(16, b.len());
    // Major brand, then the compatible brands (after the minor version).
    std::iter::once(&b[8..12]).chain(b[16..size].chunks_exact(4)).any(|brand| brand == b"avif" || brand == b"avis")
}

fn decode_avif(b: &[u8]) -> Result<DynamicImage> {
    use avif_decode::Image as I;
    let img = avif_decode::Decoder::from_avif(b).and_then(|d| d.to_image()).map_err(|e| anyhow::anyhow!("AVIF: {e}"))?;
    let bad = || anyhow::anyhow!("AVIF: unexpected pixel buffer size");
    Ok(match img {
        I::Rgb8(i) => {
            let (buf, w, h) = i.into_contiguous_buf();
            DynamicImage::ImageRgb8(image::RgbImage::from_raw(w as u32, h as u32, bytemuck::cast_slice(&buf).to_vec()).ok_or_else(bad)?)
        }
        I::Rgba8(i) => {
            let (buf, w, h) = i.into_contiguous_buf();
            DynamicImage::ImageRgba8(image::RgbaImage::from_raw(w as u32, h as u32, bytemuck::cast_slice(&buf).to_vec()).ok_or_else(bad)?)
        }
        I::Gray8(i) => {
            let (buf, w, h) = i.into_contiguous_buf();
            DynamicImage::ImageLuma8(image::GrayImage::from_raw(w as u32, h as u32, bytemuck::cast_slice(&buf).to_vec()).ok_or_else(bad)?)
        }
        I::Rgb16(i) => {
            let (buf, w, h) = i.into_contiguous_buf();
            DynamicImage::ImageRgb16(image::ImageBuffer::from_raw(w as u32, h as u32, bytemuck::cast_slice::<_, u16>(&buf).to_vec()).ok_or_else(bad)?)
        }
        I::Rgba16(i) => {
            let (buf, w, h) = i.into_contiguous_buf();
            DynamicImage::ImageRgba16(image::ImageBuffer::from_raw(w as u32, h as u32, bytemuck::cast_slice::<_, u16>(&buf).to_vec()).ok_or_else(bad)?)
        }
        I::Gray16(i) => {
            let (buf, w, h) = i.into_contiguous_buf();
            DynamicImage::ImageLuma16(image::ImageBuffer::from_raw(w as u32, h as u32, bytemuck::cast_slice::<_, u16>(&buf).to_vec()).ok_or_else(bad)?)
        }
    })
}

/// An image file's bytes (the format is told by its content).
pub fn decode(bytes: &[u8]) -> Result<DynamicImage> {
    if is_avif(bytes) {
        return decode_avif(bytes);
    }
    Ok(image::load_from_memory(bytes)?)
}

/// An image file (TGA has no signature: its extension tells it).
pub fn open(path: &Path) -> Result<DynamicImage> {
    let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    if is_avif(&bytes) {
        return decode_avif(&bytes);
    }
    let reader = image::ImageReader::new(std::io::Cursor::new(&bytes)).with_guessed_format()?;
    let reader = match (reader.format(), image::ImageFormat::from_path(path)) {
        (None, Ok(f)) => {
            let mut r = image::ImageReader::new(std::io::Cursor::new(&bytes));
            r.set_format(f);
            r
        }
        _ => reader,
    };
    Ok(reader.decode()?)
}
