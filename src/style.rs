use eframe::egui::{self, Color32, FontId, Rounding, Stroke, TextStyle};

// Palette measured on the real application's dark theme.
/// Viewport background.
pub const BG: Color32 = Color32::from_rgb(36, 36, 36);
/// Docked panel body, menu bar, tool strip, status bar.
pub const PANEL: Color32 = Color32::from_rgb(51, 51, 51);
/// Panel header bars and tab strips.
pub const HEADER: Color32 = Color32::from_rgb(43, 43, 43);
/// Input fields, slots, cards.
pub const FIELD: Color32 = Color32::from_rgb(45, 45, 45);
/// Thin separators between rows and panels.
pub const SEPARATOR: Color32 = Color32::from_rgb(26, 26, 26);
/// Selection blue (outlines, active tabs).
pub const ACCENT: Color32 = Color32::from_rgb(55, 142, 240);
/// Selected layer / texture set row (outlined in ACCENT).
pub const ROW_SELECTED: Color32 = Color32::from_rgb(38, 38, 38);

/// Family used for bold labels (section titles, names, tabs).
pub fn bold(size: f32) -> FontId {
    FontId::new(size, egui::FontFamily::Name("bold".into()))
}

/// Segoe UI from the system (regular + bold) when present, so the UI reads
/// like a native Windows application; egui's fonts stay as fallbacks.
fn install_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let dir = std::path::Path::new(r"C:\Windows\Fonts");
    let load = |f: &str| std::fs::read(dir.join(f)).ok();
    let mut bold_family = fonts.families.get(&egui::FontFamily::Proportional).cloned().unwrap_or_default();
    if let Some(r) = load("segoeui.ttf") {
        fonts.font_data.insert("segoe".into(), egui::FontData::from_owned(r));
        fonts.families.entry(egui::FontFamily::Proportional).or_default().insert(0, "segoe".into());
    }
    if let Some(b) = load("segoeuib.ttf") {
        fonts.font_data.insert("segoe bold".into(), egui::FontData::from_owned(b));
        bold_family.insert(0, "segoe bold".into());
    }
    fonts.families.insert(egui::FontFamily::Name("bold".into()), bold_family);
    ctx.set_fonts(fonts);
}

/// Dark neutral theme with a blue accent, compact metrics.
pub fn apply(ctx: &egui::Context) {
    install_fonts(ctx);
    let mut v = egui::Visuals::dark();
    v.panel_fill = PANEL;
    v.window_fill = Color32::from_rgb(46, 46, 46);
    v.window_stroke = Stroke::new(1.0f32, Color32::from_rgb(70, 70, 70));
    v.extreme_bg_color = Color32::from_rgb(38, 38, 38);
    v.faint_bg_color = Color32::from_rgb(56, 56, 56);
    v.code_bg_color = FIELD;
    v.selection.bg_fill = ACCENT;
    v.selection.stroke = Stroke::new(1.0f32, Color32::WHITE);
    v.hyperlink_color = ACCENT;
    v.override_text_color = Some(Color32::from_gray(215));
    v.widgets.noninteractive.bg_fill = PANEL;
    v.widgets.noninteractive.weak_bg_fill = PANEL;
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0f32, Color32::from_rgb(64, 64, 64));
    v.widgets.inactive.bg_fill = FIELD;
    v.widgets.inactive.weak_bg_fill = FIELD;
    v.widgets.inactive.bg_stroke = Stroke::new(1.0f32, Color32::from_rgb(66, 66, 66));
    v.widgets.hovered.bg_fill = Color32::from_rgb(58, 58, 58);
    v.widgets.hovered.weak_bg_fill = Color32::from_rgb(58, 58, 58);
    v.widgets.hovered.bg_stroke = Stroke::new(1.0f32, Color32::from_rgb(110, 110, 110));
    v.widgets.active.bg_fill = ACCENT;
    v.widgets.active.weak_bg_fill = Color32::from_rgb(64, 64, 64);
    v.widgets.active.bg_stroke = Stroke::new(1.0f32, ACCENT);
    v.widgets.open.weak_bg_fill = Color32::from_rgb(58, 58, 58);
    v.widgets.open.bg_stroke = Stroke::new(1.0f32, ACCENT);
    for w in [&mut v.widgets.noninteractive, &mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
        w.rounding = Rounding::same(2.0);
    }
    v.window_rounding = Rounding::same(3.0);
    v.menu_rounding = Rounding::same(2.0);
    v.popup_shadow = egui::Shadow { offset: egui::vec2(0.0, 4.0), blur: 12.0, spread: 0.0, color: Color32::from_black_alpha(120) };
    v.window_shadow = egui::Shadow { offset: egui::vec2(0.0, 6.0), blur: 18.0, spread: 0.0, color: Color32::from_black_alpha(140) };
    v.slider_trailing_fill = true;
    ctx.set_visuals(v);

    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = egui::vec2(6.0, 4.0);
    style.spacing.button_padding = egui::vec2(8.0, 3.0);
    style.spacing.interact_size.y = 22.0;
    style.spacing.slider_width = 110.0;
    style.spacing.indent = 14.0;
    style.spacing.combo_height = 360.0;
    style.spacing.menu_margin = egui::Margin::same(4.0);
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
