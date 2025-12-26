use std::process::Command;

use eframe::egui::{self, Align, Layout, Margin, RichText, Ui};

use crate::auth::AuthManager;
use crate::config::NetworkOptions;
use crate::git::{
    remote::{RemoteInfo, fetch_remote, list_remotes, pull_branch, push_branch},
    status::{RepoStatus, read_repo_status},
};
use crate::config::MIN_BRANCH_BOX_HEIGHT;
use crate::ui::{context::RepoContext, theme::Theme};

#[derive(Debug, Clone)]
pub struct RepoOverviewPanel {
    theme: Theme,
    status: Option<RepoStatus>,
    remotes: Vec<RemoteInfo>,
    last_repo: Option<String>,
    error: Option<String>,
    action_status: Option<String>,
    branch_box_height: f32,
    pending_branch_box_height: Option<f32>,
    network: NetworkOptions,
}

impl RepoOverviewPanel {
    pub fn new(theme: Theme, branch_box_height: f32, network: NetworkOptions) -> Self {
        Self {
            theme,
            status: None,
            remotes: Vec::new(),
            last_repo: None,
            error: None,
            action_status: None,
            branch_box_height,
            pending_branch_box_height: None,
            network,
        }
    }

    pub fn set_theme(&mut self, theme: Theme) {
        self.theme = theme;
    }

    pub fn set_branch_box_height(&mut self, height: f32) {
        self.branch_box_height = height.max(MIN_BRANCH_BOX_HEIGHT);
    }

    pub fn set_network_preferences(&mut self, network: NetworkOptions) {
        self.network = network;
    }

    pub fn take_branch_box_height_change(&mut self) -> Option<f32> {
        self.pending_branch_box_height.take()
    }

    pub fn ui(&mut self, ui: &mut Ui, repo: Option<&RepoContext>, auth: &AuthManager) {
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.heading(RichText::new("Repository overview").color(self.theme.palette.text_primary));
            if let Some(status) = &self.action_status {
                ui.add_space(12.0);
                ui.label(RichText::new(status).color(self.theme.palette.text_secondary));
            }
        });

        if let Some(repo) = repo {
            self.refresh(repo);
            if let Some(error) = &self.error {
                ui.colored_label(self.theme.palette.accent, error);
            }

            ui.add_space(6.0);
            self.summary(ui, repo);
            ui.add_space(8.0);
            self.branch_section(ui);
            ui.add_space(8.0);
            self.remotes_section(ui);
            ui.add_space(8.0);
            self.actions(ui, repo, auth);
        } else {
            ui.label(
                RichText::new("Select or clone a repository to see its Git status, remotes, and quick actions.")
                    .color(self.theme.palette.text_secondary),
            );
        }
    }

    fn refresh(&mut self, repo: &RepoContext) {
        if self.last_repo.as_deref() == Some(&repo.path) {
            return;
        }

        self.last_repo = Some(repo.path.clone());
        self.status = None;
        self.remotes.clear();
        self.error = None;
        self.action_status = None;

        match read_repo_status(&repo.path) {
            Ok(status) => self.status = Some(status),
            Err(err) => self.error = Some(format!("Failed to read repository status: {err}")),
        }

        match list_remotes(&repo.path) {
            Ok(remotes) => self.remotes = remotes,
            Err(err) => {
                self.error
                    .get_or_insert_with(|| format!("Failed to read remotes: {err}"));
            }
        }
    }

    fn summary(&self, ui: &mut Ui, repo: &RepoContext) {
        ui.vertical(|ui| {
            ui.label(
                RichText::new(&repo.name)
                    .color(self.theme.palette.text_primary)
                    .strong()
                    .size(self.theme.typography.title),
            );
            ui.label(
                RichText::new(&repo.path)
                    .color(self.theme.palette.text_secondary)
                    .italics(),
            );
        });
    }

    fn branch_section(&mut self, ui: &mut Ui) {
        let status = self.status.clone().unwrap_or_default();
        let branch = status.branch.unwrap_or_else(|| "(detached)".to_string());
        let upstream = status.upstream.unwrap_or_else(|| "No upstream".to_string());
        let ahead = status.ahead.unwrap_or(0);
        let behind = status.behind.unwrap_or(0);

        let branch_height = self.branch_box_height.max(MIN_BRANCH_BOX_HEIGHT);
        let grip_height = 6.0;
        let frame = egui::Frame::none()
            .fill(self.theme.palette.surface)
            .stroke(egui::Stroke::new(1.0, self.theme.palette.surface_highlight))
            .rounding(8.0)
            .inner_margin(Margin {
                left: 10.0,
                right: 10.0,
                top: 4.0,
                bottom: 4.0,
            });

        ui.add_space(4.0);
        ui.heading(RichText::new("Branch").color(self.theme.palette.text_primary));
        ui.add_space(4.0);
        let total_height = branch_height + grip_height;
        let (rect, _) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), total_height),
            egui::Sense::hover(),
        );
        let frame_rect = egui::Rect::from_min_size(
            rect.min,
            egui::vec2(rect.width(), branch_height),
        );
        let grip_rect = egui::Rect::from_min_size(
            egui::pos2(rect.left(), rect.top() + branch_height),
            egui::vec2(rect.width(), grip_height),
        );

        let mut content_ui = ui.child_ui(frame_rect, Layout::top_down(Align::Min));
        frame.show(&mut content_ui, |ui| {
            ui.with_layout(Layout::left_to_right(Align::TOP), |ui| {
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new(branch)
                            .color(self.theme.palette.text_primary)
                            .strong(),
                    );
                    ui.label(
                        RichText::new(format!("Upstream: {upstream}"))
                            .color(self.theme.palette.text_secondary),
                    );
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    self.stat_chip(ui, "Ahead", ahead);
                    self.stat_chip(ui, "Behind", behind);
                });
            });
        });

        let grip_id = ui.make_persistent_id("branch_box_resize_grip");
        let grip_response = ui.interact(grip_rect, grip_id, egui::Sense::click_and_drag());
        if grip_response.hovered() || grip_response.dragged() {
            ui.output_mut(|output| output.cursor_icon = egui::CursorIcon::ResizeVertical);
        }

        if grip_response.dragged() {
            let delta = ui.input(|input| input.pointer.delta().y);
            self.branch_box_height = (self.branch_box_height + delta).max(MIN_BRANCH_BOX_HEIGHT);
        }

        if grip_response.drag_stopped() {
            self.pending_branch_box_height = Some(self.branch_box_height);
        }

        let painter = ui.painter();
        painter.rect_filled(grip_rect, 0.0, self.theme.palette.surface);
        let grip_center = grip_rect.center();
        let grip_line = egui::Stroke::new(1.0, self.theme.palette.surface_highlight);
        painter.line_segment(
            [
                egui::pos2(grip_rect.left() + 12.0, grip_center.y),
                egui::pos2(grip_rect.right() - 12.0, grip_center.y),
            ],
            grip_line,
        );
    }

    fn stat_chip(&self, ui: &mut Ui, label: &str, value: usize) {
        let chip_height = ui.spacing().interact_size.y + 8.0;
        let rect = ui
            .allocate_exact_size(egui::vec2(90.0, chip_height), egui::Sense::hover())
            .0;
        let painter = ui.painter();
        painter.rect_filled(rect, 10.0, self.theme.palette.surface_highlight);
        painter.text(
            rect.left_top() + egui::vec2(10.0, 4.0),
            egui::Align2::LEFT_TOP,
            label,
            egui::FontId::proportional(self.theme.typography.label),
            self.theme.palette.text_secondary,
        );
        painter.text(
            rect.left_bottom() + egui::vec2(10.0, -4.0),
            egui::Align2::LEFT_BOTTOM,
            value.to_string(),
            egui::FontId::proportional(self.theme.typography.title),
            self.theme.palette.text_primary,
        );
    }

    fn remotes_section(&self, ui: &mut Ui) {
        ui.heading(RichText::new("Remotes").color(self.theme.palette.text_primary));
        ui.add_space(4.0);

        if self.remotes.is_empty() {
            ui.label(
                RichText::new("No remotes configured for this repository.")
                    .color(self.theme.palette.text_secondary),
            );
            return;
        }

        for remote in &self.remotes {
            let frame = egui::Frame::none()
                .fill(self.theme.palette.surface)
                .stroke(egui::Stroke::new(1.0, self.theme.palette.surface_highlight))
                .rounding(6.0)
                .inner_margin(Margin::same(10.0));

            frame.show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(&remote.name)
                            .color(self.theme.palette.text_primary)
                            .strong(),
                    );
                    ui.label(RichText::new(&remote.url).color(self.theme.palette.text_secondary));
                });
            });
            ui.add_space(6.0);
        }
    }

    fn actions(&mut self, ui: &mut Ui, repo: &RepoContext, auth: &AuthManager) {
        ui.heading(RichText::new("Quick actions").color(self.theme.palette.text_primary));
        ui.add_space(4.0);

        ui.horizontal_wrapped(|ui| {
            let control_height = ui.spacing().interact_size.y;
            for (label, action) in [
                ("Fetch", ActionKind::Fetch),
                ("Pull", ActionKind::Pull),
                ("Push", ActionKind::Push),
                ("Open terminal", ActionKind::Terminal),
                ("Open file explorer", ActionKind::FileExplorer),
            ] {
                let response = ui.add_sized([150.0, control_height], egui::Button::new(label));
                if response.clicked() {
                    let result = match action {
                        ActionKind::Fetch => self.run_fetch(repo, auth),
                        ActionKind::Pull => self.run_pull(repo, auth),
                        ActionKind::Push => self.run_push(repo, auth),
                        ActionKind::Terminal => self.open_terminal(repo),
                        ActionKind::FileExplorer => self.open_file_explorer(repo),
                    };

                    self.action_status = Some(match &result {
                        Ok(msg) => msg.clone(),
                        Err(err) => format!("{label} failed: {err}"),
                    });
                    if result.is_ok() {
                        self.refresh(repo);
                    }
                }
            }
        });
    }

    fn run_fetch(&self, repo: &RepoContext, auth: &AuthManager) -> Result<String, String> {
        let target = self.remote_target()?;
        let token = self.token_for_remote(auth, &target.remote_name);
        fetch_remote(
            &repo.path,
            &target.remote_name,
            &self.network,
            token,
        )
        .map_err(|err| err.to_string())?;
        Ok(format!("Fetched {}", target.remote_name))
    }

    fn run_pull(&self, repo: &RepoContext, auth: &AuthManager) -> Result<String, String> {
        let target = self.remote_target()?;
        let token = self.token_for_remote(auth, &target.remote_name);
        pull_branch(
            &repo.path,
            &target.remote_name,
            &target.branch,
            &self.network,
            token,
        )
        .map_err(|err| err.to_string())?;
        Ok(format!("Pulled {}/{}", target.remote_name, target.branch))
    }

    fn run_push(&self, repo: &RepoContext, auth: &AuthManager) -> Result<String, String> {
        let target = self.remote_target()?;
        let token = self.token_for_remote(auth, &target.remote_name);
        push_branch(
            &repo.path,
            &target.remote_name,
            &target.branch,
            &self.network,
            token,
        )
        .map_err(|err| err.to_string())?;
        Ok(format!("Pushed {}/{}", target.remote_name, target.branch))
    }

    fn open_terminal(&self, repo: &RepoContext) -> Result<String, String> {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "bash".to_string());
        Command::new(shell)
            .current_dir(&repo.path)
            .spawn()
            .map_err(|err| err.to_string())?;
        Ok("Terminal opened".to_string())
    }

    fn open_file_explorer(&self, repo: &RepoContext) -> Result<String, String> {
        #[cfg(target_os = "windows")]
        let mut command = {
            let mut cmd = Command::new("explorer");
            cmd.arg(&repo.path);
            cmd
        };

        #[cfg(target_os = "macos")]
        let mut command = {
            let mut cmd = Command::new("open");
            cmd.arg(&repo.path);
            cmd
        };

        #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
        let mut command = {
            let mut cmd = Command::new("xdg-open");
            cmd.arg(&repo.path);
            cmd
        };

        command.spawn().map_err(|err| err.to_string())?;
        Ok("File explorer opened".to_string())
    }

    fn remote_target(&self) -> Result<RemoteTarget, String> {
        let status = self
            .status
            .clone()
            .unwrap_or_default();
        let branch = status
            .branch
            .clone()
            .ok_or_else(|| "No active branch for remote action.".to_string())?;
        let (remote_name, remote_branch) = match status
            .upstream
            .as_deref()
            .and_then(parse_upstream)
        {
            Some((remote, upstream_branch)) => (remote, upstream_branch),
            None => (self.default_remote()?, branch.clone()),
        };

        Ok(RemoteTarget {
            remote_name,
            branch: remote_branch,
        })
    }

    fn default_remote(&self) -> Result<String, String> {
        if self.remotes.is_empty() {
            return Err("No remotes configured for this repository.".to_string());
        }

        if self.remotes.iter().any(|remote| remote.name == "origin") {
            return Ok("origin".to_string());
        }

        Ok(self
            .remotes
            .first()
            .map(|remote| remote.name.clone())
            .unwrap_or_else(|| "origin".to_string()))
    }

    fn token_for_remote(&self, auth: &AuthManager, remote_name: &str) -> Option<String> {
        let url = self
            .remotes
            .iter()
            .find(|remote| remote.name == remote_name)
            .map(|remote| remote.url.clone())?;
        auth.resolve_for_url(&url)
    }
}

enum ActionKind {
    Fetch,
    Pull,
    Push,
    Terminal,
    FileExplorer,
}

struct RemoteTarget {
    remote_name: String,
    branch: String,
}

fn parse_upstream(upstream: &str) -> Option<(String, String)> {
    let trimmed = upstream.trim();
    let candidate = trimmed.strip_prefix("refs/remotes/").unwrap_or(trimmed);
    let mut parts = candidate.splitn(2, '/');
    let remote = parts.next()?.to_string();
    let branch = parts.next()?.to_string();
    Some((remote, branch))
}
