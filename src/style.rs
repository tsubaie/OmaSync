//! Colours come from the active Omarchy theme, so the app changes with it.

use eframe::egui::{self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Stroke, TextStyle};
use std::sync::Arc;

#[derive(Clone, Copy)]
pub struct Palette {
    pub bg: Color32,
    pub surface: Color32,
    pub raised: Color32,
    pub ink: Color32,
    pub quiet: Color32,
    pub accent: Color32,
    pub good: Color32,
    pub warn: Color32,
    pub bad: Color32,
    pub dark: bool,
}

impl Palette {
    pub fn load() -> Palette {
        let state = std::env::var("XDG_STATE_HOME").unwrap_or_else(|_| format!("{}/.local/state", std::env::var("HOME").unwrap_or_default()));
        let text = std::fs::read_to_string(format!("{state}/omarchy/current/theme/colors.toml")).unwrap_or_default();
        Palette::from_toml(&text)
    }

    /// Reads `key = "#rrggbb"` lines; anything missing falls back to Omarchy's Catppuccin.
    pub fn from_toml(text: &str) -> Palette {
        let get = |key: &str, fallback: &str| {
            let v = text
                .lines()
                .filter_map(|l| l.split_once('='))
                .find(|(k, _)| k.trim() == key)
                .map(|(_, v)| v.trim().trim_matches('"').to_string());
            v.and_then(|v| Color32::from_hex(&v).ok()).unwrap_or_else(|| Color32::from_hex(fallback).unwrap())
        };
        Palette {
            bg: get("background", "#1e1e2e"),
            surface: get("dark_background", "#161622"),
            raised: get("lighter_background", "#313244"),
            ink: get("foreground", "#cdd6f4"),
            quiet: get("dark_foreground", "#6c7086"),
            accent: get("accent", "#89b4fa"),
            good: get("green", "#a6e3a1"),
            warn: get("yellow", "#f9e2af"),
            bad: get("red", "#f38ba8"),
            dark: !text.lines().any(|l| l.replace(' ', "") == "mode=\"light\""),
        }
    }
}

pub fn bold() -> FontFamily {
    FontFamily::Name("bold".into())
}

pub fn apply(ctx: &egui::Context, p: &Palette) {
    let mut fonts = FontDefinitions::default();
    // iA Writer Quattro ships with Omarchy; Adwaita Sans is the fallback, then egui's built-in font.
    let pick = |files: &[&str]| files.iter().find_map(|f| std::fs::read(f).ok());
    let regular = pick(&["/usr/share/fonts/ttf-ia-writer/iAWriterQuattroS-Regular.ttf", "/usr/share/fonts/Adwaita/AdwaitaSans-Regular.ttf"]);
    let heavy = pick(&["/usr/share/fonts/ttf-ia-writer/iAWriterQuattroS-Bold.ttf"]);
    if let Some(bytes) = regular {
        fonts.font_data.insert("regular".into(), Arc::new(FontData::from_owned(bytes)));
        fonts.families.entry(FontFamily::Proportional).or_default().insert(0, "regular".into());
    }
    let mut bold_stack = fonts.families[&FontFamily::Proportional].clone();
    if let Some(bytes) = heavy {
        fonts.font_data.insert("heavy".into(), Arc::new(FontData::from_owned(bytes)));
        bold_stack.insert(0, "heavy".into());
    }
    fonts.families.insert(bold(), bold_stack);
    ctx.set_fonts(fonts);

    let mut v = if p.dark { egui::Visuals::dark() } else { egui::Visuals::light() };
    v.panel_fill = p.bg;
    v.window_fill = p.surface;
    v.extreme_bg_color = p.surface;
    v.faint_bg_color = p.surface;
    v.override_text_color = Some(p.ink);
    v.hyperlink_color = p.accent;
    v.selection.bg_fill = p.accent.gamma_multiply(0.35);
    v.selection.stroke = Stroke::new(2.0, p.accent);
    let r = CornerRadius::same(8);
    for w in [&mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
        w.corner_radius = r;
        w.fg_stroke.color = p.ink;
    }
    v.widgets.noninteractive.fg_stroke.color = p.ink;
    v.widgets.inactive.weak_bg_fill = p.raised;
    v.widgets.inactive.bg_fill = p.raised;
    v.widgets.inactive.bg_stroke = Stroke::NONE;
    v.widgets.hovered.weak_bg_fill = p.raised.gamma_multiply(1.4);
    v.widgets.hovered.bg_fill = p.raised.gamma_multiply(1.4);
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, p.quiet);
    v.widgets.active.weak_bg_fill = p.accent.gamma_multiply(0.5);
    v.widgets.active.bg_stroke = Stroke::new(1.0, p.accent);
    ctx.set_visuals_of(if p.dark { egui::Theme::Dark } else { egui::Theme::Light }, v);
    ctx.set_theme(if p.dark { egui::Theme::Dark } else { egui::Theme::Light });

    ctx.all_styles_mut(|s| {
        s.text_styles = [
            (TextStyle::Heading, FontId::new(30.0, bold())),
            (TextStyle::Name("title".into()), FontId::new(18.0, bold())),
            (TextStyle::Body, FontId::proportional(15.0)),
            (TextStyle::Button, FontId::proportional(15.0)),
            (TextStyle::Small, FontId::proportional(13.0)),
            (TextStyle::Monospace, FontId::monospace(13.0)),
        ]
        .into();
        s.spacing.item_spacing = egui::vec2(10.0, 8.0);
        s.spacing.button_padding = egui::vec2(14.0, 7.0);
        s.spacing.interact_size.y = 30.0;
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_theme_and_falls_back() {
        let p = Palette::from_toml("mode = \"light\"\naccent = \"#ff0000\"\n");
        assert_eq!(p.accent, Color32::from_rgb(255, 0, 0));
        assert_eq!(p.bg, Color32::from_hex("#1e1e2e").unwrap());
        assert!(!p.dark);
        assert!(Palette::from_toml("").dark);
    }
}
