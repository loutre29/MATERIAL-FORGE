//! The Material Forge logo: `assets/material_forge_logo.png` (1024 px,
//! square, transparent corners) embedded in the executable and resized on
//! demand.

use image::RgbaImage;

const LOGO_PNG: &[u8] = include_bytes!("../assets/material_forge_logo.png");

/// The logo as an RGBA image of `size` x `size` pixels.
pub fn image(size: u32) -> RgbaImage {
    let full = image::load_from_memory(LOGO_PNG).expect("embedded logo").to_rgba8();
    if full.width() == size && full.height() == size {
        return full;
    }
    image::imageops::resize(&full, size, size, image::imageops::FilterType::Lanczos3)
}

/// Writes the logo as PNG (512 px) and a multi-size ICO into `dir`.
pub fn write_files(dir: &std::path::Path) -> anyhow::Result<()> {
    use image::codecs::ico::{IcoEncoder, IcoFrame};
    std::fs::create_dir_all(dir)?;
    image(512).save(dir.join("material_forge_logo_512.png"))?;
    let frames: Vec<IcoFrame> = [256u32, 64, 48, 32, 16]
        .iter()
        .map(|&s| IcoFrame::as_png(image(s).as_raw(), s, s, image::ExtendedColorType::Rgba8))
        .collect::<Result<_, _>>()?;
    let f = std::fs::File::create(dir.join("material_forge.ico"))?;
    IcoEncoder::new(f).encode_images(&frames)?;
    Ok(())
}

/// Window icon data for eframe.
pub fn icon_data() -> eframe::egui::IconData {
    let img = image(128);
    eframe::egui::IconData { rgba: img.into_raw(), width: 128, height: 128 }
}
