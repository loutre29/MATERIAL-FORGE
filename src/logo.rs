//! The Material Forge logo: a rounded square tile with "Mf" letters, drawn
//! procedurally (signed distances to strokes, anti-aliased) at any size.

use image::{Rgba, RgbaImage};

/// Tile and letter colors (forge orange on dark charcoal).
const TILE_TOP: [f32; 3] = [0.165, 0.098, 0.063];
const TILE_BOTTOM: [f32; 3] = [0.090, 0.055, 0.039];
const LETTER: [f32; 3] = [1.0, 0.56, 0.18];
const BORDER: [f32; 3] = [1.0, 0.47, 0.12];

fn seg_dist(p: (f32, f32), a: (f32, f32), b: (f32, f32)) -> f32 {
    let (px, py) = (p.0 - a.0, p.1 - a.1);
    let (bx, by) = (b.0 - a.0, b.1 - a.1);
    let h = ((px * bx + py * by) / (bx * bx + by * by)).clamp(0.0, 1.0);
    ((px - bx * h).powi(2) + (py - by * h).powi(2)).sqrt()
}

fn arc_dist(p: (f32, f32), c: (f32, f32), r: f32, a0: f32, a1: f32) -> f32 {
    let (dx, dy) = (p.0 - c.0, p.1 - c.1);
    let ang = dy.atan2(dx);
    if ang >= a0 && ang <= a1 {
        ((dx * dx + dy * dy).sqrt() - r).abs()
    } else {
        let e0 = (c.0 + r * a0.cos(), c.1 + r * a0.sin());
        let e1 = (c.0 + r * a1.cos(), c.1 + r * a1.sin());
        let d0 = ((p.0 - e0.0).powi(2) + (p.1 - e0.1).powi(2)).sqrt();
        let d1 = ((p.0 - e1.0).powi(2) + (p.1 - e1.1).powi(2)).sqrt();
        d0.min(d1)
    }
}

/// Distance to the "Mf" letters in tile units (0..1, y down).
fn letters_dist(p: (f32, f32)) -> f32 {
    let mut d = f32::MAX;
    // M
    let m = [(0.19, 0.70), (0.19, 0.31), (0.345, 0.55), (0.50, 0.31), (0.50, 0.70)];
    for w in m.windows(2) {
        d = d.min(seg_dist(p, w[0], w[1]));
    }
    // f: stem, hook and crossbar.
    d = d.min(seg_dist(p, (0.685, 0.70), (0.685, 0.41)));
    d = d.min(arc_dist(p, (0.76, 0.41), 0.075, -std::f32::consts::PI, -0.45));
    d = d.min(seg_dist(p, (0.615, 0.47), (0.79, 0.47)));
    d
}

/// The logo as an RGBA image of `size` x `size` pixels.
pub fn image(size: u32) -> RgbaImage {
    let n = size as f32;
    let aa = 1.2 / n;
    let radius = 0.18;
    let stroke = 0.036;
    RgbaImage::from_fn(size, size, |x, y| {
        let p = ((x as f32 + 0.5) / n, (y as f32 + 0.5) / n);
        // Rounded square: distance to the inner box minus the corner radius.
        let q = ((p.0 - 0.5).abs() - (0.5 - radius - 0.02), (p.1 - 0.5).abs() - (0.5 - radius - 0.02));
        let outside = (q.0.max(0.0).powi(2) + q.1.max(0.0).powi(2)).sqrt() + q.0.max(q.1).min(0.0) - radius;
        let tile_a = (0.5 - outside / aa).clamp(0.0, 1.0);
        if tile_a <= 0.0 {
            return Rgba([0, 0, 0, 0]);
        }
        let mut c = [
            TILE_TOP[0] + (TILE_BOTTOM[0] - TILE_TOP[0]) * p.1,
            TILE_TOP[1] + (TILE_BOTTOM[1] - TILE_TOP[1]) * p.1,
            TILE_TOP[2] + (TILE_BOTTOM[2] - TILE_TOP[2]) * p.1,
        ];
        // Thin border just inside the edge.
        let border = (1.0 - ((outside + 0.018).abs() - 0.005) / aa).clamp(0.0, 1.0) * 0.75;
        // Letters.
        let ld = letters_dist(p) - stroke;
        let la = (0.5 - ld / aa).clamp(0.0, 1.0);
        for k in 0..3 {
            c[k] = c[k] + (BORDER[k] - c[k]) * border;
            c[k] = c[k] + (LETTER[k] - c[k]) * la;
        }
        let to8 = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
        Rgba([to8(c[0]), to8(c[1]), to8(c[2]), to8(tile_a)])
    })
}

/// Writes the logo as PNG (512 px) and a multi-size ICO into `dir`.
pub fn write_files(dir: &std::path::Path) -> anyhow::Result<()> {
    use image::codecs::ico::{IcoEncoder, IcoFrame};
    std::fs::create_dir_all(dir)?;
    image(512).save(dir.join("material_forge_logo.png"))?;
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
