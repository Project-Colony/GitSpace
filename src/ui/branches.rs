use std::collections::BTreeMap;

use chrono::Utc;
use eframe::egui::{self, RichText, Sense, Ui};

use crate::git::branch::{
    BranchEntry, BranchKind, archive_branch, checkout_branch, create_branch,
    create_tracking_branch, delete_branch, list_branches, rename_branch,
};
use crate::git::compare::{BranchComparison, DiffSummary, compare_branch_with_head};
use crate::git::log::{CommitInfo, commits_between_refs, latest_commit_for_branch};
use crate::git::merge::{MergeOutcome, MergeStrategy, detect_conflicts, merge_branch};
use crate::ui::{context::RepoContext, theme::Theme};

const STALE_DAYS: i64 = 30;

#[derive(Default)]
struct BranchNode {
    label: String,
    children: BTreeMap<String, BranchNode>,
    branch: Option<BranchEntry>,
}

impl BranchNode {
    fn insert(&mut self, segments: &[&str], branch: BranchEntry) {
        if segments.is_empty() {
            self.branch = Some(branch);
            return;
        }

        let (first, rest) = segments.split_first().unwrap();
        let node = self.children.entry(first.to_string()).or_default();
        node.label = first.to_string();
        node.insert(rest, branch);
    }
}

pub struct BranchPanel {
    theme: Theme,
    branches: Vec<BranchEntry>,
    branch_commits: BTreeMap<String, CommitInfo>,
    new_branch: String,
    rename_buffer: String,
    last_repo: Option<String>,
    selected_branch: Option<String>,
    selected_comparison: Option<BranchComparison>,
    selected_error: Option<String>,
    compare_branch: Option<String>,
    compare_commits: Vec<CommitInfo>,
    compare_diff: Option<DiffSummary>,
    compare_error: Option<String>,
    error: Option<String>,
    status: Option<String>,
    conflict_files: Vec<String>,
    stale_only: bool,
    open_history_branch: Option<String>,
}

impl BranchPanel {
    pub fn new(theme: Theme) -> Self {
        Self {
            theme,
            branches: Vec::new(),
            branch_commits: BTreeMap::new(),
            new_branch: String::new(),
            rename_buffer: String::new(),
            last_repo: None,
            selected_branch: None,
            selected_comparison: None,
            selected_error: None,
            compare_branch: None,
            compare_commits: Vec::new(),
            compare_diff: None,
            compare_error: None,
            error: None,
            status: None,
            conflict_files: Vec::new(),
            stale_only: false,
            open_history_branch: None,
        }
    }

    pub fn set_theme(&mut self, theme: Theme) {
        self.theme = theme;
    }

    pub fn take_history_request(&mut self) -> Option<String> {
        self.open_history_branch.take()
    }

    pub fn ui(&mut self, ui: &mut Ui, repo: Option<&RepoContext>) {
        ui.add_space(8.0);
        ui.heading(RichText::new("Branch explorer").color(self.theme.palette.text_primary));
        ui.label(
            RichText::new(
                "Navigate branches, manage them, and merge or rebase with context menus.",
            )
            .color(self.theme.palette.text_secondary),
        );

        if let Some(repo) = repo {
            self.refresh(repo);
            if let Some(error) = &self.error {
                ui.colored_label(self.theme.palette.accent, error);
                return;
            }

            if !self.conflict_files.is_empty() {
                ui.add_space(6.0);
                ui.colored_label(
                    self.theme.palette.accent,
                    format!(
                        "Merge conflicts detected in: {}",
                        self.conflict_files.join(", ")
                    ),
                );
            }

            if let Some(status) = &self.status {
                ui.add_space(6.0);
                ui.label(RichText::new(status).color(self.theme.palette.text_secondary));
            }

            ui.add_space(6.0);
            self.creation_bar(ui, repo);
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.checkbox(&mut self.stale_only, "Show stale only");
            });
            ui.add_space(8.0);

            let available_height = ui.available_height();
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.set_min_height(available_height);
                    ui.set_width(ui.available_width() * 0.5);
                    self.render_tree(ui, repo, BranchKind::Local, "Local branches");
                });

                ui.separator();

                ui.vertical(|ui| {
                    ui.set_min_height(available_height);
                    ui.set_width(ui.available_width());
                    self.render_tree(ui, repo, BranchKind::Remote, "Remote branches");
                });
            });

            ui.add_space(10.0);
            self.render_selection_panel(ui);
            ui.add_space(10.0);
            self.render_compare_panel(ui);
        } else {
            ui.add_space(8.0);
            ui.label(
                RichText::new("Open a repository to browse and manage its branches.")
                    .color(self.theme.palette.text_secondary),
            );
        }
    }

    fn creation_bar(&mut self, ui: &mut Ui, repo: &RepoContext) {
        egui::Frame::none()
            .fill(self.theme.palette.surface)
            .stroke(egui::Stroke::new(1.0, self.theme.palette.surface_highlight))
            .rounding(6.0)
            .inner_margin(egui::Margin::same(8.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("New branch").color(self.theme.palette.text_secondary));
                    ui.add(
                        egui::TextEdit::singleline(&mut self.new_branch)
                            .hint_text("feature/my-branch"),
                    );
                    if ui.button("Create").clicked() {
                        self.status = None;
                        self.error = None;
                        if self.new_branch.trim().is_empty() {
                            self.error = Some("Branch name cannot be empty".to_string());
                        } else {
                            match create_branch(&repo.path, self.new_branch.trim(), None) {
                                Ok(_) => {
                                    self.status = Some(format!(
                                        "Created branch {} from HEAD",
                                        self.new_branch.trim()
                                    ));
                                    self.new_branch.clear();
                                    self.refresh(repo);
                                }
                                Err(err) => {
                                    self.error = Some(format!("Failed to create branch: {err}"))
                                }
                            }
                        }
                    }
                });
            });
    }

    fn refresh(&mut self, repo: &RepoContext) {
        if self.last_repo.as_deref() != Some(&repo.path) {
            self.branches.clear();
            self.status = None;
            self.error = None;
            self.last_repo = Some(repo.path.clone());
            self.selected_branch = None;
            self.selected_comparison = None;
            self.selected_error = None;
            self.compare_branch = None;
            self.compare_commits.clear();
            self.compare_diff = None;
            self.compare_error = None;
        }

        match list_branches(&repo.path) {
            Ok(branches) => self.branches = branches,
            Err(err) => {
                self.error = Some(format!("Failed to read branches: {err}"));
                return;
            }
        }
        self.refresh_branch_commits(repo);

        match detect_conflicts(&repo.path) {
            Ok(conflicts) => self.conflict_files = conflicts,
            Err(err) => self.error = Some(format!("Failed to detect conflicts: {err}")),
        }
    }

    fn render_tree(&mut self, ui: &mut Ui, repo: &RepoContext, kind: BranchKind, label: &str) {
        ui.heading(RichText::new(label).color(self.theme.palette.text_primary));
        ui.add_space(4.0);

        let mut root = BranchNode::default();
        for branch in self.branches.iter().filter(|b| b.kind == kind) {
            if self.stale_only && !self.is_branch_stale(branch) {
                continue;
            }
            let segments: Vec<&str> = branch.name.split('/').collect();
            root.insert(&segments, branch.clone());
        }

        if root.children.is_empty() {
            ui.label(RichText::new("No branches found.").color(self.theme.palette.text_secondary));
            return;
        }

        let kind_id = match kind {
            BranchKind::Local => "local",
            BranchKind::Remote => "remote",
        };
        egui::ScrollArea::vertical()
            .id_source(("branch_scroll", kind_id))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for node in root.children.values() {
                    self.render_node(ui, repo, node, 0);
                }
            });
    }

    fn render_node(&mut self, ui: &mut Ui, repo: &RepoContext, node: &BranchNode, depth: usize) {
        let mut render_node_contents = |ui: &mut Ui| {
            if !node.children.is_empty() {
                egui::collapsing_header::CollapsingState::load_with_default_open(
                    ui.ctx(),
                    ui.make_persistent_id((&node.label, depth)),
                    true,
                )
                .show_header(ui, |ui| {
                    ui.label(RichText::new(&node.label).color(self.theme.palette.text_secondary));
                })
                .body(|ui| {
                    for child in node.children.values() {
                        self.render_node(ui, repo, child, depth + 1);
                    }
                });
            } else if let Some(branch) = &node.branch {
                let response = self.branch_label(ui, branch);
                if response.clicked() {
                    self.select_branch(repo, &branch.name);
                }
                self.context_menu(repo, branch, &response);
                if let Some(upstream) = &branch.upstream {
                    response.on_hover_text(format!("Upstream: {upstream}"));
                }
            }
        };

        if depth == 0 {
            render_node_contents(ui);
        } else {
            ui.indent(
                ui.make_persistent_id(("branch_indent", depth, &node.label)),
                |ui| {
                    render_node_contents(ui);
                },
            );
        }
    }

    fn branch_label(&self, ui: &mut Ui, branch: &BranchEntry) -> egui::Response {
        let mut text = branch.name.clone();
        if branch.is_head {
            text.push_str(" (HEAD)");
        }
        let label = RichText::new(text)
            .color(if branch.is_head {
                self.theme.palette.accent
            } else {
                self.theme.palette.text_primary
            })
            .strong();
        let is_stale = self.is_branch_stale(branch);
        ui.horizontal(|ui| {
            let response = ui.add(egui::Label::new(label).sense(Sense::click()));
            if is_stale {
                egui::Frame::none()
                    .fill(self.theme.palette.surface_highlight)
                    .rounding(egui::Rounding::same(6.0))
                    .inner_margin(egui::Margin::symmetric(6.0, 2.0))
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new("stale")
                                .color(self.theme.palette.text_secondary)
                                .size(self.theme.typography.label),
                        );
                    });
            }
            response
        })
        .inner
    }

    fn context_menu(
        &mut self,
        repo: &RepoContext,
        branch: &BranchEntry,
        response: &egui::Response,
    ) {
        response.context_menu(|ui| {
            if ui.button("Checkout").clicked() {
                self.run_branch_action(repo, || checkout_branch(&repo.path, &branch.name));
                ui.close_menu();
            }

            if branch.kind == BranchKind::Remote {
                if ui.button("Checkout & Track").clicked() {
                    self.run_branch_action(repo, || {
                        let local_name = create_tracking_branch(&repo.path, &branch.name)?;
                        checkout_branch(&repo.path, &local_name)?;
                        Ok(())
                    });
                    ui.close_menu();
                }
            }

            if branch.kind == BranchKind::Local && !branch.is_head {
                if ui.button("Delete branch").clicked() {
                    self.run_branch_action(repo, || delete_branch(&repo.path, &branch.name));
                    ui.close_menu();
                }
            }

            if ui.button("Merge into current").clicked() {
                self.run_merge_action(repo, &branch.name, MergeStrategy::Merge);
                ui.close_menu();
            }

            if ui.button("Rebase onto current").clicked() {
                self.run_merge_action(repo, &branch.name, MergeStrategy::Rebase);
                ui.close_menu();
            }

            if ui.button("Compare with current").clicked() {
                self.compare_with_current(repo, &branch.name);
                ui.close_menu();
            }

            if ui.button("Open in History").clicked() {
                self.open_history_branch = Some(branch.name.clone());
                ui.close_menu();
            }

            if branch.kind == BranchKind::Local {
                if ui.button("Archive").clicked() {
                    self.status = None;
                    self.error = None;
                    match archive_branch(&repo.path, &branch.name) {
                        Ok(tag) => {
                            self.status = Some(format!("Archived {} as tag {}", branch.name, tag));
                            self.refresh(repo);
                        }
                        Err(err) => self.error = Some(err.to_string()),
                    }
                    ui.close_menu();
                }

                ui.separator();
                if self.rename_buffer.is_empty() {
                    self.rename_buffer = branch.name.clone();
                }
                ui.horizontal(|ui| {
                    ui.label("Rename:");
                    ui.add(egui::TextEdit::singleline(&mut self.rename_buffer));
                    if ui.button("Apply").clicked() {
                        let new_name = self.rename_buffer.trim().to_string();
                        if !new_name.is_empty() {
                            self.run_branch_action(repo, || {
                                rename_branch(&repo.path, &branch.name, new_name.as_str())
                            });
                            self.rename_buffer.clear();
                            ui.close_menu();
                        }
                    }
                });
            }
        });
    }

    fn run_branch_action<F>(&mut self, repo: &RepoContext, action: F)
    where
        F: FnOnce() -> Result<(), git2::Error>,
    {
        self.status = None;
        self.error = None;
        match action() {
            Ok(_) => {
                self.status = Some("Operation completed".to_string());
                self.refresh(repo);
            }
            Err(err) => self.error = Some(err.to_string()),
        }
    }

    fn run_merge_action(&mut self, repo: &RepoContext, branch: &str, strategy: MergeStrategy) {
        self.status = None;
        self.error = None;
        match merge_branch(&repo.path, branch, strategy) {
            Ok(outcome) => self.handle_merge_outcome(repo, outcome),
            Err(err) => self.error = Some(err),
        }
    }

    fn select_branch(&mut self, repo: &RepoContext, branch_name: &str) {
        self.selected_branch = Some(branch_name.to_string());
        self.selected_error = None;
        match compare_branch_with_head(&repo.path, branch_name) {
            Ok(comparison) => self.selected_comparison = Some(comparison),
            Err(err) => {
                self.selected_comparison = None;
                self.selected_error = Some(format!("Failed to compare branch: {err}"));
            }
        }
    }

    fn render_selection_panel(&self, ui: &mut Ui) {
        ui.heading(RichText::new("Selection details").color(self.theme.palette.text_primary));
        ui.add_space(6.0);

        let Some(branch_name) = &self.selected_branch else {
            ui.label(
                RichText::new("Select a branch to see its latest commit and comparison details.")
                    .color(self.theme.palette.text_secondary),
            );
            return;
        };

        if let Some(error) = &self.selected_error {
            ui.colored_label(self.theme.palette.accent, error);
            return;
        }

        let Some(comparison) = &self.selected_comparison else {
            ui.label(
                RichText::new("No comparison data available yet.")
                    .color(self.theme.palette.text_secondary),
            );
            return;
        };

        ui.label(
            RichText::new(branch_name)
                .color(self.theme.palette.text_primary)
                .strong(),
        );

        if let Some(commit) = &comparison.commit {
            ui.add_space(4.0);
            ui.label(
                RichText::new(commit.summary.clone())
                    .color(self.theme.palette.text_primary)
                    .strong(),
            );
            ui.label(
                RichText::new(format!("Author: {}", commit.author))
                    .color(self.theme.palette.text_secondary),
            );
            let date = chrono::DateTime::<Utc>::from_timestamp(commit.time.seconds(), 0)
                .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
                .unwrap_or_else(|| "Unknown time".to_string());
            ui.label(RichText::new(date).color(self.theme.palette.text_secondary));
        } else {
            ui.label(
                RichText::new("No commits found for this branch.")
                    .color(self.theme.palette.text_secondary),
            );
        }

        ui.add_space(8.0);
        ui.label(
            RichText::new("Comparison with current HEAD")
                .color(self.theme.palette.text_primary)
                .strong(),
        );
        if let Some(diff) = &comparison.diff {
            ui.label(
                RichText::new(format!(
                    "{} files changed • +{} / -{}",
                    diff.files_changed, diff.additions, diff.deletions
                ))
                .color(self.theme.palette.text_secondary),
            );
        } else {
            ui.label(RichText::new("No diff available.").color(self.theme.palette.text_secondary));
        }
    }

    fn render_compare_panel(&self, ui: &mut Ui) {
        ui.heading(RichText::new("Compare with current").color(self.theme.palette.text_primary));
        ui.add_space(6.0);

        let Some(branch_name) = &self.compare_branch else {
            ui.label(
                RichText::new("Use the branch context menu to compare with current HEAD.")
                    .color(self.theme.palette.text_secondary),
            );
            return;
        };

        if let Some(error) = &self.compare_error {
            ui.colored_label(self.theme.palette.accent, error);
            return;
        }

        ui.label(
            RichText::new(branch_name)
                .color(self.theme.palette.text_primary)
                .strong(),
        );

        if let Some(diff) = &self.compare_diff {
            ui.add_space(4.0);
            ui.label(
                RichText::new(format!(
                    "{} files changed • +{} / -{}",
                    diff.files_changed, diff.additions, diff.deletions
                ))
                .color(self.theme.palette.text_secondary),
            );
        }

        ui.add_space(6.0);
        ui.label(
            RichText::new("Commits between current HEAD and branch")
                .color(self.theme.palette.text_primary)
                .strong(),
        );

        if self.compare_commits.is_empty() {
            ui.label(
                RichText::new("No commits found in the selected range.")
                    .color(self.theme.palette.text_secondary),
            );
            return;
        }

        egui::ScrollArea::vertical()
            .id_source("compare_commits")
            .auto_shrink([false, false])
            .max_height(220.0)
            .show(ui, |ui| {
                for commit in &self.compare_commits {
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new(commit.summary.clone())
                                .color(self.theme.palette.text_primary)
                                .strong(),
                        );
                        ui.label(
                            RichText::new(format!(
                                "{} • {}",
                                self.short_commit_id(&commit.id),
                                commit.author
                            ))
                            .color(self.theme.palette.text_secondary),
                        );
                        ui.add_space(4.0);
                    });
                    ui.separator();
                }
            });
    }

    fn handle_merge_outcome(&mut self, repo: &RepoContext, outcome: MergeOutcome) {
        if outcome.had_conflicts {
            self.conflict_files = outcome.conflicts;
            self.status = Some(
                "Conflicts detected. Resolve them in your working tree before continuing."
                    .to_string(),
            );
        } else {
            self.conflict_files.clear();
            self.status = Some(outcome.message);
        }
        self.refresh(repo);
    }

    fn refresh_branch_commits(&mut self, repo: &RepoContext) {
        self.branch_commits.clear();
        for branch in &self.branches {
            match latest_commit_for_branch(&repo.path, &branch.name) {
                Ok(Some(commit)) => {
                    self.branch_commits.insert(self.branch_key(branch), commit);
                }
                Ok(None) => {}
                Err(err) => {
                    self.error = Some(format!("Failed to read branch history: {err}"));
                    return;
                }
            }
        }
    }

    fn branch_key(&self, branch: &BranchEntry) -> String {
        match branch.kind {
            BranchKind::Local => format!("local:{}", branch.name),
            BranchKind::Remote => format!("remote:{}", branch.name),
        }
    }

    fn is_branch_stale(&self, branch: &BranchEntry) -> bool {
        let key = self.branch_key(branch);
        let Some(commit) = self.branch_commits.get(&key) else {
            return true;
        };
        let age_seconds = Utc::now().timestamp().saturating_sub(commit.time.seconds());
        age_seconds > STALE_DAYS * 24 * 60 * 60
    }

    fn compare_with_current(&mut self, repo: &RepoContext, branch_name: &str) {
        self.compare_branch = Some(branch_name.to_string());
        self.compare_error = None;
        let comparison = compare_branch_with_head(&repo.path, branch_name);
        match comparison {
            Ok(comparison) => self.compare_diff = comparison.diff,
            Err(err) => {
                self.compare_diff = None;
                self.compare_error = Some(format!("Failed to compare branch: {err}"));
            }
        }

        match commits_between_refs(&repo.path, "HEAD", branch_name, 50) {
            Ok(commits) => self.compare_commits = commits,
            Err(err) => {
                self.compare_commits.clear();
                self.compare_error = Some(format!("Failed to load comparison commits: {err}"));
            }
        }
    }

    fn short_commit_id(&self, id: &str) -> String {
        id.chars().take(7).collect()
    }
}
