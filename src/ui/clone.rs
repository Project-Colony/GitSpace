use std::path::PathBuf;
use std::str::FromStr;
use std::sync::mpsc::{self, Receiver};

use eframe::egui::{self, Align, ComboBox, Layout, RichText, Sense, TextEdit, Ui};
use poll_promise::Promise;
use reqwest::blocking::Client;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, USER_AGENT};
use serde::Deserialize;

use crate::auth::{AuthManager, extract_host};
use crate::config::NetworkOptions;
use crate::error::{AppError, logs_directory};
use crate::git::clone::{CloneProgress, CloneRequest, clone_repository};
use crate::ui::notifications::{Notification, NotificationAction, NotificationCenter};
use crate::ui::theme::Theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    GitHub,
    GitLab,
}

impl Provider {
    fn label(&self) -> &'static str {
        match self {
            Provider::GitHub => "GitHub",
            Provider::GitLab => "GitLab",
        }
    }

    fn host(&self) -> &'static str {
        match self {
            Provider::GitHub => "github.com",
            Provider::GitLab => "gitlab.com",
        }
    }

    fn icon(&self) -> char {
        match self {
            Provider::GitHub => '\u{f408}',
            Provider::GitLab => '\u{f296}',
        }
    }

    fn icon_label(&self) -> String {
        format!("{} {}", self.icon(), self.label())
    }
}

#[derive(Debug, Clone)]
pub struct RemoteRepo {
    pub name: String,
    pub description: String,
    pub url: String,
}

enum CloneEvent {
    Progress(CloneProgress),
}

pub struct ClonePanel {
    theme: Theme,
    provider: Provider,
    repo_query: String,
    repo_url: String,
    destination: String,
    token: String,
    search_results: Vec<RemoteRepo>,
    selected_repo: Option<usize>,
    search_promise: Option<Promise<Result<Vec<RemoteRepo>, AppError>>>,
    search_status: Option<String>,
    clone_promise: Option<Promise<Result<(), AppError>>>,
    progress_rx: Option<Receiver<CloneEvent>>,
    progress: Option<CloneProgress>,
    clone_status: Option<String>,
    cloning: bool,
    active_destination: Option<PathBuf>,
    last_cloned_repo: Option<PathBuf>,
    token_source: Option<String>,
    last_request: Option<CloneRequest>,
    network: NetworkOptions,
}

impl ClonePanel {
    pub fn new(theme: Theme, destination: String, network: NetworkOptions) -> Self {
        Self {
            theme,
            provider: Provider::GitHub,
            repo_query: String::new(),
            repo_url: String::new(),
            destination,
            token: String::new(),
            search_results: Vec::new(),
            selected_repo: None,
            search_promise: None,
            search_status: None,
            clone_promise: None,
            progress_rx: None,
            progress: None,
            clone_status: None,
            cloning: false,
            active_destination: None,
            last_cloned_repo: None,
            token_source: None,
            last_request: None,
            network,
        }
    }

    pub fn set_theme(&mut self, theme: Theme) {
        self.theme = theme;
    }

    pub fn set_default_destination<S: Into<String>>(&mut self, destination: S) {
        self.destination = destination.into();
    }

    pub fn set_network_preferences(&mut self, network: NetworkOptions) {
        self.network = network;
    }

    pub fn ui(&mut self, ui: &mut Ui, auth: &AuthManager, notifications: &mut NotificationCenter) {
        self.poll_search(notifications);
        self.poll_clone_progress();
        self.poll_clone_result(notifications);

        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.heading(RichText::new("Clone a repository").color(self.theme.palette.text_primary));
            if let Some(status) = &self.clone_status {
                ui.add_space(12.0);
                ui.label(RichText::new(status).color(self.theme.palette.text_secondary));
            }
        });
        ui.label(
            RichText::new(
                "Choose a provider, search remotely, or paste a URL to clone into a local path.",
            )
            .color(self.theme.palette.text_secondary),
        );
        ui.add_space(12.0);

        self.provider_cards(ui);
        ui.add_space(8.0);
        ui.separator();
        ui.add_space(8.0);

        self.search_section(ui, auth);
        ui.add_space(12.0);
        self.destination_section(ui);
        ui.add_space(12.0);
        self.token_section(ui, auth);
        ui.add_space(12.0);
        self.action_bar(ui, auth);
        self.progress_section(ui);
    }

    fn provider_cards(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            for provider in [Provider::GitHub, Provider::GitLab] {
                let is_active = self.provider == provider;
                let (rect, response) =
                    ui.allocate_exact_size(egui::vec2(140.0, 80.0), Sense::click());
                if response.hovered() {
                    ui.output_mut(|o| o.cursor_icon = egui::CursorIcon::PointingHand);
                }
                let fill = if is_active {
                    self.theme.palette.surface_highlight
                } else {
                    self.theme.palette.surface
                };
                let stroke = egui::Stroke::new(1.0, self.theme.palette.accent_weak);
                let painter = ui.painter();
                painter.rect(rect, 8.0, fill, stroke);

                painter.text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    provider.icon_label(),
                    egui::FontId::proportional(self.theme.typography.title),
                    self.theme.palette.text_primary,
                );

                if response.clicked() {
                    self.provider = provider;
                    self.search_results.clear();
                    self.selected_repo = None;
                }
            }
        });
    }

    fn search_section(&mut self, ui: &mut Ui, auth: &AuthManager) {
        ui.vertical(|ui| {
            ui.heading(
                RichText::new("Remote repository search").color(self.theme.palette.text_primary),
            );
            let search_help = format!(
                "Search {} or {} without leaving the app. Select a result to fill the clone URL.",
                format!("{} GitHub", Provider::GitHub.icon()),
                format!("{} GitLab", Provider::GitLab.icon()),
            );
            ui.label(RichText::new(search_help).color(self.theme.palette.text_secondary));
            ui.add_space(6.0);

            ui.horizontal(|ui| {
                let query_edit = ui.add_sized(
                    [320.0, 28.0],
                    TextEdit::singleline(&mut self.repo_query).hint_text("Search repositories"),
                );

                if query_edit.changed() {
                    self.search_status = None;
                }

                let has_query = self.repo_query.trim().len() >= 2;
                let has_token = self.resolve_token_for_provider(auth).is_some();
                let search_enabled = (has_query || has_token) && !self.cloning;
                let button = ui.add_enabled(search_enabled, egui::Button::new("Search"));
                if button.clicked() {
                    self.start_search(auth);
                }

                if let Some(status) = &self.search_status {
                    ui.add_space(8.0);
                    ui.label(RichText::new(status).color(self.theme.palette.text_secondary));
                }
            });

            ui.add_space(8.0);
            ComboBox::from_label("Results")
                .selected_text(
                    self.selected_repo
                        .and_then(|idx| self.search_results.get(idx))
                        .map(|repo| repo.name.clone())
                        .unwrap_or_else(|| "Select a repository".to_string()),
                )
                .show_ui(ui, |ui| {
                    for (idx, repo) in self.search_results.iter().enumerate() {
                        let label = format!("{} — {}", repo.name, repo.description);
                        if ui
                            .selectable_label(self.selected_repo == Some(idx), label)
                            .clicked()
                        {
                            self.selected_repo = Some(idx);
                            self.repo_url = repo.url.clone();
                        }
                    }
                });

            ui.add_space(10.0);
            ui.label(RichText::new("Repository URL").color(self.theme.palette.text_primary));
            ui.add_sized(
                [520.0, 28.0],
                TextEdit::singleline(&mut self.repo_url).hint_text(
                    "https://github.com/owner/repo.git or git@gitlab.com:owner/repo.git",
                ),
            );
        });
    }

    fn destination_section(&mut self, ui: &mut Ui) {
        ui.heading(RichText::new("Local path").color(self.theme.palette.text_primary));
        ui.horizontal(|ui| {
            ui.add_sized(
                [400.0, 28.0],
                TextEdit::singleline(&mut self.destination).hint_text("Where should we clone to?"),
            );
            if ui.button("Choose").clicked() {
                if let Some(folder) = rfd::FileDialog::new()
                    .set_directory(&self.destination)
                    .pick_folder()
                {
                    self.destination = folder.display().to_string();
                }
            }
        });
    }

    fn token_section(&mut self, ui: &mut Ui, auth: &AuthManager) {
        ui.heading(
            RichText::new("Authentication (optional)").color(self.theme.palette.text_primary),
        );
        ui.horizontal(|ui| {
            ui.add_sized(
                [400.0, 28.0],
                TextEdit::singleline(&mut self.token)
                    .password(true)
                    .hint_text("Personal access token"),
            );
            ui.label(
                RichText::new("Tokens are sent only to the provider you choose.")
                    .color(self.theme.palette.text_secondary),
            );
        });

        if self.token.trim().is_empty() {
            let host_hint = extract_host(self.repo_url.trim())
                .or_else(|| Some(self.provider_host().to_string()));
            if let Some(host) = host_hint {
                if auth.resolve_for_host(&host).is_some() {
                    ui.colored_label(
                        self.theme.palette.text_secondary,
                        format!("Saved token detected for {}.", host),
                    );
                }
            }
        }

        if let Some(source) = &self.token_source {
            ui.colored_label(self.theme.palette.text_secondary, source);
        }
    }

    fn action_bar(&mut self, ui: &mut Ui, auth: &AuthManager) {
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let can_clone = !self.repo_url.trim().is_empty()
                && !self.destination.trim().is_empty()
                && !self.cloning;
            if ui
                .add_enabled(can_clone, egui::Button::new("Clone repository"))
                .clicked()
            {
                self.start_clone(auth);
            }
        });
    }

    fn progress_section(&mut self, ui: &mut Ui) {
        if let Some(progress) = &self.progress {
            let ratio = if progress.total_objects == 0 {
                0.0
            } else {
                progress.received_objects as f32 / progress.total_objects as f32
            };
            ui.add_space(10.0);
            ui.label(RichText::new("Clone progress").color(self.theme.palette.text_primary));
            ui.add(egui::ProgressBar::new(ratio).text(format!(
                "Objects {}/{} ({:.1} KB)",
                progress.received_objects,
                progress.total_objects,
                progress.received_bytes as f32 / 1024.0
            )));
            ui.label(
                RichText::new(format!(
                    "Indexed {} of {} deltas",
                    progress.indexed_deltas, progress.total_deltas
                ))
                .color(self.theme.palette.text_secondary),
            );
        }
    }

    fn start_search(&mut self, auth: &AuthManager) {
        let query = self.repo_query.trim().to_string();
        let provider = self.provider;
        let token = self.resolve_token_for_provider(auth);
        let network = self.network.clone();
        let has_query = query.len() >= 2;

        if !has_query && token.is_none() {
            self.search_status = Some(
                "Enter at least 2 characters or provide a saved token to list your repositories."
                    .to_string(),
            );
            return;
        }

        self.search_status = Some("Searching...".to_string());
        self.search_promise = Some(Promise::spawn_thread("search_repos", move || {
            if has_query {
                search_repositories(provider, &query, token.as_deref(), network)
            } else {
                list_repositories(provider, token.as_deref(), network)
            }
        }));
    }

    fn resolve_token_for_provider(&self, auth: &AuthManager) -> Option<String> {
        if self.token.trim().is_empty() {
            auth.resolve_for_host(self.provider_host())
        } else {
            Some(self.token.clone())
        }
    }

    fn start_clone(&mut self, auth: &AuthManager) {
        let url = self.repo_url.trim().to_string();
        let destination = PathBuf::from(self.destination.trim());
        let mut token = if self.token.trim().is_empty() {
            None
        } else {
            Some(self.token.clone())
        };

        self.token_source = None;
        if token.is_none() {
            if let Some(saved) = auth.resolve_for_url(&url) {
                let host = extract_host(&url).unwrap_or_else(|| "remote".to_string());
                self.token_source = Some(format!("Using stored token for {}", host));
                token = Some(saved);
            }
        }

        let request = CloneRequest {
            url,
            destination,
            token,
            network: self.network.clone(),
        };
        self.begin_clone(request);
    }

    pub fn retry_last_clone(&mut self) {
        if self.cloning {
            return;
        }
        if let Some(request) = self.last_request.clone() {
            self.begin_clone(request);
        }
    }

    fn begin_clone(&mut self, request: CloneRequest) {
        self.last_request = Some(request.clone());
        self.active_destination = Some(request.destination.clone());

        let (tx, rx) = mpsc::channel();
        self.progress_rx = Some(rx);
        self.progress = None;
        self.clone_status = Some("Starting clone...".to_string());
        self.cloning = true;

        self.clone_promise = Some(Promise::spawn_thread("clone_repo", move || {
            let sender = tx.clone();
            let result = clone_repository(request, move |progress| {
                let _ = sender.send(CloneEvent::Progress(progress));
            });
            result
        }));
    }

    fn provider_host(&self) -> &str {
        self.provider.host()
    }

    fn poll_search(&mut self, notifications: &mut NotificationCenter) {
        if let Some(promise) = &self.search_promise {
            if let Some(result) = promise.ready() {
                let result = result.clone();
                self.search_promise = None;
                match result {
                    Ok(results) => {
                        self.search_results = results.clone();
                        self.search_status =
                            Some(format!("{} result(s)", self.search_results.len()));
                        if let Some(repo) = self.search_results.first() {
                            self.repo_url = repo.url.clone();
                            self.selected_repo = Some(0);
                        }
                    }
                    Err(err) => {
                        let log_path = logs_directory();
                        self.search_status = Some(err.user_message());
                        let mut notification =
                            Notification::error("Search failed", err.user_message())
                                .with_log_path(log_path.clone())
                                .with_action(NotificationAction::CopyLogPath(log_path));
                        notification.detail = Some(err.detail().to_string());
                        notifications.push(notification);
                    }
                }
            }
        }
    }

    fn poll_clone_progress(&mut self) {
        if let Some(rx) = &self.progress_rx {
            for event in rx.try_iter() {
                match event {
                    CloneEvent::Progress(progress) => {
                        self.progress = Some(progress);
                    }
                }
            }
        }
    }

    fn poll_clone_result(&mut self, notifications: &mut NotificationCenter) {
        if let Some(promise) = &self.clone_promise {
            if let Some(result) = promise.ready() {
                let result = result.clone();
                self.clone_promise = None;
                self.progress_rx = None;
                self.cloning = false;
                match result {
                    Ok(()) => {
                        self.clone_status = Some("Clone completed successfully".to_string());
                        self.last_cloned_repo = self.active_destination.take();
                        if let Some(repo) = &self.last_cloned_repo {
                            notifications.push(
                                Notification::success(
                                    "Repository cloned",
                                    format!("Saved to {}", repo.display()),
                                )
                                .with_log_path(logs_directory()),
                            );
                        }
                    }
                    Err(err) => {
                        let log_path = logs_directory();
                        self.clone_status = Some(err.user_message());
                        let mut notification =
                            Notification::error("Clone failed", err.user_message())
                                .with_action(NotificationAction::RetryClone)
                                .with_action(NotificationAction::CopyLogPath(log_path.clone()))
                                .with_log_path(log_path);
                        notification.detail = Some(err.detail().to_string());
                        notifications.push(notification);
                        self.active_destination = None;
                    }
                }
            }
        }
    }

    pub fn take_last_cloned_repo(&mut self) -> Option<PathBuf> {
        self.last_cloned_repo.take()
    }
}

fn search_repositories(
    provider: Provider,
    query: &str,
    token: Option<&str>,
    network: NetworkOptions,
) -> Result<Vec<RemoteRepo>, AppError> {
    match provider {
        Provider::GitHub => search_github(query, token, &network),
        Provider::GitLab => search_gitlab(query, token, &network),
    }
}

fn list_repositories(
    provider: Provider,
    token: Option<&str>,
    network: NetworkOptions,
) -> Result<Vec<RemoteRepo>, AppError> {
    match provider {
        Provider::GitHub => list_github_repositories(token, &network),
        Provider::GitLab => list_gitlab_repositories(token, &network),
    }
}

fn client_with_headers(
    token: Option<&str>,
    token_header: Option<&str>,
    header: Option<(&'static str, &'static str)>,
    network: &NetworkOptions,
) -> Result<Client, AppError> {
    let mut headers = HeaderMap::new();
    headers.insert(
        USER_AGENT,
        HeaderValue::from_str("gitspace-ui/0.1")
            .map_err(|err| AppError::Validation(err.to_string()))?,
    );
    if let Some(token) = token {
        let header_name = token_header.unwrap_or("Authorization");
        let value = if header_name.eq_ignore_ascii_case("authorization") {
            format!("Bearer {}", token)
        } else {
            token.to_string()
        };
        let name = HeaderName::from_str(header_name)
            .map_err(|err| AppError::Validation(err.to_string()))?;
        let auth_value =
            HeaderValue::from_str(&value).map_err(|err| AppError::Validation(err.to_string()))?;
        headers.insert(name, auth_value);
    }
    if let Some((key, value)) = header {
        headers.insert(
            key,
            HeaderValue::from_str(value).map_err(|err| AppError::Validation(err.to_string()))?,
        );
    }
    let mut builder =
        Client::builder()
            .default_headers(headers)
            .timeout(std::time::Duration::from_secs(
                network.network_timeout_secs.max(1),
            ));

    if !network.http_proxy.is_empty() {
        builder = builder.proxy(
            reqwest::Proxy::http(&network.http_proxy)
                .map_err(|err| AppError::Validation(err.to_string()))?,
        );
    }

    if !network.https_proxy.is_empty() {
        builder = builder.proxy(
            reqwest::Proxy::https(&network.https_proxy)
                .map_err(|err| AppError::Validation(err.to_string()))?,
        );
    }

    builder.build().map_err(AppError::from)
}

fn enforce_https_policy(url: &str, network: &NetworkOptions) -> Result<(), AppError> {
    if url.starts_with("https://") && !network.use_https {
        return Err(AppError::Validation(
            "HTTPS endpoints are disabled in your network settings.".to_string(),
        ));
    }

    if url.starts_with("http://") && network.use_https {
        return Err(AppError::Validation(
            "HTTP requests are blocked. Enable HTTP in network settings or use HTTPS.".to_string(),
        ));
    }

    Ok(())
}

#[derive(Debug, Deserialize)]
struct GithubRepoItem {
    full_name: String,
    description: Option<String>,
    html_url: String,
}

#[derive(Debug, Deserialize)]
struct GithubSearchResponse {
    items: Vec<GithubRepoItem>,
}

fn search_github(
    query: &str,
    token: Option<&str>,
    network: &NetworkOptions,
) -> Result<Vec<RemoteRepo>, AppError> {
    let url = "https://api.github.com/search/repositories";
    enforce_https_policy(url, network)?;
    let client = client_with_headers(token, None, None, network)?;
    let response: GithubSearchResponse = client
        .get(url)
        .query(&[("q", query), ("per_page", "6")])
        .send()
        .map_err(AppError::from)?
        .error_for_status()
        .map_err(AppError::from)?
        .json()
        .map_err(AppError::from)?;

    Ok(response
        .items
        .into_iter()
        .map(|item| RemoteRepo {
            name: item.full_name,
            description: item.description.unwrap_or_default(),
            url: item.html_url,
        })
        .collect())
}

fn list_github_repositories(
    token: Option<&str>,
    network: &NetworkOptions,
) -> Result<Vec<RemoteRepo>, AppError> {
    let token = token.ok_or_else(|| {
        AppError::Validation(
            "A GitHub token is required to list your repositories. Add one in Auth or the Clone tab.".to_string(),
        )
    })?;

    let url = "https://api.github.com/user/repos";
    enforce_https_policy(url, network)?;
    let client = client_with_headers(Some(token), None, None, network)?;
    let response: Vec<GithubRepoItem> = client
        .get(url)
        .query(&[("per_page", "50"), ("type", "all"), ("sort", "updated")])
        .send()
        .map_err(AppError::from)?
        .error_for_status()
        .map_err(AppError::from)?
        .json()
        .map_err(AppError::from)?;

    Ok(response
        .into_iter()
        .map(|item| RemoteRepo {
            name: item.full_name,
            description: item.description.unwrap_or_default(),
            url: item.html_url,
        })
        .collect())
}

#[derive(Debug, Deserialize)]
struct GitlabProject {
    name_with_namespace: String,
    description: Option<String>,
    http_url_to_repo: String,
}

fn search_gitlab(
    query: &str,
    token: Option<&str>,
    network: &NetworkOptions,
) -> Result<Vec<RemoteRepo>, AppError> {
    let url = "https://gitlab.com/api/v4/projects";
    enforce_https_policy(url, network)?;
    let client = client_with_headers(
        token,
        Some("PRIVATE-TOKEN"),
        Some(("Accept", "application/json")),
        network,
    )?;
    let response: Vec<GitlabProject> = client
        .get(url)
        .query(&[("search", query), ("per_page", "6"), ("simple", "true")])
        .send()
        .map_err(AppError::from)?
        .error_for_status()
        .map_err(AppError::from)?
        .json()
        .map_err(AppError::from)?;

    Ok(response
        .into_iter()
        .map(|project| RemoteRepo {
            name: project.name_with_namespace,
            description: project.description.unwrap_or_default(),
            url: project.http_url_to_repo,
        })
        .collect())
}

fn list_gitlab_repositories(
    token: Option<&str>,
    network: &NetworkOptions,
) -> Result<Vec<RemoteRepo>, AppError> {
    let token = token.ok_or_else(|| {
        AppError::Validation(
            "A GitLab token is required to list your repositories. Add one in Auth or the Clone tab.".to_string(),
        )
    })?;

    let url = "https://gitlab.com/api/v4/projects";
    enforce_https_policy(url, network)?;
    let client = client_with_headers(
        Some(token),
        Some("PRIVATE-TOKEN"),
        Some(("Accept", "application/json")),
        network,
    )?;
    let response: Vec<GitlabProject> = client
        .get(url)
        .query(&[
            ("per_page", "50"),
            ("simple", "true"),
            ("membership", "true"),
        ])
        .send()
        .map_err(AppError::from)?
        .error_for_status()
        .map_err(AppError::from)?
        .json()
        .map_err(AppError::from)?;

    Ok(response
        .into_iter()
        .map(|project| RemoteRepo {
            name: project.name_with_namespace,
            description: project.description.unwrap_or_default(),
            url: project.http_url_to_repo,
        })
        .collect())
}
