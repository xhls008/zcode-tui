use std::collections::BTreeMap;

pub type Rgb = (u8, u8, u8);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThemePalette {
    pub accent: Rgb,
    pub accent_dim: Rgb,
    pub text: Rgb,
    pub dim: Rgb,
    pub good: Rgb,
    pub bad: Rgb,
    pub frame: Rgb,
    pub code_bg: Rgb,
    pub band_bg: Rgb,
    pub selection_fg: Rgb,
    pub light: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuiltInTheme {
    pub name: &'static str,
    pub display_alias: Option<&'static str>,
    pub palette: ThemePalette,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomTheme {
    pub name: String,
    pub base: &'static str,
    pub palette: ThemePalette,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ThemeRegistry {
    custom: Vec<CustomTheme>,
}

pub const DEFAULT_THEME: &str = "dark";

/// The single source of truth for built-in theme names, labels, and palettes.
/// Adding a theme requires one new entry here; parsing, persistence, help, and
/// TUI dispatch all derive from this registry.
pub const BUILT_IN_THEMES: &[BuiltInTheme] = &[
    BuiltInTheme {
        name: "dark",
        display_alias: None,
        palette: ThemePalette {
            accent: (96, 136, 255),
            accent_dim: (64, 88, 168),
            text: (222, 226, 234),
            dim: (122, 130, 146),
            good: (126, 200, 154),
            bad: (232, 116, 116),
            frame: (56, 62, 78),
            code_bg: (33, 38, 51),
            band_bg: (48, 52, 63),
            selection_fg: (14, 18, 30),
            light: false,
        },
    },
    BuiltInTheme {
        name: "light",
        display_alias: None,
        palette: ThemePalette {
            accent: (35, 91, 210),
            accent_dim: (76, 103, 160),
            text: (33, 38, 48),
            dim: (85, 94, 110),
            good: (31, 128, 76),
            bad: (190, 55, 55),
            frame: (168, 176, 191),
            code_bg: (235, 239, 247),
            band_bg: (225, 230, 240),
            selection_fg: (255, 255, 255),
            light: true,
        },
    },
    BuiltInTheme {
        name: "tsinghua",
        display_alias: Some("清华紫"),
        palette: ThemePalette {
            accent: (167, 104, 190),
            accent_dim: (105, 62, 121),
            text: (235, 228, 238),
            dim: (154, 139, 160),
            good: (120, 194, 151),
            bad: (234, 120, 128),
            frame: (82, 63, 88),
            code_bg: (36, 27, 41),
            band_bg: (46, 34, 52),
            selection_fg: (20, 14, 24),
            light: false,
        },
    },
    BuiltInTheme {
        name: "pku",
        display_alias: Some("北大红"),
        palette: ThemePalette {
            accent: (214, 79, 88),
            accent_dim: (137, 48, 55),
            text: (239, 229, 228),
            dim: (159, 139, 138),
            good: (126, 194, 145),
            bad: (244, 121, 118),
            frame: (91, 61, 62),
            code_bg: (42, 27, 28),
            band_bg: (54, 34, 35),
            selection_fg: (22, 14, 15),
            light: false,
        },
    },
    BuiltInTheme {
        name: "solarized-dark",
        display_alias: None,
        palette: ThemePalette {
            accent: (38, 139, 210),
            accent_dim: (29, 97, 125),
            text: (147, 161, 161),
            dim: (101, 123, 131),
            good: (133, 153, 0),
            bad: (220, 50, 47),
            frame: (7, 54, 66),
            code_bg: (0, 43, 54),
            band_bg: (7, 54, 66),
            selection_fg: (14, 18, 30),
            light: false,
        },
    },
    BuiltInTheme {
        name: "solarized-light",
        display_alias: None,
        palette: ThemePalette {
            accent: (38, 139, 210),
            accent_dim: (42, 124, 117),
            text: (88, 110, 117),
            dim: (88, 110, 117),
            good: (92, 107, 0),
            bad: (200, 45, 42),
            frame: (211, 204, 185),
            code_bg: (253, 246, 227),
            band_bg: (238, 232, 213),
            selection_fg: (14, 18, 30),
            light: true,
        },
    },
    BuiltInTheme {
        name: "dracula",
        display_alias: None,
        palette: ThemePalette {
            accent: (189, 147, 249),
            accent_dim: (132, 102, 173),
            text: (248, 248, 242),
            dim: (98, 114, 164),
            good: (80, 250, 123),
            bad: (255, 85, 85),
            frame: (68, 71, 90),
            code_bg: (40, 42, 54),
            band_bg: (52, 55, 70),
            selection_fg: (20, 21, 27),
            light: false,
        },
    },
    BuiltInTheme {
        name: "nord",
        display_alias: None,
        palette: ThemePalette {
            accent: (136, 192, 208),
            accent_dim: (94, 129, 172),
            text: (216, 222, 233),
            dim: (129, 142, 167),
            good: (163, 190, 140),
            bad: (191, 97, 106),
            frame: (76, 86, 106),
            code_bg: (46, 52, 64),
            band_bg: (59, 66, 82),
            selection_fg: (25, 30, 38),
            light: false,
        },
    },
    BuiltInTheme {
        name: "gruvbox-dark",
        display_alias: None,
        palette: ThemePalette {
            accent: (254, 128, 25),
            accent_dim: (214, 93, 14),
            text: (235, 219, 178),
            dim: (168, 153, 132),
            good: (184, 187, 38),
            bad: (251, 73, 52),
            frame: (80, 73, 69),
            code_bg: (40, 40, 40),
            band_bg: (60, 56, 54),
            selection_fg: (30, 28, 26),
            light: false,
        },
    },
    BuiltInTheme {
        name: "tokyo-night",
        display_alias: None,
        palette: ThemePalette {
            accent: (122, 162, 247),
            accent_dim: (61, 89, 161),
            text: (192, 202, 245),
            dim: (86, 95, 137),
            good: (158, 206, 106),
            bad: (247, 118, 142),
            frame: (59, 66, 97),
            code_bg: (26, 27, 38),
            band_bg: (36, 40, 59),
            selection_fg: (20, 22, 34),
            light: false,
        },
    },
    BuiltInTheme {
        name: "accessible",
        display_alias: Some("Okabe-Ito"),
        palette: ThemePalette {
            accent: (0, 114, 178),
            accent_dim: (230, 159, 0),
            text: (235, 239, 243),
            dim: (158, 168, 178),
            good: (0, 158, 115),
            bad: (213, 94, 0),
            frame: (204, 121, 167),
            code_bg: (17, 24, 39),
            band_bg: (27, 37, 51),
            selection_fg: (255, 255, 255),
            light: false,
        },
    },
];

pub fn built_in_theme(name: &str) -> Option<&'static BuiltInTheme> {
    BUILT_IN_THEMES.iter().find(|theme| theme.name == name)
}

pub fn is_built_in_theme(name: &str) -> bool {
    built_in_theme(name).is_some()
}

pub fn theme_names() -> impl Iterator<Item = &'static str> {
    BUILT_IN_THEMES.iter().map(|theme| theme.name)
}

pub fn theme_name_list(separator: &str) -> String {
    theme_names().collect::<Vec<_>>().join(separator)
}

pub fn theme_display_list(separator: &str) -> String {
    BUILT_IN_THEMES
        .iter()
        .map(|theme| match theme.display_alias {
            Some(alias) => format!("{} ({alias})", theme.name),
            None => theme.name.to_string(),
        })
        .collect::<Vec<_>>()
        .join(separator)
}

impl ThemeRegistry {
    pub fn add_custom_theme(
        &mut self,
        name: &str,
        base: &str,
        colors: &BTreeMap<String, Rgb>,
    ) -> Result<(), String> {
        validate_custom_theme_name(name)?;
        if is_built_in_theme(name) {
            return Err(format!(
                "custom theme name '{name}' conflicts with a built-in theme"
            ));
        }
        if self.custom.iter().any(|theme| theme.name == name) {
            return Err(format!("duplicate custom theme name '{name}'"));
        }
        let registered_base = built_in_theme(base)
            .ok_or_else(|| format!("unknown custom theme base '{base}'; base must be built-in"))?;
        self.custom.push(CustomTheme {
            name: name.to_string(),
            base: registered_base.name,
            palette: palette_with_overrides(registered_base.palette, colors),
        });
        Ok(())
    }

    pub fn palette(&self, name: &str) -> Option<ThemePalette> {
        built_in_theme(name).map(|theme| theme.palette).or_else(|| {
            self.custom
                .iter()
                .find(|theme| theme.name == name)
                .map(|theme| theme.palette)
        })
    }

    pub fn contains(&self, name: &str) -> bool {
        self.palette(name).is_some()
    }

    pub fn names(&self) -> Vec<String> {
        theme_names()
            .map(str::to_string)
            .chain(self.custom.iter().map(|theme| theme.name.clone()))
            .collect()
    }

    pub fn name_list(&self, separator: &str) -> String {
        self.names().join(separator)
    }

    pub fn display_list(&self, separator: &str) -> String {
        let mut display = theme_display_list(separator);
        for theme in &self.custom {
            display.push_str(separator);
            display.push_str(&format!("{} (custom)", theme.name));
        }
        display
    }

    pub fn custom_themes(&self) -> &[CustomTheme] {
        &self.custom
    }
}

fn validate_custom_theme_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("custom theme name cannot be empty".to_string());
    }
    if name.len() > 32 {
        return Err(format!(
            "custom theme name '{name}' is too long (maximum 32 characters)"
        ));
    }
    let valid = name.split('-').all(|part| {
        !part.is_empty()
            && part
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
    });
    if !valid {
        return Err(format!(
            "invalid custom theme name '{name}'; use 1-32 lowercase letters, digits, and single hyphens"
        ));
    }
    Ok(())
}

fn palette_with_overrides(
    mut palette: ThemePalette,
    colors: &BTreeMap<String, Rgb>,
) -> ThemePalette {
    for (key, value) in colors {
        match key.as_str() {
            "accent" => palette.accent = *value,
            "accent_dim" => palette.accent_dim = *value,
            "text" => palette.text = *value,
            "dim" => palette.dim = *value,
            "good" => palette.good = *value,
            "bad" => palette.bad = *value,
            "frame" => palette.frame = *value,
            "code_bg" => palette.code_bg = *value,
            "band_bg" => palette.band_bg = *value,
            "selection_fg" => palette.selection_fg = *value,
            _ => {}
        }
    }
    palette
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn registry_names_are_unique_and_resolve_to_their_own_entries() {
        assert_eq!(BUILT_IN_THEMES.len(), 11);
        assert!(built_in_theme(DEFAULT_THEME).is_some());
        let names = theme_names().collect::<HashSet<_>>();
        assert_eq!(names.len(), BUILT_IN_THEMES.len());
        for theme in BUILT_IN_THEMES {
            assert_eq!(built_in_theme(theme.name), Some(theme));
        }
        assert!(built_in_theme("ultraviolet").is_none());
    }

    #[test]
    fn every_selection_palette_has_readable_contrast() {
        fn luminance(rgb: Rgb) -> f64 {
            let channel = |value: u8| {
                let value = f64::from(value) / 255.0;
                if value <= 0.04045 {
                    value / 12.92
                } else {
                    ((value + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * channel(rgb.0) + 0.7152 * channel(rgb.1) + 0.0722 * channel(rgb.2)
        }
        fn contrast(a: Rgb, b: Rgb) -> f64 {
            let (lighter, darker) = if luminance(a) >= luminance(b) {
                (luminance(a), luminance(b))
            } else {
                (luminance(b), luminance(a))
            };
            (lighter + 0.05) / (darker + 0.05)
        }

        for theme in BUILT_IN_THEMES {
            assert!(
                contrast(theme.palette.selection_fg, theme.palette.accent) >= 4.5,
                "{} selection contrast is too low",
                theme.name
            );
        }
    }

    #[test]
    fn dynamic_registry_extends_built_ins_without_a_second_name_source() {
        let mut registry = ThemeRegistry::default();
        registry
            .add_custom_theme(
                "my-theme",
                "light",
                &[("accent".to_string(), (255, 136, 0))]
                    .into_iter()
                    .collect(),
            )
            .unwrap();

        assert_eq!(registry.names().len(), BUILT_IN_THEMES.len() + 1);
        assert!(registry.contains("my-theme"));
        assert_eq!(registry.palette("my-theme").unwrap().accent, (255, 136, 0));
        assert!(registry.palette("my-theme").unwrap().light);
        assert!(registry.display_list(", ").contains("my-theme (custom)"));
        assert!(registry.name_list("|").ends_with("|my-theme"));
    }
}
