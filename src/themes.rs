//! Farb-Themen für BoxDoc — mit weichem Übergang (Fade).

use egui::{Color32, Context, Stroke, Style, Visuals};
use egui::style::{WidgetVisuals, Widgets};

use crate::model::Theme;

/// Dauer des Theme-Fades in Sekunden.
const FADE_DURATION: f32 = 0.35;

/// Wendet ein Thema sofort an (ohne Fade) — beim Start.
pub fn apply(ctx: &Context, theme: Theme) {
    set_style(ctx, &visuals_for(theme));
}

/// Treibt die Fade-Animation voran. Gibt `true` zurück, solange animiert wird.
pub fn tick_fade(ctx: &Context, from: Theme, to: Theme, t: f32) -> bool {
    let t = t.min(1.0);
    if t >= 1.0 {
        set_style(ctx, &visuals_for(to));
        return false;
    }
    let eased = t * t * (3.0 - 2.0 * t); // smoothstep
    let blended = blend(&visuals_for(from), &visuals_for(to), eased);
    set_style(ctx, &blended);
    ctx.request_repaint();
    true
}

/// Gibt die Fade-Dauer zurück.
pub fn fade_duration() -> f32 {
    FADE_DURATION
}

fn set_style(ctx: &Context, visuals: &Visuals) {
    let mut style = Style::default();
    style.visuals = visuals.clone();
    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    ctx.set_style(style);
}

/// Liefert die vollständigen Visuals für ein Thema.
pub fn visuals_for(theme: Theme) -> Visuals {
    match theme {
        Theme::Light => light_tweaks(Visuals::light()),
        Theme::DarkClassic => dark_classic(),
        Theme::DarkCalm => dark_calm(),
    }
}

/// Interpoliert linear zwischen zwei Visuals-Zuständen.
fn blend(a: &Visuals, b: &Visuals, t: f32) -> Visuals {
    let mut out = a.clone();
    out.dark_mode = b.dark_mode;
    out.faint_bg_color = lerp_color(a.faint_bg_color, b.faint_bg_color, t);
    out.extreme_bg_color = lerp_color(a.extreme_bg_color, b.extreme_bg_color, t);
    out.window_fill = lerp_color(a.window_fill, b.window_fill, t);
    out.selection.bg_fill = lerp_color(a.selection.bg_fill, b.selection.bg_fill, t);
    out.selection.stroke = Stroke::new(
        lerp_f32(a.selection.stroke.width, b.selection.stroke.width, t),
        lerp_color(a.selection.stroke.color, b.selection.stroke.color, t),
    );
    out.hyperlink_color = lerp_color(a.hyperlink_color, b.hyperlink_color, t);
    out.widgets = blend_widgets(&a.widgets, &b.widgets, t);
    out
}

fn blend_widgets(a: &Widgets, b: &Widgets, t: f32) -> Widgets {
    Widgets {
        noninteractive: blend_wv(&a.noninteractive, &b.noninteractive, t),
        inactive: blend_wv(&a.inactive, &b.inactive, t),
        hovered: blend_wv(&a.hovered, &b.hovered, t),
        active: blend_wv(&a.active, &b.active, t),
        open: blend_wv(&a.open, &b.open, t),
    }
}

fn blend_wv(a: &WidgetVisuals, b: &WidgetVisuals, t: f32) -> WidgetVisuals {
    WidgetVisuals {
        bg_fill: lerp_color(a.bg_fill, b.bg_fill, t),
        weak_bg_fill: lerp_color(a.weak_bg_fill, b.weak_bg_fill, t),
        bg_stroke: Stroke::new(
            lerp_f32(a.bg_stroke.width, b.bg_stroke.width, t),
            lerp_color(a.bg_stroke.color, b.bg_stroke.color, t),
        ),
        corner_radius: a.corner_radius,
        fg_stroke: Stroke::new(
            lerp_f32(a.fg_stroke.width, b.fg_stroke.width, t),
            lerp_color(a.fg_stroke.color, b.fg_stroke.color, t),
        ),
        expansion: lerp_f32(a.expansion, b.expansion, t),
    }
}

fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    Color32::from_rgba_premultiplied(
        lerp_u8(a.r(), b.r(), t),
        lerp_u8(a.g(), b.g(), t),
        lerp_u8(a.b(), b.b(), t),
        lerp_u8(a.a(), b.a(), t),
    )
}

fn lerp_u8(a: u8, b: u8, t: f32) -> u8 {
    (a as f32 + (b as f32 - a as f32) * t).round().clamp(0.0, 255.0) as u8
}

fn lerp_f32(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

// --- Theme-Definitionen ---

fn light_tweaks(mut v: Visuals) -> Visuals {
    v.dark_mode = false;
    v.faint_bg_color = Color32::from_rgb(240, 240, 238);
    v.extreme_bg_color = Color32::from_rgb(235, 235, 232);
    v.window_fill = Color32::from_rgb(248, 248, 246);
    v.widgets.noninteractive.bg_fill = Color32::from_rgb(238, 238, 235);
    v.widgets.noninteractive.weak_bg_fill = Color32::from_rgb(242, 242, 239);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, Color32::from_rgb(90, 88, 85));
    v.widgets.inactive.weak_bg_fill = Color32::from_rgb(245, 245, 242);
    v.widgets.inactive.bg_fill = Color32::from_rgb(240, 240, 237);
    v.widgets.hovered.weak_bg_fill = Color32::from_rgb(230, 235, 245);
    v.widgets.hovered.bg_fill = Color32::from_rgb(225, 232, 242);
    v.widgets.hovered.fg_stroke = Stroke::new(1.0, Color32::from_rgb(30, 70, 160));
    v.widgets.active.weak_bg_fill = Color32::from_rgb(220, 228, 242);
    v.widgets.active.bg_fill = Color32::from_rgb(215, 225, 240);
    v.selection.bg_fill = Color32::from_rgb(40, 120, 220);
    v.selection.stroke = Stroke::new(1.0, Color32::from_rgb(30, 100, 200));
    v.hyperlink_color = Color32::from_rgb(30, 100, 200);
    v
}

fn dark_classic() -> Visuals {
    let mut v = Visuals::dark();
    v.dark_mode = true;
    v.faint_bg_color = Color32::from_rgb(38, 38, 38);
    v.extreme_bg_color = Color32::from_rgb(18, 18, 18);
    v.window_fill = Color32::from_rgb(40, 40, 40);
    v.widgets.noninteractive.bg_fill = Color32::from_rgb(30, 30, 30);
    v.widgets.noninteractive.weak_bg_fill = Color32::from_rgb(35, 35, 35);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, Color32::from_rgb(170, 170, 170));
    v.widgets.inactive.weak_bg_fill = Color32::from_rgb(42, 42, 42);
    v.widgets.inactive.bg_fill = Color32::from_rgb(45, 45, 45);
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, Color32::from_rgb(160, 160, 160));
    v.widgets.hovered.weak_bg_fill = Color32::from_rgb(52, 52, 52);
    v.widgets.hovered.bg_fill = Color32::from_rgb(56, 56, 56);
    v.widgets.hovered.fg_stroke = Stroke::new(1.0, Color32::from_rgb(220, 220, 220));
    v.widgets.active.weak_bg_fill = Color32::from_rgb(60, 60, 60);
    v.widgets.active.bg_fill = Color32::from_rgb(66, 66, 66);
    v.widgets.active.fg_stroke = Stroke::new(1.0, Color32::from_rgb(240, 240, 240));
    v.selection.bg_fill = Color32::from_rgb(40, 120, 220);
    v.selection.stroke = Stroke::new(1.0, Color32::from_rgb(80, 150, 240));
    v.hyperlink_color = Color32::from_rgb(100, 170, 255);
    v
}

fn dark_calm() -> Visuals {
    let mut v = Visuals::dark();
    v.dark_mode = true;
    v.faint_bg_color = Color32::from_rgb(42, 40, 46);
    v.extreme_bg_color = Color32::from_rgb(22, 20, 26);
    v.window_fill = Color32::from_rgb(44, 42, 48);
    v.widgets.noninteractive.bg_fill = Color32::from_rgb(34, 32, 38);
    v.widgets.noninteractive.weak_bg_fill = Color32::from_rgb(38, 36, 42);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, Color32::from_rgb(160, 150, 155));
    v.widgets.inactive.weak_bg_fill = Color32::from_rgb(46, 44, 50);
    v.widgets.inactive.bg_fill = Color32::from_rgb(49, 47, 53);
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, Color32::from_rgb(155, 145, 152));
    v.widgets.hovered.weak_bg_fill = Color32::from_rgb(56, 52, 60);
    v.widgets.hovered.bg_fill = Color32::from_rgb(60, 56, 65);
    v.widgets.hovered.fg_stroke = Stroke::new(1.0, Color32::from_rgb(210, 200, 205));
    v.widgets.active.weak_bg_fill = Color32::from_rgb(64, 58, 70);
    v.widgets.active.bg_fill = Color32::from_rgb(68, 62, 75);
    v.widgets.active.fg_stroke = Stroke::new(1.0, Color32::from_rgb(225, 215, 220));
    v.selection.bg_fill = Color32::from_rgb(70, 150, 145);
    v.selection.stroke = Stroke::new(1.0, Color32::from_rgb(100, 180, 175));
    v.hyperlink_color = Color32::from_rgb(120, 190, 180);
    v
}
