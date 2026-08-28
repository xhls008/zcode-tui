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
