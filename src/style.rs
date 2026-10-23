//! Colours come from the active Omarchy theme, then get nudged until they pass WCAG contrast,
//! because any of Omarchy's themes (or a user's own) can be the input.

use iced::widget::{button, container, rule, toggler};
use iced::{Background, Border, Color, Font, Shadow, Theme, Vector, font};

#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub bg: Color,
    /// Grouped rows sit on this, a few percent toward the text colour.
    pub card: Color,
    /// Separators inside cards.
    pub line: Color,
    /// Secondary button fill.
    pub raised: Color,
    pub ink: Color,
    /// Secondary text, at least 4.5:1 on both bg and card.
    pub quiet: Color,
    /// Fill for the primary button and switched-on tracks.
    pub accent: Color,
    /// Accent used as a mark on the background (tab underline, dots): at least 3:1.
    pub accent_mark: Color,
    /// Label colour on accent fills, whichever reads best.
    pub on_accent: Color,
    /// Knob on a switched-on track: the background colour when it's visible enough (3:1).
    pub on_knob: Color,
    pub good: Color,
    /// Warning text, at least 4.5:1 on bg and card.
    pub warn: Color,
    pub bad: Color,
    /// Off switches: a visible outline and knob (3:1, WCAG 1.4.11).
    pub off_edge: Color,
    pub off_knob: Color,
}

pub fn hex(s: &str) -> Option<Color> {
    let s = s.trim().trim_start_matches('#');
    if s.len() != 6 {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(&s[i..i + 2], 16).ok();
    Some(Color::from_rgb8(byte(0)?, byte(2)?, byte(4)?))
}

pub fn mix(a: Color, b: Color, t: f32) -> Color {
    Color { r: a.r + (b.r - a.r) * t, g: a.g + (b.g - a.g) * t, b: a.b + (b.b - a.b) * t, a: a.a }
}

fn luminance(c: Color) -> f32 {
    let lin = |v: f32| if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) };
    0.2126 * lin(c.r) + 0.7152 * lin(c.g) + 0.0722 * lin(c.b)
}

/// WCAG contrast ratio, 1.0 to 21.0.
pub fn contrast(a: Color, b: Color) -> f32 {
    let (x, y) = (luminance(a), luminance(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

/// Moves `c` toward `toward` in small steps until it reaches `ratio` against every colour in `on`.
fn ensure(c: Color, on: &[Color], ratio: f32, toward: Color) -> Color {
    (0..=20).map(|i| mix(c, toward, i as f32 / 20.0)).find(|c| on.iter().all(|b| contrast(*c, *b) >= ratio)).unwrap_or(toward)
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
        let bg = get("background", "#1e1e2e");
        let ink = get("foreground", "#cdd6f4");
        let accent = get("accent", "#89b4fa");
        let raised = get("lighter_background", "#313244");
        let card = mix(bg, ink, 0.05);
        let surfaces = [bg, card];
        let on_accent = [bg, ink, Color::BLACK, Color::WHITE]
            .into_iter()
            .max_by(|a, b| contrast(*a, accent).total_cmp(&contrast(*b, accent)))
            .unwrap();
        Palette {
            bg,
            card,
            line: mix(bg, ink, 0.12),
            raised,
            ink,
            quiet: ensure(get("dark_foreground", "#6c7086"), &surfaces, 4.5, ink),
            accent,
            accent_mark: ensure(accent, &surfaces, 3.0, ink),
            on_accent,
            on_knob: if contrast(bg, accent) >= 3.0 { bg } else { on_accent },
            good: ensure(get("green", "#a6e3a1"), &surfaces, 3.0, ink),
            warn: ensure(get("yellow", "#f9e2af"), &surfaces, 4.5, ink),
            bad: ensure(get("red", "#f38ba8"), &surfaces, 3.0, ink),
            off_edge: ensure(mix(raised, ink, 0.3), &surfaces, 3.0, ink),
            off_knob: ensure(mix(raised, ink, 0.5), &[raised], 3.0, ink),
        }
    }

    pub fn theme(&self) -> Theme {
        Theme::custom(
            "Omarchy",
            iced::theme::Palette { background: self.bg, text: self.ink, primary: self.accent, success: self.good, warning: self.warn, danger: self.bad },
        )
    }

    /// The one pill-shaped main action per screen.
    pub fn primary(self) -> impl Fn(&Theme, button::Status) -> button::Style {
        move |_, status| {
            let fill = match status {
                button::Status::Hovered => mix(self.accent, self.ink, 0.15),
                button::Status::Pressed => mix(self.accent, self.bg, 0.15),
                button::Status::Disabled => mix(self.accent, self.bg, 0.45),
                button::Status::Active => self.accent,
            };
            let lifted = matches!(status, button::Status::Hovered);
            let flat = matches!(status, button::Status::Disabled | button::Status::Pressed);
            button::Style {
                background: Some(Background::Color(fill)),
                text_color: self.on_accent,
                border: Border { radius: 24.0.into(), ..Border::default() },
                shadow: if flat {
                    Shadow::default()
                } else {
                    Shadow {
                        color: self.accent.scale_alpha(if lifted { 0.4 } else { 0.22 }),
                        offset: Vector::new(0.0, if lifted { 6.0 } else { 3.0 }),
                        blur_radius: if lifted { 22.0 } else { 14.0 },
                    }
                },
                snap: true,
            }
        }
    }

    pub fn secondary(self) -> impl Fn(&Theme, button::Status) -> button::Style {
        move |_, status| button::Style {
            background: Some(Background::Color(match status {
                button::Status::Hovered => mix(self.raised, self.ink, 0.1),
                button::Status::Pressed => mix(self.raised, self.bg, 0.2),
                _ => self.raised,
            })),
            text_color: if matches!(status, button::Status::Disabled) { self.quiet } else { self.ink },
            border: Border { radius: 10.0.into(), ..Border::default() },
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
            let (on, hover, disabled) = match status {
                toggler::Status::Active { is_toggled } => (is_toggled, false, false),
                toggler::Status::Hovered { is_toggled } => (is_toggled, true, false),
                toggler::Status::Disabled { is_toggled } => (is_toggled, false, true),
            };
            let track = if on { self.accent } else { self.raised };
            let track = if hover { mix(track, self.ink, 0.12) } else { track };
            let track = if disabled { mix(track, self.bg, 0.5) } else { track };
            toggler::Style {
                background: Background::Color(track),
                background_border_width: if on { 0.0 } else { 1.0 },
                background_border_color: self.off_edge,
                foreground: Background::Color(if on { self.on_knob } else { self.off_knob }),
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

    pub fn card(self) -> impl Fn(&Theme) -> container::Style {
        self.fill(self.card, 14.0)
    }

    pub fn page(self) -> impl Fn(&Theme) -> container::Style {
        move |_| container::Style { background: Some(Background::Color(self.bg)), text_color: Some(self.ink), ..container::Style::default() }
    }

    pub fn separator(self) -> impl Fn(&Theme) -> rule::Style {
        move |_| rule::Style { color: self.line, radius: 0.0.into(), fill_mode: rule::FillMode::Full, snap: true }
    }
}

/// Noto Sans ships with Omarchy and has correctly tagged weights.
// iA Writer Quattro was tried first, but its bold file claims a regular weight, so text came out randomly heavy.
const FAMILY: &str = "Noto Sans";
const FILES: &[&str] = &["/usr/share/fonts/noto/NotoSans-Regular.ttf", "/usr/share/fonts/noto/NotoSans-Medium.ttf", "/usr/share/fonts/noto/NotoSans-Bold.ttf"];

pub struct Fonts {
    pub body: Font,
    /// Large headlines: medium weight stays calm at 30 px.
    pub medium: Font,
    pub bold: Font,
    pub files: Vec<Vec<u8>>,
}

/// Falls back to iced's own font when Noto Sans isn't installed.
pub fn fonts() -> Fonts {
    let files: Vec<Vec<u8>> = FILES.iter().filter_map(|f| std::fs::read(f).ok()).collect();
    let body = if files.is_empty() { Font::DEFAULT } else { Font::with_name(FAMILY) };
    Fonts { body, medium: Font { weight: font::Weight::Medium, ..body }, bold: Font { weight: font::Weight::Bold, ..body }, files }
}

/// Reduced motion: Hyprland's animations switch, then GNOME's.
pub fn motion_allowed() -> bool {
    let run = |prog: &str, args: &[&str]| {
        std::process::Command::new(prog).args(args).output().ok().map(|o| String::from_utf8_lossy(&o.stdout).replace(' ', ""))
    };
    let hypr_off = run("hyprctl", &["getoption", "animations:enabled", "-j"]).is_some_and(|s| s.contains("\"bool\":false") || s.contains("\"int\":0"));
    let gnome_off = run("gsettings", &["get", "org.gnome.desktop.interface", "enable-animations"]).is_some_and(|s| s.trim() == "false");
    !(hypr_off || gnome_off)
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
        assert!((contrast(Color::BLACK, Color::WHITE) - 21.0).abs() < 0.01);
    }

    fn check(name: &str, p: &Palette) {
        for (role, c, on, min) in [
            ("ink", p.ink, p.card, 4.5),
            ("quiet", p.quiet, p.bg, 4.5),
            ("quiet", p.quiet, p.card, 4.5),
            ("warn", p.warn, p.card, 4.5),
            ("on_accent", p.on_accent, p.accent, 4.5),
            ("on_knob", p.on_knob, p.accent, 3.0),
            ("accent_mark", p.accent_mark, p.bg, 3.0),
            ("off_edge", p.off_edge, p.bg, 3.0),
            ("off_knob", p.off_knob, p.raised, 3.0),
        ] {
            assert!(contrast(c, on) >= min, "{name}: {role} is {:.2}:1, needs {min}:1", contrast(c, on));
        }
    }

    #[test]
    fn every_bundled_omarchy_theme_passes_contrast() {
        check("fallback", &Palette::from_toml(""));
        // Only present on Omarchy machines; CI checks the fallback above.
        let Ok(dirs) = std::fs::read_dir("/usr/share/omarchy/themes") else { return };
        for dir in dirs.flatten() {
            if let Ok(text) = std::fs::read_to_string(dir.path().join("colors.toml")) {
                check(&dir.file_name().to_string_lossy(), &Palette::from_toml(&text));
            }
        }
    }
}
