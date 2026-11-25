//! The older panels' theme, read from the active colony-ui palette, with the
//! typography and spacing tokens they use.

use iced::Color;

use colony_ui::theme::Palette as Kit;

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
    /// The active colony-ui theme, in the roles the older panels use.
    pub fn active() -> Self {
        Self {
            background: Kit::BG_PRIMARY(),
            surface: Kit::BG_CARD(),
            surface_highlight: Kit::BG_SELECTED(),
            text_primary: Kit::TEXT_PRIMARY(),
            text_secondary: Kit::TEXT_SECONDARY(),
            accent: Kit::ACCENT(),
            accent_weak: Kit::ACCENT_ICON(),
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
    fn with_palette(palette: Palette) -> Self {
        Self {
            palette,
            typography: Typography::default(),
            spacing: Spacing::default(),
        }
    }

    /// Reads the theme colony-ui has active; call it again after `StandardPreferences::apply`.
    pub fn active() -> Self {
        Self::with_palette(Palette::active())
    }

    /// Convertit le thème GitSpace en thème Iced natif.
    pub fn to_iced_theme(&self) -> iced::Theme {
        let iced_palette = iced::theme::Palette {
            background: self.palette.background,
            text: self.palette.text_primary,
            primary: self.palette.accent,
            success: Kit::SUCCESS(),
            warning: Kit::WARNING(),
            danger: Kit::ERROR(),
        };
        iced::Theme::custom_with_fn("GitSpace".to_string(), iced_palette, |palette| {
            iced::theme::palette::Extended::generate(palette)
        })
    }
}

/// Applique une opacité alpha à une couleur Iced.
pub fn with_alpha(color: Color, alpha: f32) -> Color {
    Color {
        a: alpha.clamp(0.0, 1.0),
        ..color
    }
}
