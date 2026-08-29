use ratatui::style::{Color, Style, Stylize};
#[cfg(test)]
use zcode_tui::theme_registry::built_in_theme;
use zcode_tui::theme_registry::{ThemePalette, DEFAULT_THEME};
use zcode_tui::UiConfig;

/// Zhipu-flavored theme in a Codex-like shell.
#[derive(Clone, Copy)]
pub(crate) struct Theme {
    pub(crate) plain: bool,
    light: bool,
    accent: Color,
    accent_dim: Color,
    text: Color,
    dim: Color,
    good: Color,
    bad: Color,
    frame: Color,
    pub(crate) code_bg: Color,
    band_bg: Color,
    selection_fg: Color,
}

impl Theme {
    #[cfg(test)]
    pub(crate) fn named(name: &str, plain: bool) -> Self {
        let palette = built_in_theme(name)
            .or_else(|| built_in_theme(DEFAULT_THEME))
            .expect("default theme is registered")
            .palette;
        Self::from_palette(palette, plain)
    }

    pub(crate) fn configured(name: &str, config: &UiConfig, plain: bool) -> Self {
        let palette = config
            .themes
            .palette(name)
            .or_else(|| config.themes.palette(DEFAULT_THEME))
            .expect("default theme is registered");
        Self::from_palette(palette, plain).with_overrides(config)
    }

    fn from_palette(palette: ThemePalette, plain: bool) -> Self {
        let color = |(r, g, b)| Color::Rgb(r, g, b);
        Self {
            plain,
            light: palette.light,
            accent: color(palette.accent),
            accent_dim: color(palette.accent_dim),
            text: color(palette.text),
            dim: color(palette.dim),
            good: color(palette.good),
            bad: color(palette.bad),
            frame: color(palette.frame),
            code_bg: color(palette.code_bg),
            band_bg: color(palette.band_bg),
            selection_fg: color(palette.selection_fg),
        }
    }

    fn styled(&self, color: Color) -> Style {
        if self.plain {
            Style::default()
        } else {
            Style::default().fg(color)
        }
    }

    pub(crate) fn accent(&self) -> Style {
        self.styled(self.accent)
    }

    pub(crate) fn accent_dim(&self) -> Style {
        self.styled(self.accent_dim)
    }

    pub(crate) fn text(&self) -> Style {
        self.styled(self.text)
    }

    pub(crate) fn dim(&self) -> Style {
        self.styled(self.dim)
    }

    pub(crate) fn good(&self) -> Style {
        self.styled(self.good)
    }

    pub(crate) fn bad(&self) -> Style {
        self.styled(self.bad)
    }

    pub(crate) fn frame(&self) -> Style {
        self.styled(self.frame)
    }

    pub(crate) fn code(&self) -> Style {
        if self.plain {
            Style::default()
        } else {
            Style::default().fg(self.text).bg(self.code_bg)
        }
    }

    pub(crate) fn band(&self) -> Style {
        if self.plain {
            Style::default()
        } else {
            Style::default().bg(self.band_bg)
        }
    }

    pub(crate) fn selection(&self) -> Style {
        if self.plain {
            Style::default().reversed()
        } else {
            Style::default().fg(self.selection_fg).bg(self.accent)
        }
    }

    /// Syntect uses a dark source palette. Darken its RGB values on light code
    /// panels so the hue remains useful without low-contrast pastel text.
    pub(crate) fn syntax_color(&self, r: u8, g: u8, b: u8) -> Color {
        if self.light {
            let darken = |channel| ((u16::from(channel) * 2) / 5) as u8;
            Color::Rgb(darken(r), darken(g), darken(b))
        } else {
            Color::Rgb(r, g, b)
        }
    }

    pub(crate) fn with_overrides(mut self, config: &UiConfig) -> Self {
        for (key, (r, g, b)) in &config.colors {
            let color = Color::Rgb(*r, *g, *b);
            match key.as_str() {
                "accent" => self.accent = color,
                "accent_dim" => self.accent_dim = color,
                "text" => self.text = color,
                "dim" => self.dim = color,
                "good" => self.good = color,
                "bad" => self.bad = color,
                "frame" => self.frame = color,
                "code_bg" => self.code_bg = color,
                "band_bg" => self.band_bg = color,
                "selection_fg" => self.selection_fg = color,
                _ => {}
            }
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zcode_tui::theme_registry::BUILT_IN_THEMES;

    #[test]
    fn registry_dispatches_every_palette_and_unknown_falls_back_to_dark() {
        for registered in BUILT_IN_THEMES {
            let theme = Theme::named(registered.name, false);
            let (r, g, b) = registered.palette.accent;
            assert_eq!(theme.accent, Color::Rgb(r, g, b));
            let (r, g, b) = registered.palette.code_bg;
            assert_eq!(theme.code_bg, Color::Rgb(r, g, b));
            assert_eq!(Theme::named(registered.name, true).text(), Style::default());
        }
        assert_eq!(
            Theme::named("ultraviolet", false).accent,
            Theme::named("dark", false).accent
        );
    }

    #[test]
    fn accessible_palette_and_selection_foregrounds_are_dispatched() {
        let accessible = Theme::named("accessible", false);
        assert_eq!(accessible.accent, Color::Rgb(0, 114, 178));
        assert_eq!(accessible.accent_dim, Color::Rgb(230, 159, 0));
        assert_eq!(accessible.good, Color::Rgb(0, 158, 115));
        assert_eq!(accessible.frame, Color::Rgb(204, 121, 167));
        assert_eq!(accessible.selection().fg, Some(Color::Rgb(255, 255, 255)));

        for registered in BUILT_IN_THEMES {
            let (r, g, b) = registered.palette.selection_fg;
            assert_eq!(
                Theme::named(registered.name, false).selection().fg,
                Some(Color::Rgb(r, g, b))
            );
        }
    }

    #[test]
    fn light_themes_adapt_dark_syntax_colors() {
        let source = (216, 222, 233);
        assert_eq!(
            Theme::named("light", false).syntax_color(source.0, source.1, source.2),
            Color::Rgb(86, 88, 93)
        );
        assert_eq!(
            Theme::named("solarized-light", false).syntax_color(source.0, source.1, source.2),
            Color::Rgb(86, 88, 93)
        );
        assert_eq!(
            Theme::named("dark", false).syntax_color(source.0, source.1, source.2),
            Color::Rgb(source.0, source.1, source.2)
        );
    }

    #[test]
    fn selection_foreground_can_be_overridden() {
        let config = UiConfig {
            colors: [("selection_fg".to_string(), (1, 2, 3))]
                .into_iter()
                .collect(),
            ..UiConfig::default()
        };
        assert_eq!(
            Theme::named("light", false)
                .with_overrides(&config)
                .selection()
                .fg,
            Some(Color::Rgb(1, 2, 3))
        );
    }

    #[test]
    fn configured_theme_uses_custom_palette_then_global_overrides() {
        let config = zcode_tui::parse_ui_config(
            "theme = my-theme\ntext = #010203\n[[custom_themes]]\nname = \"my-theme\"\nbase = \"light\"\naccent = \"#ff8800\"\n",
        );
        let theme = Theme::configured("my-theme", &config, false);
        assert_eq!(theme.accent, Color::Rgb(255, 136, 0));
        assert_eq!(theme.text, Color::Rgb(1, 2, 3));
        assert!(theme.light);
    }
}
