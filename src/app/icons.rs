//! Small vector icons drawn with the egui painter (own artwork) and the icon
//! button used by the tool strip, panel headers and toolbars.

use eframe::egui::{self, Color32, Pos2, Rect, Response, Sense, Stroke, Ui, Vec2};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Icon {
    Brush,
    Eraser,
    PolyFill,
    Smudge,
    Clone,
    Picker,
    Hide,
    Render,
    /// Packshot studio: a softbox on its stand.
    Studio,
    Eye,
    EyeOff,
    Folder,
    Fill,
    Mask,
    Effect,
    Trash,
    Gear,
    History,
    Sun,
    Sphere,
    Search,
    Grid,
    List,
    Close,
    Camera,
    Arrow,
    Plus,
    Chevron,
    ChevronUp,
    Pen,
    Menu,
    Alpha,
    Material,
    FillTab,
    Symmetry,
    Lazy,
    Stencil,
    /// Polygon fill: selection types and fill modes.
    SelMarquee,
    SelLasso,
    SelLoop,
    SelPartialLoop,
    SelTriangle,
    SelPolygon,
    SelMesh,
    SelIsland,
    /// Selection tools and modes.
    SelFaces,
    SelBrush,
    Gradient,
    Wand,
    Bucket,
    SelEllipse,
    SelPolyLasso,
    SelNew,
    SelAdd,
    SelSub,
    SelInter,
    /// Effect types (layer stack rows).
    Generator,
    Levels,
    /// Path tool: a curve through square nodes.
    Path,
    /// Swap the split views: two opposed arrows.
    Swap,
    /// Panels locked / free: a padlock, closed or open.
    Lock,
    Unlock,
}

fn s(c: Color32) -> Stroke {
    Stroke::new(1.4f32, c)
}

/// Draws `icon` centered in `r`.
pub fn paint(p: &egui::Painter, r: Rect, icon: Icon, c: Color32) {
    let k = r.width().min(r.height()) / 20.0;
    let o = r.center();
    let at = |x: f32, y: f32| Pos2::new(o.x + x * k, o.y + y * k);
    let line = |a: (f32, f32), b: (f32, f32)| p.line_segment([at(a.0, a.1), at(b.0, b.1)], s(c));
    let poly = |pts: &[(f32, f32)], closed: bool| {
        let v: Vec<Pos2> = pts.iter().map(|q| at(q.0, q.1)).collect();
        if closed {
            p.add(egui::Shape::closed_line(v, s(c)));
        } else {
            p.add(egui::Shape::line(v, s(c)));
        }
    };
    let circle = |x: f32, y: f32, rad: f32| p.circle_stroke(at(x, y), rad * k, s(c));
    let dot = |x: f32, y: f32, rad: f32| p.circle_filled(at(x, y), rad * k, c);
    match icon {
        Icon::Swap => {
            line((-7.0, -3.0), (7.0, -3.0));
            line((7.0, -3.0), (4.0, -6.0));
            line((7.0, -3.0), (4.0, 0.0));
            line((7.0, 3.0), (-7.0, 3.0));
            line((-7.0, 3.0), (-4.0, 0.0));
            line((-7.0, 3.0), (-4.0, 6.0));
        }
        Icon::Lock | Icon::Unlock => {
            p.rect_stroke(Rect::from_min_max(at(-6.0, -1.0), at(6.0, 7.0)), 2.0 * k, s(c));
            p.circle_filled(at(0.0, 3.0), 1.3 * k, c);
            // The shackle: raised and out of the body on the right when open.
            let lift = if icon == Icon::Unlock { 3.0 } else { 0.0 };
            let pts: Vec<Pos2> = (0..=12)
                .map(|i| {
                    let a = std::f32::consts::PI * (1.0 + i as f32 / 12.0);
                    at(4.0 * a.cos(), -5.0 - lift + 4.0 * a.sin())
                })
                .collect();
            p.add(egui::Shape::line(pts, s(c)));
            line((-4.0, -5.0 - lift), (-4.0, -1.0));
            line((4.0, -5.0 - lift), (4.0, -1.0 - lift * 1.4));
        }
        Icon::Path => {
            let pts: Vec<Pos2> = (0..=16)
                .map(|i| {
                    let t = i as f32 / 16.0;
                    at(-7.0 + 14.0 * t, 5.0 * (t * std::f32::consts::PI).cos())
                })
                .collect();
            p.add(egui::Shape::line(pts, s(c)));
            for (x, y) in [(-7.0, 5.0), (0.0, 0.0), (7.0, -5.0)] {
                p.rect_filled(Rect::from_center_size(at(x, y), egui::vec2(3.6 * k, 3.6 * k)), 0.0, c);
            }
        }
        Icon::Generator => {
            // A disc with a white core, like Painter's generator glyph.
            p.circle_filled(o, 7.0 * k, c);
            p.circle_filled(o, 3.2 * k, Color32::from_gray(30));
        }
        Icon::Levels => {
            // Histogram bars.
            for (i, h) in [3.0f32, 6.0, 10.0, 7.0, 4.0].iter().enumerate() {
                let x = -6.0 + i as f32 * 3.0;
                p.rect_filled(Rect::from_min_max(at(x - 1.1, 7.0 - h), at(x + 1.1, 7.0)), 0.0, c);
            }
        }
        Icon::SelMarquee => {
            // Dashed rounded rectangle.
            let r = Rect::from_center_size(o, egui::vec2(15.0 * k, 13.0 * k));
            let st = Stroke::new(1.6f32, c);
            let pts = [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()];
            for w in pts.windows(2) {
                p.extend(egui::Shape::dashed_line(&[w[0], w[1]], st, 3.0 * k, 2.2 * k));
            }
        }
        Icon::SelFaces => {
            // A cube whose top face is selected (dashed, tinted).
            poly(&[(-7.0, -2.5), (-7.0, 4.5), (0.0, 8.0), (7.0, 4.5), (7.0, -2.5)], false);
            line((0.0, 1.0), (0.0, 8.0));
            let top: Vec<Pos2> = [(-7.0, -2.5), (0.0, -6.0), (7.0, -2.5), (0.0, 1.0)].iter().map(|q| at(q.0, q.1)).collect();
            p.add(egui::Shape::convex_polygon(top.clone(), Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), 90), Stroke::NONE));
            let mut ring = top.clone();
            ring.push(top[0]);
            p.extend(egui::Shape::dashed_line(&ring, Stroke::new(1.3f32, c), 2.4 * k, 1.6 * k));
        }
        Icon::SelBrush => {
            // A brush tip over a dashed selection outline.
            let st = Stroke::new(1.3f32, c);
            let pts: Vec<Pos2> = (0..=20)
                .map(|i| {
                    let a = i as f32 / 20.0 * std::f32::consts::TAU;
                    at(-2.0 + a.cos() * 6.5, 2.0 + a.sin() * 6.0)
                })
                .collect();
            for (i, w) in pts.windows(2).enumerate() {
                if i % 2 == 0 {
                    p.line_segment([w[0], w[1]], st);
                }
            }
            line((1.0, -1.0), (7.5, -7.5));
            p.circle_filled(at(1.0, -1.0), 2.2 * k, c);
        }
        Icon::Gradient => {
            // A box shading from full to empty.
            let r = Rect::from_center_size(o, egui::vec2(16.0 * k, 12.0 * k));
            for i in 0..8 {
                let x0 = r.min.x + r.width() * i as f32 / 8.0;
                let a = (230.0 * (1.0 - i as f32 / 8.0)) as u8;
                p.rect_filled(Rect::from_min_max(Pos2::new(x0, r.min.y), Pos2::new(x0 + r.width() / 8.0 + 0.5, r.max.y)), 0.0, Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), a));
            }
            p.rect_stroke(r, 1.0, s(c));
        }
        Icon::Wand => {
            line((-7.0, 7.0), (3.0, -3.0));
            p.line_segment([at(-7.0, 7.0), at(-3.0, 3.0)], Stroke::new(2.6f32, c));
            for (a, b) in [((5.0, -9.0), (5.0, -5.5)), ((5.0, -2.5), (5.0, 1.0)), ((1.5, -5.0), (-2.0, -5.0)), ((8.5, -5.0), (12.0, -5.0))] {
                line(a, b);
            }
            dot(5.0, -5.0, 0.9);
        }
        Icon::Bucket => {
            poly(&[(-8.0, -1.0), (-1.0, -8.0), (6.0, -1.0), (-1.0, 6.0)], true);
            line((-1.0, -8.0), (-1.0, -3.0));
            poly(&[(6.0, -1.0), (8.0, 3.0), (8.5, 6.0), (7.0, 7.5), (5.5, 6.0), (6.0, 3.0)], false);
            dot(7.0, 6.0, 1.3);
        }
        Icon::SelEllipse => {
            let st = Stroke::new(1.6f32, c);
            let pts: Vec<Pos2> = (0..=24)
                .map(|i| {
                    let a = i as f32 / 24.0 * std::f32::consts::TAU;
                    at(a.cos() * 8.0, a.sin() * 6.0)
                })
                .collect();
            for (i, w) in pts.windows(2).enumerate() {
                if i % 2 == 0 {
                    p.line_segment([w[0], w[1]], st);
                }
            }
        }
        Icon::SelPolyLasso => {
            let pts = [(-7.0, -4.0), (1.0, -6.5), (7.0, -1.5), (2.5, 2.0), (-1.0, 6.0), (-7.0, 2.5)];
            let st = Stroke::new(1.4f32, c);
            for i in 0..pts.len() {
                let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
                p.extend(egui::Shape::dashed_line(&[at(a.0, a.1), at(b.0, b.1)], st, 2.6 * k, 1.8 * k));
                dot(a.0, a.1, 1.2);
            }
        }
        Icon::SelNew | Icon::SelAdd | Icon::SelSub | Icon::SelInter => {
            // Two squares: what the mode keeps is filled.
            let a = Rect::from_min_max(at(-7.5, -6.5), at(2.5, 3.5));
            let b = Rect::from_min_max(at(-2.5, -1.5), at(7.5, 8.5));
            let fill = Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), 150);
            match icon {
                Icon::SelNew => {
                    let r = Rect::from_center_size(o, egui::vec2(12.0 * k, 12.0 * k));
                    p.rect_filled(r, 1.0, fill);
                    p.rect_stroke(r, 1.0, s(c));
                }
                Icon::SelAdd => {
                    p.rect_filled(a, 0.0, fill);
                    p.rect_filled(b, 0.0, fill);
                    p.rect_stroke(a, 0.0, s(c));
                    p.rect_stroke(b, 0.0, s(c));
                }
                Icon::SelSub => {
                    p.rect_filled(a, 0.0, fill);
                    p.rect_filled(a.intersect(b), 0.0, crate::style::HEADER);
                    p.rect_stroke(a, 0.0, s(c));
                    p.rect_stroke(b, 0.0, s(c));
                }
                _ => {
                    p.rect_filled(a.intersect(b), 0.0, fill);
                    p.rect_stroke(a, 0.0, s(c));
                    p.rect_stroke(b, 0.0, s(c));
                }
            }
        }
        Icon::SelLasso => {
            // A dashed free-form loop with its tail.
            let pts: Vec<(f32, f32)> = (0..=18)
                .map(|i| {
                    let a = i as f32 / 18.0 * std::f32::consts::TAU;
                    (a.cos() * 7.0 + (a * 2.0).sin() * 0.8, a.sin() * 4.6 - 2.0)
                })
                .collect();
            for (i, w) in pts.windows(2).enumerate() {
                if i % 2 == 0 {
                    line(w[0], w[1]);
                }
            }
            poly(&[(-3.5, 1.8), (-4.5, 5.0), (-2.5, 7.5)], false);
        }
        Icon::SelLoop | Icon::SelPartialLoop => {
            // 3 x 3 grid; the loop (middle row) or part of it filled.
            let cell = 4.6;
            let filled = |i: i32, j: i32| match icon {
                Icon::SelLoop => j == 0,
                _ => (j == 0 && i >= 0) || (i == 1 && j == 1),
            };
            for j in -1..=1 {
                for i in -1..=1 {
                    let (x, y) = (i as f32 * cell, j as f32 * cell);
                    let r = Rect::from_center_size(at(x, y), egui::vec2(cell * k * 0.82, cell * k * 0.82));
                    if filled(i, j) {
                        p.rect_filled(r, 0.5, c);
                    } else {
                        p.rect_stroke(r, 0.5, Stroke::new(1.0f32, c.gamma_multiply(0.6)));
                    }
                }
            }
        }
        Icon::SelTriangle => {
            p.add(egui::Shape::convex_polygon(vec![at(0.0, -6.5), at(7.0, 6.0), at(-7.0, 6.0)], c, Stroke::NONE));
        }
        Icon::SelPolygon => {
            p.rect_filled(Rect::from_center_size(o, egui::vec2(12.5 * k, 12.5 * k)), 0.5, c);
        }
        Icon::SelMesh => {
            // Solid cube, its three faces split by dark edges.
            let top = [at(0.0, -7.5), at(6.8, -3.8), at(0.0, -0.2), at(-6.8, -3.8)];
            let left = [at(-6.8, -3.8), at(0.0, -0.2), at(0.0, 7.8), at(-6.8, 4.0)];
            let right = [at(0.0, -0.2), at(6.8, -3.8), at(6.8, 4.0), at(0.0, 7.8)];
            for f in [top, left, right] {
                p.add(egui::Shape::convex_polygon(f.to_vec(), c, Stroke::NONE));
            }
            let dark = Stroke::new(1.3f32, Color32::from_gray(40));
            p.line_segment([at(-6.8, -3.8), at(0.0, -0.2)], dark);
            p.line_segment([at(6.8, -3.8), at(0.0, -0.2)], dark);
            p.line_segment([at(0.0, -0.2), at(0.0, 7.8)], dark);
        }
        Icon::SelIsland => {
            // Scattered UV chunks.
            for (x, y) in [(-6.0, -6.0), (-1.5, -6.0), (3.0, -6.0), (-3.8, -1.5), (5.2, -1.5), (-6.0, 3.0), (0.7, 3.0), (5.2, 3.0), (-1.5, 7.0)] {
                p.rect_filled(Rect::from_center_size(at(x as f32 + 0.0, y as f32), egui::vec2(3.4 * k, 3.4 * k)), 0.3, c);
            }
        }
        Icon::Brush => {
            poly(&[(5.0, -7.0), (7.0, -5.0), (-1.0, 3.0), (-3.0, 1.0)], true);
            poly(&[(-3.0, 1.0), (-6.0, 3.0), (-7.0, 7.0), (-3.0, 6.0), (-1.0, 3.0)], false);
        }
        Icon::Eraser => {
            poly(&[(-7.0, 3.0), (1.0, -5.0), (7.0, 1.0), (1.0, 7.0), (-3.0, 7.0)], true);
            line((-3.0, -1.0), (3.0, 5.0));
            line((-3.0, 7.0), (7.0, 7.0));
        }
        Icon::PolyFill => {
            poly(&[(-6.0, -2.0), (0.0, -7.0), (6.0, -2.0), (0.0, 3.0)], true);
            poly(&[(-6.0, -2.0), (-6.0, 3.0), (0.0, 8.0), (0.0, 3.0)], false);
            line((6.0, -2.0), (6.0, 3.0));
            line((6.0, 3.0), (0.0, 8.0));
        }
        Icon::Smudge => {
            poly(&[(-2.0, 7.0), (-3.0, 0.0), (-1.0, -6.0), (1.0, -6.0), (2.0, -1.0), (4.0, -2.0), (5.0, 2.0), (3.0, 7.0)], false);
            line((-6.0, 4.0), (-4.0, 2.0));
        }
        Icon::Clone => {
            poly(&[(-5.0, 1.0), (5.0, 1.0), (5.0, 4.0), (-5.0, 4.0)], true);
            line((-7.0, 7.0), (7.0, 7.0));
            poly(&[(-2.0, 1.0), (-2.0, -3.0), (2.0, -3.0), (2.0, 1.0)], false);
            circle(0.0, -5.0, 2.5);
        }
        Icon::Picker => {
            poly(&[(3.0, -7.0), (7.0, -3.0), (5.0, -1.0), (1.0, -5.0)], true);
            poly(&[(2.0, -4.0), (-6.0, 4.0), (-7.0, 7.0), (-4.0, 6.0), (4.0, -2.0)], false);
        }
        Icon::Hide => {
            poly(&[(-6.0, -2.0), (0.0, -7.0), (6.0, -2.0), (0.0, 3.0)], true);
            line((-7.0, 7.0), (7.0, -7.0));
        }
        Icon::Render => {
            poly(&[(-7.0, -3.0), (4.0, -3.0), (4.0, 5.0), (-7.0, 5.0)], true);
            poly(&[(4.0, -1.0), (8.0, -4.0), (8.0, 6.0), (4.0, 3.0)], false);
            circle(-2.0, 1.0, 2.0);
        }
        Icon::Studio => {
            poly(&[(-7.0, -7.0), (3.0, -7.0), (3.0, 1.0), (-7.0, 1.0)], true);
            line((-2.0, 1.0), (-2.0, 7.0));
            line((-5.0, 7.0), (1.0, 7.0));
            line((5.0, -5.0), (8.0, -6.0));
            line((5.0, -3.0), (8.0, -3.0));
            line((5.0, -1.0), (8.0, 0.0));
        }
        Icon::Eye | Icon::EyeOff => {
            poly(&[(-8.0, 0.0), (-4.0, -4.0), (0.0, -5.0), (4.0, -4.0), (8.0, 0.0), (4.0, 4.0), (0.0, 5.0), (-4.0, 4.0)], true);
            if icon == Icon::Eye {
                dot(0.0, 0.0, 2.4);
            } else {
                line((-7.0, 6.0), (7.0, -6.0));
            }
        }
        Icon::Folder => {
            poly(&[(-8.0, -5.0), (-3.0, -5.0), (-1.0, -3.0), (8.0, -3.0), (8.0, 6.0), (-8.0, 6.0)], true);
        }
        Icon::Fill => {
            poly(&[(-6.0, -1.0), (0.0, -7.0), (6.0, -1.0), (0.0, 5.0)], true);
            poly(&[(6.0, -1.0), (8.0, 3.0), (7.0, 6.0), (6.0, 3.0)], false);
            line((-6.0, -1.0), (6.0, -1.0));
        }
        Icon::Mask => {
            poly(&[(-7.0, -6.0), (7.0, -6.0), (7.0, 6.0), (-7.0, 6.0)], true);
            circle(0.0, 0.0, 3.5);
        }
        Icon::Effect => {
            line((-6.0, 7.0), (-2.0, -7.0));
            line((-5.0, 0.0), (0.0, 0.0));
            line((-4.0, -7.0), (2.0, -7.0));
            line((1.0, -1.0), (7.0, 7.0));
            line((7.0, -1.0), (1.0, 7.0));
        }
        Icon::Trash => {
            poly(&[(-5.0, -4.0), (-4.0, 7.0), (4.0, 7.0), (5.0, -4.0)], false);
            line((-7.0, -4.0), (7.0, -4.0));
            poly(&[(-2.0, -4.0), (-2.0, -7.0), (2.0, -7.0), (2.0, -4.0)], false);
            line((0.0, -1.0), (0.0, 4.0));
        }
        Icon::Gear => {
            circle(0.0, 0.0, 3.0);
            for i in 0..8 {
                let a = i as f32 * std::f32::consts::FRAC_PI_4;
                line((a.cos() * 5.0, a.sin() * 5.0), (a.cos() * 7.5, a.sin() * 7.5));
            }
            circle(0.0, 0.0, 5.0);
        }
        Icon::History => {
            circle(0.0, 0.0, 7.0);
            line((0.0, 0.0), (0.0, -4.5));
            line((0.0, 0.0), (3.5, 2.0));
        }
        Icon::Sun => {
            circle(0.0, 0.0, 3.5);
            for i in 0..8 {
                let a = i as f32 * std::f32::consts::FRAC_PI_4;
                line((a.cos() * 5.5, a.sin() * 5.5), (a.cos() * 8.0, a.sin() * 8.0));
            }
        }
        Icon::Sphere => {
            circle(0.0, 0.0, 7.0);
            poly(&[(-7.0, 0.0), (-4.0, 2.0), (0.0, 2.7), (4.0, 2.0), (7.0, 0.0)], false);
            dot(-2.5, -3.0, 1.4);
        }
        Icon::Search => {
            circle(-1.5, -1.5, 5.0);
            line((2.0, 2.0), (7.0, 7.0));
        }
        Icon::Grid => {
            for (x, y) in [(-6.0, -6.0), (1.0, -6.0), (-6.0, 1.0), (1.0, 1.0)] {
                poly(&[(x, y), (x + 5.0, y), (x + 5.0, y + 5.0), (x, y + 5.0)], true);
            }
        }
        Icon::List => {
            for y in [-5.0, 0.0, 5.0] {
                dot(-6.0, y, 1.0);
                line((-3.0, y), (7.0, y));
            }
        }
        Icon::Close => {
            line((-5.0, -5.0), (5.0, 5.0));
            line((5.0, -5.0), (-5.0, 5.0));
        }
        Icon::Camera => {
            poly(&[(-8.0, -4.0), (-3.0, -4.0), (-1.0, -7.0), (3.0, -7.0), (5.0, -4.0), (8.0, -4.0), (8.0, 6.0), (-8.0, 6.0)], true);
            circle(0.0, 1.0, 3.0);
        }
        Icon::Plus => {
            line((-6.0, 0.0), (6.0, 0.0));
            line((0.0, -6.0), (0.0, 6.0));
        }
        Icon::Chevron => {
            poly(&[(-5.0, -2.5), (0.0, 2.5), (5.0, -2.5)], false);
        }
        Icon::ChevronUp => {
            poly(&[(-5.0, 2.5), (0.0, -2.5), (5.0, 2.5)], false);
        }
        Icon::Pen => {
            poly(&[(-6.0, 6.0), (-5.0, 2.0), (3.0, -6.0), (6.0, -3.0), (-2.0, 5.0)], true);
            line((-5.0, 2.0), (-2.0, 5.0));
        }
        Icon::Menu => {
            for y in [-5.0, -1.0, 3.0] {
                line((-7.0, y), (5.0, y));
            }
            poly(&[(3.0, 6.0), (5.0, 8.0), (7.0, 6.0)], false);
        }
        Icon::Alpha => {
            circle(0.0, 0.0, 7.0);
            for (x, y) in [(-3.0, -3.0), (1.0, -3.0), (-3.0, 1.0), (1.0, 1.0), (-1.0, -1.0), (3.0, -1.0), (-1.0, 3.0)] {
                dot(x + 1.0, y + 1.0, 1.1);
            }
        }
        Icon::Material => {
            circle(0.0, 0.0, 7.0);
            let pts: Vec<(f32, f32)> = (0..=12).map(|i| {
                let a = -1.2 + i as f32 * 0.2;
                (2.5 + a.cos() * 5.0, a.sin() * 6.5)
            }).collect();
            poly(&pts, false);
        }
        Icon::FillTab => {
            poly(&[(-7.0, -6.0), (7.0, -6.0), (7.0, 6.0), (-7.0, 6.0)], true);
            line((-7.0, -2.0), (7.0, -2.0));
            line((-4.0, 2.0), (1.0, 2.0));
            circle(4.0, 3.0, 1.8);
        }
        Icon::Symmetry => {
            line((0.0, -8.0), (0.0, 8.0));
            poly(&[(-2.0, -5.0), (-8.0, 5.0), (-2.0, 5.0)], true);
            poly(&[(2.0, -5.0), (8.0, 5.0), (2.0, 5.0)], true);
        }
        Icon::Lazy => {
            dot(-5.0, 5.0, 1.6);
            dot(-1.0, 1.0, 1.3);
            dot(3.0, -3.0, 1.0);
            circle(5.5, -5.5, 2.0);
        }
        Icon::Stencil => {
            poly(&[(-7.0, -7.0), (7.0, -7.0), (7.0, 7.0), (-7.0, 7.0)], true);
            circle(0.0, 0.0, 3.5);
        }
        Icon::Arrow => {
            poly(&[(-3.0, -5.0), (3.0, 0.0), (-3.0, 5.0)], false);
        }
    }
}

/// Square icon button. `on` draws the active state.
pub fn button(ui: &mut Ui, icon: Icon, on: bool, size: f32, tip: &str) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    super::demo::note_icon(tip, rect);
    let v = ui.visuals();
    let accent = v.selection.bg_fill;
    let bg = if on {
        Some(accent.linear_multiply(0.55))
    } else if resp.hovered() {
        Some(v.widgets.hovered.bg_fill)
    } else {
        None
    };
    if let Some(bg) = bg {
        ui.painter().rect_filled(rect.shrink(1.0), 3.0, bg);
    }
    let c = if on { Color32::WHITE } else if resp.hovered() { Color32::from_gray(230) } else { Color32::from_gray(175) };
    paint(ui.painter(), rect.shrink(size * 0.18), icon, c);
    if tip.is_empty() {
        resp
    } else {
        resp.on_hover_text(tip)
    }
}
