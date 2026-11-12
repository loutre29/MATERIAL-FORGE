//! Product photos as print artwork: the object found on its background
//! (a face of a box on a table), the photo's color cast, and the turns of
//! a product's face photos that make their prints meet across its edges.

use glam::{Vec2, Vec3};
use image::RgbaImage;

/// An 8 bit sRGB color in CIE Lab.
pub fn lab8(p: [u8; 3]) -> [f32; 3] {
    let lin = |c: u8| {
        let c = c as f32 / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    let [r, g, b] = p.map(lin);
    let x = (0.4124 * r + 0.3576 * g + 0.1805 * b) / 0.9505;
    let y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    let z = (0.0193 * r + 0.1192 * g + 0.9505 * b) / 1.089;
    let f = |t: f32| if t > 0.008856 { t.cbrt() } else { 7.787 * t + 16.0 / 116.0 };
    [116.0 * f(y) - 16.0, 500.0 * (f(x) - f(y)), 200.0 * (f(y) - f(z))]
}

fn dist(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

/// The object a photo shows on its background: its outline's four
/// corners (top left, top right, bottom right, bottom left, the photo's
/// pixels), the share of the photo it covers and how much of that
/// four-cornered shape it fills (near 1 for a flat face seen square on).
pub struct Found {
    pub quad: [[f32; 2]; 4],
    pub share: f32,
    pub fill: f32,
    /// Light, nearly colorless flat areas on the object, four-cornered
    /// (paper labels, the usual print sources), largest first.
    pub labels: Vec<[[f32; 2]; 4]>,
}

/// Grows (`on` true) or shrinks the mask by one pixel (3 x 3).
fn morph(m: &[bool], w: usize, h: usize, on: bool) -> Vec<bool> {
    let mut out = m.to_vec();
    for y in 0..h {
        for x in 0..w {
            if m[y * w + x] == on {
                continue;
            }
            let near = (y.saturating_sub(1)..(y + 2).min(h)).any(|yy| (x.saturating_sub(1)..(x + 2).min(w)).any(|xx| m[yy * w + xx] == on));
            if near {
                out[y * w + x] = on;
            }
        }
    }
    out
}

/// The connected parts of the mask (4 neighbors): a label per pixel
/// (0 none) and each part's size.
fn parts(m: &[bool], w: usize, h: usize) -> (Vec<u32>, Vec<usize>) {
    let mut label = vec![0u32; w * h];
    let mut sizes = vec![0usize];
    for i0 in 0..w * h {
        if !m[i0] || label[i0] != 0 {
            continue;
        }
        let id = sizes.len() as u32;
        let mut n = 0;
        let mut stack = vec![i0];
        label[i0] = id;
        while let Some(i) = stack.pop() {
            n += 1;
            let (x, y) = (i % w, i / w);
            let mut go = |j: usize| {
                if m[j] && label[j] == 0 {
                    label[j] = id;
                    stack.push(j);
                }
            };
            if x > 0 {
                go(i - 1);
            }
            if x + 1 < w {
                go(i + 1);
            }
            if y > 0 {
                go(i - w);
            }
            if y + 1 < h {
                go(i + w);
            }
        }
        sizes.push(n);
    }
    (label, sizes)
}

fn hull(mut pts: Vec<Vec2>) -> Vec<Vec2> {
    pts.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    pts.dedup();
    if pts.len() < 3 {
        return pts;
    }
    let cross = |o: Vec2, a: Vec2, b: Vec2| (a - o).perp_dot(b - o);
    let mut lower: Vec<Vec2> = Vec::new();
    for &p in &pts {
        while lower.len() >= 2 && cross(lower[lower.len() - 2], lower[lower.len() - 1], p) <= 0.0 {
            lower.pop();
        }
        lower.push(p);
    }
    let mut upper: Vec<Vec2> = Vec::new();
    for &p in pts.iter().rev() {
        while upper.len() >= 2 && cross(upper[upper.len() - 2], upper[upper.len() - 1], p) <= 0.0 {
            upper.pop();
        }
        upper.push(p);
    }
    lower.pop();
    upper.pop();
    lower.extend(upper);
    lower
}

/// The largest four-cornered shape with its corners on the convex outline.
fn largest_quad(h: &[Vec2]) -> Option<[Vec2; 4]> {
    let n = h.len();
    if n < 4 {
        return None;
    }
    let tri = |a: Vec2, b: Vec2, c: Vec2| (b - a).perp_dot(c - a).abs();
    let mut best = (0.0, None);
    for i in 0..n {
        for k in i + 2..n {
            let j = (i + 1..k).max_by(|&a, &b| tri(h[i], h[a], h[k]).total_cmp(&tri(h[i], h[b], h[k])));
            let l = (k + 1..n).chain(0..i).max_by(|&a, &b| tri(h[k], h[a], h[i]).total_cmp(&tri(h[k], h[b], h[i])));
            if let (Some(j), Some(l)) = (j, l) {
                let a = tri(h[i], h[j], h[k]) + tri(h[k], h[l], h[i]);
                if a > best.0 {
                    best = (a, Some([h[i], h[j], h[k], h[l]]));
                }
            }
        }
    }
    best.1
}

/// The four corners in order: top left first (the smallest x + y), then
/// clockwise as the photo shows them.
fn corners_in_order(q: [Vec2; 4]) -> [[f32; 2]; 4] {
    let c = (q[0] + q[1] + q[2] + q[3]) / 4.0;
    let mut v = q.to_vec();
    v.sort_by(|a, b| (a.y - c.y).atan2(a.x - c.x).total_cmp(&(b.y - c.y).atan2(b.x - c.x)));
    let first = (0..4).min_by(|&a, &b| (v[a].x + v[a].y).total_cmp(&(v[b].x + v[b].y))).unwrap_or(0);
    std::array::from_fn(|i| v[(first + i) % 4].to_array())
}

/// The object on the photo's background (a product on a table, a box
/// face on a wooden board): the colors along the photo's border make the
/// background, the largest region unlike them is the object. None when
/// there is no clear one (it touches most of the border, or is tiny).
pub fn find_object(img: &RgbaImage) -> Option<Found> {
    let (fw, fh) = img.dimensions();
    let k = (fw.max(fh) as f32 / 384.0).max(1.0);
    let (w, h) = (((fw as f32 / k).round() as u32).max(8), ((fh as f32 / k).round() as u32).max(8));
    let small = image::imageops::resize(img, w, h, image::imageops::FilterType::Triangle);
    let (w, h) = (w as usize, h as usize);
    let lab: Vec<[f32; 3]> = small.pixels().map(|p| lab8([p[0], p[1], p[2]])).collect();
    // The background: the colors of a band along the border, in 6 groups.
    let b = ((w.min(h) as f32 * 0.03) as usize).max(2);
    let ring: Vec<[f32; 3]> = (0..w * h).filter(|&i| i % w < b || i % w >= w - b || i / w < b || i / w >= h - b).map(|i| lab[i]).collect();
    let mut centers: Vec<[f32; 3]> = (0..6).map(|i| ring[i * ring.len() / 6]).collect();
    for _ in 0..10 {
        let mut sum = vec![([0.0f32; 3], 0usize); centers.len()];
        for c in &ring {
            let j = (0..centers.len()).min_by(|&a, &b| dist(*c, centers[a]).total_cmp(&dist(*c, centers[b]))).unwrap_or(0);
            for d in 0..3 {
                sum[j].0[d] += c[d];
            }
            sum[j].1 += 1;
        }
        for (j, (s, n)) in sum.into_iter().enumerate() {
            if n > 0 {
                centers[j] = s.map(|x| x / n as f32);
            }
        }
    }
    let near = |c: [f32; 3]| centers.iter().map(|m| dist(c, *m)).fold(f32::MAX, f32::min);
    let mut dr: Vec<f32> = ring.iter().map(|c| near(*c)).collect();
    dr.sort_by(f32::total_cmp);
    let thr = dr[dr.len() * 95 / 100] * 1.6 + 4.0;
    let mut m: Vec<bool> = lab.iter().map(|c| near(*c) > thr).collect();
    // Specks out, then the largest region, its gaps and holes closed.
    for on in [false, false, true, true] {
        m = morph(&m, w, h, on);
    }
    let (label, sizes) = parts(&m, w, h);
    let big = (1..sizes.len()).max_by_key(|&i| sizes[i])? as u32;
    let mut m: Vec<bool> = label.iter().map(|&l| l == big).collect();
    for on in [true, true, true, false, false, false] {
        m = morph(&m, w, h, on);
    }
    let (label, sizes) = parts(&m.iter().map(|&x| !x).collect::<Vec<_>>(), w, h);
    let mut outside = vec![false; sizes.len()];
    for i in (0..w * h).filter(|&i| i % w == 0 || i % w == w - 1 || i / w == 0 || i / w == h - 1) {
        outside[label[i] as usize] = true;
    }
    for i in 0..w * h {
        if !m[i] && !outside[label[i] as usize] {
            m[i] = true;
        }
    }
    let count = m.iter().filter(|&&x| x).count();
    let share = count as f32 / (w * h) as f32;
    // On the border on more than a third of it: not an object on a background.
    let edge = (0..w * h).filter(|&i| (i % w == 0 || i % w == w - 1 || i / w == 0 || i / w == h - 1) && m[i]).count();
    if !(0.03..0.97).contains(&share) || edge as f32 > 0.33 * (2 * (w + h)) as f32 {
        return None;
    }
    let (q, area) = quad_of(&m, w, h)?;
    let fill = count as f32 / area.max(1.0);
    let sx = fw as f32 / w as f32;
    let sy = fh as f32 / h as f32;
    let quad = corners_in_order(q.map(|p| Vec2::new(p.x * sx, p.y * sy)));
    // Labels: light, nearly colorless areas inside the object (their
    // printed text closed over), each filling its four corners.
    let mut light: Vec<bool> = (0..w * h).map(|i| m[i] && lab[i][0] > 62.0 && (lab[i][1] * lab[i][1] + lab[i][2] * lab[i][2]).sqrt() < 12.0).collect();
    for on in [true, true, false, false] {
        light = morph(&light, w, h, on);
    }
    for i in 0..w * h {
        light[i] &= m[i];
    }
    let (ids, sizes) = parts(&light, w, h);
    let mut big: Vec<usize> = (1..sizes.len()).filter(|&i| sizes[i] as f32 > 0.03 * count as f32 && (sizes[i] as f32) < 0.9 * count as f32).collect();
    big.sort_by_key(|&i| std::cmp::Reverse(sizes[i]));
    let mut labels = Vec::new();
    for i in big.into_iter().take(3) {
        let part: Vec<bool> = ids.iter().map(|&l| l as usize == i).collect();
        // On the object, not at its outline (a shadow on the ground is).
        let mut ring = part.clone();
        for _ in 0..3 {
            ring = morph(&ring, w, h, true);
        }
        let (n, out) = (0..w * h).filter(|&k| ring[k] && !part[k]).fold((0usize, 0usize), |(n, o), k| (n + 1, o + usize::from(!m[k])));
        if out as f32 > 0.08 * n as f32 {
            continue;
        }
        if let Some((q, a)) = quad_of(&part, w, h) {
            if sizes[i] as f32 / a.max(1.0) >= 0.75 {
                labels.push(corners_in_order(q.map(|p| Vec2::new(p.x * sx, p.y * sy))));
            }
        }
    }
    Some(Found { quad, share, fill, labels })
}

/// The largest four-cornered shape inside a mask's outline, and its area.
fn quad_of(m: &[bool], w: usize, h: usize) -> Option<([Vec2; 4], f32)> {
    let mut pts = Vec::new();
    for y in 0..h {
        let row = &m[y * w..(y + 1) * w];
        if let (Some(a), Some(z)) = (row.iter().position(|&x| x), row.iter().rposition(|&x| x)) {
            for yy in [y, y + 1] {
                pts.push(Vec2::new(a as f32, yy as f32));
                pts.push(Vec2::new(z as f32 + 1.0, yy as f32));
            }
        }
    }
    let q = largest_quad(&hull(pts))?;
    let area = 0.5 * ((q[1] - q[0]).perp_dot(q[2] - q[0]).abs() + (q[2] - q[0]).perp_dot(q[3] - q[0]).abs());
    Some((q, area))
}

fn linear(c: u8) -> f32 {
    let c = c as f32 / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// The photo's paper: its most common light, nearly neutral color (the
/// board of a box, a label's paper), as linear RGB and as Lab. Under a
/// colored light (the blue of shade, the yellow of a lamp) it shows the
/// light's cast. None when the photo has no such color.
pub fn paper_of(img: &RgbaImage) -> Option<([f32; 3], [f32; 3])> {
    let small = image::imageops::thumbnail(img, 256, 256);
    let mut bins: std::collections::HashMap<[i32; 3], (usize, [f32; 3], [f32; 3])> = std::collections::HashMap::new();
    let mut n = 0;
    for p in small.pixels().filter(|p| p[3] > 200 && p[0] < 250 && p[1] < 250 && p[2] < 250) {
        let l = lab8([p[0], p[1], p[2]]);
        if l[0] < 55.0 || (l[1] * l[1] + l[2] * l[2]).sqrt() > 35.0 {
            continue;
        }
        n += 1;
        let e = bins.entry([(l[0] / 8.0) as i32, (l[1] / 8.0).floor() as i32, (l[2] / 8.0).floor() as i32]).or_insert((0, [0.0; 3], [0.0; 3]));
        e.0 += 1;
        for k in 0..3 {
            e.1[k] += linear(p[k]);
            e.2[k] += l[k];
        }
    }
    let (count, lin, l) = bins.into_values().max_by_key(|b| b.0)?;
    (count >= 50 && count * 20 >= n).then(|| (lin.map(|x| x / count as f32), l.map(|x| x / count as f32)))
}

/// The per channel gains (linear) that make a color neutral, its
/// lightness kept.
pub fn neutral_gains(lin: [f32; 3]) -> [f32; 3] {
    let y = 0.2126 * lin[0] + 0.7152 * lin[1] + 0.0722 * lin[2];
    lin.map(|m| (y / m.max(1e-4)).clamp(0.6, 1.6))
}

/// The image with its channels scaled (linear light), clipped at white.
pub fn balanced(img: &RgbaImage, gains: [f32; 3]) -> RgbaImage {
    let lut: [[u8; 256]; 3] = std::array::from_fn(|k| {
        std::array::from_fn(|c| {
            let v = c as f32 / 255.0;
            let l = if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) } * gains[k];
            let l = l.clamp(0.0, 1.0);
            let s = if l <= 0.0031308 { l * 12.92 } else { 1.055 * l.powf(1.0 / 2.4) - 0.055 };
            (s * 255.0).round() as u8
        })
    });
    let mut out = img.clone();
    for p in out.pixels_mut() {
        for k in 0..3 {
            p[k] = lut[k][p[k] as usize];
        }
    }
    out
}

/// The image turned clockwise by `deg` (0, 90, 180 or 270).
pub fn turned(img: &RgbaImage, deg: u32) -> RgbaImage {
    match deg % 360 {
        90 => image::imageops::rotate90(img),
        180 => image::imageops::rotate180(img),
        270 => image::imageops::rotate270(img),
        _ => img.clone(),
    }
}

/// A side of a product (a face of its bounding box): the way it faces,
/// its upright frame, its bounds on (right, up) and the plane it lies in.
#[derive(Clone, Copy)]
pub struct Side {
    pub out: Vec3,
    pub right: Vec3,
    pub up: Vec3,
    pub lo: Vec2,
    pub hi: Vec2,
    pub plane: f32,
}

impl Side {
    /// Width / height as a print sees it upright.
    pub fn aspect(&self) -> f32 {
        let s = self.hi - self.lo;
        s.x / s.y.max(1e-6)
    }

    /// A point of the side as (u, v) of its print (0..1, v down).
    fn uv(&self, p: Vec3) -> Vec2 {
        let s = (self.hi - self.lo).max(Vec2::splat(1e-6));
        Vec2::new((p.dot(self.right) - self.lo.x) / s.x, (self.hi.y - p.dot(self.up)) / s.y)
    }
}

/// The six sides of the box `lo`..`hi` along `axes` (x, y, z): +x, -x,
/// +y, -y, +z, -z. Walls stand upright (up is +y); the top and bottom are
/// seen with -z up, as the SURFACES image shows them.
pub fn sides(axes: [Vec3; 3], lo: Vec3, hi: Vec3) -> [Side; 6] {
    std::array::from_fn(|i| {
        let a = i / 2;
        let sign = if i % 2 == 0 { 1.0 } else { -1.0 };
        let out = axes[a] * sign;
        let up = if a == 1 { -axes[2] } else { axes[1] };
        let right = up.cross(out);
        let corner = |k: usize| Vec3::new(if k & 1 == 0 { lo.x } else { hi.x }, if k & 2 == 0 { lo.y } else { hi.y }, if k & 4 == 0 { lo.z } else { hi.z });
        let (mut l, mut h) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
        let mut plane = f32::MIN;
        for k in 0..8 {
            let c = corner(k);
            let p = axes[0] * c.x + axes[1] * c.y + axes[2] * c.z;
            let q = Vec2::new(p.dot(right), p.dot(up));
            l = l.min(q);
            h = h.max(q);
            plane = plane.max(p.dot(out));
        }
        Side { out, right, up, lo: l, hi: h, plane }
    })
}

/// A color of a print turned clockwise by `rot`, at (u, v) of the turned
/// print, averaged over a small patch.
fn color_at(img: &RgbaImage, rot: u32, uv: Vec2) -> [f32; 3] {
    let (w, h) = img.dimensions();
    let mut sum = [0.0f32; 3];
    let r = 0.012;
    let mut n = 0.0;
    for dy in [-r, 0.0, r] {
        for dx in [-r, 0.0, r] {
            let (u, v) = ((uv.x + dx).clamp(0.0, 1.0), (uv.y + dy).clamp(0.0, 1.0));
            let (su, sv) = match rot % 360 {
                90 => (v, 1.0 - u),
                180 => (1.0 - u, 1.0 - v),
                270 => (1.0 - v, u),
                _ => (u, v),
            };
            let px = img.get_pixel(((su * w as f32) as u32).min(w - 1), ((sv * h as f32) as u32).min(h - 1));
            let l = lab8([px[0], px[1], px[2]]);
            for k in 0..3 {
                sum[k] += l[k];
            }
            n += 1.0;
        }
    }
    sum.map(|s| s / n)
}

/// How badly two prints meet across the edge between their sides: along
/// the edge, just inside each, how differently their colors change (a
/// stripe or a logo running over the fold), plus a quarter of how far
/// their mean colors are (faces of a box may differ in color). None when
/// the sides do not share an edge.
fn seam(a: &Side, ia: &RgbaImage, ra: u32, b: &Side, ib: &RgbaImage, rb: u32) -> Option<f32> {
    if a.out.dot(b.out).abs() > 0.1 {
        return None;
    }
    let e = a.out.cross(b.out).normalize_or_zero();
    // The edge: on both planes, along e over the extent both sides span.
    let span = |s: &Side| {
        let (r, u) = (e.dot(s.right), e.dot(s.up));
        if r.abs() > u.abs() {
            let (x0, x1) = (s.lo.x * r.signum(), s.hi.x * r.signum());
            (x0.min(x1), x0.max(x1))
        } else {
            let (y0, y1) = (s.lo.y * u.signum(), s.hi.y * u.signum());
            (y0.min(y1), y0.max(y1))
        }
    };
    let (t0, t1) = span(a);
    let base = a.out * a.plane + b.out * b.plane;
    // How far inside each print: 4% of the other side's depth there.
    let depth = |s: &Side, dir: Vec3| {
        let (r, u) = (dir.dot(s.right).abs(), dir.dot(s.up).abs());
        r * (s.hi.x - s.lo.x) + u * (s.hi.y - s.lo.y)
    };
    let (da, db) = (depth(a, b.out) * 0.04, depth(b, a.out) * 0.04);
    let n = 24;
    let (mut sa, mut sb) = (Vec::with_capacity(n), Vec::with_capacity(n));
    for i in 0..n {
        let t = t0 + (t1 - t0) * (0.08 + 0.84 * i as f32 / (n - 1) as f32);
        let p = base + e * t;
        sa.push(color_at(ia, ra, a.uv(p - b.out * da)));
        sb.push(color_at(ib, rb, b.uv(p - a.out * db)));
    }
    let mean = |s: &[[f32; 3]]| -> [f32; 3] { std::array::from_fn(|k| s.iter().map(|c| c[k]).sum::<f32>() / s.len() as f32) };
    let (ma, mb) = (mean(&sa), mean(&sb));
    let change: f32 = sa.iter().zip(&sb).map(|(x, y)| dist(std::array::from_fn(|k| x[k] - ma[k]), std::array::from_fn(|k| y[k] - mb[k]))).sum::<f32>() / n as f32;
    Some(change + 0.25 * dist(ma, mb))
}

/// A print to put on a side: its image (small is enough), its
/// proportions, a turn it must take, and whether it may move.
pub struct SidePrint {
    pub side: usize,
    pub img: RgbaImage,
    /// Width / height of the full print.
    pub aspect: f32,
    /// A turn forced (by Claude), else the turns its proportions allow.
    pub turn: Option<u32>,
    /// The turn that puts its top where Claude says (its side's own up for
    /// a wall), when the proportions allow it.
    pub up: Option<u32>,
    /// It may move to another side its proportions fit better.
    pub free: bool,
}

/// The turns (clockwise) that fit a print of `aspect` (width / height) on
/// a side of `side_aspect`: the closest proportions, both ways up.
pub fn fitting_turns(aspect: f32, side_aspect: f32) -> Vec<u32> {
    let off = |a: f32| (a / side_aspect.max(1e-6)).ln().abs();
    let (keep, turn) = (off(aspect), off(1.0 / aspect.max(1e-6)));
    if (keep - turn).abs() < 0.12 {
        vec![0, 180, 90, 270]
    } else if keep < turn {
        vec![0, 180]
    } else {
        vec![90, 270]
    }
}

/// How far a print's proportions are from a side's, at its best turn.
fn misfit(aspect: f32, side_aspect: f32) -> f32 {
    let off = |a: f32| (a / side_aspect.max(1e-6)).ln().abs();
    off(aspect).min(off(1.0 / aspect.max(1e-6)))
}

/// Every order of `a` (Heap's algorithm).
fn orders(k: usize, a: &mut Vec<usize>, out: &mut Vec<Vec<usize>>) {
    if k <= 1 {
        out.push(a.clone());
        return;
    }
    for i in 0..k {
        orders(k - 1, a, out);
        let j = if k % 2 == 0 { i } else { 0 };
        a.swap(j, k - 1);
    }
}

/// For each print, the image it gets (its index in `prints`) and its turn:
/// the free prints go to the sides their proportions fit (the sides given
/// are kept when they fit), turned, or traded between sides alike, so the
/// prints meet best across the edges they share. Also the mismatch left
/// (mean Lab distance along the shared edges, None when no two prints
/// share one).
pub fn solve_sides(all: &[Side; 6], prints: &[SidePrint]) -> (Vec<(usize, u32)>, Option<f32>) {
    let n = prints.len();
    let free: Vec<usize> = (0..n).filter(|&i| prints[i].free).collect();
    let mut perms: Vec<Vec<usize>> = Vec::new();
    if free.len() <= 7 {
        orders(free.len(), &mut free.clone(), &mut perms);
    } else {
        perms.push(free.clone());
    }
    let img_of = |perm: &[usize]| {
        let mut v: Vec<usize> = (0..n).collect();
        for (slot, &img) in free.iter().zip(perm) {
            v[*slot] = img;
        }
        v
    };
    // Proportions first: the deals that fit them best (trades between
    // sides alike change nothing there).
    let fit = |v: &[usize]| (0..n).map(|i| misfit(prints[v[i]].aspect, all[prints[i].side].aspect())).sum::<f32>();
    let best_fit = perms.iter().map(|p| fit(&img_of(p))).fold(f32::MAX, f32::min);
    let tol = best_fit + 0.08;
    let deals: Vec<Vec<usize>> = perms.iter().map(|p| img_of(p)).filter(|v| fit(v) <= tol).collect();
    let mut cache: std::collections::HashMap<(usize, usize, u32, usize, usize, u32), Option<f32>> = std::collections::HashMap::new();
    let mut best: Option<(f32, Vec<(usize, u32)>, Option<f32>)> = None;
    for v in deals {
        let moved = (0..n).filter(|&i| v[i] != i).count();
        let options: Vec<Vec<u32>> = (0..n)
            .map(|i| {
                let (pr, side) = (&prints[v[i]], &all[prints[i].side]);
                let fits = fitting_turns(pr.aspect, side.aspect());
                match (pr.turn, pr.up) {
                    (Some(t), _) => vec![t],
                    (None, Some(u)) if prints[v[i]].side == prints[i].side && fits.contains(&u) => vec![u],
                    _ => fits,
                }
            })
            .collect();
        let mut pick = vec![0usize; n];
        loop {
            let choice: Vec<(usize, u32)> = (0..n).map(|i| (v[i], options[i][pick[i]])).collect();
            let (mut cost, mut edges) = (0.0, 0);
            for i in 0..n {
                for j in i + 1..n {
                    let key = (prints[i].side, choice[i].0, choice[i].1, prints[j].side, choice[j].0, choice[j].1);
                    let c = *cache.entry(key).or_insert_with(|| seam(&all[prints[i].side], &prints[choice[i].0].img, choice[i].1, &all[prints[j].side], &prints[choice[j].0].img, choice[j].1));
                    if let Some(c) = c {
                        cost += c;
                        edges += 1;
                    }
                }
            }
            let mean = (edges > 0).then(|| cost / edges as f32);
            // The sides given win ties: each move costs a little.
            let score = mean.unwrap_or(0.0) + 1.5 * moved as f32;
            if best.as_ref().is_none_or(|b| score < b.0) {
                best = Some((score, choice, mean));
            }
            let mut k = 0;
            while k < n {
                pick[k] += 1;
                if pick[k] < options[k].len() {
                    break;
                }
                pick[k] = 0;
                k += 1;
            }
            if k == n {
                break;
            }
        }
    }
    if std::env::var("MF_MUSE_DUMP").is_ok() {
        if let Some((s, choice, _)) = &best {
            eprintln!("sides: score {s:.1}, choice {choice:?}");
            for (i, sd) in all.iter().enumerate() {
                eprintln!("  side {i}: out {:?} right {:?} up {:?} lo {:?} hi {:?} plane {}", sd.out, sd.right, sd.up, sd.lo, sd.hi, sd.plane);
            }
            for i in 0..n {
                for j in i + 1..n {
                    if let Some(c) = seam(&all[prints[i].side], &prints[choice[i].0].img, choice[i].1, &all[prints[j].side], &prints[choice[j].0].img, choice[j].1) {
                        eprintln!("  edge side {} / side {}: {c:.1}", prints[i].side, prints[j].side);
                    }
                }
            }
        }
    }
    match best {
        Some((_, choice, mean)) => (choice, mean),
        None => ((0..n).map(|i| (i, prints[i].turn.unwrap_or(0))).collect(), None),
    }
}
