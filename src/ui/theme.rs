use egui::{Color32, TextStyle};

use crate::config::ThemeMode;

#[derive(Debug, Clone)]
pub struct Palette {
    pub background: Color32,
    pub surface: Color32,
    pub surface_highlight: Color32,
    pub text_primary: Color32,
    pub text_secondary: Color32,
    pub accent: Color32,
    pub accent_weak: Color32,
}

impl Palette {
    pub fn latte() -> Self {
        Self {
            background: Color32::from_rgb(0xef, 0xf1, 0xf5),
            surface: Color32::from_rgb(0xe6, 0xe9, 0xef),
            surface_highlight: Color32::from_rgb(0xcc, 0xd0, 0xda),
            text_primary: Color32::from_rgb(0x4c, 0x4f, 0x69),
            text_secondary: Color32::from_rgb(0x5c, 0x5f, 0x77),
            accent: Color32::from_rgb(0x1e, 0x66, 0xf5),
            accent_weak: Color32::from_rgb(0x20, 0x9f, 0xb5),
        }
    }

    pub fn frappe() -> Self {
        Self {
            background: Color32::from_rgb(0x23, 0x26, 0x34),
            surface: Color32::from_rgb(0x29, 0x2c, 0x3c),
            surface_highlight: Color32::from_rgb(0x41, 0x45, 0x59),
            text_primary: Color32::from_rgb(0xc6, 0xd0, 0xf5),
            text_secondary: Color32::from_rgb(0xb5, 0xbf, 0xe2),
            accent: Color32::from_rgb(0x8c, 0xaa, 0xee),
            accent_weak: Color32::from_rgb(0x85, 0xc1, 0xdc),
        }
    }

    pub fn macchiato() -> Self {
        Self {
            background: Color32::from_rgb(0x18, 0x19, 0x26),
            surface: Color32::from_rgb(0x1e, 0x20, 0x30),
            surface_highlight: Color32::from_rgb(0x36, 0x3a, 0x4f),
            text_primary: Color32::from_rgb(0xca, 0xd3, 0xf5),
            text_secondary: Color32::from_rgb(0xb8, 0xc0, 0xe0),
            accent: Color32::from_rgb(0x8a, 0xad, 0xf4),
            accent_weak: Color32::from_rgb(0x7d, 0xc4, 0xe4),
        }
    }

    pub fn mocha() -> Self {
        Self {
            background: Color32::from_rgb(0x11, 0x11, 0x1b),
            surface: Color32::from_rgb(0x18, 0x18, 0x25),
            surface_highlight: Color32::from_rgb(0x31, 0x32, 0x44),
            text_primary: Color32::from_rgb(0xcd, 0xd6, 0xf4),
            text_secondary: Color32::from_rgb(0xba, 0xc2, 0xde),
            accent: Color32::from_rgb(0x89, 0xb4, 0xfa),
            accent_weak: Color32::from_rgb(0x74, 0xc7, 0xec),
        }
    }
}

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

#[derive(Debug, Clone)]
pub struct Theme {
    pub palette: Palette,
    pub typography: Typography,
    pub spacing: Spacing,
    is_dark: bool,
}

impl Theme {
    pub fn latte() -> Self {
        Self {
            palette: Palette::latte(),
            typography: Typography::default(),
            spacing: Spacing::default(),
            is_dark: false,
        }
    }

    pub fn frappe() -> Self {
        Self {
            palette: Palette::frappe(),
            typography: Typography::default(),
            spacing: Spacing::default(),
            is_dark: true,
        }
    }

    pub fn macchiato() -> Self {
        Self {
            palette: Palette::macchiato(),
            typography: Typography::default(),
            spacing: Spacing::default(),
            is_dark: true,
        }
    }

    pub fn mocha() -> Self {
        Self {
            palette: Palette::mocha(),
            typography: Typography::default(),
            spacing: Spacing::default(),
            is_dark: true,
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

    pub fn apply(&self, ctx: &egui::Context) {
        let mut visuals = if self.is_dark {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        };
        visuals.dark_mode = self.is_dark;
        visuals.override_text_color = Some(self.palette.text_primary);
        visuals.widgets.noninteractive.bg_fill = self.palette.background;
        visuals.widgets.noninteractive.fg_stroke.color = self.palette.text_secondary;
        visuals.widgets.inactive.bg_fill = self.palette.surface;
        visuals.widgets.inactive.fg_stroke.color = self.palette.text_primary;
        visuals.widgets.hovered.bg_fill = self.palette.surface_highlight;
        visuals.widgets.hovered.fg_stroke.color = self.palette.text_primary;
        visuals.faint_bg_color = self.palette.surface_highlight;
        visuals.extreme_bg_color = self.palette.surface;
        visuals.code_bg_color = self.palette.surface_highlight;
        visuals.window_fill = self.palette.background;
        visuals.panel_fill = self.palette.background;
        visuals.selection.bg_fill = self.palette.accent;
        visuals.selection.stroke.color = self.palette.accent_weak;
        visuals.hyperlink_color = self.palette.accent;

        ctx.set_visuals(visuals);

        let mut style = (*ctx.style()).clone();
        style.text_styles = [
            (
                TextStyle::Heading,
                egui::FontId::proportional(self.typography.heading),
            ),
            (
                TextStyle::Name("Title".into()),
                egui::FontId::proportional(self.typography.title),
            ),
            (
                TextStyle::Body,
                egui::FontId::proportional(self.typography.body),
            ),
            (
                TextStyle::Button,
                egui::FontId::proportional(self.typography.body),
            ),
            (
                TextStyle::Small,
                egui::FontId::proportional(self.typography.label),
            ),
        ]
        .into();

        ctx.set_style(style);
    }
}
