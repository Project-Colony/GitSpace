use eframe::egui::{self, RichText, ScrollArea, TextEdit, Ui};
use rfd::FileDialog;
use std::path::Path;

use crate::config::AppConfig;
use crate::ui::theme::Theme;

#[derive(Debug, Clone)]
pub struct RecentList {
    theme: Theme,
    search: String,
}

impl RecentList {
    pub fn new(theme: Theme) -> Self {
        Self {
            theme,
            search: String::new(),
        }
    }

    pub fn set_theme(&mut self, theme: Theme) {
        self.theme = theme;
    }

    pub fn ui(&mut self, ui: &mut Ui, config: &AppConfig) -> Option<String> {
        ui.heading(RichText::new("Recently opened").color(self.theme.palette.text_primary));
        ui.label(
            RichText::new("Search and reopen workspaces you've used recently.")
                .color(self.theme.palette.text_secondary),
        );
        ui.add_space(8.0);

        let browse_result = ui.horizontal(|ui| {
            ui.label(RichText::new("Filter").color(self.theme.palette.text_secondary));
            ui.add_sized(
                [320.0, 28.0],
                TextEdit::singleline(&mut self.search).hint_text("Type to filter by name or path"),
            );

            if ui.button("Browse...").clicked() {
                if let Some(path) = FileDialog::new().pick_folder() {
                    self.search.clear();
                    return Some(path.display().to_string());
                }
            }

            None
        });

        if browse_result.inner.is_some() {
            return browse_result.inner;
        }

        ui.add_space(8.0);
        let mut selected: Option<String> = None;
        ScrollArea::vertical()
            .auto_shrink([false; 2])
            .show(ui, |ui| {
                let query = self.search.to_lowercase();
                for entry in config.recent_repos() {
                    let path = Path::new(&entry.path);
                    let name = path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or(&entry.path);

                    let matches_query = query.is_empty()
                        || entry.path.to_lowercase().contains(&query)
                        || name.to_lowercase().contains(&query);

                    if !matches_query {
                        continue;
                    }

                    let button = ui.add_sized(
                        [520.0, 34.0],
                        egui::Button::new(
                            RichText::new(format!("{}", name))
                                .color(self.theme.palette.text_primary)
                                .strong(),
                        )
                        .fill(self.theme.palette.surface_highlight),
                    );

                    if button.hovered() {
                        ui.output_mut(|o| o.cursor_icon = egui::CursorIcon::PointingHand);
                        ui.label(
                            RichText::new(&entry.path)
                                .color(self.theme.palette.text_secondary)
                                .italics(),
                        );
                    }

                    if button.clicked() {
                        selected = Some(entry.path.clone());
                    }
                }

                if config.recent_repos().is_empty() {
                    ui.label(
                        RichText::new(
                            "Your recent repositories will appear here once you open a workspace.",
                        )
                        .color(self.theme.palette.text_secondary),
                    );
                }
            });

        selected
    }
}
