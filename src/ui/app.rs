use eframe::egui::{self, RichText};

use crate::ui::theme::Theme;

pub struct GitSpaceApp {
    theme: Theme,
    initialized: bool,
}

impl GitSpaceApp {
    pub fn new() -> Self {
        Self {
            theme: Theme::dark(),
            initialized: false,
        }
    }

    fn initialize_if_needed(&mut self, ctx: &egui::Context) {
        if !self.initialized {
            self.theme.apply(ctx);
            self.initialized = true;
        }
    }
}

impl eframe::App for GitSpaceApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.initialize_if_needed(ctx);

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(12.0);
                ui.heading(RichText::new("GitSpace").strong());
                ui.label(RichText::new("Centralized Git workspace shell").color(self.theme.palette.text_secondary));
                ui.add_space(12.0);
                ui.colored_label(
                    self.theme.palette.accent,
                    "Theming: dark palette, accent colors, and shared typography tokens are active.",
                );
            });
        });
    }
}
