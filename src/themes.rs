//! Farb-Themen für BoxDoc.

use egui::{Color32, Context, Stroke, Style, Visuals};

use crate::model::Theme;

/// Wendet das gewählte Thema auf den egui-Kontext an.
pub fn apply(ctx: &Context, theme: Theme) {
    let mut style = Style::default();

    match theme {
        Theme::Light => {
            style.visuals = light_tweaks(Visuals::light());
        }
        Theme::DarkClassic => {
            style.visuals = dark_classic();
        }
        Theme::DarkCalm => {
            style.visuals = dark_calm();
        }
    }

    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    ctx.set_style(style);
}

/// Feinschliff für den hellen Modus — weiches, warmes Weiß.
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

/// Klassischer Dark Mode — neutral, kontrastreich, professionell.
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

/// Calm Dark Mode — warme, entspannende Töne, augenfreundlich.
fn dark_calm() -> Visuals {
    let mut v = Visuals::dark();
    v.dark_mode = true;
    // Sanftes Dunkelgrau mit warmem Stich.
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
    // Sanftes, beruhigendes Teal/Türkis als Akzent.
    v.selection.bg_fill = Color32::from_rgb(70, 150, 145);
    v.selection.stroke = Stroke::new(1.0, Color32::from_rgb(100, 180, 175));
    v.hyperlink_color = Color32::from_rgb(120, 190, 180);
    v
}
