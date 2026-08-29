use ratatui::style::{Color, Style, Stylize};
use zcode_tui::UiConfig;

/// Zhipu-flavored theme in a Codex-like shell.
#[derive(Clone, Copy)]
pub(crate) struct Theme {
    pub(crate) plain: bool,
    accent: Color,
    accent_dim: Color,
    text: Color,
    dim: Color,
    good: Color,
    bad: Color,
    frame: Color,
    pub(crate) code_bg: Color,
    band_bg: Color,
}

impl Theme {
    pub(crate) fn named(name: &str, plain: bool) -> Self {
        match name {
            "light" => Self::light(plain),
            "tsinghua" => Self::tsinghua(plain),
            "pku" => Self::pku(plain),
            "solarized-dark" => Self::solarized_dark(plain),
            "solarized-light" => Self::solarized_light(plain),
            "dracula" => Self::dracula(plain),
            "nord" => Self::nord(plain),
            "gruvbox-dark" => Self::gruvbox_dark(plain),
            "tokyo-night" => Self::tokyo_night(plain),
            _ => Self::zhipu(plain),
        }
    }

    pub(crate) fn zhipu(plain: bool) -> Self {
        Self {
            plain,
            accent: Color::Rgb(96, 136, 255),
            accent_dim: Color::Rgb(64, 88, 168),
            text: Color::Rgb(222, 226, 234),
            dim: Color::Rgb(122, 130, 146),
            good: Color::Rgb(126, 200, 154),
            bad: Color::Rgb(232, 116, 116),
            frame: Color::Rgb(56, 62, 78),
            code_bg: Color::Rgb(33, 38, 51),
            band_bg: Color::Rgb(48, 52, 63),
        }
    }

    fn light(plain: bool) -> Self {
        Self {
            plain,
            accent: Color::Rgb(35, 91, 210),
            accent_dim: Color::Rgb(76, 103, 160),
            text: Color::Rgb(33, 38, 48),
            dim: Color::Rgb(99, 108, 124),
            good: Color::Rgb(31, 128, 76),
            bad: Color::Rgb(190, 55, 55),
            frame: Color::Rgb(168, 176, 191),
            code_bg: Color::Rgb(235, 239, 247),
            band_bg: Color::Rgb(225, 230, 240),
        }
    }

    fn tsinghua(plain: bool) -> Self {
        Self {
            plain,
            accent: Color::Rgb(167, 104, 190),
            accent_dim: Color::Rgb(105, 62, 121),
            text: Color::Rgb(235, 228, 238),
            dim: Color::Rgb(154, 139, 160),
            good: Color::Rgb(120, 194, 151),
            bad: Color::Rgb(234, 120, 128),
            frame: Color::Rgb(82, 63, 88),
            code_bg: Color::Rgb(36, 27, 41),
            band_bg: Color::Rgb(46, 34, 52),
        }
    }

    fn pku(plain: bool) -> Self {
        Self {
            plain,
            accent: Color::Rgb(214, 79, 88),
            accent_dim: Color::Rgb(137, 48, 55),
            text: Color::Rgb(239, 229, 228),
            dim: Color::Rgb(159, 139, 138),
            good: Color::Rgb(126, 194, 145),
            bad: Color::Rgb(244, 121, 118),
            frame: Color::Rgb(91, 61, 62),
            code_bg: Color::Rgb(42, 27, 28),
            band_bg: Color::Rgb(54, 34, 35),
        }
    }

    fn solarized_dark(plain: bool) -> Self {
        Self {
            plain,
            accent: Color::Rgb(38, 139, 210),
            accent_dim: Color::Rgb(29, 97, 125),
            text: Color::Rgb(147, 161, 161),
            dim: Color::Rgb(101, 123, 131),
            good: Color::Rgb(133, 153, 0),
            bad: Color::Rgb(220, 50, 47),
            frame: Color::Rgb(7, 54, 66),
            code_bg: Color::Rgb(0, 43, 54),
            band_bg: Color::Rgb(7, 54, 66),
        }
    }

    fn solarized_light(plain: bool) -> Self {
        Self {
            plain,
            accent: Color::Rgb(38, 139, 210),
            accent_dim: Color::Rgb(42, 161, 152),
            text: Color::Rgb(88, 110, 117),
            dim: Color::Rgb(131, 148, 150),
            good: Color::Rgb(133, 153, 0),
            bad: Color::Rgb(220, 50, 47),
            frame: Color::Rgb(238, 232, 213),
            code_bg: Color::Rgb(253, 246, 227),
            band_bg: Color::Rgb(238, 232, 213),
        }
    }

    fn dracula(plain: bool) -> Self {
        Self {
            plain,
            accent: Color::Rgb(189, 147, 249),
            accent_dim: Color::Rgb(132, 102, 173),
            text: Color::Rgb(248, 248, 242),
            dim: Color::Rgb(98, 114, 164),
            good: Color::Rgb(80, 250, 123),
            bad: Color::Rgb(255, 85, 85),
            frame: Color::Rgb(68, 71, 90),
            code_bg: Color::Rgb(40, 42, 54),
            band_bg: Color::Rgb(52, 55, 70),
        }
    }

    fn nord(plain: bool) -> Self {
        Self {
            plain,
            accent: Color::Rgb(136, 192, 208),
            accent_dim: Color::Rgb(94, 129, 172),
            text: Color::Rgb(216, 222, 233),
            dim: Color::Rgb(129, 142, 167),
            good: Color::Rgb(163, 190, 140),
            bad: Color::Rgb(191, 97, 106),
            frame: Color::Rgb(76, 86, 106),
            code_bg: Color::Rgb(46, 52, 64),
            band_bg: Color::Rgb(59, 66, 82),
        }
    }

    fn gruvbox_dark(plain: bool) -> Self {
        Self {
            plain,
            accent: Color::Rgb(254, 128, 25),
            accent_dim: Color::Rgb(214, 93, 14),
            text: Color::Rgb(235, 219, 178),
            dim: Color::Rgb(168, 153, 132),
            good: Color::Rgb(184, 187, 38),
            bad: Color::Rgb(251, 73, 52),
            frame: Color::Rgb(80, 73, 69),
            code_bg: Color::Rgb(40, 40, 40),
            band_bg: Color::Rgb(60, 56, 54),
        }
    }

    fn tokyo_night(plain: bool) -> Self {
        Self {
            plain,
            accent: Color::Rgb(122, 162, 247),
            accent_dim: Color::Rgb(61, 89, 161),
            text: Color::Rgb(192, 202, 245),
            dim: Color::Rgb(86, 95, 137),
            good: Color::Rgb(158, 206, 106),
            bad: Color::Rgb(247, 118, 142),
            frame: Color::Rgb(59, 66, 97),
            code_bg: Color::Rgb(26, 27, 38),
            band_bg: Color::Rgb(36, 40, 59),
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
            Style::default().fg(Color::Rgb(14, 18, 30)).bg(self.accent)
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
                _ => {}
            }
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_dispatches_built_in_palettes_and_plain_stays_plain() {
        let palettes = [
            ("dark", (96, 136, 255), (33, 38, 51)),
            ("light", (35, 91, 210), (235, 239, 247)),
            ("tsinghua", (167, 104, 190), (36, 27, 41)),
            ("pku", (214, 79, 88), (42, 27, 28)),
            ("solarized-dark", (38, 139, 210), (0, 43, 54)),
            ("solarized-light", (38, 139, 210), (253, 246, 227)),
            ("dracula", (189, 147, 249), (40, 42, 54)),
            ("nord", (136, 192, 208), (46, 52, 64)),
            ("gruvbox-dark", (254, 128, 25), (40, 40, 40)),
            ("tokyo-night", (122, 162, 247), (26, 27, 38)),
        ];

        for (name, accent, code_bg) in palettes {
            let theme = Theme::named(name, false);
            assert_eq!(theme.accent, Color::Rgb(accent.0, accent.1, accent.2));
            assert_eq!(theme.code_bg, Color::Rgb(code_bg.0, code_bg.1, code_bg.2));
            assert_eq!(Theme::named(name, true).text(), Style::default());
        }
    }
}
