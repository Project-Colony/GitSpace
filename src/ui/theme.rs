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
    pub fn dark() -> Self {
        Self {
            background: Color32::from_rgb(0x10, 0x12, 0x16),
            surface: Color32::from_rgb(0x18, 0x1b, 0x22),
            surface_highlight: Color32::from_rgb(0x21, 0x24, 0x2d),
            text_primary: Color32::from_rgb(0xed, 0xef, 0xf7),
            text_secondary: Color32::from_rgb(0xb9, 0xc0, 0xcc),
            accent: Color32::from_rgb(0x7c, 0xd1, 0xff),
            accent_weak: Color32::from_rgb(0x4d, 0x9d, 0xe6),
        }
    }

    pub fn light() -> Self {
        Self {
            background: Color32::from_rgb(0xf5, 0xf6, 0xf8),
            surface: Color32::from_rgb(0xff, 0xff, 0xff),
            surface_highlight: Color32::from_rgb(0xed, 0xf0, 0xf5),
            text_primary: Color32::from_rgb(0x17, 0x1a, 0x22),
            text_secondary: Color32::from_rgb(0x43, 0x48, 0x55),
            accent: Color32::from_rgb(0x2b, 0x6c, 0xc4),
            accent_weak: Color32::from_rgb(0x4d, 0x8c, 0xe6),
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

#[derive(Debug, Clone)]
pub struct Theme {
    pub palette: Palette,
    pub typography: Typography,
}

impl Theme {
    pub fn dark() -> Self {
        Self {
            palette: Palette::dark(),
            typography: Typography::default(),
        }
    }

    pub fn light() -> Self {
        Self {
            palette: Palette::light(),
            typography: Typography::default(),
        }
    }

    pub fn from_mode(mode: ThemeMode) -> Self {
        match mode {
            ThemeMode::Dark => Self::dark(),
            ThemeMode::Light => Self::light(),
        }
    }

    pub fn apply(&self, ctx: &egui::Context) {
        let mut visuals = egui::Visuals::dark();
        visuals.override_text_color = Some(self.palette.text_primary);
        visuals.widgets.noninteractive.bg_fill = self.palette.background;
        visuals.widgets.noninteractive.fg_stroke.color = self.palette.text_secondary;
        visuals.widgets.inactive.bg_fill = self.palette.surface;
        visuals.widgets.inactive.fg_stroke.color = self.palette.text_primary;
        visuals.widgets.hovered.bg_fill = self.palette.surface_highlight;
        visuals.widgets.hovered.fg_stroke.color = self.palette.text_primary;
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
