use eframe::egui::{self, Align2, FontId, RichText, ScrollArea, TextEdit, Ui};
use rfd::FileDialog;
use std::{collections::HashSet, path::Path};

use crate::config::AppConfig;
use crate::dotnet::{DialogOpenRequest, DialogOptions, DotnetClient};
use crate::ui::effects;
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
                if let Some(path) = self.open_workspace_dialog() {
                    self.search.clear();
                    return Some(path);
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

                    let (rect, button) = ui.allocate_exact_size(
                        egui::vec2(520.0, 36.0),
                        egui::Sense::click(),
                    );
                    let id = ui.make_persistent_id(("recent", &entry.path));
                    let hovered = button.hovered();
                    let active = button.is_pointer_button_down_on();
                    let fill = effects::animated_color(
                        ui.ctx(),
                        id.with("fill"),
                        self.theme.palette.surface,
                        self.theme.palette.surface_highlight,
                        self.theme.palette.accent_weak,
                        hovered,
                        active,
                    );
                    let stroke = effects::animated_stroke(
                        ui.ctx(),
                        id.with("stroke"),
                        egui::Stroke::new(1.0, self.theme.palette.surface_highlight),
                        egui::Stroke::new(1.5, self.theme.palette.accent_weak),
                        egui::Stroke::new(2.0, self.theme.palette.accent),
                        hovered,
                        active,
                    );
                    ui.painter().rect(rect, 10.0, fill, stroke);
                    let text_color = effects::animated_color(
                        ui.ctx(),
                        id.with("text"),
                        self.theme.palette.text_primary,
                        self.theme.palette.text_primary,
                        self.theme.palette.text_primary,
                        hovered,
                        active,
                    );
                    ui.painter().text(
                        rect.left_center() + egui::vec2(12.0, 0.0),
                        Align2::LEFT_CENTER,
                        name,
                        FontId::proportional(self.theme.typography.body),
                        text_color,
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
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new(
                                "Your recent repositories will appear here once you open a workspace.",
                            )
                            .color(self.theme.palette.text_secondary),
                        );
                        ui.add_space(8.0);

                        let open_button = ui.add_sized(
                            [520.0, 36.0],
                            egui::Button::new(
                                RichText::new("Open a workspace folder")
                                    .color(self.theme.palette.text_primary)
                                    .strong(),
                            )
                            .fill(self.theme.palette.accent),
                        );

                        if open_button.hovered() {
                            ui.output_mut(|o| o.cursor_icon = egui::CursorIcon::PointingHand);
                        }

                        if open_button.clicked() {
                            if let Some(path) = self.open_workspace_dialog() {
                                self.search.clear();
                                selected = Some(path);
                            }
                        }

                        let common_paths = Self::common_paths();
                        if !common_paths.is_empty() {
                            ui.add_space(8.0);
                            ui.label(
                                RichText::new("Quick access")
                                    .color(self.theme.palette.text_secondary)
                                    .strong(),
                            );

                            for (label, path) in common_paths {
                                let response = ui.add(
                                    egui::Label::new(
                                        RichText::new(format!("{label}: {path}"))
                                            .color(self.theme.palette.text_secondary),
                                    )
                                    .sense(egui::Sense::click()),
                                );

                                if response.hovered() {
                                    ui.output_mut(|o| o.cursor_icon = egui::CursorIcon::PointingHand);
                                }

                                if response.clicked() {
                                    self.search.clear();
                                    selected = Some(path);
                                }
                            }
                        }
                    });
                }
            });

        selected
    }

    fn common_paths() -> Vec<(String, String)> {
        let mut paths = Vec::new();
        let mut seen = HashSet::new();

        let candidates = [
            ("Home", dirs::home_dir()),
            ("Desktop", dirs::desktop_dir()),
            ("Documents", dirs::document_dir()),
            ("Downloads", dirs::download_dir()),
        ];

        for (label, path) in candidates {
            if let Some(path) = path {
                if path.exists() {
                    let display = path.display().to_string();
                    if seen.insert(display.clone()) {
                        paths.push((label.to_string(), display));
                    }
                }
            }
        }

        paths
    }

    fn open_workspace_dialog(&self) -> Option<String> {
        let request = DialogOpenRequest {
            kind: "open_folder".to_string(),
            title: Some("Select a workspace folder".to_string()),
            filters: Vec::new(),
            options: DialogOptions {
                multi_select: false,
                show_hidden: false,
            },
        };
        match DotnetClient::helper().dialog_open(request) {
            Ok(response) => response.selected_paths.first().cloned(),
            Err(err) => {
                tracing::warn!("Native dialog failed: {err}");
                FileDialog::new()
                    .pick_folder()
                    .map(|path| path.display().to_string())
            }
        }
    }
}
