use std::path::PathBuf;
use std::str::FromStr;
use std::sync::mpsc::{self, Receiver};

use eframe::egui::{self, Align, ComboBox, Layout, RichText, Sense, TextEdit, Ui};
use poll_promise::Promise;
use reqwest::blocking::Client;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, USER_AGENT};
use serde::Deserialize;

use crate::git::clone::{CloneProgress, CloneRequest, clone_repository};
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
    search_promise: Option<Promise<Result<Vec<RemoteRepo>, String>>>,
    search_status: Option<String>,
    clone_promise: Option<Promise<Result<(), String>>>,
    progress_rx: Option<Receiver<CloneEvent>>,
    progress: Option<CloneProgress>,
    clone_status: Option<String>,
    cloning: bool,
    active_destination: Option<PathBuf>,
    last_cloned_repo: Option<PathBuf>,
}

impl ClonePanel {
    pub fn new(theme: Theme) -> Self {
        let destination = std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .display()
            .to_string();

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
        }
    }

    pub fn ui(&mut self, ui: &mut Ui) {
        self.poll_search();
        self.poll_clone_progress();
        self.poll_clone_result();

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

        self.search_section(ui);
        ui.add_space(12.0);
        self.destination_section(ui);
        ui.add_space(12.0);
        self.token_section(ui);
        ui.add_space(12.0);
        self.action_bar(ui);
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
                    provider.label(),
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

    fn search_section(&mut self, ui: &mut Ui) {
        ui.vertical(|ui| {
            ui.heading(RichText::new("Remote repository search").color(self.theme.palette.text_primary));
            ui.label(
                RichText::new("Search GitHub or GitLab without leaving the app. Select a result to fill the clone URL.")
                    .color(self.theme.palette.text_secondary),
            );
            ui.add_space(6.0);

            ui.horizontal(|ui| {
                let query_edit = ui.add_sized(
                    [320.0, 28.0],
                    TextEdit::singleline(&mut self.repo_query).hint_text("Search repositories"),
                );

                if query_edit.changed() {
                    self.search_status = None;
                }

                let search_enabled = self.repo_query.trim().len() >= 2 && !self.cloning;
                let button = ui.add_enabled(search_enabled, egui::Button::new("Search"));
                if button.clicked() {
                    self.start_search();
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
                        if ui.selectable_label(self.selected_repo == Some(idx), label).clicked() {
                            self.selected_repo = Some(idx);
                            self.repo_url = repo.url.clone();
                        }
                    }
                });

            ui.add_space(10.0);
            ui.label(RichText::new("Repository URL").color(self.theme.palette.text_primary));
            ui.add_sized(
                [520.0, 28.0],
                TextEdit::singleline(&mut self.repo_url)
                    .hint_text("https://github.com/owner/repo.git or git@gitlab.com:owner/repo.git"),
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

    fn token_section(&mut self, ui: &mut Ui) {
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
    }

    fn action_bar(&mut self, ui: &mut Ui) {
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let can_clone = !self.repo_url.trim().is_empty()
                && !self.destination.trim().is_empty()
                && !self.cloning;
            if ui
                .add_enabled(can_clone, egui::Button::new("Clone repository"))
                .clicked()
            {
                self.start_clone();
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

    fn start_search(&mut self) {
        let query = self.repo_query.trim().to_string();
        if query.len() < 2 {
            return;
        }
        let provider = self.provider;
        let token = if self.token.trim().is_empty() {
            None
        } else {
            Some(self.token.clone())
        };
        self.search_status = Some("Searching...".to_string());
        self.search_promise = Some(Promise::spawn_thread("search_repos", move || {
            search_repositories(provider, &query, token.as_deref())
        }));
    }

    fn start_clone(&mut self) {
        let url = self.repo_url.trim().to_string();
        let destination = PathBuf::from(self.destination.trim());
        let token = if self.token.trim().is_empty() {
            None
        } else {
            Some(self.token.clone())
        };

        let request = CloneRequest {
            url,
            destination,
            token,
        };

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

    fn poll_search(&mut self) {
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
                        self.search_status = Some(format!("Search failed: {}", err));
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

    fn poll_clone_result(&mut self) {
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
                    }
                    Err(err) => {
                        self.clone_status = Some(format!("Clone failed: {}", err));
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
) -> Result<Vec<RemoteRepo>, String> {
    match provider {
        Provider::GitHub => search_github(query, token),
        Provider::GitLab => search_gitlab(query, token),
    }
}

fn client_with_headers(
    token: Option<&str>,
    token_header: Option<&str>,
    header: Option<(&'static str, &'static str)>,
) -> Result<Client, String> {
    let mut headers = HeaderMap::new();
    headers.insert(
        USER_AGENT,
        HeaderValue::from_str("gitspace-ui/0.1").map_err(|e| e.to_string())?,
    );
    if let Some(token) = token {
        let header_name = token_header.unwrap_or("Authorization");
        let value = if header_name.eq_ignore_ascii_case("authorization") {
            format!("Bearer {}", token)
        } else {
            token.to_string()
        };
        let name = HeaderName::from_str(header_name).map_err(|e| e.to_string())?;
        let auth_value = HeaderValue::from_str(&value).map_err(|e| e.to_string())?;
        headers.insert(name, auth_value);
    }
    if let Some((key, value)) = header {
        headers.insert(
            key,
            HeaderValue::from_str(value).map_err(|e| e.to_string())?,
        );
    }
    Client::builder()
        .default_headers(headers)
        .build()
        .map_err(|e| e.to_string())
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

fn search_github(query: &str, token: Option<&str>) -> Result<Vec<RemoteRepo>, String> {
    let client = client_with_headers(token, None, None)?;
    let url = "https://api.github.com/search/repositories";
    let response: GithubSearchResponse = client
        .get(url)
        .query(&[("q", query), ("per_page", "6")])
        .send()
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .json()
        .map_err(|e| e.to_string())?;

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

#[derive(Debug, Deserialize)]
struct GitlabProject {
    name_with_namespace: String,
    description: Option<String>,
    http_url_to_repo: String,
}

fn search_gitlab(query: &str, token: Option<&str>) -> Result<Vec<RemoteRepo>, String> {
    let client = client_with_headers(
        token,
        Some("PRIVATE-TOKEN"),
        Some(("Accept", "application/json")),
    )?;
    let url = "https://gitlab.com/api/v4/projects";
    let response: Vec<GitlabProject> = client
        .get(url)
        .query(&[("search", query), ("per_page", "6"), ("simple", "true")])
        .send()
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .json()
        .map_err(|e| e.to_string())?;

    Ok(response
        .into_iter()
        .map(|project| RemoteRepo {
            name: project.name_with_namespace,
            description: project.description.unwrap_or_default(),
            url: project.http_url_to_repo,
        })
        .collect())
}
