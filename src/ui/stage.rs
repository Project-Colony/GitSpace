use std::collections::{BTreeSet, HashMap};
use std::path::Path;

use eframe::egui::{self, Align, ComboBox, Layout, RichText, ScrollArea, Ui, Window};
use git2::{Repository, Signature, Status, StatusOptions, StatusShow};

use crate::git::branch::restore_file_from_branch;
use crate::git::diff::{diff_file, staged_diff, working_tree_diff};
use crate::git::stash::{StashEntry, apply_stash, create_stash, drop_stash, list_stashes};
use crate::git::status::read_repo_status;
use crate::ui::{context::RepoContext, theme::Theme};

#[derive(Debug, Clone)]
struct FileEntry {
    path: String,
    status_label: String,
    diff: String,
    checked: bool,
}

pub struct StagePanel {
    theme: Theme,
    staged: Vec<FileEntry>,
    unstaged: Vec<FileEntry>,
    selected_diff: Option<(bool, String)>,
    last_repo: Option<String>,
    status: Option<String>,
    error: Option<String>,
    commit_message: String,
    include_signoff: bool,
    selected_template: usize,
    signoff_line: String,
    stash_message: String,
    stashes: Vec<StashEntry>,
    include_untracked_in_stash: bool,
    needs_refresh: bool,
    restore_dialog_open: bool,
    restore_selection: Option<String>,
}

const COMMIT_TEMPLATES: &[(&str, &str)] = &[
    ("WIP", "WIP: describe the work in progress"),
    (
        "Feature",
        "feat: short summary\n\n- describe the change\n- add context or links",
    ),
    (
        "Fix",
        "fix: bug summary\n\nExplain root cause and how it was addressed.",
    ),
];

impl StagePanel {
    pub fn new(theme: Theme) -> Self {
        let signoff_line = default_signoff_line();
        Self {
            theme,
            staged: Vec::new(),
            unstaged: Vec::new(),
            selected_diff: None,
            last_repo: None,
            status: None,
            error: None,
            commit_message: String::new(),
            include_signoff: false,
            selected_template: 0,
            signoff_line,
            stash_message: String::from("WIP changes"),
            stashes: Vec::new(),
            include_untracked_in_stash: true,
            needs_refresh: true,
            restore_dialog_open: false,
            restore_selection: None,
        }
    }

    pub fn set_theme(&mut self, theme: Theme) {
        self.theme = theme;
    }

    pub fn ui(&mut self, ui: &mut Ui, repo: Option<&RepoContext>) {
        ui.heading(RichText::new("Staging & commits").color(self.theme.palette.text_primary));
        ui.label(
            RichText::new("Review unstaged and staged changes, preview diffs, and manage commits.")
                .color(self.theme.palette.text_secondary),
        );

        if let Some(repo) = repo {
            self.refresh_if_needed(repo);

            if let Some(error) = &self.error {
                ui.colored_label(self.theme.palette.accent, error);
                return;
            }

            if let Some(status) = &self.status {
                ui.label(RichText::new(status).color(self.theme.palette.text_secondary));
            }

            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.set_height(260.0);
                ui.vertical(|ui| {
                    ui.set_width(ui.available_width() * 0.45);
                    self.render_change_list(ui, repo, false);
                });

                ui.separator();

                ui.vertical(|ui| {
                    ui.set_width(ui.available_width());
                    self.render_change_list(ui, repo, true);
                });
            });

            ui.add_space(8.0);
            self.render_diff(ui);
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                let commit_width = ui.available_width() * 0.6;
                ui.vertical(|ui| {
                    ui.set_width(commit_width);
                    self.render_commit_editor(ui);
                });

                ui.add_space(12.0);

                ui.vertical(|ui| {
                    ui.set_width(ui.available_width());
                    self.render_stash_controls(ui, repo);
                });
            });
            self.render_restore_dialog(ui, repo);
        } else {
            ui.add_space(8.0);
            ui.label(
                RichText::new("Open a repository to inspect and stage its changes.")
                    .color(self.theme.palette.text_secondary),
            );
        }
    }

    fn refresh_if_needed(&mut self, repo: &RepoContext) {
        if self.last_repo.as_deref() != Some(&repo.path) {
            self.last_repo = Some(repo.path.clone());
            self.selected_diff = None;
            self.commit_message.clear();
            self.status = None;
            self.error = None;
            self.needs_refresh = true;
        }

        if self.needs_refresh {
            match read_statuses(&repo.path) {
                Ok((staged, unstaged)) => {
                    self.staged = staged;
                    self.unstaged = unstaged;
                    self.error = None;
                }
                Err(err) => {
                    self.error = Some(format!("Failed to read changes: {err}"));
                }
            }

            match list_stashes(&repo.path) {
                Ok(entries) => self.stashes = entries,
                Err(err) => self.error = Some(format!("Failed to read stashes: {err}")),
            }

            self.needs_refresh = false;
        }
    }

    fn render_change_list(&mut self, ui: &mut Ui, repo: &RepoContext, staged: bool) {
        let title = if staged { "Staged" } else { "Unstaged" };
        let list = if staged {
            &mut self.staged
        } else {
            &mut self.unstaged
        };

        ui.heading(RichText::new(format!("{title} files")).color(self.theme.palette.text_primary));
        ui.add_space(4.0);

        if list.is_empty() {
            ui.label(
                RichText::new("No files in this section.").color(self.theme.palette.text_secondary),
            );
            return;
        }

        let mut pending_action: Option<(bool, String)> = None;
        let mut pending_diff: Option<(bool, String)> = None;
        let mut pending_restore: Option<String> = None;

        ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for entry in list.iter_mut() {
                    ui.horizontal(|ui| {
                        let mut toggle = entry.checked;
                        let label = format!("{} ({})", entry.path, entry.status_label);
                        let response = ui.checkbox(&mut toggle, label);
                        if response.changed() {
                            entry.checked = toggle;
                            pending_action = Some((staged, entry.path.clone()));
                        }

                        if ui.button("Diff").clicked() {
                            pending_diff = Some((staged, entry.path.clone()));
                        }

                        response.context_menu(|ui| {
                            if ui.button("Restore file...").clicked() {
                                pending_restore = Some(entry.path.clone());
                                ui.close_menu();
                            }
                        });
                    });
                }
            });

        if let Some((is_staged, path)) = pending_action {
            if is_staged {
                self.handle_unstage(repo, &path);
            } else {
                self.handle_stage(repo, &path);
            }
        }

        if let Some(diff) = pending_diff {
            self.selected_diff = Some(diff);
        }

        if let Some(path) = pending_restore {
            self.restore_dialog_open = true;
            self.restore_selection = Some(path);
        }
    }

    fn render_diff(&mut self, ui: &mut Ui) {
        egui::Frame::none()
            .fill(self.theme.palette.surface)
            .stroke(egui::Stroke::new(1.0, self.theme.palette.surface_highlight))
            .inner_margin(egui::Margin::same(8.0))
            .rounding(6.0)
            .show(ui, |ui| {
                ui.with_layout(Layout::left_to_right(Align::TOP), |ui| {
                    ui.heading(
                        RichText::new("Diff preview").color(self.theme.palette.text_primary),
                    );
                    if let Some((staged, path)) = &self.selected_diff {
                        ui.label(
                            RichText::new(format!(
                                "— {} ({})",
                                path,
                                if *staged { "staged" } else { "unstaged" }
                            ))
                            .color(self.theme.palette.text_secondary),
                        );
                    }
                });
                ui.separator();

                if let Some((staged, path)) = &self.selected_diff {
                    let list = if *staged {
                        &self.staged
                    } else {
                        &self.unstaged
                    };
                    if let Some(entry) = list.iter().find(|f| &f.path == path) {
                        ScrollArea::vertical()
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                ui.code(&entry.diff);
                            });
                        return;
                    }
                }

                ui.label(
                    RichText::new("Select a file to view its patch.")
                        .color(self.theme.palette.text_secondary),
                );
            });
    }

    fn render_commit_editor(&mut self, ui: &mut Ui) {
        egui::Frame::none()
            .fill(self.theme.palette.surface)
            .stroke(egui::Stroke::new(1.0, self.theme.palette.surface_highlight))
            .inner_margin(egui::Margin::same(10.0))
            .rounding(6.0)
            .show(ui, |ui| {
                ui.heading(RichText::new("Commit editor").color(self.theme.palette.text_primary));
                ui.add_space(6.0);

                ui.horizontal(|ui| {
                    ui.label(RichText::new("Template").color(self.theme.palette.text_secondary));
                    ComboBox::from_id_source("commit_template")
                        .selected_text(COMMIT_TEMPLATES[self.selected_template].0)
                        .show_ui(ui, |ui| {
                            for (idx, (label, _)) in COMMIT_TEMPLATES.iter().enumerate() {
                                if ui
                                    .selectable_label(self.selected_template == idx, *label)
                                    .clicked()
                                {
                                    self.selected_template = idx;
                                    self.commit_message = self.decorate_commit_message(
                                        COMMIT_TEMPLATES[idx].1.to_string(),
                                    );
                                }
                            }
                        });
                });

                ui.add_space(6.0);
                ui.add(
                    egui::TextEdit::multiline(&mut self.commit_message)
                        .desired_rows(6)
                        .hint_text("Write your commit message..."),
                );

                ui.checkbox(&mut self.include_signoff, "Add Signed-off-by");
                self.apply_signoff();
            });
    }

    fn render_stash_controls(&mut self, ui: &mut Ui, repo: &RepoContext) {
        egui::Frame::none()
            .fill(self.theme.palette.surface)
            .stroke(egui::Stroke::new(1.0, self.theme.palette.surface_highlight))
            .inner_margin(egui::Margin::same(10.0))
            .rounding(6.0)
            .show(ui, |ui| {
                ui.heading(
                    RichText::new("Stash management").color(self.theme.palette.text_primary),
                );
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.stash_message)
                            .hint_text("Describe the stash..."),
                    );
                    ui.checkbox(&mut self.include_untracked_in_stash, "Include untracked");
                    if ui.button("Create stash").clicked() {
                        self.status = None;
                        match create_stash(
                            &repo.path,
                            self.stash_message.trim(),
                            self.include_untracked_in_stash,
                        ) {
                            Ok(_) => {
                                self.status = Some("Stashed working tree".to_string());
                                self.needs_refresh = true;
                            }
                            Err(err) => self.error = Some(format!("Failed to stash: {err}")),
                        }
                    }
                });

                ui.add_space(8.0);
                ui.label(
                    RichText::new("Apply or drop an existing stash.")
                        .color(self.theme.palette.text_secondary),
                );
                ui.add_space(4.0);

                if self.stashes.is_empty() {
                    ui.label(
                        RichText::new("No stashes available.")
                            .color(self.theme.palette.text_secondary),
                    );
                    return;
                }

                for stash in self.stashes.clone() {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(format!("#{} — {}", stash.index, stash.message))
                                .color(self.theme.palette.text_primary),
                        );
                        if ui.button("Apply").clicked() {
                            match apply_stash(&repo.path, stash.index) {
                                Ok(_) => {
                                    self.status = Some(format!("Applied stash #{}", stash.index));
                                    self.needs_refresh = true;
                                }
                                Err(err) => {
                                    self.error = Some(format!(
                                        "Failed to apply stash #{}: {err}",
                                        stash.index
                                    ))
                                }
                            }
                        }
                        if ui.button("Drop").clicked() {
                            match drop_stash(&repo.path, stash.index) {
                                Ok(_) => {
                                    self.status = Some(format!("Dropped stash #{}", stash.index));
                                    self.needs_refresh = true;
                                }
                                Err(err) => {
                                    self.error = Some(format!(
                                        "Failed to drop stash #{}: {err}",
                                        stash.index
                                    ))
                                }
                            }
                        }
                    });
                }
            });
    }

    fn render_restore_dialog(&mut self, ui: &mut Ui, repo: &RepoContext) {
        if !self.restore_dialog_open {
            return;
        }

        let mut open = self.restore_dialog_open;
        let mut request_close = false;
        let mut request_restore: Option<String> = None;
        let current_branch = read_repo_status(&repo.path)
            .ok()
            .and_then(|status| status.branch)
            .unwrap_or_else(|| "HEAD".to_string());
        let candidates = self.restore_candidates();

        Window::new("Restore file")
            .open(&mut open)
            .collapsible(false)
            .show(ui.ctx(), |ui| {
                ui.label(
                    RichText::new(format!(
                        "Restore file from {current_branch} (discard local changes)."
                    ))
                    .color(self.theme.palette.text_secondary),
                );
                ui.add_space(6.0);

                if candidates.is_empty() {
                    ui.label(
                        RichText::new("No modified files to restore.")
                            .color(self.theme.palette.text_secondary),
                    );
                    return;
                }

                ScrollArea::vertical()
                    .max_height(220.0)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for path in candidates {
                            let selected = self.restore_selection.as_deref() == Some(&path);
                            if ui.selectable_label(selected, &path).clicked() {
                                self.restore_selection = Some(path);
                            }
                        }
                    });

                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        request_close = true;
                    }

                    let restore_enabled = self.restore_selection.is_some();
                    if ui
                        .add_enabled(restore_enabled, egui::Button::new("Restore"))
                        .clicked()
                    {
                        request_restore = self.restore_selection.clone();
                    }
                });
            });

        if request_close {
            open = false;
        }

        if let Some(path) = request_restore {
            self.status = None;
            match restore_file_from_branch(&repo.path, &current_branch, &path) {
                Ok(()) => {
                    self.status = Some(format!("Restored {path} from {current_branch}"));
                    self.needs_refresh = true;
                    open = false;
                }
                Err(err) => {
                    self.error = Some(format!("Failed to restore {path}: {err}"));
                }
            }
        }

        if !open {
            self.restore_selection = None;
        }
        self.restore_dialog_open = open;
    }

    fn restore_candidates(&self) -> Vec<String> {
        let mut candidates = BTreeSet::new();
        for entry in self.staged.iter().chain(self.unstaged.iter()) {
            candidates.insert(entry.path.clone());
        }
        candidates.into_iter().collect()
    }

    fn handle_stage(&mut self, repo: &RepoContext, path: &str) {
        self.status = None;
        match stage_path(&repo.path, path) {
            Ok(_) => {
                self.status = Some(format!("Staged {path}"));
                self.needs_refresh = true;
            }
            Err(err) => self.error = Some(format!("Failed to stage {path}: {err}")),
        }
    }

    fn handle_unstage(&mut self, repo: &RepoContext, path: &str) {
        self.status = None;
        match unstage_path(&repo.path, path) {
            Ok(_) => {
                self.status = Some(format!("Unstaged {path}"));
                self.needs_refresh = true;
            }
            Err(err) => self.error = Some(format!("Failed to unstage {path}: {err}")),
        }
    }

    fn apply_signoff(&mut self) {
        if self.include_signoff && !self.commit_message.contains(&self.signoff_line) {
            if !self.commit_message.ends_with('\n') && !self.commit_message.is_empty() {
                self.commit_message.push('\n');
            }
            if !self.commit_message.ends_with('\n') {
                self.commit_message.push('\n');
            }
            self.commit_message.push_str(&self.signoff_line);
        } else if !self.include_signoff
            && let Some(idx) = self.commit_message.find(&self.signoff_line)
        {
            self.commit_message
                .replace_range(idx..idx + self.signoff_line.len(), "");
            self.commit_message = self.commit_message.trim_end().to_string();
        }
    }

    fn decorate_commit_message(&self, message: String) -> String {
        if self.include_signoff {
            let mut msg = message;
            if !msg.ends_with('\n') {
                msg.push('\n');
            }
            msg.push_str(&self.signoff_line);
            msg
        } else {
            message
        }
    }
}

fn format_status_label(status: Status) -> String {
    if status.is_wt_new() || status.is_index_new() {
        "added".to_string()
    } else if status.is_wt_deleted() || status.is_index_deleted() {
        "deleted".to_string()
    } else if status.is_wt_modified() || status.is_index_modified() {
        "modified".to_string()
    } else if status.is_wt_renamed() || status.is_index_renamed() {
        "renamed".to_string()
    } else {
        "changed".to_string()
    }
}

fn read_statuses(repo_path: &str) -> Result<(Vec<FileEntry>, Vec<FileEntry>), git2::Error> {
    let repo = Repository::open(repo_path)?;
    let mut status_opts = StatusOptions::new();
    status_opts
        .show(StatusShow::IndexAndWorkdir)
        .include_untracked(true)
        .recurse_untracked_dirs(true)
        .include_unmodified(false);

    let statuses = repo.statuses(Some(&mut status_opts))?;
    let staged_map = build_diff_map(staged_diff(repo_path)?);
    let unstaged_map = build_diff_map(working_tree_diff(repo_path)?);

    let mut staged = Vec::new();
    let mut unstaged = Vec::new();

    for entry in statuses.iter() {
        let path = entry.path().unwrap_or("(unknown)").to_string();
        let status = entry.status();

        if status.is_index_new()
            || status.is_index_modified()
            || status.is_index_deleted()
            || status.is_index_renamed()
            || status.is_index_typechange()
        {
            staged.push(FileEntry {
                status_label: format_status_label(status),
                diff: lookup_or_refresh_diff(&staged_map, repo_path, &path, true)?,
                path: path.clone(),
                checked: true,
            });
        }

        if status.is_wt_new()
            || status.is_wt_modified()
            || status.is_wt_deleted()
            || status.is_wt_renamed()
            || status.is_wt_typechange()
        {
            unstaged.push(FileEntry {
                status_label: format_status_label(status),
                diff: lookup_or_refresh_diff(&unstaged_map, repo_path, &path, false)?,
                path,
                checked: false,
            });
        }
    }

    Ok((staged, unstaged))
}

fn build_diff_map(diffs: Vec<crate::git::diff::FileDiff>) -> HashMap<String, String> {
    diffs
        .into_iter()
        .map(|diff| (diff.path, diff.patch))
        .collect()
}

fn lookup_or_refresh_diff(
    diffs: &HashMap<String, String>,
    repo_path: &str,
    path: &str,
    staged: bool,
) -> Result<String, git2::Error> {
    if let Some(patch) = diffs.get(path) {
        return Ok(patch.clone());
    }

    let patch = diff_file(repo_path, path, staged)?
        .map(|entry| entry.patch)
        .unwrap_or_else(|| "(no textual diff available)\n".to_string());
    Ok(patch)
}

fn stage_path(repo_path: &str, path: &str) -> Result<(), git2::Error> {
    let repo = Repository::open(repo_path)?;
    let mut index = repo.index()?;
    let path_ref = Path::new(path);
    if path_ref.exists() {
        index.add_path(path_ref)?;
    } else {
        index.remove_path(path_ref)?;
    }
    index.write()
}

fn unstage_path(repo_path: &str, path: &str) -> Result<(), git2::Error> {
    let repo = Repository::open(repo_path)?;
    repo.reset_default(None, [Path::new(path)])
}

fn default_signoff_line() -> String {
    match Signature::now("GitSpace", "gitspace@example.com") {
        Ok(sig) => format!(
            "Signed-off-by: {} <{}>",
            sig.name().unwrap_or("GitSpace"),
            sig.email().unwrap_or("gitspace@example.com")
        ),
        Err(_) => "Signed-off-by: GitSpace <gitspace@example.com>".to_string(),
    }
}
