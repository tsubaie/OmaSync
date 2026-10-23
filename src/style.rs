//! Colours come from the active Omarchy theme, so the app changes with it.

use iced::widget::{button, container, rule, toggler};
use iced::{Background, Border, Color, Font, Shadow, Theme, Vector, font};

#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub bg: Color,
    pub raised: Color,
    pub ink: Color,
    pub quiet: Color,
    pub accent: Color,
    pub good: Color,
    pub warn: Color,
    pub bad: Color,
}

pub fn hex(s: &str) -> Option<Color> {
    let s = s.trim().trim_start_matches('#');
    if s.len() != 6 {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(&s[i..i + 2], 16).ok();
    Some(Color::from_rgb8(byte(0)?, byte(2)?, byte(4)?))
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
            text.lines()
                .filter_map(|l| l.split_once('='))
                .find(|(k, _)| k.trim() == key)
                .and_then(|(_, v)| hex(v.trim().trim_matches('"')))
                .unwrap_or_else(|| hex(fallback).unwrap())
        };
        Palette {
            bg: get("background", "#1e1e2e"),
            raised: get("lighter_background", "#313244"),
            ink: get("foreground", "#cdd6f4"),
            quiet: get("dark_foreground", "#6c7086"),
            accent: get("accent", "#89b4fa"),
            good: get("green", "#a6e3a1"),
            warn: get("yellow", "#f9e2af"),
            bad: get("red", "#f38ba8"),
        }
    }

    pub fn theme(&self) -> Theme {
        Theme::custom(
            "Omarchy",
            iced::theme::Palette { background: self.bg, text: self.ink, primary: self.accent, success: self.good, warning: self.warn, danger: self.bad },
        )
    }

    pub fn primary(self) -> impl Fn(&Theme, button::Status) -> button::Style {
        move |_, status| {
            let fill = match status {
                button::Status::Hovered => mix(self.accent, self.ink, 0.15),
                button::Status::Pressed => mix(self.accent, self.bg, 0.15),
                button::Status::Disabled => self.accent.scale_alpha(0.45),
                button::Status::Active => self.accent,
            };
            let lifted = matches!(status, button::Status::Hovered);
            button::Style {
                background: Some(Background::Color(fill)),
                text_color: self.bg,
                border: Border { radius: 12.0.into(), ..Border::default() },
                shadow: Shadow {
                    color: self.accent.scale_alpha(if lifted { 0.45 } else { 0.25 }),
                    offset: Vector::new(0.0, if lifted { 6.0 } else { 3.0 }),
                    blur_radius: if lifted { 22.0 } else { 14.0 },
                },
                snap: true,
            }
        }
    }

    pub fn secondary(self) -> impl Fn(&Theme, button::Status) -> button::Style {
        move |_, status| button::Style {
            background: Some(Background::Color(match status {
                button::Status::Hovered => mix(self.raised, self.ink, 0.08),
                _ => self.raised,
            })),
            text_color: self.ink,
            border: Border { radius: 12.0.into(), ..Border::default() },
            shadow: Shadow::default(),
            snap: true,
        }
    }

    /// Text-only button: quiet until hovered.
    pub fn link(self, color: Color) -> impl Fn(&Theme, button::Status) -> button::Style {
        move |_, status| button::Style {
            background: None,
            text_color: if matches!(status, button::Status::Hovered) { self.ink } else { color },
            border: Border::default(),
            shadow: Shadow::default(),
            snap: true,
        }
    }

    pub fn switch(self) -> impl Fn(&Theme, toggler::Status) -> toggler::Style {
        move |_, status| {
            let (on, hover, off) = match status {
                toggler::Status::Active { is_toggled } => (is_toggled, false, false),
                toggler::Status::Hovered { is_toggled } => (is_toggled, true, false),
                toggler::Status::Disabled { is_toggled } => (is_toggled, false, true),
            };
            let track = if on { self.accent } else { self.raised };
            let track = if hover { mix(track, self.ink, 0.12) } else { track };
            toggler::Style {
                background: Background::Color(if off { track.scale_alpha(0.5) } else { track }),
                background_border_width: 0.0,
                background_border_color: Color::TRANSPARENT,
                foreground: Background::Color(if on { self.bg } else { self.quiet }),
                foreground_border_width: 0.0,
                foreground_border_color: Color::TRANSPARENT,
                text_color: Some(self.ink),
                border_radius: None,
                padding_ratio: 0.2,
            }
        }
    }

    pub fn fill(self, color: Color, radius: f32) -> impl Fn(&Theme) -> container::Style {
        move |_| container::Style {
            background: Some(Background::Color(color)),
            border: Border { radius: radius.into(), ..Border::default() },
            ..container::Style::default()
        }
    }

    pub fn page(self) -> impl Fn(&Theme) -> container::Style {
        move |_| container::Style { background: Some(Background::Color(self.bg)), text_color: Some(self.ink), ..container::Style::default() }
    }

    pub fn hairline(self) -> impl Fn(&Theme) -> rule::Style {
        move |_| rule::Style { color: self.raised, radius: 0.0.into(), fill_mode: rule::FillMode::Full, snap: true }
    }
}

pub fn mix(a: Color, b: Color, t: f32) -> Color {
    Color { r: a.r + (b.r - a.r) * t, g: a.g + (b.g - a.g) * t, b: a.b + (b.b - a.b) * t, a: a.a }
}

/// Noto Sans ships with Omarchy and has correctly tagged weights.
// iA Writer Quattro was tried first, but its bold file claims a regular weight, so text came out randomly heavy.
const BODY: (&str, &str) = ("Noto Sans", "/usr/share/fonts/noto/NotoSans-Regular.ttf");
const HEAD: (&str, &str) = ("Noto Sans", "/usr/share/fonts/noto/NotoSans-Bold.ttf");

/// (body font, heading font, font files to load); falls back to iced's own fonts.
pub fn fonts() -> (Font, Font, Vec<Vec<u8>>) {
    let mut files = Vec::new();
    let mut pick = |(name, path): (&'static str, &str), fallback: Font| match std::fs::read(path) {
        Ok(bytes) => {
            files.push(bytes);
            Font::with_name(name)
        }
        Err(_) => fallback,
    };
    let body = pick(BODY, Font::DEFAULT);
    let head = Font { weight: font::Weight::Bold, ..pick(HEAD, Font::DEFAULT) };
    (body, head, files)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_theme_and_falls_back() {
        let p = Palette::from_toml("accent = \"#ff0000\"\n");
        assert_eq!(p.accent, Color::from_rgb8(255, 0, 0));
        assert_eq!(p.bg, hex("#1e1e2e").unwrap());
        assert_eq!(hex("#12345"), None);
    }
}
