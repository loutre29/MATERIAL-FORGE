//! Small vector icons drawn with the egui painter (own artwork) and the icon
//! button used by the tool strip, panel headers and toolbars.

use eframe::egui::{self, Color32, Pos2, Rect, Response, Sense, Stroke, Ui, Vec2};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Icon {
    Brush,
    Eraser,
    Projection,
    PolyFill,
    Smudge,
    Clone,
    Picker,
    Hide,
    Bake,
    Export,
    Render,
    Eye,
    EyeOff,
    Folder,
    Fill,
    Layer,
    Mask,
    Effect,
    Trash,
    Gear,
    History,
    Log,
    Sun,
    Sphere,
    Search,
    Filter,
    Grid,
    List,
    Close,
    Camera,
    Split,
    Single,
    Plus,
    Import,
    Refresh,
    Arrow,
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
        Icon::Brush => {
            poly(&[(5.0, -7.0), (7.0, -5.0), (-1.0, 3.0), (-3.0, 1.0)], true);
            poly(&[(-3.0, 1.0), (-6.0, 3.0), (-7.0, 7.0), (-3.0, 6.0), (-1.0, 3.0)], false);
        }
        Icon::Eraser => {
            poly(&[(-7.0, 3.0), (1.0, -5.0), (7.0, 1.0), (1.0, 7.0), (-3.0, 7.0)], true);
            line((-3.0, -1.0), (3.0, 5.0));
            line((-3.0, 7.0), (7.0, 7.0));
        }
        Icon::Projection => {
            poly(&[(-7.0, -6.0), (7.0, -6.0), (7.0, 6.0), (-7.0, 6.0)], true);
            poly(&[(-5.0, 4.0), (-1.0, -1.0), (2.0, 2.0), (4.0, 0.0), (5.0, 4.0)], false);
            dot(3.0, -3.0, 1.2);
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
        Icon::Bake => {
            poly(&[(-6.0, -3.0), (0.0, -7.0), (6.0, -3.0), (6.0, 4.0), (0.0, 8.0), (-6.0, 4.0)], true);
            line((0.0, 1.0), (0.0, 8.0));
            poly(&[(-6.0, -3.0), (0.0, 1.0), (6.0, -3.0)], false);
        }
        Icon::Export => {
            poly(&[(-2.0, -6.0), (-7.0, -6.0), (-7.0, 7.0), (6.0, 7.0), (6.0, 2.0)], false);
            line((-1.0, 1.0), (7.0, -7.0));
            poly(&[(2.0, -7.0), (7.0, -7.0), (7.0, -2.0)], false);
        }
        Icon::Render => {
            poly(&[(-7.0, -3.0), (4.0, -3.0), (4.0, 5.0), (-7.0, 5.0)], true);
            poly(&[(4.0, -1.0), (8.0, -4.0), (8.0, 6.0), (4.0, 3.0)], false);
            circle(-2.0, 1.0, 2.0);
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
        Icon::Layer => {
            poly(&[(-7.0, -2.0), (0.0, -6.0), (7.0, -2.0), (0.0, 2.0)], true);
            poly(&[(-7.0, 2.0), (0.0, 6.0), (7.0, 2.0)], false);
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
        Icon::Log => {
            poly(&[(-6.0, -7.0), (6.0, -7.0), (6.0, 7.0), (-6.0, 7.0)], true);
            for y in [-3.0, 0.0, 3.0] {
                line((-3.5, y), (3.5, y));
            }
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
        Icon::Filter => {
            poly(&[(-7.0, -6.0), (7.0, -6.0), (1.5, 1.0), (1.5, 7.0), (-1.5, 5.0), (-1.5, 1.0)], true);
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
        Icon::Split => {
            poly(&[(-8.0, -6.0), (8.0, -6.0), (8.0, 6.0), (-8.0, 6.0)], true);
            line((0.0, -6.0), (0.0, 6.0));
        }
        Icon::Single => {
            poly(&[(-8.0, -6.0), (8.0, -6.0), (8.0, 6.0), (-8.0, 6.0)], true);
        }
        Icon::Plus => {
            line((-6.0, 0.0), (6.0, 0.0));
            line((0.0, -6.0), (0.0, 6.0));
        }
        Icon::Import => {
            line((0.0, -7.0), (0.0, 3.0));
            poly(&[(-4.0, -1.0), (0.0, 3.0), (4.0, -1.0)], false);
            poly(&[(-7.0, 2.0), (-7.0, 7.0), (7.0, 7.0), (7.0, 2.0)], false);
        }
        Icon::Refresh => {
            let pts: Vec<(f32, f32)> = (0..=10).map(|i| {
                let a = -0.6 + i as f32 * 0.5;
                (a.cos() * 6.0, a.sin() * 6.0)
            }).collect();
            poly(&pts, false);
            poly(&[(4.5, -6.5), (5.0, -3.5), (2.0, -3.0)], false);
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
