use eframe::egui;

use crate::ui::{
    layout::{MainTab, ShellLayout},
    theme::Theme,
};

pub struct GitSpaceApp {
    theme: Theme,
    initialized: bool,
    active_tab: MainTab,
}

impl GitSpaceApp {
    pub fn new() -> Self {
        Self {
            theme: Theme::dark(),
            initialized: false,
            active_tab: MainTab::Clone,
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

        let layout = ShellLayout::new(&self.theme);
        layout.header(ctx);
        layout.sidebar(ctx);
        layout.right_panel(ctx);

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.set_min_height(ui.available_height());
            layout.tab_bar(ui, &mut self.active_tab);
            layout.tab_content(ui, self.active_tab);
        });
    }
}
