use eframe::egui::{self, Color32, FontId, Rounding, Stroke, TextStyle};

// Material Forge's design system (docs/design/material_forge_design_system.html):
// graphite surfaces that lighten as they rise, the logo's forge orange as the
// only brand color, red for masks; rounded floating panels over the canvas.
/// The canvas behind the views (and the start screen).
pub const BG: Color32 = Color32::from_rgb(20, 20, 22);
/// Floating panel body (surface 0).
pub const PANEL: Color32 = Color32::from_rgb(27, 27, 30);
/// Panel headers, tab strips (surface 0, a hair lighter).
pub const HEADER: Color32 = Color32::from_rgb(29, 29, 32);
/// Input fields, slots, cards (surface 1 / 2).
pub const FIELD: Color32 = Color32::from_rgb(38, 38, 43);
/// Hover surface (surface 3).
pub const HOVER: Color32 = Color32::from_rgb(52, 52, 58);
/// Pressed surface (surface 4).
pub const PRESSED: Color32 = Color32::from_rgb(63, 63, 70);
/// Thin separators and panel outlines (white at 7 %).
pub const SEPARATOR: Color32 = Color32::from_rgba_premultiplied(18, 18, 18, 18);
/// Stronger outline (white at 12 %).
pub const LINE: Color32 = Color32::from_rgba_premultiplied(31, 31, 31, 31);
/// Forge orange (action, selection, active tool, the primary button).
pub const ACCENT: Color32 = Color32::from_rgb(242, 107, 29);
/// Selected rows: the accent at 16 % over the panel.
pub const ROW_SELECTED: Color32 = Color32::from_rgb(61, 39, 28);
/// Masks.
pub const MASK: Color32 = Color32::from_rgb(229, 72, 77);
/// Text levels.
pub const TEXT: Color32 = Color32::from_rgb(241, 241, 244);
pub const TEXT2: Color32 = Color32::from_rgb(180, 180, 189);
pub const TEXT3: Color32 = Color32::from_rgb(124, 124, 134);
/// Panel radius, control radius.
pub const R_PANEL: f32 = 12.0;
pub const R_CONTROL: f32 = 8.0;
/// Gap between floating panels and the window edge.
pub const GAP: f32 = 10.0;

/// A gray of the theme (graphite, a touch cool), for painted fills and lines.
pub const fn warm(g: u8) -> Color32 {
    Color32::from_rgb(g, g, g.saturating_add(4))
}

/// Shadow of floating panels (elevation 1), of the dock and windows (2),
/// and of popups and tooltips (3: its blur stays under a tooltip's height,
/// a wider one folds over itself into a dark streak under it).
pub fn shadow(level: u8) -> egui::Shadow {
    match level {
        1 => egui::Shadow { offset: egui::vec2(0.0, 8.0), blur: 24.0, spread: 0.0, color: Color32::from_black_alpha(90) },
        3 => egui::Shadow { offset: egui::vec2(0.0, 6.0), blur: 16.0, spread: 0.0, color: Color32::from_black_alpha(120) },
        _ => egui::Shadow { offset: egui::vec2(0.0, 14.0), blur: 40.0, spread: 0.0, color: Color32::from_black_alpha(140) },
    }
}

/// Family used for bold labels (section titles, names, tabs).
pub fn bold(size: f32) -> FontId {
    FontId::new(size, egui::FontFamily::Name("bold".into()))
}

/// Segoe UI Variable from the system (regular, semibold for titles) and
/// Cascadia Mono for numbers when present; egui's fonts stay as fallbacks.
fn install_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let dir = std::path::Path::new(r"C:\Windows\Fonts");
    let load = |f: &str| std::fs::read(dir.join(f)).ok();
    let mut bold_family = fonts.families.get(&egui::FontFamily::Proportional).cloned().unwrap_or_default();
    if let Some(r) = load("SegUIVar.ttf").or_else(|| load("segoeui.ttf")) {
        fonts.font_data.insert("segoe".into(), egui::FontData::from_owned(r));
        fonts.families.entry(egui::FontFamily::Proportional).or_default().insert(0, "segoe".into());
    }
    if let Some(b) = load("seguisb.ttf").or_else(|| load("segoeuib.ttf")) {
        fonts.font_data.insert("segoe bold".into(), egui::FontData::from_owned(b));
        bold_family.insert(0, "segoe bold".into());
    }
    if let Some(m) = load("CascadiaMono.ttf") {
        fonts.font_data.insert("cascadia".into(), egui::FontData::from_owned(m));
        fonts.families.entry(egui::FontFamily::Monospace).or_default().insert(0, "cascadia".into());
    }
    fonts.families.insert(egui::FontFamily::Name("bold".into()), bold_family);
    ctx.set_fonts(fonts);
}

/// Graphite theme with the forge orange accent, rounded controls.
pub fn apply(ctx: &egui::Context) {
    install_fonts(ctx);
    let mut v = egui::Visuals::dark();
    v.panel_fill = PANEL;
    v.window_fill = Color32::from_rgb(29, 29, 32);
    v.window_stroke = Stroke::new(1.0f32, LINE);
    v.extreme_bg_color = Color32::from_rgb(34, 34, 38);
    v.faint_bg_color = Color32::from_rgb(34, 34, 38);
    v.code_bg_color = FIELD;
    v.selection.bg_fill = ACCENT;
    v.selection.stroke = Stroke::new(1.0f32, Color32::WHITE);
    v.hyperlink_color = ACCENT;
    v.override_text_color = Some(TEXT);
    v.widgets.noninteractive.bg_fill = PANEL;
    v.widgets.noninteractive.weak_bg_fill = PANEL;
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0f32, SEPARATOR);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0f32, TEXT2);
    v.widgets.inactive.bg_fill = FIELD;
    v.widgets.inactive.weak_bg_fill = FIELD;
    v.widgets.inactive.bg_stroke = Stroke::NONE;
    v.widgets.hovered.bg_fill = HOVER;
    v.widgets.hovered.weak_bg_fill = HOVER;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0f32, LINE);
    v.widgets.active.bg_fill = ACCENT;
    v.widgets.active.weak_bg_fill = PRESSED;
    v.widgets.active.bg_stroke = Stroke::new(1.0f32, ACCENT);
    v.widgets.open.weak_bg_fill = HOVER;
    v.widgets.open.bg_stroke = Stroke::new(1.0f32, ACCENT);
    for w in [&mut v.widgets.noninteractive, &mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
        w.rounding = Rounding::same(R_CONTROL);
    }
    v.window_rounding = Rounding::same(16.0);
    v.menu_rounding = Rounding::same(R_PANEL);
    v.popup_shadow = shadow(3);
    v.window_shadow = shadow(2);
    v.slider_trailing_fill = true;
    ctx.set_visuals(v);

    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = egui::vec2(6.0, 5.0);
    style.spacing.button_padding = egui::vec2(12.0, 5.0);
    style.spacing.interact_size.y = 26.0;
    style.spacing.slider_width = 110.0;
    style.spacing.indent = 14.0;
    style.spacing.combo_height = 360.0;
    style.spacing.menu_margin = egui::Margin::same(6.0);
    // Snappy: no eased programmatic scrolling, short open / close animations.
    style.scroll_animation = egui::style::ScrollAnimation::none();
    style.animation_time = 0.05;
    style.text_styles = [
        (TextStyle::Small, FontId::proportional(10.5)),
        (TextStyle::Body, FontId::proportional(12.0)),
        (TextStyle::Button, FontId::proportional(12.0)),
        (TextStyle::Heading, FontId::proportional(15.0)),
        (TextStyle::Monospace, FontId::monospace(12.0)),
    ]
    .into();
    ctx.set_style(style);
    // Painter's labels are not selectable text: a click on one does nothing.
    ctx.style_mut(|s| s.interaction.selectable_labels = false);
}
