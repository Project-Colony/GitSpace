use std::collections::HashMap;
use std::sync::Arc;

use chrono::{Datelike, NaiveDate, TimeZone, Utc};
use eframe::egui::{self, Align, Layout, Pos2, RichText, Sense, Ui};

use crate::git::{
    diff::{FileDiff, FileDiffSummary, commit_diff_file, commit_diff_summaries},
    log::{CommitFilter, CommitInfo, list_local_branches, read_commit_log},
};
use crate::ui::{context::RepoContext, menu, theme::SharedTheme};

const MAX_COMMITS: usize = 200;
const ROW_HEIGHT: f32 = 88.0;

#[derive(Default, Clone)]
pub struct HistoryFilters {
    pub branch: String,
    pub author: String,
    pub search: String,
    pub since: String,
    pub until: String,
}

pub struct HistoryPanel {
    theme: SharedTheme,
    filters: HistoryFilters,
    branches: Vec<String>,
    commits: Vec<CommitInfo>,
    selected_commit: Option<String>,
    /// Lightweight summaries - loaded when commit is selected
    diff_summaries: Vec<FileDiffSummary>,
    /// Full diffs loaded on demand when file is expanded (keyed by file path)
    loaded_patches: HashMap<String, FileDiff>,
    last_repo: Option<String>,
    error: Option<String>,
    diff_error: Option<String>,
    pending_refresh: bool,
}

impl HistoryPanel {
    pub fn new(theme: SharedTheme) -> Self {
        Self {
            theme,
            filters: HistoryFilters::default(),
            branches: Vec::new(),
            commits: Vec::new(),
            selected_commit: None,
            diff_summaries: Vec::new(),
            loaded_patches: HashMap::new(),
            last_repo: None,
            error: None,
            diff_error: None,
            pending_refresh: false,
        }
    }

    pub fn set_theme(&mut self, theme: SharedTheme) {
        self.theme = theme;
    }

    pub fn set_branch_filter(&mut self, branch: String, repo: Option<&RepoContext>) {
        self.filters.branch = branch;
        self.selected_commit = None;
        if let Some(repo) = repo {
            self.refresh(repo);
            self.pending_refresh = false;
        } else {
            self.pending_refresh = true;
        }
    }

    pub fn ui(&mut self, ui: &mut Ui, repo: Option<&RepoContext>) {
        ui.add_space(8.0);
        ui.heading(RichText::new("Commit history").color(self.theme.palette.text_primary));
        ui.label(
            RichText::new("Explore commits, filter by branch or author, and inspect diffs.")
                .color(self.theme.palette.text_secondary),
        );
        ui.add_space(8.0);

        if let Some(repo) = repo {
            if self.last_repo.as_deref() != Some(&repo.path) {
                self.refresh(repo);
            }
            if self.pending_refresh {
                self.refresh(repo);
                self.pending_refresh = false;
            }

            if let Some(error) = &self.error {
                ui.colored_label(self.theme.palette.accent, error);
                return;
            }

            self.filters_ui(ui, repo);
            ui.add_space(8.0);
            ui.separator();
            ui.add_space(6.0);

            let available_height = ui.available_height();
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.set_min_height(available_height);
                    ui.set_width(ui.available_width() * 0.55);
                    self.commit_list(ui);
                });

                ui.separator();

                ui.vertical(|ui| {
                    ui.set_min_height(available_height);
                    ui.set_width(ui.available_width());
                    self.details_pane(ui);
                });
            });
        } else {
            ui.label(
                RichText::new("Select or clone a repository to view its commit history.")
                    .color(self.theme.palette.text_secondary),
            );
        }
    }

    fn filters_ui(&mut self, ui: &mut Ui, repo: &RepoContext) {
        if self.branches.is_empty() {
            if let Ok(branches) = list_local_branches(&repo.path) {
                self.branches = branches;
            }
        }

        egui::Frame::none()
            .fill(self.theme.palette.surface)
            .stroke(egui::Stroke::new(1.0, self.theme.palette.surface_highlight))
            .rounding(8.0)
            .inner_margin(egui::Margin::same(10.0))
            .show(ui, |ui| {
                ui.vertical(|ui| {
                    ui.heading(RichText::new("Filters").color(self.theme.palette.text_primary));
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Branch").color(self.theme.palette.text_secondary));
                        let icon_id = ui.make_persistent_id("history-branch-icon");
                        egui::ComboBox::from_id_source("branch_filter")
                            .selected_text(if self.filters.branch.is_empty() {
                                "All"
                            } else {
                                &self.filters.branch
                            })
                            .icon(menu::combo_icon(Arc::clone(&self.theme), icon_id))
                            .show_ui(ui, |ui| {
                                menu::with_menu_popup_motion(ui, "branch-filter-menu", |ui| {
                                    if menu::menu_item(
                                        ui,
                                        &self.theme,
                                        "branch-filter-all",
                                        "All",
                                        self.filters.branch.is_empty(),
                                    )
                                    .clicked()
                                    {
                                        self.filters.branch.clear();
                                    }
                                    for branch in &self.branches {
                                        if menu::menu_item(
                                            ui,
                                            &self.theme,
                                            ("branch-filter-item", branch),
                                            branch,
                                            self.filters.branch == *branch,
                                        )
                                        .clicked()
                                        {
                                            self.filters.branch = branch.clone();
                                        }
                                    }
                                });
                            });

                        ui.label(RichText::new("Author").color(self.theme.palette.text_secondary));
                        ui.add(
                            egui::TextEdit::singleline(&mut self.filters.author)
                                .hint_text("name or email"),
                        );
                    });

                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Search").color(self.theme.palette.text_secondary));
                        ui.add(
                            egui::TextEdit::singleline(&mut self.filters.search)
                                .hint_text("message contains"),
                        );
                        ui.label(RichText::new("Since").color(self.theme.palette.text_secondary));
                        ui.add(
                            egui::TextEdit::singleline(&mut self.filters.since)
                                .hint_text("YYYY-MM-DD"),
                        );
                        ui.label(RichText::new("Until").color(self.theme.palette.text_secondary));
                        ui.add(
                            egui::TextEdit::singleline(&mut self.filters.until)
                                .hint_text("YYYY-MM-DD"),
                        );
                        if ui.button("Apply filters").clicked() {
                            self.refresh(repo);
                        }
                    });
                });
            });
    }

    fn commit_list(&mut self, ui: &mut Ui) {
        let palette = self.theme.palette.clone();
        let mut newly_selected: Option<String> = None;
        if self.commits.is_empty() {
            ui.label(
                RichText::new("No commits match the current filters.")
                    .color(palette.text_secondary),
            );
            return;
        }

        let available_height = ui.available_height();
        egui::ScrollArea::vertical()
            .id_source("history_commit_list")
            .auto_shrink([false, false])
            .max_height(available_height)
            .show(ui, |ui| {
                for (idx, commit) in self.commits.iter().enumerate() {
                    let is_selected = self
                        .selected_commit
                        .as_deref()
                        .map(|id| id == commit.id)
                        .unwrap_or(false);

                    let bg_color = if is_selected {
                        palette.surface
                    } else {
                        palette.background
                    };

                    let stroke = egui::Stroke::new(2.0, palette.surface_highlight);
                    let frame = egui::Frame::none()
                        .fill(bg_color)
                        .stroke(stroke)
                        .rounding(6.0)
                        .inner_margin(egui::Margin::symmetric(12.0, 8.0));
                    let response = frame
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.set_min_height(ROW_HEIGHT);
                            ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                                ui.vertical(|ui| {
                                    ui.horizontal_wrapped(|ui| {
                                        ui.add(
                                            egui::Label::new(
                                                RichText::new(&commit.summary)
                                                    .color(palette.text_primary)
                                                    .strong(),
                                            )
                                            .wrap(true),
                                        );
                                        ui.add(
                                            egui::Label::new(
                                                RichText::new(&commit.short_id)
                                                    .color(palette.text_secondary),
                                            )
                                            .wrap(true),
                                        );
                                    });
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(format!("{}", commit.author))
                                                .color(palette.text_secondary),
                                        )
                                        .wrap(true),
                                    );
                                    let date = chrono::DateTime::<Utc>::from_timestamp(
                                        commit.time.seconds(),
                                        0,
                                    )
                                    .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
                                    .unwrap_or_else(|| "Unknown time".to_string());
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(date).color(palette.text_secondary),
                                        )
                                        .wrap(true),
                                    );
                                });
                            });
                        })
                        .response
                        .interact(Sense::click());

                    self.paint_graph(ui, response.rect, idx, commit.parents.len() > 1);

                    if response.clicked() {
                        newly_selected = Some(commit.id.clone());
                    }
                }
            });

        if let Some(selected) = newly_selected {
            self.selected_commit = Some(selected);
            self.load_diff();
        }
    }

    fn paint_graph(&self, ui: &mut Ui, rect: egui::Rect, index: usize, is_merge: bool) {
        let palette = self.theme.palette.clone();
        let painter = ui.painter();
        let x = rect.left() + 18.0;
        let center = Pos2::new(x, rect.center().y);

        if index > 0 {
            painter.line_segment(
                [Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())],
                egui::Stroke::new(2.0, palette.surface_highlight),
            );
        }

        if index + 1 < self.commits.len() {
            painter.line_segment(
                [Pos2::new(x, rect.center().y), Pos2::new(x, rect.bottom())],
                egui::Stroke::new(2.0, palette.surface_highlight),
            );
        }

        let radius = if is_merge { 8.0 } else { 6.5 };
        painter.circle_filled(center, radius, palette.accent);
        if is_merge {
            painter.circle_stroke(center, radius + 4.0, egui::Stroke::new(1.5, palette.accent));
        }
    }

    fn details_pane(&mut self, ui: &mut Ui) {
        ui.heading(RichText::new("Details").color(self.theme.palette.text_primary));
        ui.add_space(6.0);
        if let Some(id) = &self.selected_commit {
            if let Some(commit) = self.commits.iter().find(|c| &c.id == id) {
                ui.label(
                    RichText::new(&commit.summary)
                        .color(self.theme.palette.text_primary)
                        .strong(),
                );
                let full_message = commit.message.trim();
                let summary_trimmed = commit.summary.trim();
                let message_body = if full_message.starts_with(summary_trimmed) {
                    full_message[summary_trimmed.len()..].trim_start()
                } else {
                    full_message
                };
                if !message_body.is_empty() {
                    ui.add(
                        egui::Label::new(
                            RichText::new(message_body).color(self.theme.palette.text_secondary),
                        )
                        .wrap(true),
                    );
                }
                if let (Some(files), Some(additions), Some(deletions)) =
                    (commit.files_changed, commit.additions, commit.deletions)
                {
                    ui.label(
                        RichText::new(format!(
                            "Files changed: {files} (+{additions}, -{deletions})"
                        ))
                        .color(self.theme.palette.text_secondary),
                    );
                }
                ui.add_space(8.0);
                ui.separator();
                ui.add_space(8.0);
                ui.heading(RichText::new("Files changed").color(self.theme.palette.text_primary));
                if let Some(error) = &self.diff_error {
                    ui.colored_label(self.theme.palette.accent, error);
                }
                if let Some(email) = &commit.email {
                    ui.add(
                        egui::Label::new(
                            RichText::new(email).color(self.theme.palette.text_secondary),
                        )
                        .wrap(true),
                    );
                    ui.add_space(6.0);
                }
                let diff_height = ui.available_height().max(220.0);
                // Collect file paths that need patch loading
                let mut files_to_load: Vec<String> = Vec::new();

                egui::ScrollArea::vertical()
                    .id_source("history_details_files")
                    .auto_shrink([false, false])
                    .min_scrolled_height(diff_height)
                    .show(ui, |ui| {
                        for (idx, summary) in self.diff_summaries.iter().enumerate() {
                            let file_path = summary.path.clone();
                            ui.push_id(idx, |ui| {
                                let header_text = if summary.is_binary {
                                    format!("{} (binary)", summary.path)
                                } else {
                                    format!(
                                        "{} (+{}, -{})",
                                        summary.path, summary.additions, summary.deletions
                                    )
                                };

                                let header_id = ui.make_persistent_id(("diff_file", &file_path));
                                let state = egui::collapsing_header::CollapsingState::load_with_default_open(
                                    ui.ctx(),
                                    header_id,
                                    false,
                                );

                                let is_open = state.is_open();

                                // If expanded and not yet loaded, mark for loading
                                if is_open && !self.loaded_patches.contains_key(&file_path) {
                                    files_to_load.push(file_path.clone());
                                }

                                state
                                    .show_header(ui, |ui| {
                                        ui.label(
                                            RichText::new(header_text)
                                                .color(self.theme.palette.text_primary),
                                        );
                                    })
                                    .body(|ui| {
                                        if let Some(diff) = self.loaded_patches.get(&file_path) {
                                            let mut patch_text = diff.patch.clone();
                                            if diff.truncated {
                                                ui.label(
                                                    RichText::new("⚠ File truncated (too large)")
                                                        .color(self.theme.palette.accent)
                                                        .small(),
                                                );
                                            }
                                            ui.add(
                                                egui::TextEdit::multiline(&mut patch_text)
                                                    .font(egui::TextStyle::Monospace)
                                                    .desired_width(f32::INFINITY)
                                                    .interactive(false),
                                            );
                                        } else {
                                            ui.label(
                                                RichText::new("Loading...")
                                                    .color(self.theme.palette.text_secondary),
                                            );
                                        }
                                    });
                                ui.add_space(6.0);
                            });
                        }
                    });

                // Load patches for expanded files (outside the UI loop)
                for file_path in files_to_load {
                    self.load_file_patch(&file_path);
                }
            } else {
                ui.label(
                    RichText::new("Commit not found.").color(self.theme.palette.text_secondary),
                );
            }
        } else {
            ui.label(
                RichText::new("Select a commit from the list to see its details and diff.")
                    .color(self.theme.palette.text_secondary),
            );
        }
    }

    fn refresh(&mut self, repo: &RepoContext) {
        self.error = None;
        self.diff_error = None;
        self.last_repo = Some(repo.path.clone());
        self.selected_commit = None;
        self.diff_summaries.clear();
        self.loaded_patches.clear();

        let filter = CommitFilter {
            branch: if self.filters.branch.is_empty() {
                None
            } else {
                Some(self.filters.branch.clone())
            },
            author: if self.filters.author.is_empty() {
                None
            } else {
                Some(self.filters.author.clone())
            },
            search: if self.filters.search.is_empty() {
                None
            } else {
                Some(self.filters.search.clone())
            },
            since: parse_date(&self.filters.since),
            until: parse_date(&self.filters.until),
        };

        match read_commit_log(&repo.path, &filter, MAX_COMMITS, false) {
            Ok(commits) => self.commits = commits,
            Err(err) => self.error = Some(format!("Failed to read commits: {err}")),
        }
    }

    fn load_diff(&mut self) {
        // Clear previously loaded patches when selecting a new commit
        self.loaded_patches.clear();

        if let Some(repo) = self.last_repo.clone() {
            if let Some(commit) = &self.selected_commit {
                // Load only summaries - patches will be loaded on demand
                match commit_diff_summaries(&repo, commit) {
                    Ok(summaries) => {
                        self.diff_summaries = summaries;
                        self.diff_error = None;
                    }
                    Err(err) => {
                        self.diff_summaries.clear();
                        self.diff_error = Some(format!("Failed to load diff: {err}"));
                    }
                }
            }
        }
    }

    /// Load a file's patch on demand (lazy loading)
    fn load_file_patch(&mut self, file_path: &str) {
        if self.loaded_patches.contains_key(file_path) {
            return; // Already loaded
        }

        if let (Some(repo), Some(commit)) = (self.last_repo.as_ref(), self.selected_commit.as_ref()) {
            match commit_diff_file(repo, commit, file_path) {
                Ok(Some(diff)) => {
                    self.loaded_patches.insert(file_path.to_string(), diff);
                }
                Ok(None) => {
                    // File not found in diff, insert empty placeholder
                    self.loaded_patches.insert(file_path.to_string(), FileDiff {
                        path: file_path.to_string(),
                        additions: 0,
                        deletions: 0,
                        patch: String::from("(no changes)"),
                        truncated: false,
                    });
                }
                Err(err) => {
                    self.loaded_patches.insert(file_path.to_string(), FileDiff {
                        path: file_path.to_string(),
                        additions: 0,
                        deletions: 0,
                        patch: format!("Error loading diff: {err}"),
                        truncated: false,
                    });
                }
            }
        }
    }
}

fn parse_date(input: &str) -> Option<i64> {
    if input.trim().is_empty() {
        return None;
    }

    NaiveDate::parse_from_str(input.trim(), "%Y-%m-%d")
        .ok()
        .and_then(|date| {
            Utc.with_ymd_and_hms(date.year(), date.month(), date.day(), 0, 0, 0)
                .earliest()
        })
        .map(|dt| dt.timestamp())
}
