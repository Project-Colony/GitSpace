//! Système de thèmes Catppuccin pour GitSpace avec Iced.
//!
//! Fournit les palettes Latte, Frappé, Macchiato et Mocha en tant que
//! thèmes custom Iced, avec les tokens de typographie et d'espacement.

use iced::Color;

use crate::config::ThemeMode;

/// Palette de couleurs Catppuccin.
#[derive(Debug, Clone)]
pub struct Palette {
    pub background: Color,
    pub surface: Color,
    pub surface_highlight: Color,
    pub text_primary: Color,
    pub text_secondary: Color,
    pub accent: Color,
    pub accent_weak: Color,
}

impl Palette {
    pub fn latte() -> Self {
        Self {
            background: color_from_rgb(0xef, 0xf1, 0xf5),
            surface: color_from_rgb(0xe6, 0xe9, 0xef),
            surface_highlight: color_from_rgb(0xcc, 0xd0, 0xda),
            text_primary: color_from_rgb(0x4c, 0x4f, 0x69),
            text_secondary: color_from_rgb(0x5c, 0x5f, 0x77),
            accent: color_from_rgb(0x1e, 0x66, 0xf5),
            accent_weak: color_from_rgb(0x20, 0x9f, 0xb5),
        }
    }

    pub fn frappe() -> Self {
        Self {
            background: color_from_rgb(0x23, 0x26, 0x34),
            surface: color_from_rgb(0x29, 0x2c, 0x3c),
            surface_highlight: color_from_rgb(0x41, 0x45, 0x59),
            text_primary: color_from_rgb(0xc6, 0xd0, 0xf5),
            text_secondary: color_from_rgb(0xb5, 0xbf, 0xe2),
            accent: color_from_rgb(0x8c, 0xaa, 0xee),
            accent_weak: color_from_rgb(0x85, 0xc1, 0xdc),
        }
    }

    pub fn macchiato() -> Self {
        Self {
            background: color_from_rgb(0x18, 0x19, 0x26),
            surface: color_from_rgb(0x1e, 0x20, 0x30),
            surface_highlight: color_from_rgb(0x36, 0x3a, 0x4f),
            text_primary: color_from_rgb(0xca, 0xd3, 0xf5),
            text_secondary: color_from_rgb(0xb8, 0xc0, 0xe0),
            accent: color_from_rgb(0x8a, 0xad, 0xf4),
            accent_weak: color_from_rgb(0x7d, 0xc4, 0xe4),
        }
    }

    pub fn mocha() -> Self {
        Self {
            background: color_from_rgb(0x11, 0x11, 0x1b),
            surface: color_from_rgb(0x18, 0x18, 0x25),
            surface_highlight: color_from_rgb(0x31, 0x32, 0x44),
            text_primary: color_from_rgb(0xcd, 0xd6, 0xf4),
            text_secondary: color_from_rgb(0xba, 0xc2, 0xde),
            accent: color_from_rgb(0x89, 0xb4, 0xfa),
            accent_weak: color_from_rgb(0x74, 0xc7, 0xec),
        }
    }
}

/// Tokens de typographie.
#[derive(Debug, Clone)]
pub struct Typography {
    pub heading: f32,
    pub title: f32,
    pub body: f32,
    pub label: f32,
}

impl Default for Typography {
    fn default() -> Self {
        Self {
            heading: 28.0,
            title: 20.0,
            body: 16.0,
            label: 14.0,
        }
    }
}

/// Tokens d'espacement.
#[derive(Debug, Clone, Copy)]
pub struct Spacing {
    pub xs: f32,
    pub sm: f32,
    pub md: f32,
    pub lg: f32,
}

impl Default for Spacing {
    fn default() -> Self {
        Self {
            xs: 4.0,
            sm: 8.0,
            md: 12.0,
            lg: 16.0,
        }
    }
}

/// Thème GitSpace complet avec palette, typographie et espacement.
#[derive(Debug, Clone)]
pub struct Theme {
    pub palette: Palette,
    pub typography: Typography,
    pub spacing: Spacing,
}

impl Theme {
    pub fn latte() -> Self {
        Self {
            palette: Palette::latte(),
            typography: Typography::default(),
            spacing: Spacing::default(),
        }
    }

    pub fn frappe() -> Self {
        Self {
            palette: Palette::frappe(),
            typography: Typography::default(),
            spacing: Spacing::default(),
        }
    }

    pub fn macchiato() -> Self {
        Self {
            palette: Palette::macchiato(),
            typography: Typography::default(),
            spacing: Spacing::default(),
        }
    }

    pub fn mocha() -> Self {
        Self {
            palette: Palette::mocha(),
            typography: Typography::default(),
            spacing: Spacing::default(),
        }
    }

    pub fn from_mode(mode: ThemeMode) -> Self {
        match mode {
            ThemeMode::Latte => Self::latte(),
            ThemeMode::Frappe => Self::frappe(),
            ThemeMode::Macchiato => Self::macchiato(),
            ThemeMode::Mocha => Self::mocha(),
        }
    }

    /// Convertit le thème GitSpace en thème Iced natif.
    pub fn to_iced_theme(&self) -> iced::Theme {
        let iced_palette = iced::theme::Palette {
            background: self.palette.background,
            text: self.palette.text_primary,
            primary: self.palette.accent,
            success: color_from_rgb(0xa6, 0xe3, 0xa1),
            danger: color_from_rgb(0xf3, 0x8b, 0xa8),
        };
        iced::Theme::custom_with_fn(
            "Catppuccin".to_string(),
            iced_palette,
            |palette| iced::theme::palette::Extended::generate(palette),
        )
    }
}

/// Convertit des composantes RGB (0-255) en `iced::Color`.
fn color_from_rgb(r: u8, g: u8, b: u8) -> Color {
    Color::from_rgb8(r, g, b)
}

/// Applique une opacité alpha à une couleur Iced.
pub fn with_alpha(color: Color, alpha: f32) -> Color {
    Color { a: alpha.clamp(0.0, 1.0), ..color }
}
