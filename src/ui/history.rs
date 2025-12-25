use chrono::{Datelike, NaiveDate, TimeZone, Utc};
use eframe::egui::{self, Align, Layout, Pos2, RichText, Sense, Ui, Vec2};

use crate::git::{
    diff::{FileDiff, commit_diff},
    log::{CommitFilter, CommitInfo, list_local_branches, read_commit_log},
};
use crate::ui::{context::RepoContext, theme::Theme};

const MAX_COMMITS: usize = 200;
const ROW_HEIGHT: f32 = 72.0;

#[derive(Default, Clone)]
pub struct HistoryFilters {
    pub branch: String,
    pub author: String,
    pub search: String,
    pub since: String,
    pub until: String,
}

pub struct HistoryPanel {
    theme: Theme,
    filters: HistoryFilters,
    branches: Vec<String>,
    commits: Vec<CommitInfo>,
    selected_commit: Option<String>,
    diffs: Vec<FileDiff>,
    last_repo: Option<String>,
    error: Option<String>,
    diff_error: Option<String>,
}

impl HistoryPanel {
    pub fn new(theme: Theme) -> Self {
        Self {
            theme,
            filters: HistoryFilters::default(),
            branches: Vec::new(),
            commits: Vec::new(),
            selected_commit: None,
            diffs: Vec::new(),
            last_repo: None,
            error: None,
            diff_error: None,
        }
    }

    pub fn set_theme(&mut self, theme: Theme) {
        self.theme = theme;
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

            if let Some(error) = &self.error {
                ui.colored_label(self.theme.palette.accent, error);
                return;
            }

            self.filters_ui(ui, repo);
            ui.add_space(8.0);
            ui.separator();
            ui.add_space(6.0);

            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(ui.available_width() * 0.55);
                    self.commit_list(ui);
                });

                ui.separator();

                ui.vertical(|ui| {
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
                        egui::ComboBox::from_id_source("branch_filter")
                            .selected_text(if self.filters.branch.is_empty() {
                                "All"
                            } else {
                                &self.filters.branch
                            })
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut self.filters.branch, String::new(), "All");
                                for branch in &self.branches {
                                    ui.selectable_value(
                                        &mut self.filters.branch,
                                        branch.clone(),
                                        branch,
                                    );
                                }
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

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for (idx, commit) in self.commits.iter().enumerate() {
                    let is_selected = self
                        .selected_commit
                        .as_deref()
                        .map(|id| id == commit.id)
                        .unwrap_or(false);

                    let (rect, response) = ui.allocate_exact_size(
                        Vec2::new(ui.available_width(), ROW_HEIGHT),
                        Sense::click(),
                    );

                    let stroke = egui::Stroke::new(2.0, palette.surface_highlight);
                    let bg_color = if is_selected {
                        palette.surface
                    } else {
                        palette.background
                    };
                    ui.painter().rect(rect, 6.0, bg_color, stroke);

                    self.paint_graph(ui, rect, idx, commit.parents.len() > 1);

                    let inner = rect.shrink2(Vec2::new(12.0, 8.0));
                    let mut content = ui.child_ui(inner, Layout::left_to_right(Align::Min));
                    content.vertical(|ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(&commit.summary)
                                    .color(palette.text_primary)
                                    .strong(),
                            );
                            ui.label(
                                RichText::new(format!(
                                    "{}",
                                    commit.id.chars().take(8).collect::<String>()
                                ))
                                .color(palette.text_secondary),
                            );
                        });
                        ui.label(
                            RichText::new(format!("{}", commit.author))
                                .color(palette.text_secondary),
                        );
                        let date =
                            chrono::DateTime::<Utc>::from_timestamp(commit.time.seconds(), 0)
                                .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
                                .unwrap_or_else(|| "Unknown time".to_string());
                        ui.label(RichText::new(date).color(palette.text_secondary));
                    });

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
                ui.label(
                    RichText::new(commit.message.trim()).color(self.theme.palette.text_secondary),
                );
                if let (Some(files), Some(additions), Some(deletions)) = (
                    commit.files_changed,
                    commit.additions,
                    commit.deletions,
                ) {
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
                egui::ScrollArea::vertical().show(ui, |ui| {
                    if let Some(email) = &commit.email {
                        ui.label(RichText::new(email).color(self.theme.palette.text_secondary));
                        ui.add_space(6.0);
                    }
                    for diff in &self.diffs {
                        ui.collapsing(
                            RichText::new(format!(
                                "{} (+{}, -{})",
                                diff.path, diff.additions, diff.deletions
                            ))
                            .color(self.theme.palette.text_primary),
                            |ui| {
                                ui.add(
                                    egui::TextEdit::multiline(&mut diff.patch.clone())
                                        .font(egui::TextStyle::Monospace)
                                        .desired_width(f32::INFINITY)
                                        .interactive(false),
                                );
                            },
                        );
                        ui.add_space(6.0);
                    }
                });
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
        self.diffs.clear();

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
        if let Some(repo) = self.last_repo.clone() {
            if let Some(commit) = &self.selected_commit {
                match commit_diff(&repo, commit) {
                    Ok(diffs) => {
                        self.diffs = diffs;
                        self.diff_error = None;
                    }
                    Err(err) => {
                        self.diffs.clear();
                        self.diff_error = Some(format!("Failed to load diff: {err}"));
                    }
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
