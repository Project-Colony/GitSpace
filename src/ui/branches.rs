use std::collections::BTreeMap;

use eframe::egui::{self, RichText, Sense, Ui};

use crate::git::branch::{
    BranchEntry, BranchKind, checkout_branch, create_branch, delete_branch, list_branches,
    rename_branch,
};
use crate::git::merge::{MergeOutcome, MergeStrategy, detect_conflicts, merge_branch};
use crate::ui::{context::RepoContext, theme::Theme};

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
    new_branch: String,
    rename_buffer: String,
    last_repo: Option<String>,
    error: Option<String>,
    status: Option<String>,
    conflict_files: Vec<String>,
}

impl BranchPanel {
    pub fn new(theme: Theme) -> Self {
        Self {
            theme,
            branches: Vec::new(),
            new_branch: String::new(),
            rename_buffer: String::new(),
            last_repo: None,
            error: None,
            status: None,
            conflict_files: Vec::new(),
        }
    }

    pub fn set_theme(&mut self, theme: Theme) {
        self.theme = theme;
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
                ui.vertical(|ui| {
                    ui.set_width(ui.available_width() * 0.5);
                    self.render_tree(ui, repo, BranchKind::Local, "Local branches");
                });

                ui.separator();

                ui.vertical(|ui| {
                    ui.set_width(ui.available_width());
                    self.render_tree(ui, repo, BranchKind::Remote, "Remote branches");
                });
            });
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
        }

        match list_branches(&repo.path) {
            Ok(branches) => self.branches = branches,
            Err(err) => {
                self.error = Some(format!("Failed to read branches: {err}"));
                return;
            }
        }

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
            let segments: Vec<&str> = branch.name.split('/').collect();
            root.insert(&segments, branch.clone());
        }

        if root.children.is_empty() {
            ui.label(RichText::new("No branches found.").color(self.theme.palette.text_secondary));
            return;
        }

        egui::ScrollArea::vertical()
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
                let label = self.branch_label(branch);
                let response = ui.add(egui::Label::new(label).sense(Sense::click()));
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

    fn branch_label(&self, branch: &BranchEntry) -> RichText {
        let mut text = branch.name.clone();
        if branch.is_head {
            text.push_str(" (HEAD)");
        }
        RichText::new(text)
            .color(if branch.is_head {
                self.theme.palette.accent
            } else {
                self.theme.palette.text_primary
            })
            .strong()
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

            if branch.kind == BranchKind::Local && !branch.is_head {
                if ui.button("Delete").clicked() {
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

            if branch.kind == BranchKind::Local {
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
}
