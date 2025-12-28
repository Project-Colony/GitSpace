use std::process::Command;

use eframe::egui::{self, Align, Layout, Margin, RichText, Ui};

use crate::auth::AuthManager;
use crate::config::{MIN_BRANCH_BOX_HEIGHT, NetworkOptions};
use crate::git::{
    remote::{PullOutcome, RemoteInfo, fetch_remote, list_remotes, pull_branch, push_branch},
    status::{RepoStatus, read_repo_status},
};
use crate::ui::{animation::motion_settings, context::RepoContext, perf::PerfScope, theme::Theme};

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
    resize_delta_accumulator: f32,
    last_resize_update: Option<f64>,
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
            resize_delta_accumulator: 0.0,
            last_resize_update: None,
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
        let _scope = PerfScope::new("repo_overview::branch_section");
        let status = self.status.clone().unwrap_or_default();
        let branch = status.branch.unwrap_or_else(|| "(detached)".to_string());
        let upstream = status.upstream.unwrap_or_else(|| "No upstream".to_string());
        let ahead = status.ahead.unwrap_or(0);
        let behind = status.behind.unwrap_or(0);

        let motion = motion_settings(ui.ctx());
        let shadow = motion
            .effects()
            .soft_shadow
            .to_egui_shadow(self.theme.palette.text_primary);
        let branch_height = self.branch_box_height.max(MIN_BRANCH_BOX_HEIGHT);
        let grip_height = 6.0;
        let frame = egui::Frame::none()
            .fill(self.theme.palette.surface)
            .stroke(egui::Stroke::new(1.0, self.theme.palette.surface_highlight))
            .rounding(8.0)
            .shadow(shadow)
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
        let frame_rect =
            egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), branch_height));
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
            let (delta, now) = ui.input(|input| (input.pointer.delta().y, input.time));
            self.resize_delta_accumulator += delta;
            let should_apply = self
                .last_resize_update
                .map_or(true, |last| (now - last) >= 0.016);
            if should_apply && self.resize_delta_accumulator.abs() > f32::EPSILON {
                self.branch_box_height = (self.branch_box_height + self.resize_delta_accumulator)
                    .max(MIN_BRANCH_BOX_HEIGHT);
                self.resize_delta_accumulator = 0.0;
                self.last_resize_update = Some(now);
            }
        }

        if grip_response.drag_stopped() {
            if self.resize_delta_accumulator.abs() > f32::EPSILON {
                self.branch_box_height = (self.branch_box_height + self.resize_delta_accumulator)
                    .max(MIN_BRANCH_BOX_HEIGHT);
                self.resize_delta_accumulator = 0.0;
            }
            self.pending_branch_box_height = Some(self.branch_box_height);
            self.last_resize_update = None;
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
        let motion = motion_settings(ui.ctx());
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
            let shadow = motion
                .effects()
                .soft_shadow
                .to_egui_shadow(self.theme.palette.text_primary);
            let frame = egui::Frame::none()
                .fill(self.theme.palette.surface)
                .stroke(egui::Stroke::new(1.0, self.theme.palette.surface_highlight))
                .shadow(shadow)
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
                        ActionKind::Fetch => self.fetch(repo, auth),
                        ActionKind::Pull => self.pull(repo, auth),
                        ActionKind::Push => self.push(repo, auth),
                        ActionKind::Terminal => self.open_terminal(repo),
                        ActionKind::FileExplorer => self.open_file_explorer(repo),
                    };

                    self.action_status = Some(match result {
                        Ok(msg) => msg,
                        Err(err) => format!("{label} failed: {err}"),
                    });
                }
            }
        });
    }

    fn fetch(&self, repo: &RepoContext, auth: &AuthManager) -> Result<String, String> {
        let selection = self.resolve_remote_selection()?;
        let token = self.resolve_remote_token(auth, &selection.remote_name);
        fetch_remote(&repo.path, &selection.remote_name, &self.network, token)
            .map_err(|err| err.to_string())?;
        Ok(format!("Fetched {}", selection.remote_name))
    }

    fn pull(&self, repo: &RepoContext, auth: &AuthManager) -> Result<String, String> {
        let selection = self.resolve_remote_selection()?;
        let branch = selection
            .branch
            .ok_or_else(|| "No branch checked out for pull.".to_string())?;
        let token = self.resolve_remote_token(auth, &selection.remote_name);
        let outcome = pull_branch(
            &repo.path,
            &selection.remote_name,
            &branch,
            &self.network,
            token,
        )
        .map_err(|err| err.to_string())?;
        let message = match outcome {
            PullOutcome::UpToDate => "Already up to date.".to_string(),
            PullOutcome::FastForward => format!("Pulled {} from {}", branch, selection.remote_name),
        };
        Ok(message)
    }

    fn push(&self, repo: &RepoContext, auth: &AuthManager) -> Result<String, String> {
        let selection = self.resolve_remote_selection()?;
        let branch = selection
            .branch
            .ok_or_else(|| "No branch checked out for push.".to_string())?;
        let token = self.resolve_remote_token(auth, &selection.remote_name);
        push_branch(
            &repo.path,
            &selection.remote_name,
            &branch,
            &self.network,
            token,
        )
        .map_err(|err| err.to_string())?;
        Ok(format!("Pushed {} to {}", branch, selection.remote_name))
    }

    fn resolve_remote_selection(&self) -> Result<RemoteSelection, String> {
        let status = self.status.clone().unwrap_or_default();
        let upstream = status
            .upstream
            .as_deref()
            .and_then(|name| split_upstream(name));
        let (remote_name, upstream_branch) = if let Some((remote, branch)) = upstream {
            (remote.to_string(), Some(branch.to_string()))
        } else {
            let remote = self
                .remotes
                .first()
                .map(|remote| remote.name.clone())
                .ok_or_else(|| "No remotes configured for this repository.".to_string())?;
            (remote, None)
        };

        let branch = upstream_branch.or(status.branch);
        Ok(RemoteSelection { remote_name, branch })
    }

    fn resolve_remote_token(&self, auth: &AuthManager, remote_name: &str) -> Option<String> {
        let remote = self
            .remotes
            .iter()
            .find(|remote| remote.name == remote_name)?;
        if remote.url == "(no url)" {
            return None;
        }
        auth.resolve_for_url(&remote.url)
            .or_else(|| auth.resolve_for_host(&remote.url))
    }

    fn open_terminal(&self, repo: &RepoContext) -> Result<String, String> {
        #[cfg(target_os = "windows")]
        {
            Command::new("cmd")
                .args(["/K", "cd", "/d", &repo.path])
                .spawn()
                .map_err(|err| err.to_string())?;
            return Ok("Terminal opened".to_string());
        }

        #[cfg(target_os = "macos")]
        {
            Command::new("open")
                .args(["-a", "Terminal", &repo.path])
                .spawn()
                .map_err(|err| err.to_string())?;
            return Ok("Terminal opened".to_string());
        }

        #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
        {
            let xterm_command = format!("cd '{}' && exec bash", repo.path);
            let candidates: Vec<(&str, Vec<String>)> = vec![
                ("x-terminal-emulator", Vec::new()),
                (
                    "gnome-terminal",
                    vec!["--working-directory".into(), repo.path.clone()],
                ),
                ("konsole", vec!["--workdir".into(), repo.path.clone()]),
                (
                    "xfce4-terminal",
                    vec!["--working-directory".into(), repo.path.clone()],
                ),
                (
                    "xterm",
                    vec!["-e".into(), "bash".into(), "-lc".into(), xterm_command],
                ),
                (
                    "alacritty",
                    vec!["--working-directory".into(), repo.path.clone()],
                ),
                ("kitty", vec!["--directory".into(), repo.path.clone()]),
                (
                    "wezterm",
                    vec!["start".into(), "--cwd".into(), repo.path.clone()],
                ),
            ];

            for (terminal, args) in candidates {
                let mut command = Command::new(terminal);
                command.args(args);
                command.current_dir(&repo.path);
                if command.spawn().is_ok() {
                    return Ok("Terminal opened".to_string());
                }
            }

            Err("No supported terminal emulator found on PATH".to_string())
        }
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
}

enum ActionKind {
    Fetch,
    Pull,
    Push,
    Terminal,
    FileExplorer,
}

#[derive(Debug, Clone)]
struct RemoteSelection {
    remote_name: String,
    branch: Option<String>,
}

fn split_upstream(upstream: &str) -> Option<(&str, &str)> {
    let mut parts = upstream.splitn(2, '/');
    let remote = parts.next()?;
    let branch = parts.next()?;
    Some((remote, branch))
}
